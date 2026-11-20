//! Ordinary differential equations: separable and first-order linear equations exactly,
//! and the classical Runge–Kutta method (RK4) numerically, for single equations and
//! systems.
//!
//! Exact methods implement [`OdeMethod`] and are tried in the order of [`methods`];
//! more methods (exact, Bernoulli, constant coefficients …) slot in the same way.

use crate::domains::solve::isolate;
use crate::domains::util::{expect_args_between, is_zero, real_arg, sym_arg};
use crate::domains::{Ctx, Domain, Value};
use crate::error::{MathError, MathResult};
use crate::eval::Env;
use crate::expr::{Expr, ExprNode};
use crate::functions::Func;
use crate::simplify::mul;

/// Most RK4 steps one call may take.
pub const MAX_STEPS: usize = 1_000_000;
/// Most rows a returned table may have.
pub const MAX_TABLE: usize = 10_000;
/// The placeholder for `y'` while an equation is solved for it.
const DY: &str = "\u{1d466}′";

/// A first-order equation in explicit form `y' = rhs(x, y)`.
#[derive(Clone, Debug, PartialEq)]
pub struct FirstOrder {
    /// The independent variable.
    pub x: String,
    /// The unknown function.
    pub y: String,
    /// The right-hand side.
    pub rhs: Expr,
}

/// An exact solution.
#[derive(Clone, Debug, PartialEq)]
pub struct OdeSolution {
    /// `y = …` when explicit, otherwise an implicit relation between `x` and `y`.
    pub solution: Expr,
    /// The method's name, for the steps and the result note.
    pub method: &'static str,
    /// The name of the arbitrary constant (absent once an initial condition fixed it).
    pub constant: Option<String>,
}

/// One exact solution method.
pub trait OdeMethod: Send + Sync {
    /// The method's name ("separable", "linear (integrating factor)" …).
    fn name(&self) -> &'static str;

