//! The numerical methods as operators: Riemann sums and the quadrature rules, `newton`,
//! `euler` and `rk4`, `taylorbound` and `termsneeded`, answered by
//! [`mpp_core::numerics`].

use mpp_core::number::format_f64;
use mpp_core::numerics::{self, QuadratureRule};
use mpp_core::{Expr, MathError, MathResult};

use super::operator_args::*;
use crate::holders::Ctx;

pub(super) fn quadrature(call: &Call, ctx: &Ctx<'_>) -> MathResult<Expr> {
    let p = call.parts();
    let rule = match call.name.as_str() {
        "leftsum" => QuadratureRule::Left,
        "rightsum" => QuadratureRule::Right,
        "midsum" | "midpoint" => QuadratureRule::Midpoint,
        "trapezoid" | "trapezoidal" => QuadratureRule::Trapezoid,
        "simpson" => QuadratureRule::Simpson,
        _ => match p
            .word(&["left", "right", "mid", "midpoint", "trapezoid", "trapezoidal", "simpson"])
            .as_deref()
        {
            Some("left") => QuadratureRule::Left,
            Some("right") => QuadratureRule::Right,
            Some("mid" | "midpoint") => QuadratureRule::Midpoint,
            Some("trapezoid" | "trapezoidal") => QuadratureRule::Trapezoid,
            Some("simpson") => QuadratureRule::Simpson,
            _ => {
                return Err(missing(
                    "riemann",
                    "the kind of sum: write riemann_{left}, riemann_{right}, riemann_{\\mathrm{mid}} or use midpoint, trapezoid or simpson",
                ))
            }
        },
    };
    let var = p.variable(rule.name())?;
    let (a, b) = p.interval(rule.name())?;
    let f = p
        .functions
        .first()
        .ok_or_else(|| missing(rule.name(), "a function"))?;
    let n = p
        .numbers
        .first()
        .ok_or_else(|| missing(rule.name(), "the number of subintervals n"))?;
    let n = to_count(n, rule.name())?;
    let q = logged(ctx, |s| {
        numerics::quadrature_rule(f, &var, &a, &b, n, rule, s)
    })?;
    if let Some(bound) = q.error_bound {
        ctx.note(format!(
            "{} with n = {n}: error bound |E| ≤ {}",
            rule.name(),
            format_f64(bound, 4)
        ));
    }
    Ok(q.value)
}

pub(super) fn newton(call: &Call, ctx: &Ctx<'_>) -> MathResult<Expr> {
    let p = call.parts();
    let var = p.variable("Newton's method")?;
    let f = match (p.functions.first(), p.relations.first()) {
        (Some(f), _) => f.clone(),
        (None, Some((l, r))) => mpp_core::simplify(&(l - r)),
        _ => return Err(missing("Newton's method", "a function")),
    };
    let x0 = p
        .numbers
        .first()
        .ok_or_else(|| missing("Newton's method", "a starting point x₀"))?;
    let x0 = to_f64(x0, "the starting point")?;
    let tol = match p.numbers.get(1) {
        Some(t) => to_f64(t, "the tolerance")?,
        None => 1e-12,
    };
    let (root, table) = logged(ctx, |s| numerics::newton(&f, &var, x0, tol, s))?;
    ctx.note(format!(
        "Newton's method from x₀ = {} converged in {} iterations",
        format_f64(x0, 10),
        table.len()
    ));
    Ok(Expr::float(root))
}

