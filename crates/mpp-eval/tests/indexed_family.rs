//! Members of an indexed family print as subscripts (`a₁ + a₂`), never as calls.

use std::sync::Arc;

use mpp_eval::{NoImports, Session};

fn shown(src: &str) -> Vec<(String, String, String)> {
    let ev = Session::new()
        .with_importer(Arc::new(NoImports))
        .run_str(src)
        .clone();
    assert!(ev.errors().is_empty(), "{:?}", ev.errors());
    ev.queries()
        .map(|(_, l)| {
            (
                l.text.clone().unwrap_or_default(),
                l.plain.clone().unwrap_or_default(),
                l.latex.clone().unwrap_or_default(),
            )
        })
        .collect()
}

#[test]
fn members_are_subscripts_in_every_form() {
    let got = shown(
        r"$$ \sum_{i=1}^{3} a_i = ? $$
$$ \prod_{k=1}^{2} x_k = ? $$
$$ p_1 + p_2 + \dots + p_4 = ? $$",
    );
    assert_eq!(got[0].0, "a₁ + a₂ + a₃");
    assert_eq!(got[0].1, "a_1 + a_2 + a_3");
    assert_eq!(got[0].2, "a_{1} + a_{2} + a_{3}");
    assert_eq!(got[1].0, "x₁x₂");
    assert_eq!(got[2].0, "p₁ + p₂ + p₃ + p₄");
    for (text, plain, latex) in &got {
        for s in [text, plain, latex] {
            assert!(!s.contains("(1)") && !s.contains(r"\left("), "{s}");
        }
    }
}

#[test]
fn a_function_is_still_a_call() {
    let got = shown(
        r"$$ f(x) := x^2 $$
$$ \sum_{i=1}^{3} f(i) = ? $$
$$ f(y) + 1 = ? $$",
    );
    assert_eq!(got[0].0, "14");
    assert!(got[1].0.contains("y²"), "{got:?}");
}
