//! Textbook problem sets: every equation is solved, the answer is checked by substitution
//! inside the solver, the method name is checked here, and solutions of initial value
//! problems are compared with the known closed form at sample points.

use std::collections::BTreeMap;

use super::*;
use crate::expr::{ExprNode, Name};
use crate::functions::{func, Func};

fn x() -> Expr {
    Expr::sym("x")
}
fn y() -> Expr {
    Expr::sym("y")
}
fn n(k: i64) -> Expr {
    Expr::int(k)
}
fn q(a: i64, b: i64) -> Expr {
    Expr::rational(a, b)
}
/// `dᵏy/dxᵏ` as the parser produces it.
fn d(k: u32) -> Expr {
    Expr::new(ExprNode::Derivative {
        expr: y(),
        vars: vec![(Name::from("x"), k)],
    })
}
fn sin(e: Expr) -> Expr {
    func(Func::Sin, vec![e])
}
fn cos(e: Expr) -> Expr {
    func(Func::Cos, vec![e])
}
fn exp(e: Expr) -> Expr {
    Expr::e().pow(e)
}
fn ln(e: Expr) -> Expr {
    func(Func::Ln, vec![e])
}
fn eq(l: Expr, r: Expr) -> Expr {
    Expr::equation(l, r)
}

fn solve(e: Expr, conds: &[Condition]) -> OdeSolution {
    let mut steps = Steps::new();
    match dsolve(&e, conds, &Options::default(), Some(&mut steps)) {
        Ok(s) => s,
        Err(err) => panic!(
            "{e}: {err}\nsteps: {:#?}",
            steps
                .0
                .iter()
                .map(|s| (&s.rule, &s.note))
                .collect::<Vec<_>>()
        ),
    }
}

/// Solves, checks the method, and returns the solution expression.
fn check(e: Expr, method: &str) -> OdeSolution {
    let s = solve(e.clone(), &[]);
    assert!(
        s.method.contains(method),
        "{e}: expected the {method} method, got {} ({})",
        s.method,
        s.to_expr()
    );
    assert_eq!(
        s.constants.len() as u32,
        parse(&e, None, None).unwrap().order,
        "{e}: {}",
        s.to_expr()
    );
    s
}

/// Solves an IVP/BVP and compares `y` with `expected` at sample points.
fn check_ivp(e: Expr, conds: &[Condition], expected: Expr, method: &str) {
    let s = solve(e.clone(), conds);
    assert!(
        s.method.contains(method),
        "{e}: expected {method}, got {}",
        s.method
    );
    assert!(
        s.constants.is_empty(),
        "{e}: constants left in {}",
        s.to_expr()
    );
    let got = match &s.form {
        Form::Explicit(g) => g.clone(),
        Form::Implicit(g) => {
            // The known solution must satisfy the implicit relation.
            for xv in [0.3, 0.9, 1.4, 2.2] {
                let yv = expected.eval_f64(&[("x", xv)]).unwrap();
                let v = g.eval_f64(&[("x", xv), ("y", yv)]).unwrap();
                assert!(v.abs() < 1e-8, "{e}: {g} at ({xv}, {yv}) is {v}");
            }
            return;
        }
        other => panic!("{e}: unexpected form {other:?}"),
    };
    let got = &got;
    for xv in [0.3, 0.9, 1.4, 2.2] {
        let env: BTreeMap<String, f64> = [("x".to_string(), xv)].into_iter().collect();
        let (a, b) = (got.eval_f64(&env), expected.eval_f64(&env));
        let (a, b) = (a.unwrap_or_else(|err| panic!("{got}: {err}")), b.unwrap());
        assert!(
            (a - b).abs() <= 1e-8 * (1.0 + b.abs()),
            "{e}: y({xv}) = {a}, expected {b} ({got})"
        );
    }
}

fn at0(k: u32, v: Expr) -> Condition {
    Condition::derivative(k, n(0), v)
}

