//! Integration: indefinite integrals by rules, definite and improper integrals, numerical
//! quadrature, and iterated (double, triple) and line integrals.
//!
//! The symbolic engine tries, in order: the constant rule, linearity, the table of standard
//! integrals (with linear inner functions), partial fractions, quadratic and square-root forms
//! (trigonometric substitution), trigonometric forms, product-to-sum identities,
//! u-substitution, integration by parts (including the cyclic case), expansion and rewriting
//! hyperbolic functions with exponentials. Every technique that fires records a step.
//!
//! Every antiderivative is differentiated back and compared with the integrand before it is
//! returned, so the engine reports [`MathError::NoClosedForm`] or
//! [`MathError::Inconclusive`] rather than give a wrong answer.

mod change;
mod definite;
mod engine;
mod multiple;
mod numeric;
mod oscillatory;
mod parts;
mod radical;
pub(crate) mod rational;
mod subst;
mod table;
mod trig;
mod verify;

#[cfg(test)]
mod tests;

use std::fmt;
use std::sync::Arc;

use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode};
use crate::simplify::{add, simplify};
use crate::steps::Steps;

pub use definite::integrate_definite;
pub use multiple::{integrate_multiple, line_integral, line_integral_scalar};
pub use numeric::{numeric_integrate, DEFAULT_TOLERANCE};

use engine::Integrator;

/// An antiderivative together with the name of its constant of integration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntegralResult {
    /// One antiderivative `F` (with `F′ = f`).
    pub antiderivative: Expr,
    /// The name used for the arbitrary constant (normally `C`).
    pub constant: String,
}

impl IntegralResult {
    /// `F + C` as an expression.
    pub fn with_constant(&self) -> Expr {
        add(vec![self.antiderivative.clone(), Expr::sym(&self.constant)])
    }
}

impl fmt::Display for IntegralResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.antiderivative.is_zero() {
            write!(f, "{}", self.constant)
        } else {
            write!(f, "{} + {}", self.antiderivative, self.constant)
        }
    }
}

/// The unevaluated integral `∫ e d(var)` (used in the steps).
pub(crate) fn holder(e: &Expr, var: &str) -> Expr {
    Expr::new(ExprNode::Integral {
        expr: e.clone(),
        var: Arc::from(var),
        bounds: None,
    })
}

/// The unevaluated integral `∫_a^b e d(var)`.
pub(crate) fn definite_holder(e: &Expr, var: &str, a: &Expr, b: &Expr) -> Expr {
    Expr::new(ExprNode::Integral {
        expr: e.clone(),
        var: Arc::from(var),
        bounds: Some((a.clone(), b.clone())),
    })
}

/// True for nodes that are not scalar expressions (relations, matrices, sets …).
fn is_scalar(e: &Expr) -> bool {
    !matches!(
        e.node(),
        ExprNode::Relation(..)
            | ExprNode::Bool(_)
            | ExprNode::Logic(..)
            | ExprNode::Matrix { .. }
            | ExprNode::Set(_)
            | ExprNode::Interval { .. }
            | ExprNode::Tuple(_)
    )
}

/// An antiderivative of `expr` with respect to `var` (without the constant; see
/// [`integrate_with_constant`] for `F + C`).
///
/// Each technique records a step in `steps`. The answer is checked by differentiating it;
/// when no rule applies the error is [`MathError::NoClosedForm`], and an answer that fails its
/// check is [`MathError::Inconclusive`].
pub fn integrate(expr: &Expr, var: &str, steps: Option<&mut Steps>) -> MathResult<Expr> {
    let e = simplify(expr);
    if !is_scalar(&e) {
        return Err(MathError::NotANumber(format!("{e}")));
    }
    if e.is_undefined() || e.is_infinite() {
        return Err(MathError::Undefined(format!("the integrand {e}")));
    }
    let mut local = Steps::new();
    let mut engine = Integrator::new(var);
    let found = engine
        .int(&e, &mut local)
        .ok_or_else(|| MathError::NoClosedForm(format!("∫ {e} d{var}")))?;
    let found = simplify(&found);
    verify::check_antiderivative(&e, &found, var)?;
    if let Some(log) = steps {
        // Show a cancellation or tidy-up the techniques started from, such as
        // (x + 1)/(x² + 3x + 2) = 1/(x + 2).
        if e.to_string() != expr.to_string() {
            log.push(
                "simplify the integrand",
                holder(expr, var),
                holder(&e, var),
                None,
            );
        }
        log.0.extend(local.0);
    }
    Ok(found)
}

/// Like [`integrate`], but returns the antiderivative with its constant of integration.
pub fn integrate_with_constant(
    expr: &Expr,
    var: &str,
    steps: Option<&mut Steps>,
) -> MathResult<IntegralResult> {
    let mut log = steps;
    let e = simplify(expr);
    let f = integrate(&e, var, log.as_deref_mut())?;
    let constant = constant_name(&e, var);
    let result = IntegralResult {
        antiderivative: f,
        constant,
    };
    Steps::record(
        &mut log,
        "add the constant of integration",
        &holder(&e, var),
        &result.with_constant(),
        Some(format!("{} is an arbitrary constant", result.constant)),
    );
    Ok(result)
}

/// `C`, or `C_1`, `C_2` … when `C` is already in use.
fn constant_name(e: &Expr, var: &str) -> String {
    let used = |name: &str| name == var || e.contains_sym(name);
    if !used("C") {
        return "C".into();
    }
    (1..)
        .map(|k| format!("C_{k}"))
        .find(|n| !used(n))
        .unwrap_or_else(|| "C".into())
}
