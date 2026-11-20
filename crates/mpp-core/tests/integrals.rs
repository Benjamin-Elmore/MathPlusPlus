//! Integration: a table of textbook antiderivatives (each differentiated back and compared
//! with the integrand at sample points), definite and improper integrals with known values,
//! divergent cases, multiple and line integrals, the step log, and property tests.

use std::collections::BTreeMap;

use mpp_core::calc::{
    diff, integrate, integrate_definite, integrate_multiple, integrate_with_constant,
    line_integral, line_integral_scalar, numeric_integrate,
};
use mpp_core::functions::func;
use mpp_core::{Expr, Func, MathError, Steps};
use proptest::prelude::*;

fn x() -> Expr {
    Expr::sym("x")
}
fn y() -> Expr {
    Expr::sym("y")
}
fn z() -> Expr {
    Expr::sym("z")
}
fn a() -> Expr {
    Expr::sym("a")
}
fn n(k: i64) -> Expr {
    Expr::int(k)
}
fn q(p: i64, d: i64) -> Expr {
    Expr::rational(p, d)
}
fn f(g: Func, u: Expr) -> Expr {
    func(g, vec![u])
}
fn sin(u: Expr) -> Expr {
    f(Func::Sin, u)
}
fn cos(u: Expr) -> Expr {
    f(Func::Cos, u)
}
fn tan(u: Expr) -> Expr {
    f(Func::Tan, u)
}
fn sec(u: Expr) -> Expr {
    f(Func::Sec, u)
}
fn csc(u: Expr) -> Expr {
    f(Func::Csc, u)
}
fn cot(u: Expr) -> Expr {
    f(Func::Cot, u)
}
fn ln(u: Expr) -> Expr {
    f(Func::Ln, u)
}
fn exp(u: Expr) -> Expr {
    Expr::e().pow(u)
}

const POINTS: [f64; 15] = [
    0.31, 0.47, 0.73, 0.91, 1.37, 2.29, 3.11, -0.41, -0.77, -1.63, 4.7, 0.13, 5.3, -6.1, 8.9,
];

fn eval(e: &Expr, xv: f64) -> Option<f64> {
    let env: BTreeMap<String, f64> = [("x".to_string(), xv), ("a".to_string(), 2.0)]
        .into_iter()
        .collect();
    e.eval_f64(&env).ok().filter(|v| v.is_finite())
}

/// Integrates `e`, differentiates the answer back and compares with `e` at sample points.
fn check(e: &Expr) -> Expr {
    let big_f = integrate(e, "x", None).unwrap_or_else(|err| panic!("∫ {e} dx failed: {err}"));
    let d = diff(&big_f, "x", 1, None).expect("derivative");
    let mut agree = 0;
    for p in POINTS {
        if let (Some(u), Some(v)) = (eval(e, p), eval(&d, p)) {
            assert!(
                (u - v).abs() <= 1e-6 * (1.0 + u.abs()),
                "∫ {e} dx = {big_f}, but its derivative {d} is {v} at x = {p}, not {u}"
            );
            agree += 1;
        }
    }
    assert!(agree >= 3, "too few sample points for ∫ {e} dx = {big_f}");
    big_f
}

#[test]
fn table_of_standard_integrals() {
    let cases = vec![
        x().pow(3),
        x().pow(-2),
        x().recip(),
        x().sqrt(),
        x().pow(q(2, 3)),
        n(3) * x().pow(2) + n(2) * x() + n(1),
        exp(x()),
        exp(n(3) * x()),
        n(2).pow(x()),
        sin(x()),
        cos(n(2) * x() + n(1)),
        tan(x()),
        sec(x()).pow(2),
        csc(x()).pow(2),
        sec(x()) * tan(x()),
        csc(x()) * cot(x()),
        sec(x()),
        csc(x()),
        cot(x()),
        (n(1) + x().pow(2)).recip(),
        (n(1) - x().pow(2)).pow(q(-1, 2)),
        (a().pow(2) + x().pow(2)).recip(),
        (a().pow(2) - x().pow(2)).pow(q(-1, 2)),
        f(Func::Sinh, x()),
        f(Func::Cosh, x()),
        f(Func::Tanh, x()),
        ln(x()),
        f(Func::Cosh, x()).pow(-2),
        (n(2) * x() + n(3)).pow(7),
        (n(3) * x() - n(2)).recip(),
    ];
    for e in &cases {
        check(e);
    }
}