    /// Tries to solve; `None` when the equation is not of this type.
    fn try_solve(&self, ode: &FirstOrder, c: &str, ctx: &mut Ctx<'_>) -> Option<MathResult<Expr>>;
}

/// The exact methods, in the order they are tried.
pub fn methods() -> Vec<Box<dyn OdeMethod>> {
    vec![Box::new(Separable), Box::new(Linear)]
}

/// Replaces `y(x)` by `y` and every form of `y'` by a placeholder.
fn normalise(e: &Expr, x: &str, y: &str) -> Expr {
    match e.node() {
        ExprNode::Derivative { expr, vars }
            if vars.len() == 1 && vars[0].1 == 1 && &*vars[0].0 == x =>
        {
            let inner = normalise(expr, x, y);
            if inner.is_sym(y) {
                return Expr::sym(DY);
            }
            Expr::new(ExprNode::Derivative {
                expr: inner,
                vars: vars.clone(),
            })
        }
        ExprNode::Apply(name, args) if args.len() == 1 && args[0].is_sym(x) => {
            if &**name == y {
                Expr::sym(y)
            } else if name.strip_suffix('\'') == Some(y) || name.strip_suffix('′') == Some(y) {
                Expr::sym(DY)
            } else {
                e.clone()
            }
        }
        ExprNode::Sym(s) if s.strip_suffix('\'') == Some(y) || s.strip_suffix('′') == Some(y) => {
            Expr::sym(DY)
        }
        _ => crate::simplify::canonical(e.map_children(&mut |c| normalise(c, x, y))),
    }
}

/// Reads `y' = f(x, y)` (any arrangement that is linear in `y'`); a plain expression is
/// taken as the right-hand side.
pub fn first_order(eq: &Expr, x: &str, y: &str) -> MathResult<FirstOrder> {
    let n = normalise(eq, x, y);
    let rhs = match n.node() {
        ExprNode::Relation(crate::expr::RelOp::Eq, l, r) => {
            let f = (l - r).simplify();
            let c = f.as_poly(DY).ok_or_else(|| not_first_order(eq))?;
            match c.as_slice() {
                [c0, c1] if !c1.contains_sym(DY) && !is_zero(c1) => (-c0 / c1.clone()).simplify(),
                _ => return Err(not_first_order(eq)),
            }
        }
        _ => n,
    };
    if rhs.contains_sym(DY) {
        return Err(not_first_order(eq));
    }
    Ok(FirstOrder {
        x: x.into(),
        y: y.into(),
        rhs,
    })
}

fn not_first_order(eq: &Expr) -> MathError {
    MathError::Unsupported(format!(
        "{eq} as a differential equation (only first-order equations so far)"
    ))
}

/// A constant name not used in the equation.
fn fresh_constant(ode: &FirstOrder) -> String {
    let used = ode.rhs.free_symbols();
    std::iter::once("C".to_string())
        .chain((1..).map(|i| format!("C_{i}")))
        .find(|c| !used.iter().any(|s| **s == **c) && *c != ode.x && *c != ode.y)
        .unwrap_or_else(|| "C".into())
}

/// Solves `ode` exactly, fixing the constant from `y(x0) = y0` when given.
pub fn solve_first_order(
    ode: &FirstOrder,
    initial: Option<(&Expr, &Expr)>,
    ctx: &mut Ctx<'_>,
) -> MathResult<OdeSolution> {
    let c = fresh_constant(ode);
    for m in methods() {
        if let Some(result) = m.try_solve(ode, &c, ctx) {
            let mut solution = match result {
                Ok(s) => s,
                // Not solvable this way after all: try the next method.
                Err(MathError::Unsupported(_)) => continue,
                Err(e) => return Err(e),
            };
            let mut constant = Some(c.clone());
            if let Some((x0, y0)) = initial {
                solution = apply_initial(&solution, ode, &c, x0, y0)?;
                constant = None;
            }
            ctx.note(format!("method: {}", m.name()));
            ctx.step(
                m.name(),
                &Expr::equation(Expr::sym(&format!("{}'", ode.y)), ode.rhs.clone()),
                &solution,
                None,
            );
            return Ok(OdeSolution {
                solution,
                method: m.name(),
                constant,
            });
        }
    }
    Err(MathError::Unsupported(format!(
        "an exact solution of {}' = {} (try rk4 for a numeric one)",
        ode.y, ode.rhs
    )))
}

/// Substitutes the initial condition and solves for the constant.
fn apply_initial(sol: &Expr, ode: &FirstOrder, c: &str, x0: &Expr, y0: &Expr) -> MathResult<Expr> {
    let ExprNode::Relation(op, l, r) = sol.node() else {
        return Ok(sol.clone());
    };
    let pairs = [(ode.x.as_str(), x0.clone()), (ode.y.as_str(), y0.clone())];
    let f = (l.subs_all(&pairs) - r.subs_all(&pairs)).simplify();
    let mut ctx = Ctx::new();
    let s = crate::domains::solve::solve(&f, c, None, &mut ctx)?;
    let value = match s.values() {
        Some([v]) => v.clone(),
        _ => {
            return Err(MathError::Undefined(
                "a single constant matching the initial condition".into(),
            ))
        }
    };
    Ok(Expr::relation(
        *op,
        l.subs(c, &value).simplify(),
        r.subs(c, &value).simplify(),
    ))
}

/// `y' = g(x)·h(y)`: `∫ dy/h(y) = ∫ g(x) dx + C`.
pub struct Separable;

impl OdeMethod for Separable {
    fn name(&self) -> &'static str {
        "separable"
    }

    fn try_solve(&self, ode: &FirstOrder, c: &str, ctx: &mut Ctx<'_>) -> Option<MathResult<Expr>> {
        let (x, y) = (ode.x.as_str(), ode.y.as_str());
        let factors = match ode.rhs.node() {
            ExprNode::Mul(fs) => fs.clone(),
            _ => vec![ode.rhs.clone()],
        };
        if factors
            .iter()
            .any(|f| f.contains_sym(x) && f.contains_sym(y))
        {
            return None;
        }
        let (hs, gs): (Vec<Expr>, Vec<Expr>) = factors.into_iter().partition(|f| f.contains_sym(y));
        let (g, h) = (mul(gs), mul(hs));
        Some(separable(&g, &h, x, y, c, ctx))
    }
}

