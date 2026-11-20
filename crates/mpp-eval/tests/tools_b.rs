//! The second set of calculus tools through whole documents: differential equations,
//! special functions and the operator forms' error messages.

use std::sync::Arc;

use mpp_eval::{Evaluated, NoImports, Session};

fn run(src: &str) -> Evaluated {
    Session::new()
        .with_importer(Arc::new(NoImports))
        .run_str(src)
        .clone()
}

fn answer(src: &str) -> String {
    let ev = run(src);
    let errs: Vec<String> = ev.errors().iter().map(|d| d.message.clone()).collect();
    assert!(errs.is_empty(), "{src}: {errs:?}");
    let (_, line) = ev.queries().next().expect("a query");
    line.text.clone().unwrap_or_default()
}

fn error(src: &str) -> String {
    let ev = run(src);
    ev.errors()
        .first()
        .map(|d| d.message.clone())
        .unwrap_or_else(|| panic!("{src} should fail"))
}

#[test]
fn differential_equations_in_documents() {
    assert_eq!(
        answer(r"$$ \operatorname{solve}(y' = 2y) = ? $$"),
        "y(x) = e^(2*x)*C"
    );
    assert_eq!(
        answer(r"$$ \operatorname{dsolve}(\frac{dy}{dx} = x y) = ? $$"),
        "y(x) = e^(x^2/2)*C"
    );
    assert_eq!(
        answer(r"$$ \operatorname{dsolve}(y'' + 4y = 0, (0, 0, 2)) = ? $$"),
        "y(x) = sin(2*x)"
    );
    // The method is named in the working.
    let ev = run(r"$$ \operatorname{dsolve}(y' + y = x) = ?! $$");
    let (_, line) = ev.queries().next().unwrap();
    let steps = line.steps.as_ref().expect("steps");
    assert!(steps.iter().any(|s| s.rule.contains("linear")), "{steps:?}");
    // Conditions written y(0) = 1: y is the unknown function, so y(0) is its value at 0.
    assert_eq!(
        answer(r"$$ \operatorname{dsolve}(y' = y, y(0) = 1) = ? $$"),
        "y(x) = e^x"
    );
}

#[test]
fn initial_conditions_of_the_unknown_function() {
    // y(0) and y'(0) after a bare equation are values of the unknown, not y·0.
    assert_eq!(
        answer(r"$$ y'' + y = 0 = ?, \quad y(0) = 1, y'(0) = 0 $$"),
        "y(x) = cos(x)"
    );
    // The Insert menu's form.
    assert_eq!(
        answer(
            r"$$ \operatorname{solve}_{y(x)} \left( {y'' + y = 0} \right) = ?, \quad y(0) = 0, y'(0) = 2 $$"
        ),
        "y(x) = 2*sin(x)"
    );
    // A value of y elsewhere is still a product: y has a value here.
    let src = concat!("$$ y := 3 $$", "\n\n", r"$$ y(2) = ? $$");
    assert_eq!(answer(src), "6");
}

#[test]
fn leibniz_and_dot_notation_stay_derivatives_of_the_unknown() {
    assert_eq!(
        answer(r"$$ \frac{dy}{dx} = x y = ? $$"),
        "y(x) = e^(x^2/2)*C"
    );
    assert_eq!(
        answer(r"$$ dy/dx = 2y = ?, \quad y(0) = 3 $$"),
        "y(x) = 3*e^(2*x)"
    );
    assert_eq!(
        answer(r"$$ \frac{dx}{dt} = -2x = ?, \quad x(0) = 3 $$"),
        "x(t) = 3*e^(-2*t)"
    );
    // A system written as a brace, with x and y unknown functions of t.
    assert_eq!(
        answer(
            r"$$ \operatorname{solve}_{x, y} \left( \begin{cases} x' = y \\ y' = -x \end{cases} \right) = ?, \quad x(0) = 1, y(0) = 0 $$"
        ),
        "(x(t) = cos(t), y(t) = -sin(t))"
    );
}

#[test]
fn step_and_delta_functions() {
    assert_eq!(answer(r"$$ H(2) - H(-1) = ? $$"), "1");
    // At the jump the conventions differ, so H(0) stays as written.
    assert_eq!(answer(r"$$ H(0) = ? $$"), "H(0)");
    assert_eq!(answer(r"$$ \delta(3) = ? $$"), "0");
    assert_eq!(
        answer(r"$$ \operatorname{dsolve}(y'' + y = H(t - 1), (0, 0, 0)) = ? $$"),
        "y(t) = H(t - 1)*(-cos(t - 1) + 1)"
    );
}

#[test]
fn derivatives_of_unknown_functions_stay_symbolic() {
    // y has no value, so dy/dx is not 0: it stays as written.
    assert_eq!(answer(r"$$ \frac{d}{dx} y = ? $$"), "d/dx(y)");
    // A known value is still differentiated.
    let src = concat!("$$ a := 3 $$", "\n\n", r"$$ \frac{d}{dx} (a x^2) = ? $$");
    assert_eq!(answer(src), "6*x");
}

#[test]
fn special_functions_and_messages() {
    assert_eq!(answer(r"$$ \operatorname{erf}(0) = ? $$"), "0");
    assert_eq!(answer(r"$$ \operatorname{Si}(0) = ? $$"), "0");
    // The names typed as commands.
    assert_eq!(answer(r"$$ \erf(0) + \Si(0) = ? $$"), "0");
    let m = error(r"$$ \operatorname{riemann}(x^2, [0, 1], 4) = ? $$");
    assert!(m.contains("left"), "{m}");
    let m = error(r"$$ \operatorname{simpson}(x^2, [0, 1], 3) = ? $$");
    assert!(m.contains("even"), "{m}");
}
