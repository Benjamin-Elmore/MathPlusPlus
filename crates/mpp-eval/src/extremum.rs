//! `\max_{t \in [a, b]} f(t)` and `\min_{t \in [a, b]} f(t)`: the largest or smallest
//! value of an expression over an interval, found numerically (dense sampling, then a
//! golden-section refinement around the best sample). The expression may use
//! numerical solutions of differential equations (see [`crate::ivp_wire`]).

use mpp_core::ode::ivp::{Ivp, IVP};
use mpp_core::{EmptyEnv, Expr, ExprNode, MathError, MathResult, Name};

use crate::holders::Ctx;

/// `max over(f, t, a, b)`.
pub const MAX_OVER: &str = "max over";
/// `min over(f, t, a, b)`.
pub const MIN_OVER: &str = "min over";

/// Samples across the interval before refining.
const SAMPLES: usize = 2000;

/// The variable bound by the operator (masked while its body is resolved).
pub fn bound_names(name: &str, args: &[Expr]) -> Vec<Name> {
    match (name, args) {
        (MAX_OVER | MIN_OVER, [_, var, ..]) => var.as_sym().map(Name::from).into_iter().collect(),
        _ => Vec::new(),
    }
}

/// Evaluates `max over` / `min over` once the bounds are numbers.
pub fn evaluate(e: &Expr, ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
    let ExprNode::Apply(name, args) = e.node() else {
        return None;
    };
    let largest = match &**name {
        MAX_OVER => true,
        MIN_OVER => false,
        _ => return None,
    };
    let [body, var, lo, hi] = args.as_slice() else {
        return None;
    };
    let var = var.as_sym()?;
    let bound = |b: &Expr| {
        let b = b.simplify();
        if !b.is_constant() || crate::units_wire::contains_units(&b) {
            return None;
        }
        b.eval_f64(&EmptyEnv).ok().filter(|v| v.is_finite())
    };
    let (Some(a), Some(b)) = (bound(lo), bound(hi)) else {
        return Some(Err(MathError::Unsupported(
            "a maximum over an interval whose ends are not plain numbers".into(),
        )));
    };
    if a > b {
        return Some(Err(MathError::Domain {
            function: if largest { "max" } else { "min" }.into(),
            detail: "the interval is empty".into(),
        }));
    }
    ctx.note(format!(
        "found numerically: {SAMPLES} samples over [{a}, {b}], then refined"
    ));
    Some(extremum(body, var, a, b, largest).map(Expr::float))
}

/// The value of `body` at `x`; `None` where it is not a finite number.
fn value(body: &Expr, var: &str, x: f64) -> Option<f64> {
    let at = with_solutions(&body.subs(var, &Expr::float(x)))?.simplify();
    if crate::units_wire::contains_units(&at) {
        return None;
    }
    at.eval_f64(&EmptyEnv).ok().filter(|v| v.is_finite())
}

/// `e` with every numerical solution at a number replaced by its value.
fn with_solutions(e: &Expr) -> Option<Expr> {
    if let ExprNode::Apply(name, args) = e.node() {
        if &**name == IVP {
            let (sys, k, t) = Ivp::read(args)?;
            let t = with_solutions(&t)?.simplify().eval_f64(&EmptyEnv).ok()?;
            let y = crate::ivp_wire::value_at(&sys, t).ok()?;
            return Some(Expr::float(y[k]));
        }
    }
    let mut ok = true;
    let node = e.map_children(&mut |c| match with_solutions(c) {
        Some(v) => v,
        None => {
            ok = false;
            c.clone()
        }
    });
    ok.then(|| Expr::new(node))
}

fn extremum(body: &Expr, var: &str, a: f64, b: f64, largest: bool) -> MathResult<f64> {
    let sign = if largest { 1.0 } else { -1.0 };
    let f = |x: f64| value(body, var, x).map(|v| sign * v);
    if a == b {
        return f(a)
            .map(|v| sign * v)
            .ok_or_else(|| MathError::NotANumber(body.to_string()));
    }
    let step = (b - a) / SAMPLES as f64;
    let mut best: Option<(usize, f64)> = None;
    let mut missing = 0;
    for i in 0..=SAMPLES {
        match f(a + step * i as f64) {
            Some(v) => {
                if best.is_none_or(|(_, w)| v > w) {
                    best = Some((i, v));
                }
            }
            None => missing += 1,
        }
    }
    // A value that cannot be computed on much of the interval is no basis for a
    // maximum.
    if missing * 10 > SAMPLES {
        return Err(MathError::NotANumber(format!(
            "{body} on much of the interval"
        )));
    }
    let (i, mut top) = best.ok_or_else(|| MathError::NotANumber(body.to_string()))?;
    // Golden-section search between the neighbouring samples.
    let (mut lo, mut hi) = (
        a + step * i.saturating_sub(1) as f64,
        (a + step * (i + 1) as f64).min(b),
    );
    let g = (5f64.sqrt() - 1.0) / 2.0;
    for _ in 0..80 {
        let (x1, x2) = (hi - g * (hi - lo), lo + g * (hi - lo));
        match (f(x1), f(x2)) {
            (Some(v1), Some(v2)) => {
                top = top.max(v1).max(v2);
                if v1 < v2 {
                    lo = x1;
                } else {
                    hi = x2;
                }
            }
            _ => break,
        }
        if hi - lo <= 1e-12 * (b - a) {
            break;
        }
    }
    Ok(sign * top)
}