fn separable(g: &Expr, h: &Expr, x: &str, y: &str, c: &str, ctx: &mut Ctx<'_>) -> MathResult<Expr> {
    let ys = Expr::sym(y);
    let cs = Expr::sym(c);
    let big_f = ctx.calculus.integrate(g, x)?;
    if !h.contains_sym(y) {
        return Ok(Expr::equation(ys, (big_f + cs).simplify()));
    }
    // h(y) = k·y gives ln|y| = F/k…: write y = C e^{F/k} directly.
    if let Some(k) = h.as_poly(y).and_then(|p| match p.as_slice() {
        [z, k] if z.is_zero() && !k.contains_sym(y) => Some(k.clone()),
        _ => None,
    }) {
        let sol = cs * Expr::e().pow((k * big_f).simplify());
        return Ok(Expr::equation(ys, sol.simplify()));
    }
    let big_g = ctx.calculus.integrate(&h.recip(), y)?;
    let rhs = (big_f + cs).simplify();
    if let Ok(Some(iso)) = isolate::isolate(&big_g, &rhs, y, "k") {
        if let [one] = iso.values.as_slice() {
            if !iso.periodic {
                return Ok(Expr::equation(ys, one.clone()));
            }
        }
    }
    Ok(Expr::equation(big_g, rhs))
}

/// `y' + P(x) y = Q(x)`: `y = (∫ μ Q dx + C)/μ` with `μ = e^{∫P dx}`.
pub struct Linear;

impl OdeMethod for Linear {
    fn name(&self) -> &'static str {
        "linear (integrating factor)"
    }

    fn try_solve(&self, ode: &FirstOrder, c: &str, ctx: &mut Ctx<'_>) -> Option<MathResult<Expr>> {
        let (x, y) = (ode.x.as_str(), ode.y.as_str());
        let coeffs = ode.rhs.as_poly(y)?;
        let (q, p) = match coeffs.as_slice() {
            [q, a] => (q.clone(), -a.clone()),
            _ => return None,
        };
        if q.contains_sym(y) || p.contains_sym(y) {
            return None;
        }
        Some((|| {
            let int_p = ctx.calculus.integrate(&p, x)?;
            let mu = exp_simplified(&int_p)?;
            let int_mq = ctx.calculus.integrate(&(&mu * &q).simplify(), x)?;
            let sol = ((int_mq + Expr::sym(c)) / mu).simplify();
            let sol = sol.expand().map(|e| e.simplify()).unwrap_or(sol);
            Ok(Expr::equation(Expr::sym(y), sol))
        })())
    }
}

/// The integrating factor `e^u`, writing `e^{k ln|v|}` as `v^k` (any constant multiple
/// of an integrating factor is one too, so the sign of `v` doesn't matter).
fn exp_simplified(u: &Expr) -> MathResult<Expr> {
    let (k, rest) = crate::simplify::split_coeff(u);
    if let ExprNode::Func(Func::Ln, args) = rest.node() {
        if let [a] = args.as_slice() {
            let v = match a.node() {
                ExprNode::Func(Func::Abs, inner) if inner.len() == 1 => inner[0].clone(),
                _ => a.clone(),
            };
            return Ok(v.pow(Expr::num(k)));
        }
    }
    Ok(Expr::e().pow(u.clone()))
}

struct Point<'a> {
    names: [&'a str; 2],
    values: [f64; 2],
}

impl Env for Point<'_> {
    fn real(&self, name: &str) -> Option<f64> {
        self.names
            .iter()
            .position(|n| *n == name)
            .map(|i| self.values[i])
    }
}

struct Vector<'a> {
    names: &'a [&'a str],
    values: &'a [f64],
}

impl Env for Vector<'_> {
    fn real(&self, name: &str) -> Option<f64> {
        self.names
            .iter()
            .position(|n| *n == name)
            .map(|i| self.values[i])
    }
}

