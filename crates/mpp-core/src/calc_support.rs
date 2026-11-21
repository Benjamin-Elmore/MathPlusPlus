//! Small helpers shared by the tools: numeric evaluation, sampling for maxima, sign-aware
//! integration of `|h|`, and the real zeros of a function on an interval.

use std::collections::BTreeMap;

use crate::calc::integrate::integrate_definite;
use crate::domains::solve::{numeric::roots_in, solve, Range, Solutions};
use crate::domains::Ctx;
use crate::error::{MathError, MathResult};
use crate::eval::EmptyEnv;
use crate::expr::{Expr, ExprNode};
use crate::simplify::{add, mul, simplify};
use crate::steps::Steps;

/// Samples used when looking for the largest value of `|f|` on an interval.
const MAX_SAMPLES: usize = 4_000;

/// The value of a constant expression, if it is a finite real number.
pub(crate) fn value(e: &Expr) -> Option<f64> {
    e.eval_f64(&EmptyEnv).ok().filter(|v| v.is_finite())
}

/// `e` at `var = x`, if finite.
pub(crate) fn at(e: &Expr, var: &str, x: f64) -> Option<f64> {
    let env: BTreeMap<String, f64> = [(var.to_string(), x)].into_iter().collect();
    e.eval_f64(&env).ok().filter(|v| v.is_finite())
}

/// `a − b`.
pub(crate) fn sub(a: &Expr, b: &Expr) -> Expr {
    simplify(&add(vec![a.clone(), mul(vec![Expr::int(-1), b.clone()])]))
}

/// The largest value of `|e|` on `[a, b]`: a dense sample refined by golden-section search
/// around the best sample. `None` when `e` is not finite somewhere on the grid.
pub(crate) fn max_abs(e: &Expr, var: &str, a: f64, b: f64) -> Option<f64> {
    let (lo, hi) = (a.min(b), a.max(b));
    if lo == hi {
        return at(e, var, lo).map(f64::abs);
    }
    let n = MAX_SAMPLES;
    let h = (hi - lo) / n as f64;
    let mut best = (lo, 0.0_f64);
    for i in 0..=n {
        let x = lo + h * i as f64;
        let v = at(e, var, x)?.abs();
        if v > best.1 {
            best = (x, v);
        }
    }
    // Golden-section refinement of |e| on the two cells beside the best sample.
    let (mut l, mut r) = ((best.0 - h).max(lo), (best.0 + h).min(hi));
    let g = 0.5 * (5f64.sqrt() - 1.0);
    for _ in 0..60 {
        let m1 = r - g * (r - l);
        let m2 = l + g * (r - l);
        let (v1, v2) = (at(e, var, m1)?.abs(), at(e, var, m2)?.abs());
        if v1 > v2 {
            r = m2;
        } else {
            l = m1;
        }
        best.1 = best.1.max(v1).max(v2);
    }
    Some(best.1)
}

/// Real zeros of `h` strictly inside `(a, b)`, sorted: exact when the solver finds them,
/// otherwise numeric. Points where `h` is not finite are ignored.
pub(crate) fn zeros_inside(h: &Expr, var: &str, a: &Expr, b: &Expr) -> Vec<(Expr, f64)> {
    let (Some(av), Some(bv)) = (value(a), value(b)) else {
        return Vec::new();
    };
    let (lo, hi) = (av.min(bv), av.max(bv));
    let inside = |v: f64| v > lo + 1e-12 * (1.0 + lo.abs()) && v < hi - 1e-12 * (1.0 + hi.abs());
    let mut out: Vec<(Expr, f64)> = Vec::new();
    let mut ctx = Ctx::new();
    let range = Range::closed(a.clone(), b.clone());
    if let Ok(Solutions::Finite { values, exact }) = solve(h, var, Some(&range), &mut ctx) {
        for v in values {
            if let Some(x) = value(&v) {
                if inside(x) {
                    let shown = if exact { v } else { Expr::float(x) };
                    out.push((shown, x));
                }
            }
        }
        if exact {
            out.sort_by(|p, q| p.1.total_cmp(&q.1));
            out.dedup_by(|p, q| (p.1 - q.1).abs() < 1e-12);
            return out;
        }
    }
    if let Ok(roots) = roots_in(h, var, lo, hi) {
        for x in roots {
            if inside(x)
                && out
                    .iter()
                    .all(|(_, y)| (y - x).abs() > 1e-9 * (1.0 + x.abs()))
            {
                out.push((Expr::float(x), x));
            }
        }
    }
    out.sort_by(|p, q| p.1.total_cmp(&q.1));
    out
}

