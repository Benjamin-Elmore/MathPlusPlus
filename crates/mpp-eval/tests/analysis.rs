//! Function analysis in documents: the operator forms (`\operatorname{crit} f = ?`) and
//! the helper sentences ("Find the critical points of $f$ on $[0, 4]$.") give the same
//! answers.

use std::sync::Arc;

use mpp_core::tools::table;
use mpp_eval::{Evaluated, LineKind, LineResult, NoImports, Session, Value};
use mpp_syntax::Severity;

fn run(src: &str) -> Evaluated {
    Session::new()
        .with_importer(Arc::new(NoImports))
        .run_str(src)
        .clone()
}

fn no_errors(ev: &Evaluated) {
    let errs: Vec<String> = ev.errors().iter().map(|d| d.message.clone()).collect();
    assert!(errs.is_empty(), "unexpected errors: {errs:?}");
}

/// Every answered line (queries and helper sentences) in order.
fn answered(ev: &Evaluated) -> Vec<LineResult> {
    ev.blocks
        .iter()
        .flat_map(|b| b.values.iter())
        .filter(|l| {
            matches!(l.kind, LineKind::Query { .. })
                || (l.kind == LineKind::Helper && l.value.is_some())
        })
        .cloned()
        .collect()
}

fn texts(src: &str) -> Vec<String> {
    let ev = run(src);
    no_errors(&ev);
    answered(&ev)
        .into_iter()
        .map(|l| l.text.unwrap_or_default())
        .collect()
}

fn row(line: &LineResult, label: &str) -> String {
    let Some(Value::Table(t)) = &line.value else {
        panic!("not a table: {:?}", line.value);
    };
    table::row(t, label)
        .map(|v| v.to_string())
        .unwrap_or_else(|| panic!("no row {label}"))
}

const F: &str = "$$ f(x) := x^3 - 3x^2 + 1 $$\n\n";

#[test]
fn operator_and_sentence_forms_agree() {
    let src = format!(
        "{F}$$ \\operatorname{{crit}} f = ? $$\n\nFind the critical points of $f$.\n\n\
         $$ \\operatorname{{tangent}}_{{x=1}} f = ? $$\n\nFind the tangent line of $f$ at $x = 1$.\n\n\
         $$ \\operatorname{{normal}}_{{x=1}} f = ? $$\n\nFind the normal line of $f$ at $x = 1$.\n\n\
         $$ \\operatorname{{linearisation}}_{{x=1}} f = ? $$\n\nFind the linearization of $f$ at $x = 1$.\n"
    );
    let t = texts(&src);
    assert_eq!(t[0], "{0, 2}");
    assert_eq!(t[0], t[1]);
    assert_eq!(t[2], "y = -3*x + 2");
    assert_eq!(t[2], t[3]);
    assert_eq!(t[4], "y = x/3 - 4/3");
    assert_eq!(t[4], t[5]);
    assert_eq!(t[6], "L(x) = -3*x + 2");
    assert_eq!(t[6], t[7]);
}

#[test]
fn closed_interval_method_on_a_sentence() {
    let src = format!("{F}Find the absolute extrema of $f$ on $[-1/2, 4]$.\n");
    let ev = run(&src);
    no_errors(&ev);
    let line = &answered(&ev)[0];
    assert_eq!(row(line, "absolute maximum"), "{(4, 17)}");
    assert_eq!(row(line, "absolute minimum"), "{(2, -3)}");
}

#[test]
fn domain_range_asymptotes_of_a_rational_function() {
    let src = "$$ g(x) := \\frac{2x^2 + 1}{x^2 - 1} $$\n\n\
               Find the domain of $g$.\n\n\
               $$ \\operatorname{range} g = ? $$\n\n\
               Find the asymptotes of $g$.\n";
    let ev = run(src);
    no_errors(&ev);
    let a = answered(&ev);
    assert_eq!(
        a[0].text.as_deref(),
        Some("union((-∞, -1), (-1, 1), (1, ∞))")
    );
    assert_eq!(a[1].text.as_deref(), Some("union((-∞, -1], (2, ∞))"));
    assert_eq!(row(&a[2], "vertical"), "{x = -1, x = 1}");
    assert_eq!(row(&a[2], "horizontal"), "{y = 2}");
}

#[test]
fn analyse_gives_a_summary_table() {
    let ev = run("$$ h(x) := x e^{-x} $$\n\nAnalyse $h$.\n");
    no_errors(&ev);
    let line = &answered(&ev)[0];
    assert_eq!(row(line, "local maxima"), "{(1, 1/e)}");
    assert_eq!(row(line, "inflection points"), "{(2, 2/e^2)}");
    assert_eq!(row(line, "horizontal asymptotes"), "{y = 0}");
    assert_eq!(row(line, "increasing on"), "(-∞, 1)");
    assert!(line
        .latex
        .as_deref()
        .is_some_and(|l| l.starts_with("\\begin{array}")));
}

#[test]
fn theorems_on_an_interval() {
    let src = "$$ p(x) := x^3 - x $$\n\n\
               $$ \\operatorname{mvt}_{[0, 2]} p = ? $$\n\n\
               $$ \\operatorname{ivt}_{[0, 1]}^{0} (x^3 + x - 1) = ? $$\n";
    let ev = run(src);
    no_errors(&ev);
    let a = answered(&ev);
    assert_eq!(row(&a[0], "average rate of change"), "3");
    assert_eq!(row(&a[0], "c with f′(c) equal to it"), "{2*sqrt(3)/3}");
    assert_eq!(row(&a[1], "0 lies between f(0) and f(1)"), "true");
}

#[test]
fn a_defined_variable_does_not_leak_into_the_helper() {
    // `x` has a value, but the helper's variable stays symbolic.
    let src = "$$ x := 5 $$\n\n$$ q(t) := t^2 - 4t $$\n\n$$ \\operatorname{crit} q = ? $$\n";
    assert_eq!(texts(src), vec!["{2}"]);
}

#[test]
fn what_cannot_be_settled_is_reported() {
    let ev = run("$$ \\operatorname{crit} \\sin x = ? $$\n");
    let errs: Vec<String> = ev
        .all_diagnostics()
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.clone())
        .collect();
    assert!(
        errs.iter().any(|e| e.contains("give an interval")),
        "{errs:?}"
    );
    // With an interval it works.
    let t = texts("Find the critical points of $\\sin x$ on $[0, 2\\pi]$.\n");
    assert_eq!(t, vec!["{π/2, 3*π/2}"]);
}

#[test]
fn numeric_fallback_is_marked() {
    let ev = run("$$ \\operatorname{crit} (x^5 - 5x + 1 + \\cos x) = ? $$\n");
    let notes: Vec<String> = ev
        .all_diagnostics()
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .map(|d| d.message.clone())
        .collect();
    assert!(notes.iter().any(|n| n.contains('≈')), "{notes:?}");
}
