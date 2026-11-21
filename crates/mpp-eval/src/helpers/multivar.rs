//! The multivariable and vector calculus helpers as holders, answered by
//! [`mpp_core::tools::multivar`].
//!
//! Points are written as a subscript `(x, y) = (1, 2)`; integration ranges as
//! `r \in [0, 2]` items, outermost first, either as arguments or in the subscript:
//!
//! - `\operatorname{tangent}_{(x,y)=(1,2)} f` (tangent plane; a level surface `F = c` gives
//!   `∇F · (r − p) = 0`), `\operatorname{linearisation}_{(x,y)=(3,4)} f`,
//!   `\operatorname{grad}_{(x,y)=(1,2)} f`, `\operatorname{dirderiv}_{(x,y)=(1,2)}(f, (3, 4))`;
//! - `\operatorname{classify} f` (second-derivative test), `\operatorname{lagrange}(f, g = c)`;
//! - `\operatorname{polar}(f, \theta \in [0, 2\pi], r \in [0, 2])`, `cylindrical`,
//!   `spherical`, `\operatorname{changevars}(f, (x, y) = (u + v, u - v), u \in …, v \in …)`;
//! - `\operatorname{lineintegral}(F, (\cos t, \sin t), t \in [0, 2\pi])` (a scalar first
//!   argument integrates with respect to arc length), `surfaceintegral(f, Φ, u \in …, v \in …)`,
//!   `flux(F, Φ, u \in …, v \in …)`;
//! - `green(F, x \in …, y \in …)` or `green(F, (x, y) = Φ(u, v), u \in …, v \in …)`,
//!   `stokes(F, Φ, u \in …, v \in …)`, `divthm(F, x \in …, y \in …, z \in …)` (or in
//!   cylindrical `r, θ, z` or spherical `ρ, φ, θ` ranges);
//! - `\operatorname{implicit}_x (x^2 + y^2 = 25)` (dy/dx),
//!   `\operatorname{rates}_t(x^2 + y^2 = 25, x = 3, y = 4, dx = 2)` (`dx` is dx/dt);
//! - `\operatorname{laplace} f` (t → s), `\operatorname{invlaplace} F` (s → t),
//!   `\operatorname{lsolve}((a_0, a_1, a_2), g, (y_0, y_1))` for `a₂y″ + a₁y′ + a₀y = g(t)`.

use mpp_core::calc::{line_integral, line_integral_scalar};
use mpp_core::tools::multivar::{
    self as mv, change_of_variables, check_divergence_theorem, check_greens_theorem,
    check_stokes_theorem, components, flux_integral, implicit_derivative, integrate_in, lagrange,
    linear_approximation, related_rates, second_derivative_test, solve_rates, surface_integral,
    tangent_plane, tuple, Coordinates, Patch, Solid, TheoremCheck,
};
use mpp_core::tools::table;
use mpp_core::tools_b::ode::laplace as lt;
use mpp_core::{Expr, ExprNode, MathError, MathResult, Name, RelOp};

use crate::holders::Ctx;

const OPS: &[&str] = &[
    "tangent",
    "tangentplane",
    "linearisation",
    "linearization",
    "linapprox",
    "grad",
    "dirderiv",
    "classify",
    "lagrange",
    "polar",
    "cylindrical",
    "spherical",
    "changevars",
    "lineintegral",
    "surfaceintegral",
    "flux",
    "green",
    "stokes",
    "divthm",
    "implicit",
    "rates",
    "laplace",
    "invlaplace",
    "lsolve",
];

/// Operators that only exist here (the others share a name with a one-variable helper or
/// a calculus operator and are taken only in their multivariable form).
fn own(name: &str) -> bool {
    OPS.contains(&name) && !matches!(name, "tangent" | "linearisation" | "linearization" | "grad")
}

