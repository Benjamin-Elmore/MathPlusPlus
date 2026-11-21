//! The `mpp` command line: run, check, prove, render, export, calc and ide.

#![forbid(unsafe_code)]

pub mod prove;
pub mod unicode;

use std::fmt::Write;
use std::path::Path;

use mpp_eval::{Evaluated, LineKind, LineResult, QueryMode, Session};
use mpp_syntax::{Diagnostic, Severity};

pub use prove::{proof_report, prove, split_target, ProveArgs, ProveIo, ProveOutcome};
pub use unicode::to_unicode;

/// One typeset result line of a run: the maths asked and its answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunLine {
    /// Line number in the file (1-based).
    pub line: usize,
    /// What was asked, as Unicode text (`∫₀¹ x² dx =`).
    pub asked: String,
    /// The answer as Unicode text (`9π`, `2xy`), or the error message for a line that
    /// failed.
    pub answer: String,
    /// The same answer in the stable ASCII form (`9*π`, `2*x*y`), for golden files.
    pub plain: String,
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
                    plain: errors[i].message.clone(),
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
                    plain: d.message.clone(),
                    error: true,
                    steps: vec![],
                },
            ));
        }
    }
    items.sort_by_key(|(at, _)| *at);
    items.into_iter().map(|(_, r)| r).collect()
}

/// Longest source text echoed back for one line, in characters.
const MAX_SNIPPET: usize = 400;

fn snippet(ev: &Evaluated, start: usize, end: usize) -> String {
    let text = ev.source().get(start..end).unwrap_or("");
    // A huge line is shortened (its start and end), never echoed whole.
    if text.len() > 4 * MAX_SNIPPET {
        let head: String = text.chars().take(MAX_SNIPPET / 2).collect();
        let tail: Vec<char> = text.chars().rev().take(MAX_SNIPPET / 4).collect();
        let tail: String = tail.into_iter().rev().collect();
        let join = |t: &str| t.split_whitespace().collect::<Vec<_>>().join(" ");
        return format!("{} … {}", join(&head), join(&tail));
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn answer_line(ev: &Evaluated, line: &LineResult, src: &str) -> RunLine {
    let n = ev.line_of(line.span.start);
    if let Some(v) = &line.verdict {
        return RunLine {
            line: n,
            asked: to_unicode(src),
            answer: v.describe(),
            plain: v.describe(),
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
    // An exact query with no closed form falls back to a number, shown with `≈`: when
    // there is no exact text, or the exact text only repeats what was asked
    // (`erf(1/2) = erf(1/2)`).
    let plain_text = line.plain.clone().or_else(|| line.text.clone());
    let numeric_fallback = !approx_query && line.shows_decimal_only();
    let (shown, plain) = if approx_query || numeric_fallback {
        let a = line.approx.clone();
        (
            a.clone().or_else(|| line.text.clone()),
            a.or_else(|| plain_text.clone()),
        )
    } else {
        (line.text.clone(), plain_text)
    };
    // Everything before the `?` is the question; the answer takes the `?`'s place.
    let mut asked = match src.rfind('?') {
        Some(q) => to_unicode(&src[..q]),
        None => to_unicode(src),
    };
    // A sentence (`Solve $…$ for $x$.`) asks no `?`: a colon parts it from the answer.
    if matches!(line.kind, LineKind::Helper | LineKind::Query { .. })
        && !src.contains('?')
        && !asked.trim_end().ends_with(':')
    {
        asked = format!("{}:", asked.trim_end());
    }
    if numeric_fallback {
        if let Some(eq) = asked.trim_end().strip_suffix('=') {
            asked = format!("{} ≈", eq.trim_end());
        }
    }
    RunLine {
        line: n,
        asked,
        answer: shown.unwrap_or_else(|| "(no answer)".into()),
        plain: plain.unwrap_or_else(|| "(no answer)".into()),
        error: false,
        steps: line
            .steps
            .iter()
            .flat_map(|s| s.iter())
            .map(|st| {
                let mut t = format!(
                    "{}: {} → {}",
                    st.rule,
                    st.before.to_text(),
                    st.after.to_text()
                );
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
    for d in failed_checks(ev) {
        let _ = write!(out, "{}", format_diagnostic(file, ev, &d));
    }
    out
}

/// A `\overset{?}{=}` check that came out ✗, as an error: a false statement in a
/// document is a mistake `mpp check` must report (and fail on).
pub fn failed_checks(ev: &Evaluated) -> Vec<Diagnostic> {
    ev.queries()
        .filter_map(|(_, l)| {
            let v = l.verdict.as_ref()?;
            (v.passed() == Some(false))
                .then(|| Diagnostic::error(l.span, format!("the check fails {}", v.describe())))
        })
        .collect()
}

fn format_diagnostic(file: &str, ev: &Evaluated, d: &Diagnostic) -> String {
    let level = match d.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
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

/// Reads and runs a file; each query may take up to
/// [`mpp_eval::DEFAULT_TIME_LIMIT`].
pub fn run_file(path: &Path) -> Result<Evaluated, String> {
    run_file_with_limit(path, Some(mpp_eval::DEFAULT_TIME_LIMIT))
}

/// Reads and runs a file; a query that runs longer than `limit` stops with
/// "stopped after N s". `None` is no limit.
pub fn run_file_with_limit(
    path: &Path,
    limit: Option<std::time::Duration>,
) -> Result<Evaluated, String> {
    let mut session = Session::new();
    session.set_time_limit(limit);
    session
        .run_file(path)
        .cloned()
        .map_err(|e| format!("could not read {}: {e}", path.display()))
}

/// The quick calculator: `\int_0^1 x^2 dx` answers `∫₀¹ x² dx = 1/3`.
///
/// The answer comes from [`mpp_eval::calc_line`]; this only writes the question in
/// front of it. An equation (`x^2 = 9`) answers with its solutions alone.
pub fn calc_line(input: &str) -> Result<String, String> {
    let answer = mpp_eval::calc_line(input).map_err(|e| e.0)?;
    let maths = input.trim();
    let maths = maths
        .strip_prefix("$$")
        .and_then(|m| m.strip_suffix("$$"))
        .or_else(|| maths.strip_prefix('$').and_then(|m| m.strip_suffix('$')))
        .unwrap_or(maths)
        .trim();
    if answer.starts_with(['✓', '✗']) {
        return Ok(format!("{} {answer}", to_unicode(maths)));
    }
    // The question without its `= ?`, `≈ ?` or `\approx ?` tail.
    let asked = match maths.rfind('?') {
        Some(q) => {
            let head = maths[..q].trim_end();
            [r"\approx", "≈", "="]
                .iter()
                .find_map(|r| head.strip_suffix(r))
                .unwrap_or(head)
                .trim_end()
        }
        None => maths,
    };
    let asked = to_unicode(asked);
    if answer.starts_with('≈') {
        return Ok(format!("{asked} {answer}"));
    }
    let relation = ['=', '<', '>', '≤', '≥', '≠']
        .iter()
        .any(|c| asked.contains(*c));
    if relation {
        Ok(answer)
    } else {
        Ok(format!("{asked} = {answer}"))
    }
}
