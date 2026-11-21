//! The registry's form of the classical Runge–Kutta method: `rk4(f, x, y, x₀, y₀, x₁, n)`
//! (`y(x₁)`) and `rk4table` (every point), for a first-order equation `y' = f(x, y)` in
//! any arrangement, plus RK4 for systems. The steps are taken by the one RK4 stepper,
//! [`rk4_march`](crate::tools_b::numerics::rk4_march). Equations are solved exactly by the
//! differential-equations toolkit (`tools_b::ode`), the only `dsolve`.

use crate::domains::util::{expect_args_between, is_zero, real_arg, sym_arg};
use crate::domains::{Ctx, Domain, Value};
use crate::error::{MathError, MathResult};
use crate::eval::Env;
use crate::expr::{Expr, ExprNode};

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
/// point, the initial one included. The steps are taken by
/// [`rk4_march`](crate::tools_b::numerics::rk4_march), the one RK4 stepper.
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
    crate::tools_b::numerics::rk4_march(
        eval,
        x0,
        y0,
        h,
        steps,
        |_, _, _, _| {},
        |xv| MathError::Undefined(format!("the solution at {xv} (it blew up)")),
    )
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
        crate::budget::check()?;
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

/// The ODE domain: `rk4`, `rk4table`.
pub struct Odes;

const OPS: &[&str] = &["rk4", "rk4table"];

impl Domain for Odes {
    fn name(&self) -> &'static str {
        "differential equations"
    }

    fn operators(&self) -> &'static [&'static str] {
        OPS
    }

    fn apply(&self, op: &str, args: &[Value], ctx: &mut Ctx<'_>) -> Option<MathResult<Value>> {
        Some(match op {
            "rk4" | "rk4table" => rk4_op(op, args, ctx),
            _ => return None,
        })
    }
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
        let eq = Expr::equation(dy(), s("y"));
        let r = Odes
            .apply(
                "rk4",
                &[eq, s("x"), s("y"), Expr::zero(), Expr::one(), Expr::one()],
                &mut ctx,
            )
            .unwrap()
            .unwrap();
        assert!((r.eval_f64(&crate::eval::EmptyEnv).unwrap() - std::f64::consts::E).abs() < 1e-7);
    }
}
