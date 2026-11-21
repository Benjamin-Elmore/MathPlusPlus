//! Partial differential equations written as statements:
//!
//! `\frac{\partial u}{\partial t} = \alpha \frac{\partial^2 u}{\partial x^2}, \quad
//! u(0, t) = u(L, t) = 0, \quad u(x, 0) = f(x)`
//!
//! The heat equation `u_t = k u_xx` and the wave equation `u_tt = c² u_xx` on
//! `0 < x < L` with `u = 0` at both ends are solved by separation of variables. When
//! the initial data are a finite sum of the modes `sin(nπx/L)`, the solution is that
//! finite sum, each mode with its own decay or oscillation (and units carried
//! through); otherwise the Fourier sine series of the ODE toolkit is used, for data
//! without units and without sines or cosines of x (whose coefficients could hit a
//! removable singularity at some n).

use mpp_core::ode::pde as core_pde;
use mpp_core::{EmptyEnv, Expr, ExprNode, Func, LogicOp, MathError, MathResult, RelOp};

/// The kinds of equation recognised.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Heat,
    Wave,
}

/// The solution of a statement: the function's parameters and its body.
pub struct Solved {
    pub params: Vec<String>,
    pub body: Expr,
    pub method: String,
}

/// True when `eq` differentiates the unknown `u` with respect to two variables.
pub fn is_pde(eq: &Expr, u: &str) -> bool {
    let mut vars = Vec::new();
    derivatives(eq, u, &mut vars);
    let mut names: Vec<&String> = vars.iter().map(|(v, _)| v).collect();
    names.sort();
    names.dedup();
    names.len() >= 2
}

/// `(variable, order)` of every derivative of `u` in `e`.
fn derivatives(e: &Expr, u: &str, out: &mut Vec<(String, u32)>) {
    if let ExprNode::Derivative { expr, vars } = e.node() {
        if names_unknown(expr, u) {
            for (v, k) in vars {
                out.push((v.to_string(), *k));
            }
            return;
        }
    }
    for c in e.children() {
        derivatives(c, u, out);
    }
}

fn names_unknown(e: &Expr, u: &str) -> bool {
    match e.node() {
        ExprNode::Sym(s) => &**s == u,
        ExprNode::Apply(f, _) => &**f == u,
        _ => false,
    }
}

