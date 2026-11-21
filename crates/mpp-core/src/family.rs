//! Members of an indexed family: `a_i` inside a sum over i, or `p_1, p_2, …` written
//! with an ellipsis, where neither `a` nor `a_i` has a value.
//!
//! Each member is its own value, so a member is an application of the family to its
//! index; the name carries a trailing [`MARK`] so that printing shows it as a
//! subscript (`a_1`, `a₁`, `a_{1}`), never as a call `a(1)`. The mark can't be typed
//! as part of a name, so a member never collides with a user function.

use crate::expr::{Expr, ExprNode};

/// Ends the name of a family member's application.
pub const MARK: char = '_';

/// Member `index` of the family `base`.
pub fn member(base: &str, index: Expr) -> Expr {
    Expr::apply(&format!("{base}{MARK}"), vec![index])
}

/// The family and index of a member, or `None` for any other expression.
pub fn as_member(e: &Expr) -> Option<(&str, &Expr)> {
    let ExprNode::Apply(name, args) = e.node() else {
        return None;
    };
    let [index] = args.as_slice() else {
        return None;
    };
    let base = name.strip_suffix(MARK)?;
    (!base.is_empty()).then_some((base, index))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn members_print_as_subscripts() {
        let a1 = member("a", Expr::int(1));
        assert_eq!(as_member(&a1).map(|(b, _)| b), Some("a"));
        assert_eq!(a1.to_string(), "a_1");
        assert_eq!(a1.to_text(), "a₁");
        assert_eq!(a1.to_latex(), "a_{1}");
        let shifted = member("x", Expr::sym("i") + Expr::int(1));
        assert_eq!(shifted.to_text(), "x_(i + 1)");
        assert_eq!(shifted.to_latex(), "x_{i + 1}");
        let sum = member("a", Expr::int(1)) + member("a", Expr::int(2));
        assert_eq!(sum.to_text(), "a₁ + a₂");
        let prod = member("x", Expr::int(1)) * member("x", Expr::int(2));
        assert_eq!(prod.to_text(), "x₁x₂");
        // A user function is still a call.
        assert_eq!(Expr::apply("f", vec![Expr::int(1)]).to_text(), "f(1)");
        assert!(as_member(&Expr::apply("f", vec![Expr::int(1)])).is_none());
    }
}
