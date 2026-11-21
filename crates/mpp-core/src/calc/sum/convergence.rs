//! Convergence tests for `Σ_{k=a}^{∞} t(k)`, each reported by name: the divergence test,
//! geometric series, p-series, the ratio and root tests, limit comparison with a p-series,
//! the alternating series test, direct comparison (bounded factors) and the integral test.

use std::cmp::Ordering;

use super::antidiff::{poly_geometric_parts, telescoping_parts};
use super::rational::as_ratfun;
use super::{alternating_split, shift};
use crate::calc::integrate::integrate;
use crate::calc::limit::limit;
use crate::error::MathError;
use crate::expr::{Constant, Expr, ExprNode, LimitDir};
use crate::functions::{func, Func};
use crate::number::Number;
use crate::poly::as_poly;
use crate::simplify::{add, mul, pow, simplify};

/// Largest shift of a factorial argument expanded by the ratio test.
const MAX_FACTORIAL_SHIFT: i64 = 64;

/// The verdict of a convergence test.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConvergenceReport {
    /// `Some(true)` converges, `Some(false)` diverges, `None` when no test decided.
    pub converges: Option<bool>,
    /// The name of the test that decided (or `"none"`).
    pub test: String,
    /// A short explanation, such as `"the ratio |aₖ₊₁/aₖ| tends to 1/2 < 1"`.
    pub detail: String,
}

impl std::fmt::Display for ConvergenceReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let verdict = match self.converges {
            Some(true) => "converges",
            Some(false) => "diverges",
            None => "undecided",
        };
        write!(f, "{verdict} by the {}: {}", self.test, self.detail)
    }
}

fn report(converges: Option<bool>, test: &str, detail: String) -> ConvergenceReport {
    ConvergenceReport {
        converges,
        test: test.into(),
        detail,
    }
}

fn numeric(e: &Expr) -> Option<f64> {
    e.eval_f64(&crate::eval::EmptyEnv)
        .ok()
        .filter(|v| v.is_finite())
}

fn lim_inf(e: &Expr, k: &str) -> Result<Expr, MathError> {
    limit(e, k, &Expr::infinity(), LimitDir::Both, None)
}

/// Sample points far out, used for sign and monotonicity checks.
const SAMPLES: [f64; 7] = [20.0, 50.0, 100.0, 1e3, 1e4, 1e5, 1e6];

/// The sign of `t` for large `k`, when every finite, non-zero sample agrees.
pub(crate) fn eventual_sign(t: &Expr, k: &str) -> Option<Ordering> {
    let mut sign = None;
    let mut count = 0;
    for x in SAMPLES {
        let Some(v) = t
            .eval_f64(&[(k, x)])
            .ok()
            .filter(|v| v.is_finite() && *v != 0.0)
        else {
            continue;
        };
        let s = v.partial_cmp(&0.0)?;
        if sign.is_some_and(|p| p != s) {
            return None;
        }
        sign = Some(s);
        count += 1;
    }
    if count >= 3 {
        sign
    } else {
        None
    }
}

/// True when `|t|` decreases along the samples (and at neighbouring integers).
pub(crate) fn eventually_decreasing(t: &Expr, k: &str) -> bool {
    let mut prev = f64::INFINITY;
    let mut checked = 0;
    for x in SAMPLES {
        for y in [x, x + 1.0] {
            let Some(v) = t.eval_f64(&[(k, y)]).ok().filter(|v| v.is_finite()) else {
                return checked >= 6;
            };
            if v.abs() > prev {
                return false;
            }
            prev = v.abs();
            checked += 1;
        }
    }
    true
}

/// Replaces `factorial(u(k+1))` by `factorial(u(k)) · Π (u(k) + j)` (and the same for Γ) so
/// that ratios of factorials cancel.
fn expand_factorial_shifts(e: &Expr, k: &str) -> Expr {
    let node = e.map_children(&mut |c| expand_factorial_shifts(c, k));
    let rebuilt = crate::simplify::canonical(node);
    if let ExprNode::Func(f @ (Func::Factorial | Func::Gamma), args) = rebuilt.node() {
        if let [u] = args.as_slice() {
            let v = shift(u, k, -1);
            let m = simplify(&add(vec![u.clone(), mul(vec![Expr::int(-1), v.clone()])]));
            if let Some(m) = m.as_number().and_then(Number::as_i64) {
                if (1..=MAX_FACTORIAL_SHIFT).contains(&m) {
                    let offset = if *f == Func::Factorial { 1 } else { 0 };
                    let mut factors = vec![func(*f, vec![v.clone()])];
                    for j in 0..m {
                        factors.push(add(vec![v.clone(), Expr::int(j + offset)]));
                    }
                    return mul(factors);
                }
            }
        }
    }
    rebuilt
}