/// Solves the statement `eq` with `conditions` (both resolved) for the unknown `u`.
pub fn solve(eq: &Expr, conditions: &[Expr], u: &str) -> MathResult<Solved> {
    let unsupported =
        |what: &str| MathError::Unsupported(format!("this partial differential equation ({what})"));
    let ExprNode::Relation(RelOp::Eq, l, r) = eq.node() else {
        return Err(unsupported("not an equation"));
    };
    let residual = (l.clone() - r.clone()).simplify();
    let mut ders = Vec::new();
    derivatives(&residual, u, &mut ders);
    let first: Vec<&String> = ders
        .iter()
        .filter(|(_, k)| *k == 1)
        .map(|(v, _)| v)
        .collect();
    let second: Vec<&String> = ders
        .iter()
        .filter(|(_, k)| *k == 2)
        .map(|(v, _)| v)
        .collect();
    let (kind, t, x) = match (first.as_slice(), second.as_slice()) {
        ([t], [x]) if t != x => (Kind::Heat, (*t).clone(), (*x).clone()),
        ([], [a, b]) if a != b => {
            // u_tt = c² u_xx: the time variable is the one the conditions start from.
            let t = if conditions_start_at(conditions, u, a) {
                a
            } else {
                b
            };
            let x = if t == a { b } else { a };
            (Kind::Wave, (*t).clone(), (*x).clone())
        }
        _ => return Err(unsupported("only the heat and wave equations are solved")),
    };
    if ders.len() != 2 {
        return Err(unsupported("several derivatives in one variable"));
    }
    // a · ∂ₜᵏu + b · ∂ₓ²u = 0, with constant a and b.
    let time_d = Expr::new(ExprNode::Derivative {
        expr: Expr::sym(u),
        vars: vec![(t.as_str().into(), if kind == Kind::Heat { 1 } else { 2 })],
    });
    let space_d = Expr::new(ExprNode::Derivative {
        expr: Expr::sym(u),
        vars: vec![(x.as_str().into(), 2)],
    });
    let (p, q) = ("#p", "#q");
    let plain = normalise(&residual, u)
        .subs_expr(&time_d, &Expr::sym(p))
        .subs_expr(&space_d, &Expr::sym(q));
    let a = mpp_core::calc::diff(&plain, p, 1, None)?.simplify();
    let b = mpp_core::calc::diff(&plain, q, 1, None)?.simplify();
    let rest = plain
        .subs(p, &Expr::zero())
        .subs(q, &Expr::zero())
        .simplify();
    let constant = |c: &Expr| {
        ![p, q, t.as_str(), x.as_str(), u]
            .iter()
            .any(|n| c.contains_sym(n))
    };
    if !rest.is_zero() || a.is_zero() || b.is_zero() || !constant(&a) || !constant(&b) {
        return Err(unsupported(
            "it must be linear and homogeneous with constant coefficients",
        ));
    }
    let coefficient = (-(b) / a).simplify(); // k, or c²
                                             // The conditions.
    let c = read_conditions(conditions, u, &x, &t)?;
    let (zero_end, length) = match c.ends.as_slice() {
        [e1, e2] => {
            let (lo, hi) = if is_zero(e1) { (e1, e2) } else { (e2, e1) };
            (lo.clone(), hi.clone())
        }
        _ => return Err(unsupported("u = 0 is needed at both ends x = 0 and x = L")),
    };
    if !is_zero(&zero_end) || is_zero(&length) {
        return Err(unsupported("the ends must be x = 0 and x = L"));
    }
    let f = c
        .initial
        .ok_or_else(|| unsupported("the initial values u(x, 0) are needed"))?;
    let params = c.params;
    let body = match kind {
        Kind::Heat => match finite_modes(&f, &x, &length) {
            Some(modes) => {
                let terms: Vec<Expr> = modes
                    .into_iter()
                    .map(|(n, coef)| {
                        let lam = Expr::int(n) * Expr::pi() / length.clone();
                        coef * Expr::e().pow(
                            -(coefficient.clone() * lam.clone().pow(Expr::int(2)) * Expr::sym(&t)),
                        ) * Expr::func(Func::Sin, vec![lam * Expr::sym(&x)])
                            .unwrap_or_else(|_| Expr::undefined())
                    })
                    .collect();
                Expr::new(ExprNode::Add(terms)).simplify()
            }
            None => {
                series_allowed(&f, &x, &length, &coefficient)?;
                core_pde::heat(&f, &x, &t, &length, &coefficient, None)?
            }
        },
        Kind::Wave => {
            let g = c.initial_rate.unwrap_or_else(Expr::zero);
            let speed = coefficient.clone().sqrt().simplify();
            match (finite_modes(&f, &x, &length), finite_modes(&g, &x, &length)) {
                (Some(fm), Some(gm)) => {
                    let mut terms = Vec::new();
                    let mode = |n: i64| Expr::int(n) * Expr::pi() / length.clone();
                    let sin = |e: Expr| {
                        Expr::func(Func::Sin, vec![e]).unwrap_or_else(|_| Expr::undefined())
                    };
                    let cos = |e: Expr| {
                        Expr::func(Func::Cos, vec![e]).unwrap_or_else(|_| Expr::undefined())
                    };
                    for (n, coef) in fm {
                        let w = speed.clone() * mode(n);
                        terms.push(coef * cos(w * Expr::sym(&t)) * sin(mode(n) * Expr::sym(&x)));
                    }
                    for (n, coef) in gm {
                        let w = speed.clone() * mode(n);
                        terms.push(
                            coef / w.clone()
                                * sin(w * Expr::sym(&t))
                                * sin(mode(n) * Expr::sym(&x)),
                        );
                    }
                    Expr::new(ExprNode::Add(terms)).simplify()
                }
                _ => {
                    series_allowed(&f, &x, &length, &coefficient)?;
                    series_allowed(&g, &x, &length, &coefficient)?;
                    core_pde::wave(&f, &g, &x, &t, &length, &speed, None)?
                }
            }
        }
    };
    Ok(Solved {
        params,
        body,
        method: format!(
            "separation of variables: {} equation on 0 < {x} < L with u = 0 at both ends",
            if kind == Kind::Heat {
                "the heat"
            } else {
                "the wave"
            }
        ),
    })
}