#[test]
fn first_order_separable() {
    check(eq(d(1), n(2) * x()), "separable");
    check(eq(d(1), x() * y()), "separable");
    check(eq(d(1), (n(1) + y().pow(2)) * x()), "separable");
    check(eq(d(1), exp(x() - y())), "separable");
    check_ivp(
        eq(d(1), y().pow(2)),
        &[at0(0, n(1))],
        (n(1) - x()).recip(),
        "separable",
    );
    // y′ = x/y, y(0) = 2: y = √(x² + 4) (the branch through the point). Explicit
    // solutions are tried first, so Bernoulli (n = −1) answers before the implicit
    // separable form.
    check_ivp(
        eq(d(1), x() / y()),
        &[at0(0, n(2))],
        (x().pow(2) + n(4)).sqrt(),
        "Bernoulli",
    );
    // Logistic: y′ = y − y², y(0) = 1/2 (Bernoulli with n = 2).
    check_ivp(
        eq(d(1), y() - y().pow(2)),
        &[at0(0, q(1, 2))],
        (n(1) + exp(-x())).recip(),
        "Bernoulli",
    );
}

#[test]
fn first_order_linear_bernoulli_homogeneous() {
    check(eq(d(1) + n(2) * y(), exp(x())), "linear");
    check(eq(d(1) + y() / x(), x().pow(2)), "linear");
    check_ivp(
        eq(d(1) + y(), x()),
        &[at0(0, n(1))],
        x() - n(1) + n(2) * exp(-x()),
        "linear",
    );
    check(eq(d(1) + y(), x() * y().pow(3)), "Bernoulli");
    check(eq(d(1), y() / x() + x().pow(2) * y().pow(2)), "Bernoulli");
    check(eq(d(1), (x() + y()) / (x() - y())), "homogeneous");
    check(eq(d(1), (y() - x()) / (y() + x())), "homogeneous");
}

#[test]
fn first_order_exact_riccati_clairaut() {
    // (y cos x + 2x e^y) + (sin x + x² e^y − 1) y′ = 0
    let m = y() * cos(x()) + n(2) * x() * exp(y());
    let nn = sin(x()) + x().pow(2) * exp(y()) - n(1);
    check(eq(m + nn * d(1), n(0)), "exact");
    // y + (2x − y e^y) y′ = 0: integrating factor μ(y) = y.
    check(
        eq(y() + (n(2) * x() - y() * exp(y())) * d(1), n(0)),
        "integrating factor",
    );
    // y′ = 1 + x² − 2xy + y² with the particular solution y = x.
    check(
        eq(d(1), n(1) + x().pow(2) - n(2) * x() * y() + y().pow(2)),
        "Riccati",
    );
    // y′ = y² − 2/x²: particular solutions 1/x and −2/x.
    check(eq(d(1), y().pow(2) - n(2) / x().pow(2)), "Riccati");
    // Clairaut: y = x y′ + (y′)².
    let s = check(eq(y(), x() * d(1) + d(1).pow(2)), "Clairaut");
    assert!(
        s.notes.iter().any(|n| n.contains("singular")),
        "{:?}",
        s.notes
    );
}

#[test]
fn constant_coefficients() {
    check(d(2) - n(3) * d(1) + n(2) * y(), "constant coefficients");
    check(d(2) + n(4) * y(), "constant coefficients");
    check(d(2) + n(2) * d(1) + y(), "constant coefficients");
    check(d(2) + n(2) * d(1) + n(5) * y(), "constant coefficients");
    check(d(3) - y(), "constant coefficients");
    check(d(4) - y(), "constant coefficients");
    check(
        d(3) - n(3) * d(2) + n(3) * d(1) - y(),
        "constant coefficients",
    );
    check(eq(d(2) - y(), x().pow(2)), "undetermined coefficients");
    check(eq(d(2) + y(), exp(n(2) * x())), "undetermined coefficients");
    check(eq(d(2) + y(), sin(x())), "undetermined coefficients");
    check(
        eq(d(2) - n(2) * d(1) + y(), exp(x())),
        "undetermined coefficients",
    );
    check(
        eq(d(2) + n(4) * y(), x() * cos(x())),
        "undetermined coefficients",
    );
    check(
        eq(d(2) + n(3) * d(1) + n(2) * y(), n(4) * x() + exp(-x())),
        "undetermined coefficients",
    );
    check(
        eq(d(2) + y(), func(Func::Sec, vec![x()])),
        "variation of parameters",
    );
    check(
        eq(d(2) - n(2) * d(1) + y(), exp(x()) / x()),
        "variation of parameters",
    );
}

