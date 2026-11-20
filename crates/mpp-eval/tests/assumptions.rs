//! Assumptions in documents: `Assume …` sentences scoped top to bottom, bracketed
//! conditions for one block, the CAS using them, the safe form without them, and
//! contradictions.

use std::sync::Arc;

use mpp_eval::{Evaluated, NoImports, Session};

fn run(src: &str) -> Evaluated {
    Session::new()
        .with_importer(Arc::new(NoImports))
        .run_str(src)
        .clone()
}

fn errors(ev: &Evaluated) -> Vec<String> {
    ev.errors().iter().map(|d| d.message.clone()).collect()
}

fn answers(src: &str) -> Vec<String> {
    let ev = run(src);
    let errs = errors(&ev);
    assert!(errs.is_empty(), "{src}: unexpected errors {errs:?}");
    ev.queries()
        .map(|(_, l)| l.text.clone().unwrap_or_else(|| "<none>".into()))
        .collect()
}

#[test]
fn the_safe_form_without_assumptions() {
    let a = answers(
        r"$$ \sqrt{x^2} = ? $$
$$ |x| = ? $$
$$ \ln(e^x) = ? $$
$$ (x^a)^b = ? $$
",
    );
    assert_eq!(a, vec!["|x|", "|x|", "ln(e^x)", "(x^a)^b"]);
}

#[test]
fn the_working_says_why_the_safe_form_stays() {
    let ev = run(r"$$ \sqrt{x^2} = ?! $$");
    let (_, line) = ev.queries().next().expect("a query");
    let steps = line.steps.as_ref().expect("steps");
    assert!(
        steps.0.iter().any(|s| s
            .note
            .as_deref()
            .is_some_and(|n| n.contains("may be negative"))),
        "{steps:?}"
    );
}

#[test]
fn assumptions_hold_from_where_they_are_written() {
    let a = answers(
        r"$$ \sqrt{x^2} = ? $$

Assume $x > 0$.

$$ \sqrt{x^2} = ? $$
$$ |x| = ? $$
$$ \ln(e^x) = ? $$
$$ \sqrt{4 x^2} = ? $$
",
    );
    assert_eq!(a, vec!["|x|", "x", "x", "x", "2*x"]);
    let neg = answers("Assume $t < 0$.\n\n$$ |t| = ? $$\n$$ \\sqrt{t^2} = ? $$\n");
    assert_eq!(neg, vec!["-t", "-t"]);
}

#[test]
fn powers_of_powers_need_a_positive_base() {
    let a = answers(
        r"Assume $x > 0$, $a \in \mathbb{R}$ and $b \in \mathbb{R}$.

$$ (x^a)^b = ? $$
",
    );
    assert_eq!(a, vec!["x^(a*b)"]);
    let b = answers("Assume $n \\in \\mathbb{Z}$.\n\n$$ (y^2)^n = ? $$\n");
    assert_eq!(b, vec!["y^(2*n)"]);
}

#[test]
fn a_bracketed_condition_holds_for_its_block_only() {
    let a = answers(
        r"$$ \sqrt{y^2} = ? \quad (y < 0) $$
$$ \sqrt{y^2} = ? $$
",
    );
    assert_eq!(a, vec!["-y", "|y|"]);
    let block = answers("$$ \\sqrt{y^2} = ? \\quad (y \\ge 0) \\\\ |y| = ? $$\n");
    assert_eq!(block, vec!["y", "y"]);
}

#[test]
fn power_integrals_split_into_cases() {
    let a = answers(r"$$ \int x^n \, dx = ? $$");
    assert_eq!(
        a,
        vec!["{ x^(n + 1)/(n + 1) if n ≠ -1; ln(|x|) otherwise } + C"]
    );
    let b = answers("Assume $n \\neq -1$.\n\n$$ \\int x^n \\, dx = ? $$\n");
    assert_eq!(b, vec!["x^(n + 1)/(n + 1) + C"]);
    let c = answers("Assume $n > 0$.\n\n$$ \\int_0^1 x^n \\, dx = ? $$\n");
    assert_eq!(c, vec!["1/(n + 1)"]);
}

#[test]
fn limits_and_convergence_that_depend_on_a_sign() {
    let split = answers(r"$$ \lim_{x \to \infty} e^{a x} = ? $$");
    assert_eq!(split, vec!["{ ∞ if a > 0; 1 if a = 0; 0 otherwise }"]);
    let neg = answers("Assume $k > 0$.\n\n$$ \\lim_{x \\to \\infty} e^{-k x} = ? $$\n$$ \\int_0^\\infty e^{-k x} \\, dx = ? $$\n");
    assert_eq!(neg, vec!["0", "1/k"]);
    let p = answers("Assume $p > 1$.\n\n$$ \\int_1^\\infty \\frac{1}{x^p} \\, dx = ? $$\n");
    assert_eq!(p.len(), 1);
    let ev = run("Assume $p < 1$.\n\n$$ \\int_1^\\infty x^{-p} \\, dx = ? $$\n");
    assert!(
        errors(&ev).iter().any(|e| e.contains("diverges")),
        "{:?}",
        errors(&ev)
    );
    let pow = answers("Assume $q < 0$.\n\n$$ \\lim_{x \\to \\infty} x^q = ? $$\n");
    assert_eq!(pow, vec!["0"]);
}

#[test]
fn integers_and_trigonometry() {
    let a = answers(
        r"Assume $n \in \mathbb{Z}$.

$$ \cos(n \pi) = ? $$
$$ \sin(n \pi) = ? $$
",
    );
    assert_eq!(a, vec!["(-1)^n", "0"]);
}

#[test]
fn contradictions_are_errors() {
    let ev = run("Assume $x > 0$.\n\nAssume $x < -1$.\n");
    assert_eq!(
        errors(&ev),
        vec!["these assumptions can't all hold: x > 0 and x < -1"]
    );
    let ev = run("Assume $n \\in \\mathbb{Z}$, $0 < n$, $n < 1$.\n");
    assert_eq!(errors(&ev).len(), 1, "{:?}", errors(&ev));
    let ev = run("$$ x := 3 $$\n\nAssume $x > 0$.\n");
    assert!(errors(&ev)[0].contains("already has a value"));
    let ev = run("Assume $x^2 > 4$.\n");
    assert!(errors(&ev)[0].contains("not supported"));
}

#[test]
fn adding_an_assumption_reruns_the_blocks_below() {
    let mut session = Session::new().with_importer(Arc::new(NoImports));
    let before = "Intro.\n\n$$ \\sqrt{x^2} = ? $$\n";
    let after = "Assume $x > 0$.\n\n$$ \\sqrt{x^2} = ? $$\n";
    let first = session.run_str(before).clone();
    let a: Vec<String> = first
        .queries()
        .filter_map(|(_, l)| l.text.clone())
        .collect();
    assert_eq!(a, vec!["|x|"]);
    let second = session.update(after).clone();
    let b: Vec<String> = second
        .queries()
        .filter_map(|(_, l)| l.text.clone())
        .collect();
    assert_eq!(b, vec!["x"]);
}
