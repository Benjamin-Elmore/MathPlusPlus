//! Partial derivatives and the tools built on them: the gradient and directional
//! derivative at a point, tangent planes, linear approximation, the second-derivative test,
//! Lagrange multipliers, implicit differentiation and related rates.

use std::cmp::Ordering;

use crate::calc::{derivative_holder, diff};
use crate::domains::solve::solve_system;
use crate::domains::util::{self, to_zero_form};
use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode};
use crate::simplify::{add, mul, pow, simplify};

use super::{dot, subs_all};

/// `∂ⁿf/∂…` taken in the order given: `[("x", 1), ("y", 2)]` is `∂³f/∂x∂y²`.
pub fn partial(f: &Expr, by: &[(&str, u32)]) -> MathResult<Expr> {
    let mut current = f.clone();
    for (v, k) in by {
        current = diff(&current, v, *k, None)?;
    }
    Ok(current)
}

fn check_point(vars: &[&str], point: &[Expr]) -> MathResult<()> {
    if vars.len() != point.len() {
        return Err(MathError::WrongArity {
            function: "the point".into(),
            expected: vars.len().to_string(),
            got: point.len(),
        });
    }
    Ok(())
}

/// A finite real value at the point, or an error naming the point.
fn at(e: &Expr, vars: &[&str], point: &[Expr], what: &str) -> MathResult<Expr> {
    let v = subs_all(e, vars, point);
    if v.is_undefined() || v.is_infinite() {
        return Err(MathError::Undefined(format!("{what} at this point")));
    }
    Ok(v)
}

/// `∇f` at a point.
pub fn gradient_at(f: &Expr, vars: &[&str], point: &[Expr]) -> MathResult<Vec<Expr>> {
    check_point(vars, point)?;
    vars.iter()
        .map(|v| at(&diff(f, v, 1, None)?, vars, point, "the gradient"))
        .collect()
}

/// `D_u f = ∇f · u/|u|` at a point.
pub fn directional_derivative_at(
    f: &Expr,
    vars: &[&str],
    point: &[Expr],
    direction: &[Expr],
) -> MathResult<Expr> {
    let g = gradient_at(f, vars, point)?;
    let n2 = simplify(&add(direction
        .iter()
        .map(|c| pow(c.clone(), Expr::int(2)))
        .collect()));
    if util::is_zero(&n2) {
        return Err(MathError::Undefined("a direction of length zero".into()));
    }
    let along = dot(&g, direction)?;
    Ok(simplify(&mul(vec![along, pow(n2, Expr::rational(-1, 2))])))
}

/// The tangent plane at a point: `z = f(a, b) + f_x(a, b)(x − a) + f_y(a, b)(y − b)` for
/// a graph `z = f(x, y)`, or `∇F(p) · (r − p) = 0` for a level surface `F(x, y, z) = c`
/// (given as an equation in three variables).
pub fn tangent_plane(f: &Expr, vars: &[&str], point: &[Expr]) -> MathResult<Expr> {
    check_point(vars, point)?;
    if let ExprNode::Relation(..) = f.node() {
        let big_f = to_zero_form(f);
        let level = at(&big_f, vars, point, "the surface")?;
        if !util::is_zero(&level) {
            return Err(MathError::Domain {
                function: "the tangent plane".into(),
                detail: "the point is not on the surface".into(),
            });
        }
        let g = gradient_at(&big_f, vars, point)?;
        if g.iter().all(util::is_zero) {
            return Err(MathError::Undefined(
                "the tangent plane (the gradient is zero at this point)".into(),
            ));
        }
        let offsets: Vec<Expr> = vars
            .iter()
            .zip(point)
            .map(|(v, p)| Expr::sym(v) - p.clone())
            .collect();
        let lhs = simplify(&crate::expand::expand(&dot(&g, &offsets)?)?);
        return Ok(Expr::equation(lhs, Expr::zero()));
    }
    let l = linear_part(f, vars, point)?;
    Ok(Expr::equation(Expr::sym("z"), l))
}

fn linear_part(f: &Expr, vars: &[&str], point: &[Expr]) -> MathResult<Expr> {
    let f0 = at(f, vars, point, "the function")?;
    let g = gradient_at(f, vars, point)?;
    let mut terms = vec![f0];
    for ((v, p), gi) in vars.iter().zip(point).zip(g) {
        terms.push(mul(vec![gi, Expr::sym(v) - p.clone()]));
    }
    Ok(simplify(&crate::expand::expand(&add(terms))?))
}

