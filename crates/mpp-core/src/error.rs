//! Errors raised by the core with user-facing messages.

use std::fmt;

/// Every way a core computation can fail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MathError {
    /// Division by an exact or approximate zero.
    DivisionByZero,
    /// The result has no value (for example `∞ - ∞` or `0^0` with a symbolic side).
    Undefined(String),
    /// A function was called outside its domain.
    Domain { function: String, detail: String },
    /// A variable has no value in the environment.
    UnboundVariable(String),
    /// A name was called as a function but nothing defines it.
    UnknownFunction(String),
    /// A function got the wrong number of arguments.
    WrongArity {
        function: String,
        expected: String,
        got: usize,
    },
    /// An expression is not a polynomial in the given variable.
    NotPolynomial { var: String },
    /// The result would be too large to compute or print.
    TooLarge(String),
    /// The value is not a number (a relation, matrix, set …).
    NotANumber(String),
    /// The operation is valid maths that this version does not do yet.
    Unsupported(String),
    /// Units that don't fit together; the message names both ("can't add m and s").
    Dimension(String),
}

impl fmt::Display for MathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MathError::DivisionByZero => write!(f, "division by zero"),
            MathError::Undefined(what) => write!(f, "{what} is undefined"),
            MathError::Domain { function, detail } => {
                write!(f, "{function} is not defined here: {detail}")
            }
            MathError::UnboundVariable(name) => write!(f, "`{name}` has no value"),
            MathError::UnknownFunction(name) => write!(f, "`{name}` is not a known function"),
            MathError::WrongArity {
                function,
                expected,
                got,
            } => write!(
                f,
                "{function} takes {expected} argument{}, but got {got}",
                if expected == "1" { "" } else { "s" }
            ),
            MathError::NotPolynomial { var } => write!(f, "this is not a polynomial in {var}"),
            MathError::TooLarge(what) => write!(f, "{what} is too large to compute"),
            MathError::NotANumber(what) => write!(f, "{what} is not a number"),
            MathError::Unsupported(what) => write!(f, "{what} is not supported yet"),
            MathError::Dimension(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for MathError {}

/// Result type of the core.
pub type MathResult<T> = Result<T, MathError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_read_well() {
        assert_eq!(MathError::DivisionByZero.to_string(), "division by zero");
        assert_eq!(
            MathError::WrongArity {
                function: "sin".into(),
                expected: "1".into(),
                got: 2
            }
            .to_string(),
            "sin takes 1 argument, but got 2"
        );
        assert_eq!(
            MathError::UnboundVariable("x".into()).to_string(),
            "`x` has no value"
        );
    }
}
