//! Definite and improper integrals: the fundamental theorem of calculus with limits at
//! infinite and singular endpoints, splitting at singular points inside the range, and a
//! numerical fallback (or cross-check) by adaptive Gauss–Kronrod quadrature.

use std::collections::BTreeMap;
use std::f64::consts::PI;

use num_traits::Zero;

use crate::calc::limit::limit;
use crate::error::{MathError, MathResult};
use crate::eval::EmptyEnv;
use crate::expr::{Constant, Expr, ExprNode, LimitDir};
use crate::factor::factor_poly;
use crate::functions::{func, Func};
use crate::number::format_f64;
use crate::poly::Poly;
use crate::simplify::{add, mul, simplify};
use crate::steps::Steps;

use super::engine::{div, linear_coeffs};
use super::numeric::{quad, DEFAULT_TOLERANCE};
use super::{definite_holder, integrate};

/// Subintervals allowed for the numerical value of one definite integral.
const MAX_INTERVALS: usize = 2_000;
/// Width of the search window for singular points on an infinite range.
const WINDOW: f64 = 1_000.0;
/// Samples used to look for sign changes when hunting singular points.
const SCAN_SAMPLES: usize = 4_000;

/// `∫_a^b expr d(var)`.
///
/// Exact when an antiderivative is found: `F(b) − F(a)`, with limits at infinite bounds and
/// at points where the integrand is not finite (improper integrals), splitting the range at
/// singular points inside it. An improper integral with an infinite or oscillating limit is
/// [`MathError::Divergent`] (so `∫_{−1}^{1} 1/x² dx` diverges). Without a closed form the
/// value is numerical and returned as an approximate number. With symbolic bounds or
/// parameters the integrand is assumed continuous between the bounds.
pub fn integrate_definite(
    expr: &Expr,
    var: &str,
    a: &Expr,
    b: &Expr,
    steps: Option<&mut Steps>,
) -> MathResult<Expr> {
    let mut log = steps;
    let e = simplify(expr);
    let (a, b) = (simplify(a), simplify(b));
    if a.contains_sym(var) || b.contains_sym(var) {
        return Err(MathError::Undefined(format!(
            "a bound that depends on the integration variable {var}"
        )));
    }
    if a.is_undefined() || b.is_undefined() {
        return Err(MathError::Undefined("a bound".into()));
    }
    if a == b {
        Steps::record(
            &mut log,
            "equal bounds",
            &definite_holder(&e, var, &a, &b),
            &Expr::zero(),
            None,
        );
        return Ok(Expr::zero());
    }
    let only_var = e.free_symbols().iter().all(|s| &**s == var);
    match (bound_value(&a), bound_value(&b)) {
        (Some(av), Some(bv)) if only_var => {
            if av > bv {
                let mut inner = Steps::new();
                let r = integrate_definite(&e, var, &b, &a, Some(&mut inner))?;
                let r = simplify(&mul(vec![Expr::int(-1), r]));
                if let Some(l) = log.as_deref_mut() {
                    l.push(
                        "reverse the bounds",
                        definite_holder(&e, var, &a, &b),
                        mul(vec![Expr::int(-1), definite_holder(&e, var, &b, &a)]),
                        None,
                    );
                    l.0.extend(inner.0);
                }
                return Ok(r);
            }
            numeric_bounds(&e, var, (&a, av), (&b, bv), log)
        }
        _ => symbolic_bounds(&e, var, &a, &b, log),
    }
}

/// The value of a bound as `f64` (`±∞` allowed), if it is a fixed real number.
fn bound_value(e: &Expr) -> Option<f64> {
    if e.is_infinite() {
        return Some(if matches!(e.node(), ExprNode::Const(Constant::Infinity)) {
            f64::INFINITY
        } else {
            f64::NEG_INFINITY
        });
    }
    let v = e.eval_f64(&EmptyEnv).ok()?;
    v.is_finite().then_some(v)
}