/// The linear approximation `L(x, y) = f(a, b) + f_x(a, b)(x − a) + f_y(a, b)(y − b)`.
pub fn linear_approximation(f: &Expr, vars: &[&str], point: &[Expr]) -> MathResult<Expr> {
    check_point(vars, point)?;
    let l = linear_part(f, vars, point)?;
    let head = Expr::apply("L", vars.iter().map(|v| Expr::sym(v)).collect());
    Ok(Expr::equation(head, l))
}

/// A critical point of `f(x, y)` classified by the second-derivative test.
#[derive(Clone, Debug, PartialEq)]
pub struct Classified {
    /// `(a, b)`.
    pub point: Vec<Expr>,
    /// `f(a, b)`.
    pub value: Expr,
    /// `D = f_xx f_yy − f_xy²` at the point.
    pub d: Expr,
    /// `f_xx` at the point.
    pub fxx: Expr,
    /// "local minimum", "local maximum", "saddle point" or "inconclusive (D = 0)".
    pub kind: &'static str,
}

/// Finds the critical points of `f(x, y)` (`f_x = f_y = 0`) and classifies each with
/// `D = f_xx f_yy − f_xy²`.
pub fn second_derivative_test(f: &Expr, x: &str, y: &str) -> MathResult<Vec<Classified>> {
    let fx = diff(f, x, 1, None)?;
    let fy = diff(f, y, 1, None)?;
    let fxx = diff(&fx, x, 1, None)?;
    let fyy = diff(&fy, y, 1, None)?;
    let fxy = diff(&fx, y, 1, None)?;
    let d = simplify(&(fxx.clone() * fyy - fxy.pow(2)));
    let vars = [x.to_string(), y.to_string()];
    let mut ctx = crate::domains::Ctx::new();
    let sol = solve_system(&[fx, fy], &vars, &mut ctx)?;
    if !sol.free.is_empty() {
        return Err(MathError::Unsupported(
            "the second-derivative test when the critical points form a curve".into(),
        ));
    }
    let names = [x, y];
    let mut out = Vec::new();
    for a in &sol.assignments {
        let point: Option<Vec<Expr>> = vars
            .iter()
            .map(|v| a.iter().find(|(n, _)| n == v).map(|(_, e)| simplify(e)))
            .collect();
        let Some(point) = point else { continue };
        if point.iter().any(|p| util::numeric(p).is_none()) {
            continue; // complex
        }
        let dv = subs_all(&d, &names, &point);
        let fxxv = subs_all(&fxx, &names, &point);
        let kind = match util::sign(&dv) {
            Some(Ordering::Less) => "saddle point",
            Some(Ordering::Equal) => "inconclusive (D = 0)",
            Some(Ordering::Greater) => match util::sign(&fxxv) {
                Some(Ordering::Greater) => "local minimum",
                Some(Ordering::Less) => "local maximum",
                _ => "inconclusive (D = 0)",
            },
            None => {
                return Err(MathError::Unsupported(format!(
                    "deciding the sign of D = {dv}"
                )))
            }
        };
        out.push(Classified {
            value: subs_all(f, &names, &point),
            point,
            d: dv,
            fxx: fxxv,
            kind,
        });
    }
    out.sort_by(|p, q| {
        let key = |c: &Classified| {
            c.point
                .iter()
                .map(|e| util::numeric(e).unwrap_or(f64::NAN))
                .collect::<Vec<_>>()
        };
        key(p).partial_cmp(&key(q)).unwrap_or(Ordering::Equal)
    });
    Ok(out)
}

/// A candidate point from Lagrange multipliers with the value of `f` there.
#[derive(Clone, Debug, PartialEq)]
pub struct LagrangePoint {
    pub point: Vec<Expr>,
    pub value: Expr,
}

