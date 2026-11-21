//! Improper integrals the adaptive quadrature cannot settle: convergence decided by limit
//! comparison with a p-integral (`|x − p|^(−α)` with `α < 1` beside a singular point,
//! `x^(−α)` with `α > 1` at an infinite bound, after bounding sines and cosines by 1),
//! then the value by double-exponential (tanh–sinh) quadrature for endpoint
//! singularities and by Richardson extrapolation over whole periods for slowly decaying
//! tails.

use std::collections::BTreeMap;
use std::f64::consts::PI;

use crate::calc::limit::limit;
use crate::eval::EmptyEnv;
use crate::expr::{Expr, ExprNode, LimitDir};
use crate::functions::{func, Func};
use crate::number::format_f64;
use crate::simplify::{add, mul, pow, simplify};

use super::numeric::adaptive;

/// Most pieces of length π summed for one slowly decaying tail.
const MAX_TAIL_PIECES: usize = 1_000_000;

/// The value and error estimate of a convergent improper integral, with the reasons it
/// converges; `None` when convergence is not proven or the value does not settle.
pub(super) fn convergent(
    e: &Expr,
    var: &str,
    points: &[(Expr, f64)],
) -> Option<(f64, f64, String)> {
    let mut reasons = Vec::new();
    let mut total = 0.0;
    let mut err = 0.0;
    let f = |x: f64| -> Option<f64> {
        let env: BTreeMap<String, f64> = [(var.to_string(), x)].into_iter().collect();
        e.eval_f64(&env).ok().filter(|v| v.is_finite())
    };
    for w in points.windows(2) {
        let ((pa, a), (pb, b)) = ((&w[0].0, w[0].1), (&w[1].0, w[1].1));
        // Beside a finite singular end the integrand is summed numerically up to a
        // distance δ and as c|x − p|^(−α) inside it, since a float cannot resolve x − p
        // much below the spacing of floats near p.
        let (mut lo, mut hi, mut near) = (a, b, 0.0);
        for (p, v, side) in [(pa, a, LimitDir::Right), (pb, b, LimitDir::Left)] {
            if v.is_infinite() {
                reasons.push(infinite_end(e, var, p, v)?);
            } else if f(v).is_none() {
                let end = finite_end(e, var, p, v, side)?;
                if let Some(r) = end.reason {
                    reasons.push(r);
                    let delta = 1e-8 * v.abs().max(1.0);
                    near += end.c * delta.powf(1.0 - end.alpha) / (1.0 - end.alpha);
                    if side == LimitDir::Right {
                        lo = v + delta;
                    } else {
                        hi = v - delta;
                    }
                }
            }
        }
        if !(lo.is_nan() || hi.is_nan()) && lo >= hi {
            return None;
        }
        let (v, de) = match (a.is_finite(), b.is_finite()) {
            (true, true) => tanh_sinh(&f, lo, hi).map(|(v, de)| (v + near, de))?,
            (true, false) => {
                let c = lo.max(0.0);
                let (v1, e1) = if c > lo {
                    tanh_sinh(&f, lo, c)?
                } else {
                    (0.0, 0.0)
                };
                let (v2, e2) = tail(&f, c, 1.0, var, e)?;
                (v1 + v2 + near, e1 + e2)
            }
            (false, true) => {
                let c = hi.min(0.0);
                let (v1, e1) = if c < hi {
                    tanh_sinh(&f, c, hi)?
                } else {
                    (0.0, 0.0)
                };
                let (v2, e2) = tail(&f, c, -1.0, var, e)?;
                (v1 + v2 + near, e1 + e2)
            }
            (false, false) => {
                let (v1, e1) = tail(&f, 0.0, 1.0, var, e)?;
                let (v2, e2) = tail(&f, 0.0, -1.0, var, e)?;
                (v1 + v2, e1 + e2)
            }
        };
        total += v;
        err += de;
    }
    if reasons.is_empty() {
        return None;
    }
    let note = format!(
        "the integral converges: {}; its value by double-exponential quadrature and extrapolation is ≈ {} (estimated error ≤ {})",
        reasons.join("; "),
        format_f64(total, 10),
        format_f64(err.max(f64::MIN_POSITIVE), 2)
    );
    Some((total, err, note))
}

