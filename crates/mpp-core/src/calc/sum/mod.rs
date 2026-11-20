//! Sums and products: finite ones exactly (directly for small concrete ranges, by closed
//! forms for symbolic bounds), infinite ones by known closed forms (geometric, ζ(2k),
//! exponential, telescoping, rational terms via partial fractions and the digamma function),
//! otherwise numerically with an accelerated method once a named convergence test has shown
//! that the series converges. A divergent series is reported as [`MathError::Divergent`]
//! and never given a value.

mod antidiff;
mod convergence;
mod infinite;
mod numeric;
mod product;
mod rational;
mod special;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::error::{MathError, MathResult};
use crate::expr::{BigOpKind, Expr, ExprNode};
use crate::factor::factor;
use crate::number::Number;
use crate::simplify::{add, mul, simplify};
use crate::steps::Steps;

pub use convergence::{convergence, ConvergenceReport};
pub use product::product;

/// Most terms added one by one in exact arithmetic.
pub const MAX_DIRECT_TERMS: u64 = 2_000;
/// Most terms added in floating point for a long concrete range without a closed form.
pub const MAX_NUMERIC_TERMS: u64 = 2_000_000;

/// The unevaluated `Σ_{index=from}^{to} term` (or `Π`), used in the steps.
pub(crate) fn holder(kind: BigOpKind, term: &Expr, index: &str, from: &Expr, to: &Expr) -> Expr {
    Expr::new(ExprNode::BigOp {
        kind,
        expr: term.clone(),
        var: Arc::from(index),
        lower: from.clone(),
        upper: to.clone(),
    })
}

/// `e` with `k` replaced by `k + m`.
pub(crate) fn shift(e: &Expr, k: &str, m: i64) -> Expr {
    e.subs(k, &add(vec![Expr::sym(k), Expr::int(m)]))
}

/// Splits a product into the factors free of `k` and the rest.
pub(crate) fn split_constant(t: &Expr, k: &str) -> (Expr, Expr) {
    let factors = match t.node() {
        ExprNode::Mul(fs) => fs.clone(),
        _ => vec![t.clone()],
    };
    let (c, rest): (Vec<Expr>, Vec<Expr>) = factors.into_iter().partition(|f| !f.contains_sym(k));
    (mul(c), mul(rest))
}

/// The exact integer value of `e`, if it is one.
pub(crate) fn int_value(e: &Expr) -> Option<BigInt> {
    e.as_number().and_then(Number::as_integer)
}

pub(crate) fn is_pos_infinity(e: &Expr) -> bool {
    matches!(e.node(), ExprNode::Const(crate::expr::Constant::Infinity))
}

/// `t` at the integer `k = n`, refusing undefined values.
pub(crate) fn term_at(t: &Expr, k: &str, n: &BigInt) -> MathResult<Expr> {
    let v = t.subs(k, &Expr::bigint(n.clone()));
    if v.is_undefined() || v.is_infinite() {
        return Err(MathError::Undefined(format!("the term at {k} = {n}")));
    }
    Ok(v)
}

/// Floating-point value of a constant expression.
pub(crate) fn value_f64(e: &Expr) -> Option<f64> {
    e.eval_f64(&crate::eval::EmptyEnv)
        .ok()
        .filter(|v| v.is_finite())
}

/// Floating-point value of the term at `k = n`.
pub(crate) fn term_f64(t: &Expr, k: &str, n: f64) -> Option<f64> {
    t.eval_f64(&[(k, n)]).ok().filter(|v| v.is_finite())
}

/// Writes a result in a readable form: a single-variable rational function is factored.
pub(crate) fn tidy(e: &Expr) -> Expr {
    let e = simplify(e);
    let syms = e.free_symbols();
    if syms.len() != 1 {
        return e;
    }
    let Some(s) = syms.iter().next().map(|s| s.to_string()) else {
        return e;
    };
    let Some(r) = rational::as_ratfun(&e, &s) else {
        return e;
    };
    let num = factor(&r.num.to_expr(&s));
    if r.den.degree() == Some(0) {
        return num;
    }
    let den = factor(&r.den.to_expr(&s));
    simplify(&mul(vec![num, crate::simplify::pow(den, Expr::int(-1))]))
}

fn check_bounds(term: &Expr, index: &str, from: &Expr, to: &Expr) -> MathResult<()> {
    if from.contains_sym(index) || to.contains_sym(index) {
        return Err(MathError::Undefined(format!(
            "a bound that depends on the index {index}"
        )));
    }
    if from.is_undefined() || to.is_undefined() || term.is_undefined() {
        return Err(MathError::Undefined("this sum".into()));
    }
    if from.is_infinite() {
        return Err(MathError::Unsupported(
            "a sum or product that starts at −∞ or ∞".into(),
        ));
    }
    if to.is_infinite() && !is_pos_infinity(to) {
        return Err(MathError::Unsupported(
            "a sum or product that runs down to −∞".into(),
        ));
    }
    if let Some(n) = from.as_number() {
        if !n.is_integer() {
            return Err(MathError::Domain {
                function: "a sum or product".into(),
                detail: "the lower bound must be an integer".into(),
            });
        }
    }
    Ok(())
}

