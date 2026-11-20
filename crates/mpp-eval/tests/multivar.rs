//! Multivariable and vector calculus helpers in documents.

use std::sync::Arc;

use mpp_core::tools::table;
use mpp_eval::{Evaluated, LineResult, NoImports, Session, Value};

fn run(src: &str) -> Evaluated {
    Session::new()
        .with_importer(Arc::new(NoImports))
        .run_str(src)
        .clone()
}

fn answers(src: &str) -> Vec<LineResult> {
    let ev = run(src);
    let errs: Vec<String> = ev.errors().iter().map(|d| d.message.clone()).collect();
    assert!(errs.is_empty(), "unexpected errors: {errs:?}\n{src}");
    ev.queries().map(|(_, l)| l.clone()).collect()
}

fn text(src: &str) -> String {
    let a = answers(src);
    assert_eq!(a.len(), 1, "{src}");
    a[0].text.clone().unwrap_or_default()
}

fn number(src: &str) -> f64 {
    let a = answers(src);
    let line = &a[0];
    line.approx
        .as_deref()
        .or(line.text.as_deref())
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("no number in {line:?}"))
}

fn row(src: &str, label: &str) -> String {
    let a = answers(src);
    let Some(Value::Table(t)) = &a[0].value else {
        panic!("not a table: {:?}", a[0].value);
    };
    table::row(t, label)
        .map(|v| v.to_string())
        .unwrap_or_else(|| panic!("no row {label} in {:?}", table::rows(t)))
}

const PI: f64 = std::f64::consts::PI;

#[test]
fn points_planes_and_linear_approximation() {
    let f = "$$ f(x, y) := x^2 + y^2 $$\n\n";
    assert_eq!(
        text(&format!(
            "{f}$$ \\operatorname{{tangent}}_{{(x,y)=(1,2)}} f = ? $$"
        )),
        "z = 2*x + 4*y - 5"
    );
    assert_eq!(
        text(&format!(
            "{f}$$ \\operatorname{{grad}}_{{(x,y)=(1,2)}} f = ? $$"
        )),
        "(2, 4)"
    );
    assert_eq!(
        text(&format!(
            "{f}$$ \\operatorname{{dirderiv}}_{{(x,y)=(1,2)}}(f, (3, 4)) = ? $$"
        )),
        "22/5"
    );
    assert_eq!(
        text("$$ \\operatorname{linearisation}_{(x,y)=(3,4)} \\sqrt{x^2 + y^2} = ? $$"),
        "L(x, y) = 3*x/5 + 4*y/5"
    );
    assert_eq!(
        text("$$ \\operatorname{tangentplane}_{(x,y,z)=(1,2,3)} (x^2 + y^2 + z^2 = 14) = ? $$"),
        "2*x + 4*y + 6*z - 28 = 0"
    );
    // The one-variable tangent still works.
    assert_eq!(
        text("$$ g(x) := x^2 $$\n\n$$ \\operatorname{tangent}_{x=1} g = ? $$"),
        "y = 2*x - 1"
    );
}

#[test]
fn second_derivative_test_and_lagrange() {
    let src = "$$ \\operatorname{classify} (x^3 + y^3 - 3xy) = ? $$";
    assert_eq!(row(src, "saddle point at (0, 0)"), "0");
    assert_eq!(row(src, "local minimum at (1, 1)"), "-1");
    let src = "$$ \\operatorname{lagrange}(x + y, x^2 + y^2 = 2) = ? $$";
    assert_eq!(row(src, "largest value"), "2");
    assert_eq!(row(src, "smallest value"), "-2");
    assert_eq!(row(src, "f(1, 1)"), "2");
}

#[test]
fn coordinates_and_change_of_variables() {
    assert_eq!(
        text("$$ \\operatorname{polar}(x^2 + y^2, \\theta \\in [0, 2\\pi], r \\in [0, 2]) = ? $$"),
        "8*π"
    );
    assert_eq!(
        text("$$ \\operatorname{spherical}_{\\theta \\in [0, 2\\pi], \\phi \\in [0, \\pi], \\rho \\in [0, 1]} 1 = ? $$"),
        "4*π/3"
    );
    assert_eq!(
        text("$$ \\operatorname{cylindrical}(z, \\theta \\in [0, 2\\pi], r \\in [0, 1], z \\in [0, 2]) = ? $$"),
        "2*π"
    );
    assert_eq!(
        text("$$ \\operatorname{changevars}(x + y, (x, y) = (u + v, u - v), u \\in [0, 1], v \\in [0, 1]) = ? $$"),
        "2"
    );
}

