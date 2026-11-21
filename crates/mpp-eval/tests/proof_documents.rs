//! The proof documents (the eleven historic proofs and the Navier–Stokes test) show
//! no evaluator errors: their statements are the proof checker's, and their
//! congruences (`\pmod`) are read as congruences.

use std::path::{Path, PathBuf};

use mpp_eval::{LineKind, Session};

fn proof_documents() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../work/pipeline-tests");
    let mut out: Vec<PathBuf> = std::fs::read_dir(root.join("historic"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "mpp"))
        .collect();
    out.push(root.join("navier-stokes.mpp"));
    out.sort();
    out
}

#[test]
fn no_proof_document_shows_evaluator_errors() {
    let docs = proof_documents();
    assert_eq!(docs.len(), 12, "{docs:?}");
    let mut bad = vec![];
    for path in docs {
        let src = std::fs::read_to_string(&path).unwrap();
        let mut s = Session::new();
        s.set_file(&path);
        let ev = s.run_str(&src).clone();
        for d in ev.errors() {
            bad.push(format!("{}: {}", path.display(), d.message));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

fn verdicts(src: &str) -> Vec<String> {
    let ev = Session::new().run_str(src).clone();
    assert!(ev.errors().is_empty(), "{:?}", ev.errors());
    ev.blocks
        .iter()
        .flat_map(|b| b.values.iter())
        .filter(|l| matches!(l.kind, LineKind::Check))
        .map(|l| {
            l.verdict
                .as_ref()
                .map(|v| v.mark().to_string())
                .unwrap_or_default()
        })
        .collect()
}

#[test]
fn congruence_checks_use_exact_integers() {
    let got = verdicts(
        r"$$ 3^{12} \overset{?}{\equiv} 1 \pmod{13} $$
$$ 3^{12} \overset{?}{\equiv} 2 \pmod{13} $$
$$ 2^{1000} \overset{?}{\equiv} 2^{8} \pmod{17} $$
$$ -1 \overset{?}{\equiv} 12 \pmod{13} $$
$$ n + 7 \overset{?}{\equiv} n \pmod 7 $$
$$ n^2 \overset{?}{\equiv} n \pmod 7 $$",
    );
    assert_eq!(got, ["✓", "✗", "✓", "✓", "✓", "✗"]);
}

#[test]
fn congruence_relations_are_not_errors() {
    let ev = Session::new()
        .run_str(
            r"$$ a^p \equiv a \pmod p $$
$$ 10 \equiv 3 \pmod 7 $$",
        )
        .clone();
    assert!(ev.errors().is_empty(), "{:?}", ev.errors());
}

#[test]
fn worksheets_still_report_notation_they_cannot_compute() {
    // Without a proof in the document, a displayed statement the evaluator cannot
    // read is still reported.
    let ev = Session::new()
        .run_str(r"$$ \sum_{i \in S} a_i = 1 $$")
        .clone();
    assert!(!ev.errors().is_empty());
}
