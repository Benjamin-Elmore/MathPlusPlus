//! Small helpers shared by the domains: argument checks, exact zero tests and numeric
//! fall-backs.

use num_bigint::BigInt;
use num_rational::BigRational;

use crate::error::{MathError, MathResult};
use crate::eval::EmptyEnv;
use crate::expr::{Expr, ExprNode, RelOp};
use crate::number::Number;

/// Most items a list argument (data, set elements) may have.
pub const MAX_ITEMS: usize = 1_000_000;

/// Relative tolerance used when a constant expression is compared to zero numerically.
const ZERO_TOL: f64 = 1e-11;

/// A wrong-arity error for operator `op`.
pub fn arity(op: &str, expected: &str, got: usize) -> MathError {
    MathError::WrongArity {
        function: op.into(),
        expected: expected.into(),
        got,
    }
}

/// Checks that `args` has exactly `n` items.
pub fn expect_args(op: &str, args: &[Expr], n: usize) -> MathResult<()> {
    if args.len() == n {
        Ok(())
    } else {
        Err(arity(op, &n.to_string(), args.len()))
    }
}

/// Checks that `args` has between `lo` and `hi` items.
pub fn expect_args_between(op: &str, args: &[Expr], lo: usize, hi: usize) -> MathResult<()> {
    if (lo..=hi).contains(&args.len()) {
        Ok(())
    } else {
        Err(arity(op, &format!("{lo} to {hi}"), args.len()))
    }
}

/// Simplified and expanded form (the expansion is skipped when it would be too large).
pub fn normalize(e: &Expr) -> Expr {
    let s = e.simplify();
    s.expand().unwrap_or(s)
}

/// The value of a constant expression in `f64`, when it has one.
pub fn numeric(e: &Expr) -> Option<f64> {
    if !e.is_constant() {
        return None;
    }
    e.eval_f64(&EmptyEnv).ok().filter(|v| v.is_finite())
}

/// True when `e` is zero: exactly after simplifying and expanding, or, for constant
/// expressions the exact rules can't settle (nested radicals), numerically.
pub fn is_zero(e: &Expr) -> bool {
    let n = normalize(e);
    if n.is_zero() {
        return true;
    }
    if n.is_number() || !n.is_constant() {
        return false;
    }
    match n.eval_complex(&EmptyEnv) {
        Ok(v) => {
            let scale = magnitude_scale(&n).max(1.0);
            v.norm() <= ZERO_TOL * scale
        }
        Err(_) => false,
    }
}

/// A rough size of the terms in a sum, used to scale the numeric zero tolerance.
fn magnitude_scale(e: &Expr) -> f64 {
    match e.node() {
        ExprNode::Add(terms) => terms
            .iter()
            .filter_map(|t| t.eval_complex(&EmptyEnv).ok())
            .map(|v| v.norm())
            .fold(0.0, f64::max),
        _ => e.eval_complex(&EmptyEnv).map(|v| v.norm()).unwrap_or(1.0),
    }
}

/// True when `a` and `b` are equal (see [`is_zero`]).
pub fn equal(a: &Expr, b: &Expr) -> bool {
    a == b || is_zero(&(a - b))
}

/// The exact rational value of `e`, if it is one.
pub fn rational(e: &Expr) -> Option<BigRational> {
    e.as_number()?.as_rational().cloned()
}

/// The exact integer value of `e`, if it is one.
pub fn integer(e: &Expr) -> Option<BigInt> {
    e.as_number()?.as_integer()
}

/// An exact integer argument, or an error naming `op`.
pub fn int_arg(op: &str, e: &Expr) -> MathResult<BigInt> {
    integer(&e.simplify()).ok_or_else(|| MathError::Domain {
        function: op.into(),
        detail: format!("{e} is not an integer"),
    })
}

/// A symbol argument, or an error naming `op`.
pub fn sym_arg(op: &str, e: &Expr) -> MathResult<String> {
    e.as_sym()
        .map(str::to_string)
        .ok_or_else(|| MathError::Domain {
            function: op.into(),
            detail: format!("{e} is not a variable name"),
        })
}