/// `u` and its derivatives written with the unknown as a call `u(x, t)` become the bare
/// name, so the derivatives compare equal.
fn normalise(e: &Expr, u: &str) -> Expr {
    match e.node() {
        ExprNode::Apply(f, args) if &**f == u && args.iter().all(|a| a.as_sym().is_some()) => {
            Expr::sym(u)
        }
        _ => Expr::new(e.map_children(&mut |c| normalise(c, u))),
    }
}

fn is_zero(e: &Expr) -> bool {
    e.simplify().is_zero()
}

/// The conditions read: the ends where u = 0, the initial values and rates, and the
/// order of u's parameters.
struct Conditions {
    ends: Vec<Expr>,
    initial: Option<Expr>,
    initial_rate: Option<Expr>,
    params: Vec<String>,
}

/// True when a condition gives u with the variable `v` at a number (`u(x, 0) = …`).
fn conditions_start_at(conditions: &[Expr], u: &str, v: &str) -> bool {
    let mut eqs = Vec::new();
    for c in conditions {
        flatten(c, &mut eqs);
    }
    eqs.iter().any(|(a, b)| {
        [a, b].iter().any(|s| match s.node() {
            ExprNode::Apply(f, args) if &**f == u => {
                args.iter().any(|a| a.is_constant()) && !args.iter().any(|a| a.is_sym(v))
            }
            _ => false,
        })
    })
}

/// The equalities in a condition, a chain `a = b = c` given as pairs.
fn flatten(c: &Expr, out: &mut Vec<(Expr, Expr)>) {
    match c.node() {
        ExprNode::Logic(LogicOp::And, items) => items.iter().for_each(|i| flatten(i, out)),
        ExprNode::Relation(RelOp::Eq, a, b) => out.push((a.clone(), b.clone())),
        _ => {}
    }
}

fn read_conditions(conditions: &[Expr], u: &str, x: &str, t: &str) -> MathResult<Conditions> {
    let unsupported = |what: &str| {
        MathError::Unsupported(format!(
            "this condition of a partial differential equation ({what})"
        ))
    };
    let mut eqs = Vec::new();
    for c in conditions {
        flatten(c, &mut eqs);
    }
    // Values of the calls of u, found through chains (u(0, t) = u(L, t) = 0).
    let is_call = |e: &Expr| {
        matches!(e.node(), ExprNode::Apply(f, _) if &**f == u)
            || matches!(e.node(), ExprNode::Apply(f, args) if &**f == "subs" && args.first().is_some_and(|d| matches!(d.node(), ExprNode::Derivative { .. })))
    };
    let mut known: Vec<(Expr, Expr)> = Vec::new();
    for _ in 0..eqs.len() + 1 {
        for (a, b) in &eqs {
            for (call, other) in [(a, b), (b, a)] {
                if !is_call(call) || known.iter().any(|(c, _)| c == call) {
                    continue;
                }
                if !is_call(other) {
                    known.push((call.clone(), other.clone()));
                } else if let Some((_, v)) = known.iter().find(|(c, _)| c == other) {
                    known.push((call.clone(), v.clone()));
                }
            }
        }
    }
    let mut ends = Vec::new();
    let mut initial = None;
    let mut initial_rate = None;
    let mut params: Option<Vec<String>> = None;
    for (call, value) in known {
        match call.node() {
            ExprNode::Apply(f, args) if &**f == u => {
                let [a0, a1] = args.as_slice() else {
                    return Err(unsupported("u must take two arguments"));
                };
                // u(a, t) = 0: an end; u(x, 0) = f(x): the start.
                let (pos, at) = if a1.is_sym(t) && !a0.contains_sym(t) && !a0.contains_sym(x) {
                    (vec![x.to_string(), t.to_string()], (true, a0.clone()))
                } else if a0.is_sym(t) && !a1.contains_sym(t) && !a1.contains_sym(x) {
                    (vec![t.to_string(), x.to_string()], (true, a1.clone()))
                } else if a0.is_sym(x) && !a1.contains_sym(x) && !a1.contains_sym(t) {
                    (vec![x.to_string(), t.to_string()], (false, a1.clone()))
                } else if a1.is_sym(x) && !a0.contains_sym(x) && !a0.contains_sym(t) {
                    (vec![t.to_string(), x.to_string()], (false, a0.clone()))
                } else {
                    return Err(unsupported("each condition fixes x or t at a value"));
                };
                if params.as_ref().is_some_and(|p| *p != pos) {
                    return Err(unsupported("u's arguments in two different orders"));
                }
                params = Some(pos);
                match at {
                    (true, end) => {
                        if !is_zero(&value) {
                            return Err(unsupported("only u = 0 at the ends is solved"));
                        }
                        ends.push(end);
                    }
                    (false, start) => {
                        if !is_zero(&start) {
                            return Err(unsupported("the initial values must be at t = 0"));
                        }
                        initial = Some(value);
                    }
                }
            }
            // u_t(x, 0) = g(x), lowered as subs(∂u/∂t, t = 0).
            ExprNode::Apply(_, args) => {
                let [d, at] = args.as_slice() else {
                    continue;
                };
                let ExprNode::Derivative { vars, .. } = d.node() else {
                    continue;
                };
                let ExprNode::Relation(RelOp::Eq, v, point) = at.node() else {
                    continue;
                };
                if v.is_sym(t) && is_zero(point) && vars.len() == 1 && vars[0].0.as_ref() == t {
                    initial_rate = Some(value);
                }
            }
            _ => {}
        }
    }
    Ok(Conditions {
        ends,
        initial,
        initial_rate,
        params: params.unwrap_or_else(|| vec![x.to_string(), t.to_string()]),
    })
}

