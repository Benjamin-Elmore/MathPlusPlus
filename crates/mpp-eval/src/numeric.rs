//! Numeric answers for `≈ ?`: 10 significant digits, complex values supported.

use mpp_core::number::{format_f64, Number};
use mpp_core::{EmptyEnv, Expr, ExprNode, MathError};
use num_complex::Complex64;

use crate::error::EvalError;

/// Significant digits in a numeric answer.
pub const DIGITS: usize = 10;

/// Formats a complex value, dropping a negligible imaginary part.
pub fn format_complex(z: Complex64, digits: usize) -> String {
    if let Some(shown) = mpp_core::display::format_approx(z, digits) {
        return shown;
    }
    if z.im.abs() <= 1e-12 * z.re.abs().max(1.0) {
        format_f64(z.re, digits)
    } else {
        Number::complex_float(z.re, z.im).to_decimal(digits)
    }
}

/// The value of a closed expression as decimal text.
pub fn approximate(e: &Expr) -> Result<String, EvalError> {
    let free: Vec<String> = e.free_symbols().iter().map(|s| s.to_string()).collect();
    if !free.is_empty() {
        return Err(EvalError::Unbound(free));
    }
    approx_closed(e)
}

fn approx_closed(e: &Expr) -> Result<String, EvalError> {
    match e.node() {
        ExprNode::Matrix { cols, data, .. } => {
            let cells = data
                .iter()
                .map(approx_closed)
                .collect::<Result<Vec<_>, _>>()?;
            let rows: Vec<String> = cells
                .chunks((*cols).max(1))
                .map(|r| format!("[{}]", r.join(", ")))
                .collect();
            Ok(format!("[{}]", rows.join(", ")))
        }
        ExprNode::Tuple(items) | ExprNode::Set(items) => {
            let parts = items
                .iter()
                .map(approx_closed)
                .collect::<Result<Vec<_>, _>>()?;
            let (open, close) = if matches!(e.node(), ExprNode::Set(_)) {
                ("{", "}")
            } else {
                ("(", ")")
            };
            Ok(format!("{open}{}{close}", parts.join(", ")))
        }
        ExprNode::Bool(b) => Ok(b.to_string()),
        _ => scalar(e).map(|z| format_complex(z, DIGITS)),
    }
}

/// The complex value of a closed scalar expression.
pub fn scalar(e: &Expr) -> Result<Complex64, EvalError> {
    match e.eval_f64(&EmptyEnv) {
        Ok(x) => Ok(Complex64::new(x, 0.0)),
        Err(real_err) => match e.eval_complex(&EmptyEnv) {
            Ok(z) if z.re.is_finite() && z.im.is_finite() => Ok(z),
            Ok(_) => Err(EvalError::Math(MathError::Undefined("the value".into()))),
            Err(MathError::Domain { .. }) | Err(MathError::Undefined(_)) => {
                Err(EvalError::Math(real_err))
            }
            Err(e) => Err(EvalError::Math(e)),
        },
    }
}

/// True for an exact rational (the answer is already a plain number).
pub fn is_exact_rational(e: &Expr) -> bool {
    e.as_number()
        .is_some_and(|n| n.as_rational().is_some() || !n.is_exact())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(approximate(&Expr::pi()).unwrap(), "3.141592654");
        assert_eq!(approximate(&Expr::int(-4).sqrt()).unwrap(), "2*i");
        assert_eq!(
            approximate(&Expr::sym("x")),
            Err(EvalError::Unbound(vec!["x".into()]))
        );
        assert!(is_exact_rational(&Expr::rational(1, 3)));
        assert!(!is_exact_rational(&Expr::int(2).sqrt()));
    }
}
