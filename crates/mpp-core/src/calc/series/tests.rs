use super::*;

fn x() -> Expr {
    Expr::sym("x")
}
fn i(v: i64) -> Expr {
    Expr::int(v)
}
fn q(p: i64, d: i64) -> Expr {
    Expr::rational(p, d)
}
fn f(name: &str, a: Expr) -> Expr {
    Expr::call(name, vec![a]).unwrap_or_else(|_| Expr::undefined())
}
fn ser(e: &Expr, order: u32) -> Series {
    series(e, "x", &Expr::zero(), order, None).unwrap_or_else(|err| panic!("series of {e}: {err}"))
}

#[test]
fn maclaurin_coefficients_of_standard_functions() {
    let table: Vec<(Expr, Vec<Expr>)> = vec![
        (
            f("exp", x()),
            vec![i(1), i(1), q(1, 2), q(1, 6), q(1, 24), q(1, 120)],
        ),
        (
            f("sin", x()),
            vec![i(0), i(1), i(0), q(-1, 6), i(0), q(1, 120)],
        ),
        (
            f("cos", x()),
            vec![i(1), i(0), q(-1, 2), i(0), q(1, 24), i(0)],
        ),
        (
            f("ln", i(1) + x()),
            vec![i(0), i(1), q(-1, 2), q(1, 3), q(-1, 4), q(1, 5)],
        ),
        (
            (i(1) + x()).sqrt(),
            vec![i(1), q(1, 2), q(-1, 8), q(1, 16), q(-5, 128), q(7, 256)],
        ),
        (
            (i(1) - x()).recip(),
            vec![i(1), i(1), i(1), i(1), i(1), i(1)],
        ),
        (
            f("atan", x()),
            vec![i(0), i(1), i(0), q(-1, 3), i(0), q(1, 5)],
        ),
        (
            f("asin", x()),
            vec![i(0), i(1), i(0), q(1, 6), i(0), q(3, 40)],
        ),
        (
            f("sinh", x()),
            vec![i(0), i(1), i(0), q(1, 6), i(0), q(1, 120)],
        ),
        (
            f("cosh", x()),
            vec![i(1), i(0), q(1, 2), i(0), q(1, 24), i(0)],
        ),
        (
            f("tan", x()),
            vec![i(0), i(1), i(0), q(1, 3), i(0), q(2, 15)],
        ),
        (
            f("tanh", x()),
            vec![i(0), i(1), i(0), q(-1, 3), i(0), q(2, 15)],
        ),
        (
            f("sec", x()),
            vec![i(1), i(0), q(1, 2), i(0), q(5, 24), i(0)],
        ),
        (
            f("atanh", x()),
            vec![i(0), i(1), i(0), q(1, 3), i(0), q(1, 5)],
        ),
        (
            f("exp", f("sin", x())),
            vec![i(1), i(1), q(1, 2), i(0), q(-1, 8), q(-1, 15)],
        ),
        (
            f("sin", x()) / x(),
            vec![i(1), i(0), q(-1, 6), i(0), q(1, 120), i(0)],
        ),
        (i(2).pow(x()), {
            let l = f("ln", i(2));
            vec![
                i(1),
                l.clone(),
                l.pow(2) / i(2),
                l.pow(3) / i(6),
                l.pow(4) / i(24),
                l.pow(5) / i(120),
            ]
        }),
        (
            (i(1) + x()).pow(q(1, 3)),
            vec![i(1), q(1, 3), q(-1, 9), q(5, 81), q(-10, 243), q(22, 729)],
        ),
    ];
    for (e, want) in table {
        let s = ser(&e, 6);
        assert_eq!(s.order, 6, "{e}");
        for (k, c) in want.iter().enumerate() {
            assert_eq!(&s.coeff(k as i64), c, "coefficient {k} of {e}: {s}");
        }
    }
}