/// `a/b` (`b ≤ 100`) within 0.0002 of `x` (an exponent read off two samples).
fn small_fraction(x: f64) -> Option<(i64, i64)> {
    let mut best: Option<(i64, i64, f64)> = None;
    for b in 1..=100i64 {
        let a = (x * b as f64).round() as i64;
        let d = (x - a as f64 / b as f64).abs();
        if best.is_none_or(|(_, _, bd)| d < bd - 1e-12) {
            best = Some((a, b, d));
        }
    }
    best.filter(|(_, _, d)| *d < 2e-4).map(|(a, b, _)| (a, b))
}

fn finite_limit(e: &Expr, var: &str, at: &Expr, dir: LimitDir) -> Option<f64> {
    let l = limit(e, var, at, dir, None).ok()?;
    if l.is_infinite() {
        return None;
    }
    l.eval_f64(&EmptyEnv).ok().filter(|v| v.is_finite())
}

/// The behaviour beside a finite end where the integrand is not finite.
struct End {
    /// Why the piece converges there (`None` for a removable point).
    reason: Option<String>,
    /// `f ≈ c |x − p|^(−α)` beside the point (`c = 0` when bounded).
    c: f64,
    alpha: f64,
}

/// Beside a finite point where the integrand blows up: `|f| ~ c |x − p|^(−α)` with
/// `α < 1`, confirmed by the limit of `|x − p|^α f` (symbolic, else numerical).
fn finite_end(e: &Expr, var: &str, p: &Expr, pv: f64, side: LimitDir) -> Option<End> {
    let dir = if side == LimitDir::Right { 1.0 } else { -1.0 };
    let at = |d: f64| {
        let env: BTreeMap<String, f64> = [(var.to_string(), pv + dir * d)].into_iter().collect();
        e.eval_f64(&env).ok().filter(|v| v.is_finite() && *v != 0.0)
    };
    let (d1, d2) = (1e-5, 1e-7);
    let (f1, f2) = (at(d1)?, at(d2)?);
    let alpha = -(f2.abs().ln() - f1.abs().ln()) / (d2.ln() - d1.ln());
    let (num, den) = small_fraction(alpha)?;
    if num == 0 {
        // Bounded beside the point (sin x / x at 0): nothing to prove.
        settled(&at)?;
        return Some(End {
            reason: None,
            c: 0.0,
            alpha: 0.0,
        });
    }
    if num >= den || num < 0 {
        return None;
    }
    let a = num as f64 / den as f64;
    let x = Expr::sym(var);
    let gap = add(vec![x, mul(vec![Expr::int(-1), p.clone()])]);
    // |x − p|^α as ((x − p)²)^(α/2).
    let weight = pow(pow(gap, Expr::int(2)), Expr::rational(num, 2 * den));
    let l = match finite_limit(&simplify(&mul(vec![weight, e.clone()])), var, p, side) {
        Some(l) => l,
        None => settled(&|d: f64| at(d).map(|v| v * d.powf(a)))?,
    };
    if l == 0.0 {
        return None;
    }
    let r = Expr::rational(num, den);
    Some(End {
        reason: Some(format!(
            "beside {var} = {p} the integrand behaves like {}·|{var} − {p}|^(−{r}), and ∫ |{var} − {p}|^(−{r}) d{var} converges since {r} < 1 (limit comparison)",
            format_f64(l, 6),
        )),
        c: l,
        alpha: a,
    })
}

/// The common value of `g(d)` for `d = 10⁻³ … 10⁻⁹` when the values agree to 0.1%
/// (a numerical limit as `d → 0`).
fn settled(g: &dyn Fn(f64) -> Option<f64>) -> Option<f64> {
    let vals: Vec<f64> = [1e-3, 1e-5, 1e-7, 1e-9]
        .iter()
        .map(|d| g(*d))
        .collect::<Option<_>>()?;
    let last = *vals.last()?;
    let spread = vals[1..]
        .iter()
        .map(|v| (v - last).abs())
        .fold(0.0, f64::max);
    (last.is_finite() && spread <= 1e-3 * last.abs()).then_some(last)
}

/// `|f|` with sine and cosine factors bounded by 1 (`sin²x/x² ≤ 1/x²`), when what is
/// left has no trigonometric part.
fn envelope(e: &Expr) -> Option<Expr> {
    let bounded = |f: &Expr| match f.node() {
        ExprNode::Func(Func::Sin | Func::Cos, _) => true,
        ExprNode::Pow(b, k) => {
            matches!(b.node(), ExprNode::Func(Func::Sin | Func::Cos, _))
                && k.as_number().is_some_and(|n| n.to_f64() > 0.0)
        }
        _ => false,
    };
    let kept: Vec<Expr> = match e.node() {
        ExprNode::Mul(fs) => fs.iter().filter(|f| !bounded(f)).cloned().collect(),
        _ if bounded(e) => Vec::new(),
        _ => vec![e.clone()],
    };
    let g = simplify(&func(Func::Abs, vec![mul(kept)]));
    let trig = |x: &Expr| x.to_string().contains("sin") || x.to_string().contains("cos");
    (!trig(&g)).then_some(g)
}