/// `∫_a^b |h| d(var)`: the range is split where `h` changes sign and each piece is
/// integrated with the sign `h` has there. Returns the value and the split points.
pub(crate) fn integrate_abs(
    h: &Expr,
    var: &str,
    a: &Expr,
    b: &Expr,
    mut steps: Option<&mut Steps>,
) -> MathResult<(Expr, Vec<Expr>)> {
    let cuts = zeros_inside(h, var, a, b);
    let mut points: Vec<Expr> = vec![a.clone()];
    points.extend(cuts.iter().map(|(e, _)| e.clone()));
    points.push(b.clone());
    let mut total = Vec::new();
    for w in points.windows(2) {
        let (l, r) = (&w[0], &w[1]);
        let (Some(lv), Some(rv)) = (value(l), value(r)) else {
            return Err(MathError::Unsupported(
                "an interval whose ends are not numbers".into(),
            ));
        };
        let mid = 0.5 * (lv + rv);
        let sign = match at(h, var, mid) {
            Some(v) if v < 0.0 => -1,
            Some(_) => 1,
            None => {
                return Err(MathError::Undefined(format!(
                    "the integrand at {var} = {mid}"
                )))
            }
        };
        let piece = integrate_definite(h, var, l, r, steps.as_deref_mut())?;
        total.push(mul(vec![Expr::int(sign), piece]));
    }
    Ok((
        simplify(&add(total)),
        cuts.into_iter().map(|(e, _)| e).collect(),
    ))
}

/// The unevaluated integral `∫_a^b e d(var)` (for the steps).
pub(crate) fn integral_holder(e: &Expr, var: &str, a: &Expr, b: &Expr) -> Expr {
    Expr::new(ExprNode::Integral {
        expr: e.clone(),
        var: std::sync::Arc::from(var),
        bounds: Some((a.clone(), b.clone())),
    })
}

/// Uses `sin²u + cos²u = 1` (and `cosh²u − sinh²u = 1`): every `sin(u)²` is replaced by
/// `1 − cos(u)²` (and `sinh(u)²` by `cosh(u)² − 1`) and the result is kept when it is
/// smaller after expanding.
pub(crate) fn pythagorean_tidy(e: &Expr) -> Expr {
    use crate::functions::{func, Func};
    fn rewrite(e: &Expr) -> Expr {
        let node = e.map_children(&mut rewrite);
        let rebuilt = crate::simplify::canonical(node);
        if let ExprNode::Pow(b, x) = rebuilt.node() {
            if let (ExprNode::Func(f @ (Func::Sin | Func::Sinh), args), Some(n)) =
                (b.node(), x.as_number().and_then(|n| n.as_i64()))
            {
                if n == 2 {
                    let partner = if *f == Func::Sin {
                        Func::Cos
                    } else {
                        Func::Cosh
                    };
                    let c2 = crate::simplify::pow(func(partner, args.clone()), Expr::int(2));
                    return if *f == Func::Sin {
                        sub(&Expr::one(), &c2)
                    } else {
                        sub(&c2, &Expr::one())
                    };
                }
            }
        }
        rebuilt
    }
    fn expand_inside(e: &Expr) -> Expr {
        let node = e.map_children(&mut expand_inside);
        let rebuilt = crate::simplify::canonical(node);
        if matches!(rebuilt.node(), ExprNode::Add(_)) {
            if let Ok(x) = rebuilt.expand() {
                return x;
            }
        }
        rebuilt
    }
    let candidate = simplify(&expand_inside(&rewrite(e)));
    if candidate.size() < e.size() {
        candidate
    } else {
        e.clone()
    }
}

/// A variable name not free in `e`.
pub(crate) fn fresh(e: &Expr, preferred: &[&str]) -> String {
    preferred
        .iter()
        .map(|s| s.to_string())
        .chain((1..).map(|k| format!("t_{k}")))
        .find(|n| !e.contains_sym(n))
        .unwrap_or_else(|| "t_0".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maxima_and_zeros() {
        let x = Expr::sym("x");
        let f = x.pow(2) - Expr::int(2);
        let m = max_abs(&f, "x", -1.0, 3.0).unwrap();
        assert!((m - 7.0).abs() < 1e-9);
        let z = zeros_inside(&f, "x", &Expr::int(0), &Expr::int(3));
        assert_eq!(z.len(), 1);
        assert_eq!(z[0].0, Expr::int(2).sqrt());
        let (v, cuts) = integrate_abs(&x, "x", &Expr::int(-1), &Expr::int(1), None).unwrap();
        assert_eq!(v, Expr::one());
        assert_eq!(cuts, vec![Expr::zero()]);
    }
}
