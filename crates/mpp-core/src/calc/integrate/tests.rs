//! Unit tests for the integration helpers.

use std::cmp::Ordering;

use super::engine::{fresh_name, linear_coeffs};
use super::numeric::adaptive;
use super::radical::sign_of;
use super::rational::to_rational;
use super::verify::check_antiderivative;
use crate::expr::Expr;

fn x() -> Expr {
    Expr::sym("x")
}

#[test]
fn linear_inner_functions() {
    let (a, b) = linear_coeffs(&(Expr::int(3) * x() + Expr::int(2)), "x").unwrap();
    assert_eq!((a, b), (Expr::int(3), Expr::int(2)));
    assert!(linear_coeffs(&x().pow(2), "x").is_none());
    assert!(linear_coeffs(&Expr::int(4), "x").is_none());
}

#[test]
fn fresh_names_avoid_clashes() {
    let e = Expr::sym("u") + x();
    assert_eq!(fresh_name(&[&e], "x"), "w");
    assert_eq!(fresh_name(&[&x()], "u"), "w");
}

#[test]
fn signs_of_textbook_constants() {
    let a = Expr::sym("a");
    assert_eq!(sign_of(&a.pow(2)), Some(Ordering::Greater));
    assert_eq!(sign_of(&-(a.pow(2))), Some(Ordering::Less));
    assert_eq!(sign_of(&a), None);
    assert_eq!(sign_of(&(Expr::pi() - Expr::int(4))), Some(Ordering::Less));
}

#[test]
fn rational_functions_are_read_in_lowest_terms() {
    let e = (x().pow(2) - Expr::int(1)) / (x() - Expr::int(1));
    let (n, d) = to_rational(&e, "x").unwrap();
    assert_eq!(n.degree(), Some(1));
    assert_eq!(d.degree(), Some(0));
    assert!(to_rational(&x().sqrt(), "x").is_none());
}

#[test]
fn wrong_antiderivatives_are_rejected() {
    assert!(check_antiderivative(&x(), &x().pow(2), "x").is_err());
    assert!(check_antiderivative(&x(), &(x().pow(2) / Expr::int(2)), "x").is_ok());
}

#[test]
fn gauss_kronrod_is_exact_for_polynomials() {
    let (v, err) = adaptive(&mut |t| Some(t.powi(5)), 0.0, 2.0, 1e-12, 10).unwrap();
    assert!((v - 64.0 / 6.0).abs() < 1e-12 && err < 1e-10);
    assert!(adaptive(&mut |_| None, 0.0, 1.0, 1e-10, 10).is_err());
}

fn ln(e: Expr) -> Expr {
    Expr::call("ln", vec![e]).unwrap()
}

#[test]
fn improper_integrals_with_logarithmic_tails() {
    use super::integrate_definite;
    use crate::error::MathError;
    let inv_x_ln2 = Expr::one() / (x() * ln(x()).pow(2));
    let inf = Expr::infinity();
    // ∫_3^∞ dx / (x ln²x) = 1/ln 3: the antiderivative −1/ln x creeps to 0 slowly.
    let v = integrate_definite(&inv_x_ln2, "x", &Expr::int(3), &inf, None).unwrap();
    assert_eq!(v, Expr::one() / ln(Expr::int(3)));
    let v = integrate_definite(&inv_x_ln2, "x", &Expr::int(2), &inf, None).unwrap();
    assert_eq!(v, Expr::one() / ln(Expr::int(2)));
    let v = integrate_definite(&inv_x_ln2, "x", &Expr::e(), &inf, None).unwrap();
    assert_eq!(v, Expr::one());
    // Written with (ln x)^3 the tail converges too: ∫_e^∞ dx / (x ln³x) = 1/2.
    let cube = Expr::one() / (x() * ln(x()).pow(3));
    let v = integrate_definite(&cube, "x", &Expr::e(), &inf, None).unwrap();
    assert_eq!(v, Expr::rational(1, 2));
    // ∫_2^∞ dx / (x ln x) = lim ln(ln x) diverges.
    let inv_x_ln = Expr::one() / (x() * ln(x()));
    assert!(matches!(
        integrate_definite(&inv_x_ln, "x", &Expr::int(2), &inf, None),
        Err(MathError::Divergent(_))
    ));
}

mod improper_oscillating {
    use crate::calc::integrate::{integrate, integrate_definite};
    use crate::error::MathError;
    use crate::eval::EmptyEnv;
    use crate::expr::Expr;
    use crate::functions::{func, Func};
    use crate::steps::Steps;