fn check_steps(steps: usize) -> MathResult<()> {
    if steps == 0 {
        return Err(MathError::Domain {
            function: "rk4".into(),
            detail: "the number of steps must be positive".into(),
        });
    }
    if steps > MAX_STEPS {
        return Err(MathError::TooLarge(format!("{steps} steps")));
    }
    Ok(())
}

/// RK4 for `y' = f(x, y)` from `(x0, y0)` to `x1` in `steps` equal steps; returns every
/// point, the initial one included.
pub fn rk4(
    f: &Expr,
    x: &str,
    y: &str,
    x0: f64,
    y0: f64,
    x1: f64,
    steps: usize,
) -> MathResult<Vec<(f64, f64)>> {
    check_steps(steps)?;
    let h = (x1 - x0) / steps as f64;
    let eval = |xv: f64, yv: f64| -> MathResult<f64> {
        f.eval_f64(&Point {
            names: [x, y],
            values: [xv, yv],
        })
    };
    let mut out = Vec::with_capacity(steps + 1);
    let (mut xv, mut yv) = (x0, y0);
    out.push((xv, yv));
    for i in 0..steps {
        let k1 = eval(xv, yv)?;
        let k2 = eval(xv + h / 2.0, yv + h * k1 / 2.0)?;
        let k3 = eval(xv + h / 2.0, yv + h * k2 / 2.0)?;
        let k4 = eval(xv + h, yv + h * k3)?;
        yv += h * (k1 + 2.0 * k2 + 2.0 * k3 + k4) / 6.0;
        xv = x0 + h * (i + 1) as f64;
        if !yv.is_finite() {
            return Err(MathError::Undefined(format!(
                "the solution at {xv} (it blew up)"
            )));
        }
        out.push((xv, yv));
    }
    Ok(out)
}

/// RK4 for a system `yᵢ' = fᵢ(t, y₁, …, yₙ)`.
pub fn rk4_system(
    fs: &[Expr],
    t: &str,
    ys: &[&str],
    t0: f64,
    y0: &[f64],
    t1: f64,
    steps: usize,
) -> MathResult<Vec<(f64, Vec<f64>)>> {
    check_steps(steps)?;
    if fs.len() != ys.len() || y0.len() != ys.len() || ys.is_empty() {
        return Err(MathError::Domain {
            function: "rk4".into(),
            detail: "give one equation and one initial value per unknown".into(),
        });
    }
    let mut names: Vec<&str> = vec![t];
    names.extend_from_slice(ys);
    let eval = |tv: f64, yv: &[f64]| -> MathResult<Vec<f64>> {
        let mut values = vec![tv];
        values.extend_from_slice(yv);
        let env = Vector {
            names: &names,
            values: &values,
        };
        fs.iter().map(|f| f.eval_f64(&env)).collect()
    };
    let n = ys.len();
    let h = (t1 - t0) / steps as f64;
    let axpy = |a: &[f64], k: &[f64], s: f64| -> Vec<f64> {
        a.iter().zip(k).map(|(x, d)| x + s * d).collect()
    };
    let mut yv = y0.to_vec();
    let mut tv = t0;
    let mut out = Vec::with_capacity(steps + 1);
    out.push((tv, yv.clone()));
    for i in 0..steps {
        let k1 = eval(tv, &yv)?;
        let k2 = eval(tv + h / 2.0, &axpy(&yv, &k1, h / 2.0))?;
        let k3 = eval(tv + h / 2.0, &axpy(&yv, &k2, h / 2.0))?;
        let k4 = eval(tv + h, &axpy(&yv, &k3, h))?;
        for j in 0..n {
            yv[j] += h * (k1[j] + 2.0 * k2[j] + 2.0 * k3[j] + k4[j]) / 6.0;
        }
        tv = t0 + h * (i + 1) as f64;
        if yv.iter().any(|v| !v.is_finite()) {
            return Err(MathError::Undefined(format!(
                "the solution at {tv} (it blew up)"
            )));
        }
        out.push((tv, yv.clone()));
    }
    Ok(out)
}

