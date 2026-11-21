//! "Show the working": `= ?!` queries and the session's "Show steps" option give a step
//! tree whose rules read like a textbook solution and whose last step is the answer.

use std::sync::Arc;

use mpp_core::step_tree::StepTree;
use mpp_eval::{Evaluated, LineResult, NoImports, Session};

fn session() -> Session {
    Session::new().with_importer(Arc::new(NoImports))
}

fn run(src: &str) -> Evaluated {
    session().run_str(src).clone()
}

fn only_query(ev: &Evaluated) -> LineResult {
    let errs: Vec<String> = ev
        .errors()
        .iter()
        .map(|d| mpp_core::note::plain(&d.message))
        .collect();
    assert!(errs.is_empty(), "unexpected errors: {errs:?}");
    let q: Vec<&LineResult> = ev.queries().map(|(_, l)| l).collect();
    assert_eq!(q.len(), 1);
    q[0].clone()
}

fn tree_of(src: &str) -> (LineResult, StepTree) {
    let line = only_query(&run(src));
    let tree = line.step_tree().expect("a step tree");
    (line, tree)
}

/// The last step ends at the answer the query shows.
fn ends_at_answer(line: &LineResult, tree: &StepTree) {
    let answer = line.value.as_ref().expect("a value").expr();
    let last = tree.final_result().expect("steps");
    let shown = last.to_string();
    assert!(
        *last == answer || shown == answer.to_string() || shown.ends_with(&answer.to_string()),
        "last step {shown} is not the answer {answer}\n{}",
        tree.to_text()
    );
}

#[test]
fn derivative_rules_are_named_per_step() {
    let (line, tree) = tree_of("$$ \\frac{d}{dx} x^2 \\sin(3x) = ?! $$");
    let rules = tree.rules();
    for rule in ["product rule", "power rule", "chain rule"] {
        assert!(rules.contains(&rule), "{rule} missing from {rules:?}");
    }
    assert_eq!(tree.roots[0].rule, "product rule");
    assert!(!tree.roots[0].children.is_empty(), "the product rule folds");
    ends_at_answer(&line, &tree);
    for (_, node) in tree.walk() {
        assert!(!node.sentence.is_empty());
        assert!(!node.before_latex.is_empty() && !node.after_latex.is_empty());
    }
}

#[test]
fn substitution_shows_u_and_du() {
    let (line, tree) = tree_of("$$ \\int 2x \\cos(x^2) \\, dx = ?! $$");
    let sub = tree
        .walk()
        .into_iter()
        .find(|(_, n)| n.rule == "substitution")
        .map(|(_, n)| n.clone())
        .expect("a substitution step");
    assert_eq!(sub.note.as_deref(), Some("u = x^2, du = 2·x dx"));
    assert_eq!(
        sub.children.last().map(|c| c.rule.as_str()),
        Some("substitute back")
    );
    ends_at_answer(&line, &tree);
}

#[test]
fn integration_by_parts_names_u_dv_du_v() {
    let (line, tree) = tree_of("$$ \\int x e^x \\, dx = ?! $$");
    let parts = tree
        .walk()
        .into_iter()
        .find(|(_, n)| n.rule == "integration by parts")
        .map(|(_, n)| n.clone())
        .expect("integration by parts");
    let note = parts.note.unwrap_or_default();
    for piece in ["u = x", "dv = e^x dx", "du = 1 dx", "v = e^x"] {
        assert!(note.contains(piece), "{piece} missing from {note}");
    }
    assert!(parts.sentence.contains("uv − ∫ v du"));
    ends_at_answer(&line, &tree);
}

#[test]
fn partial_fractions_show_the_decomposition() {
    let (line, tree) = tree_of("$$ \\int \\frac{1}{x^2 - 1} \\, dx = ?! $$");
    let pf = tree
        .walk()
        .into_iter()
        .find(|(_, n)| n.rule == "partial fractions")
        .map(|(_, n)| n.clone())
        .expect("partial fractions");
    let note = pf.note.unwrap_or_default();
    assert!(note.starts_with("decomposition:"), "{note}");
    assert_eq!(pf.children.len(), 2, "{}", tree.to_text());
    ends_at_answer(&line, &tree);
}

#[test]
fn lhopital_steps_name_the_form() {
    let (line, tree) = tree_of("$$ \\lim_{x \\to 0} \\frac{e^x - 1 - x}{x^2} = ?! $$");
    assert_eq!(line.plain.as_deref(), Some("1/2"));
    let lh: Vec<String> = tree
        .walk()
        .into_iter()
        .filter(|(_, n)| n.rule == "L'Hôpital's rule")
        .filter_map(|(_, n)| n.note.clone())
        .collect();
    assert_eq!(lh.len(), 2, "{}", tree.to_text());
    assert!(lh[0].contains("0/0"));
    ends_at_answer(&line, &tree);
}

#[test]
fn series_terms_are_listed() {
    let (line, tree) = tree_of("$$ \\operatorname{series}_{x=0}^{4} e^x = ?! $$");
    let combine = tree
        .walk()
        .into_iter()
        .find(|(_, n)| n.rule == "combine the series")
        .map(|(_, n)| n.clone())
        .expect("combine the series");
    let note = combine.note.unwrap_or_default();
    assert!(
        note.contains("c_2 = 1/2") && note.contains("c_3 = 1/6"),
        "{note}"
    );
    assert!(line.value.is_some());
}

#[test]
fn definite_integrals_end_with_the_fundamental_theorem() {
    let (line, tree) = tree_of("$$ \\int_0^2 x^2 \\, dx = ?! $$");
    assert_eq!(line.plain.as_deref(), Some("8/3"));
    assert_eq!(
        tree.roots.last().map(|n| n.rule.as_str()),
        Some("fundamental theorem of calculus")
    );
    ends_at_answer(&line, &tree);
}

#[test]
fn show_steps_option_records_plain_queries() {
    let src = "$$ \\frac{d}{dx} x^3 = ? $$";
    let mut s = session();
    assert!(!s.show_steps());
    assert!(only_query(s.run_str(src)).steps.is_none());
    s.set_show_steps(true);
    let line = only_query(s.update(src));
    let tree = line.step_tree().expect("steps with the option on");
    assert_eq!(tree.rules(), vec!["power rule"]);
    assert_eq!(line.plain.as_deref(), Some("3*x^2"));
    s.set_show_steps(false);
    assert!(only_query(s.update(src)).steps.is_none());
}
