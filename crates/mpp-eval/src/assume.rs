//! Assumptions in documents: the calculus that depends on facts about names.
//!
//! `Assume $x > 0$.` (and a bracketed `(x > 0)` after a query) put facts in force; the
//! block runner collects them into the [`Ctx`] of each statement. This holder answers
//! what changes with those facts, before the plain calculus is asked:
//! - `∫ c·xⁿ dx` with a symbolic `n`: split on `n = −1` unless the facts decide it;
//! - definite integrals over an infinite range and limits at ±∞ whose value depends on
//!   the sign of a parameter (`∫_1^∞ x^{−p} dx`, `lim_{x→∞} e^{a x}`): the facts decide,
//!   or the answer is piecewise over the cases.
//!
//! Everything else falls through to [`crate::calculus`]. The simplifications that need
//! facts (`√(x²) = x` for `x ≥ 0` …) run on the final value (see the block runner).

use std::cmp::Ordering;

use mpp_core::domains::assume::{
    improper_integral, integrate_power, limit_at_infinity, simplify_with,
};
use mpp_core::domains::{util, Assumptions};
use mpp_core::{add, simplify, Expr, ExprNode, Func, MathError, MathResult};

use crate::holders::Ctx;

/// Evaluates the holders whose answer depends on assumptions.
pub fn evaluate(e: &Expr, ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
    let none = Assumptions::new();
    let facts = ctx.assumptions().unwrap_or(&none);
    match e.node() {
        ExprNode::Integral {
            expr,
            var,
            bounds: None,
        } if !matches!(expr.node(), ExprNode::Integral { .. }) => {
            let f = integrate_power(expr, var, facts)?;
            Some(Ok(with_constant(ctx, e, &f)))
        }
        ExprNode::Integral {
            expr,
            var,
            bounds: Some((lo, hi)),
        } if !matches!(expr.node(), ExprNode::Integral { .. }) => {
            if lo.is_infinite() || hi.is_infinite() {
                improper(e, expr, var, lo, hi, facts, ctx)
            } else {
                let f = integrate_power(expr, var, facts)?;
                let v = simplify_with(&definite(&f, var, lo, hi), facts, None);
                ctx.log("evaluate the antiderivative at the bounds", e, &v, None);
                Some(Ok(v))
            }
        }
        ExprNode::Limit {
            expr, var, point, ..
        } if point.is_infinite() && has_parameters(expr, var) => {
            let negative = !is_plus_infinity(point);
            match limit_at_infinity(expr, var, negative, facts) {
                Ok(Some(v)) => {
                    explain_split(ctx, &v);
                    ctx.log("limit, using the signs of the parameters", e, &v, None);
                    Some(Ok(v))
                }
                Ok(None) => None,
                Err(err) => Some(Err(err)),
            }
        }
        _ => None,
    }
}

/// `+∞` (as opposed to `−∞`, which is `−1 · ∞`).
fn is_plus_infinity(e: &Expr) -> bool {
    matches!(e.node(), ExprNode::Const(mpp_core::Constant::Infinity))
}

/// True when `e` mentions a name other than `var`.
fn has_parameters(e: &Expr, var: &str) -> bool {
    e.free_symbols().iter().any(|s| &**s != var)
}

fn improper(
    whole: &Expr,
    body: &Expr,
    var: &str,
    lo: &Expr,
    hi: &Expr,
    facts: &Assumptions,
    ctx: &Ctx<'_>,
) -> Option<MathResult<Expr>> {
    if !has_parameters(body, var) || !continuous_on(body, var, lo, hi) {
        return None;
    }
    match improper_integral(body, var, lo, hi, facts) {
        Ok(Some(v)) => {
            if v.is_infinite() {
                return Some(Err(MathError::Divergent(format!("the integral {whole}"))));
            }
            explain_split(ctx, &v);
            ctx.log(
                "integrate, then take the limit at the infinite bound",
                whole,
                &v,
                None,
            );
            Some(Ok(v))
        }
        Ok(None) => None,
        Err(err) => Some(Err(err)),
    }
}

/// Notes a piecewise answer: which cases there are, and that ∞ means divergence.
fn explain_split(ctx: &Ctx<'_>, v: &Expr) {
    if let ExprNode::Piecewise { cases, otherwise } = v.node() {
        ctx.note(
            "the answer depends on a parameter; add an assumption (Assume $…$.) to pick a case",
        );
        let infinite = cases.iter().any(|(c, _)| c.is_infinite())
            || otherwise.as_ref().is_some_and(Expr::is_infinite);
        if infinite {
            ctx.note("∞ marks the cases where it diverges");
        }
    }
}

/// Conservative check that the antiderivative route is valid on the whole range: the
/// integrand may only have powers and logarithms of the variable where the finite bound
/// is positive (for a range up to +∞), and exponentials anywhere.
fn continuous_on(e: &Expr, var: &str, lo: &Expr, hi: &Expr) -> bool {
    let up = is_plus_infinity(hi);
    let positive_start = up && util::sign(lo) == Some(Ordering::Greater);
    nice(e, var, positive_start)
}

fn nice(e: &Expr, var: &str, positive: bool) -> bool {
    if !e.contains_sym(var) {
        return true;
    }
    match e.node() {
        ExprNode::Sym(_) => true,
        ExprNode::Add(v) | ExprNode::Mul(v) => v.iter().all(|t| nice(t, var, positive)),
        ExprNode::Pow(b, k) if !k.contains_sym(var) => {
            let natural = k
                .as_number()
                .and_then(|n| n.as_integer())
                .is_some_and(|n| n >= 0u32.into());
            (natural && nice(b, var, positive)) || (positive && b.is_sym(var))
        }
        ExprNode::Pow(b, g) if !b.contains_sym(var) => nice(g, var, positive),
        ExprNode::Func(Func::Ln, args) => positive && args.iter().all(|a| a.is_sym(var)),
        ExprNode::Func(Func::Sin | Func::Cos, args) => args.iter().all(|a| nice(a, var, positive)),
        _ => false,
    }
}

/// `F(hi) − F(lo)`, case by case for a piecewise antiderivative.
fn definite(f: &Expr, var: &str, lo: &Expr, hi: &Expr) -> Expr {
    let at = |g: &Expr| (g.subs(var, hi) - g.subs(var, lo)).simplify();
    match f.node() {
        ExprNode::Piecewise { cases, otherwise } => Expr::new(ExprNode::Piecewise {
            cases: cases.iter().map(|(g, c)| (at(g), c.clone())).collect(),
            otherwise: otherwise.as_ref().map(at),
        }),
        _ => at(f),
    }
}

/// `F + C`, shown with the constant last.
fn with_constant(ctx: &Ctx<'_>, whole: &Expr, f: &Expr) -> Expr {
    let c = if f.contains_sym("C") { "C_1" } else { "C" };
    let value = simplify(&add(vec![f.clone(), Expr::sym(c)]));
    ctx.show_with_constant(&value, f, c);
    let note = matches!(f.node(), ExprNode::Piecewise { .. })
        .then(|| "the power rule needs n ≠ −1; n = −1 gives the logarithm".to_string());
    if note.is_some() {
        ctx.note("the answer depends on whether the exponent is −1; add an assumption such as Assume $n \\neq -1$. to pick a case");
    }
    ctx.log("integrate the power, case by case", whole, &value, note);
    value
}