/// `f` as a finite sum of modes `c_n sin(nπx/L)` (n a positive whole number, c_n free
/// of x): the pairs `(n, c_n)`. Zero is the empty sum.
fn finite_modes(f: &Expr, x: &str, length: &Expr) -> Option<Vec<(i64, Expr)>> {
    let f = f.simplify();
    if f.is_zero() {
        return Some(Vec::new());
    }
    let expanded = f.expand().ok()?;
    let terms = match expanded.node() {
        ExprNode::Add(ts) => ts.clone(),
        _ => vec![expanded.clone()],
    };
    let mut out: Vec<(i64, Expr)> = Vec::new();
    for term in terms {
        let factors = match term.node() {
            ExprNode::Mul(fs) => fs.clone(),
            _ => vec![term.clone()],
        };
        let mut mode = None;
        let mut coef = Vec::new();
        for fac in factors {
            match fac.node() {
                ExprNode::Func(Func::Sin, args) if args[0].contains_sym(x) && mode.is_none() => {
                    let m = (args[0].clone() / Expr::sym(x)).simplify();
                    if m.contains_sym(x) {
                        return None;
                    }
                    let n = (m * length.clone() / Expr::pi()).simplify();
                    let n = n.as_number()?.as_i64().filter(|n| *n > 0)?;
                    mode = Some(n);
                }
                _ if fac.contains_sym(x) => return None,
                _ => coef.push(fac),
            }
        }
        let n = mode?;
        let c = Expr::new(ExprNode::Mul(coef)).simplify();
        match out.iter_mut().find(|(m, _)| *m == n) {
            Some((_, prev)) => *prev = (prev.clone() + c).simplify(),
            None => out.push((n, c)),
        }
    }
    Some(out)
}

/// The Fourier series path: data with no units, no trigonometric functions of x, and a
/// numeric length and coefficient.
fn series_allowed(f: &Expr, x: &str, length: &Expr, coefficient: &Expr) -> MathResult<()> {
    let refuse = |what: &str| {
        Err(MathError::Unsupported(format!(
            "a Fourier sine series here ({what})"
        )))
    };
    if [f, length, coefficient]
        .iter()
        .any(|e| crate::units_wire::contains_units(e))
    {
        return refuse("the data carry units and are not a finite sum of sine modes");
    }
    fn trig_of(e: &Expr, x: &str) -> bool {
        match e.node() {
            ExprNode::Func(Func::Sin | Func::Cos | Func::Tan, args) => args[0].contains_sym(x),
            _ => e.children().into_iter().any(|c| trig_of(c, x)),
        }
    }
    if trig_of(f, x) {
        return refuse("initial data with sines or cosines of x that are not whole modes");
    }
    if length.eval_f64(&EmptyEnv).is_err() {
        return refuse("the length is not a number");
    }
    Ok(())
}