/// At an infinite bound: `|f| ≤ g ~ c |x|^(−α)` with `α > 1`, confirmed by the limit of
/// `|x|^α g`.
fn infinite_end(e: &Expr, var: &str, p: &Expr, pv: f64) -> Option<String> {
    let g = envelope(e)?;
    let dir = pv.signum();
    let at = |x: f64| {
        let env: BTreeMap<String, f64> = [(var.to_string(), dir * x)].into_iter().collect();
        g.eval_f64(&env).ok().filter(|v| v.is_finite() && *v > 0.0)
    };
    let (x1, x2) = (1e4, 1e6);
    let alpha = -(at(x2)?.ln() - at(x1)?.ln()) / (x2.ln() - x1.ln());
    let (num, den) = small_fraction(alpha)?;
    if num <= den {
        return None;
    }
    let x = Expr::sym(var);
    let weight = pow(pow(x, Expr::int(2)), Expr::rational(num, 2 * den));
    finite_limit(
        &simplify(&mul(vec![weight, g.clone()])),
        var,
        p,
        LimitDir::Both,
    )?;
    let a = Expr::rational(num, den);
    Some(format!(
        "as {var} → {p}, |f| ≤ {g} ≤ C·|{var}|^(−{a}) and ∫ |{var}|^(−{a}) d{var} converges at infinity since {a} > 1 (comparison)"
    ))
}

/// `∫_a^b f` by tanh–sinh quadrature, which tolerates integrable endpoint singularities.
fn tanh_sinh(f: &dyn Fn(f64) -> Option<f64>, a: f64, b: f64) -> Option<(f64, f64)> {
    let half = (b - a) / 2.0;
    let term = |t: f64| -> f64 {
        let u = PI / 2.0 * t.sinh();
        let ch = u.cosh();
        let w = half * PI / 2.0 * t.cosh() / (ch * ch);
        // Distance to the nearer end, without cancellation.
        let gap = 2.0 * half / (1.0 + (2.0 * u.abs()).exp());
        let x = if t >= 0.0 { b - gap } else { a + gap };
        if x <= a || x >= b || w == 0.0 {
            return 0.0;
        }
        f(x).map_or(0.0, |y| y * w)
    };
    let mut h = 0.5;
    let mut sum = term(0.0);
    let mut j = 1;
    loop {
        let t = j as f64 * h;
        if t > 4.5 {
            break;
        }
        sum += term(t) + term(-t);
        j += 1;
    }
    let mut prev = sum * h;
    for _ in 0..8 {
        crate::budget::check().ok()?;
        h /= 2.0;
        let mut j = 1;
        loop {
            let t = j as f64 * h;
            if t > 4.5 {
                break;
            }
            sum += term(t) + term(-t);
            j += 2;
        }
        let cur = sum * h;
        let diff = (cur - prev).abs();
        if diff <= 1e-11 * (1.0 + cur.abs()) {
            return Some((cur, diff.max(1e-15 * cur.abs())));
        }
        prev = cur;
    }
    None
}

