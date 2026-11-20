//! Textbook multivariable and vector calculus problems with known answers.

use super::*;
use crate::tools::testkit::p;

fn s(e: &Expr) -> String {
    e.to_string()
}

fn ps(v: &[&str]) -> Vec<Expr> {
    v.iter().map(|x| p(x)).collect()
}

fn bound(v: &str, a: &str, b: &str) -> integrals::Bound {
    (v.to_string(), p(a), p(b))
}

fn close(e: &Expr, want: f64) -> bool {
    util::numeric(e).is_some_and(|v| (v - want).abs() <= 1e-6 * (1.0 + want.abs()))
}

#[test]
fn partials_gradient_directional() {
    assert_eq!(
        s(&partial(&p("x^3*y^2"), &[("x", 1), ("y", 1)]).unwrap()),
        "6*x^2*y"
    );
    let f = p("x^2*y + y^3");
    let g = gradient_at(&f, &["x", "y"], &ps(&["1", "2"])).unwrap();
    assert_eq!(g.iter().map(s).collect::<Vec<_>>(), vec!["4", "13"]);
    let d = directional_derivative_at(&p("x^2*y"), &["x", "y"], &ps(&["1", "2"]), &ps(&["3", "4"]))
        .unwrap();
    assert_eq!(s(&d), "16/5");
    assert!(
        directional_derivative_at(&f, &["x", "y"], &ps(&["1", "2"]), &ps(&["0", "0"])).is_err()
    );
}

#[test]
fn tangent_planes_and_linear_approximation() {
    let tp = tangent_plane(&p("x^2 + y^2"), &["x", "y"], &ps(&["1", "2"])).unwrap();
    assert_eq!(s(&tp), "z = 2*x + 4*y - 5");
    let sphere = Expr::equation(p("x^2 + y^2 + z^2"), p("14"));
    let tp = tangent_plane(&sphere, &["x", "y", "z"], &ps(&["1", "2", "3"])).unwrap();
    assert_eq!(s(&tp), "2*x + 4*y + 6*z - 28 = 0");
    assert!(tangent_plane(&sphere, &["x", "y", "z"], &ps(&["1", "1", "1"])).is_err());
    let l = linear_approximation(&p("sqrt(x^2 + y^2)"), &["x", "y"], &ps(&["3", "4"])).unwrap();
    assert_eq!(s(&l), "L(x, y) = 3*x/5 + 4*y/5");
}

#[test]
fn second_derivative_test_classifies() {
    let c = second_derivative_test(&p("x^3 + y^3 - 3*x*y"), "x", "y").unwrap();
    let got: Vec<(String, &str)> = c
        .iter()
        .map(|c| (format!("({}, {})", s(&c.point[0]), s(&c.point[1])), c.kind))
        .collect();
    assert_eq!(
        got,
        vec![
            ("(0, 0)".to_string(), "saddle point"),
            ("(1, 1)".to_string(), "local minimum")
        ]
    );
    assert_eq!(s(&c[1].value), "-1");
    let c = second_derivative_test(&p("4 - x^2 - y^2"), "x", "y").unwrap();
    assert_eq!(c[0].kind, "local maximum");
    let c = second_derivative_test(&p("x^4 + y^4"), "x", "y").unwrap();
    assert_eq!(c[0].kind, "inconclusive (D = 0)");
}

#[test]
fn lagrange_multipliers() {
    let pts = lagrange(
        &p("x + y"),
        &[Expression::eq("x^2 + y^2", "2")],
        &["x", "y"],
    )
    .unwrap();
    assert_eq!(pts.len(), 2);
    assert_eq!(s(&pts[0].value), "-2");
    assert_eq!(s(&pts[1].value), "2");
    assert_eq!(
        pts[1].point.iter().map(s).collect::<Vec<_>>(),
        vec!["1", "1"]
    );
    let pts = lagrange(&p("x*y"), &[Expression::eq("x^2 + y^2", "1")], &["x", "y"]).unwrap();
    assert_eq!(pts.len(), 4);
    assert_eq!(s(&pts[0].value), "-1/2");
    assert_eq!(s(&pts[3].value), "1/2");
    assert!(lagrange(&p("x"), &[], &["x", "y"]).is_err());
}

struct Expression;
impl Expression {
    fn eq(a: &str, b: &str) -> Expr {
        Expr::equation(p(a), p(b))
    }
}