fn symbolic_bounds(
    e: &Expr,
    var: &str,
    a: &Expr,
    b: &Expr,
    log: Option<&mut Steps>,
) -> MathResult<Expr> {
    let mut local = Steps::new();
    let big_f = integrate(e, var, Some(&mut local)).map_err(|_| {
        MathError::NoClosedForm(format!(
            "∫ {e} d{var} (the bounds or parameters are symbolic, so no numerical value either)"
        ))
    })?;
    let at = |p: &Expr| -> MathResult<Expr> {
        if p.is_infinite() {
            return limit(&big_f, var, p, LimitDir::Both, None);
        }
        Ok(simplify(&big_f.subs(var, p)))
    };
    let (upper, lower) = (at(b)?, at(a)?);
    let value = simplify(&add(vec![
        upper.clone(),
        mul(vec![Expr::int(-1), lower.clone()]),
    ]));
    if has_bad_constant(&value) {
        return Err(MathError::Divergent(format!(
            "∫ {e} d{var} from {a} to {b}"
        )));
    }
    if let Some(l) = log {
        l.0.extend(local.0);
        l.push(
            "fundamental theorem of calculus",
            definite_holder(e, var, a, b),
            value.clone(),
            Some(format!(
                "F({var}) = {big_f}; F({b}) − F({a}) = ({upper}) − ({lower})"
            )),
        );
    }
    Ok(value)
}

fn has_bad_constant(e: &Expr) -> bool {
    matches!(
        e.node(),
        ExprNode::Const(Constant::Undefined | Constant::Infinity)
    ) || e.children().iter().any(|c| has_bad_constant(c))
}

fn numeric_bounds(
    e: &Expr,
    var: &str,
    (a, av): (&Expr, f64),
    (b, bv): (&Expr, f64),
    mut log: Option<&mut Steps>,
) -> MathResult<Expr> {
    let inner = singular_points(e, var, av, bv);
    let mut points: Vec<(Expr, f64)> = vec![(a.clone(), av)];
    points.extend(inner.iter().cloned());
    points.push((b.clone(), bv));
    let mut local = Steps::new();
    let exact = match integrate(e, var, Some(&mut local)) {
        Ok(big_f) => match ftc(e, &big_f, var, &points) {
            Ok((value, note)) => Some((value, big_f, note)),
            Err(err @ MathError::Divergent(_)) => return Err(err),
            Err(_) => None,
        },
        Err(_) => None,
    };
    let numeric = numeric_pieces(e, var, &points);
    let holder = definite_holder(e, var, a, b);
    if let Some((value, big_f, note)) = exact {
        let ev = value.eval_f64(&EmptyEnv).ok();
        let disagree = match (&numeric, ev) {
            (Ok((nv, err)), Some(ev)) => {
                *err <= 1e-8 * (1.0 + nv.abs()) && (nv - ev).abs() > 1e-6 * (1.0 + nv.abs())
            }
            _ => false,
        };
        if !disagree {
            if let Some(l) = log.as_deref_mut() {
                l.0.extend(local.0);
                if !inner.is_empty() {
                    let at: Vec<String> = inner.iter().map(|(p, _)| p.to_string()).collect();
                    l.push(
                        "split at singular points",
                        holder.clone(),
                        holder.clone(),
                        Some(format!(
                            "the integrand is not finite at {var} = {}, so the integral is split there and each piece is improper",
                            at.join(", ")
                        )),
                    );
                }
                l.push(
                    "fundamental theorem of calculus",
                    holder,
                    value.clone(),
                    Some(format!("F({var}) = {big_f}; {note}")),
                );
            }
            return Ok(value);
        }
    }
    match numeric {
        Ok((v, err)) => {
            let result = Expr::float(v);
            Steps::record(
                &mut log,
                "numerical integration",
                &holder,
                &result,
                Some(format!(
                    "no closed form was found, so adaptive Gauss–Kronrod (7–15 point) quadrature gives ≈ {} with estimated error ≤ {}",
                    format_f64(v, 10),
                    format_f64(err.max(f64::MIN_POSITIVE), 2)
                )),
            );
            Ok(result)
        }
        Err(err) => Err(err),
    }
}

/// `Σ (F(right⁻) − F(left⁺))` over the pieces, with limits where needed.
fn ftc(f: &Expr, big_f: &Expr, var: &str, points: &[(Expr, f64)]) -> MathResult<(Expr, String)> {
    let mut total = Vec::new();
    let mut notes = Vec::new();
    for w in points.windows(2) {
        let (l, r) = (&w[0], &w[1]);
        let upper = value_at(f, big_f, var, r, LimitDir::Left)?;
        let lower = value_at(f, big_f, var, l, LimitDir::Right)?;
        notes.push(format!(
            "F({}) − F({}) = ({}) − ({})",
            r.0, l.0, upper.0, lower.0
        ));
        if upper.1 || lower.1 {
            notes.push("(improper: the values at the ends are limits)".into());
        }
        total.push(upper.0);
        total.push(mul(vec![Expr::int(-1), lower.0]));
    }
    let value = simplify(&add(total));
    if has_bad_constant(&value) {
        return Err(MathError::Divergent(format!("∫ {f} d{var}")));
    }
    Ok((value, notes.join("; ")))
}

