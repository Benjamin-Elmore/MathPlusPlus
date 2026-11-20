//! Iterated integrals (double, triple …) over rectangles and regions with variable bounds,
//! and line integrals along a parametrised curve.

use std::collections::BTreeMap;

use crate::calc::diff::diff;
use crate::error::{MathError, MathResult};
use crate::eval::EmptyEnv;
use crate::expr::{Expr, ExprNode};
use crate::number::format_f64;
use crate::simplify::{add, mul, pow, simplify};
use crate::steps::Steps;
use crate::subs::subs_all;

use super::definite::integrate_definite;
use super::definite_holder;

/// Requested accuracy of each level of nested quadrature.
const NESTED_TOL: f64 = 1e-9;
/// Subintervals per level of nested quadrature.
const NESTED_INTERVALS: usize = 200;

/// The iterated integral `∫…∫ expr d(var₁) … d(varₙ)`.
///
/// `bounds` lists `(variable, lower, upper)` from the innermost integral outwards, as the
/// differentials are written: `∫_0^1 ∫_0^x f dy dx` is `[("y", 0, x), ("x", 0, 1)]`. A bound
/// may use the variables of the integrals outside it. Each stage is exact when it has a
/// closed form; otherwise the whole integral is computed by nested adaptive quadrature and
/// returned as an approximate number. An exact answer is also checked against the nested
/// quadrature when the bounds are numbers.
pub fn integrate_multiple(
    expr: &Expr,
    bounds: &[(&str, Expr, Expr)],
    steps: Option<&mut Steps>,
) -> MathResult<Expr> {
    let mut log = steps;
    if bounds.is_empty() {
        return Ok(simplify(expr));
    }
    for (i, (_, lo, hi)) in bounds.iter().enumerate() {
        for (inner, _, _) in &bounds[..=i] {
            if lo.contains_sym(inner) || hi.contains_sym(inner) {
                return Err(MathError::Undefined(format!(
                    "a bound that uses {inner}, which is integrated inside it"
                )));
            }
        }
    }
    let mut local = Steps::new();
    let mut current = simplify(expr);
    let mut exact = true;
    for (var, lo, hi) in bounds {
        let mut stage = Steps::new();
        match integrate_definite(&current, var, lo, hi, Some(&mut stage)) {
            Ok(v) if !contains_float(&v) => {
                local.push(
                    "iterated integral",
                    definite_holder(&current, var, lo, hi),
                    v.clone(),
                    Some(format!(
                        "integrate with respect to {var}, holding the other variables fixed"
                    )),
                );
                local.0.extend(stage.0);
                current = v;
            }
            Ok(_) | Err(MathError::NoClosedForm(_)) | Err(MathError::Inconclusive(_)) => {
                exact = false;
                break;
            }
            Err(err) => return Err(err),
        }
    }
    let numeric = nested_numeric(&simplify(expr), bounds);
    if exact {
        let agrees = match (&numeric, current.eval_f64(&EmptyEnv)) {
            (Ok((nv, err)), Ok(ev)) => {
                *err > 1e-7 * (1.0 + nv.abs()) || (nv - ev).abs() <= 1e-6 * (1.0 + nv.abs())
            }
            _ => true,
        };
        if agrees {
            if let Some(l) = log.as_deref_mut() {
                l.0.extend(local.0);
            }
            return Ok(current);
        }
    }
    let (v, err) = numeric?;
    let result = Expr::float(v);
    Steps::record(
        &mut log,
        "numerical integration",
        &simplify(expr),
        &result,
        Some(format!(
            "no closed form for every stage, so nested adaptive Gauss–Kronrod quadrature gives ≈ {} with estimated error ≤ {}",
            format_f64(v, 10),
            format_f64(err.max(f64::MIN_POSITIVE), 2)
        )),
    );
    Ok(result)
}

fn contains_float(e: &Expr) -> bool {
    e.as_number().is_some_and(|n| !n.is_exact()) || e.children().iter().any(|c| contains_float(c))
}

/// Nested quadrature; bounds are evaluated with the outer variables' current values.
struct Nested<'a> {
    expr: &'a Expr,
    bounds: &'a [(&'a str, Expr, Expr)],
    env: BTreeMap<String, f64>,
    error: Option<MathError>,
    err_total: f64,
}