#[test]
fn implicit_and_related_rates() {
    let circle = Expression::eq("x^2 + y^2", "25");
    assert_eq!(s(&implicit_derivative(&circle, "x", "y").unwrap()), "-x/y");
    let rel = related_rates(&circle, &["x", "y"], "t").unwrap();
    assert_eq!(s(&rel), "2*x*d/dt(x) + 2*y*d/dt(y) = 0");
    let (name, v) = solve_rates(
        &circle,
        &["x", "y"],
        "t",
        &[("x".into(), p("3")), ("y".into(), p("4"))],
        &[("x".into(), p("2"))],
    )
    .unwrap();
    assert_eq!((name.as_str(), s(&v).as_str()), ("y", "-3/2"));
    // y is fixed by the relation only up to sign: it must be given.
    assert!(solve_rates(
        &circle,
        &["x", "y"],
        "t",
        &[("x".into(), p("3"))],
        &[("x".into(), p("2"))]
    )
    .is_err());
}

#[test]
fn integrals_in_other_coordinates() {
    let polar = integrate_in(
        &p("x^2 + y^2"),
        Coordinates::Polar,
        &[bound("θ", "0", "2*pi"), bound("r", "0", "2")],
    )
    .unwrap();
    assert_eq!(s(&polar), "8*π");
    let ball = integrate_in(
        &p("1"),
        Coordinates::Spherical,
        &[
            bound("θ", "0", "2*pi"),
            bound("φ", "0", "pi"),
            bound("ρ", "0", "1"),
        ],
    )
    .unwrap();
    assert_eq!(s(&ball), "4*π/3");
    let cyl = integrate_in(
        &p("z"),
        Coordinates::Cylindrical,
        &[
            bound("θ", "0", "2*pi"),
            bound("r", "0", "1"),
            bound("z", "0", "2"),
        ],
    )
    .unwrap();
    assert_eq!(s(&cyl), "2*π");
    let (v, jac) = change_of_variables(
        &p("x + y"),
        &["x", "y"],
        &ps(&["u + v", "u - v"]),
        &[bound("u", "0", "1"), bound("v", "0", "1")],
    )
    .unwrap();
    assert_eq!((s(&v), s(&jac)), ("2".into(), "2".into()));
}

#[test]
fn vector_operators() {
    let f = ps(&["x*y", "y*z", "z*x"]);
    let v = ["x", "y", "z"];
    assert_eq!(s(&divergence(&f, &v).unwrap()), "x + y + z");
    assert_eq!(
        curl(&f, &v).unwrap().iter().map(s).collect::<Vec<_>>(),
        vec!["-y", "-z", "-x"]
    );
    assert_eq!(s(&laplacian(&p("x^2 + y^2 + z^2"), &v).unwrap()), "6");
    assert_eq!(
        curl(&ps(&["-y", "x"]), &["x", "y"])
            .unwrap()
            .iter()
            .map(s)
            .collect::<Vec<_>>(),
        vec!["2"]
    );
}

fn unit_sphere() -> Patch {
    Patch {
        map: ps(&["sin(u)*cos(v)", "sin(u)*sin(v)", "cos(u)"]),
        u: bound("u", "0", "pi"),
        v: bound("v", "0", "2*pi"),
    }
}

#[test]
fn surface_integrals() {
    let flux = flux_integral(&ps(&["x", "y", "z"]), &["x", "y", "z"], &unit_sphere()).unwrap();
    assert!(close(&flux, 4.0 * std::f64::consts::PI), "{flux}");
    let area = surface_integral(&p("1"), &["x", "y", "z"], &unit_sphere()).unwrap();
    assert!(close(&area, 4.0 * std::f64::consts::PI), "{area}");
}

#[test]
fn greens_theorem_both_sides() {
    let square = Patch {
        map: ps(&["u", "v"]),
        u: bound("u", "0", "1"),
        v: bound("v", "0", "1"),
    };
    let c = check_greens_theorem(&ps(&["-y", "x"]), &["x", "y"], &square).unwrap();
    assert!(c.agree);
    assert_eq!(s(&c.interior), "2");
    let disk = Patch {
        map: ps(&["r*cos(t)", "r*sin(t)"]),
        u: bound("r", "0", "1"),
        v: bound("t", "0", "2*pi"),
    };
    let c = check_greens_theorem(&ps(&["-y", "x"]), &["x", "y"], &disk).unwrap();
    assert!(c.agree, "{c:?}");
    assert!(close(&c.boundary, 2.0 * std::f64::consts::PI));
    // A region under a curve: y from 0 to x^2, x from 0 to 1, F = (x*y, x).
    let under = Patch {
        map: ps(&["u", "v"]),
        u: bound("u", "0", "1"),
        v: bound("v", "0", "u^2"),
    };
    let c = check_greens_theorem(&ps(&["x*y", "x"]), &["x", "y"], &under).unwrap();
    assert!(c.agree, "{c:?}");
}