/// `F` at `p` (by substitution when the integrand is finite there, otherwise as a one-sided
/// limit) and whether a limit was needed.
fn value_at(
    f: &Expr,
    big_f: &Expr,
    var: &str,
    p: &(Expr, f64),
    side: LimitDir,
) -> MathResult<(Expr, bool)> {
    let diverges = || MathError::Divergent(format!("∫ {f} d{var} (near {var} = {})", p.0));
    let finite_here = p.1.is_finite() && {
        let env: BTreeMap<String, f64> = [(var.to_string(), p.1)].into_iter().collect();
        f.eval_f64(&env).is_ok_and(f64::is_finite)
    };
    if finite_here {
        let v = simplify(&big_f.subs(var, &p.0));
        if !has_bad_constant(&v) && v.eval_f64(&EmptyEnv).is_ok_and(f64::is_finite) {
            return Ok((v, false));
        }
    }
    let dir = if p.0.is_infinite() {
        LimitDir::Both
    } else {
        side
    };
    match limit(big_f, var, &p.0, dir, None) {
        Ok(v) if v.is_infinite() || has_bad_constant(&v) => Err(diverges()),
        Ok(v) => Ok((v, true)),
        Err(MathError::LimitDoesNotExist(_)) => Err(diverges()),
        Err(other) => Err(other),
    }
}

fn numeric_pieces(e: &Expr, var: &str, points: &[(Expr, f64)]) -> MathResult<(f64, f64)> {
    let mut env: BTreeMap<String, f64> = BTreeMap::new();
    let mut g = |x: f64| {
        env.insert(var.to_string(), x);
        e.eval_f64(&env).ok()
    };
    let (mut total, mut err) = (0.0, 0.0);
    for w in points.windows(2) {
        let (v, de) = quad(&mut g, w[0].1, w[1].1, DEFAULT_TOLERANCE, MAX_INTERVALS)?;
        total += v;
        err += de;
    }
    Ok((total, err))
}

/// Points strictly inside `(lo, hi)` where the integrand is not finite, sorted.
pub(crate) fn singular_points(e: &Expr, var: &str, lo: f64, hi: f64) -> Vec<(Expr, f64)> {
    let wlo = if lo.is_finite() {
        lo
    } else if hi.is_finite() {
        hi - WINDOW
    } else {
        -WINDOW
    };
    let whi = if hi.is_finite() {
        hi
    } else {
        wlo.max(lo) + 2.0 * WINDOW
    };
    let mut zeros_of = Vec::new();
    dangerous(e, var, &mut zeros_of);
    let mut out: Vec<(Expr, f64)> = Vec::new();
    for g in zeros_of {
        for (p, v) in zeros(&g, var, wlo, whi) {
            let margin = 1e-12 * (1.0 + v.abs());
            if v > lo + margin && v < hi - margin && out.iter().all(|(_, w)| (w - v).abs() > margin)
            {
                out.push((p, v));
            }
        }
    }
    out.sort_by(|x, y| x.1.total_cmp(&y.1));
    out
}

/// Sub-expressions whose zeros make the integrand infinite or undefined.
fn dangerous(e: &Expr, var: &str, out: &mut Vec<Expr>) {
    if !e.contains_sym(var) {
        return;
    }
    match e.node() {
        ExprNode::Pow(b, x) if b.contains_sym(var) => {
            if x.as_number()
                .is_some_and(|n| n.is_real() && n.is_negative())
            {
                out.push(b.clone());
            }
        }
        ExprNode::Func(Func::Ln | Func::Log, args) => out.push(args[0].clone()),
        ExprNode::Func(Func::Tan | Func::Sec, args) => {
            out.push(func(Func::Cos, vec![args[0].clone()]));
        }
        ExprNode::Func(Func::Cot | Func::Csc, args) => {
            out.push(func(Func::Sin, vec![args[0].clone()]));
        }
        _ => {}
    }
    for c in e.children() {
        dangerous(c, var, out);
    }
}