/// `Σ_{index=from}^{to} term`.
///
/// `to` may be `∞`. Finite sums are exact: small concrete ranges are added term by term,
/// other ranges use a closed form found by Faulhaber's formula (polynomials), geometric and
/// arithmetico-geometric sums, telescoping and partial fractions (with the digamma function
/// for harmonic-type blocks). A long concrete range with no closed form is added in floating
/// point and returned as an approximate number. Infinite sums first get a convergence
/// verdict ([`convergence`]); a divergent series is a [`MathError::Divergent`] naming the
/// test. Convergent ones use a closed form when one is known, otherwise an accelerated
/// numeric sum (an approximate number).
pub fn sum(
    term: &Expr,
    index: &str,
    from: &Expr,
    to: &Expr,
    steps: Option<&mut Steps>,
) -> MathResult<Expr> {
    let mut log = steps;
    let t = simplify(term);
    let (a, b) = (simplify(from), simplify(to));
    check_bounds(&t, index, &a, &b)?;
    if is_pos_infinity(&b) {
        return infinite::infinite_sum(&t, index, &a, &mut log);
    }
    finite_sum(&t, index, &a, &b, &mut log)
}

fn finite_sum(
    t: &Expr,
    k: &str,
    a: &Expr,
    b: &Expr,
    log: &mut Option<&mut Steps>,
) -> MathResult<Expr> {
    let shown = holder(BigOpKind::Sum, t, k, a, b);
    let concrete = int_value(a).zip(int_value(b));
    if let Some((lo, hi)) = &concrete {
        if hi < lo {
            Steps::record(
                log,
                "empty sum",
                &shown,
                &Expr::zero(),
                Some("the upper bound is below the lower bound".into()),
            );
            return Ok(Expr::zero());
        }
        let count = (hi - lo + 1u32).to_u64().unwrap_or(u64::MAX);
        if count <= MAX_DIRECT_TERMS {
            let mut terms = Vec::with_capacity(count as usize);
            let mut n = lo.clone();
            while &n <= hi {
                terms.push(term_at(t, k, &n)?);
                n += 1u32;
            }
            let value = simplify(&add(terms));
            Steps::record(
                log,
                "direct summation",
                &shown,
                &value,
                Some(format!("{count} terms added")),
            );
            return Ok(value);
        }
    }
    if let Some(ad) = antidiff::antidifference(t, k) {
        let upper = ad.expr.subs(k, &add(vec![b.clone(), Expr::one()]));
        let lower = ad.expr.subs(k, a);
        let value = tidy(&add(vec![upper, mul(vec![Expr::int(-1), lower])]));
        if !value.is_undefined() {
            Steps::record(
                log,
                ad.rule,
                &shown,
                &value,
                Some(format!(
                    "antidifference T({k}) = {}, sum = T(upper + 1) − T(lower)",
                    ad.expr
                )),
            );
            return Ok(value);
        }
    }
    if let Some((lo, hi)) = concrete {
        let count = (&hi - &lo + 1u32).to_u64().unwrap_or(u64::MAX);
        if count <= MAX_NUMERIC_TERMS && t.free_symbols().iter().all(|s| &**s == k) {
            let value = numeric::kahan_range(t, k, &lo, &hi)?;
            let out = Expr::float(value);
            Steps::record(
                log,
                "numeric summation",
                &shown,
                &out,
                Some(format!("{count} terms added in floating point")),
            );
            return Ok(out);
        }
        return Err(MathError::TooLarge(format!("a sum of {count} terms")));
    }
    Err(MathError::NoClosedForm(format!("{shown}")))
}

/// Splits off a sign factor `(−1)^(αk + β)` with odd `α`: returns `(sign, rest)`.
pub(crate) fn alternating_split(t: &Expr, k: &str) -> Option<(Expr, Expr)> {
    let factors = match t.node() {
        ExprNode::Mul(fs) => fs.clone(),
        _ => vec![t.clone()],
    };
    let mut sign = None;
    let mut rest = Vec::with_capacity(factors.len());
    for f in factors {
        if sign.is_none() {
            if let ExprNode::Pow(base, x) = f.node() {
                let minus_one = base.as_number().is_some_and(Number::is_minus_one);
                let odd_slope = crate::poly::as_poly(x, k).is_some_and(|c| {
                    c.len() == 2
                        && c[1]
                            .as_number()
                            .and_then(Number::as_integer)
                            .is_some_and(|s| num_integer::Integer::is_odd(&s))
                });
                if minus_one && odd_slope {
                    sign = Some(f.clone());
                    continue;
                }
            }
        }
        rest.push(f);
    }
    sign.map(|s| (s, mul(rest)))
}
