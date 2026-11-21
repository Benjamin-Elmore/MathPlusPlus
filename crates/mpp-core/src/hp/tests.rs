use proptest::prelude::*;

use super::*;
use crate::expr::Expr;
use crate::functions::Func;

fn f(name: Func, x: Expr) -> Expr {
    Expr::new(crate::expr::ExprNode::Func(name, vec![x]))
}

fn q(n: i64, d: i64) -> Expr {
    Expr::rational(n, d)
}

/// `e` to `sig` digits as `d.ddd…e±k`.
fn sci(e: &Expr, sig: usize) -> String {
    match approx_closed(e, sig) {
        Some(Approx::Value { value, .. }) => {
            let d = value.rounded(sig);
            let sign = if d.neg { "-" } else { "" };
            format!("{sign}{}.{}e{}", &d.digits[..1], &d.digits[1..], d.k)
        }
        Some(Approx::Zero { .. }) => "0".into(),
        None => "none".into(),
    }
}

/// References computed with mpmath at 60 digits (120 for the cancellation).
#[test]
fn matches_high_precision_references() {
    let cases: Vec<(Expr, &str)> = vec![
        (Expr::pi(), "3.14159265358979323846264338328e0"),
        (Expr::e(), "2.71828182845904523536028747135e0"),
        (Expr::int(2).sqrt(), "1.41421356237309504880168872421e0"),
        (
            f(Func::Ln, Expr::int(2)),
            "6.93147180559945309417232121458e-1",
        ),
        (
            Expr::num(crate::number::Number::Rational(BigRational::from_integer(
                (1..=200u32).map(BigInt::from).product(),
            ))),
            "7.88657867364790503552363213932e374",
        ),
        (
            Expr::e().pow(Expr::int(1000)),
            "1.97007111401704699388887935224e434",
        ),
        (
            f(Func::Ln, Expr::int(10).pow(Expr::int(400))),
            "9.21034037197618273607196581874e2",
        ),
        (
            f(Func::Gamma, Expr::int(1_000_000)),
            "8.26393168833124006237664610317e5565702",
        ),
        (
            Expr::pi() - q(314_159_265_358_979, 100_000_000_000_000),
            "3.23846264338327950288419716940e-15",
        ),
        (
            Expr::e().pow(Expr::pi() * Expr::int(163).sqrt()) - Expr::int(262_537_412_640_768_744),
            "-7.49927402801814311120646143663e-13",
        ),
        (f(Func::Gamma, q(1, 3)), "2.67893853470774763365569294097e0"),
        (
            f(Func::Gamma, q(-3, 2)),
            "2.36327180120735470306422331112e0",
        ),
        (
            f(Func::Sin, Expr::int(10_000_000_000)),
            "-4.87506025087510691527794294348e-1",
        ),
        (f(Func::Atan, q(1, 7)), "1.41897054604163922812851617103e-1"),
        (
            f(Func::Asin, q(3, 10)),
            "3.04692654015397507972002961228e-1",
        ),
        (
            f(Func::Tanh, Expr::int(3)),
            "9.95054753686730451331880185255e-1",
        ),
        (
            f(Func::Asinh, Expr::int(-2)),
            "-1.44363547517881034249327674027e0",
        ),
        (
            f(Func::Cos, Expr::pi() / Expr::int(7)),
            "9.00968867902419126236102319507e-1",
        ),
        (
            Expr::apply("zeta", vec![q(10001, 10000)]),
            "1.00005772229464376290700185888e4",
        ),
        (
            Expr::apply("zeta", vec![q(101, 100)]),
            "1.00577943338496872490282154286e2",
        ),
        (
            Expr::apply("zeta", vec![Expr::int(3)]),
            "1.20205690315959428539973816151e0",
        ),
        (
            Expr::apply("hurwitzzeta", vec![Expr::int(2), q(1, 2)]),
            "4.93480220054467930941724549994e0",
        ),
        (
            f(Func::Factorial, Expr::int(10_000_000_000)),
            "2.32579620567308336510494471995e95657055186",
        ),
    ];
    for (e, want) in cases {
        assert_eq!(sci(&e, 30), want, "{e}");
    }
}