    fn x() -> Expr {
        Expr::sym("x")
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
    fn inf() -> Expr {
        Expr::infinity()
    }
    fn value(e: &Expr) -> f64 {
        e.eval_f64(&EmptyEnv).unwrap()
    }
    fn close(e: &Expr, v: f64, tol: f64) {
        let got = value(e);
        assert!(
            (got - v).abs() <= tol * (1.0 + v.abs()),
            "{e} ≈ {got}, expected {v}"
        );
    }

    #[test]
    fn dirichlet_integral_is_pi_over_two() {
        // The reported bug: this used to be called divergent.
        let v = integrate_definite(&(sin(x()) / x()), "x", &Expr::zero(), &inf(), None).unwrap();
        assert_eq!(v, Expr::rational(1, 2) * Expr::pi());
        let v = integrate_definite(
            &(sin(Expr::int(3) * x()) / x()),
            "x",
            &Expr::zero(),
            &inf(),
            None,
        )
        .unwrap();
        assert_eq!(v, Expr::rational(1, 2) * Expr::pi());
        // Over the whole line it is π.
        let v = integrate_definite(&(sin(x()) / x()), "x", &Expr::neg_infinity(), &inf(), None)
            .unwrap();
        assert_eq!(v, Expr::pi());
    }

    #[test]
    fn antiderivatives_use_si_ci_and_erf() {
        let f = integrate(&(sin(x()) / x()), "x", None).unwrap();
        assert_eq!(f, func(Func::Si, vec![x()]));
        let f = integrate(&(cos(Expr::int(2) * x()) / x()), "x", None).unwrap();
        assert_eq!(f, func(Func::Ci, vec![Expr::int(2) * x()]));
        let f = integrate(&exp(-x().pow(2)), "x", None).unwrap();
        assert_eq!(f.to_string(), "sqrt(π)*erf(x)/2");
        // ∫_0^1 sin x / x = Si(1) ≈ 0.946083
        let v =
            integrate_definite(&(sin(x()) / x()), "x", &Expr::zero(), &Expr::one(), None).unwrap();
        close(&v, 0.946_083_070_367_183, 1e-12);
    }

    #[test]
    fn gaussian_integrals() {
        let v = integrate_definite(&exp(-x().pow(2)), "x", &Expr::neg_infinity(), &inf(), None)
            .unwrap();
        assert_eq!(v, Expr::pi().sqrt());
        let v = integrate_definite(&exp(-x().pow(2)), "x", &Expr::zero(), &inf(), None).unwrap();
        assert_eq!(v, Expr::pi().sqrt() / Expr::int(2));
        // e^{−(x² + 2x)} = e·e^{−(x+1)²}: the whole line gives e√π.
        let g = exp(-(x().pow(2) + Expr::int(2) * x()));
        let v = integrate_definite(&g, "x", &Expr::neg_infinity(), &inf(), None).unwrap();
        close(&v, std::f64::consts::E * std::f64::consts::PI.sqrt(), 1e-12);
    }

    #[test]
    fn fresnel_integrals_are_summed_between_zeros() {
        let fresnel = (std::f64::consts::PI / 8.0).sqrt();
        let mut steps = Steps::new();
        let v = integrate_definite(
            &sin(x().pow(2)),
            "x",
            &Expr::zero(),
            &inf(),
            Some(&mut steps),
        )
        .unwrap();
        close(&v, fresnel, 1e-8);
        assert!(
            steps.iter().any(|s| s.rule == "integrate between zeros"),
            "{steps:?}"
        );
        let v = integrate_definite(&cos(x().pow(2)), "x", &Expr::zero(), &inf(), None).unwrap();
        close(&v, fresnel, 1e-8);
        // ∫_1^∞ cos x / √x: no elementary antiderivative, converges (Dirichlet test).
        let v =
            integrate_definite(&(cos(x()) / x().sqrt()), "x", &Expr::one(), &inf(), None).unwrap();
        // √(π/2) − 2∫₀¹ cos t² dt
        close(&v, -0.555_734_338_485_043_7, 1e-8);
    }

    #[test]
    fn divergence_is_only_reported_with_a_proof() {
        // Proven: ∫_1^∞ e^x/x has an integrand that grows without bound.
        let r = integrate_definite(&(exp(x()) / x()), "x", &Expr::one(), &inf(), None);
        assert!(
            matches!(&r, Err(MathError::Divergent(m)) if m.contains("divergence test")),
            "{r:?}"
        );
        // ∫_0^1 e^x/x behaves like 1/x at 0.
        let r = integrate_definite(&(exp(x()) / x()), "x", &Expr::zero(), &Expr::one(), None);
        assert!(
            matches!(&r, Err(MathError::Divergent(m)) if m.contains("limit comparison")),
            "{r:?}"
        );
        // ∫_2^∞ 1/ln x: x · (1/ln x) → ∞, so it diverges by comparison with 1/x.
        let r = integrate_definite(
            &func(Func::Ln, vec![x()]).recip(),
            "x",
            &Expr::int(2),
            &inf(),
            None,
        );
        assert!(matches!(&r, Err(MathError::Divergent(_))), "{r:?}");
    }

    #[test]
    fn unknown_convergence_is_said_plainly() {
        // x² sin(x²) oscillates with growing pieces and has no elementary antiderivative:
        // no value, and no test proves divergence, so the answer says it could not decide.
        let e = sin(x().pow(2)) * x().pow(2);
        let r = integrate_definite(&e, "x", &Expr::one(), &inf(), None);
        match r {
            Err(MathError::Undetermined(m)) => assert!(m.contains("converges"), "{m}"),
            Err(MathError::Divergent(_)) => {}
            other => panic!("expected no value, got {other:?}"),
        }
    }
}
