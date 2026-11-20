//! `Figure:` lines (grammar §4).

use super::ast::*;
use super::cursor::Cursor;
use super::inline::parse_inlines;
use crate::math::{parse_clause, parse_math, Clause, Statement};
use crate::span::{Diagnostic, Span};

/// Parses the text after `Figure:`; `rest` is its range in `src`.
pub(crate) fn parse_figure(src: &str, rest: Span, diags: &mut Vec<Diagnostic>) -> Figure {
    let mut c = Cursor::new(src, rest);
    let mut items = Vec::new();
    let mut caption = Vec::new();

    loop {
        c.skip_ws();
        let start = c.pos;
        match parse_item(src, &mut c, diags) {
            Some(kind) => items.push(FigureItem {
                kind,
                span: Span::new(start, c.pos),
            }),
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
        if c.eat(',') {
            continue;
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
    Figure { items, caption }
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
    let arg = c.math().map(|m| keyword_clause(src, m.start, m.end, diags));
    Some(FigureItemKind::Keyword { keyword, arg })
}

/// Splits the text of a keyword argument at the commas outside brackets, giving each
/// piece with its offset in `text`: `n = 8, (0, 1)` is `n = 8` and `(0, 1)`.
pub fn split_keyword_list(text: &str) -> Vec<(usize, &str)> {
    let mut pieces = Vec::new();
    let (mut depth, mut start, mut escaped) = (0i32, 0usize, false);
    for (i, c) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                pieces.push((start, &text[start..i]));
                start = i + 1;
            }
            _ => {}
        }
    }
    pieces.push((start, &text[start..]));
    pieces
}

/// Parses the `$…$` argument of a keyword. Most take one clause (`x = 1`); several take a
/// list (`n = 8, x_0 = 1`, `y' = x, (0, 1)`), in which case the clause is the first entry
/// and the plot crate reads the whole list from the source.
fn keyword_clause(src: &str, start: usize, end: usize, diags: &mut Vec<Diagnostic>) -> Clause {
    let text = src.get(start..end).unwrap_or("");
    let pieces: Vec<(usize, &str)> = split_keyword_list(text)
        .into_iter()
        .filter(|(_, p)| !p.trim().is_empty())
        .collect();
    let mut first: Option<Clause> = None;
    for (off, piece) in &pieces {
        let (clause, d) = parse_clause(piece, start + off);
        diags.extend(d);
        first.get_or_insert(clause);
    }
    match first {
        Some(c) => c,
        None => {
            let (clause, d) = parse_clause(text, start);
            diags.extend(d);
            clause
        }
    }
}
