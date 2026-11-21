use super::*;
use crate::expr::Constant;
use crate::functions::Func;

fn k() -> Expr {
    Expr::sym("k")
}
fn n() -> Expr {
    Expr::sym("n")
}
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
fn fact(a: Expr) -> Expr {
    crate::functions::func(Func::Factorial, vec![a])
}
fn inf() -> Expr {
    Expr::infinity()
}
fn pi() -> Expr {
    Expr::pi()
}
fn s(t: &Expr, a: &Expr, b: &Expr) -> MathResult<Expr> {
    sum(t, "k", a, b, None)
}

/// Equal as exact expressions, or numerically at a few values of the free symbols.
fn same(got: &Expr, want: &Expr) -> bool {
    if got == want {
        return true;
    }
    let diff = simplify(&add(vec![got.clone(), mul(vec![i(-1), want.clone()])]));
    if diff.is_zero() || crate::expand::expand(&diff).is_ok_and(|d| d.is_zero()) {
        return true;
    }
    let syms: Vec<String> = diff.free_symbols().iter().map(|s| s.to_string()).collect();
    [0.3, 2.0, 5.0, 11.0].iter().all(|v| {
        let env: std::collections::HashMap<&str, f64> =
            syms.iter().map(|s| (s.as_str(), *v)).collect();
        let a = got.eval_f64(&env);
        let b = want.eval_f64(&env);
        match (a, b) {
            (Ok(a), Ok(b)) => (a - b).abs() <= 1e-9 * b.abs().max(1.0),
            _ => false,
        }
    })
}

#[test]
fn finite_sums_with_symbolic_bounds() {
    let table: Vec<(Expr, Expr, Expr, Expr)> = vec![
        // Faulhaber
        (k(), i(1), n(), n() * (n() + i(1)) / i(2)),
        (
            k().pow(2),
            i(1),
            n(),
            n() * (n() + i(1)) * (i(2) * n() + i(1)) / i(6),
        ),
        (k().pow(3), i(1), n(), (n() * (n() + i(1)) / i(2)).pow(2)),
        (i(1), i(1), n(), n()),
        (Expr::sym("c"), i(1), n(), Expr::sym("c") * n()),
        (i(2) * k() - i(1), i(1), n(), n().pow(2)),
        (
            k().pow(2),
            i(0),
            n() - i(1),
            (n() - i(1)) * n() * (i(2) * n() - i(1)) / i(6),
        ),
        (
            k(),
            Expr::sym("m"),
            n(),
            (n() - Expr::sym("m") + i(1)) * (n() + Expr::sym("m")) / i(2),
        ),
        // geometric and arithmetico-geometric
        (i(2).pow(k()), i(0), n(), i(2).pow(n() + i(1)) - i(1)),
        (
            x().pow(k()),
            i(0),
            n(),
            (i(1) - x().pow(n() + i(1))) / (i(1) - x()),
        ),
        (
            k() * i(2).pow(k()),
            i(1),
            n(),
            (n() - i(1)) * i(2).pow(n() + i(1)) + i(2),
        ),
        // telescoping and partial fractions
        ((k() * (k() + i(1))).recip(), i(1), n(), n() / (n() + i(1))),
        (
            (k() + i(1)).sqrt() - k().sqrt(),
            i(0),
            n(),
            (n() + i(1)).sqrt(),
        ),
        (
            (k() * (k() + i(2))).recip(),
            i(1),
            n(),
            q(3, 4) - (n() + i(1)).recip() / i(2) - (n() + i(2)).recip() / i(2),
        ),
    ];
    assert!(table.len() >= 14);
    for (t, a, b, want) in table {
        let got = s(&t, &a, &b).unwrap_or_else(|e| panic!("Σ {t}: {e}"));
        assert!(
            same(&got, &want),
            "Σ_{{k={a}}}^{{{b}}} {t} = {got}, want {want}"
        );
    }
}

#[test]
fn sum_of_squares_reads_factored() {
    let got = s(&k().pow(2), &i(1), &n()).unwrap_or_else(|_| Expr::undefined());
    assert_eq!(got.to_string(), "n*(n + 1)*(2*n + 1)/6");
}

