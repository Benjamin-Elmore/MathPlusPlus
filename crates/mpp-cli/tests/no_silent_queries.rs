//! No silent drops: every `= ?`, `≈ ?` and `\overset{?}{=}` in every example and
//! every pipeline document either has an answer or a diagnostic on its line, so
//! `mpp check` never says ok about a question that got no answer.
//!
//! The question marks are found in the raw text, independently of the parser, so a
//! question the parser reads as prose (and nothing evaluates) is caught too.

use std::path::{Path, PathBuf};

use mpp_cli::run_file;
use mpp_eval::{Evaluated, LineKind};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn mpp_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            mpp_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "mpp") {
            out.push(p);
        }
    }
}

/// Offsets of the `?` of each query outside code fences, code spans and comments.
fn marks(src: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut fence = false;
    let mut note = false;
    let mut start = 0;
    for raw in src.split('\n') {
        let line_start = start;
        start += raw.len() + 1;
        let t = raw.trim_start();
        if t.starts_with("```") {
            fence = !fence;
            continue;
        }
        if !fence && t.starts_with("%{") {
            note = true;
        }
        if note {
            note = !t.trim_end().ends_with("%}");
            continue;
        }
        if fence || t.starts_with('%') {
            continue;
        }
        let mut code = false;
        for (k, c) in raw.char_indices() {
            match c {
                '`' => code = !code,
                '%' if !code && !raw[..k].ends_with('\\') => break,
                '?' if !code => {
                    let before = raw[..k].trim_end();
                    let query = before.ends_with('=')
                        || before.ends_with('≈')
                        || before.ends_with("\\approx")
                        || raw[..k].ends_with("\\overset{");
                    if query {
                        out.push(line_start + k);
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// The problems with one evaluated document: each `?` with neither an answer nor a
/// diagnostic on its line.
fn silent(ev: &Evaluated) -> Vec<String> {
    let src = ev.source();
    let diag_lines: Vec<usize> = ev
        .all_diagnostics()
        .iter()
        .map(|d| ev.line_of(d.span.start))
        .collect();
    let mut out = Vec::new();
    for at in marks(src) {
        let line = ev.line_of(at);
        if diag_lines.contains(&line) {
            continue;
        }
        let answered = ev.blocks.iter().flat_map(|b| b.values.iter()).any(|v| {
            v.span.start <= at
                && at < v.span.end
                && matches!(v.kind, LineKind::Query { .. } | LineKind::Check)
                && (v.value.is_some()
                    || v.text.is_some()
                    || v.approx.is_some()
                    || v.verdict.is_some()
                    || v.noeval)
        });
        if !answered {
            let text = src.lines().nth(line.saturating_sub(1)).unwrap_or("").trim();
            out.push(format!("line {line}: {text}"));
        }
    }
    out
}

#[test]
fn every_query_in_every_document_has_an_answer_or_a_diagnostic() {
    let mut files = Vec::new();
    mpp_files(&root().join("examples"), &mut files);
    mpp_files(&root().join("work").join("pipeline-tests"), &mut files);
    files.sort();
    assert!(files.len() > 40, "found only {} documents", files.len());
    let mut problems = Vec::new();
    for f in &files {
        let ev = run_file(f).unwrap_or_else(|e| panic!("{e}"));
        for p in silent(&ev) {
            problems.push(format!("{}: {p}", f.display()));
        }
    }
    assert!(
        problems.is_empty(),
        "questions with no answer and no diagnostic:\n{}",
        problems.join("\n")
    );
}

#[test]
fn a_dropped_question_is_never_silent() {
    // The three shapes that used to vanish: a display after a list number, a display
    // after prose on the same line, and a `?` in plain text.
    let dir = std::env::temp_dir().join(format!("mpp-silent-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("homework.mpp");
    std::fs::write(
        &f,
        "4. $$ \\int x \\cos x \\, dx = ? $$\n\nPartial fractions: $$ 1 + 1 = ? $$\n\nThe cost is $$5 = ? here.\n",
    )
    .unwrap();
    let ev = run_file(&f).unwrap();
    assert!(silent(&ev).is_empty(), "{:?}", silent(&ev));
    let report = mpp_cli::check_report("homework.mpp", &ev);
    assert!(report.contains("not evaluated"), "{report}");
    let _ = std::fs::remove_dir_all(&dir);
}
