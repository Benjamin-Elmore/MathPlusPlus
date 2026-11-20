//! Property tests for calculus: symbolic derivatives against finite differences, and limits
//! of continuous functions against substitution.

use mpp_core::calc::{diff, limit};
use mpp_core::{Expr, ExprNode, Func, LimitDir};
use proptest::prelude::*;

fn leaf() -> impl Strategy<Value = Expr> {
    prop_oneof![
        (-4i64..=4).prop_map(Expr::int),
        (-3i64..=3, 1i64..=3).prop_map(|(p, q)| Expr::rational(p, q)),
        Just(Expr::sym("x")),
        Just(Expr::sym("x")),
        Just(Expr::pi()),
    ]
}

fn unary(f: Func, a: Expr) -> Expr {
    Expr::new(ExprNode::Func(f, vec![a]))
}

/// Raw trees in x built from the rules the differentiator knows.
fn expr() -> impl Strategy<Value = Expr> {
    leaf().prop_recursive(4, 24, 3, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 2..4).prop_map(|v| Expr::new(ExprNode::Add(v))),
            prop::collection::vec(inner.clone(), 2..3).prop_map(|v| Expr::new(ExprNode::Mul(v))),
            (inner.clone(), -2i64..=3).prop_map(|(b, k)| Expr::new(ExprNode::Pow(b, Expr::int(k)))),
            inner.clone().prop_map(|a| unary(Func::Sin, a)),
            inner.clone().prop_map(|a| unary(Func::Cos, a)),
            inner.clone().prop_map(|a| unary(Func::Atan, a)),
            inner.clone().prop_map(|a| unary(Func::Tanh, a)),
            // ln and sqrt of something positive
            inner.clone().prop_map(|a| unary(
                Func::Ln,
                Expr::new(ExprNode::Add(vec![
                    Expr::new(ExprNode::Pow(a, Expr::int(2))),
                    Expr::one()
                ]))
            )),
            inner.clone().prop_map(|a| Expr::new(ExprNode::Pow(
                Expr::new(ExprNode::Add(vec![
                    Expr::new(ExprNode::Pow(a, Expr::int(2))),
                    Expr::int(2)
                ])),
                Expr::rational(1, 2)
            ))),
            inner.prop_map(|a| Expr::new(ExprNode::Pow(Expr::e(), unary(Func::Sin, a)))),
        ]
    })
}

fn at(e: &Expr, x: f64) -> Option<f64> {
    e.eval_f64(&[("x", x)]).ok().filter(|v| v.is_finite())
}

fn central(e: &Expr, x: f64, h: f64) -> Option<f64> {
    Some((at(e, x + h)? - at(e, x - h)?) / (2.0 * h))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn derivatives_match_finite_differences(e in expr(), x in -2.0f64..2.0) {
        let d = diff(&e, "x", 1, None);
        prop_assert!(d.is_ok());
        let d = d.unwrap_or_else(|_| Expr::zero());
        let value = at(&e, x);
        prop_assume!(value.is_some_and(|v| v.abs() < 1e4));
        // Skip points where the function is not smooth enough for a finite difference.
        let (fd1, fd2) = (central(&e, x, 1e-4), central(&e, x, 1e-5));
        prop_assume!(fd1.is_some() && fd2.is_some());
        let (fd1, fd2) = (fd1.unwrap_or_default(), fd2.unwrap_or_default());
        prop_assume!((fd1 - fd2).abs() <= 1e-4 * fd1.abs().max(1.0));
        let exact = at(&d, x);
        prop_assert!(exact.is_some(), "d/dx {} = {} has no value at {}", e, d, x);
        let exact = exact.unwrap_or_default();
        prop_assert!(
            (exact - fd2).abs() <= 1e-3 * fd2.abs().max(1.0),
            "d/dx {} = {} gives {} at x = {}, finite difference {}", e, d, exact, x, fd2
        );
    }

    #[test]
    fn second_derivatives_match(e in expr(), x in -2.0f64..2.0) {
        let d1 = diff(&e, "x", 1, None).unwrap_or_else(|_| Expr::zero());
        let d2 = diff(&e, "x", 2, None).unwrap_or_else(|_| Expr::zero());
        let (a, b) = (central(&d1, x, 1e-4), central(&d1, x, 1e-5));
        prop_assume!(a.is_some() && b.is_some());
        let (a, b) = (a.unwrap_or_default(), b.unwrap_or_default());
        prop_assume!(a.abs() < 1e4 && (a - b).abs() <= 1e-4 * a.abs().max(1.0));
        let exact = at(&d2, x);
        prop_assume!(exact.is_some());
        let exact = exact.unwrap_or_default();
        prop_assert!((exact - b).abs() <= 1e-3 * b.abs().max(1.0), "{} at {}", e, x);
    }

    #[test]
    fn limits_of_continuous_functions(e in expr(), a in -2i64..=2) {
        let point = Expr::int(a);
        let direct = at(&e.simplify(), a as f64);
        let near = at(&e, a as f64 + 1e-6).zip(at(&e, a as f64 - 1e-6));
        prop_assume!(direct.is_some() && near.is_some());
        let v = direct.unwrap_or_default();
        let (r, l) = near.unwrap_or_default();
        prop_assume!(v.abs() < 1e4 && (r - v).abs() < 1e-3 && (l - v).abs() < 1e-3);
        let got = limit(&e, "x", &point, LimitDir::Both, None);
        prop_assert!(got.is_ok(), "lim {} at {} failed: {:?}", e, a, got);
        let got = got.unwrap_or_else(|_| Expr::zero());
        let gv = at(&got, 0.0);
        prop_assert!(gv.is_some_and(|g| (g - v).abs() <= 1e-6 * v.abs().max(1.0)), "lim {} at {} = {}", e, a, got);
    }
}
