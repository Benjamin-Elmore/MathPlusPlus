//! Differential equations as operators: `dsolve` (also `solve` with a differential
//! equation, and systems), statements that are differential equations, and the PDE
//! helpers `heat`, `wave` and `laplacerect`, answered by [`mpp_core::ode`].

use mpp_core::ode::{self as core_ode, pde as core_pde, systems, Condition, Form, Options};
use mpp_core::{Expr, ExprNode, MathError, MathResult, RelOp};

use super::operator_args::*;
use crate::holders::Ctx;

/// `dⁿy/dxⁿ` of a name with no value (an unknown function) stays as written, so that a
/// differential equation reaches the solver intact instead of `y` being treated as a
/// constant with derivative 0.
pub(super) fn unknown_function_derivative(e: &Expr) -> Option<Expr> {
    let ExprNode::Derivative { expr, vars } = e.node() else {
        return None;
    };
    let name = expr.as_sym()?;
    if vars.iter().any(|(v, _)| &**v == name) {
        return None;
    }
    Some(e.clone())
}

/// True when `e` is a differential equation or a tuple of them.
pub(crate) fn is_differential_equation_or_system(e: &Expr) -> bool {
    match e.node() {
        ExprNode::Tuple(items) => {
            !items.is_empty() && items.iter().all(mpp_core::ode::is_differential_equation)
        }
        _ => mpp_core::ode::is_differential_equation(e),
    }
}

/// Solves a statement that is a differential equation (for the engine: an equation with
/// `y′`, `dy/dx` or `y″` in a query or an auto-evaluated block, with its conditions).
/// `None` when `e` is not a differential equation.
pub fn ode_statement(e: &Expr, conditions: &[Expr], ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
    if !is_differential_equation_or_system(e) {
        return None;
    }
    let mut args = vec![e.clone()];
    args.extend(conditions.iter().cloned());
    let call = Call {
        name: "dsolve".into(),
        sub: None,
        sup: None,
        args,
    };
    Some(dsolve(&call, ctx))
}

/// An argument of `dsolve` saying the solution will define the unknowns: an
/// explicit solution, or else a numerical one usable at any point (never a
/// truncated series or an implicit relation).
pub(crate) const FUNCTION_MARKER: &str = "#as function";

pub(super) const NUMERIC_NOTE: &str = "no closed form was found, so the solution is numerical (RK45, tolerance 10⁻¹²), computed wherever it is used";

/// `(y, t)` when the second and third arguments of `dsolve(eq, y, t, …)` name the
/// unknown of the equation and its independent variable.
pub(crate) fn named_unknown(eq: &Expr, args: &[Expr]) -> Option<(String, String)> {
    let y = args.get(1)?.as_sym()?;
    let x = args.get(2)?.as_sym()?;
    if y == x || is_function_marker(&args[1]) || is_function_marker(&args[2]) {
        return None;
    }
    let found = core_ode::parse(eq, None, None).ok()?;
    (found.y == y).then(|| (y.to_string(), x.to_string()))
}

pub(crate) fn is_function_marker(e: &Expr) -> bool {
    e.as_sym() == Some(FUNCTION_MARKER)
}