#[test]
fn finite_sums_with_concrete_bounds() {
    let table: Vec<(Expr, Expr, Expr, Expr)> = vec![
        (k().pow(2), i(1), i(100), i(338_350)),
        (k(), i(1), i(1_000_000), i(500_000_500_000)),
        (k().recip(), i(1), i(4), q(25, 12)),
        (i(2).pow(k()), i(0), i(10), i(2047)),
        (k(), i(5), i(4), i(0)),
        (fact(k()), i(0), i(5), i(154)),
        (x().pow(k()), i(0), i(2), i(1) + x() + x().pow(2)),
    ];
    for (t, a, b, want) in table {
        let got = s(&t, &a, &b).unwrap_or_else(|e| panic!("Σ {t}: {e}"));
        assert!(same(&got, &want), "Σ {t} = {got}, want {want}");
    }
    // A long range with no closed form is added in floating point.
    let got = s(&f("sin", k()), &i(1), &i(5_000)).unwrap_or_else(|_| Expr::undefined());
    let want: f64 = (1..=5_000).map(|v| (v as f64).sin()).sum();
    let v = got.eval_f64(&crate::eval::EmptyEnv).unwrap_or(f64::NAN);
    assert!((v - want).abs() < 1e-9, "{v} vs {want}");
    assert!(s(&f("sin", k()), &i(1), &Expr::sym("n")).is_err());
}

#[test]
fn harmonic_numbers_use_digamma() {
    let got = s(&k().recip(), &i(1), &n()).unwrap_or_else(|_| Expr::undefined());
    let h10: f64 = (1..=10).map(|v| 1.0 / v as f64).sum();
    let v = got.eval_f64(&[("n", 10.0)]).unwrap_or(f64::NAN);
    assert!((v - h10).abs() < 1e-12, "{got} gave {v}");
}

#[test]
fn known_infinite_sums() {
    let table: Vec<(Expr, Expr, Expr)> = vec![
        (q(1, 2).pow(k()), i(0), i(2)),
        (q(1, 3).pow(k()), i(1), q(1, 2)),
        (k() * q(1, 2).pow(k()), i(1), i(2)),
        (k().pow(-2), i(1), pi().pow(2) / i(6)),
        (k().pow(-4), i(1), pi().pow(4) / i(90)),
        (k().pow(-6), i(1), pi().pow(6) / i(945)),
        ((i(2) * k() - i(1)).pow(-2), i(1), pi().pow(2) / i(8)),
        (k().pow(-2), i(2), pi().pow(2) / i(6) - i(1)),
        ((k() * (k() + i(1))).recip(), i(1), i(1)),
        ((i(4) * k().pow(2) - i(1)).recip(), i(1), q(1, 2)),
        (
            (k().pow(2) * (k() + i(1))).recip(),
            i(1),
            pi().pow(2) / i(6) - i(1),
        ),
        (i(-1).pow(k() + i(1)) / k(), i(1), f("ln", i(2))),
        (i(-1).pow(k()) / (i(2) * k() + i(1)), i(0), pi() / i(4)),
        (fact(k()).recip(), i(0), Expr::e()),
        (x().pow(k()) / fact(k()), i(0), f("exp", x())),
        (i(2).pow(k()) / fact(k()), i(1), Expr::e().pow(i(2)) - i(1)),
        (
            i(-1).pow(k()) * x().pow(i(2) * k() + i(1)) / fact(i(2) * k() + i(1)),
            i(0),
            f("sin", x()),
        ),
        (
            i(-1).pow(k()) * x().pow(i(2) * k()) / fact(i(2) * k()),
            i(0),
            f("cos", x()),
        ),
        ((k().recip() - (k() + i(1)).recip()) * i(3), i(1), i(3)),
        (f("atan", k() + i(1)) - f("atan", k()), i(0), pi() / i(2)),
        (x().pow(k()), i(0), (i(1) - x()).recip()),
    ];
    assert!(table.len() >= 20);
    for (t, a, want) in table {
        let got = s(&t, &a, &inf()).unwrap_or_else(|e| panic!("Σ {t}: {e}"));
        assert!(same(&got, &want), "Σ_{{k={a}}}^∞ {t} = {got}, want {want}");
        assert!(got.as_number().is_none_or(|v| v.is_exact()), "{got}");
    }
}

#[test]
fn zeta_two_is_exact_text() {
    let got = s(&k().pow(-2), &i(1), &inf()).unwrap_or_else(|_| Expr::undefined());
    assert_eq!(got.to_string(), "π^2/6");
    let got =
        s(&(i(-1).pow(k() + i(1)) / k()), &i(1), &inf()).unwrap_or_else(|_| Expr::undefined());
    assert_eq!(got, f("ln", i(2)));
}

