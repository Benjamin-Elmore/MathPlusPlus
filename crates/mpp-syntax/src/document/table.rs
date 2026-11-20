//! Tables: `| a | b |` rows with a `|---|:-:|` rule under the header.

use super::ast::{Align, Cell, Table};
use super::inline::parse_inlines;
use crate::span::{Diagnostic, Span};

/// Splits one table row into trimmed cell spans. A `|` inside `$…$`, a code
/// span or after a backslash does not split.
pub(crate) fn split_cells(src: &str, line: Span) -> Vec<Span> {
    let text = src.get(line.start..line.end).unwrap_or("");
    let lead = text.len() - text.trim_start().len();
    let mut start = line.start + lead;
    let bytes = src.as_bytes();
    let mut k = start;
    if bytes.get(k) == Some(&b'|') {
        k += 1;
        start = k;
    }
    let mut cells = Vec::new();
    let mut pending = true;
    while k < line.end {
        match bytes.get(k) {
            Some(b'\\') => k += 2,
            Some(&q @ (b'$' | b'`')) => {
                k += 1;
                // Skip to the matching quote on this line, if there is one.
                let mut j = k;
                while j < line.end {
                    match bytes.get(j) {
                        Some(b'\\') if q == b'$' => j += 2,
                        Some(&b) if b == q => break,
                        _ => j += 1,
                    }
                }
                if j < line.end {
                    k = j + 1;
                }
            }
            Some(b'|') => {
                cells.push(trim(src, Span::new(start, k)));
                k += 1;
                start = k;
                pending = false;
            }
            _ => k += 1,
        }
    }
    let tail = trim(src, Span::new(start.min(line.end), line.end));
    if pending || !tail.is_empty() {
        cells.push(tail);
    }
    cells
}

fn trim(src: &str, span: Span) -> Span {
    let s = src.get(span.start..span.end).unwrap_or("");
    let t = s.trim_start();
    let start = span.start + (s.len() - t.len());
    Span::new(start, start + t.trim_end().len())
}

/// Reads a rule row (`|---|:--|--:|:-:|`) as column alignments.
pub(crate) fn rule_aligns(src: &str, line: Span) -> Option<Vec<Align>> {
    let text = src.get(line.start..line.end)?;
    if !text.trim_start().starts_with('|') {
        return None;
    }
    let cells = split_cells(src, line);
    if cells.is_empty() {
        return None;
    }
    cells
        .iter()
        .map(|c| {
            let t = src.get(c.start..c.end)?;
            let left = t.starts_with(':');
            let right = t.ends_with(':') && t.len() > 1;
            let dashes = t.trim_matches(':');
            if dashes.is_empty() || !dashes.bytes().all(|b| b == b'-') {
                return None;
            }
            Some(match (left, right) {
                (true, true) => Align::Center,
                (true, false) => Align::Left,
                (false, true) => Align::Right,
                (false, false) => Align::Default,
            })
        })
        .collect()
}

/// Builds a table from its header line, rule line and body lines.
pub(crate) fn parse_table(
    src: &str,
    header: Span,
    aligns: Vec<Align>,
    body: &[Span],
    diags: &mut Vec<Diagnostic>,
) -> Table {
    let mut row = |line: Span| -> Vec<Cell> {
        split_cells(src, line)
            .into_iter()
            .map(|c| parse_inlines(src, c, diags))
            .collect()
    };
    let header_cells = row(header);
    let rows: Vec<Vec<Cell>> = body.iter().map(|l| row(*l)).collect();
    if header_cells.len() != aligns.len() {
        diags.push(
            Diagnostic::error(
                header,
                format!(
                    "the header has {} columns but the rule row under it has {}",
                    header_cells.len(),
                    aligns.len()
                ),
            )
            .expecting(["the same number of columns in both rows"]),
        );
    }
    for (line, cells) in body.iter().zip(&rows) {
        if cells.len() != header_cells.len() {
            diags.push(Diagnostic::warning(
                *line,
                format!(
                    "this row has {} cells but the header has {}",
                    cells.len(),
                    header_cells.len()
                ),
            ));
        }
    }
    Table {
        header: header_cells,
        aligns,
        rows,
    }
}