/// Binomial coefficients that depend on `k` written with factorials.
fn binomials_as_factorials(e: &Expr, k: &str) -> Expr {
    let node = e.map_children(&mut |c| binomials_as_factorials(c, k));
    let rebuilt = crate::simplify::canonical(node);
    if let ExprNode::Func(Func::Binomial, args) = rebuilt.node() {
        if let [a, b] = args.as_slice() {
            if rebuilt.contains_sym(k) {
                let diff = simplify(&add(vec![a.clone(), mul(vec![Expr::int(-1), b.clone()])]));
                return mul(vec![
                    func(Func::Factorial, vec![a.clone()]),
                    pow(func(Func::Factorial, vec![b.clone()]), Expr::int(-1)),
                    pow(func(Func::Factorial, vec![diff]), Expr::int(-1)),
                ]);
            }
        }
    }
    rebuilt
}

/// `t(k + 1)/t(k)` with factorials cancelled.
pub(crate) fn term_ratio(t: &Expr, k: &str) -> Expr {
    // `C(a, b) = a!/(b! (a − b)!)`, so ratios of binomial coefficients cancel too.
    let t = &binomials_as_factorials(t, k);
    let next = expand_factorial_shifts(&shift(t, k, 1), k);
    simplify(&mul(vec![next, pow(t.clone(), Expr::int(-1))]))
}

/// `t = c · (αk + β)^(−p)`: returns `p`.
fn p_series_exponent(t: &Expr, k: &str) -> Option<Expr> {
    let factors = match t.node() {
        ExprNode::Mul(fs) => fs.clone(),
        _ => vec![t.clone()],
    };
    let mut p = None;
    for f in factors {
        if !f.contains_sym(k) {
            continue;
        }
        let (base, x) = match f.node() {
            ExprNode::Pow(b, x) => (b.clone(), x.clone()),
            _ => (f.clone(), Expr::one()),
        };
        if p.is_some() || x.contains_sym(k) || as_poly(&base, k).is_none_or(|c| c.len() != 2) {
            return None;
        }
        p = Some(simplify(&mul(vec![Expr::int(-1), x])));
    }
    p
}

fn mentions_index_in_exponent(e: &Expr, k: &str) -> bool {
    match e.node() {
        ExprNode::Pow(_, x) if x.contains_sym(k) => true,
        _ => e
            .children()
            .iter()
            .any(|c| mentions_index_in_exponent(c, k)),
    }
}

fn is_zero_value(e: &Expr) -> bool {
    e.is_zero() || (e.is_constant() && numeric(e).is_some_and(|v| v.abs() < 1e-14))
}

