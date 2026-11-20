use super::*;

fn x() -> Expr {
    Expr::sym("x")
}
fn n(k: i64) -> Expr {
    Expr::int(k)
}
fn q(p: i64, d: i64) -> Expr {
    Expr::rational(p, d)
}
fn f(name: &str, a: Expr) -> Expr {
    Expr::call(name, vec![a]).unwrap()
}
fn inf() -> Expr {
    Expr::infinity()
}
fn ninf() -> Expr {
    Expr::neg_infinity()
}

fn lim(e: &Expr, at: &Expr, dir: LimitDir) -> MathResult<Expr> {
    limit(e, "x", at, dir, None)
}

#[test]
fn textbook_limits() {
    use LimitDir::{Both, Left, Right};
    let half_pi = q(1, 2) * Expr::pi();
    let table: Vec<(Expr, Expr, LimitDir, Expr)> = vec![
        // direct substitution and cancellation
        (x().pow(2), n(2), Both, n(4)),
        ((x().pow(2) - n(4)) / (x() - n(2)), n(2), Both, n(4)),
        ((x().pow(3) - n(1)) / (x() - n(1)), n(1), Both, n(3)),
        ((n(5) - x()) / (x().pow(2) - n(25)), n(5), Both, q(-1, 10)),
        (f("cos", x()), n(0), Both, n(1)),
        // L'Hôpital and standard forms
        (f("sin", x()) / x(), n(0), Both, n(1)),
        ((n(1) - f("cos", x())) / x().pow(2), n(0), Both, q(1, 2)),
        ((f("exp", x()) - n(1)) / x(), n(0), Both, n(1)),
        (f("ln", n(1) + x()) / x(), n(0), Both, n(1)),
        (f("tan", x()) / x(), n(0), Both, n(1)),
        ((f("cos", x()) - n(1)) / x(), n(0), Both, n(0)),
        (f("sinh", x()) / x(), n(0), Both, n(1)),
        ((x().sqrt() - n(2)) / (x() - n(4)), n(4), Both, q(1, 4)),
        (
            (f("exp", x()) - n(1) - x()) / x().pow(2),
            n(0),
            Both,
            q(1, 2),
        ),
        ((n(1) + x()).pow(x().recip()), n(0), Both, Expr::e()),
        // 0·∞, 0^0, ∞ − ∞
        (x() * f("ln", x()), n(0), Right, n(0)),
        (x().pow(x()), n(0), Right, n(1)),
        (f("sin", x()).recip() - x().recip(), n(0), Both, n(0)),
        (x() * f("sin", x().recip()), n(0), Both, n(0)),
        // one-sided and infinite
        (x().recip(), n(0), Right, inf()),
        (x().recip(), n(0), Left, ninf()),
        (x().pow(-2), n(0), Both, inf()),
        (f("ln", x()), n(0), Right, ninf()),
        (f("tan", x()), half_pi.clone(), Left, inf()),
        (f("tan", x()), half_pi.clone(), Right, ninf()),
        (f("abs", x()) / x(), n(0), Right, n(1)),
        (f("abs", x()) / x(), n(0), Left, n(-1)),
        (f("floor", x()), n(1), Left, n(0)),
        (f("floor", x()), n(1), Right, n(1)),
        ((x() - n(1)).recip(), n(1), Right, inf()),
        // at infinity
        (
            (n(3) * x().pow(2) + n(1)) / (n(2) * x().pow(2) - x()),
            inf(),
            Both,
            q(3, 2),
        ),
        ((x() + n(1)) / (x().pow(2) + n(1)), inf(), Both, n(0)),
        (x().pow(3) / (x().pow(2) + n(1)), inf(), Both, inf()),
        (x().pow(3) / (x().pow(2) + n(1)), ninf(), Both, ninf()),
        (
            (n(2) * x().pow(3) - x()) / (n(5) * x().pow(3) + n(2)),
            ninf(),
            Both,
            q(2, 5),
        ),
        (f("exp", x()) / x().pow(3), inf(), Both, inf()),
        (x().pow(5) * f("exp", -x()), inf(), Both, n(0)),
        (f("ln", x()) / x(), inf(), Both, n(0)),
        ((x().pow(2) + x()).sqrt() - x(), inf(), Both, q(1, 2)),
        (f("atan", x()), inf(), Both, half_pi.clone()),
        (f("atan", x()), ninf(), Both, -half_pi),
        (f("exp", -x()), inf(), Both, n(0)),
        (f("exp", x()), ninf(), Both, n(0)),
        (f("sin", x()) / x(), inf(), Both, n(0)),
        ((n(1) + x().recip()).pow(x()), inf(), Both, Expr::e()),
        (
            (n(1) + n(2) / x()).pow(x()),
            inf(),
            Both,
            Expr::e().pow(n(2)),
        ),
        (
            (n(1) - x().recip()).pow(x()),
            inf(),
            Both,
            Expr::e().recip(),
        ),
        (x().pow(x().recip()), inf(), Both, n(1)),
        (x().pow(2), inf(), Both, inf()),
        (-x().pow(3), inf(), Both, ninf()),
        (n(3), inf(), Both, n(3)),
        (f("tanh", x()), ninf(), Both, n(-1)),
    ];
    assert!(table.len() >= 40);
    for (e, at, dir, expected) in table {
        let got = lim(&e, &at, dir);
        assert_eq!(got, Ok(expected), "lim {e} as x → {at} ({dir:?})");
    }
}

