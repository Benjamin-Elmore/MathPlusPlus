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
    // Conditions written y(0) = 1 lose y today (y(0) reads as y·0): a clear message.
    let m = error(r"$$ \operatorname{dsolve}(y' = y, y(0) = 1) = ? $$");
    assert!(m.contains("tuple"), "{m}");
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
    let m = error(r"$$ \operatorname{riemann}(x^2, [0, 1], 4) = ? $$");
    assert!(m.contains("left"), "{m}");
    let m = error(r"$$ \operatorname{simpson}(x^2, [0, 1], 3) = ? $$");
    assert!(m.contains("even"), "{m}");
}