pub(super) fn stepper(call: &Call, ctx: &Ctx<'_>) -> MathResult<Expr> {
    let p = call.parts();
    let rk = call.name != "euler";
    let what = if rk { "RK4" } else { "Euler's method" };
    let rhs = match (p.functions.first(), p.relations.first()) {
        (Some(f), _) => f.clone(),
        (None, Some((_, r))) => r.clone(),
        _ => return Err(missing(what, "the slope f(x, y) of y′ = f(x, y)")),
    };
    let free: Vec<String> = rhs.free_symbols().iter().map(|s| s.to_string()).collect();
    let x = if free.iter().any(|s| s == "t") && !free.iter().any(|s| s == "x") {
        "t"
    } else {
        "x"
    };
    let y = "y";
    // Numbers: x₀, y₀, h, n — or a tuple (x₀, y₀) followed by h, n.
    let mut nums: Vec<Expr> = Vec::new();
    if let Some(t) = p.tuples.first() {
        nums.extend(t.iter().cloned());
    }
    nums.extend(p.numbers.iter().cloned());
    let [x0, y0, h, n] = nums.as_slice() else {
        return Err(missing(
            what,
            "x₀, y₀, the step h and the number of steps n",
        ));
    };
    let (x0, y0, h) = (
        to_f64(x0, "x₀")?,
        to_f64(y0, "y₀")?,
        to_f64(h, "the step h")?,
    );
    let n = to_count(n, what)?;
    let table = logged(ctx, |s| {
        if rk {
            numerics::rk4(&rhs, x, y, x0, y0, h, n, s)
        } else {
            numerics::euler(&rhs, x, y, x0, y0, h, n, s)
        }
    })?;
    let (xn, yn) = table.rows.last().copied().unwrap_or((x0, y0));
    ctx.note(format!(
        "{} with h = {}: y({}) ≈ {} after {n} steps",
        table.method,
        format_f64(h, 6),
        format_f64(xn, 8),
        format_f64(yn, 10)
    ));
    Ok(Expr::float(yn))
}

pub(super) fn taylor_parts(
    call: &Call,
    what: &str,
) -> MathResult<(Expr, String, Expr, Expr, Expr)> {
    let p = call.parts();
    let var = p.variable(what)?;
    let f = p
        .functions
        .first()
        .ok_or_else(|| missing(what, "a function"))?
        .clone();
    let [a, x, last] = p.numbers.as_slice() else {
        return Err(missing(
            what,
            "the centre a, the point x and the degree (or ε)",
        ));
    };
    Ok((f, var, a.clone(), x.clone(), last.clone()))
}

/// `taylorbound(f, a, n, x)` (the Insert menu's order: centre, degree, point), or with
/// the point written as a condition, `taylorbound(f, a, n, x = 1)`.
pub(super) fn taylor_bound(call: &Call, ctx: &Ctx<'_>) -> MathResult<Expr> {
    let what = "Taylor bound";
    let p = call.parts();
    let var = p.variable(what)?;
    let f = p
        .functions
        .first()
        .ok_or_else(|| missing(what, "a function"))?
        .clone();
    let at = p
        .relations
        .iter()
        .find(|(l, _)| l.as_sym() == Some(var.as_str()))
        .map(|(_, r)| r.clone());
    let (a, n, x) = match (p.numbers.as_slice(), at) {
        ([a, n], Some(x)) => (a.clone(), n.clone(), x),
        ([a, n, x], None) => (a.clone(), n.clone(), x.clone()),
        _ => {
            return Err(missing(
                what,
                "the centre a, the degree n and the point x, in that order",
            ))
        }
    };
    let n = to_count(&mpp_core::simplify(&(n + Expr::one())), "the degree")? - 1;
    let n = u32::try_from(n).map_err(|_| MathError::TooLarge("the degree".into()))?;
    let tb = logged(ctx, |s| numerics::taylor_bound(&f, &var, &a, &x, n, s))?;
    Ok(Expr::float(tb.bound))
}

pub(super) fn taylor_terms(call: &Call, ctx: &Ctx<'_>) -> MathResult<Expr> {
    let (f, var, a, x, eps) = taylor_parts(call, "Taylor terms")?;
    let eps = to_f64(&eps, "ε")?;
    let tb = logged(ctx, |s| {
        numerics::taylor_terms_needed(&f, &var, &a, &x, eps, s)
    })?;
    ctx.note(format!(
        "degree {} (the Lagrange bound is {} < {})",
        tb.degree,
        format_f64(tb.bound, 4),
        format_f64(eps, 4)
    ));
    Ok(Expr::int(i64::from(tb.degree)))
}
