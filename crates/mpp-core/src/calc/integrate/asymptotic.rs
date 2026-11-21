//! Oscillating integrals far from the origin, such as `∫_{10^17}^∞ sin x / x² dx`.
//!
//! A double near `10^17` is 16 apart from the next one, so the integrand cannot even be
//! sampled within one period there: any quadrature returns rounding noise. Closer in it
//! can be sampled, but the value (about `1/c²`) is so much smaller than `∫|f|` (about
//! `1/c`) that rounding still eats most of its digits. Such a range is worked out from
//! the asymptotic expansion instead, by integrating by parts again and again:
//!
//! `∫_c^∞ e^{iωx} x^{−p} dx = (i/ω) e^{iωc} c^{−p} Σ_k (−i)^k (p)_k (ωc)^{−k}`,
//!
//! where `(p)_k` is the rising factorial. Cutting the series after `K` terms leaves at
//! most `2 (p)_K (ωc)^{−K}` times the leading factor, and `cos(ωc + φ)`, `sin(ωc + φ)`
//! come from the exact `c` at high precision. A range on the negative side is reflected
//! (`x → −x`), and a finite range far out is the difference of two tails. Other
//! integrands that oscillate are refused once a finite end of the range is too far out
//! to sample.

use std::collections::HashMap;

use crate::eval::EmptyEnv;
use crate::expr::{Expr, ExprNode};
use crate::functions::{func, Func};
use crate::hp::{approx, Approx, Decimal};
use crate::note::maths;
use crate::number::format_f64;
use crate::simplify::{add, mul, simplify};

use super::engine::linear_coeffs;

/// From this distance on an oscillation is not sampled in floating point (the spacing
/// of doubles there is about `1.5·10^−8`).
pub(super) const FAR: f64 = 1e8;

/// From this many radians out (`|ω|·c`) the expansion is used whenever it applies: it is
/// then accurate to rounding, while sampling loses digits to cancellation.
const NEAR_ENOUGH: f64 = 100.0;

/// Most terms of the expansion summed.
const MAX_TERMS: usize = 40;

/// What [`far_range`] found.
pub(super) enum Far {
    /// The value, its error bound and a note for the steps.
    Value(f64, f64, String),
    /// The integrand oscillates too fast to sample and is not of a form worked out here.
    Refused(String),
}

/// `∫_lo^hi e` when the integrand oscillates and the range lies far out on one side of
/// the origin: the asymptotic expansion when the integrand is `k·sin(ωx + φ)·x^(−p)`
/// (or with `cos`), a refusal when it is not and a finite end of the range is too far
/// out to sample, and `None` otherwise (the usual numerical methods apply).
pub(super) fn far_range(
    e: &Expr,
    var: &str,
    (lo_expr, lo): (&Expr, f64),
    (hi_expr, hi): (&Expr, f64),
) -> Option<Far> {
    if !oscillates(e, var) {
        return None;
    }
    let far_end = [lo, hi].iter().any(|v| v.is_finite() && v.abs() >= FAR);
    let refuse = || {
        Far::Refused(format!(
            "{} (the integrand oscillates far faster than it can be sampled that far out)",
            maths(&super::definite_holder(e, var, lo_expr, hi_expr))
        ))
    };
    let expanded = if lo >= 0.0 {
        expansion(e, var, (lo_expr, lo), (hi_expr, hi))
    } else if hi <= 0.0 {
        // ∫_lo^hi f(x) dx = ∫_{−hi}^{−lo} f(−x) dx.
        let x = Expr::sym(var);
        let mirrored = simplify(&e.subs(var, &mul(vec![Expr::int(-1), x])));
        let neg = |b: &Expr| simplify(&mul(vec![Expr::int(-1), b.clone()]));
        expansion(&mirrored, var, (&neg(hi_expr), -hi), (&neg(lo_expr), -lo)).map(|r| {
            r.map(|(v, err, note)| {
                (
                    v,
                    err,
                    format!("reflect {var} → −{var} so the range is on the positive side; {note}"),
                )
            })
        })
    } else {
        None
    };
    match expanded {
        Some(Ok((v, err, note))) => Some(Far::Value(v, err, note)),
        Some(Err(why)) => Some(Far::Refused(format!(
            "{} ({why})",
            maths(&super::definite_holder(e, var, lo_expr, hi_expr))
        ))),
        None if far_end => Some(refuse()),
        None => None,
    }
}