impl Nested<'_> {
    fn level(&mut self, k: usize) -> Option<f64> {
        let bounds = self.bounds;
        let (var, lo, hi) = &bounds[k];
        let lo = lo.eval_f64(&self.env).ok()?;
        let hi = hi.eval_f64(&self.env).ok()?;
        let var = var.to_string();
        let mut f = |t: f64| -> Option<f64> {
            self.env.insert(var.clone(), t);
            if k == 0 {
                self.expr.eval_f64(&self.env).ok().filter(|y| y.is_finite())
            } else {
                self.level(k - 1)
            }
        };
        match super::numeric::quad(&mut f, lo, hi, NESTED_TOL, NESTED_INTERVALS) {
            Ok((v, e)) => {
                self.err_total += e;
                Some(v)
            }
            Err(err) => {
                if self.error.is_none() {
                    self.error = Some(err);
                }
                None
            }
        }
    }
}

fn nested_numeric(expr: &Expr, bounds: &[(&str, Expr, Expr)]) -> MathResult<(f64, f64)> {
    let vars: Vec<&str> = bounds.iter().map(|(v, _, _)| *v).collect();
    if let Some(s) = expr.free_symbols().iter().find(|s| !vars.contains(&&***s)) {
        return Err(MathError::NoClosedForm(format!(
            "this multiple integral (it depends on {s}, which has no value)"
        )));
    }
    let mut n = Nested {
        expr,
        bounds,
        env: BTreeMap::new(),
        error: None,
        err_total: 0.0,
    };
    match n.level(bounds.len() - 1) {
        Some(v) if v.is_finite() => Ok((v, n.err_total)),
        _ => Err(n
            .error
            .unwrap_or_else(|| MathError::Divergent("the multiple integral".into()))),
    }
}

/// Checks the curve and returns `r′(t)`.
fn velocity(curve: &[Expr], param: &str) -> MathResult<Vec<Expr>> {
    curve.iter().map(|c| diff(c, param, 1, None)).collect()
}

fn on_curve(e: &Expr, vars: &[&str], curve: &[Expr]) -> Expr {
    let pairs: Vec<(&str, Expr)> = vars.iter().copied().zip(curve.iter().cloned()).collect();
    subs_all(e, &pairs)
}

/// The line integral of a vector field, `∫_C F · dr = ∫_{t₀}^{t₁} F(r(t)) · r′(t) dt`, along
/// the curve `vars = curve(param)`. For a closed curve (`∮`) give a parametrisation with
/// `r(t₀) = r(t₁)`.
pub fn line_integral(
    field: &[Expr],
    vars: &[&str],
    curve: &[Expr],
    param: &str,
    t0: &Expr,
    t1: &Expr,
    steps: Option<&mut Steps>,
) -> MathResult<Expr> {
    if field.len() != vars.len() || curve.len() != vars.len() {
        return Err(MathError::WrongArity {
            function: "the line integral".into(),
            expected: vars.len().to_string(),
            got: field.len().min(curve.len()),
        });
    }
    let mut log = steps;
    let dr = velocity(curve, param)?;
    let integrand = simplify(&add(field
        .iter()
        .zip(&dr)
        .map(|(f, d)| mul(vec![on_curve(f, vars, curve), d.clone()]))
        .collect()));
    Steps::record(
        &mut log,
        "parametrise the curve",
        &Expr::new(ExprNode::Tuple(field.to_vec())),
        &definite_holder(&integrand, param, t0, t1),
        Some(format!(
            "r({param}) = ({}), r′({param}) = ({}); F·dr = F(r({param}))·r′({param}) d{param}",
            join(curve),
            join(&dr)
        )),
    );
    integrate_definite(&integrand, param, t0, t1, log)
}

/// The line integral of a scalar function, `∫_C f ds = ∫_{t₀}^{t₁} f(r(t)) |r′(t)| dt`.
pub fn line_integral_scalar(
    f: &Expr,
    vars: &[&str],
    curve: &[Expr],
    param: &str,
    t0: &Expr,
    t1: &Expr,
    steps: Option<&mut Steps>,
) -> MathResult<Expr> {
    if curve.len() != vars.len() {
        return Err(MathError::WrongArity {
            function: "the line integral".into(),
            expected: vars.len().to_string(),
            got: curve.len(),
        });
    }
    let mut log = steps;
    let dr = velocity(curve, param)?;
    let speed = pow(
        add(dr.iter().map(|d| pow(d.clone(), Expr::int(2))).collect()),
        Expr::rational(1, 2),
    );
    let integrand = simplify(&mul(vec![on_curve(f, vars, curve), speed.clone()]));
    Steps::record(
        &mut log,
        "parametrise the curve",
        f,
        &definite_holder(&integrand, param, t0, t1),
        Some(format!(
            "r({param}) = ({}), ds = |r′({param})| d{param} = {speed} d{param}",
            join(curve)
        )),
    );
    integrate_definite(&integrand, param, t0, t1, log)
}

fn join(v: &[Expr]) -> String {
    v.iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}
