//! Substitution of symbols and sub-expressions.

use crate::expr::{Expr, ExprNode};
use crate::simplify::canonical;

/// Replaces free occurrences of symbols at the same time, then simplifies.
///
/// Bound variables (of sums, limits, definite integrals) are left alone, and so is the
/// body of an unevaluated derivative taken with respect to a replaced symbol.
pub fn subs_all(e: &Expr, pairs: &[(&str, Expr)]) -> Expr {
    if pairs.is_empty() {
        return e.clone();
    }
    match e.node() {
        ExprNode::Sym(s) => {
            if let Some((_, v)) = pairs.iter().find(|(n, _)| *n == &**s) {
                return v.clone();
            }
            e.clone()
        }
        ExprNode::Derivative { vars, .. }
            if vars
                .iter()
                .any(|(v, _)| pairs.iter().any(|(n, _)| *n == &**v)) =>
        {
            e.clone()
        }
        _ => {
            if let Some(bound) = e.bound_var() {
                if pairs.iter().any(|(n, _)| *n == &**bound) {
                    // Substitute everywhere except in the body.
                    let inner: Vec<(&str, Expr)> = pairs
                        .iter()
                        .filter(|(n, _)| *n != &**bound)
                        .cloned()
                        .collect();
                    let body = body_of(e);
                    let node = e.map_children(&mut |c| {
                        if body.is_some_and(|b| std::ptr::eq(b, c)) {
                            subs_all(c, &inner)
                        } else {
                            subs_all(c, pairs)
                        }
                    });
                    return canonical(node);
                }
            }
            canonical(e.map_children(&mut |c| subs_all(c, pairs)))
        }
    }
}

fn body_of(e: &Expr) -> Option<&Expr> {
    match e.node() {
        ExprNode::BigOp { expr, .. }
        | ExprNode::Limit { expr, .. }
        | ExprNode::Integral { expr, .. } => Some(expr),
        _ => None,
    }
}

/// Replaces every occurrence of the sub-expression `from` by `to`, then simplifies.
pub fn subs_expr(e: &Expr, from: &Expr, to: &Expr) -> Expr {
    if e == from {
        return to.clone();
    }
    canonical(e.map_children(&mut |c| subs_expr(c, from, to)))
}

impl Expr {
    /// Replaces the symbol `name` by `value` (see [`subs_all`]).
    pub fn subs(&self, name: &str, value: &Expr) -> Expr {
        subs_all(self, &[(name, value.clone())])
    }

    /// Replaces several symbols at once.
    pub fn subs_all(&self, pairs: &[(&str, Expr)]) -> Expr {
        subs_all(self, pairs)
    }

    /// Replaces a sub-expression (see [`subs_expr`]).
    pub fn subs_expr(&self, from: &Expr, to: &Expr) -> Expr {
        subs_expr(self, from, to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::BigOpKind;

    fn x() -> Expr {
        Expr::sym("x")
    }
    fn y() -> Expr {
        Expr::sym("y")
    }

    #[test]
    fn substitutes_and_simplifies() {
        let e = x().pow(2) + Expr::int(2) * x() + Expr::one();
        assert_eq!(e.subs("x", &Expr::int(3)), Expr::int(16));
        assert_eq!(
            e.subs("x", &(y() - Expr::one())),
            (y() - Expr::one()).pow(2) + Expr::int(2) * y() - Expr::one()
        );
        let s = Expr::call("sin", vec![x()]).unwrap();
        assert_eq!(
            s.subs("x", &(Expr::rational(1, 6) * Expr::pi())),
            Expr::rational(1, 2)
        );
    }

    #[test]
    fn simultaneous() {
        let e = x() - y();
        assert_eq!(e.subs_all(&[("x", y()), ("y", x())]), y() - x());
    }

    #[test]
    fn bound_variables_are_kept() {
        let sum = Expr::new(ExprNode::BigOp {
            kind: BigOpKind::Sum,
            expr: x() * Expr::sym("n"),
            var: "n".into(),
            lower: Expr::one(),
            upper: Expr::sym("n"),
        });
        let out = sum.subs("n", &Expr::int(5));
        match out.node() {
            ExprNode::BigOp { expr, upper, .. } => {
                assert_eq!(expr, &(x() * Expr::sym("n")));
                assert_eq!(upper, &Expr::int(5));
            }
            _ => panic!("expected a sum"),
        }
    }

    #[test]
    fn sub_expressions() {
        let s = Expr::call("sin", vec![x()]).unwrap();
        let e = s.pow(2) + s.clone();
        assert_eq!(e.subs_expr(&s, &y()), y().pow(2) + y());
    }
}
