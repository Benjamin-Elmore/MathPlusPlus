//! Numerical solutions as functions in documents: a differential equation (or system)
//! with no closed form, whose initial conditions are all given at one point, defines
//! its unknowns as `ivp(…)` expressions (see [`mpp_core::ode::ivp`]). This
//! module evaluates them at a number, keeping each problem's accepted steps so later
//! evaluations start from the nearest one, and builds them from a parsed equation or
//! system.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use mpp_core::ode::ivp::{component, Ivp, IVP, VAR};
use mpp_core::ode::{parse, Condition};
use mpp_core::{EmptyEnv, Expr, ExprNode, MathError, MathResult};

use crate::holders::Ctx;

/// Problems kept at once; the cache is emptied when it grows beyond this.
const MAX_CACHED: usize = 64;

/// The accepted steps of one problem, forwards and backwards from its start.
#[derive(Default)]
struct Path {
    forward: Vec<(f64, Vec<f64>)>,
    backward: Vec<(f64, Vec<f64>)>,
}

fn cache() -> &'static Mutex<HashMap<String, Arc<Mutex<Path>>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<Mutex<Path>>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// `ivp(F, Y₀, t₀, k, t)` at a number `t`.
pub fn evaluate(e: &Expr, _ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
    let ExprNode::Apply(name, args) = e.node() else {
        return None;
    };
    if &**name != IVP {
        return None;
    }
    let (sys, k, t) = Ivp::read(args)?;
    let t = t.simplify();
    if !t.is_constant() {
        return None;
    }
    let t = match t.eval_f64(&EmptyEnv) {
        Ok(v) if v.is_finite() => v,
        _ => return Some(Err(MathError::NotANumber(t.to_string()))),
    };
    Some(value_at(&sys, t).map(|y| Expr::float(y[k])))
}

/// The solution at `t`: from the nearest kept step at or before `t` (in the direction
/// away from the start), integrated to `t` exactly.
pub fn value_at(sys: &Ivp, t: f64) -> MathResult<Vec<f64>> {
    if t == sys.t0 {
        return Ok(sys.y0.clone());
    }
    let key = sys.key();
    let path = {
        let mut map = cache()
            .lock()
            .map_err(|_| MathError::Undefined("the numerical solution cache".into()))?;
        if map.len() > MAX_CACHED {
            map.clear();
        }
        map.entry(key).or_default().clone()
    };
    let mut path = path
        .lock()
        .map_err(|_| MathError::Undefined("the numerical solution cache".into()))?;
    let forward = t > sys.t0;
    let steps = if forward {
        &mut path.forward
    } else {
        &mut path.backward
    };
    let beyond = |s: f64| if forward { s >= t } else { s <= t };
    // Extend the kept steps past t (with room, so nearby points need no new run).
    if steps.last().is_none_or(|(s, _)| !beyond(*s)) {
        let (start_t, start_y) = steps.last().cloned().unwrap_or((sys.t0, sys.y0.clone()));
        let reach = sys.t0 + (t - sys.t0) * 1.5;
        let mut more = sys.integrate(start_t, &start_y, reach)?;
        if !steps.is_empty() {
            more.remove(0);
        }
        steps.extend(more);
    }
    // The last kept step not past t.
    let i = steps
        .partition_point(|(s, _)| if forward { *s <= t } else { *s >= t })
        .saturating_sub(1);
    let (s, y) = steps.get(i).cloned().unwrap_or((sys.t0, sys.y0.clone()));
    if s == t {
        return Ok(y);
    }
    let run = sys.integrate(s, &y, t)?;
    run.last()
        .map(|(_, y)| y.clone())
        .ok_or_else(|| MathError::Undefined("the numerical solution".into()))
}

/// The unknowns of a system, each with the order of its highest derivative.
pub struct Unknowns {
    pub names: Vec<String>,
    pub orders: Vec<u32>,
    pub var: String,
}

