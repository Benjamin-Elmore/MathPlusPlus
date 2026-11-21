//! `Figure:` lines (grammar §4).

use super::ast::*;
use super::cursor::Cursor;
use super::inline::parse_inlines;
use crate::math::{parse_clause, parse_math, Statement};
use crate::span::{Diagnostic, Span};

/// Parses the text after `Figure:`; `rest` is its range in `src`.
pub(crate) fn parse_figure(src: &str, rest: Span, diags: &mut Vec<Diagnostic>) -> Figure {
    let mut c = Cursor::new(src, rest);
    let mut items = Vec::new();
    let mut caption = Vec::new();
    let mut words = Vec::new();

    loop {
        c.skip_ws();
        let start = c.pos;
        match parse_item(src, &mut c, diags) {
            Some(kind) => items.push(FigureItem {
                kind,
                span: Span::new(start, c.pos),
            }),
            // Words that describe the figure (`the data points and $V_C(t)$`).
            None if prose(&mut c, &mut words) => {}
            None => {
                c.skip_ws();
                let at = c.pos;
                let width = c.rest().chars().next().map_or(0, char::len_utf8);
                diags.push(
                    Diagnostic::error(
                        Span::new(at, at + width),
                        "expected a `$…$` item or a figure keyword",
                    )
                    .expecting(["`$…$`", "a figure keyword such as `polar`"]),
                );
                break;
            }
        }
        c.skip_ws();
        if c.eat(',') {
            continue;
        }
        // `$P$ against $V$`, `$\theta(t)$ and the small-angle solution $…$`.
        if c.rest().starts_with('$') {
            continue;
        }
        if !c.rest().starts_with('.') && prose(&mut c, &mut words) {
            c.skip_ws();
            if c.eat(',') || c.rest().starts_with('$') {
                continue;
            }
        }
        if c.rest().starts_with('.') {
            let after = c.pos + 1;
            let boundary = src
                .get(after..)
                .and_then(|s| s.chars().next())
                .is_none_or(char::is_whitespace);
            if boundary {
                caption = parse_inlines(src, trim_start(src, Span::new(after, rest.end)), diags);
            } else {
                diags.push(
                    Diagnostic::error(Span::new(c.pos, after), "expected `. ` before the caption")
                        .expecting(["a space after `.`"]),
                );
            }
            break;
        }
        if !c.at_end() {
            diags.push(
                Diagnostic::error(
                    Span::new(c.pos, rest.end),
                    "expected `,` or `.` after a figure item",
                )
                .expecting(["`,`", "`.`"]),
            );
        }
        break;
    }
    Figure {
        items,
        caption,
        words,
    }
}

/// Reads words up to the next `$`, `,` or sentence end (`. `), recording their span.
/// False (reading nothing) when no word starts here.
fn prose(c: &mut Cursor<'_>, words: &mut Vec<Span>) -> bool {
    let rest = c.rest();
    if !rest.starts_with(|ch: char| ch.is_alphabetic() || ch == '(') {
        return false;
    }
    let start = c.pos;
    let mut depth = 0usize;
    let mut end = rest.len();
    for (i, ch) in rest.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            '$' if depth == 0 => {
                end = i;
                break;
            }
            ',' if depth == 0 => {
                end = i;
                break;
            }
            '.' if depth == 0 => {
                let next = rest[i + 1..].chars().next();
                if next.is_none_or(char::is_whitespace) {
                    end = i;
                    break;
                }
            }
            _ => {}
        }
    }
    let text = rest[..end].trim_end();
    if text.is_empty() {
        return false;
    }
    let stop = start + text.len();
    words.push(Span::new(start, stop));
    c.pos = stop;
    true
}

fn trim_start(src: &str, span: Span) -> Span {
    let s = src.get(span.start..span.end).unwrap_or("");
    let skipped = s.len() - s.trim_start().len();
    Span::new(span.start + skipped, span.end)
}

fn parse_item(
    src: &str,
    c: &mut Cursor<'_>,
    diags: &mut Vec<Diagnostic>,
) -> Option<FigureItemKind> {
    if let Some(m) = c.math() {
        let text = src.get(m.start..m.end).unwrap_or("");
        let (ast, d) = parse_math(text, m.start);
        diags.extend(d);
        if ast.lines.len() == 1 {
            if let Some(Statement::Range(r)) = ast.lines.first().map(|l| &l.statement) {
                return Some(FigureItemKind::Range(r.clone()));
            }
        }
        return Some(FigureItemKind::Expr(ast));
    }
    let keyword = FigureKeyword::ALL
        .iter()
        .copied()
        .find(|k| c.word(k.as_str()))?;
    let arg = c.math().map(|m| {
        let text = src.get(m.start..m.end).unwrap_or("");
        let (clause, d) = parse_clause(text, m.start);
        diags.extend(d);
        clause
    });
    Some(FigureItemKind::Keyword { keyword, arg })
}
