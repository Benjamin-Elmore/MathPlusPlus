//! Regression tests from the language-feel review: whole documents written the way a
//! newcomer writes them from the guide, each test naming what went wrong.

use std::sync::Arc;

use mpp_eval::{Evaluated, NoImports, Session};
use mpp_syntax::Severity;

fn run(src: &str) -> Evaluated {
    Session::new()
        .with_importer(Arc::new(NoImports))
        .run_str(src)
        .clone()
}

/// The answers of every query, in order (`None` for a query with no answer).
fn answers(ev: &Evaluated) -> Vec<Option<String>> {
    ev.queries()
        .map(|(_, l)| {
            l.verdict
                .as_ref()
                .map(|v| v.describe())
                .or_else(|| l.text.clone())
                .or_else(|| l.approx.clone())
        })
        .collect()
}

fn messages(ev: &Evaluated, severity: Severity) -> Vec<String> {
    ev.all_diagnostics()
        .into_iter()
        .filter(|d| d.severity == severity)
        .map(|d| d.message.clone())
        .collect()
}

#[test]
fn display_maths_on_the_same_line_as_a_list_number_is_evaluated() {
    let src = concat!(
        "4. $$ \\int x \\cos x \\, dx = ?! $$\n",
        "\n",
        "7. Partial fractions: $$ \\int \\frac{1}{x^2 - 1} \\, dx = ? $$\n",
        "\n",
        "Evaluate: $$ 2 + 3 = ? $$\n",
    );
    let ev = run(src);
    let got = answers(&ev);
    assert_eq!(got.len(), 3, "{got:?}");
    assert!(got.iter().all(Option::is_some), "{got:?}");
    assert!(got[0].as_deref().unwrap().contains("sin"), "{got:?}");
    assert_eq!(got[2].as_deref(), Some("5"));
    assert!(
        ev.errors().is_empty(),
        "{:?}",
        messages(&ev, Severity::Error)
    );
}

#[test]
fn a_question_mark_nothing_evaluates_is_reported() {
    // Two dollars inside running text never open a display, so this `?` is prose.
    let ev = run("The cost is $$5 = ? in total.\n");
    let warnings = messages(&ev, Severity::Warning);
    assert!(
        warnings.iter().any(|w| w.contains("not evaluated")),
        "{warnings:?}"
    );
}

#[test]
fn an_unknown_function_that_echoes_itself_is_reported() {
    let ev = run(concat!(
        r"$$ \operatorname{taylor}(e^x, x, 0, 4) = ? $$",
        "\n\n",
        r"$$ \operatorname{foo}(x) = ? $$",
    ));
    let warnings = messages(&ev, Severity::Warning);
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("`taylor` is not a function") && w.contains("series")),
        "{warnings:?}"
    );
    assert!(warnings.iter().any(|w| w.contains("`foo`")), "{warnings:?}");
    // A set operation that rightly stays as written is not reported.
    let ev = run(r"$$ [0, 1) \cup (2, 3] = ? $$");
    assert!(messages(&ev, Severity::Warning).is_empty());
    // The Taylor series the guide shows works.
    let ev = run(r"$$ \operatorname{series}_{x=0}^{4} e^x = ? $$");
    let got = answers(&ev);
    assert!(got[0].as_deref().unwrap().contains("O(x"), "{got:?}");
}

/// The plain (ASCII) answer of every query, in order.
fn plain_answers(ev: &Evaluated) -> Vec<String> {
    ev.queries()
        .map(|(_, l)| {
            l.plain
                .clone()
                .or_else(|| l.text.clone())
                .unwrap_or_default()
        })
        .collect()
}

#[test]
fn dsolve_takes_y_of_0_for_any_order_and_answers_in_the_variable_given() {
    let ev = run(concat!(
        r"$$ \operatorname{dsolve}(y' = 3 - y, y, t, y(0) = 0) = ? $$",
        "\n\n",
        r"$$ \operatorname{dsolve}(y'' + y = 0, y, t, y(0) = 1, y'(0) = 0) = ? $$",
        "\n\n",
        r"$$ \operatorname{dsolve}(V' = (12 - V)/0.47, V, t, (0, 0)) = ? $$",
        "\n",
    ));
    assert!(
        ev.errors().is_empty(),
        "{:?}",
        messages(&ev, Severity::Error)
    );
    let got = plain_answers(&ev);
    // First order with y(0) = 0: explicit, in t.
    assert!(
        got[0].starts_with("y(t) = ") || got[0].starts_with("y = "),
        "{got:?}"
    );
    assert!(
        got[0].contains("exp(-t)") || got[0].contains("e^(-t)"),
        "{got:?}"
    );
    // Second order: named variable t, not x.
    assert_eq!(got[1], "y(t) = cos(t)", "{got:?}");
    assert!(!got[1].contains('x'));
    // The RC circuit: V = 12(1 - e^(-t/0.47)), explicit.
    assert!(
        got[2].starts_with("V(t) = ") || got[2].starts_with("V = "),
        "{got:?}"
    );
    assert!(!got[2].contains("ln"), "{got:?}");
}

#[test]
fn a_dsolve_answer_never_hides_a_unit_inside_an_exponential() {
    let ev = run(concat!(
        r"$$ \tau := 0.47\,\mathrm{s} $$",
        "\n\n",
        r"$$ \operatorname{dsolve}(\tau \frac{dV}{dt} + V = 12, V, t) = ? $$",
        "\n",
    ));
    let errors = messages(&ev, Severity::Error);
    assert!(
        errors.iter().any(|e| e.contains("with its units")),
        "{errors:?} {:?}",
        plain_answers(&ev)
    );
}