#[test]
fn line_and_surface_integrals() {
    assert!(
        (number("$$ \\operatorname{lineintegral}((-y, x), (\\cos t, \\sin t), t \\in [0, 2\\pi]) = ? $$")
            - 2.0 * PI)
            .abs()
            < 1e-9
    );
    // Arc length of the unit circle.
    assert!(
        (number(
            "$$ \\operatorname{lineintegral}(1, (\\cos t, \\sin t), t \\in [0, 2\\pi]) = ? $$"
        ) - 2.0 * PI)
            .abs()
            < 1e-9
    );
    let sphere = "(\\sin u \\cos v, \\sin u \\sin v, \\cos u), u \\in [0, \\pi], v \\in [0, 2\\pi]";
    assert!(
        (number(&format!(
            "$$ \\operatorname{{flux}}((x, y, z), {sphere}) = ? $$"
        )) - 4.0 * PI)
            .abs()
            < 1e-6
    );
}

#[test]
fn integral_theorems_compare_both_sides() {
    let src = "$$ \\operatorname{green}((-y, x), x \\in [0, 1], y \\in [0, 1]) = ? $$";
    assert_eq!(row(src, "the two sides agree"), "true");
    assert_eq!(row(src, "∮ P dx + Q dy"), "2");
    let src = "$$ \\operatorname{green}((x y, x), x \\in [0, 1], y \\in [0, x^2]) = ? $$";
    assert_eq!(row(src, "the two sides agree"), "true");
    let src = "$$ \\operatorname{stokes}((-y, x, 0), (\\sin u \\cos v, \\sin u \\sin v, \\cos u), u \\in [0, \\pi/2], v \\in [0, 2\\pi]) = ? $$";
    assert_eq!(row(src, "the two sides agree"), "true");
    let src =
        "$$ \\operatorname{divthm}((x, y, z), x \\in [0, 1], y \\in [0, 1], z \\in [0, 1]) = ? $$";
    assert_eq!(row(src, "∭ ∇ · F dV"), "3");
    assert_eq!(row(src, "the two sides agree"), "true");
    let src = "$$ \\operatorname{divthm}((x, y, z), \\rho \\in [0, 1], \\phi \\in [0, \\pi], \\theta \\in [0, 2\\pi]) = ? $$";
    assert_eq!(row(src, "the two sides agree"), "true");
}

#[test]
fn implicit_differentiation_and_related_rates() {
    assert_eq!(
        text("$$ \\operatorname{implicit}_x (x^2 + y^2 = 25) = ? $$"),
        "d/dx(y) = -x/y"
    );
    assert_eq!(
        text("$$ \\operatorname{rates}_t (x^2 + y^2 = 25) = ? $$"),
        "2*x*d/dt(x) + 2*y*d/dt(y) = 0"
    );
    assert_eq!(
        text("$$ \\operatorname{rates}_t (x^2 + y^2 = 25, x = 3, y = 4, dx = 2) = ? $$"),
        "d/dt(y) = -3/2"
    );
    // A defined x does not leak in.
    assert_eq!(
        text("$$ x := 7 $$\n\n$$ \\operatorname{implicit}_x (x y = 1) = ? $$"),
        "d/dx(y) = -y/x"
    );
}

#[test]
fn laplace_transforms_and_odes() {
    assert_eq!(
        text("$$ \\operatorname{laplace} t^2 e^{3t} = ? $$"),
        "2/(s - 3)^3"
    );
    assert_eq!(
        text("$$ \\operatorname{invlaplace} \\frac{1}{s^2 + 1} = ? $$"),
        "sin(t)"
    );
    assert_eq!(
        text("$$ \\operatorname{lsolve}((1, 0, 1), 0, (1, 0)) = ? $$"),
        "y(t) = cos(t)"
    );
    let a = answers("$$ \\operatorname{lsolve}((2, -3, 1), 0, (0, 1)) = ?! $$");
    let tree = a[0].step_tree().expect("steps");
    assert!(tree
        .rules()
        .contains(&"inverse transform by partial fractions"));
}