#[test]
fn stokes_and_divergence_theorems() {
    let hemisphere = Patch {
        map: ps(&["sin(u)*cos(v)", "sin(u)*sin(v)", "cos(u)"]),
        u: bound("u", "0", "pi/2"),
        v: bound("v", "0", "2*pi"),
    };
    let c = check_stokes_theorem(&ps(&["-y", "x", "0"]), &["x", "y", "z"], &hemisphere).unwrap();
    assert!(c.agree, "{c:?}");
    assert!(close(&c.interior, 2.0 * std::f64::consts::PI));
    let cube = Solid {
        coords: None,
        bounds: vec![
            bound("x", "0", "1"),
            bound("y", "0", "1"),
            bound("z", "0", "1"),
        ],
    };
    let c = check_divergence_theorem(&ps(&["x", "y", "z"]), &["x", "y", "z"], &cube).unwrap();
    assert!(c.agree);
    assert_eq!(s(&c.interior), "3");
    let ball = Solid {
        coords: Some(Coordinates::Spherical),
        bounds: vec![
            bound("ρ", "0", "1"),
            bound("φ", "0", "pi"),
            bound("θ", "0", "2*pi"),
        ],
    };
    let c = check_divergence_theorem(&ps(&["x", "y", "z"]), &["x", "y", "z"], &ball).unwrap();
    assert!(c.agree, "{c:?}");
    assert!(close(&c.interior, 4.0 * std::f64::consts::PI));
}

#[test]
fn laplace_table_shift_and_inverse() {
    assert_eq!(
        s(&laplace(&p("t^2*exp(3*t)"), "t", "s").unwrap()),
        "2/(s - 3)^3"
    );
    assert_eq!(
        s(&laplace(&p("sin(2*t)"), "t", "s").unwrap()),
        "2/(s^2 + 4)"
    );
    assert_eq!(s(&laplace(&p("5"), "t", "s").unwrap()), "5/s");
    let shifted = laplace(&p("exp(-t)*cos(2*t)"), "t", "s").unwrap();
    assert_eq!(
        s(&inverse_laplace(&shifted, "s", "t").unwrap()),
        s(&p("exp(-t)*cos(2*t)"))
    );
    assert_eq!(
        s(&inverse_laplace(&p("1/(s^2 + 1)"), "s", "t").unwrap()),
        "sin(t)"
    );
    assert_eq!(
        s(&inverse_laplace(&p("1/(s*(s + 1))"), "s", "t").unwrap()),
        s(&p("1 - exp(-t)"))
    );
    assert_eq!(
        s(&inverse_laplace(&p("1/(s - 2)^2"), "s", "t").unwrap()),
        s(&p("t*exp(2*t)"))
    );
    assert!(inverse_laplace(&p("s^2/(s + 1)"), "s", "t").is_err());
    assert!(laplace(&p("ln(t)"), "t", "s").is_err());
}

#[test]
fn laplace_solves_odes() {
    // y'' + y = 0, y(0) = 1, y'(0) = 0.
    let (_, y) = laplace_ode(&ps(&["1", "0", "1"]), &p("0"), &ps(&["1", "0"]), "t", "s").unwrap();
    assert_eq!(s(&y), "cos(t)");
    // y'' − 3y' + 2y = 0, y(0) = 0, y'(0) = 1.
    let (_, y) = laplace_ode(&ps(&["2", "-3", "1"]), &p("0"), &ps(&["0", "1"]), "t", "s").unwrap();
    assert_eq!(s(&y), s(&p("exp(2*t) - exp(t)")));
    // y' + y = 1, y(0) = 0.
    let (_, y) = laplace_ode(&ps(&["1", "1"]), &p("1"), &ps(&["0"]), "t", "s").unwrap();
    assert_eq!(s(&y), s(&p("1 - exp(-t)")));
    assert!(laplace_ode(&ps(&["1", "1"]), &p("0"), &[], "t", "s").is_err());
}