/// `(name, sub, sup, items)` for either form, when it is one of these helpers.
fn parts(e: &Expr) -> Option<(String, Expr, Expr, Vec<Expr>)> {
    let ExprNode::Apply(name, args) = e.node() else {
        return None;
    };
    let empty = || Expr::new(ExprNode::Tuple(Vec::new()));
    if &**name == "operatorname" {
        let [which, sub, sup, body] = args.as_slice() else {
            return None;
        };
        let n = which.as_sym()?;
        if !OPS.contains(&n) {
            return None;
        }
        let ExprNode::Tuple(items) = body.node() else {
            return None;
        };
        // The shared names are ours only with a point (x, y) = (a, b) below.
        if !own(n) && tuple_point(sub).is_none() {
            return None;
        }
        return Some((n.to_string(), sub.clone(), sup.clone(), items.clone()));
    }
    if own(name) {
        return Some((name.to_string(), empty(), empty(), args.clone()));
    }
    None
}

/// `(x, y) = (1, 2)`: the variables and the point.
fn tuple_point(sub: &Expr) -> Option<(Vec<String>, Vec<Expr>)> {
    let ExprNode::Relation(RelOp::Eq, l, r) = sub.node() else {
        return None;
    };
    let (ExprNode::Tuple(vs), ExprNode::Tuple(ps)) = (l.node(), r.node()) else {
        return None;
    };
    let names: Option<Vec<String>> = vs.iter().map(|v| v.as_sym().map(str::to_string)).collect();
    let names = names?;
    (names.len() == ps.len()).then(|| (names, ps.clone()))
}

/// `v \in [a, b]` (lowered as `in(v, [a, b])`).
fn range_of(e: &Expr) -> Option<(String, Expr, Expr)> {
    let ExprNode::Apply(name, args) = e.node() else {
        return None;
    };
    if &**name != "in" {
        return None;
    }
    let [v, iv] = args.as_slice() else {
        return None;
    };
    let ExprNode::Interval { lo, hi, .. } = iv.node() else {
        return None;
    };
    Some((v.as_sym()?.to_string(), lo.clone(), hi.clone()))
}

/// The ranges among `items` and in the subscript, outermost first.
fn ranges(sub: &Expr, items: &[Expr]) -> Vec<(String, Expr, Expr)> {
    let mut out: Vec<(String, Expr, Expr)> = Vec::new();
    let from_sub: Vec<Expr> = match sub.node() {
        ExprNode::Tuple(xs) => xs.clone(),
        _ => vec![sub.clone()],
    };
    for e in from_sub.iter().chain(items) {
        if let Some(r) = range_of(e) {
            out.push(r);
        }
    }
    out
}

/// Names these helpers bind: range variables, the point's variables, `x, y, z` of fields
/// and the subscript variable of `implicit_x` and `rates_t`.
pub fn bound_names(name: &str, args: &[Expr]) -> Vec<Name> {
    let e = Expr::new(ExprNode::Apply(Name::from(name), args.to_vec()));
    let Some((op, sub, _, items)) = parts(&e) else {
        return Vec::new();
    };
    let mut out: Vec<Name> = Vec::new();
    let mut add = |s: &str| {
        if !out.iter().any(|o| &**o == s) {
            out.push(Name::from(s));
        }
    };
    for (v, _, _) in ranges(&sub, &items) {
        add(&v);
    }
    if let Some((vs, _)) = tuple_point(&sub) {
        vs.iter().for_each(|v| add(v));
    }
    if let Some(s) = sub.as_sym() {
        add(s);
    }
    for it in &items {
        if let ExprNode::Relation(RelOp::Eq, l, _) = it.node() {
            if let ExprNode::Tuple(vs) = l.node() {
                vs.iter().filter_map(Expr::as_sym).for_each(&mut add);
            }
        }
        if let ExprNode::Apply(_, call) = it.node() {
            call.iter().filter_map(Expr::as_sym).for_each(&mut add);
        }
    }
    if matches!(
        op.as_str(),
        "lineintegral"
            | "surfaceintegral"
            | "flux"
            | "green"
            | "stokes"
            | "divthm"
            | "classify"
            | "lagrange"
            | "rates"
            | "implicit"
    ) {
        for v in ["x", "y", "z"] {
            add(v);
        }
    }
    if matches!(op.as_str(), "laplace" | "lsolve") {
        add("t");
    }
    if op == "invlaplace" {
        add("s");
    }
    out
}