#[test]
fn substitution_table() {
    let cases = vec![
        x() * exp(x().pow(2)),
        cos(x()) * sin(x()).pow(4),
        ln(x()) / x(),
        x() / (x().pow(2) + n(1)),
        n(2) * x() * cos(x().pow(2)),
        exp(x().sqrt()) / x().sqrt(),
        x() * (x() + n(1)).sqrt(),
        exp(x()) / (n(1) + exp(x())),
        tan(x()) * sec(x()).pow(2),
        x() / (n(1) - x().pow(2)).sqrt(),
        (x() * ln(x())).recip(),
        cos(x()) / (n(1) + sin(x()).pow(2)),
        x().pow(2) / (n(1) + x().pow(3)),
        f(Func::Cosh, x()) * f(Func::Sinh, x()),
        x().pow(3) * exp(x().pow(2)),
    ];
    for e in &cases {
        check(e);
    }
}

#[test]
fn parts_table() {
    let cases = vec![
        x() * exp(x()),
        x().pow(2) * exp(x()),
        x() * sin(x()),
        x().pow(2) * cos(x()),
        x() * ln(x()),
        ln(x()).pow(2),
        f(Func::Atan, x()),
        f(Func::Asin, x()),
        exp(x()) * sin(x()),
        exp(n(2) * x()) * cos(n(3) * x()),
        x() * f(Func::Cosh, x()),
        exp(-x()) * x().pow(3),
        x() * f(Func::Atan, x()),
    ];
    for e in &cases {
        check(e);
    }
}

#[test]
fn partial_fractions_table() {
    let cases = vec![
        (x().pow(2) - n(1)).recip(),
        ((x() + n(1)) * (x() + n(2))).recip(),
        (x() + n(1)) / (x().pow(2) * (x() - n(1))),
        (x() * (x().pow(2) + n(1))).recip(),
        (n(3) * x() + n(5)) / (x().pow(2) + n(2) * x() + n(5)),
        x().pow(3) / (x().pow(2) + n(1)),
        (x().pow(2) + n(1)).pow(-2),
        (x().pow(2) + n(2) * x() + n(5)).recip(),
        (x().pow(2) - n(2)).recip(),
        (n(2) * x() + n(1)) / (x() - n(3)).pow(3),
    ];
    for e in &cases {
        check(e);
    }
}

#[test]
fn trig_and_radical_table() {
    let cases = vec![
        sin(x()).pow(2),
        cos(x()).pow(4),
        sin(x()).pow(3),
        sin(x()).pow(2) * cos(x()).pow(2),
        sin(x()).pow(3) * cos(x()).pow(2),
        tan(x()).pow(3),
        sec(x()).pow(3),
        tan(x()).pow(2),
        sec(x()).pow(4),
        tan(x()).pow(2) * sec(x()).pow(4),
        sin(n(3) * x()) * cos(n(5) * x()),
        (n(1) - x().pow(2)).sqrt(),
        (x().pow(2) + n(4)).sqrt(),
        (x().pow(2) + n(4)).pow(q(-1, 2)),
        (x().pow(2) - n(9)).pow(q(-1, 2)),
        (x().pow(2) - n(1)).sqrt(),
        (n(5) + n(4) * x() - x().pow(2)).pow(q(-1, 2)),
        f(Func::Sinh, x()).pow(2),
        (x().pow(2) + n(1)).pow(q(-3, 2)),
        cot(x()).pow(2),
        csc(x()).pow(3),
        sin(n(2) * x()).pow(2),
    ];
    for e in &cases {
        check(e);
    }
}

#[test]
fn constant_is_added_on_request() {
    let r = integrate_with_constant(&x(), "x", None).unwrap();
    assert_eq!(r.to_string(), "x^2/2 + C");
    let r = integrate_with_constant(&Expr::sym("C"), "x", None).unwrap();
    assert_eq!(r.constant, "C_1");
}

#[test]
fn no_closed_form_is_reported() {
    for e in [exp(x().pow(2)), exp(x().pow(3)), (n(1) + x().pow(3)).sqrt()] {
        match integrate(&e, "x", None) {
            Err(MathError::NoClosedForm(_)) => {}
            other => panic!("∫ {e} dx should have no closed form, got {other:?}"),
        }
    }
}

fn definite(e: Expr, lo: Expr, hi: Expr) -> Result<Expr, MathError> {
    integrate_definite(&e, "x", &lo, &hi, None)
}

