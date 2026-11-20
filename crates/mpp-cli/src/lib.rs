//! The `mpp` command line: run, check, render, export, calc and ide.

#![forbid(unsafe_code)]

pub mod unicode;

use std::fmt::Write;
use std::path::Path;

use mpp_eval::{Evaluated, LineKind, LineResult, QueryMode, Session};
use mpp_syntax::{Diagnostic, Severity};

pub use unicode::to_unicode;

/// One typeset result line of a run: the maths asked and its answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunLine {
    /// Line number in the file (1-based).
    pub line: usize,
    /// What was asked, as Unicode text (`∫₀¹ x² dx =`).
    pub asked: String,
    /// The answer (`1/3`), or the error message for a line that failed.
    pub answer: String,
    /// True when the line is an error.
    pub error: bool,
    /// The working of a `= ?!` query, one step per line (`rule: before → after`).
    pub steps: Vec<String>,
}

impl RunLine {
    /// `∫₀¹ x² dx = 1/3`.
    pub fn text(&self) -> String {
        if self.error {
            format!("{} ✗ {}", self.asked, self.answer)
                .trim()
                .to_string()
        } else {
            format!("{} {}", self.asked, self.answer)
        }
    }
}

/// The query and check lines of an evaluated document, typeset, in order, with
/// errors in place (an error counts as the answer of the line it is on).
pub fn run_lines(ev: &Evaluated) -> Vec<RunLine> {
    let errors = ev.errors();
    let mut used = vec![false; errors.len()];
    let mut items: Vec<(usize, RunLine)> = Vec::new();
    for (_, line) in ev.queries() {
        let inside = errors
            .iter()
            .position(|d| d.span.start >= line.span.start && d.span.start < line.span.end);
        let src = snippet(ev, line.span.start, line.span.end);
        let run = match inside {
            Some(i) => {
                used[i] = true;
                RunLine {
                    line: ev.line_of(line.span.start),
                    asked: to_unicode(&src),
                    answer: errors[i].message.clone(),
                    error: true,
                    steps: vec![],
                }
            }
            None => answer_line(ev, line, &src),
        };
        items.push((line.span.start, run));
    }
    for (i, d) in errors.iter().enumerate() {
        if !used[i] {
            items.push((
                d.span.start,
                RunLine {
                    line: ev.line_of(d.span.start),
                    asked: to_unicode(&snippet(ev, d.span.start, d.span.end)),
                    answer: d.message.clone(),
                    error: true,
                    steps: vec![],
                },
            ));
        }
    }
    items.sort_by_key(|(at, _)| *at);
    items.into_iter().map(|(_, r)| r).collect()
}

fn snippet(ev: &Evaluated, start: usize, end: usize) -> String {
    let text = ev.source().get(start..end).unwrap_or("");
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn answer_line(ev: &Evaluated, line: &LineResult, src: &str) -> RunLine {
    let n = ev.line_of(line.span.start);
    if let Some(v) = &line.verdict {
        return RunLine {
            line: n,
            asked: to_unicode(src),
            answer: v.describe(),
            error: false,
            steps: vec![],
        };
    }
    let approx_query = matches!(
        line.kind,
        LineKind::Query {
            mode: QueryMode::Approx,
            ..
        }
    );
    let shown = if approx_query {
        line.approx.clone().or_else(|| line.text.clone())
    } else {
        line.text.clone()
    };
    // Everything before the `?` is the question; the answer takes the `?`'s place.
    let asked = match src.rfind('?') {
        Some(q) => to_unicode(&src[..q]),
        None => to_unicode(src),
    };
    RunLine {
        line: n,
        asked,
        answer: shown.unwrap_or_else(|| "(no answer)".into()),
        error: false,
        steps: line
            .steps
            .iter()
            .flat_map(|s| s.iter())
            .map(|st| {
                let mut t = format!("{}: {} → {}", st.rule, st.before, st.after);
                if let Some(n) = &st.note {
                    t.push_str(&format!(" ({n})"));
                }
                t
            })
            .collect(),
    }
}

/// The text `mpp run` prints.
pub fn run_report(ev: &Evaluated) -> String {
    run_report_with(ev, false)
}

/// The text `mpp run` prints; with `steps`, the working of `= ?!` queries under each answer.
pub fn run_report_with(ev: &Evaluated, steps: bool) -> String {
    let mut out = String::new();
    for l in run_lines(ev) {
        let _ = writeln!(out, "{}", l.text());
        if steps {
            for s in &l.steps {
                let _ = writeln!(out, "    {s}");
            }
        }
    }
    out
}

/// The diagnostics of a document as the lines `mpp check` prints.
pub fn check_report(file: &str, ev: &Evaluated) -> String {
    let mut out = String::new();
    for d in ev.all_diagnostics() {
        let _ = write!(out, "{}", format_diagnostic(file, ev, d));
    }
    out
}

fn format_diagnostic(file: &str, ev: &Evaluated, d: &Diagnostic) -> String {
    let level = match d.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    };
    let line = ev.line_of(d.span.start);
    let shown = to_unicode(&snippet(ev, d.span.start, d.span.end));
    let mut s = format!("{level}: {}\n  --> {file}:{line}\n", d.message);
    if !shown.is_empty() {
        let _ = writeln!(s, "   | {shown}");
    }
    if !d.expected.is_empty() {
        let _ = writeln!(s, "   = expected {}", d.expected.join(" or "));
    }
    s
}

/// Reads and runs a file.
pub fn run_file(path: &Path) -> Result<Evaluated, String> {
    let mut session = Session::new();
    session
        .run_file(path)
        .cloned()
        .map_err(|e| format!("could not read {}: {e}", path.display()))
}

/// The quick calculator: `\int_0^1 x^2 dx` answers `∫₀¹ x² dx = 1/3`.
///
/// An input without a `?` is asked as `input = ?`; an input that already ends
/// in `= ?` or `≈ ?` is run as written.
pub fn calc_line(input: &str) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("nothing to calculate".into());
    }
    let asked = if input.contains('?') {
        input.to_string()
    } else {
        format!("{input} = ?")
    };
    let src = format!("$$ {asked} $$\n");
    let mut session = Session::new();
    let ev = session.run_str(&src).clone();
    let lines = run_lines(&ev);
    if let Some(e) = lines.iter().find(|l| l.error) {
        return Err(e.answer.clone());
    }
    match lines.into_iter().next() {
        Some(l) => Ok(l.text()),
        None => Err("that is not something to calculate".into()),
    }
}