#[test]
fn divergent_series_are_never_summed() {
    let cases: Vec<(Expr, &str)> = vec![
        (k().recip(), "p-series"),
        (i(1), "divergence test"),
        (i(-1).pow(k()), "divergence test"),
        (i(2).pow(k()), "geometric series"),
        (k() / (k() + i(1)), "divergence test"),
        (k().sqrt().recip(), "p-series"),
        ((k() + i(1)).recip(), "p-series"),
        (fact(k()) / i(10).pow(k()), "ratio test"),
        ((k() * f("ln", k())).recip(), "integral test"),
        (
            (i(2) * k() + i(3)) / (k().pow(2) + i(1)),
            "limit comparison test",
        ),
    ];
    for (t, test) in cases {
        let got = s(&t, &i(2), &inf());
        assert!(
            matches!(&got, Err(MathError::Divergent(m)) if m.contains(test)),
            "Σ {t} gave {got:?}, expected divergence by the {test}"
        );
        let report = convergence(&t, "k", &i(2));
        assert_eq!(report.converges, Some(false), "{t}: {report}");
        assert_eq!(report.test, test, "{t}: {report}");
    }
}

#[test]
fn convergence_reports_name_the_test() {
    let cases: Vec<(Expr, bool, &str)> = vec![
        (k().pow(-2), true, "p-series"),
        (q(2, 3).pow(k()), true, "geometric series"),
        (fact(k()).recip(), true, "ratio test"),
        (k().pow(10) / fact(k()), true, "ratio test"),
        ((k() / (i(2) * k() + i(1))).pow(k()), true, "root test"),
        (i(-1).pow(k()) / k(), true, "alternating series test"),
        (i(-1).pow(k()) / k().sqrt(), true, "alternating series test"),
        ((k().pow(2) + i(1)).recip(), true, "limit comparison test"),
        (f("ln", k()) / k().pow(2), true, "integral test"),
        (f("sin", k()) / k().pow(2), true, "comparison test"),
        ((k() * f("ln", k()).pow(2)).recip(), true, "integral test"),
        (k().recip(), false, "p-series"),
    ];
    for (t, converges, test) in cases {
        let r = convergence(&t, "k", &i(2));
        assert_eq!(r.converges, Some(converges), "{t}: {r}");
        assert_eq!(r.test, test, "{t}: {r}");
        assert!(!r.detail.is_empty());
    }
    let r = convergence(&x().pow(k()), "k", &i(0));
    assert_eq!(r.converges, None);
    assert!(r.detail.contains("|x| < 1"), "{}", r.detail);
}

#[test]
fn numeric_sums_need_a_convergence_proof() {
    let cases: Vec<(Expr, f64)> = vec![
        (k().pow(-3), 1.202_056_903_159_594_2),
        (i(-1).pow(k() + i(1)) / k().sqrt(), 0.604_898_643_421_630_4),
        ((k().pow(2) + i(1)).recip(), 1.076_674_047_468_581_2),
    ];
    for (t, want) in cases {
        let mut steps = Steps::new();
        let got =
            sum(&t, "k", &i(1), &inf(), Some(&mut steps)).unwrap_or_else(|e| panic!("Σ {t}: {e}"));
        let v = got.eval_f64(&crate::eval::EmptyEnv).unwrap_or(f64::NAN);
        assert!((v - want).abs() < 1e-10, "Σ {t} ≈ {v}, want {want}");
        assert!(steps.iter().any(|s| s.rule == "numeric summation"));
    }
}

#[test]
fn real_power_series_are_hurwitz_zeta_values() {
    // Σ 1/k^(3/2) = ζ(3/2); Σ 1/(2k+1)^(3/2) = 2^(−3/2) ζ(3/2, 3/2).
    let got = sum(&k().pow(q(-3, 2)), "k", &i(1), &inf(), None).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(got.to_string(), "zeta(3/2)");
    let t = (i(2) * k() + i(1)).pow(q(-3, 2));
    let got = sum(&t, "k", &i(1), &inf(), None).unwrap_or_else(|e| panic!("{e}"));
    let Some(crate::hp::Approx::Value { value, .. }) = crate::hp::approx_closed(&got, 15) else {
        panic!("{got} has no value");
    };
    assert_eq!(value.rounded(15).digits, "688761186655448", "{got}");
}