/// The ODE domain: `dsolve`, `rk4`, `rk4table`.
pub struct Odes;

const OPS: &[&str] = &["dsolve", "rk4", "rk4table"];

impl Domain for Odes {
    fn name(&self) -> &'static str {
        "differential equations"
    }

    fn operators(&self) -> &'static [&'static str] {
        OPS
    }

    fn apply(&self, op: &str, args: &[Value], ctx: &mut Ctx<'_>) -> Option<MathResult<Value>> {
        Some(match op {
            "dsolve" => dsolve_op(args, ctx),
            "rk4" | "rk4table" => rk4_op(op, args, ctx),
            _ => return None,
        })
    }
}

/// `dsolve(eq, y, x)` or `dsolve(eq, y, x, (x0, y0))`.
fn dsolve_op(args: &[Value], ctx: &mut Ctx<'_>) -> MathResult<Value> {
    expect_args_between("dsolve", args, 3, 4)?;
    let y = sym_arg("dsolve", &args[1])?;
    let x = sym_arg("dsolve", &args[2])?;
    let ode = first_order(&args[0], &x, &y)?;
    let initial = match args.get(3).map(Expr::node) {
        Some(ExprNode::Tuple(v)) if v.len() == 2 => Some((&v[0], &v[1])),
        Some(_) => {
            return Err(MathError::Domain {
                function: "dsolve".into(),
                detail: "the initial condition is a pair (x0, y0)".into(),
            })
        }
        None => None,
    };
    let s = solve_first_order(&ode, initial, ctx)?;
    if let Some(c) = &s.constant {
        ctx.note(format!("{c} is an arbitrary constant"));
    }
    Ok(s.solution)
}