/// Reads conditions: `y(a) = v`, `y′(a) = v` (as `subs` holders), or a tuple
/// `(x₀, y₀, y′₀, …)` of starting values.
pub(super) fn conditions(args: &[Expr], y: &str) -> MathResult<Vec<Condition>> {
    let mut out = Vec::new();
    for a in args {
        match a.node() {
            ExprNode::Relation(RelOp::Eq, l, r) => {
                if let Some(c) = condition_of(l, r, y) {
                    out.push(c);
                } else if l.is_constant() && r.is_constant() {
                    return Err(MathError::Domain {
                        function: "dsolve".into(),
                        detail: format!(
                            "the condition {a} was read as numbers only; write the starting values as a tuple (x₀, y₀, y′₀, …)"
                        ),
                    });
                }
            }
            // The starting point must be a number; the starting values may be
            // names (`(0, N_0)` gives N(0) = N_0).
            ExprNode::Tuple(items) if items.len() >= 2 && items[0].is_constant() => {
                for (k, v) in items[1..].iter().enumerate() {
                    out.push(Condition::derivative(k as u32, items[0].clone(), v.clone()));
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

pub(super) fn condition_of(l: &Expr, r: &Expr, y: &str) -> Option<Condition> {
    let ExprNode::Apply(f, args) = l.node() else {
        return None;
    };
    // y(a) = v
    if &**f == y && args.len() == 1 && args[0].is_constant() {
        return Some(Condition::value(args[0].clone(), r.clone()));
    }
    // y′(a) = v, lowered as subs(dᵏ/dxᵏ y(x), x = a)
    if &**f == "subs" && args.len() == 2 {
        let ExprNode::Derivative { expr, vars } = args[0].node() else {
            return None;
        };
        let inner_ok = expr.is_sym(y) || matches!(expr.node(), ExprNode::Apply(g, _) if &**g == y);
        let ExprNode::Relation(RelOp::Eq, _, at) = args[1].node() else {
            return None;
        };
        let k: u32 = vars.iter().map(|(_, k)| *k).sum();
        return inner_ok.then(|| Condition::derivative(k, at.clone(), r.clone()));
    }
    // y′(a) typed as a call of the name `y′`.
    if args.len() == 1 && args[0].is_constant() {
        let base = f.trim_end_matches(['\'', '′']);
        if base == y && base.len() < f.len() {
            let k = (f.chars().count() - base.chars().count()) as u32;
            return Some(Condition::derivative(k, args[0].clone(), r.clone()));
        }
    }
    None
}

/// `\operatorname{dsolve}(equation, conditions…)` (also `solve` with a differential
/// equation, and a tuple of equations for a system).
pub(crate) fn dsolve(call: &Call, ctx: &Ctx<'_>) -> MathResult<Expr> {
    let Some(eq) = call.args.first() else {
        return Err(missing("dsolve", "a differential equation"));
    };
    if let ExprNode::Tuple(eqs) = eq.node() {
        return system(eqs, &call.args[1..], ctx);
    }
    // `dsolve(eq, y, t, …)`: the unknown and the independent variable named as the
    // second and third arguments (the textbook IVP form), so the answer is in t.
    let named = named_unknown(eq, &call.args);
    let (ny, nx) = match &named {
        Some((y, x)) => (Some(y.as_str()), Some(x.as_str())),
        None => (None, None),
    };
    let mut parsed = core_ode::parse(eq, ny, None)?;
    if let Some(x) = nx {
        // Primes are read as d/dx; with the variable named (and no other x in the
        // equation) they mean d/dt, so the answer is y(t).
        if parsed.x != x && !parsed.residual.contains_sym(&parsed.x) {
            parsed.x = x.to_string();
        }
    }
    let skip = if named.is_some() { 3 } else { 1 };
    let rest: Vec<Expr> = call
        .args
        .iter()
        .skip(skip)
        .chain(call.sub.iter())
        .chain(call.sup.iter())
        .cloned()
        .collect();
    // Asked for a function to define (a statement whose solution later lines use):
    // an explicit solution, or else a numerical one usable at any point.
    let as_function = rest.iter().any(is_function_marker);
    let rest: Vec<Expr> = rest
        .into_iter()
        .filter(|a| !is_function_marker(a))
        .collect();
    let conds = conditions(&rest, &parsed.y)?;
    let mut options = Options {
        y: Some(parsed.y.clone()),
        x: named.as_ref().map(|_| parsed.x.clone()),
        ..Options::default()
    };
    let mut numeric_wanted = false;
    let mut end = None;
    for a in &rest {
        if let Some(w) = keyword(a) {
            if w == "numeric" {
                numeric_wanted = true;
            } else if w != "auto" {
                options.method = Some(w);
            }
        }
        if let ExprNode::Interval { hi, .. } = a.node() {
            end = Some(hi.clone());
        }
        // A known solution, written y_1 = … (Riccati or reduction of order).
        if let ExprNode::Relation(RelOp::Eq, l, r) = a.node() {
            let known = format!("{}_", parsed.y);
            if l.as_sym().is_some_and(|s| s.starts_with(&known)) {
                options.known = Some(r.clone());
            }
        }
    }
    let exact = if numeric_wanted {
        Err(MathError::NoClosedForm(
            "(a numerical solution was asked for)".into(),
        ))
    } else {
        logged(ctx, |s| {
            if named.is_none() {
                return core_ode::dsolve(eq, &conds, &options, s);
            }
            let mut local = mpp_core::Steps::new();
            let r = core_ode::solve_parsed(&parsed, &conds, &options, &mut local);
            if let Some(log) = s {
                log.0.extend(local.0);
            }
            r
        })
    };
    if as_function && !matches!(&exact, Ok(sol) if matches!(sol.form, Form::Explicit(_))) {
        let unknowns = crate::ivp_wire::Unknowns {
            names: vec![parsed.y.clone()],
            orders: vec![parsed.order],
            var: parsed.x.clone(),
        };
        let indexed: Vec<(usize, Condition)> = conds.iter().cloned().map(|c| (0, c)).collect();
        let sol = crate::ivp_wire::numeric_solution(std::slice::from_ref(eq), &unknowns, &indexed)?;
        ctx.note(NUMERIC_NOTE);
        return match sol.node() {
            ExprNode::Tuple(items) if items.len() == 1 => Ok(items[0].clone()),
            _ => Ok(sol),
        };
    }
    match exact {
        Ok(sol) => {
            ctx.note(format!("{}: {}", sol.method, sol.verified.describe()));
            for n in &sol.notes {
                ctx.note(n.clone());
            }
            if let Form::Series { .. } = &sol.form {
                ctx.note("a truncated series solution");
            }
            Ok(sol.to_expr())
        }
        Err(MathError::NoClosedForm(_) | MathError::Unsupported(_)) if !conds.is_empty() => {
            let x0 = to_f64(&conds[0].at, "the starting point")?;
            let x_end = match end {
                Some(e) => to_f64(&e, "the end point")?,
                None => x0 + 10.0,
            };
            let num = logged(ctx, |s| core_ode::solve_numeric(&parsed, &conds, x_end, s))?;
            let (xe, ye) = num
                .last()
                .ok_or_else(|| MathError::Undefined("the numerical solution".into()))?;
            ctx.note(format!(
                "no closed form was found, so the equation was solved numerically ({}{}); {} points are available for a graph",
                num.method,
                if num.stiff { ", stiffness detected" } else { "" },
                num.points.len()
            ));
            Ok(Expr::relation(
                RelOp::Approx,
                Expr::apply(&parsed.y, vec![Expr::float(xe)]),
                Expr::float(ye),
            ))
        }
        Err(e) => Err(e),
    }
}

/// The unknown function whose derivative `e` holds (`ẍ`, `d²x/dt²`), with the
/// variable.
pub(super) fn differentiated_unknowns(e: &Expr, out: &mut Vec<String>) {
    if let ExprNode::Derivative { expr, .. } = e.node() {
        let name = match expr.node() {
            ExprNode::Sym(s) => Some(s.to_string()),
            ExprNode::Apply(f, args) if args.len() == 1 => Some(f.to_string()),
            _ => None,
        };
        if let Some(n) = name {
            if !out.contains(&n) {
                out.push(n);
            }
        }
    }
    for c in e.children() {
        differentiated_unknowns(c, out);
    }
}

/// True when `e` uses the function `u` (as a name, a call, a primed call or a
/// derivative).
pub(super) fn uses(e: &Expr, u: &str) -> bool {
    match e.node() {
        ExprNode::Sym(s) => &**s == u,
        ExprNode::Apply(f, _) if f.trim_end_matches(['′', '\'']) == u => true,
        _ => e.children().into_iter().any(|c| uses(c, u)),
    }
}

/// The unknown a condition is about: `x(0) = 0`, `x′(0) = v`, or `subs(ẋ, t = 0) = v`.
pub(super) fn condition_unknown(c: &Expr, unknowns: &[String]) -> Option<String> {
    let ExprNode::Relation(RelOp::Eq, l, _) = c.node() else {
        return None;
    };
    let mut found: Vec<&String> = unknowns.iter().filter(|u| uses(l, u)).collect();
    match (found.pop(), found.is_empty()) {
        (Some(u), true) => Some(u.clone()),
        _ => None,
    }
}

/// A system whose equations do not couple (`ẍ = −k ẋ`, `ÿ = −g − k ẏ`): each
/// equation is solved alone with its own conditions. `None` when an equation
/// involves another unknown, or a condition cannot be given to one unknown.
pub(super) fn decoupled(
    eqs: &[Expr],
    rest: &[Expr],
    as_function: bool,
    ctx: &Ctx<'_>,
) -> Option<MathResult<Expr>> {
    let mut unknowns = Vec::with_capacity(eqs.len());
    for eq in eqs {
        let mut own = Vec::new();
        differentiated_unknowns(eq, &mut own);
        let [u] = own.as_slice() else {
            return None;
        };
        if unknowns.contains(u) {
            return None;
        }
        unknowns.push(u.clone());
    }
    if eqs
        .iter()
        .zip(&unknowns)
        .any(|(eq, u)| unknowns.iter().any(|v| v != u && uses(eq, v)))
    {
        return None;
    }
    let mut per: Vec<Vec<Expr>> = vec![Vec::new(); eqs.len()];
    for c in rest {
        let u = condition_unknown(c, &unknowns)?;
        let i = unknowns.iter().position(|v| *v == u)?;
        per[i].push(c.clone());
    }
    let mut solutions = Vec::with_capacity(eqs.len());
    for (eq, conds) in eqs.iter().zip(per) {
        let mut args = vec![eq.clone()];
        args.extend(conds);
        if as_function {
            args.push(Expr::sym(FUNCTION_MARKER));
        }
        let one = Expr::apply("dsolve", args);
        match crate::holders::evaluate_holder(&one, ctx) {
            Ok(v) if matches!(v.node(), ExprNode::Relation(RelOp::Eq, ..)) => solutions.push(v),
            Ok(_) => return None,
            Err(e) => return Some(Err(e)),
        }
    }
    Some(Ok(Expr::new(ExprNode::Tuple(solutions))))
}

pub(super) fn system(eqs: &[Expr], rest: &[Expr], ctx: &Ctx<'_>) -> MathResult<Expr> {
    let as_function = rest.iter().any(is_function_marker);
    let rest: Vec<Expr> = rest
        .iter()
        .filter(|a| !is_function_marker(a))
        .cloned()
        .collect();
    let exact = match decoupled(eqs, &rest, as_function, ctx) {
        Some(r) => r,
        None => linear_system(eqs, &rest, ctx),
    };
    match exact {
        Err(
            MathError::NoClosedForm(_) | MathError::Unsupported(_) | MathError::Inconclusive(_),
        ) if as_function => {
            let sol = numeric_system(eqs, &rest)?;
            ctx.note(NUMERIC_NOTE);
            Ok(sol)
        }
        r => r,
    }
}

/// A system with no closed form, as numerical functions (see [`crate::ivp_wire`]).
pub(super) fn numeric_system(eqs: &[Expr], rest: &[Expr]) -> MathResult<Expr> {
    let mut names: Vec<String> = Vec::new();
    let mut orders: Vec<u32> = Vec::new();
    let mut var: Option<String> = None;
    fn walk(e: &Expr, names: &mut Vec<String>, orders: &mut Vec<u32>, var: &mut Option<String>) {
        if let ExprNode::Derivative { expr, vars } = e.node() {
            let name = match expr.node() {
                ExprNode::Sym(s) => Some(s.to_string()),
                ExprNode::Apply(f, args) if args.len() == 1 => Some(f.to_string()),
                _ => None,
            };
            if let (Some(n), [(v, k)]) = (name, vars.as_slice()) {
                var.get_or_insert_with(|| v.to_string());
                match names.iter().position(|m| *m == n) {
                    Some(i) => orders[i] = orders[i].max(*k),
                    None => {
                        names.push(n);
                        orders.push(*k);
                    }
                }
            }
        }
        for c in e.children() {
            walk(c, names, orders, var);
        }
    }
    for eq in eqs {
        walk(eq, &mut names, &mut orders, &mut var);
    }
    let var = var.ok_or_else(|| missing("dsolve", "a derivative"))?;
    let mut conds = Vec::new();
    for c in rest {
        let (Some(u), ExprNode::Relation(RelOp::Eq, l, r)) =
            (condition_unknown(c, &names), c.node())
        else {
            continue;
        };
        let i = names.iter().position(|n| *n == u).unwrap_or(0);
        if let Some(cond) = condition_of(l, r, &u) {
            conds.push((i, cond));
        }
    }
    // Each equation goes with the unknown it differentiates most.
    let mut ordered: Vec<Expr> = Vec::with_capacity(names.len());
    for (u, k) in names.iter().zip(&orders) {
        let eq = eqs
            .iter()
            .find(|eq| {
                let (mut n, mut o, mut v) = (Vec::new(), Vec::new(), None);
                walk(eq, &mut n, &mut o, &mut v);
                n.iter().zip(&o).any(|(m, j)| m == u && j == k) && !ordered.contains(eq)
            })
            .ok_or_else(|| missing("dsolve", "one equation for each unknown"))?;
        ordered.push(eq.clone());
    }
    let unknowns = crate::ivp_wire::Unknowns { names, orders, var };
    crate::ivp_wire::numeric_solution(&ordered, &unknowns, &conds)
}

pub(super) fn linear_system(eqs: &[Expr], rest: &[Expr], ctx: &Ctx<'_>) -> MathResult<Expr> {
    let mut initial: Option<Vec<Expr>> = rest.iter().find_map(|a| match a.node() {
        ExprNode::Tuple(v) if v.iter().all(Expr::is_constant) => Some(v.clone()),
        _ => None,
    });
    // `x(0) = 1, y(0) = 0`: starting values at 0 written as conditions, put in the
    // order of the solver's unknowns.
    let at_zero: Vec<(String, Expr)> = rest
        .iter()
        .filter_map(|a| match a.node() {
            ExprNode::Relation(RelOp::Eq, l, r) => match l.node() {
                ExprNode::Apply(f, at) if at.len() == 1 && at[0].is_zero() => {
                    Some((f.to_string(), r.clone()))
                }
                _ => None,
            },
            _ => None,
        })
        .collect();
    if initial.is_none() && !at_zero.is_empty() {
        let general = systems::solve_system(eqs, None, None)?;
        let ordered: Option<Vec<Expr>> = general
            .unknowns
            .iter()
            .map(|u| at_zero.iter().find(|(f, _)| f == u).map(|(_, v)| v.clone()))
            .collect();
        match ordered {
            Some(values) if values.len() == at_zero.len() => initial = Some(values),
            _ => {
                return Err(MathError::Domain {
                    function: "dsolve".into(),
                    detail: format!(
                        "a system needs one starting value at 0 for each of {}",
                        general.unknowns.join(", ")
                    ),
                })
            }
        }
    }
    let sol = logged(ctx, |s| systems::solve_system(eqs, initial.as_deref(), s))?;
    ctx.note(format!(
        "matrix exponential {}; the eigenvalues of {} are {}",
        mpp_core::note::tex(
            "e^{At} = \\mathcal{L}^{-1}\\left\\{(sI - A)^{-1}\\right\\}",
            "e^(At) = L⁻¹{(sI − A)⁻¹}"
        ),
        mpp_core::note::tex("A", "A"),
        sol.eigenvalues
            .iter()
            .map(mpp_core::note::maths)
            .collect::<Vec<_>>()
            .join(", ")
    ));
    if let Some(p) = &sol.portrait {
        ctx.note(format!("the origin is a {}", p.kind.name()));
    }
    Ok(Expr::new(ExprNode::Tuple(
        sol.unknowns
            .iter()
            .zip(&sol.solution)
            .map(|(u, s)| Expr::equation(Expr::apply(u, vec![Expr::sym(&sol.t)]), s.clone()))
            .collect(),
    )))
}

/// The PDE helpers.
pub(crate) enum Pde {
    Heat,
    Wave,
    Laplace,
}

pub(crate) fn pde(call: &Call, ctx: &Ctx<'_>, which: Pde) -> MathResult<Expr> {
    let p = call.parts();
    let mut values: Vec<Expr> = p.functions.clone();
    values.extend(p.numbers.iter().cloned());
    let get = |i: usize, what: &str| -> MathResult<Expr> {
        values
            .get(i)
            .cloned()
            .ok_or_else(|| missing("the PDE helper", what))
    };
    let (v, second) = match which {
        Pde::Heat => {
            let (f, l, k) = (
                get(0, "f(x)")?,
                get(1, "the length L")?,
                get(2, "the constant k")?,
            );
            (
                logged(ctx, |s| core_pde::heat(&f, "x", "t", &l, &k, s))?,
                "t",
            )
        }
        Pde::Wave => {
            let (f, g, l, c) = (
                get(0, "f(x)")?,
                get(1, "g(x)")?,
                get(2, "the length L")?,
                get(3, "the speed c")?,
            );
            (
                logged(ctx, |s| core_pde::wave(&f, &g, "x", "t", &l, &c, s))?,
                "t",
            )
        }
        Pde::Laplace => {
            let (f, a, b) = (
                get(0, "f(x)")?,
                get(1, "the width a")?,
                get(2, "the height b")?,
            );
            (
                logged(ctx, |s| {
                    core_pde::laplace_rectangle(&f, "x", "y", &a, &b, s)
                })?,
                "y",
            )
        }
    };
    ctx.note("separation of variables with a Fourier sine series; each term satisfies the equation and the boundary conditions");
    Ok(Expr::equation(
        Expr::apply("u", vec![Expr::sym("x"), Expr::sym(second)]),
        v,
    ))
}