#[test]
fn limits_that_do_not_exist() {
    let cases = [
        (x().recip(), n(0)),
        ((x() - n(1)).recip(), n(1)),
        (f("abs", x()) / x(), n(0)),
        (f("floor", x()), n(2)),
    ];
    for (e, at) in cases {
        let got = lim(&e, &at, LimitDir::Both);
        assert!(
            matches!(got, Err(MathError::LimitDoesNotExist(_))),
            "lim {e} at {at} gave {got:?}"
        );
    }
    let osc = lim(&f("sin", x()), &inf(), LimitDir::Both);
    assert!(matches!(osc, Err(MathError::LimitDoesNotExist(_))));
    let msg = lim(&x().recip(), &n(0), LimitDir::Both)
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    assert!(msg.contains("from the left it is -∞"), "{msg}");
}

#[test]
fn steps_show_the_working() {
    let mut steps = Steps::new();
    let e = f("sin", x()) / x();
    limit(&e, "x", &n(0), LimitDir::Right, Some(&mut steps)).unwrap();
    let rules: Vec<&str> = steps.iter().map(|s| s.rule.as_str()).collect();
    assert!(rules.contains(&"L'Hôpital's rule"), "{rules:?}");

    let mut steps = Steps::new();
    let e = (x().pow(2) - n(4)) / (x() - n(2));
    limit(&e, "x", &n(2), LimitDir::Both, Some(&mut steps)).unwrap();
    let rules: Vec<&str> = steps.iter().map(|s| s.rule.as_str()).collect();
    assert!(rules.contains(&"cancel the common factor"), "{rules:?}");
    assert_eq!(rules.last(), Some(&"both one-sided limits agree"));

    let mut steps = Steps::new();
    limit(&x().pow(2), "x", &n(3), LimitDir::Both, Some(&mut steps)).unwrap();
    let rules: Vec<&str> = steps.iter().map(|s| s.rule.as_str()).collect();
    assert_eq!(rules, vec!["direct substitution"]);
}

#[test]
fn parameters_and_constants() {
    let a = Expr::sym("a");
    assert_eq!(
        lim(&(a.clone() * x()), &n(2), LimitDir::Both),
        Ok(n(2) * a.clone())
    );
    assert_eq!(lim(&a, &n(2), LimitDir::Both), Ok(a));
}

#[test]
fn limits_that_need_dominant_terms_or_series() {
    let ex = f("exp", x());
    let emx = f("exp", -x());
    let tanh_like = (ex.clone() - emx.clone()) / (ex.clone() + emx.clone());
    let table: Vec<(Expr, Expr, LimitDir, Expr)> = vec![
        (tanh_like.clone(), inf(), LimitDir::Both, n(1)),
        (tanh_like, ninf(), LimitDir::Both, n(-1)),
        (
            (f("exp", n(2) * x()) + ex.clone()) / (n(3) * f("exp", n(2) * x()) - n(1)),
            inf(),
            LimitDir::Both,
            q(1, 3),
        ),
        ((ex.clone() + emx.clone()) / ex, inf(), LimitDir::Both, n(1)),
    ];
    for (e, at, dir, expected) in table {
        let got = lim(&e, &at, dir);
        assert_eq!(got, Ok(expected), "lim {e} as x → {at}");
    }
    // (1 − cos x)⁷/x¹⁴ needs 14 rounds of L'Hôpital's rule, more than it is allowed; the
    // series (x²/2)⁷ settles it, and the abandoned L'Hôpital steps are not shown.
    let e = (n(1) - f("cos", x())).pow(n(7)) / x().pow(n(14));
    let mut steps = Steps::new();
    let got = limit(&e, "x", &n(0), LimitDir::Both, Some(&mut steps));
    assert_eq!(got, Ok(q(1, 128)));
    let rules: Vec<&str> = steps.iter().map(|s| s.rule.as_str()).collect();
    assert!(rules.contains(&"series expansion"), "{rules:?}");
    assert!(!rules.contains(&"L'Hôpital's rule"), "{rules:?}");
    // L'Hôpital's rule still shows when it works.
    let e = (f("tan", f("sin", x())) - f("sin", f("tan", x()))) / x().pow(n(7));
    assert_eq!(lim(&e, &n(0), LimitDir::Both), Ok(q(1, 30)));
}