/// `∫_c^{dir·∞} f` for a slowly decaying (possibly oscillating) integrand: the integrals
/// to `X_k = dir·2π·n₀·2^k` (whole periods, so the oscillation sits at the same phase
/// each time) extrapolated by Richardson in powers of `1/X`.
fn tail(
    f: &dyn Fn(f64) -> Option<f64>,
    c: f64,
    dir: f64,
    var: &str,
    e: &Expr,
) -> Option<(f64, f64)> {
    let g = envelope(e)?;
    let at = |x: f64| {
        let env: BTreeMap<String, f64> = [(var.to_string(), dir * x)].into_iter().collect();
        g.eval_f64(&env).ok().filter(|v| v.is_finite() && *v > 0.0)
    };
    let alpha = -(at(1e6)?.ln() - at(1e4)?.ln()) / (1e6f64.ln() - 1e4f64.ln());
    let lead = alpha.round().max(2.0) - 1.0;
    let mut n0 = 4.0;
    while 2.0 * PI * n0 <= c.abs() + 1.0 {
        n0 *= 2.0;
    }
    let levels = 9;
    // The pieces from `c` to `2^8` times as far grow with |c|: past this many, or where
    // a piece of length π no longer moves a double (|c| above about 10^16), give up.
    let mut budget = MAX_TAIL_PIECES;
    let mut sums: Vec<f64> = Vec::with_capacity(levels);
    let mut acc = 0.0;
    let mut from = c;
    let mut acc_err = 0.0;
    for k in 0..levels {
        let to = dir * 2.0 * PI * n0 * 2f64.powi(k as i32);
        // Pieces of length π.
        let mut x = from;
        while (to - x) * dir > 0.0 {
            crate::budget::check().ok()?;
            budget = budget.checked_sub(1)?;
            let next = if (to - (x + dir * PI)) * dir > 0.0 {
                x + dir * PI
            } else {
                to
            };
            if next == x {
                return None;
            }
            let (lo, hi) = if dir > 0.0 { (x, next) } else { (next, x) };
            let (v, de) = adaptive(&mut |t| f(t), lo, hi, 1e-13, 200).ok()?;
            acc += dir * v;
            acc_err += de;
            x = next;
        }
        from = to;
        sums.push(acc);
    }
    // Richardson: error terms in X^(−lead), X^(−lead−1), …; X doubles each level.
    let mut table = sums.clone();
    let mut best = (table[levels - 1], f64::INFINITY);
    for j in 0..5 {
        let factor = 2f64.powf(lead + j as f64);
        let next: Vec<f64> = table
            .windows(2)
            .map(|w| w[1] + (w[1] - w[0]) / (factor - 1.0))
            .collect();
        if next.len() < 2 {
            break;
        }
        let est = (next[next.len() - 1] - next[next.len() - 2]).abs();
        if est < best.1 {
            best = (next[next.len() - 1], est);
        }
        table = next;
    }
    let (v, est) = best;
    (est <= 1e-8 * (1.0 + v.abs())).then_some((v, est + acc_err))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(e: &Expr, pts: &[(Expr, f64)]) -> f64 {
        convergent(e, "x", pts).expect("convergent").0
    }

    #[test]
    fn endpoint_singularities_and_slow_tails() {
        let x = Expr::sym("x");
        // ∫₀² (x − 1)^(−2/3) dx = 6 (real cube roots).
        let e = simplify(&pow(
            add(vec![x.clone(), Expr::int(-1)]),
            Expr::rational(-2, 3),
        ));
        let pts = [
            (Expr::int(0), 0.0),
            (Expr::int(1), 1.0),
            (Expr::int(2), 2.0),
        ];
        assert!((value(&e, &pts) - 6.0).abs() < 1e-8);
        // ∫₀^∞ sin²x/x² dx = π/2.
        let s = simplify(&mul(vec![
            pow(func(Func::Sin, vec![x.clone()]), Expr::int(2)),
            pow(x.clone(), Expr::int(-2)),
        ]));
        let pts = [(Expr::int(0), 0.0), (Expr::infinity(), f64::INFINITY)];
        assert!((value(&s, &pts) - PI / 2.0).abs() < 1e-8);
        // ∫₀¹ x^(−99/100) dx = 100.
        let p = pow(x.clone(), Expr::rational(-99, 100));
        let pts = [(Expr::int(0), 0.0), (Expr::int(1), 1.0)];
        assert!((value(&p, &pts) - 100.0).abs() < 1e-6);
        // ∫₁^∞ 1/x dx is not proved convergent.
        let h = pow(x, Expr::int(-1));
        let pts = [(Expr::int(1), 1.0), (Expr::infinity(), f64::INFINITY)];
        assert!(convergent(&h, "x", &pts).is_none());
    }

    /// A tail starting where a step of π no longer moves a double ends at once, with
    /// no budget installed (it looped until the budget stopped it).
    #[test]
    fn a_tail_from_very_far_out_gives_up_at_once() {
        let x = Expr::sym("x");
        let e = simplify(&mul(vec![
            func(Func::Sin, vec![x.clone()]),
            pow(x, Expr::int(-2)),
        ]));
        let f = |t: f64| Some(t.sin() / (t * t));
        let start = std::time::Instant::now();
        assert!(tail(&f, 1e17, 1.0, "x", &e).is_none());
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
    }
}