#[test]
fn numeric_sums_refuse_digits_they_cannot_confirm() {
    // Σ sin k / k² = 1.01395913236…: oscillating with slowly shrinking terms, so plain
    // summation would print wrong digits; it is refused instead.
    let t = f("sin", k()) / k().pow(2);
    match sum(&t, "k", &i(1), &inf(), None) {
        Ok(v) => {
            let v = v.eval_f64(&crate::eval::EmptyEnv).unwrap_or(f64::NAN);
            assert!((v - 1.013_959_132_360_768_5).abs() < 1e-10, "{v}");
        }
        Err(e) => assert!(e.to_string().contains("too slowly"), "{e}"),
    }
}

#[test]
fn steps_name_the_formula() {
    let mut steps = Steps::new();
    sum(&k().pow(2), "k", &i(1), &n(), Some(&mut steps)).ok();
    assert_eq!(
        steps.iter().map(|s| s.rule.as_str()).collect::<Vec<_>>(),
        vec!["Faulhaber's formula"]
    );
    let mut steps = Steps::new();
    sum(&k().pow(-2), "k", &i(1), &inf(), Some(&mut steps)).ok();
    let rules: Vec<&str> = steps.iter().map(|s| s.rule.as_str()).collect();
    assert_eq!(rules[0], "p-series");
    assert!(rules[1].starts_with("p-series (ζ(2m)"), "{rules:?}");
    let mut steps = Steps::new();
    sum(&q(1, 2).pow(k()), "k", &i(0), &inf(), Some(&mut steps)).ok();
    assert!(steps.iter().any(|s| s.rule == "geometric series"));
}

#[test]
fn bad_input_is_an_error_not_a_panic() {
    assert!(s(&k(), &q(1, 2), &i(3)).is_err());
    assert!(s(&k(), &k(), &i(3)).is_err());
    assert!(s(&k(), &Expr::neg_infinity(), &i(3)).is_err());
    assert!(s(&k(), &i(0), &Expr::neg_infinity()).is_err());
    assert!(s(&k().recip(), &i(0), &i(3)).is_err());
    assert!(s(&(k() - i(3)).pow(-2), &i(1), &inf()).is_err());
    assert!(s(&Expr::constant(Constant::Undefined), &i(0), &i(3)).is_err());
}

#[test]
fn finite_products() {
    let table: Vec<(Expr, Expr, Expr, Expr)> = vec![
        (k(), i(1), i(5), i(120)),
        (k(), i(1), n(), fact(n())),
        (k() + i(1), i(1), n(), fact(n() + i(1))),
        (i(2), i(1), n(), i(2).pow(n())),
        (
            i(2).pow(k()),
            i(1),
            n(),
            i(2).pow(n() * (n() + i(1)) / i(2)),
        ),
        ((k() + i(1)) / k(), i(1), n(), n() + i(1)),
        (i(1) - k().pow(-2), i(2), n(), (n() + i(1)) / (i(2) * n())),
        (k(), i(3), i(2), i(1)),
    ];
    for (t, a, b, want) in table {
        let got = product(&t, "k", &a, &b, None).unwrap_or_else(|e| panic!("Π {t}: {e}"));
        assert!(same(&got, &want), "Π {t} = {got}, want {want}");
    }
}

#[test]
fn infinite_products() {
    let wallis = i(4) * k().pow(2) / (i(4) * k().pow(2) - i(1));
    let got = product(&wallis, "k", &i(1), &inf(), None).unwrap_or_else(|_| Expr::undefined());
    assert_eq!(got, pi() / i(2));
    let got = product(&(i(1) - k().pow(-2)), "k", &i(2), &inf(), None)
        .unwrap_or_else(|_| Expr::undefined());
    assert_eq!(got, q(1, 2));
    let got = product(&(i(1) + k().pow(-3)), "k", &i(1), &inf(), None)
        .unwrap_or_else(|_| Expr::undefined());
    let v = got.eval_f64(&crate::eval::EmptyEnv).unwrap_or(f64::NAN);
    assert!((v - 2.428_189_792_098_87).abs() < 1e-8, "{v}");
    for t in [i(1) + k().recip(), i(2), k()] {
        assert!(
            matches!(
                product(&t, "k", &i(1), &inf(), None),
                Err(MathError::Divergent(_))
            ),
            "Π {t}"
        );
    }
}