/// Decides whether `Σ_{k=from}^{∞} term` converges, naming the test used.
///
/// The answer does not depend on where the sum starts, so `_from` is only checked by callers.
pub fn convergence(term: &Expr, index: &str, _from: &Expr) -> ConvergenceReport {
    let t = simplify(term);
    let k = index;
    if t.is_zero() {
        return report(Some(true), "zero series", "every term is 0".into());
    }
    if !t.contains_sym(k) {
        return match numeric(&t) {
            Some(_) => report(
                Some(false),
                "divergence test",
                format!("the terms are the constant {t}, not 0"),
            ),
            None => report(
                None,
                "divergence test",
                format!("the terms are the constant {t}; the series diverges unless it is 0"),
            ),
        };
    }
    if let Some((q, _, r)) = poly_geometric_parts(&t, k) {
        let name = if q.len() == 1 {
            "geometric series"
        } else {
            "ratio test"
        };
        return match numeric(&r).map(f64::abs) {
            Some(v) if (v - 1.0).abs() < 1e-15 => report(
                Some(false),
                "divergence test",
                format!("the terms keep the size of the first one (common ratio {r})"),
            ),
            Some(v) if v < 1.0 => report(
                Some(true),
                name,
                format!("the common ratio is {r} and |{r}| < 1"),
            ),
            Some(_) => report(
                Some(false),
                name,
                format!("the common ratio is {r} and |{r}| ≥ 1, so the terms do not tend to 0"),
            ),
            None => report(
                None,
                name,
                format!("the common ratio is {r}: it converges exactly when |{r}| < 1"),
            ),
        };
    }
    if t.free_symbols().iter().any(|s| &**s != k) {
        return report(
            None,
            "none",
            "the answer depends on the other symbols in the term".into(),
        );
    }
    let (alternating, mag) = match alternating_split(&t, k) {
        Some((_, f)) => (true, f),
        None => (false, t.clone()),
    };
    if let Some(r) = divergence_test(&mag, k) {
        return r;
    }
    let sign = eventual_sign(&mag, k);
    let pos = match sign {
        Some(Ordering::Less) => simplify(&mul(vec![Expr::int(-1), mag.clone()])),
        _ => mag.clone(),
    };
    let absolute = |r: ConvergenceReport| {
        if alternating && r.converges == Some(true) {
            ConvergenceReport {
                detail: format!("{} (the series converges absolutely)", r.detail),
                ..r
            }
        } else {
            r
        }
    };
    if sign.is_some() {
        if let Some(r) = p_series(&pos, k) {
            if !alternating || r.converges == Some(true) {
                return absolute(r);
            }
        }
        if let Some(r) = rational_comparison(&pos, k) {
            if !alternating || r.converges == Some(true) {
                return absolute(r);
            }
        }
    }
    if let Some(r) = telescoping(&t, k) {
        return r;
    }
    let root_first = has_power_tower(&pos, k);
    if root_first && sign.is_some() {
        if let Some(r) = root_test(&pos, k) {
            if !alternating || r.converges == Some(true) {
                return absolute(r);
            }
        }
    }
    if let Some(r) = ratio_test(&pos, k) {
        if !alternating || r.converges == Some(true) {
            return absolute(r);
        }
    }
    if sign.is_some() {
        if !root_first {
            if let Some(r) = root_test(&pos, k) {
                if !alternating || r.converges == Some(true) {
                    return absolute(r);
                }
            }
        }
        if let Some(r) = limit_comparison(&pos, k) {
            if !alternating || r.converges == Some(true) {
                return absolute(r);
            }
        }
    }
    if alternating {
        if sign.is_some() && eventually_decreasing(&mag, k) {
            return report(
                Some(true),
                "alternating series test",
                format!("the terms alternate in sign and |aₖ| = {pos} decreases to 0"),
            );
        }
        return report(
            None,
            "none",
            "the alternating series test does not apply".into(),
        );
    }
    if let Some(r) = bounded_comparison(&t, k) {
        return r;
    }
    if sign.is_some() {
        if let Some(r) = integral_test(&pos, k) {
            return r;
        }
    }
    report(None, "none", "no test could decide".into())
}

pub(crate) fn divergence_test(mag: &Expr, k: &str) -> Option<ConvergenceReport> {
    let l = lim_inf(mag, k).ok()?;
    if l.is_infinite() || (!is_zero_value(&l) && numeric(&l).is_some()) {
        return Some(report(
            Some(false),
            "divergence test",
            format!("the terms do not tend to 0 (their size tends to {l})"),
        ));
    }
    None
}

pub(crate) fn p_series(pos: &Expr, k: &str) -> Option<ConvergenceReport> {
    let p = p_series_exponent(pos, k)?;
    let v = numeric(&p)?;
    Some(if v > 1.0 {
        report(Some(true), "p-series", format!("p = {p} > 1"))
    } else {
        report(Some(false), "p-series", format!("p = {p} ≤ 1"))
    })
}

/// A rational term `p(k)/q(k)` behaves like `c/k^d` with `d = deg q − deg p`.
pub(crate) fn rational_comparison(pos: &Expr, k: &str) -> Option<ConvergenceReport> {
    let r = as_ratfun(pos, k)?;
    let d = r.den.degree()? as i64 - r.num.degree()? as i64;
    let converges = d >= 2;
    Some(report(
        Some(converges),
        "limit comparison test",
        format!(
            "aₖ behaves like a constant times 1/k^{d}, and Σ 1/k^{d} {} (p-series)",
            if converges { "converges" } else { "diverges" }
        ),
    ))
}

