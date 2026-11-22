//! Property tests: rewriting must never change the value of an expression.

use mpp_core::{Expr, ExprNode, Func, Poly};
use proptest::prelude::*;

fn leaf() -> impl Strategy<Value = Expr> {
    prop_oneof![
        (-5i64..=5).prop_map(Expr::int),
        (-5i64..=5, 1i64..=4).prop_map(|(p, q)| Expr::rational(p, q)),
        Just(Expr::sym("x")),
        Just(Expr::sym("y")),
        Just(Expr::pi()),
    ]
}

/// Raw (unsimplified) trees so the simplifier has real work to do.
fn raw_expr() -> impl Strategy<Value = Expr> {
    leaf().prop_recursive(4, 32, 3, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 2..4).prop_map(|v| Expr::new(ExprNode::Add(v))),
            prop::collection::vec(inner.clone(), 2..4).prop_map(|v| Expr::new(ExprNode::Mul(v))),
            (inner.clone(), -1i64..=3).prop_map(|(b, k)| Expr::new(ExprNode::Pow(b, Expr::int(k)))),
            inner
                .clone()
                .prop_map(|a| Expr::new(ExprNode::Func(Func::Sin, vec![a]))),
            inner
                .clone()
                .prop_map(|a| Expr::new(ExprNode::Func(Func::Cos, vec![a]))),
            inner.prop_map(|a| Expr::new(ExprNode::Pow(Expr::e(), a))),
        ]
    })
}

/// Raw polynomial trees in x and y.
fn raw_poly() -> impl Strategy<Value = Expr> {
    let leaf = prop_oneof![
        (-4i64..=4).prop_map(Expr::int),
        Just(Expr::sym("x")),
        Just(Expr::sym("y")),
    ];
    leaf.prop_recursive(4, 24, 3, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 2..4).prop_map(|v| Expr::new(ExprNode::Add(v))),
            prop::collection::vec(inner.clone(), 2..3).prop_map(|v| Expr::new(ExprNode::Mul(v))),
            (inner, 0i64..=3).prop_map(|(b, k)| Expr::new(ExprNode::Pow(b, Expr::int(k)))),
        ]
    })
}

fn value(e: &Expr, x: f64, y: f64) -> Option<f64> {
    e.eval_f64(&[("x", x), ("y", y)])
        .ok()
        .filter(|v| v.is_finite() && v.abs() < 1e8)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1.0)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn simplify_keeps_the_value(e in raw_expr(), x in -2.0f64..2.0, y in -2.0f64..2.0) {
        let raw = value(&e, x, y);
        prop_assume!(raw.is_some());
        // Skip points where the value is ill-conditioned (such as sin(e^{27})), where a
        // nudge in the last digits of x and y already changes it.
        let nudged = value(&e, x * (1.0 + 1e-12), y * (1.0 + 1e-12));
        prop_assume!(nudged.is_some_and(|n| close(n, raw.unwrap_or_default())));
        let s = e.simplify();
        let simplified = value(&s, x, y);
        prop_assume!(simplified.is_some());
        let (a, b) = (raw.unwrap_or_default(), simplified.unwrap_or_default());
        // A closed expression's float value can be all rounding (e^(π³) sin π is 0.0036
        // in floats); its high-precision value decides instead.
        // The same goes for open ones (e^(e^π)·x·sin π is 1e10 · 1e-16 of rounding):
        // put exact fractions in for x and y and let the high-precision value decide.
        if !close(a, b) {
            use mpp_core::hp::{approx_closed, Approx};
            const DEN: i64 = 1_000_000;
            let (xq, yq) = ((x * DEN as f64).round() as i64, (y * DEN as f64).round() as i64);
            let (xr, yr) = (xq as f64 / DEN as f64, yq as f64 / DEN as f64);
            let closed = e.subs_all(&[("x", Expr::rational(xq, DEN)), ("y", Expr::rational(yq, DEN))]);
            let b = value(&s, xr, yr).unwrap_or(b);
            let truth = match approx_closed(&closed, 12) {
                Some(Approx::Zero { .. }) => Some(0.0),
                Some(Approx::Value { value, .. }) => value
                    .to_ratio()
                    .and_then(|q| num_traits::ToPrimitive::to_f64(&q)),
                None => None,
            };
            if let Some(t) = truth {
                prop_assert!(close(t, b), "{e} = {t} but {s} = {b}");
                return Ok(());
            }
        }
        prop_assert!(close(a, b), "{e} = {a} but {s} = {b}");
    }

    #[test]
    fn simplify_is_idempotent(e in raw_expr()) {
        let once = e.simplify();
        prop_assert_eq!(once.simplify(), once);
    }

    #[test]
    fn expand_keeps_the_value(e in raw_poly(), x in -2.0f64..2.0, y in -2.0f64..2.0) {
        let expanded = e.expand();
        prop_assert!(expanded.is_ok());
        let expanded = expanded.unwrap_or_else(|_| Expr::zero());
        let (a, b) = (value(&e, x, y), value(&expanded, x, y));
        prop_assume!(a.is_some() && b.is_some());
        prop_assert!(close(a.unwrap_or_default(), b.unwrap_or_default()), "{e} vs {expanded}");
        prop_assert!(expanded.is_polynomial_in("x"));
        prop_assert!(expanded.as_poly("x").is_some());
    }

    #[test]
    fn factor_keeps_the_value(e in raw_poly(), x in -2.0f64..2.0, y in -2.0f64..2.0) {
        let f = e.factor();
        let (a, b) = (value(&e, x, y), value(&f, x, y));
        prop_assume!(a.is_some() && b.is_some());
        prop_assert!(close(a.unwrap_or_default(), b.unwrap_or_default()), "{e} vs {f}");
    }

    #[test]
    fn factor_then_expand_round_trips(
        roots in prop::collection::vec((-4i64..=4, 1i64..=3, 1u32..=2), 1..4),
        lead in prop_oneof![-3i64..=-1, 1i64..=3],
        extra in prop::collection::vec(-3i64..=3, 0..3),
    ) {
        // lead · Π (q x − p)^m · (extra polynomial)
        let mut p = Poly::from_ints(&[lead]);
        for (num, den, m) in &roots {
            for _ in 0..*m {
                p = p.mul(&Poly::from_ints(&[-num, *den]));
            }
        }
        if !extra.is_empty() {
            let mut coeffs = extra.clone();
            coeffs.push(1);
            p = p.mul(&Poly::from_ints(&coeffs));
        }
        let e = p.to_expr("x");
        let f = e.factor();
        let back = f.expand();
        prop_assert!(back.is_ok());
        prop_assert_eq!(back.unwrap_or_else(|_| Expr::zero()), e.clone(), "factor gave {}", f);
    }

    #[test]
    fn exact_square_roots(p in -50i64..=50, q in 1i64..=50) {
        let r = Expr::rational(p, q);
        prop_assert_eq!(r.pow(2).sqrt(), Expr::call("abs", vec![r]).unwrap_or_else(|_| Expr::zero()));
    }

    #[test]
    fn substitution_matches_evaluation(e in raw_poly(), k in -3i64..=3, y in -2.0f64..2.0) {
        let substituted = e.subs("x", &Expr::int(k));
        let (a, b) = (value(&e, k as f64, y), value(&substituted, 0.0, y));
        prop_assume!(a.is_some() && b.is_some());
        prop_assert!(close(a.unwrap_or_default(), b.unwrap_or_default()));
        prop_assert!(!substituted.contains_sym("x"));
    }
}
