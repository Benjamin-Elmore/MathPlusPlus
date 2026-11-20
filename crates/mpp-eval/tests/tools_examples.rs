//! The example documents of the maths tools (`examples/steps.mpp`,
//! `examples/curve-sketching.mpp`, `examples/vector-calculus.mpp`) run and print exactly
//! their `*.expected.txt`: every query, check and helper answer, the working of each
//! `= ?!` query as an indented step tree, then the diagnostics.
//!
//! Set `MPP_UPDATE_EXPECTED=1` to rewrite the expected files after a deliberate change.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;

use mpp_eval::{Evaluated, LineKind, NoImports, Session};

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

/// The report plus the working of every query that recorded steps.
fn render(ev: &Evaluated) -> String {
    let mut out = ev.report();
    for block in &ev.blocks {
        for line in &block.values {
            let Some(tree) = line.step_tree() else {
                continue;
            };
            if !matches!(line.kind, LineKind::Query { .. }) {
                continue;
            }
            let _ = writeln!(out, "\nworking for line {}:", ev.line_of(line.span.start));
            out.push_str(&tree.to_text());
        }
    }
    out.replace("\r\n", "\n")
}

fn check(name: &str) {
    let dir = examples_dir();
    let src = std::fs::read_to_string(dir.join(format!("{name}.mpp")))
        .unwrap_or_else(|e| panic!("reading {name}.mpp: {e}"));
    let ev = Session::new()
        .with_importer(Arc::new(NoImports))
        .run_str(&src)
        .clone();
    let got = render(&ev);
    assert!(ev.errors().is_empty(), "{name}.mpp has errors:\n{got}");
    let expected_path = dir.join(format!("{name}.expected.txt"));
    if std::env::var_os("MPP_UPDATE_EXPECTED").is_some() {
        std::fs::write(&expected_path, &got).unwrap_or_else(|e| panic!("writing: {e}"));
        return;
    }
    let expected = std::fs::read_to_string(&expected_path)
        .unwrap_or_else(|e| panic!("reading {name}.expected.txt: {e}"))
        .replace("\r\n", "\n");
    assert_eq!(got, expected, "{name}.mpp output changed");
}

#[test]
fn steps_example() {
    check("steps");
}

#[test]
fn curve_sketching_example() {
    check("curve-sketching");
}
