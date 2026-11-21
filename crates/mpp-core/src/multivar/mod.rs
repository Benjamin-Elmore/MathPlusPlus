//! Multivariable and vector calculus helpers: partial derivatives and the tools built on
//! them ([`differential`]), integrals in other coordinates, over surfaces and the
//! integral theorems ([`integrals`]) and the vector operators ([`vector`]). Laplace
//! transforms live with the differential equations (`transforms::laplace`).
//!
//! Vectors and points are tuples of expressions; a vector field is a list of components
//! in the order of its variables.

pub mod differential;
pub mod integrals;
pub mod vector;

pub use differential::{
    directional_derivative_at, gradient_at, implicit_derivative, lagrange, linear_approximation,
    partial, related_rates, second_derivative_test, solve_rates, tangent_plane, Classified,
    LagrangePoint,
};
pub use integrals::{
    change_of_variables, check_divergence_theorem, check_greens_theorem, check_stokes_theorem,
    flux_integral, integrate_in, surface_integral, Coordinates, Patch, Region, Solid, TheoremCheck,
};
pub use vector::{curl, divergence, laplacian};

use crate::domains::util;
use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode};
use crate::simplify::{add, mul, simplify};

/// `a · b` for vectors of the same length.
pub fn dot(a: &[Expr], b: &[Expr]) -> MathResult<Expr> {
    if a.len() != b.len() {
        return Err(MathError::WrongArity {
            function: "the dot product".into(),
            expected: a.len().to_string(),
            got: b.len(),
        });
    }
    Ok(simplify(&add(a
        .iter()
        .zip(b)
        .map(|(x, y)| mul(vec![x.clone(), y.clone()]))
        .collect())))
}

/// `a × b` for 3-vectors.
pub fn cross(a: &[Expr], b: &[Expr]) -> MathResult<Vec<Expr>> {
    let ([a1, a2, a3], [b1, b2, b3]) = (a, b) else {
        return Err(MathError::WrongArity {
            function: "the cross product".into(),
            expected: "3".into(),
            got: a.len().min(b.len()),
        });
    };
    let c = |p: &Expr, q: &Expr, r: &Expr, s: &Expr| {
        simplify(&(p.clone() * q.clone() - r.clone() * s.clone()))
    };
    Ok(vec![
        c(a2, b3, a3, b2),
        c(a3, b1, a1, b3),
        c(a1, b2, a2, b1),
    ])
}

/// The components of a tuple (or a lone expression as a 1-vector).
pub fn components(e: &Expr) -> Vec<Expr> {
    match e.node() {
        ExprNode::Tuple(items) => items.clone(),
        ExprNode::Matrix { data, rows, cols } if *rows == 1 || *cols == 1 => data.clone(),
        _ => vec![e.clone()],
    }
}

/// A tuple of expressions.
pub fn tuple(items: Vec<Expr>) -> Expr {
    Expr::new(ExprNode::Tuple(items))
}

/// `e` with each `vars[i]` replaced by `values[i]` at once, simplified.
pub fn subs_all(e: &Expr, vars: &[&str], values: &[Expr]) -> Expr {
    let pairs: Vec<(&str, Expr)> = vars.iter().copied().zip(values.iter().cloned()).collect();
    simplify(&e.subs_all(&pairs))
}

/// True when two exact or numeric results agree.
pub fn agree(a: &Expr, b: &Expr) -> bool {
    if util::equal(a, b) {
        return true;
    }
    match (util::numeric(a), util::numeric(b)) {
        (Some(x), Some(y)) => (x - y).abs() <= 1e-7 * (1.0 + x.abs().max(y.abs())),
        _ => false,
    }
}

#[cfg(test)]
mod tests;