/// Real zeros of `g` in `[lo, hi]`: exact for rational polynomials and trig functions of a
/// linear argument, otherwise found numerically.
fn zeros(g: &Expr, var: &str, lo: f64, hi: f64) -> Vec<(Expr, f64)> {
    if let Some(p) = Poly::from_expr(g, var) {
        return poly_zeros(&p, var, lo, hi);
    }
    if let ExprNode::Func(f @ (Func::Sin | Func::Cos), args) = g.node() {
        if let Some((a, c)) = linear_coeffs(&args[0], var) {
            if let (Ok(av), Ok(cv)) = (a.eval_f64(&EmptyEnv), c.eval_f64(&EmptyEnv)) {
                return trig_zeros(*f, (&a, av), (&c, cv), lo, hi);
            }
        }
    }
    let mut env: BTreeMap<String, f64> = BTreeMap::new();
    let mut h = |x: f64| {
        env.insert(var.to_string(), x);
        g.eval_f64(&env).ok().filter(|y| y.is_finite())
    };
    scan(&mut h, lo, hi)
        .into_iter()
        .map(|x| (Expr::float(x), x))
        .collect()
}

fn poly_zeros(p: &Poly, var: &str, lo: f64, hi: f64) -> Vec<(Expr, f64)> {
    let mut out = Vec::new();
    if p.degree().is_none_or(|d| d == 0) {
        return out;
    }
    let (_, factors) = factor_poly(p);
    for (f, _) in factors {
        let c = f.coeffs();
        if f.degree() == Some(1) && !c[1].is_zero() {
            let root = -&c[0] / &c[1];
            let v = crate::number::ratio_to_f64(&root);
            if v >= lo && v <= hi {
                out.push((Expr::ratio(root), v));
            }
        } else {
            let fe = f.to_expr(var);
            let mut env: BTreeMap<String, f64> = BTreeMap::new();
            let mut h = |x: f64| {
                env.insert(var.to_string(), x);
                fe.eval_f64(&env).ok()
            };
            out.extend(
                scan(&mut h, lo, hi)
                    .into_iter()
                    .map(|x| (Expr::float(x), x)),
            );
        }
    }
    out
}

/// Zeros of `sin(a x + c)` or `cos(a x + c)` in `[lo, hi]`, exactly.
fn trig_zeros(
    f: Func,
    (a, av): (&Expr, f64),
    (c, cv): (&Expr, f64),
    lo: f64,
    hi: f64,
) -> Vec<(Expr, f64)> {
    let mut out = Vec::new();
    if av == 0.0 {
        return out;
    }
    let offset = if f == Func::Cos { 0.5 } else { 0.0 };
    // a x + c = (k + offset)π
    let (t1, t2) = (av * lo + cv, av * hi + cv);
    let (tmin, tmax) = (t1.min(t2), t1.max(t2));
    let kmin = (tmin / PI - offset).ceil() as i64;
    let kmax = (tmax / PI - offset).floor() as i64;
    if kmax - kmin > 1_000 {
        return out;
    }
    for k in kmin..=kmax {
        let angle = mul(vec![
            add(vec![
                Expr::int(k),
                Expr::rational(if offset > 0.0 { 1 } else { 0 }, 2),
            ]),
            Expr::pi(),
        ]);
        let x = simplify(&div(
            add(vec![angle, mul(vec![Expr::int(-1), c.clone()])]),
            a.clone(),
        ));
        if let Ok(v) = x.eval_f64(&EmptyEnv) {
            out.push((x, v));
        }
    }
    out
}

/// Sign changes and exact zeros of `h` on a grid, refined by bisection.
fn scan(h: &mut dyn FnMut(f64) -> Option<f64>, lo: f64, hi: f64) -> Vec<f64> {
    let mut out = Vec::new();
    if !(lo.is_finite() && hi.is_finite()) || hi <= lo {
        return out;
    }
    let n = SCAN_SAMPLES;
    let step = (hi - lo) / n as f64;
    let mut prev: Option<(f64, f64)> = None;
    for i in 0..=n {
        let x = lo + step * i as f64;
        let Some(y) = h(x) else {
            prev = None;
            continue;
        };
        if y == 0.0 {
            out.push(x);
        } else if let Some((px, py)) = prev {
            if py != 0.0 && py.signum() != y.signum() {
                if let Some(root) = bisect(h, px, x) {
                    out.push(root);
                }
            }
        }
        prev = Some((x, y));
    }
    out
}

fn bisect(h: &mut dyn FnMut(f64) -> Option<f64>, mut a: f64, mut b: f64) -> Option<f64> {
    let mut fa = h(a)?;
    for _ in 0..200 {
        let m = 0.5 * (a + b);
        if m <= a || m >= b {
            break;
        }
        let fm = h(m)?;
        if fm == 0.0 {
            return Some(m);
        }
        if fm.signum() == fa.signum() {
            a = m;
            fa = fm;
        } else {
            b = m;
        }
    }
    Some(0.5 * (a + b))
}