/// A real numeric argument, exact or approximate.
pub fn real_arg(op: &str, e: &Expr) -> MathResult<f64> {
    numeric(e).ok_or_else(|| MathError::Domain {
        function: op.into(),
        detail: format!("{e} is not a real number"),
    })
}

/// `lhs - rhs` for an equation, the expression itself otherwise.
pub fn to_zero_form(e: &Expr) -> Expr {
    match e.node() {
        ExprNode::Relation(RelOp::Eq, l, r) => (l - r).simplify(),
        _ => e.simplify(),
    }
}

/// The items of a list argument: one tuple, set or vector argument is unpacked,
/// several arguments are taken as the items themselves.
pub fn items(op: &str, args: &[Expr]) -> MathResult<Vec<Expr>> {
    let out: Vec<Expr> = match args {
        [single] => match single.node() {
            ExprNode::Tuple(v) | ExprNode::Set(v) => v.clone(),
            ExprNode::Matrix { rows, cols, data } if *rows == 1 || *cols == 1 => data.clone(),
            ExprNode::Matrix { .. } => {
                return Err(MathError::Domain {
                    function: op.into(),
                    detail: "expected a list or a vector, not a matrix".into(),
                })
            }
            _ => vec![single.clone()],
        },
        _ => args.to_vec(),
    };
    if out.len() > MAX_ITEMS {
        return Err(MathError::TooLarge(format!(
            "a list of {} items",
            out.len()
        )));
    }
    Ok(out)
}

/// An exact rational as an expression.
pub fn rat_expr(q: BigRational) -> Expr {
    Expr::num(Number::from_ratio(q))
}

/// Sign of a constant real expression: exact for numbers, numeric otherwise.
pub fn sign(e: &Expr) -> Option<std::cmp::Ordering> {
    if let Some(n) = e.as_number() {
        return n.sign();
    }
    if is_zero(e) {
        return Some(std::cmp::Ordering::Equal);
    }
    numeric(e).and_then(|v| v.partial_cmp(&0.0))
}

/// Compares two constant real expressions (±∞ allowed).
pub fn compare(a: &Expr, b: &Expr) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    let inf_sign = |e: &Expr| -> Option<Ordering> {
        if !e.is_infinite() {
            return None;
        }
        Some(if matches!(e.node(), ExprNode::Const(_)) {
            Ordering::Greater
        } else {
            Ordering::Less
        })
    };
    match (inf_sign(a), inf_sign(b)) {
        (Some(x), Some(y)) => return Some(x.cmp(&y)),
        (Some(x), None) => return Some(x),
        (None, Some(y)) => return Some(y.reverse()),
        (None, None) => {}
    }
    if let (Some(x), Some(y)) = (a.as_number(), b.as_number()) {
        if let Some(o) = x.cmp_real(y) {
            return Some(o);
        }
    }
    sign(&(b - a)).map(Ordering::reverse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_tests() {
        let x = Expr::sym("x");
        assert!(is_zero(
            &((&x + &Expr::one()).pow(2) - x.pow(2) - Expr::int(2) * x.clone() - Expr::one())
        ));
        assert!(!is_zero(&x));
        let s5 = Expr::int(5).sqrt();
        let lhs = (Expr::one() + s5.clone()).pow(2);
        let rhs = Expr::int(6) + Expr::int(2) * s5;
        assert!(equal(&lhs, &rhs));
    }

    #[test]
    fn comparisons() {
        use std::cmp::Ordering;
        assert_eq!(compare(&Expr::pi(), &Expr::int(3)), Some(Ordering::Greater));
        assert_eq!(
            compare(&Expr::neg_infinity(), &Expr::int(3)),
            Some(Ordering::Less)
        );
        assert_eq!(
            compare(&Expr::infinity(), &Expr::infinity()),
            Some(Ordering::Equal)
        );
        assert_eq!(compare(&Expr::sym("x"), &Expr::int(3)), None);
    }

    #[test]
    fn list_arguments() {
        let t = Expr::new(ExprNode::Tuple(vec![Expr::int(1), Expr::int(2)]));
        assert_eq!(items("mean", &[t]).unwrap().len(), 2);
        assert_eq!(
            items("mean", &[Expr::int(1), Expr::int(2), Expr::int(3)])
                .unwrap()
                .len(),
            3
        );
    }
}
