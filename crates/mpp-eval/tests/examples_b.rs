//! The worked examples in `examples/` (integral applications, numerical methods and
//! differential equations) run through a session, and each answer is compared with the
//! example's `.expected.txt`. Set `MPP_BLESS=1` to rewrite the expected files.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mpp_eval::{LineIndex, NoImports, Session};
use mpp_syntax::Severity;

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

/// One line per query: its line number and source, then the answer or the error, then the
/// remarks the evaluator left.
fn render(src: &str) -> String {
    let ev = Session::new()
        .with_importer(Arc::new(NoImports))
        .run_str(src)
        .clone();
    let index = LineIndex::new(src);
    let mut out = String::new();
    for (block, line) in ev.queries() {
        let text = src.get(line.span.start..line.span.end).unwrap_or("").trim();
        let _ = writeln!(out, "{}: {text}", index.line_of(line.span.start));
        if let Some(answer) = line.answer() {
            let _ = writeln!(out, "    {answer}");
        }
        if let Some(steps) = &line.steps {
            for s in steps.iter() {
                let _ = writeln!(out, "    step: {}", s.rule);
            }
        }
        for d in &block.diagnostics {
            if d.span.start >= line.span.start && d.span.end <= line.span.end {
                let kind = match d.severity {
                    Severity::Error => "error",
                    Severity::Warning => "note",
                };
                let _ = writeln!(out, "    {kind}: {}", d.message);
            }
        }
    }
    out.replace("\r\n", "\n")
}

fn check(name: &str) {
    let dir = examples_dir();
    let src = std::fs::read_to_string(dir.join(format!("{name}.mpp")))
        .unwrap_or_else(|e| panic!("{name}.mpp: {e}"));
    let got = render(&src.replace("\r\n", "\n"));
    let expected_path = dir.join(format!("{name}.expected.txt"));
    if std::env::var_os("MPP_BLESS").is_some() {
        std::fs::write(&expected_path, &got).expect("write expected");
        return;
    }
    let expected = std::fs::read_to_string(&expected_path)
        .unwrap_or_else(|e| panic!("{name}.expected.txt: {e}"))
        .replace("\r\n", "\n");
    assert!(
        got == expected,
        "{name}: output differs from the expected file\n--- got ---\n{got}\n--- expected ---\n{expected}"
    );
}

#[test]
fn integral_applications() {
    check("integral-applications");
}

#[test]
fn numerical_methods() {
    check("numerical-methods");
}

#[test]
fn differential_equations() {
    check("differential-equations");
}

#[test]
fn insert_menu_helpers() {
    check("insert-helpers");
}