/// Builds `u_i(t) = ivp(…)` for each unknown of `equations` (one equation per
/// unknown, each linear in that unknown's highest derivative), from conditions at one
/// point giving every component. `None` when the problem is not of that shape.
pub fn numeric_solution(
    equations: &[Expr],
    unknowns: &Unknowns,
    conditions: &[(usize, Condition)],
) -> MathResult<Expr> {
    let shape = |what: &str| MathError::Unsupported(format!("a numerical solution: {what}"));
    if equations.len() != unknowns.names.len() {
        return Err(shape("one equation is needed for each unknown"));
    }
    // The state: each unknown and its derivatives below the highest.
    let mut offsets = Vec::with_capacity(unknowns.names.len());
    let mut size = 0usize;
    for &k in &unknowns.orders {
        if k == 0 {
            return Err(shape("an equation without a derivative"));
        }
        offsets.push(size);
        size += k as usize;
    }
    // Placeholders of every unknown's derivatives → state components.
    let state_of = |e: &Expr| -> Expr {
        let mut out = e.clone();
        for (i, u) in unknowns.names.iter().enumerate() {
            for j in 0..unknowns.orders[i] {
                out = out.subs(
                    &parse::placeholder(u, j),
                    &Expr::sym(&component(offsets[i] + j as usize)),
                );
            }
        }
        out.subs(&unknowns.var, &Expr::sym(VAR))
    };
    let mut rhs = vec![Expr::zero(); size];
    for (i, eq) in equations.iter().enumerate() {
        // The equation in placeholders for every unknown's derivatives.
        let mut residual = parse(eq, Some(&unknowns.names[i]), Some(&unknowns.var))?.residual;
        for (j, other) in unknowns.names.iter().enumerate() {
            if j == i {
                continue;
            }
            let as_eq = Expr::equation(residual.clone(), Expr::zero());
            residual = match parse(&as_eq, Some(other), Some(&unknowns.var)) {
                Ok(o) => o.residual,
                Err(_) => residual.subs_expr(
                    &Expr::apply(other, vec![Expr::sym(&unknowns.var)]),
                    &Expr::sym(&parse::placeholder(other, 0)),
                ),
            };
        }
        let top = parse::placeholder(&unknowns.names[i], unknowns.orders[i]);
        let a = mpp_core::calc::diff(&residual, &top, 1, None)?.simplify();
        if a.is_zero() || a.contains_sym(&top) {
            return Err(shape(&format!(
                "{eq} cannot be solved for its highest derivative"
            )));
        }
        let b = residual.subs(&top, &Expr::zero()).simplify();
        // Another unknown's highest derivative in it would need the equations solved
        // together.
        for (j, other) in unknowns.names.iter().enumerate() {
            let other_top = parse::placeholder(other, unknowns.orders[j]);
            if j != i && (a.contains_sym(&other_top) || b.contains_sym(&other_top)) {
                return Err(shape("equations coupled through their highest derivatives"));
            }
        }
        let top_value = (-(b) / a).simplify();
        let o = offsets[i];
        let k = unknowns.orders[i] as usize;
        for j in 0..k - 1 {
            rhs[o + j] = Expr::sym(&component(o + j + 1));
        }
        rhs[o + k - 1] = state_of(&top_value);
    }
    // The starting values, all at one point.
    let mut t0: Option<f64> = None;
    let mut y0 = vec![f64::NAN; size];
    for (i, c) in conditions {
        let at =
            c.at.eval_f64(&EmptyEnv)
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(|| shape("a condition at a point that is not a number"))?;
        if t0.is_some_and(|s| (s - at).abs() > 1e-12 * s.abs().max(1.0)) {
            return Err(shape("initial conditions at different points"));
        }
        t0 = Some(at);
        let j = c.order as usize;
        if j >= unknowns.orders[*i] as usize {
            return Err(shape("a condition on the highest derivative"));
        }
        y0[offsets[*i] + j] = c
            .value
            .eval_f64(&EmptyEnv)
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(|| MathError::NotANumber(c.value.to_string()))?;
    }
    let t0 = t0.ok_or_else(|| shape("initial conditions are needed"))?;
    if y0.iter().any(|v| v.is_nan()) {
        return Err(shape(
            "a starting value for every unknown and its lower derivatives",
        ));
    }
    let sys = Ivp { rhs, y0, t0 };
    sys.validate()?;
    let t = Expr::sym(&unknowns.var);
    Ok(Expr::new(ExprNode::Tuple(
        unknowns
            .names
            .iter()
            .zip(&offsets)
            .map(|(u, &o)| Expr::equation(Expr::apply(u, vec![t.clone()]), sys.expr(o, t.clone())))
            .collect(),
    )))
}

/// An expression in numerical solutions, simplified with each solution taken as a
/// name: the chain rule leaves `(δ − γ/x)(αx − βxy) + …` in them, which expands to 0
/// for a conserved quantity. Kept as it was when the expansion is not smaller.
pub fn tidy(e: &Expr) -> Expr {
    let mut atoms: Vec<Expr> = Vec::new();
    collect(e, &mut atoms);
    if atoms.is_empty() {
        return e.clone();
    }
    let names: Vec<String> = (0..atoms.len()).map(|i| format!("#ivp{i}")).collect();
    let mut plain = e.clone();
    for (a, n) in atoms.iter().zip(&names) {
        plain = plain.subs_expr(a, &Expr::sym(n));
    }
    let Ok(expanded) = plain.expand() else {
        return e.clone();
    };
    let expanded = expanded.simplify();
    if expanded.size() >= plain.size() {
        return e.clone();
    }
    let pairs: Vec<(&str, Expr)> = names
        .iter()
        .map(String::as_str)
        .zip(atoms.iter().cloned())
        .collect();
    expanded.subs_all(&pairs)
}

fn collect(e: &Expr, out: &mut Vec<Expr>) {
    if let ExprNode::Apply(name, _) = e.node() {
        if &**name == IVP {
            if !out.contains(e) {
                out.push(e.clone());
            }
            return;
        }
    }
    for c in e.children() {
        collect(c, out);
    }
}