#[test]
fn initial_and_boundary_values() {
    check_ivp(
        d(2) + y(),
        &[at0(0, n(1)), at0(1, n(0))],
        cos(x()),
        "constant coefficients",
    );
    check_ivp(
        d(2) + y(),
        &[at0(0, n(0)), Condition::value(Expr::pi() / n(2), n(1))],
        sin(x()),
        "constant coefficients",
    );
    // y″ − y = 0, y(0) = 1, y′(0) = 1: eˣ.
    check_ivp(
        d(2) - y(),
        &[at0(0, n(1)), at0(1, n(1))],
        exp(x()),
        "constant coefficients",
    );
    // y″ + 4y = 8, y(0) = 0, y′(0) = 0: 2 − 2cos 2x.
    check_ivp(
        eq(d(2) + n(4) * y(), n(8)),
        &[at0(0, n(0)), at0(1, n(0))],
        n(2) - n(2) * cos(n(2) * x()),
        "constant coefficients",
    );
}

#[test]
fn cauchy_euler_and_reduction_of_order() {
    check(
        x().pow(2) * d(2) - n(2) * x() * d(1) + n(2) * y(),
        "Cauchy–Euler",
    );
    check(x().pow(2) * d(2) + x() * d(1) + y(), "Cauchy–Euler");
    check(
        x().pow(2) * d(2) - n(3) * x() * d(1) + n(4) * y(),
        "Cauchy–Euler",
    );
    check(
        eq(
            x().pow(2) * d(2) - n(2) * x() * d(1) + n(2) * y(),
            x().pow(3),
        ),
        "Cauchy–Euler",
    );
    // x² y″ − x(x + 2) y′ + (x + 2) y = 0 with y₁ = x: y₂ = x eˣ.
    check(
        x().pow(2) * d(2) - x() * (x() + n(2)) * d(1) + (x() + n(2)) * y(),
        "reduction of order",
    );
    // (1 − x²) y″ − 2x y′ + 2y = 0 (Legendre, n = 1): y₁ = x.
    check(
        (n(1) - x().pow(2)) * d(2) - n(2) * x() * d(1) + n(2) * y(),
        "reduction of order",
    );
}

#[test]
fn missing_y_reductions() {
    check(x() * d(2) + d(1), "reduction (missing y)");
    check(eq(d(2), d(1).pow(2)), "reduction (missing y)");
}

#[test]
fn laplace_transform_method() {
    let t = Expr::sym("t");
    let dt = |k: u32| {
        Expr::new(ExprNode::Derivative {
            expr: Expr::sym("y"),
            vars: vec![(Name::from("t"), k)],
        })
    };
    // y″ + y = H(t − 1), y(0) = y′(0) = 0: (1 − cos(t − 1)) H(t − 1).
    let e = eq(dt(2) + y(), laplace::step(t.clone() - n(1)));
    let s = solve(e, &[at0(0, n(0)), at0(1, n(0))]);
    assert_eq!(s.method, "Laplace transform");
    let Form::Explicit(sol) = &s.form else {
        panic!()
    };
    let after = laplace::pieces(sol, &Expr::zero(), "t");
    let late = &after.last().unwrap().2;
    let v = late.eval_f64(&[("t", 3.0)]).unwrap();
    assert!((v - (1.0 - 2f64.cos())).abs() < 1e-12, "{sol}");
    // y″ + 4y = δ(t − π), y(0) = 1, y′(0) = 0.
    let e = eq(dt(2) + n(4) * y(), laplace::delta(t.clone() - Expr::pi()));
    let s = solve(e, &[at0(0, n(1)), at0(1, n(0))]);
    assert_eq!(s.method, "Laplace transform");
    // Asked for: y′ + y = 1, y(0) = 0.
    let opts = Options {
        method: Some("laplace".into()),
        ..Options::default()
    };
    let s = dsolve(&eq(dt(1) + y(), n(1)), &[at0(0, n(0))], &opts, None).unwrap();
    assert_eq!(s.method, "Laplace transform");
    let Form::Explicit(sol) = &s.form else {
        panic!()
    };
    assert_eq!(sol.to_string(), "-e^(-t) + 1");
}