/// `∫_lo^hi e` for `0 ≤ lo < hi` from the expansion, when `e` has the form it covers and
/// `lo` is far enough out; `Some(Err(reason))` when it applies but the value cannot be
/// represented.
#[allow(clippy::type_complexity)]
fn expansion(
    e: &Expr,
    var: &str,
    (lo_expr, lo): (&Expr, f64),
    (hi_expr, hi): (&Expr, f64),
) -> Option<Result<(f64, f64, String), String>> {
    let form = trig_power(e, var)?;
    if form.p <= 0.0 || !(lo.is_finite() && form.omega.abs() * lo >= NEAR_ENOUGH) {
        // A tail that does not converge is for the divergence tests.
        return None;
    }
    let (v_lo, e_lo) = match tail(&form, lo_expr, lo) {
        Ok(t) => t?,
        Err(why) => return Some(Err(why)),
    };
    let (v_hi, e_hi) = if hi.is_finite() {
        match tail(&form, hi_expr, hi) {
            Ok(t) => t?,
            Err(why) => return Some(Err(why)),
        }
    } else {
        (0.0, 0.0)
    };
    let value = v_lo - v_hi;
    let err = e_lo + e_hi + 2e-16 * (v_lo.abs() + v_hi.abs());
    let trig = if form.sine { "sin" } else { "cos" };
    let how = if hi.is_finite() {
        format!("the integral is the difference of the tails from {lo_expr} and from {hi_expr}, each from")
    } else {
        "the tail comes from".into()
    };
    let note = format!(
        "the range lies so far out that {trig} oscillates many times while the rest of the integrand barely changes, and sampling it would lose the value to rounding; {how} its asymptotic expansion, integrating by parts repeatedly: ∫_c^∞ e^(iω{var}) {var}^(−p) d{var} = (i/ω) e^(iωc) c^(−p) Σ (−i)^k (p)_k (ωc)^(−k), with p = {}, ω = {} and cos, sin at the exact c; ≈ {} (error ≤ {})",
        format_f64(form.p, 6),
        format_f64(form.omega, 6),
        format_f64(value, 10),
        format_f64(err.max(f64::MIN_POSITIVE), 2)
    );
    Some(Ok((value, err, note)))
}

/// `k · sin(ωx + φ) · x^(−p)` (or with `cos`).
struct TrigPower {
    k: f64,
    sine: bool,
    p: f64,
    omega: f64,
    /// `ω` and `φ` exactly.
    arg: (Expr, Expr),
}

/// `∫_c^∞` of the form, with its error bound: `Ok(None)` when the trigonometric values
/// cannot be found, `Err` when the value is below the smallest double.
fn tail(form: &TrigPower, c_expr: &Expr, c: f64) -> Result<Option<(f64, f64)>, String> {
    let w = form.omega.abs();
    // θ(c) = ωc + φ, exactly.
    let theta = simplify(&add(vec![
        mul(vec![form.arg.0.clone(), c_expr.clone()]),
        form.arg.1.clone(),
    ]));
    let (Some(s), Some(co)) = (
        decimal_of(&func(Func::Sin, vec![theta.clone()])),
        decimal_of(&func(Func::Cos, vec![theta])),
    ) else {
        return Ok(None);
    };
    // For ω < 0, e^{iθ(x)} is the conjugate of e^{i(|ω|x − φ)}: sin θ changes sign, and so
    // does the imaginary part of the result.
    let flip = form.omega.signum();
    let s = flip * s;
    // Σ (−i)^k (p)_k (|ω|c)^{−k}, as (re, im).
    let z = 1.0 / (w * c);
    let (mut sr, mut si) = (0.0, 0.0);
    let mut term = 1.0;
    let mut omitted = f64::INFINITY;
    for kk in 0..MAX_TERMS {
        match kk % 4 {
            0 => sr += term,
            1 => si -= term,
            2 => sr -= term,
            _ => si += term,
        }
        let next = term * (form.p + kk as f64) * z;
        omitted = next;
        if next >= term || next <= 1e-20 * (sr.abs() + si.abs()) {
            // Stop before the expansion starts to grow, or once the rest is negligible.
            break;
        }
        term = next;
    }
    let scale = form.k.abs() * c.powf(-form.p) / w;
    if !scale.is_normal() {
        return Err("the value is too small for a double: about c^(−p) with c far out".to_string());
    }
    // (i/|ω|) e^{iθ} S = (−sin θ + i cos θ)(sr + i si)/|ω|.
    let re = -s * sr - co * si;
    let im = (-s * si + co * sr) * flip;
    let value = form.k.signum() * scale * if form.sine { im } else { re };
    // The cut series, then rounding of sin θ, cos θ and the terms.
    let size = (s * sr).abs() + (co * si).abs() + (s * si).abs() + (co * sr).abs();
    let err = scale * (2.0 * omitted + 4e-16 * size);
    Ok(Some((value, err)))
}

