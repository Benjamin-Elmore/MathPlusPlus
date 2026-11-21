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
fn classify_finds_the_real_critical_points() {
    let got = answer(r"$$ \operatorname{classify} (x y e^{-(x^2+y^2)/2}) = ? $$");
    let want = [
        "local maximum at (-1, -1): 1/e",
        "local minimum at (-1, 1): -1/e",
        "saddle point at (0, 0): 0",
        "local minimum at (1, -1): -1/e",
        "local maximum at (1, 1): 1/e",
    ];
    assert_eq!(got.lines().collect::<Vec<_>>(), want);
}

#[test]
fn range_of_data_and_range_of_a_function() {
    let doc = concat!(
        "| $u$ |\n|---|\n| 1 |\n| 2 |\n| 3 |\n| 4 |\n| 5 |\n| 10 |\n\n",
        r"$$ \operatorname{range}(u) = ? $$",
        "\n"
    );
    assert_eq!(answer(doc), "9");
    assert_eq!(
        answer(r"$$ \operatorname{range} x^3 - 3x = ? $$"),
        "(-∞, ∞)"
    );
}

#[test]
fn factor_splits_over_the_rationals() {
    assert_eq!(
        answer(r"$$ \operatorname{factor}(x^6 - 1) = ? $$"),
        "(x - 1)(x + 1)(x² + x + 1)(x² - x + 1)"
    );
    assert_eq!(
        answer(r"$$ \operatorname{factor}(x^5 + x + 1) = ? $$"),
        "(x² + x + 1)(x³ - x² + 1)"
    );
    assert_eq!(
        answer(r"$$ \operatorname{factor}(x^4 + 4) = ? $$"),
        "(x² - 2x + 2)(x² + 2x + 2)"
    );
    assert_eq!(
        answer(r"$$ \operatorname{factor}(x^2 y - y^3) = ? $$"),
        "y(x + y)(x - y)"
    );
}

#[test]
fn the_ratio_test_decides_factorial_over_power() {
    // aₖ₊₁/aₖ = (k/(k+1))^k → 1/e < 1.
    let ev = run(r"$$ \sum_{n=1}^{\infty} \frac{n!}{n^n} \approx ? $$");
    assert!(ev.errors().is_empty());
    let (_, line) = ev.queries().next().expect("a query");
    assert_eq!(line.approx.as_deref(), Some("1.879853862"));
    let ev = run(r"$$ \sum_{n=1}^{\infty} \frac{1}{\binom{2n}{n}} \approx ? $$");
    assert!(ev.errors().is_empty());
    let (_, line) = ev.queries().next().expect("a query");
    assert_eq!(line.approx.as_deref(), Some("0.7363998587"));
    assert_eq!(
        answer(r"$$ \lim_{n \to \infty} \frac{n^n}{(n+1)^{n}} = ? $$"),
        "1/e"
    );
}

#[test]
fn the_logistic_equation_is_solved_exactly() {
    assert_eq!(
        answer(r"$$ \operatorname{dsolve}(y' = y (1 - y)) = ? $$"),
        "y(x) = 1/(C e^(-x) + 1)"
    );
    assert_eq!(
        answer(r"$$ \operatorname{dsolve}(y' = y (1 - y), (0, 1/2)) = ? $$"),
        "y(x) = 1/(e^(-x) + 1)"
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
fn x_to_the_x_is_not_direct_substitution() {
    let ev = run(r"$$ \lim_{x \to 0^+} x^{x} = ?! $$");
    let (_, line) = ev.queries().next().expect("a query");
    assert_eq!(line.text.as_deref(), Some("1"));
    let steps = line.steps.as_ref().expect("steps");
    let rules: Vec<&str> = steps.iter().map(|s| s.rule.as_str()).collect();
    assert!(!rules.contains(&"direct substitution"), "{rules:?}");
    assert_eq!(rules.first(), Some(&"rewrite as an exponential"));
}

#[test]
fn set_builder_with_several_conditions() {
    assert_eq!(
        answer(r"$$ \{x \in \mathbb{R} : \sin x = 0, 0 \le x \le 7\} = ? $$"),
        "{0, π, 2π}"
    );
    assert_eq!(
        answer(r"$$ \{x \in \mathbb{R} : x^2 = 4, x > 0\} = ? $$"),
        "{2}"
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