fn approx(e: &Expr, v: f64) {
    let got = e
        .eval_f64(&mpp_core::EmptyEnv)
        .unwrap_or_else(|err| panic!("{e} has no value: {err}"));
    assert!(
        (got - v).abs() < 1e-8 * (1.0 + v.abs()),
        "{e} ≈ {got}, expected {v}"
    );
}

#[test]
fn definite_integrals_with_known_values() {
    assert_eq!(definite(x().pow(2), n(0), n(1)).unwrap(), q(1, 3));
    assert_eq!(definite(exp(-x()), n(0), Expr::infinity()).unwrap(), n(1));
    assert_eq!(definite(sin(x()), n(0), Expr::pi()).unwrap(), n(2));
    assert_eq!(definite(ln(x()), n(0), n(1)).unwrap(), n(-1));
    assert_eq!(definite(x().pow(-2), n(1), Expr::infinity()).unwrap(), n(1));
    assert_eq!(definite(x().pow(q(-1, 2)), n(0), n(1)).unwrap(), n(2));
    assert_eq!(definite(x(), n(2), n(0)).unwrap(), n(-2));
    assert_eq!(
        definite(
            (n(1) + x().pow(2)).recip(),
            Expr::neg_infinity(),
            Expr::infinity()
        )
        .unwrap(),
        Expr::pi()
    );
    // (x − 1)^(−1/3) is singular at 1, inside [0, 2]; the two halves cancel.
    let v = definite((x() - n(1)).pow(q(-1, 3)), n(0), n(2)).unwrap();
    approx(&v, 0.0);
    // Symbolic bounds.
    let b = Expr::sym("b");
    assert_eq!(
        definite(x().pow(2), n(0), b.clone()).unwrap(),
        b.pow(3) / n(3)
    );
}

#[test]
fn numeric_definite_integrals() {
    let gauss = definite(exp(-x().pow(2)), Expr::neg_infinity(), Expr::infinity()).unwrap();
    // Exact through the error function: √π/2 (erf(∞) − erf(−∞)) = √π.
    assert_eq!(gauss, Expr::pi().sqrt());
    let v = definite(sin(x().pow(2)), n(0), n(1)).unwrap();
    approx(&v, 0.310_268_301_723_381_1);
    let (v, err) = numeric_integrate(&exp(-x().pow(2)), "x", 0.0, 1.0, 1e-12).unwrap();
    assert!((v - 0.746_824_132_812_427).abs() < 1e-12 && err < 1e-10);
    let (v, _) = numeric_integrate(&x().pow(q(-1, 2)), "x", 0.0, 1.0, 1e-10).unwrap();
    assert!((v - 2.0).abs() < 1e-8);
    let (v, _) = numeric_integrate(&exp(-x()), "x", 0.0, f64::INFINITY, 1e-10).unwrap();
    assert!((v - 1.0).abs() < 1e-9);
    assert!(numeric_integrate(&x().recip(), "x", 0.0, 1.0, 1e-10).is_err());
    assert!(matches!(
        numeric_integrate(&(x() * y()), "x", 0.0, 1.0, 1e-10),
        Err(MathError::UnboundVariable(_))
    ));
}

#[test]
fn divergent_integrals_are_reported() {
    let cases = vec![
        (x().pow(-2), n(-1), n(1)),
        (x().recip(), n(1), Expr::infinity()),
        (x().recip(), n(0), n(1)),
        (sin(x()), n(0), Expr::infinity()),
        (x(), Expr::neg_infinity(), Expr::infinity()),
        (tan(x()), n(0), Expr::pi()),
    ];
    for (e, lo, hi) in cases {
        match definite(e.clone(), lo.clone(), hi.clone()) {
            Err(MathError::Divergent(_)) => {}
            other => panic!("∫_{lo}^{hi} {e} dx should diverge, got {other:?}"),
        }
    }
}