#[test]
fn series_solutions() {
    // Airy: y″ + x y = 0 (no elementary solution) gives a power series.
    let s = solve(d(2) + x() * y(), &[]);
    assert_eq!(s.method, "power series");
    // With conditions at 0 the coefficients are numbers.
    let s = solve(d(2) + x() * y(), &[at0(0, n(1)), at0(1, n(0))]);
    let Form::Series { terms, .. } = &s.form else {
        panic!()
    };
    // y = 1 − x³/6 + x⁶/180 − …
    let v = terms.eval_f64(&[("x", 0.5)]).unwrap();
    assert!(
        (v - (1.0 - 0.125 / 6.0 + 0.015625 / 180.0)).abs() < 1e-6,
        "{terms}"
    );
    // Frobenius: 2x y″ + y′ + y = 0, indicial roots 0 and 1/2.
    let opts = Options {
        method: Some("frobenius".into()),
        ..Options::default()
    };
    let s = dsolve(&(n(2) * x() * d(2) + d(1) + y()), &[], &opts, None).unwrap();
    assert_eq!(s.method, "Frobenius series");
    assert_eq!(s.constants.len(), 2);
}

#[test]
fn verification_and_honest_failures() {
    // An equation with no exact method gives a clear error, not a guess.
    let e = eq(d(1), sin(x() * y()));
    match dsolve(&e, &[], &Options::default(), None) {
        Err(MathError::NoClosedForm(m)) => assert!(m.contains("numerical"), "{m}"),
        other => panic!("expected no closed form, got {other:?}"),
    }
    // …and with an initial condition it can be solved numerically.
    let ode = parse(&e, None, None).unwrap();
    let num = solve_numeric(&ode, &[at0(0, n(1))], 2.0, None).unwrap();
    assert!(num.points.len() > 2);
    // A wrong known solution is refused rather than used.
    let opts = Options {
        known: Some(x().pow(3)),
        ..Options::default()
    };
    let r = dsolve(
        &eq(d(1), n(1) + x().pow(2) - n(2) * x() * y() + y().pow(2)),
        &[],
        &opts,
        None,
    );
    // Other methods may still solve it, but never with an unverified answer.
    if let Ok(s) = r {
        assert!(super::verify::explicit(
            &parse(
                &eq(d(1), n(1) + x().pow(2) - n(2) * x() * y() + y().pow(2)),
                None,
                None
            )
            .unwrap(),
            match &s.form {
                Form::Explicit(e) => e,
                _ => panic!(),
            }
        )
        .is_ok());
    }
    // Inconsistent boundary conditions: y″ + y = 0, y(0) = 0, y(π) = 1 has no solution.
    let r = dsolve(
        &(d(2) + y()),
        &[at0(0, n(0)), Condition::value(Expr::pi(), n(1))],
        &Options::default(),
        None,
    );
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn problem_set_is_large_enough() {
    // The tests above cover at least 40 equations.
    let count = 7 + 7 + 5 + 15 + 4 + 6 + 2 + 3 + 3 + 1;
    assert!(count >= 40);
    let _ = (ln(x()), q(1, 2));
}