#[test]
fn a_letter_after_a_number_is_the_variable_when_it_is_bound_and_warned_otherwise() {
    // t is the function's parameter, ζ and g are defined: all variables.
    let ev = run(concat!(
        "$$ \\zeta := 0.2 $$\n\n",
        "$$ u(t) := e^{-\\zeta 20 t} \\cos(20 t) $$\n\n",
        "$$ u(0.1) \\approx ? $$\n\n",
        "$$ g \\equiv 9.81 $$\n\n",
        "$$ \\frac{9}{2 g} \\approx ? $$\n",
    ));
    assert!(messages(&ev, Severity::Warning).is_empty());
    let got = answers(&ev);
    assert_eq!(got[0].as_deref(), Some("cos(2)/e^(2/5)"), "{got:?}");
    assert_eq!(got[1].as_deref(), Some("50/109"), "{got:?}");
    // Unbound: read as the unit, with a warning naming both spellings.
    let ev = run("$$ 5 m + 2 m = ? $$\n");
    let w = messages(&ev, Severity::Warning);
    assert!(
        w.iter().any(|m| m.contains("metres") && m.contains("5m")),
        "{w:?}"
    );
    // A thin space is a deliberate unit: no warning.
    let ev = run("$$ 5\\,m + 2\\,m = ? $$\n");
    assert!(messages(&ev, Severity::Warning).is_empty());
}

#[test]
fn delta_x_is_one_name() {
    let ev =
        run("$$ \\Delta x := 5\\,\\mathrm{m} $$\n\n$$ \\Delta x = ? $$\n\n$$ 2 \\Delta x = ? $$\n");
    assert!(
        ev.errors().is_empty(),
        "{:?}",
        messages(&ev, Severity::Error)
    );
    let got = answers(&ev);
    assert_eq!(got[0].as_deref(), Some("5 m"), "{got:?}");
    assert_eq!(got[1].as_deref(), Some("10 m"), "{got:?}");
}

#[test]
fn the_evaluation_bar_puts_the_point_in() {
    let ev = run(concat!(
        r"$$ \left. \frac{d}{dx} x^3 \right|_{x=2} = ? $$",
        "\n\n",
        r"$$ \frac{d}{dt}(t^2) \bigg|_{t=2} = ? $$",
        "\n\n",
        r"$$ x^2 |_{x=3} = ? $$",
        "\n\n",
        r"$$ |x - 5| = ? $$",
        "\n",
    ));
    assert!(
        ev.errors().is_empty(),
        "{:?}",
        messages(&ev, Severity::Error)
    );
    let got = answers(&ev);
    assert_eq!(got[0].as_deref(), Some("12"), "{got:?}");
    assert_eq!(got[1].as_deref(), Some("4"), "{got:?}");
    assert_eq!(got[2].as_deref(), Some("9"), "{got:?}");
    // An absolute value is still an absolute value.
    assert!(got[3].as_deref().unwrap().contains('|'), "{got:?}");
}

#[test]
fn the_physics_worksheet_runs_as_first_written() {
    // The reviewer's worksheet without its workarounds: `u(t)` with `20 t` and a figure
    // of `u(t)`, `V(0) = 0` in dsolve, and a displayed ODE with no question.
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/language-feel/physics-as-written.mpp"),
    )
    .unwrap();
    let ev = run(&src);
    assert!(
        ev.errors().is_empty(),
        "{:?}",
        messages(&ev, Severity::Error)
    );
    // Nothing is reported for the displayed ODE, the methods used or `20 t`.
    let warnings = messages(&ev, Severity::Warning);
    assert!(warnings.is_empty(), "{warnings:?}");
    let got = plain_answers(&ev);
    assert!(
        got.iter()
            .any(|a| a.starts_with("V(t) = ") && !a.contains("ln")),
        "{got:?}"
    );
    assert!(got.iter().any(|a| a == "x(t) = cos(20*t)/10"), "{got:?}");
    assert!(got.last().is_some_and(|a| a == "cos(2)/e^(2/5)"), "{got:?}");
}

#[test]
fn a_variable_after_a_number_in_an_integral_is_the_variable() {
    let ev = run(concat!(r"$$ \int_0^1 2 t \, dt = ? $$", "\n"));
    assert!(messages(&ev, Severity::Warning).is_empty());
    assert_eq!(answers(&ev)[0].as_deref(), Some("1"));
}

#[test]
fn special_functions_the_evaluator_knows_are_not_called_unknown() {
    let ev = run(concat!(
        r"$$ \operatorname{lambertw}(1) = ? $$",
        "\n\n",
        r"$$ \operatorname{besselj}(0, 1) = ? $$",
        "\n\n",
        r"$$ \operatorname{ellipk}(1/2) = ? $$",
    ));
    let warnings = messages(&ev, Severity::Warning);
    assert!(
        !warnings.iter().any(|w| w.contains("is not a function")),
        "{warnings:?}"
    );
    assert!(answers(&ev).iter().all(Option::is_some));
}

#[test]
fn dsolve_for_a_name_the_document_defined_says_so() {
    let ev = run(concat!(
        r"$$ y(t) := 3 t $$",
        "\n\n",
        r"$$ \operatorname{dsolve}(y' = y, y, x) = ? $$",
    ));
    let errors = messages(&ev, Severity::Error);
    assert!(
        errors.iter().any(|e| e.contains("already defined")),
        "{errors:?}"
    );
    // With a fresh name it solves.
    let ev = run(r"$$ \operatorname{dsolve}(w' = w, w, x) = ? $$");
    assert!(messages(&ev, Severity::Error).is_empty());
}