/// True when the integrand has a sine or cosine (or another trigonometric function)
/// whose argument grows without bound as the variable does: `sin x`, `cos(x²)`, but
/// not `sin(1/x)` or `sin(ln x)`.
pub(super) fn oscillates(e: &Expr, var: &str) -> bool {
    let here = match e.node() {
        ExprNode::Func(
            Func::Sin | Func::Cos | Func::Tan | Func::Sec | Func::Csc | Func::Cot,
            args,
        ) => args
            .iter()
            .any(|a| a.contains_sym(var) && degree(a, var).is_none_or(|d| d > 0.0)),
        _ => false,
    };
    here || e.children().iter().any(|c| oscillates(c, var))
}

/// The leading power of `e` as the variable grows (`x³ + x → 3`, `1/x → −1`,
/// `ln x → 0`, `eˣ → ∞`), or `None` when it is not worked out here.
fn degree(e: &Expr, var: &str) -> Option<f64> {
    if !e.contains_sym(var) {
        return Some(0.0);
    }
    match e.node() {
        ExprNode::Sym(_) => Some(1.0),
        ExprNode::Add(ts) => ts
            .iter()
            .map(|t| degree(t, var))
            .try_fold(f64::NEG_INFINITY, |m, d| Some(m.max(d?))),
        ExprNode::Mul(fs) => fs.iter().map(|f| degree(f, var)).sum(),
        ExprNode::Pow(b, x) if !x.contains_sym(var) => {
            let n = x.eval_f64(&EmptyEnv).ok()?;
            Some(degree(b, var)? * n)
        }
        ExprNode::Pow(b, x) if !b.contains_sym(var) => {
            let base = b.eval_f64(&EmptyEnv).ok()?;
            let d = degree(x, var)?;
            Some(if d > 0.0 && base > 1.0 {
                f64::INFINITY
            } else {
                0.0
            })
        }
        ExprNode::Func(f, args) => {
            let inner = args
                .iter()
                .map(|a| degree(a, var))
                .try_fold(f64::NEG_INFINITY, |m, d| Some(m.max(d?)))?;
            match f {
                Func::Abs => Some(inner),
                // Bounded, or growing slower than any power.
                Func::Sin
                | Func::Cos
                | Func::Atan
                | Func::Tanh
                | Func::Erf
                | Func::Si
                | Func::Sign
                | Func::Ln
                | Func::Log => Some(0.0),
                _ => None,
            }
        }
        _ => None,
    }
}

/// `k · sin(ωx + φ) · x^(−p)` or with `cos`, for numbers `k`, `ω ≠ 0`, `φ`, `p`.
fn trig_power(e: &Expr, var: &str) -> Option<TrigPower> {
    let factors: Vec<Expr> = match e.node() {
        ExprNode::Mul(fs) => fs.to_vec(),
        _ => vec![e.clone()],
    };
    let is_var = |x: &Expr| x.as_sym() == Some(var);
    let mut k = 1.0;
    let mut p = 0.0;
    let mut trig: Option<(bool, Expr, Expr, f64)> = None;
    for f in &factors {
        if !f.contains_sym(var) {
            k *= f.eval_f64(&EmptyEnv).ok()?;
            continue;
        }
        match f.node() {
            ExprNode::Func(which @ (Func::Sin | Func::Cos), args)
                if trig.is_none() && args.len() == 1 =>
            {
                let (a, b) = linear_coeffs(&args[0], var)?;
                let w = a.eval_f64(&EmptyEnv).ok()?;
                b.eval_f64(&EmptyEnv).ok()?;
                if w == 0.0 || !w.is_finite() {
                    return None;
                }
                trig = Some((*which == Func::Sin, a, b, w));
            }
            ExprNode::Sym(_) if is_var(f) => p -= 1.0,
            ExprNode::Pow(b, n) if is_var(b) && !n.contains_sym(var) => {
                p -= n.eval_f64(&EmptyEnv).ok()?;
            }
            _ => return None,
        }
    }
    let (sine, a, b, omega) = trig?;
    (k.is_finite() && k != 0.0 && p.is_finite()).then_some(TrigPower {
        k,
        sine,
        p,
        omega,
        arg: (a, b),
    })
}

