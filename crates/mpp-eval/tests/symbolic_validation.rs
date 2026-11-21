//! Regressions from the maths-correctness review of the symbolic side: each test gives
//! the exact input and the answer it must produce.

use std::sync::Arc;

use mpp_eval::{Evaluated, NoImports, Session};

fn run(src: &str) -> Evaluated {
    Session::new()
        .with_importer(Arc::new(NoImports))
        .run_str(src)
        .clone()
}

/// The first query's answer as text (exact, else the decimal), with no errors allowed.
fn answer(src: &str) -> String {
    let ev = run(src);
    let errs: Vec<String> = ev.errors().iter().map(|d| d.message.clone()).collect();
    assert!(errs.is_empty(), "{src}: {errs:?}");
    let (_, line) = ev.queries().next().expect("a query");
    line.text
        .clone()
        .or_else(|| line.approx.clone())
        .unwrap_or_default()
}

#[test]
fn a_where_binding_goes_in_after_differentiating() {
    assert_eq!(answer(r"$$ \frac{d}{dx} x^3 = ?, \quad x = 2 $$"), "12");
    let ev = run(r"$$ \frac{d}{dx} x^3 \approx ?, \quad x = 2 $$");
    assert!(ev.errors().is_empty());
    let (_, line) = ev.queries().next().expect("a query");
    assert_eq!(line.approx.as_deref(), Some("12"));
    assert_eq!(
        answer(r"$$ \frac{\partial}{\partial x} x^2 y = ?, \quad x = 3, y = 2 $$"),
        "12"
    );
}

#[test]
fn an_improper_integral_with_a_parameter_diverges_at_a_zero() {
    assert_eq!(
        answer(r"$$ \int_0^{\infty} e^{-a x} \, dx = ? $$"),
        "{ ∞ if a ≤ 0; 1/a otherwise }"
    );
}

#[test]
fn the_fundamental_theorem_differentiates_an_integral_with_no_closed_form() {
    assert_eq!(
        answer(r"$$ \frac{d}{dx} \int_0^{x} e^{t^2} \, dt = ? $$"),
        "e^(x²)"
    );
    assert_eq!(
        answer(r"$$ \frac{d}{dx} \int_{\sin x}^{x^3} e^{t^2} \, dt = ? $$"),
        "3x² e^(x⁶) - e^(sin(x)²) cos(x)"
    );
    assert_eq!(
        answer(r"$$ \frac{d}{dx} \int_0^{x^2} \sin t \, dt = ? $$"),
        "2x sin(x²)"
    );
}

#[test]
fn terms_after_a_limit_in_its_variable_go_inside_it() {
    assert_eq!(
        answer(r"$$ \lim_{x \to \infty} \sqrt{x^2 + x} - x = ? $$"),
        "1/2"
    );
    assert_eq!(
        answer(r"$$ 2 + \lim_{x \to 0} \frac{\sin x}{x} = ? $$"),
        "3"
    );
}
