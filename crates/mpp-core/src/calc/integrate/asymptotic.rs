//! Oscillating tails that start very far out, such as `∫_{10^17}^∞ sin x / x² dx`.
//!
//! A double near `10^17` is 16 apart from the next one, so the integrand cannot even be
//! sampled within one period there: any quadrature returns rounding noise. Such a tail
//! is worked out from the asymptotic expansion instead, by integrating by parts again
//! and again:
//!
//! `∫_c^∞ e^{ix} x^{−p} dx = i e^{ic} c^{−p} Σ_k (−i)^k (p)_k c^{−k}`,
//!
//! where `(p)_k` is the rising factorial. Cutting the series after `K` terms leaves at
//! most `2 (p)_K c^{−p−K}`, and `cos c`, `sin c` come from the exact `c` at high
//! precision. Other integrands with a sine or cosine are refused that far out.

use std::collections::HashMap;

use crate::expr::{Expr, ExprNode};
use crate::functions::{func, Func};
use crate::hp::{approx, Approx, Decimal};
use crate::number::format_f64;

/// From this distance on a tail's oscillation is not sampled in floating point (the
/// spacing of doubles there is about `1.5·10^−8`).
pub(super) const FAR: f64 = 1e8;

/// Most terms of the expansion summed.
const MAX_TERMS: usize = 40;

/// What [`far_tail`] found.
pub(super) enum Far {
    /// The value, its error bound and a note for the steps.
    Value(f64, f64, String),
    /// The tail oscillates too fast to sample and is not of a form worked out here.
    Refused(String),
}

/// `∫_c^∞ e` when the range starts at `c ≥` [`FAR`] (the expression `c_expr`) and the
/// integrand has a sine or cosine of the variable; `None` otherwise (the usual
/// numerical methods apply).
pub(super) fn far_tail(e: &Expr, var: &str, c_expr: &Expr, c: f64) -> Option<Far> {
    if !(c.is_finite() && c >= FAR) || !has_trig(e, var) {
        return None;
    }
    let refuse = || {
        Far::Refused(format!(
            "∫ {e} d{var} from {c_expr} to ∞ (the integrand oscillates far faster than it can be sampled that far out)"
        ))
    };
    let Some((k, sine, p)) = trig_power(e, var) else {
        return Some(refuse());
    };
    if p <= 0.0 {
        // Does not converge; the divergence tests say why.
        return None;
    }
    let (Some(s), Some(co)) = (
        decimal_of(&func(Func::Sin, vec![c_expr.clone()])),
        decimal_of(&func(Func::Cos, vec![c_expr.clone()])),
    ) else {
        return Some(refuse());
    };
    // Σ (−i)^k (p)_k c^{−k}, as (re, im).
    let z = 1.0 / c;
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
        let next = term * (p + kk as f64) * z;
        if next >= term {
            // The expansion has started to grow: stop before it.
            omitted = next;
            break;
        }
        omitted = next;
        if next <= 1e-20 * (sr.abs() + si.abs()) {
            break;
        }
        term = next;
    }
    let scale = c.powf(-p);
    // i e^{ic} S = (−sin c + i cos c)(sr + i si).
    let re = -s * sr - co * si;
    let im = -s * si + co * sr;
    let value = k * scale * if sine { im } else { re };
    let err = k.abs() * scale * 2.0 * omitted + value.abs() * 1e-15;
    let trig = if sine { "sin" } else { "cos" };
    let note = format!(
        "the range starts so far out that {trig} {var} cannot be sampled there in floating point, so the integral comes from its asymptotic expansion, integrating by parts repeatedly: ∫_c^∞ e^(i{var}) {var}^(−p) d{var} = i e^(ic) c^(−p) Σ (−i)^k (p)_k c^(−k), with p = {} and cos c, sin c from the exact c; ≈ {} (error ≤ {})",
        format_f64(p, 6),
        format_f64(value, 10),
        format_f64(err.max(f64::MIN_POSITIVE), 2)
    );
    Some(Far::Value(value, err, note))
}

fn has_trig(e: &Expr, var: &str) -> bool {
    matches!(e.node(), ExprNode::Func(Func::Sin | Func::Cos | Func::Tan, args)
        if args.iter().any(|a| a.free_symbols().iter().any(|s| &**s == var)))
        || e.children().iter().any(|c| has_trig(c, var))
}

/// `k · sin(x) · x^(−p)` or `k · cos(x) · x^(−p)`: `(k, is_sine, p)`.
fn trig_power(e: &Expr, var: &str) -> Option<(f64, bool, f64)> {
    let factors: Vec<Expr> = match e.node() {
        ExprNode::Mul(fs) => fs.to_vec(),
        _ => vec![e.clone()],
    };
    let is_var = |x: &Expr| x.as_sym() == Some(var);
    let mut k = 1.0;
    let mut p = 0.0;
    let mut trig: Option<bool> = None;
    for f in &factors {
        if let Some(n) = f.as_number() {
            k *= n.to_f64();
            continue;
        }
        match f.node() {
            ExprNode::Func(which @ (Func::Sin | Func::Cos), args)
                if trig.is_none() && args.len() == 1 && is_var(&args[0]) =>
            {
                trig = Some(*which == Func::Sin);
            }
            ExprNode::Sym(_) if is_var(f) => p -= 1.0,
            ExprNode::Pow(b, n) if is_var(b) => p -= n.as_number()?.to_f64(),
            _ => return None,
        }
    }
    (k.is_finite() && p.is_finite()).then_some((k, trig?, p))
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
        let cv = c.eval_f64(&crate::eval::EmptyEnv).unwrap();
        match far_tail(e, "x", c, cv) {
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
        let e = func(Func::Sin, vec![mul(vec![x.clone(), x.clone()])]);
        let c = Expr::int(1_000_000_000);
        assert!(matches!(far_tail(&e, "x", &c, 1e9), Some(Far::Refused(_))));
        // Near the origin the usual methods apply.
        assert!(far_tail(&e, "x", &Expr::int(10), 10.0).is_none());
    }
}
