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
