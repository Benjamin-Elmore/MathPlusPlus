//! Golden tests: every example in `examples/` is run like `mpp run` and compared with
//! its `.expected.txt`. Each line of that file is `what was asked → the answer`, in
//! document order (an error counts as the answer of the line it is on); a line
//! `figures → N` says how many graphs the document draws.
//!
//! Set `MPP_BLESS=1` to write the expected file of an example that has none.

use std::path::{Path, PathBuf};

use mpp_cli::{run_file, run_lines};

/// Examples whose `.expected.txt` belongs to another harness, in that harness's own
/// format (file, harness). Here they must run like `mpp run` with an answer on every
/// line, and every error they show must be one their expected file records.
const OWNED: &[(&str, &str)] = &[
    ("auto-evaluation.mpp", "mpp-eval tests/auto_eval.rs"),
    ("checks.mpp", "mpp-eval tests/checks.rs"),
    ("curve-sketching.mpp", "mpp-eval tests/tools_examples.rs"),
    ("steps.mpp", "mpp-eval tests/tools_examples.rs"),
    ("vector-calculus.mpp", "mpp-eval tests/tools_examples.rs"),
    ("differential-equations.mpp", "mpp-eval tests/examples_b.rs"),
    ("integral-applications.mpp", "mpp-eval tests/examples_b.rs"),
    ("numerical-methods.mpp", "mpp-eval tests/examples_b.rs"),
    ("insert-helpers.mpp", "mpp-eval tests/examples_b.rs"),
    ("standard-library.mpp", "mpp-eval tests/stdlib.rs"),
];

/// Examples whose features are not on this branch yet: (file, reason). The
/// integrator clears the list; an example that starts passing is reported so
/// it can be removed.
const SKIP: &[(&str, &str)] = &[];

/// Examples whose `.expected.txt` belongs to a test of its own, in its own format
/// (line numbers with the working, or answers worked out through the domain
/// registry): (file, the test that checks it). They are not compared here.
const OWN_TEST: &[(&str, &str)] = &[
    ("auto-evaluation.mpp", "mpp-eval/tests/auto_eval.rs"),
    ("checks.mpp", "mpp-eval/tests/checks.rs"),
    ("curve-sketching.mpp", "mpp-eval/tests/tools_examples.rs"),
    ("differential-equations.mpp", "mpp-eval/tests/examples_b.rs"),
    ("fields.mpp", "mpp-core/tests/fields_example.rs"),
    ("insert-helpers.mpp", "mpp-eval/tests/examples_b.rs"),
    ("integral-applications.mpp", "mpp-eval/tests/examples_b.rs"),
    ("numerical-methods.mpp", "mpp-eval/tests/examples_b.rs"),
    ("standard-library.mpp", "mpp-eval/tests/stdlib.rs"),
    ("steps.mpp", "mpp-eval/tests/tools_examples.rs"),
    ("vector-calculus.mpp", "mpp-eval/tests/tools_examples.rs"),
    ("visual-calculus.mpp", "mpp-plot/tests/visual_calculus.rs"),
];

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

/// Every example document: `*.mpp` in `examples/` and `main.mpp` in its subfolders.
fn examples() -> Vec<PathBuf> {
    let mut out = vec![];
    for entry in std::fs::read_dir(examples_dir()).expect("examples dir") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "mpp") {
            out.push(path);
        } else if path.is_dir() {
            let main = path.join("main.mpp");
            if main.exists() {
                out.push(main);
            }
        }
    }
    out.sort();
    out
}

fn expected_path(doc: &Path) -> PathBuf {
    doc.with_extension("expected.txt")
}

/// The answers `mpp run` gives, as expected-file lines.
fn actual(doc: &Path) -> Vec<String> {
    let ev = run_file(doc).unwrap_or_else(|e| panic!("{e}"));
    let mut lines: Vec<String> = run_lines(&ev)
        .into_iter()
        .map(|l| {
            if l.error {
                format!("{} → {}", l.asked, l.plain)
            } else {
                format!("{} ? → {}", l.asked, l.plain)
            }
        })
        .collect();
    let figures = mpp_plot_count(&ev);
    if figures > 0 {
        lines.push(format!("figures → {figures}"));
    }
    lines
}

fn mpp_plot_count(ev: &mpp_eval::Evaluated) -> usize {
    ev.document
        .blocks
        .iter()
        .filter(|b| matches!(b.kind, mpp_syntax::document::BlockKind::Figure(_)))
        .count()
}

fn answers(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|l| l.rsplit_once('→').map(|(_, r)| r.trim().to_string()))
        .collect()
}

#[test]
fn there_are_at_least_ten_examples() {
    let n = examples().len();
    assert!(n >= 10, "only {n} examples in examples/");
}

#[test]
fn every_example_matches_its_expected_output() {
    let bless = std::env::var("MPP_BLESS").is_ok();
    let mut failures = vec![];
    for doc in examples() {
        let name = doc
            .strip_prefix(examples_dir())
            .unwrap_or(&doc)
            .display()
            .to_string();
        if OWN_TEST.iter().any(|(f, _)| name.replace('\\', "/") == *f) {
            continue;
        }
        let skip = SKIP.iter().find(|(f, _)| name.replace('\\', "/") == *f);
        let got = actual(&doc);
        let path = expected_path(&doc);
        if !path.exists() {
            if bless {
                std::fs::write(&path, got.join("\n") + "\n").expect("write expected");
                continue;
            }
            failures.push(format!("{name}: no .expected.txt (run with MPP_BLESS=1)"));
            continue;
        }
        let want_text = std::fs::read_to_string(&path).expect("read expected");
        if OWNED.iter().any(|(f, _)| name.replace('\\', "/") == *f) {
            let ev = run_file(&doc).unwrap_or_else(|e| panic!("{e}"));
            for l in run_lines(&ev) {
                if l.answer == "(no answer)" {
                    failures.push(format!("{name}: no answer for {}", l.asked));
                } else if l.error && !want_text.contains(&l.answer) {
                    failures.push(format!("{name}: unexpected error {}", l.answer));
                }
            }
            continue;
        }
        let want: Vec<String> = want_text.lines().map(str::to_string).collect();
        let same = answers(&got) == answers(&want);
        match (skip, same) {
            (Some((_, why)), true) => {
                failures.push(format!("{name}: passes now, remove it from SKIP ({why})"))
            }
            (Some(_), false) => {}
            (None, true) => {}
            (None, false) => failures.push(format!(
                "{name}:\n  expected {:?}\n  got      {:?}",
                answers(&want),
                answers(&got)
            )),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
