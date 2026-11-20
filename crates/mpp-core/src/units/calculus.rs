//! How units pass through calculus, for the calculus code to call: `d/dt` of a length
//! is a speed, `∫ v dt` is a length, and the bounds of a definite integral must be in
//! the variable's dimension.

use crate::error::{MathError, MathResult};
use crate::expr::Expr;
use crate::units::quantity::{unit_name, Quantity};
use crate::units::unit::Unit;

/// The unit of `d(of)/d(wrt)` and the factor its value must be multiplied by.
pub fn derivative_unit(of: &Unit, wrt: &Unit) -> MathResult<(Unit, Expr)> {
    Ok(of.div(wrt)?.simplified())
}

/// The unit of `∫ integrand d(var)` and the factor its value must be multiplied by.
pub fn integral_unit(integrand: &Unit, var: &Unit) -> MathResult<(Unit, Expr)> {
    Ok(integrand.mul(var)?.simplified())
}

/// Attaches units to a derivative the calculus computed on plain values.
pub fn derivative(value: Expr, of: &Unit, wrt: &Unit) -> MathResult<Quantity> {
    let (u, k) = derivative_unit(of, wrt)?;
    Ok(Quantity::new((value * k).simplify(), u))
}

/// Attaches units to an antiderivative or definite integral computed on plain values.
pub fn integral(value: Expr, integrand: &Unit, var: &Unit) -> MathResult<Quantity> {
    let (u, k) = integral_unit(integrand, var)?;
    Ok(Quantity::new((value * k).simplify(), u))
}

/// Converts a bound of a definite integral to a plain value in the variable's unit,
/// with an error when its dimension is wrong.
pub fn bound_value(bound: &Quantity, var: &Unit) -> MathResult<Expr> {
    if bound.unit.dimension() != var.dimension() {
        return Err(MathError::Dimension(format!(
            "the integral runs over {}, but this bound is in {}",
            unit_name(var),
            unit_name(&bound.unit)
        )));
    }
    Ok(bound.convert_to(var)?.value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Unit {
        crate::units::parse_unit(s).unwrap()
    }

    #[test]
    fn derivatives_and_integrals() {
        let v = derivative(Expr::int(3), &u("m"), &u("s")).unwrap();
        assert_eq!(v.to_string(), "3 m/s");
        let x = integral(Expr::int(10), &u("m/s"), &u("s")).unwrap();
        assert_eq!(x.to_string(), "10 m");
        let e = integral(Expr::int(2), &u("N"), &u("m")).unwrap();
        assert_eq!(e.to_string(), "2 J");
        let mixed = integral(Expr::int(1), &u("km/h"), &u("s")).unwrap();
        assert_eq!(mixed.to_string(), "5/18 m");
    }

    #[test]
    fn bounds_are_checked_and_converted() {
        let b = Quantity::parse(Expr::int(2), "min").unwrap();
        assert_eq!(bound_value(&b, &u("s")).unwrap(), Expr::int(120));
        let bad = Quantity::parse(Expr::int(2), "m").unwrap();
        assert_eq!(
            bound_value(&bad, &u("s")).unwrap_err().to_string(),
            "the integral runs over s, but this bound is in m"
        );
    }
}