/// Evaluates one of these helpers; `None` for other nodes.
pub fn evaluate(e: &Expr, ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
    let (op, sub, _, mut items) = parts(e)?;
    // The multivariable helpers work on the function itself.
    if let Some(first) = items.first_mut() {
        *first = super::unwrap_where_defined(first).0;
    }
    Some(run(&op, &sub, &items, ctx))
}

fn arity(op: &str, expected: &str, got: usize) -> MathError {
    MathError::WrongArity {
        function: op.into(),
        expected: expected.into(),
        got,
    }
}

/// The free variables of `e`, `x, y, z` first.
fn vars_of(e: &[&Expr], n: usize) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for x in e {
        for s in x.free_symbols() {
            if !found.iter().any(|f| f == &*s) {
                found.push(s.to_string());
            }
        }
    }
    let mut out: Vec<String> = ["x", "y", "z"]
        .iter()
        .take(n)
        .map(|s| s.to_string())
        .collect();
    if found.iter().all(|f| out.contains(f)) {
        return out;
    }
    found.sort();
    out = found;
    out.truncate(n);
    out
}

fn first<'a>(op: &str, items: &'a [Expr]) -> MathResult<&'a Expr> {
    items.first().ok_or_else(|| arity(op, "at least 1", 0))
}

fn check_table(boundary: &str, interior: &str, c: &TheoremCheck) -> Expr {
    table::table(vec![
        (boundary.into(), c.boundary.clone()),
        (interior.into(), c.interior.clone()),
        (
            "the two sides agree".into(),
            Expr::new(ExprNode::Bool(c.agree)),
        ),
    ])
}