#[test]
fn display_and_latex_show_the_remainder() {
    let s = ser(&f("sin", x()), 6);
    assert_eq!(s.to_string(), "x - x^3/6 + x^5/120 + O(x^6)");
    assert!(
        s.to_latex().ends_with("+ O\\left(x^{6}\\right)"),
        "{}",
        s.to_latex()
    );
    let s = series(&f("exp", x()), "x", &i(1), 3, None).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(s.coeff(2), Expr::e() / i(2));
    assert!(s.to_string().ends_with("O((x - 1)^3)"), "{s}");
    let s = ser(&i(0), 3);
    assert_eq!(s.to_string(), "O(x^3)");
}

#[test]
fn laurent_and_infinity() {
    let s = ser(&f("sin", x()).recip(), 4);
    assert_eq!(s.valuation, -1);
    assert_eq!(s.coeff(-1), i(1));
    assert_eq!(s.coeff(1), q(1, 6));
    let s = series(&(x() / (x() + i(1))), "x", &Expr::infinity(), 3, None)
        .unwrap_or_else(|e| panic!("{e}"));
    // 1 − 1/x + 1/x² + O(1/x³)
    assert_eq!(s.coeff(0), i(1));
    assert_eq!(s.coeff(1), i(-1));
    assert_eq!(s.coeff(2), i(1));
    assert!(series(&x().sqrt(), "x", &i(0), 3, None).is_err());
    assert!(series(&x(), "x", &i(0), MAX_ORDER + 1, None).is_err());
}

#[test]
fn symbolic_centres_and_fallback() {
    let a = Expr::sym("a");
    let s = series(&f("sin", x()), "x", &a, 3, None).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(s.coeff(0), f("sin", a.clone()));
    assert_eq!(s.coeff(1), f("cos", a.clone()));
    assert_eq!(s.coeff(2), -f("sin", a) / i(2));
    // Γ has no table entry: derivatives at the centre.
    let s = series(&f("Gamma", i(1) + x()), "x", &i(0), 2, None).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(s.coeff(0), i(1));
}

#[test]
fn steps_name_the_expansions() {
    let mut steps = Steps::new();
    series(
        &(f("exp", x()) * f("cos", x())),
        "x",
        &i(0),
        4,
        Some(&mut steps),
    )
    .ok();
    let rules: Vec<&str> = steps.iter().map(|s| s.rule.as_str()).collect();
    assert!(rules.contains(&"exponential series"), "{rules:?}");
    assert!(rules.contains(&"series of cos"), "{rules:?}");
    assert_eq!(rules.last(), Some(&"combine the series"));
}

mod props {
    use super::*;
    use proptest::prelude::*;

    fn function() -> impl Strategy<Value = Expr> {
        let base = prop_oneof![
            Just(f("exp", x())),
            Just(f("sin", x())),
            Just(f("cos", x())),
            Just(f("ln", i(1) + x())),
            Just(f("atan", x())),
            Just((i(1) + x()).sqrt()),
            Just(f("tan", x())),
            Just((i(2) - x()).recip()),
        ];
        (base.clone(), base, 0..3usize).prop_map(|(a, b, op)| match op {
            0 => a + b,
            1 => a * b,
            _ => a,
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(24))]
        #[test]
        fn series_matches_the_function_near_the_centre(e in function(), h in -0.05f64..0.05) {
            let s = series(&e, "x", &Expr::zero(), 8, None).map_err(|err| TestCaseError::fail(format!("{e}: {err}")))?;
            let approx = s.terms.eval_f64(&[("x", h)]).unwrap_or(f64::NAN);
            let exact = e.eval_f64(&[("x", h)]).unwrap_or(f64::NAN);
            // The error is O(h^8): far below 1e-8 for |h| ≤ 0.05.
            prop_assert!((approx - exact).abs() < 1e-8, "{} at {}: {} vs {}", e, h, approx, exact);
        }
    }
}
