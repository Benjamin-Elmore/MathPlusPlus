//! Theorem headings with numbers and titles, conjectures, reference lists, and the
//! proof-step syntax.

use mpp_syntax::document::proof::{proof_steps, NoteKind, ReasonKind};
use mpp_syntax::document::{parse_document, BlockKind, Theorem, TheoremKind};

fn theorems(src: &str) -> Vec<Theorem> {
    let doc = parse_document(src);
    doc.blocks
        .iter()
        .filter_map(|b| match &b.kind {
            BlockKind::Theorem(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn headings_with_numbers_and_titles() {
    let src = "**Lemma 1** (Scaling). If $x > 0$ then $x^2 > 0$.\n\n\
               **Theorem 2.** $\\sqrt 2$ is irrational.\n\n\
               **Theorem 2.1** (Euclid; *Elements* IX.20 [@euclid]). There are infinitely many primes.\n\n\
               **Proof** (pairing).\n1. $1 = 1$ — by algebra.\n∎\n\n\
               **Conjecture 3.** Every even number above 2 is a sum of two primes.\n\n\
               **Remark.** Plain as before.\n\n\
               **Theorem** is a word here, not a heading.\n\n\
               ## References\n- [@euclid] Euclid, *Elements*.\n";
    let doc = parse_document(src);
    assert!(
        doc.errors().next().is_none(),
        "no errors expected: {:?}",
        doc.diagnostics
    );
    let ts = theorems(src);
    let kinds: Vec<TheoremKind> = ts.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TheoremKind::Lemma,
            TheoremKind::Theorem,
            TheoremKind::Theorem,
            TheoremKind::Proof,
            TheoremKind::Conjecture,
            TheoremKind::Remark
        ]
    );
    assert_eq!(ts[0].number.as_deref(), Some("1"));
    assert!(ts[0].title.is_some());
    assert_eq!(ts[1].number.as_deref(), Some("2"));
    assert!(ts[1].title.is_none());
    assert_eq!(ts[2].number.as_deref(), Some("2.1"));
    assert_eq!(ts[3].number, None);
    assert!(ts[3].title.is_some());
    assert_eq!(ts[4].number.as_deref(), Some("3"));
    assert_eq!(ts[5].number, None);
}

#[test]
fn reference_list_defines_keys() {
    let src = "See [@a] and [@b].\n\n## References\n\n- [@a] A. Author, *A book*, 2000.\n- [@b] B. Author, *Another*, 2001.\n";
    let doc = parse_document(src);
    assert!(doc.errors().next().is_none(), "{:?}", doc.diagnostics);
    let entries = doc.reference_list_entries();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].key, "a");
    assert!(entries[0].title.contains("A book"));
    // A missing entry is still an error.
    let doc = parse_document("See [@c].\n\n## References\n\n- [@a] A.\n");
    assert_eq!(doc.errors().count(), 1);
}

#[test]
fn bare_integral_sign_in_prose() {
    let doc = parse_document("Exchange $\\frac{d}{dt}$ and $\\int$ here.\n");
    assert!(doc.errors().next().is_none(), "{:?}", doc.diagnostics);
}

#[test]
fn steps_of_a_numbered_proof() {
    let src = "**Theorem 1.** It holds.\n\n**Proof.**\n\
               1. A multi-agent system reported a proof — *(external: [@x])*.\n\
               2. It is not verified — *(gap: the whole proof)*.\n\
               3. $a = b$ — by Theorem 1.\n∎\n\n## References\n- [@x] X.\n";
    let ts = theorems(src);
    let s = proof_steps(&ts[1]);
    assert_eq!(s.steps.len(), 3);
    assert_eq!(s.steps[0].notes[0].kind, NoteKind::External);
    assert_eq!(s.steps[0].notes[0].keys, vec!["x".to_string()]);
    assert!(s.steps[0].reason.is_none());
    assert_eq!(s.steps[1].notes[0].kind, NoteKind::Gap);
    assert_eq!(
        s.steps[2].reason.as_ref().map(|r| r.kind),
        Some(ReasonKind::Reference)
    );
}