/// The size of the phase of the fastest oscillation at `x`, `|θ(x)|·max(1, deg θ)`
/// (about `|x θ′(x)|`): rounding `x` to a double moves the phase by this times the unit
/// roundoff. 0 when nothing oscillates.
pub(super) fn phase_at(e: &Expr, var: &str, x: f64) -> f64 {
    let here = match e.node() {
        ExprNode::Func(
            Func::Sin | Func::Cos | Func::Tan | Func::Sec | Func::Csc | Func::Cot,
            args,
        ) => args
            .iter()
            .filter(|a| a.contains_sym(var))
            .map(|a| {
                let d = degree(a, var).unwrap_or(1.0);
                let env: std::collections::BTreeMap<String, f64> =
                    [(var.to_string(), x)].into_iter().collect();
                // Where the argument cannot be evaluated nothing is sampled either.
                let v = a.eval_f64(&env).map(f64::abs).unwrap_or(0.0);
                if d > 0.0 {
                    v * d.max(1.0)
                } else {
                    0.0
                }
            })
            .fold(0.0, f64::max),
        _ => 0.0,
    };
    e.children()
        .iter()
        .map(|c| phase_at(c, var, x))
        .fold(here, f64::max)
}

/// A closed real expression to about 20 digits, as a double.
fn decimal_of(e: &Expr) -> Option<f64> {
    match approx(e, &HashMap::new(), 20)? {
        Approx::Value { value, .. } => Some(to_f64(&value)),
        Approx::Zero { .. } => Some(0.0),
    }
}

fn to_f64(d: &Decimal) -> f64 {
    let (head, rest) = d.digits.split_at(1);
    let text = format!(
        "{}{head}.{}e{}",
        if d.neg { "-" } else { "" },
        if rest.is_empty() { "0" } else { rest },
        d.k
    );
    text.parse().unwrap_or(f64::NAN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simplify::{mul, pow};

    fn value(e: &Expr, c: &Expr) -> (f64, f64) {
        let cv = c.eval_f64(&EmptyEnv).unwrap();
        match far_range(e, "x", (c, cv), (&Expr::infinity(), f64::INFINITY)) {
            Some(Far::Value(v, err, _)) => (v, err),
            _ => panic!("no value for {e}"),
        }
    }

    #[test]
    fn far_tails_follow_the_asymptotic_expansion() {
        let x = Expr::sym("x");
        let sin_x2 = mul(vec![
            func(Func::Sin, vec![x.clone()]),
            pow(x.clone(), Expr::int(-2)),
        ]);
        // c = 10^17: cos c / c² + 2 sin c / c³ = −8.8555732829763069434…·10^−35.
        let c = pow(Expr::int(10), Expr::int(17));
        let (v, err) = value(&sin_x2, &c);
        let want = -8.855_573_282_976_307e-35;
        assert!((v - want).abs() <= 1e-12 * want.abs(), "{v} vs {want}");
        assert!(err < 1e-45, "{err}");
        // c = 10^8 against the leading terms worked out by hand.
        let c = Expr::int(100_000_000);
        let (v, _) = value(&sin_x2, &c);
        let (s, co) = (1e8f64.sin(), 1e8f64.cos());
        let want = co / 1e16 + 2.0 * s / 1e24 - 6.0 * co / 1e32;
        assert!((v - want).abs() <= 1e-9 * want.abs(), "{v} vs {want}");
    }

    #[test]
    fn other_fast_oscillations_far_out_are_refused() {
        let x = Expr::sym("x");
        let inf = (&Expr::infinity(), f64::INFINITY);
        let e = func(Func::Sin, vec![mul(vec![x.clone(), x.clone()])]);
        let c = Expr::int(1_000_000_000);
        assert!(matches!(
            far_range(&e, "x", (&c, 1e9), inf),
            Some(Far::Refused(_))
        ));
        // Near the origin the usual methods apply.
        assert!(far_range(&e, "x", (&Expr::int(10), 10.0), inf).is_none());
    }

    #[test]
    fn only_growing_arguments_oscillate() {
        let x = Expr::sym("x");
        let inv = pow(x.clone(), Expr::int(-1));
        assert!(oscillates(&func(Func::Sin, vec![x.clone()]), "x"));
        assert!(oscillates(
            &func(Func::Cos, vec![pow(x.clone(), Expr::rational(1, 2))]),
            "x"
        ));
        assert!(!oscillates(&func(Func::Sin, vec![inv.clone()]), "x"));
        assert!(!oscillates(
            &pow(func(Func::Sin, vec![inv]), Expr::int(2)),
            "x"
        ));
        assert!(!oscillates(
            &func(Func::Sin, vec![func(Func::Ln, vec![x])]),
            "x"
        ));
    }
}