/// `rk4(f, x, y, x0, y0, x1, steps)`: `y(x1)`; `rk4table` returns every point.
fn rk4_op(op: &str, args: &[Value], ctx: &mut Ctx<'_>) -> MathResult<Value> {
    expect_args_between(op, args, 6, 7)?;
    let x = sym_arg(op, &args[1])?;
    let y = sym_arg(op, &args[2])?;
    let ode = first_order(&args[0], &x, &y)?;
    let x0 = real_arg(op, &args[3])?;
    let y0 = real_arg(op, &args[4])?;
    let x1 = real_arg(op, &args[5])?;
    let steps = match args.get(6) {
        Some(n) => usize::try_from(crate::domains::util::int_arg(op, n)?).map_err(|_| {
            MathError::Domain {
                function: op.into(),
                detail: "the number of steps must be positive".into(),
            }
        })?,
        None => 100,
    };
    let pts = rk4(&ode.rhs, &x, &y, x0, y0, x1, steps)?;
    ctx.note(format!(
        "RK4 with {steps} steps of h = {}",
        (x1 - x0) / steps as f64
    ));
    if op == "rk4" {
        return Ok(Expr::float(pts.last().map_or(y0, |p| p.1)));
    }
    if pts.len() > MAX_TABLE {
        return Err(MathError::TooLarge(format!(
            "a table of {} rows",
            pts.len()
        )));
    }
    let rows = pts
        .into_iter()
        .map(|(a, b)| vec![Expr::float(a), Expr::float(b)])
        .collect();
    Ok(crate::domains::linalg::Matrix::from_rows(rows)?.to_expr())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(n: &str) -> Expr {
        Expr::sym(n)
    }

    fn dy() -> Expr {
        Expr::new(ExprNode::Derivative {
            expr: s("y"),
            vars: vec![(std::sync::Arc::from("x"), 1)],
        })
    }

    /// Checks y' − f(x, y) ≈ 0 for an explicit solution, with C = 1.3.
    fn check(sol: &Expr, rhs: &Expr) {
        let ExprNode::Relation(_, l, r) = sol.node() else {
            panic!()
        };
        assert!(l.is_sym("y"), "not explicit: {sol}");
        let calc = crate::domains::BasicCalculus;
        use crate::domains::Calculus;
        let d = calc.diff(r, "x").unwrap();
        for xv in [0.2, 0.9, 1.7] {
            let yv = r.eval_f64(&[("x", xv), ("C", 1.3)]).unwrap();
            let lhs = d.eval_f64(&[("x", xv), ("C", 1.3)]).unwrap();
            let want = rhs.eval_f64(&[("x", xv), ("y", yv)]).unwrap();
            assert!((lhs - want).abs() < 1e-9, "{sol}: {lhs} vs {want}");
        }
    }

    #[test]
    fn separable_equations() {
        let mut ctx = Ctx::new();
        let eq = Expr::equation(dy(), s("x") * s("y"));
        let ode = first_order(&eq, "x", "y").unwrap();
        let sol = solve_first_order(&ode, None, &mut ctx).unwrap();
        assert_eq!(sol.method, "separable");
        check(&sol.solution, &ode.rhs);
        let eq = Expr::equation(dy(), s("x").pow(2) * s("y").pow(2));
        let ode = first_order(&eq, "x", "y").unwrap();
        let sol = solve_first_order(&ode, None, &mut ctx).unwrap();
        check(&sol.solution, &ode.rhs);
    }

    #[test]
    fn linear_equations() {
        let mut ctx = Ctx::new();
        // y' + y = x
        let eq = Expr::equation(dy() + s("y"), s("x"));
        let ode = first_order(&eq, "x", "y").unwrap();
        let sol = solve_first_order(&ode, None, &mut ctx).unwrap();
        assert_eq!(sol.method, "linear (integrating factor)");
        check(&sol.solution, &ode.rhs);
        // y' = 2y + e^x with y(0) = 1: y = 2e^{2x} − e^x
        let eq = Expr::equation(dy(), Expr::int(2) * s("y") + Expr::e().pow(s("x")));
        let ode = first_order(&eq, "x", "y").unwrap();
        let sol = solve_first_order(&ode, Some((&Expr::zero(), &Expr::one())), &mut ctx).unwrap();
        let ExprNode::Relation(_, _, r) = sol.solution.node() else {
            panic!()
        };
        let v = r.eval_f64(&[("x", 1.0)]).unwrap();
        let e = std::f64::consts::E;
        assert!((v - (2.0 * e * e - e)).abs() < 1e-9, "{}", sol.solution);
        assert!(sol.constant.is_none());
    }

    #[test]
    fn rk4_matches_exp() {
        let pts = rk4(&s("y"), "x", "y", 0.0, 1.0, 1.0, 100).unwrap();
        assert!((pts[100].1 - std::f64::consts::E).abs() < 1e-8);
        // Harmonic oscillator: x' = v, v' = −x.
        let sys = rk4_system(
            &[s("v"), -s("x")],
            "t",
            &["x", "v"],
            0.0,
            &[1.0, 0.0],
            1.0,
            200,
        )
        .unwrap();
        let last = &sys.last().unwrap().1;
        assert!((last[0] - 1f64.cos()).abs() < 1e-9);
        assert!(rk4(&s("y"), "x", "y", 0.0, 1.0, 1.0, 0).is_err());
    }

    #[test]
    fn domain_operators() {
        let mut ctx = Ctx::new();
        let eq = Expr::equation(dy(), Expr::int(3) * s("y"));
        let r = Odes
            .apply("dsolve", &[eq, s("y"), s("x")], &mut ctx)
            .unwrap()
            .unwrap();
        assert_eq!(
            r,
            Expr::equation(s("y"), s("C") * Expr::e().pow(Expr::int(3) * s("x")))
        );
        let r = Odes
            .apply(
                "rk4",
                &[
                    s("y"),
                    s("x"),
                    s("y"),
                    Expr::zero(),
                    Expr::one(),
                    Expr::one(),
                ],
                &mut ctx,
            )
            .unwrap()
            .unwrap();
        assert!((r.eval_f64(&crate::eval::EmptyEnv).unwrap() - std::f64::consts::E).abs() < 1e-7);
    }
}