fn run(op: &str, sub: &Expr, items: &[Expr], ctx: &Ctx<'_>) -> MathResult<Expr> {
    match op {
        "tangent" | "tangentplane" | "linearisation" | "linearization" | "linapprox" | "grad"
        | "dirderiv" => {
            let (names, point) = tuple_point(sub).ok_or_else(|| {
                MathError::Undefined(
                    "the point (write it below, as in tangentplane_{(x,y)=(1,2)})".into(),
                )
            })?;
            let vars: Vec<&str> = names.iter().map(String::as_str).collect();
            let f = first(op, items)?;
            match op {
                "tangent" | "tangentplane" => tangent_plane(f, &vars, &point),
                "grad" => mv::gradient_at(f, &vars, &point).map(tuple),
                "dirderiv" => {
                    let dir = items
                        .get(1)
                        .map(components)
                        .ok_or_else(|| arity(op, "2", 1))?;
                    mv::directional_derivative_at(f, &vars, &point, &dir)
                }
                _ => linear_approximation(f, &vars, &point),
            }
        }
        "classify" => {
            let f = first(op, items)?;
            let vars = vars_of(&[f], 2);
            let [x, y] = vars.as_slice() else {
                return Err(MathError::Undefined(
                    "the two variables of the function".into(),
                ));
            };
            let found = second_derivative_test(f, x, y)?;
            if found.is_empty() {
                ctx.note("f has no critical points");
            }
            Ok(table::table(
                found
                    .into_iter()
                    .map(|c| {
                        (
                            format!("{} at ({}, {})", c.kind, c.point[0], c.point[1]),
                            c.value,
                        )
                    })
                    .collect(),
            ))
        }
        "lagrange" => {
            let f = first(op, items)?;
            let constraints: Vec<Expr> = items[1..].to_vec();
            let mut all: Vec<&Expr> = vec![f];
            all.extend(constraints.iter());
            let names = vars_of(&all, 3);
            let names: Vec<String> = names
                .into_iter()
                .filter(|v| all.iter().any(|e| e.contains_sym(v)))
                .collect();
            let vars: Vec<&str> = names.iter().map(String::as_str).collect();
            let pts = lagrange(f, &constraints, &vars)?;
            let mut rows: Vec<(String, Expr)> = pts
                .iter()
                .map(|p| {
                    let shown: Vec<String> = p.point.iter().map(|e| e.to_string()).collect();
                    (format!("f({})", shown.join(", ")), p.value.clone())
                })
                .collect();
            if let (Some(lo), Some(hi)) = (pts.first(), pts.last()) {
                rows.push(("largest value".into(), hi.value.clone()));
                rows.push(("smallest value".into(), lo.value.clone()));
                ctx.note("on a closed and bounded constraint set the largest and smallest values are the absolute maximum and minimum");
            }
            Ok(table::table(rows))
        }
        "polar" | "cylindrical" | "spherical" => {
            let f = first(op, items)?;
            let coords = match op {
                "polar" => Coordinates::Polar,
                "cylindrical" => Coordinates::Cylindrical,
                _ => Coordinates::Spherical,
            };
            integrate_in(f, coords, &ranges(sub, items))
        }
        "changevars" => {
            let f = first(op, items)?;
            let (names, map) = items
                .iter()
                .find_map(tuple_point)
                .ok_or_else(|| MathError::Undefined("the substitution (x, y) = (…, …)".into()))?;
            let vars: Vec<&str> = names.iter().map(String::as_str).collect();
            let (value, jac) = change_of_variables(f, &vars, &map, &ranges(sub, items))?;
            ctx.note(format!("the Jacobian factor is |∂(x, y)/∂(u, v)| = {jac}"));
            Ok(value)
        }
        "lineintegral" => {
            let [field, curve, ..] = items else {
                return Err(arity(op, "3", items.len()));
            };
            let curve = components(curve);
            let r = ranges(sub, items);
            let [(t, a, b)] = r.as_slice() else {
                return Err(MathError::Undefined(
                    "the parameter range, as in t \\in [0, 1]".into(),
                ));
            };
            let names = vars_of(&[], curve.len());
            let vars: Vec<&str> = names.iter().map(String::as_str).collect();
            match field.node() {
                ExprNode::Tuple(f) => line_integral(f, &vars, &curve, t, a, b, None),
                _ => line_integral_scalar(field, &vars, &curve, t, a, b, None),
            }
        }
        "surfaceintegral" | "flux" | "stokes" => {
            let [field, map, ..] = items else {
                return Err(arity(op, "4", items.len()));
            };
            let patch = patch(components(map), sub, items)?;
            let names = vars_of(&[], 3);
            let vars: Vec<&str> = names.iter().map(String::as_str).collect();
            match op {
                "surfaceintegral" => surface_integral(field, &vars, &patch),
                "flux" => flux_integral(&components(field), &vars, &patch),
                _ => check_stokes_theorem(&components(field), &vars, &patch)
                    .map(|c| check_table("∮ F · dr", "∬ (∇ × F) · dS", &c)),
            }
        }
        "green" => {
            let field = first(op, items)?;
            let map = items.iter().find_map(tuple_point).map(|(_, m)| m);
            let r = ranges(sub, items);
            let map = map.unwrap_or_else(|| r.iter().map(|(v, _, _)| Expr::sym(v)).collect());
            let region = patch(map, sub, items)?;
            let vars = ["x", "y"];
            let c = check_greens_theorem(&components(field), &vars, &region)?;
            Ok(check_table("∮ P dx + Q dy", "∬ (∂Q/∂x − ∂P/∂y) dA", &c))
        }
        "divthm" => {
            let field = first(op, items)?;
            let bounds = ranges(sub, items);
            let roles: Vec<&str> = bounds.iter().map(|(v, _, _)| v.as_str()).collect();
            let coords = if roles.iter().any(|v| matches!(*v, "ρ" | "rho")) {
                Some(Coordinates::Spherical)
            } else if roles.contains(&"r") {
                Some(Coordinates::Cylindrical)
            } else {
                None
            };
            let solid = Solid { coords, bounds };
            let c = check_divergence_theorem(&components(field), &["x", "y", "z"], &solid)?;
            Ok(check_table("∯ F · dS", "∭ ∇ · F dV", &c))
        }
        "implicit" => {
            let rel = first(op, items)?;
            let x = sub.as_sym().unwrap_or("x").to_string();
            let others: Vec<String> = rel
                .free_symbols()
                .iter()
                .map(|s| s.to_string())
                .filter(|s| *s != x)
                .collect();
            let [y] = others.as_slice() else {
                return Err(MathError::Undefined(
                    "the dependent variable (the relation must have exactly two variables)".into(),
                ));
            };
            let d = implicit_derivative(rel, &x, y)?;
            let lhs = mpp_core::calc::derivative_holder(&Expr::sym(y), &x);
            Ok(Expr::equation(lhs, d))
        }
        "rates" => rates(sub, items),
        "laplace" => lt::transform(first(op, items)?, "t", "s"),
        "invlaplace" => lt::inverse(first(op, items)?, "s", "t"),
        "lsolve" => {
            let [coeffs, g, init] = items else {
                return Err(arity(op, "3", items.len()));
            };
            let (big_y, y) =
                lt::solve_coefficients(&components(coeffs), g, &components(init), "t", "s")?;
            ctx.log(
                "Laplace transform of the equation",
                &Expr::apply("lsolve", items.to_vec()),
                &Expr::equation(Expr::apply("Y", vec![Expr::sym("s")]), big_y.clone()),
                Some("L{y′} = sY − y(0), L{y″} = s²Y − s·y(0) − y′(0)".into()),
            );
            ctx.log("inverse transform by partial fractions", &big_y, &y, None);
            Ok(Expr::equation(Expr::apply("y", vec![Expr::sym("t")]), y))
        }
        _ => Err(MathError::UnknownFunction(op.into())),
    }
}