#[test]
fn exact_zero_is_recognised() {
    let two = Expr::int(2).sqrt();
    let e = Expr::new(crate::expr::ExprNode::Add(vec![
        Expr::new(crate::expr::ExprNode::Mul(vec![two.clone(), two])),
        Expr::int(-2),
    ]));
    assert!(matches!(approx_closed(&e, 10), Some(Approx::Zero { .. })));
}

#[test]
fn decimal_rounding() {
    let d = Decimal {
        neg: false,
        digits: "2675".into(),
        k: 0,
    };
    assert_eq!(d.rounded(3).digits, "268");
    let d = Decimal {
        neg: false,
        digits: "99996".into(),
        k: 2,
    };
    let r = d.rounded(4);
    assert_eq!((r.digits.as_str(), r.k), ("1000", 3));
}

/// A random elementary expression in `x` that stays well conditioned for `x` in
/// `[1/2, 2]`.
fn arb_expr() -> impl Strategy<Value = Expr> {
    let leaf = prop_oneof![
        Just(Expr::sym("x")),
        (1i64..9).prop_map(Expr::int),
        (1i64..9, 2i64..9).prop_map(|(n, d)| Expr::rational(n, d)),
        Just(Expr::pi()),
    ];
    leaf.prop_recursive(3, 12, 2, |inner| {
        use crate::expr::ExprNode::{Add, Mul};
        prop_oneof![
            (inner.clone(), inner.clone()).prop_map(|(a, b)| Expr::new(Add(vec![a, b]))),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| Expr::new(Mul(vec![a, b]))),
            inner.clone().prop_map(|a| f(Func::Sin, a)),
            inner.clone().prop_map(|a| f(Func::Atan, a)),
            inner.clone().prop_map(|a| Expr::e().pow(f(Func::Cos, a))),
            inner
                .clone()
                .prop_map(|a| f(Func::Ln, Expr::int(1) + a.clone() * a)),
            inner.prop_map(|a| (Expr::int(2) + f(Func::Sin, a)).sqrt()),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Ten digits agree with the first ten of a 40-digit evaluation, and with `f64`
    /// wherever `f64` is trustworthy.
    #[test]
    fn approx_agrees_with_references(e in arb_expr(), n in 1i64..16) {
        let mut vars = HashMap::new();
        let x = BigRational::new(BigInt::from(n + 8), BigInt::from(16));
        vars.insert("x".to_string(), x.clone());
        let short = approx(&e, &vars, 10);
        let long = approx(&e, &vars, 40);
        if let (Some(Approx::Value { value: a, .. }), Some(Approx::Value { value: b, .. })) =
            (&short, &long)
        {
            prop_assert_eq!(a.rounded(10), b.rounded(10));
            let fx = num_traits::ToPrimitive::to_f64(&x).unwrap_or(0.0);
            if let Ok(v) = e.eval_f64(&[("x", fx)]) {
                let r = b.rounded(17).to_ratio().and_then(|q| num_traits::ToPrimitive::to_f64(&q));
                if let Some(r) = r {
                    prop_assert!((v - r).abs() <= 1e-9 * r.abs().max(1.0), "{} {} {}", e, v, r);
                }
            }
        }
    }
}

/// A cancelled query stops the evaluator at once instead of finishing a huge reduction,
/// and a deadline stops it soon after it passes.
#[test]
fn honours_the_query_budget() {
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    let big = f(Func::Sin, Expr::int(2).pow(Expr::int(100_000)));
    let flag = Arc::new(AtomicBool::new(true));
    {
        let _g = crate::budget::install(crate::budget::Budget::new(flag, None));
        let start = Instant::now();
        assert_eq!(approx_closed(&big, 10), None);
        assert!(start.elapsed() < Duration::from_secs(2));
    }
    let flag = Arc::new(AtomicBool::new(false));
    let _g = crate::budget::install(crate::budget::Budget::new(
        flag,
        Some(Duration::from_millis(1)),
    ));
    std::thread::sleep(Duration::from_millis(5));
    let start = Instant::now();
    assert_eq!(approx_closed(&big, 10), None);
    assert!(start.elapsed() < Duration::from_secs(2));
}