/// Lagrange multipliers: solves `∇f = λ∇g (+ μ∇h)` with the constraints, and returns the
/// candidate points with `f` there, sorted by value (the first is the minimum and the last
/// the maximum when the constraint set is closed and bounded).
pub fn lagrange(f: &Expr, constraints: &[Expr], vars: &[&str]) -> MathResult<Vec<LagrangePoint>> {
    if constraints.is_empty() || constraints.len() >= vars.len() {
        return Err(MathError::WrongArity {
            function: "Lagrange multipliers".into(),
            expected: format!("1 to {} constraints", vars.len().saturating_sub(1)),
            got: constraints.len(),
        });
    }
    let gs: Vec<Expr> = constraints.iter().map(to_zero_form).collect();
    let coords: Vec<String> = vars.iter().map(|v| v.to_string()).collect();
    let mut ctx = crate::domains::Ctx::new();
    // ∇f, ∇g, … are dependent exactly when the minors of the matrix of gradients vanish;
    // solving that with the constraints avoids the multipliers altogether.
    if let Some(minors) = rank_conditions(f, &gs, vars)? {
        let mut eqs = gs.clone();
        eqs.extend(minors);
        if let Ok(sol) = solve_system(&eqs, &coords, &mut ctx) {
            if sol.free.is_empty() {
                return Ok(candidates(f, vars, &sol.assignments));
            }
        }
    }
    let multipliers: Vec<String> = ["λ", "μ", "ν"]
        .iter()
        .take(constraints.len())
        .map(|s| s.to_string())
        .collect();
    let mut eqs = Vec::new();
    for v in vars {
        let mut rhs = Vec::new();
        for (g, m) in gs.iter().zip(&multipliers) {
            rhs.push(mul(vec![Expr::sym(m), diff(g, v, 1, None)?]));
        }
        eqs.push(simplify(&(diff(f, v, 1, None)? - add(rhs))));
    }
    eqs.extend(gs.iter().cloned());
    let mut unknowns = coords;
    unknowns.extend(multipliers);
    let sol = solve_system(&eqs, &unknowns, &mut ctx)?;
    // A multiplier left free does not matter; a free coordinate means a curve of points.
    if sol.free.iter().any(|f| vars.contains(&f.as_str())) {
        return Err(MathError::Unsupported(
            "Lagrange multipliers with infinitely many candidate points".into(),
        ));
    }
    Ok(candidates(f, vars, &sol.assignments))
}

/// The conditions for `∇f` to lie in the span of the constraint gradients: every 2×2
/// minor for one constraint, the determinant for two constraints in three variables.
fn rank_conditions(f: &Expr, gs: &[Expr], vars: &[&str]) -> MathResult<Option<Vec<Expr>>> {
    let grad =
        |e: &Expr| -> MathResult<Vec<Expr>> { vars.iter().map(|v| diff(e, v, 1, None)).collect() };
    let gf = grad(f)?;
    match gs {
        [g] => {
            let gg = grad(g)?;
            let mut out = Vec::new();
            for i in 0..vars.len() {
                for j in i + 1..vars.len() {
                    out.push(simplify(
                        &(gf[i].clone() * gg[j].clone() - gf[j].clone() * gg[i].clone()),
                    ));
                }
            }
            Ok(Some(out))
        }
        [g, h] if vars.len() == 3 => {
            let (a, b) = (grad(g)?, grad(h)?);
            let c = super::cross(&a, &b)?;
            Ok(Some(vec![super::dot(&gf, &c)?]))
        }
        _ => Ok(None),
    }
}

/// The real points of the solutions with `f` there, without repeats, sorted by value.
fn candidates(f: &Expr, vars: &[&str], assignments: &[Vec<(String, Expr)>]) -> Vec<LagrangePoint> {
    let mut out: Vec<LagrangePoint> = Vec::new();
    for a in assignments {
        let point: Option<Vec<Expr>> = vars
            .iter()
            .map(|v| a.iter().find(|(n, _)| n == v).map(|(_, e)| simplify(e)))
            .collect();
        let Some(point) = point else { continue };
        if point.iter().any(|p| util::numeric(p).is_none()) {
            continue;
        }
        if out
            .iter()
            .any(|o| o.point.iter().zip(&point).all(|(p, q)| util::equal(p, q)))
        {
            continue;
        }
        out.push(LagrangePoint {
            value: subs_all(f, vars, &point),
            point,
        });
    }
    out.sort_by(|p, q| util::compare(&p.value, &q.value).unwrap_or(Ordering::Equal));
    out
}

/// `dy/dx` for the curve `F(x, y) = 0` (given as an equation): `−F_x/F_y`.
pub fn implicit_derivative(relation: &Expr, x: &str, y: &str) -> MathResult<Expr> {
    let big_f = to_zero_form(relation);
    let fx = diff(&big_f, x, 1, None)?;
    let fy = diff(&big_f, y, 1, None)?;
    if util::is_zero(&fy) {
        return Err(MathError::Undefined(format!(
            "d{y}/d{x} (∂F/∂{y} is zero, so {y} is not a function of {x} here)"
        )));
    }
    Ok(simplify(&(-(fx / fy))))
}