/// `aₖ = u(k) − u(k + m)` with `u` tending to a finite limit.
pub(crate) fn telescoping(t: &Expr, k: &str) -> Option<ConvergenceReport> {
    let (u, m) = telescoping_parts(t, k)?;
    let l = lim_inf(&u, k).ok()?;
    if l.is_infinite() || numeric(&l).is_none() {
        return None;
    }
    Some(report(
        Some(true),
        "telescoping series",
        format!("aₖ = u(k) − u(k + {m}) with u(k) = {u} tending to {l}"),
    ))
}

/// True when a power has the index in both its base and its exponent, like `(k/(2k+1))^k`.
pub(crate) fn has_power_tower(e: &Expr, k: &str) -> bool {
    match e.node() {
        ExprNode::Pow(b, x) if b.contains_sym(k) && x.contains_sym(k) => true,
        _ => e.children().iter().any(|c| has_power_tower(c, k)),
    }
}

pub(crate) fn ratio_test(pos: &Expr, k: &str) -> Option<ConvergenceReport> {
    let r = term_ratio(pos, k);
    if !r.contains_sym(k) && numeric(&r).is_none() {
        return None;
    }
    let l = lim_inf(&r, k).ok()?;
    let detail = |cmp: &str| format!("|aₖ₊₁/aₖ| = {r} tends to {l} {cmp} 1");
    if matches!(l.node(), ExprNode::Const(Constant::Infinity)) {
        return Some(report(Some(false), "ratio test", detail(">")));
    }
    let v = numeric(&l)?.abs();
    if (v - 1.0).abs() < 1e-12 {
        None
    } else if v < 1.0 {
        Some(report(Some(true), "ratio test", detail("<")))
    } else {
        Some(report(Some(false), "ratio test", detail(">")))
    }
}

pub(crate) fn root_test(pos: &Expr, k: &str) -> Option<ConvergenceReport> {
    if !mentions_index_in_exponent(pos, k) {
        return None;
    }
    let root = pow(pos.clone(), pow(Expr::sym(k), Expr::int(-1)));
    let l = lim_inf(&root, k).ok()?;
    let detail = |cmp: &str| format!("|aₖ|^(1/k) tends to {l} {cmp} 1");
    if matches!(l.node(), ExprNode::Const(Constant::Infinity)) {
        return Some(report(Some(false), "root test", detail(">")));
    }
    let v = numeric(&l)?;
    if (v - 1.0).abs() < 1e-12 {
        None
    } else if v < 1.0 {
        Some(report(Some(true), "root test", detail("<")))
    } else {
        Some(report(Some(false), "root test", detail(">")))
    }
}

/// Compares `pos` with `1/k^p` for a `p` read off numerically and then confirmed by an
/// exact limit.
pub(crate) fn limit_comparison(pos: &Expr, k: &str) -> Option<ConvergenceReport> {
    let (n1, n2) = (1e4, 1e5);
    let v1 = pos.eval_f64(&[(k, n1)]).ok()?;
    let v2 = pos.eval_f64(&[(k, n2)]).ok()?;
    if !(v1 > 0.0 && v2 > 0.0 && v1.is_finite() && v2.is_finite()) {
        return None;
    }
    let p_est = -(v2 / v1).log10();
    // Nearest multiple of 1/6 (integers, halves and thirds).
    let six = (p_est * 6.0).round();
    if (p_est - six / 6.0).abs() > 0.08 || six.abs() > 600.0 {
        return None;
    }
    let p = Expr::rational(six as i64, 6);
    let try_p = |p: &Expr| -> Option<Expr> {
        let scaled = mul(vec![pos.clone(), pow(Expr::sym(k), p.clone())]);
        lim_inf(&scaled, k).ok()
    };
    let pv = numeric(&p)?;
    let l = try_p(&p)?;
    let finite_positive = numeric(&l).is_some_and(|v| v > 0.0) && !l.is_infinite();
    let compared = |p: &Expr, l: &Expr, c: bool| {
        report(
            Some(c),
            "limit comparison test",
            format!("aₖ · k^{p} tends to {l}; Σ 1/k^{p} {} (p-series)", {
                if c {
                    "converges"
                } else {
                    "diverges"
                }
            }),
        )
    };
    if finite_positive {
        return Some(compared(&p, &l, pv > 1.0));
    }
    let is_zero = is_zero_value(&l);
    let is_inf = matches!(l.node(), ExprNode::Const(Constant::Infinity));
    if is_zero && pv > 1.0 {
        return Some(compared(&p, &l, true));
    }
    if is_inf && pv <= 1.0 {
        return Some(compared(&p, &l, false));
    }
    // A logarithmic factor: compare with a p halfway towards 1.
    let mid = simplify(&mul(vec![
        add(vec![p.clone(), Expr::one()]),
        Expr::rational(1, 2),
    ]));
    if (pv - 1.0).abs() < 1e-12 {
        return None;
    }
    let l2 = try_p(&mid)?;
    if pv > 1.0 && is_zero_value(&l2) {
        return Some(compared(&mid, &l2, true));
    }
    if pv < 1.0 && matches!(l2.node(), ExprNode::Const(Constant::Infinity)) {
        return Some(compared(&mid, &l2, false));
    }
    None
}

