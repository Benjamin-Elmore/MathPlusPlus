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
