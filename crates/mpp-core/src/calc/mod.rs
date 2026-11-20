//! Calculus: derivatives, partial derivatives and their arrays, limits, integrals, sums and
//! products, series (Taylor, Laurent, Fourier) and recurrences.

pub mod diff;
pub mod fourier;
pub mod integrate;
pub mod limit;
pub mod recurrence;
pub mod series;
pub mod sum;

pub use diff::{derivative_holder, diff};
pub use fourier::{fourier_series, FourierSeries};
pub use integrate::{
    integrate, integrate_definite, integrate_multiple, integrate_with_constant, line_integral,
    line_integral_scalar, numeric_integrate, IntegralResult,
};
pub use limit::limit;
pub use recurrence::Recurrence;
pub use series::{series, Series};
pub use sum::{convergence, product, sum, ConvergenceReport};

use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode};
use crate::simplify::{add, mul, pow};

/// The gradient `(∂f/∂v₁, …, ∂f/∂vₙ)` as a tuple.
pub fn gradient(expr: &Expr, vars: &[&str]) -> MathResult<Expr> {
    let parts = vars
        .iter()
        .map(|v| diff(expr, v, 1, None))
        .collect::<MathResult<Vec<_>>>()?;
    Ok(Expr::new(ExprNode::Tuple(parts)))
}

/// The Jacobian matrix `∂fᵢ/∂vⱼ` (one row per function).
pub fn jacobian(exprs: &[Expr], vars: &[&str]) -> MathResult<Expr> {
    let mut rows = Vec::with_capacity(exprs.len());
    for e in exprs {
        rows.push(
            vars.iter()
                .map(|v| diff(e, v, 1, None))
                .collect::<MathResult<Vec<_>>>()?,
        );
    }
    Expr::matrix(rows)
        .ok_or_else(|| MathError::NotANumber("a Jacobian with no functions or variables".into()))
}

/// The Hessian matrix `∂²f/∂vᵢ∂vⱼ`.
pub fn hessian(expr: &Expr, vars: &[&str]) -> MathResult<Expr> {
    let firsts = vars
        .iter()
        .map(|v| diff(expr, v, 1, None))
        .collect::<MathResult<Vec<_>>>()?;
    jacobian(&firsts, vars)
}

/// The directional derivative `∇f · u/|u|` along `direction`.
pub fn directional_derivative(expr: &Expr, vars: &[&str], direction: &[Expr]) -> MathResult<Expr> {
    if vars.len() != direction.len() {
        return Err(MathError::WrongArity {
            function: "the directional derivative".into(),
            expected: vars.len().to_string(),
            got: direction.len(),
        });
    }
    let norm_sq = add(direction
        .iter()
        .map(|c| pow(c.clone(), Expr::int(2)))
        .collect());
    if norm_sq.is_zero() {
        return Err(MathError::Undefined("a direction of length zero".into()));
    }
    let mut terms = Vec::with_capacity(vars.len());
    for (v, c) in vars.iter().zip(direction) {
        terms.push(mul(vec![diff(expr, v, 1, None)?, c.clone()]));
    }
    Ok(mul(vec![add(terms), pow(norm_sq, Expr::rational(-1, 2))]))
}

/// The `order`-th derivative with respect to `var`, evaluated at `var = at`.
pub fn evaluate_derivative_at(expr: &Expr, var: &str, order: u32, at: &Expr) -> MathResult<Expr> {
    let d = diff(expr, var, order, None)?;
    let value = d.subs(var, at);
    if value.is_undefined() {
        return Err(MathError::Undefined(format!(
            "the derivative at {var} = {at}"
        )));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x() -> Expr {
        Expr::sym("x")
    }
    fn y() -> Expr {
        Expr::sym("y")
    }
    fn n(k: i64) -> Expr {
        Expr::int(k)
    }

    #[test]
    fn gradient_jacobian_hessian() {
        let f = x().pow(2) * y() + y().pow(3);
        let g = gradient(&f, &["x", "y"]).unwrap();
        assert_eq!(g.to_string(), "(2*x*y, x^2 + 3*y^2)");
        let h = hessian(&f, &["x", "y"]).unwrap();
        assert_eq!(h.to_string(), "[[2*y, 2*x], [2*x, 6*y]]");
        let j = jacobian(&[x() * y(), x() + y()], &["x", "y"]).unwrap();
        assert_eq!(j.to_string(), "[[y, x], [1, 1]]");
        assert!(jacobian(&[], &["x"]).is_err());
    }

    #[test]
    fn directional_and_point_values() {
        let f = x().pow(2) + y().pow(2);
        let dd = directional_derivative(&f, &["x", "y"], &[n(3), n(4)]).unwrap();
        assert_eq!(dd.to_string(), "(6*x + 8*y)/5");
        assert!(directional_derivative(&f, &["x", "y"], &[n(0), n(0)]).is_err());
        assert!(directional_derivative(&f, &["x", "y"], &[n(1)]).is_err());
        let v = evaluate_derivative_at(&x().pow(3), "x", 2, &n(2)).unwrap();
        assert_eq!(v, n(12));
        assert!(evaluate_derivative_at(&x().sqrt(), "x", 1, &n(0)).is_err());
    }
}
