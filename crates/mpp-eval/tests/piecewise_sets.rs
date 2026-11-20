//! Piecewise functions, intervals and sets as values: calculus across pieces with the
//! joins reported, set operations, set-builder notation and inequalities solved to
//! intervals.

use std::sync::Arc;

use mpp_eval::{Evaluated, NoImports, Session};
use mpp_syntax::Severity;

fn run(src: &str) -> Evaluated {
    Session::new()
        .with_importer(Arc::new(NoImports))
        .run_str(src)
        .clone()
}

fn errors(ev: &Evaluated) -> Vec<String> {
    ev.errors().iter().map(|d| d.message.clone()).collect()
}

fn notes(ev: &Evaluated) -> Vec<String> {
    ev.all_diagnostics()
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .map(|d| d.message.clone())
        .collect()
}

fn answers(src: &str) -> Vec<String> {
    let ev = run(src);
    let errs = errors(&ev);
    assert!(errs.is_empty(), "{src}: unexpected errors {errs:?}");
    ev.queries()
        .map(|(_, l)| l.text.clone().unwrap_or_else(|| "<none>".into()))
        .collect()
}

fn answer(src: &str) -> String {
    let a = answers(src);
    assert_eq!(a.len(), 1, "{src}: {a:?}");
    a[0].clone()
}

const SMOOTH: &str = r"$$ f(x) := \begin{cases} x^2 & x < 1 \\ 2x - 1 & x \ge 1 \end{cases} $$
";

#[test]
fn piecewise_values_and_calculus() {
    let src = format!(
        r"{SMOOTH}
$$ f(-2) = ? $$
$$ f(3) = ? $$
$$ \frac{{d}}{{dx}} f(x) = ? $$
$$ \int_0^2 f(x) \, dx = ? $$
$$ \int f(x) \, dx = ? $$
$$ \int_2^0 f(x) \, dx = ? $$
"
    );
    assert_eq!(
        answers(&src),
        vec![
            "4",
            "5",
            "{ 2*x if x < 1; 2 if x ≥ 1 }",
            "7/3",
            "{ x^3/3 if x < 1; x^2 - x + 1/3 if x ≥ 1 } + C",
            "-7/3"
        ]
    );
    let ev = run(&src);
    assert!(notes(&ev)
        .iter()
        .any(|n| n == "at x = 1 it is continuous and differentiable"));
}

#[test]
fn corners_and_jumps_at_the_joins() {
    let corner = r"$$ g(x) := \begin{cases} -x & x < 0 \\ x & \text{otherwise} \end{cases} $$
$$ \frac{d}{dx} g(x) = ? $$
";
    assert_eq!(answers(corner), vec!["{ -1 if x < 0; 1 if x > 0 }"]);
    let n = notes(&run(corner));
    assert!(
        n.iter()
            .any(|m| m.contains("at x = 0 it is continuous but not differentiable")),
        "{n:?}"
    );
    let jump = r"$$ h(x) := \begin{cases} 1 & x < 2 \\ 3 & x \ge 2 \end{cases} $$
$$ \frac{d}{dx} h(x) = ? $$
$$ \int_0^4 h(x) \, dx = ? $$
";
    assert_eq!(answers(jump), vec!["{ 0 if x < 2; 0 if x > 2 }", "8"]);
    let n = notes(&run(jump));
    assert!(n.iter().any(|m| m.contains("jumps from 1 to 3")), "{n:?}");
    // Pieces defined line by line with conditions.
    let lines = r"$$ k(x) := x^2, x \ge 0 \\ k(x) := -x, x < 0 $$
$$ \int_{-1}^{1} k(x) \, dx = ? $$
";
    assert_eq!(answers(lines), vec!["5/6"]);
}

#[test]
fn intervals_and_set_operations() {
    assert_eq!(answer(r"$$ [0, 1) \cup (2, 3] = ? $$"), "[0, 1) ∪ (2, 3]");
    assert_eq!(answer(r"$$ [0, 3] \cap (1, 5) = ? $$"), "(1, 3]");
    assert_eq!(
        answer(r"$$ [0, 3] \setminus \{1\} = ? $$"),
        "[0, 1) ∪ (1, 3]"
    );
    assert_eq!(
        answer(r"$$ \mathbb{R} \setminus [0, 1] = ? $$"),
        "(-∞, 0) ∪ (1, ∞)"
    );
    assert_eq!(answer(r"$$ [0, 1]^c = ? $$"), "(-∞, 0) ∪ (1, ∞)");
    assert_eq!(answer(r"$$ 2 \in [0, 1) \cup (1, 3] = ? $$"), "true");
    assert_eq!(answer(r"$$ 1 \in [0, 1) \cup (1, 3] = ? $$"), "false");
    assert_eq!(answer(r"$$ [1, 2] \subset [0, 3] = ? $$"), "true");
    // Named sets.
    assert_eq!(
        answers("$$ A := [0, 2] $$\n$$ B := (1, 4) $$\n$$ A \\cap B = ? $$\n$$ A \\cup B = ? $$\n"),
        vec!["(1, 2]", "[0, 4)"]
    );
}

#[test]
fn set_builder_notation() {
    assert_eq!(
        answer(r"$$ \{x \in \mathbb{R} : x^2 < 4\} = ? $$"),
        "(-2, 2)"
    );
    assert_eq!(
        answer(r"$$ \{x \mid x^2 \ge 9\} = ? $$"),
        "(-∞, -3] ∪ [3, ∞)"
    );
    assert_eq!(
        answer(r"$$ \{n \in \mathbb{Z} : n^2 < 10\} = ? $$"),
        "{-3, -2, -1, 0, 1, 2, 3}"
    );
    assert_eq!(answer(r"$$ \{x \in [0, 5] : x > 2\} = ? $$"), "(2, 5]");
    // The variable of a set-builder is its own, even when the document defines it.
    assert_eq!(
        answers("$$ x := 7 $$\n$$ \\{x \\in \\mathbb{R} : x < 1\\} = ? $$\n"),
        vec!["(-∞, 1)"]
    );
}

#[test]
fn inequalities_solve_to_intervals() {
    assert_eq!(answer("Solve $x^2 - 4 < 0$ for $x$."), "x ∈ (-2, 2)");
    assert_eq!(
        answer("Solve $x^2 \\ge 4$ for $x$."),
        "x ∈ (-∞, -2] ∪ [2, ∞)"
    );
    assert_eq!(
        answer("Solve $\\frac{x - 1}{x + 2} \\le 0$ for $x$."),
        "x ∈ (-2, 1]"
    );
    assert_eq!(answer("Solve $0 < 2x - 1 \\le 3$ for $x$."), "x ∈ (1/2, 2]");
    assert_eq!(
        answer(r"$$ \operatorname{solve}_x (\sqrt{x} < 2) = ? $$"),
        "[0, 4)"
    );
    assert_eq!(answer("Solve $x^2 + 1 < 0$ for $x$."), "no solution");
    let ev = run("Solve $\\sin x > 0$ for $x$.");
    assert_eq!(errors(&ev).len(), 1, "{:?}", errors(&ev));
}
