//! The Lean bridge: the exported project, statements and alignment data, and `lake build`
//! when Lean is installed.

use mpp_proof::lean::{apply, build, export, find_lake, parse_lake_output, LeanConfig};
use mpp_proof::{check_document, StepStatus};
use mpp_syntax::document::parse_document;

const GAUSS: &str = "**Theorem 1.** For every integer $n \\ge 1$, $\\sum_{k=1}^{n} k = \\frac{n(n+1)}{2}$.\n\n\
**Proof** (induction on $n$).\n\
1. Base case: $\\sum_{k=1}^{1} k = 1$ — by algebra.\n\
2. Inductive step: if $\\sum_{k=1}^{n} k = \\frac{n(n+1)}{2}$, then $\\sum_{k=1}^{n+1} k = \\frac{n(n+1)}{2} + (n+1)$ — by algebra.\n\
3. So the formula holds for every $n \\ge 1$ — by induction.\n∎\n";

const PROSE: &str =
    "**Theorem 1.** For $\\nu > 0$ there exist smooth data with no smooth solution.\n\n\
**Proof.**\n1. Somebody claims it — *(external: [@x])*.\n∎\n\n## References\n- [@x] X.\n";

#[test]
fn a_formal_statement_with_its_meaning_made_visible() {
    let doc = parse_document(GAUSS);
    let check = check_document(&doc, GAUSS, &[]);
    let ex = export(&doc, GAUSS, &check, "gauss", &LeanConfig::default());
    let t = ex.theorem("Theorem 1").expect("exported");
    assert!(!t.statement_sorry);
    let lean = t.alignment.lean.clone().unwrap_or_default();
    assert!(lean.contains("(n : ℕ)"), "{lean}");
    assert!(lean.contains("n ≥ 1"), "{lean}");
    assert!(lean.contains("Finset.Icc"), "{lean}");
    // ℕ division rounds down: the alignment view says so, and typesets the floor.
    assert!(t
        .alignment
        .warnings
        .iter()
        .any(|w| w.contains("rounds down")));
    assert!(t
        .alignment
        .lean_latex
        .as_deref()
        .unwrap_or("")
        .contains("\\lfloor"));
    assert!(
        !t.alignment.confirmed,
        "only a person confirms an alignment"
    );
    // The project's files.
    let names: Vec<String> = ex
        .files
        .iter()
        .map(|(p, _)| p.to_string_lossy().replace('\\', "/"))
        .collect();
    for want in [
        "lean-toolchain",
        "lakefile.toml",
        "MppProof.lean",
        "MppProof/Gauss.lean",
    ] {
        assert!(names.iter().any(|n| n == want), "missing {want}: {names:?}");
    }
    assert!(ex.source().starts_with("import Mathlib"));
    assert!(ex.source().contains("theorem gauss_theorem_1"));
}

#[test]
fn only_symbolically_checked_steps_get_a_tactic() {
    let doc = parse_document(GAUSS);
    let check = check_document(&doc, GAUSS, &[]);
    let ex = export(&doc, GAUSS, &check, "gauss", &LeanConfig::default());
    let t = ex.theorem("Theorem 1").expect("exported");
    for (s, c) in t.steps.iter().zip(&check.results[0].proofs[0].steps) {
        assert_eq!(s.status, c.status);
        if c.status != StepStatus::Symbolic {
            assert!(
                s.sorry,
                "step {:?} is {} but has no sorry",
                s.number,
                c.status.name()
            );
        }
    }
    // The proof's end is always sorry: the bridge does not rebuild the argument.
    assert!(ex.source().trim_end().ends_with("sorry"));
}

#[test]
fn a_prose_statement_is_never_formalised_by_guessing() {
    // The only relation in the statement is ν > 0; stating the theorem as `ν > 0` would
    // be exactly the mismatch the alignment view exists to catch.
    let doc = parse_document(PROSE);
    let check = check_document(&doc, PROSE, &[]);
    let ex = export(&doc, PROSE, &check, "prose", &LeanConfig::default());
    let t = ex.theorem("Theorem 1").expect("exported");
    assert!(t.statement_sorry);
    assert!(t.alignment.lean.is_none());
    assert!(ex.source().contains("(sorry : Prop)"));
    assert!(t.steps.iter().all(|s| s.sorry));
}

#[test]
fn lake_results_mark_compiled_steps_and_nothing_else() {
    let doc = parse_document(GAUSS);
    let mut check = check_document(&doc, GAUSS, &[]);
    let ex = export(&doc, GAUSS, &check, "gauss", &LeanConfig::default());
    let t = ex.theorem("Theorem 1").expect("exported").clone();
    let with_tactic: Vec<_> = t.steps.iter().filter(|s| !s.sorry).collect();
    // A build where the first tactic step failed to compile.
    let mut log = String::from("info: built\n");
    if let Some(first) = with_tactic.first() {
        log.push_str(&format!(
            "MppProof/Gauss.lean:{}:2: error: linarith failed\n",
            first.lines.1
        ));
    }
    let outcome = parse_lake_output(&log, "MppProof/Gauss.lean", false);
    let changed = apply(&mut check, &ex, &outcome);
    assert_eq!(changed, with_tactic.len().saturating_sub(1));
    for (s, c) in t.steps.iter().zip(&check.results[0].proofs[0].steps) {
        if s.sorry {
            assert_ne!(c.status, StepStatus::Lean, "a sorry step can never be ⊢");
        }
    }
}

#[test]
fn lake_build_when_lean_is_installed() {
    let Some(lake) = find_lake() else {
        eprintln!("skipped: Lean is not installed on this machine (no `lake` on PATH), so `lake build` cannot run; the export itself is tested above");
        return;
    };
    let doc = parse_document(GAUSS);
    let mut check = check_document(&doc, GAUSS, &[]);
    let ex = export(&doc, GAUSS, &check, "gauss", &LeanConfig::default());
    let dir = std::env::temp_dir().join(format!("mpp-lean-test-{}", std::process::id()));
    match build(&ex, &dir, &LeanConfig::default()) {
        Ok(outcome) => {
            let _ = apply(&mut check, &ex, &outcome);
            eprintln!(
                "lake at {} finished: success = {}",
                lake.display(),
                outcome.success
            );
        }
        Err(e) => panic!("lake build failed to run: {e}"),
    }
}