#[test]
fn multiple_integrals() {
    let xy = x() * y();
    let rect = integrate_multiple(&xy, &[("y", n(0), n(1)), ("x", n(0), n(1))], None).unwrap();
    assert_eq!(rect, q(1, 4));
    let tri = integrate_multiple(&xy, &[("y", n(0), x()), ("x", n(0), n(1))], None).unwrap();
    assert_eq!(tri, q(1, 8));
    let cube = integrate_multiple(
        &(x() * y() * z()),
        &[("z", n(0), n(1)), ("y", n(0), n(1)), ("x", n(0), n(1))],
        None,
    )
    .unwrap();
    assert_eq!(cube, q(1, 8));
    let tetra = integrate_multiple(
        &n(1),
        &[
            ("z", n(0), n(1) - x() - y()),
            ("y", n(0), n(1) - x()),
            ("x", n(0), n(1)),
        ],
        None,
    )
    .unwrap();
    assert_eq!(tetra, q(1, 6));
    // No closed form for the inner stage: nested quadrature.
    let g = exp(-(x().pow(2) + y().pow(2)));
    let v = integrate_multiple(&g, &[("y", n(0), n(1)), ("x", n(0), n(1))], None).unwrap();
    approx(&v, 0.746_824_132_812_427_f64.powi(2));
    // Symbolic outer bound stays exact.
    let b = Expr::sym("b");
    let s = integrate_multiple(&n(1), &[("y", n(0), x()), ("x", n(0), b.clone())], None).unwrap();
    assert_eq!(s, b.pow(2) / n(2));
    // A bound may not use a variable integrated inside it.
    assert!(integrate_multiple(&xy, &[("y", n(0), y())], None).is_err());
}

#[test]
fn line_integrals() {
    let t = Expr::sym("t");
    let circle = [cos(t.clone()), sin(t.clone())];
    let two_pi = n(2) * Expr::pi();
    let work = line_integral(
        &[-y(), x()],
        &["x", "y"],
        &circle,
        "t",
        &n(0),
        &two_pi,
        None,
    )
    .unwrap();
    approx(&work, 2.0 * std::f64::consts::PI);
    let length =
        line_integral_scalar(&n(1), &["x", "y"], &circle, "t", &n(0), &two_pi, None).unwrap();
    approx(&length, 2.0 * std::f64::consts::PI);
}

fn notes(steps: &Steps) -> Vec<String> {
    steps
        .iter()
        .map(|s| format!("{}: {}", s.rule, s.note.clone().unwrap_or_default()))
        .collect()
}

#[test]
fn steps_show_the_technique() {
    let mut steps = Steps::new();
    integrate(&(x() * exp(x().pow(2))), "x", Some(&mut steps)).unwrap();
    let all = notes(&steps);
    assert!(
        all.iter().any(|s| s.starts_with("substitution")
            && s.contains("u = x^2")
            && s.contains("du = 2*x dx")),
        "{all:#?}"
    );

    let mut steps = Steps::new();
    integrate(&(x() * exp(x())), "x", Some(&mut steps)).unwrap();
    let all = notes(&steps);
    assert!(
        all.iter().any(|s| s.starts_with("integration by parts")
            && s.contains("u = x")
            && s.contains("dv = e^x dx")),
        "{all:#?}"
    );

    let mut steps = Steps::new();
    integrate(
        &((x() + n(1)) * (x() + n(2))).recip(),
        "x",
        Some(&mut steps),
    )
    .unwrap();
    let all = notes(&steps);
    assert!(
        all.iter()
            .any(|s| s.starts_with("partial fractions") && s.contains("decomposition")),
        "{all:#?}"
    );

    let mut steps = Steps::new();
    integrate(&(exp(x()) * sin(x())), "x", Some(&mut steps)).unwrap();
    assert!(steps.iter().any(|s| s.rule == "solve for the integral"));

    let mut steps = Steps::new();
    integrate(&(n(1) - x().pow(2)).sqrt(), "x", Some(&mut steps)).unwrap();
    assert!(notes(&steps).iter().any(|s| s.contains("sin θ")));

    let mut steps = Steps::new();
    integrate_definite(&exp(-x()), "x", &n(0), &Expr::infinity(), Some(&mut steps)).unwrap();
    assert!(steps
        .iter()
        .any(|s| s.rule == "fundamental theorem of calculus"));
}

fn poly_times() -> impl Strategy<Value = Expr> {
    let poly = prop::collection::vec(-3i64..=3, 1..=4).prop_map(|cs| {
        cs.iter()
            .enumerate()
            .fold(n(0), |acc, (k, c)| acc + n(*c) * x().pow(k as i64))
    });
    let k = prop_oneof![Just(1i64), Just(2), Just(3), Just(-1)];
    let kind = 0usize..4;
    (poly, k, kind).prop_map(|(p, k, kind)| {
        let kx = n(k) * x();
        let g = match kind {
            0 => exp(kx),
            1 => sin(kx),
            2 => cos(kx),
            _ => exp(kx.clone()) * sin(kx),
        };
        p * g
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(40))]

    #[test]
    fn polynomial_times_exp_or_trig_differentiates_back(e in poly_times()) {
        check(&e);
    }
}