/// The rate of `v` with respect to `t`, as the unevaluated `d v/d t`.
pub fn rate(v: &str, t: &str) -> Expr {
    derivative_holder(&Expr::sym(v), t)
}

/// `d/dt` of a relation between quantities that all change with `t`:
/// `Σ ∂F/∂v · dv/dt = 0` (the related-rates equation).
pub fn related_rates(relation: &Expr, vars: &[&str], t: &str) -> MathResult<Expr> {
    let (lhs, rhs) = match relation.node() {
        ExprNode::Relation(_, l, r) => (l.clone(), r.clone()),
        _ => (relation.clone(), Expr::zero()),
    };
    let side = |e: &Expr| -> MathResult<Expr> {
        let mut terms = Vec::new();
        for v in vars {
            let p = diff(e, v, 1, None)?;
            if !util::is_zero(&p) {
                terms.push(mul(vec![p, rate(v, t)]));
            }
        }
        Ok(simplify(&add(terms)))
    };
    Ok(Expr::equation(side(&lhs)?, side(&rhs)?))
}

/// Solves the related-rates equation for the one rate not given. `values` gives the
/// quantities at the instant (one may be missing if the relation fixes it uniquely) and
/// `rates` the known rates. Returns the unknown quantity's name and its rate.
pub fn solve_rates(
    relation: &Expr,
    vars: &[&str],
    t: &str,
    values: &[(String, Expr)],
    rates: &[(String, Expr)],
) -> MathResult<(String, Expr)> {
    let unknown: Vec<&&str> = vars
        .iter()
        .filter(|v| !rates.iter().any(|(n, _)| n == **v))
        .collect();
    let [target] = unknown.as_slice() else {
        return Err(MathError::Undefined(format!(
            "which rate to find (give every rate but one; unknown: {})",
            unknown
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    };
    let mut values = values.to_vec();
    let missing: Vec<&&str> = vars
        .iter()
        .filter(|v| !values.iter().any(|(n, _)| n == **v))
        .collect();
    match missing.as_slice() {
        [] => {}
        [m] => {
            let given: Vec<&str> = values.iter().map(|(n, _)| n.as_str()).collect();
            let vals: Vec<Expr> = values.iter().map(|(_, e)| e.clone()).collect();
            let fixed = subs_all(&to_zero_form(relation), &given, &vals);
            let mut ctx = crate::domains::Ctx::new();
            let sols = crate::domains::solve::solve(&fixed, m, None, &mut ctx)?;
            let real: Vec<Expr> = sols
                .values()
                .unwrap_or(&[])
                .iter()
                .filter(|v| util::numeric(v).is_some())
                .cloned()
                .collect();
            match real.as_slice() {
                [one] => values.push((m.to_string(), one.clone())),
                _ => {
                    return Err(MathError::Undefined(format!(
                        "the value of {m} at this instant ({} values fit the relation; give it)",
                        real.len()
                    )))
                }
            }
        }
        many => {
            return Err(MathError::Undefined(format!(
                "the values of {} at this instant",
                many.iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )))
        }
    }
    let eq = to_zero_form(&related_rates(relation, vars, t)?);
    // Put the rates in as plain symbols, then the values.
    let unknown_sym = format!("__rate_{target}");
    let mut e = eq;
    for v in vars {
        let r = rate(v, t);
        let replacement = match rates.iter().find(|(n, _)| n == v) {
            Some((_, val)) => val.clone(),
            None => Expr::sym(&unknown_sym),
        };
        e = e.subs_expr(&r, &replacement);
    }
    let names: Vec<&str> = values.iter().map(|(n, _)| n.as_str()).collect();
    let vals: Vec<Expr> = values.iter().map(|(_, e)| e.clone()).collect();
    let e = subs_all(&e, &names, &vals);
    let mut ctx = crate::domains::Ctx::new();
    let sol = crate::domains::solve::solve(&e, &unknown_sym, None, &mut ctx)?;
    match sol.values() {
        Some([one]) => Ok((target.to_string(), simplify(one))),
        _ => Err(MathError::Undefined(format!(
            "d{target}/d{t} (the rates equation does not fix it at this instant)"
        ))),
    }
}