/// Drops factors bounded by a constant (`|sin|, |cos| ≤ 1`, `|atan| < π/2`, `|tanh| < 1`)
/// and compares with what is left.
pub(crate) fn bounded_comparison(t: &Expr, k: &str) -> Option<ConvergenceReport> {
    let factors = match t.node() {
        ExprNode::Mul(fs) => fs.clone(),
        _ => vec![t.clone()],
    };
    let bounded = |f: &Expr| -> bool {
        let (b, x) = match f.node() {
            ExprNode::Pow(b, x) => (b, Some(x)),
            _ => (f, None),
        };
        let positive_exp = x.is_none_or(|x| numeric(x).is_some_and(|v| v > 0.0));
        positive_exp
            && matches!(
                b.node(),
                ExprNode::Func(Func::Sin | Func::Cos | Func::Atan | Func::Tanh, _)
            )
    };
    if !factors.iter().any(|f| f.contains_sym(k) && bounded(f)) {
        return None;
    }
    let rest = mul(factors.into_iter().filter(|f| !bounded(f)).collect());
    let rest = match eventual_sign(&rest, k)? {
        Ordering::Less => simplify(&mul(vec![Expr::int(-1), rest])),
        _ => rest,
    };
    let inner = convergence(&rest, k, &Expr::one());
    (inner.converges == Some(true)).then(|| {
        report(
            Some(true),
            "comparison test",
            format!(
                "|aₖ| ≤ C·{rest} and Σ {rest} converges ({}); so the series converges absolutely",
                inner.test
            ),
        )
    })
}

/// The integral test from an exact antiderivative `F`: the series converges exactly when
/// `F` has a finite limit at `∞` (the term is positive and decreasing).
pub(crate) fn integral_test(pos: &Expr, k: &str) -> Option<ConvergenceReport> {
    if !eventually_decreasing(pos, k) {
        return None;
    }
    let antiderivative = integrate(pos, k, None).ok()?;
    // Logarithmic antiderivatives settle too slowly for the limit's numeric check; with
    // k = e^u they become plain functions of u.
    let at_inf = match lim_inf(&antiderivative, k) {
        Ok(v) => v,
        Err(MathError::Inconclusive(_)) => {
            let u = "\u{1}u";
            let swapped = antiderivative.subs(k, &pow(Expr::e(), Expr::sym(u)));
            limit(&swapped, u, &Expr::infinity(), LimitDir::Both, None).ok()?
        }
        Err(_) => return None,
    };
    if at_inf.is_infinite() {
        return Some(report(
            Some(false),
            "integral test",
            format!("∫ {pos} d{k} = {antiderivative} grows without bound"),
        ));
    }
    numeric(&at_inf)?;
    let value = simplify(&add(vec![
        at_inf,
        mul(vec![Expr::int(-1), antiderivative.subs(k, &Expr::int(3))]),
    ]));
    Some(report(
        Some(true),
        "integral test",
        format!("∫ {pos} d{k} from 3 to ∞ = {value} is finite"),
    ))
}
