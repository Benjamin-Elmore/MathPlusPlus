//! No silent drops: every `= ?`, `≈ ?` and `\overset{?}{=}` in a document either
//! gets an answer or a diagnostic.
//!
//! Two things are caught after a run: a query line that was read but produced no
//! answer and no error, and a `?` that no maths line covers at all (the text around
//! it was read as prose, so nothing evaluated it).

use mpp_syntax::{Diagnostic, Severity, Span};

use crate::result::{BlockResult, LineIndex, LineKind};

/// Adds a warning for every query that got no answer and no diagnostic.
pub(crate) fn flag_unanswered(
    src: &str,
    lines: &LineIndex,
    blocks: &mut [BlockResult],
    loose: &mut Vec<Diagnostic>,
) {
    let has_diag = |blocks: &[BlockResult], loose: &[Diagnostic], line: usize| {
        blocks
            .iter()
            .flat_map(|b| b.diagnostics.iter())
            .chain(loose.iter())
            .any(|d| lines.line_of(d.span.start) == line)
    };

    // Query lines that were read but came back empty.
    let mut extra: Vec<(usize, Diagnostic)> = Vec::new();
    for (bi, b) in blocks.iter().enumerate() {
        for v in &b.values {
            let query = matches!(v.kind, LineKind::Query { .. } | LineKind::Check);
            if !query || v.noeval || v.auto || v.pending || v.hint.is_some() {
                continue;
            }
            let answered =
                v.value.is_some() || v.text.is_some() || v.approx.is_some() || v.verdict.is_some();
            let errored = b.diagnostics.iter().any(|d| {
                d.severity == Severity::Error
                    && d.span.start < v.span.end.max(v.span.start + 1)
                    && d.span.end >= v.span.start
            });
            if !answered && !errored {
                extra.push((bi, Diagnostic::warning(v.span, "this query got no answer")));
            }
        }
    }
    for (bi, d) in extra {
        blocks[bi].diagnostics.push(d);
    }

    // `?` marks that no maths line read.
    for at in query_marks(src) {
        let covered = blocks.iter().any(|b| {
            b.values
                .iter()
                .any(|v| v.span.start <= at && at < v.span.end.max(v.span.start + 1))
        });
        if covered || has_diag(blocks, loose, lines.line_of(at)) {
            continue;
        }
        let span = Span::new(at, at + 1);
        let d = Diagnostic::warning(
            span,
            "this `?` is not evaluated: it is not inside maths that Math++ reads",
        )
        .expecting(["the question in `$ … $` or on its own `$$ … $$` line"]);
        // Put it in the block it falls in, so the editor shows it in place.
        match blocks
            .iter_mut()
            .filter(|b| b.span.start <= at && at < b.span.end)
            .min_by_key(|b| b.span.end - b.span.start)
        {
            Some(b) => b.diagnostics.push(d),
            None => loose.push(d),
        }
    }
    for b in blocks.iter_mut() {
        b.diagnostics.sort_by_key(|d| (d.span.start, d.span.end));
    }
}

/// Offsets of the `?` of each `= ?`, `≈ ?`, `\approx ?` and `\overset{?}` outside
/// code fences, code spans and `%` comment lines.
pub(crate) fn query_marks(src: &str) -> Vec<usize> {
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
        let bytes = raw.as_bytes();
        let mut k = 0;
        while k < bytes.len() {
            match bytes[k] {
                b'\\' if !code && bytes.get(k + 1).is_some_and(|c| !c.is_ascii_alphabetic()) => {
                    k += 2;
                    continue;
                }
                b'`' => code = !code,
                b'%' if !code => break,
                b'?' if !code && is_query_mark(&raw[..k]) => out.push(line_start + k),
                _ => {}
            }
            k += 1;
        }
    }
    out
}

/// True when the text before a `?` ends with `=`, `≈`, `\approx` or `\overset{`.
fn is_query_mark(before: &str) -> bool {
    let b = before.trim_end();
    b.ends_with('=') && !b.ends_with("\\=")
        || b.ends_with('≈')
        || b.ends_with("\\approx")
        || before.ends_with("\\overset{")
}

#[cfg(test)]
mod tests {
    use super::query_marks;

    #[test]
    fn marks_skip_code_and_comments() {
        let src = "a $x = ?$ b\n`y = ?` and % z = ?\n```\nw = ?\n```\n$$ \\overset{?}{=} $$";
        let marks = query_marks(src);
        assert_eq!(marks.len(), 2, "{marks:?}");
        assert_eq!(&src[marks[0]..marks[0] + 1], "?");
    }
}