/// A patch from a map and the first two ranges.
fn patch(map: Vec<Expr>, sub: &Expr, items: &[Expr]) -> MathResult<Patch> {
    let r = ranges(sub, items);
    let [u, v] = r.as_slice() else {
        return Err(MathError::Undefined(
            "the two parameter ranges, as in u \\in [0, 1], v \\in [0, 2\\pi]".into(),
        ));
    };
    Ok(Patch {
        map,
        u: u.clone(),
        v: v.clone(),
    })
}

/// `rates_t(relation, x = 3, y = 4, dx = 2)`: the differentiated relation, or the one
/// unknown rate when the others and the values are given.
fn rates(sub: &Expr, items: &[Expr]) -> MathResult<Expr> {
    let t = sub.as_sym().unwrap_or("t").to_string();
    let rel = items
        .first()
        .ok_or_else(|| arity("rates", "at least 1", 0))?;
    let mut names: Vec<String> = rel
        .free_symbols()
        .iter()
        .map(|s| s.to_string())
        .filter(|s| *s != t)
        .collect();
    names.sort();
    let vars: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut values = Vec::new();
    let mut known = Vec::new();
    for it in &items[1..] {
        let ExprNode::Relation(RelOp::Eq, l, r) = it.node() else {
            return Err(MathError::Domain {
                function: "rates".into(),
                detail: format!("expected a value such as x = 3 or dx = 2, not {it}"),
            });
        };
        match l.node() {
            ExprNode::Sym(v) => values.push((v.to_string(), r.clone())),
            // `dx` reads as d·x: the rate of x.
            ExprNode::Mul(fs) if fs.len() == 2 && fs[0].is_sym("d") => match fs[1].as_sym() {
                Some(v) => known.push((v.to_string(), r.clone())),
                None => return Err(MathError::Undefined(format!("the rate {l}"))),
            },
            _ => return Err(MathError::Undefined(format!("what {l} = {r} gives"))),
        }
    }
    if known.is_empty() {
        return related_rates(rel, &vars, &t);
    }
    let (name, value) = solve_rates(rel, &vars, &t, &values, &known)?;
    let lhs = mpp_core::calc::derivative_holder(&Expr::sym(&name), &t);
    Ok(Expr::equation(lhs, value))
}
