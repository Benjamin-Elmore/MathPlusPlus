//! The sentence forms of grammar §3: `Using … from`, the helpers, `Analyse`,
//! `Solve`, `Assume`, `Data` and the `Let` convenience definition.
//!
//! A sentence is recognised by its shape alone (words and `$…$` spans). Only
//! once the shape fits is the maths parsed, so prose that merely starts with
//! `Using $x$ …` stays prose and reports nothing.

use super::ast::*;
use super::cursor::Cursor;
use super::inline::parse_inlines;
use crate::math::{
    parse_clause, parse_expr, parse_interval, parse_name_list, Bracket, Node, NodeKind,
};
use crate::span::{Diagnostic, Span};

const HELPERS: &[(&str, HelperName)] = &[
    ("the domain", HelperName::Domain),
    ("the range", HelperName::Range),
    ("the intercepts", HelperName::Intercepts),
    ("the critical points", HelperName::CriticalPoints),
    ("the extrema", HelperName::Extrema),
    ("the local extrema", HelperName::LocalExtrema),
    ("the absolute extrema", HelperName::AbsoluteExtrema),
    ("the intervals of increase", HelperName::IntervalsOfIncrease),
    ("the concavity", HelperName::Concavity),
    ("the inflection points", HelperName::InflectionPoints),
    ("the asymptotes", HelperName::Asymptotes),
    ("the tangent line", HelperName::TangentLine),
    ("the normal line", HelperName::NormalLine),
    ("the linearisation", HelperName::Linearisation),
    ("the linearization", HelperName::Linearisation),
];

/// Reads `src[range]` as a sentence form, or returns `None` when it is prose.
/// Diagnostics from the maths are added to `diags` only on success.
pub(crate) fn parse_sentence(
    src: &str,
    range: Span,
    diags: &mut Vec<Diagnostic>,
) -> Option<BlockKind> {
    if let Some(kind) = using(src, range, diags) {
        return Some(kind);
    }
    if let Some(kind) = helper(src, range, diags) {
        return Some(kind);
    }
    if let Some(kind) = analyse(src, range, diags) {
        return Some(kind);
    }
    if let Some(kind) = solve(src, range, diags) {
        return Some(kind);
    }
    if let Some(kind) = assume(src, range, diags) {
        return Some(kind);
    }
    if let Some(kind) = data(src, range, diags) {
        return Some(kind);
    }
    let_sentence(src, range, diags)
}

fn text(src: &str, span: Span) -> &str {
    src.get(span.start..span.end).unwrap_or("")
}

fn using(src: &str, range: Span, diags: &mut Vec<Diagnostic>) -> Option<BlockKind> {
    let mut c = Cursor::new(src, range);
    if !c.word("using") {
        return None;
    }
    let names = c.math()?;
    let aliases = if c.word("as") { Some(c.math()?) } else { None };
    if !c.word("from") {
        return None;
    }
    let path = c.wiki_path()?;
    if !c.finish_sentence() {
        return None;
    }
    let (names, d) = parse_name_list(text(src, names), names.start);
    diags.extend(d);
    let aliases = aliases.map(|s| {
        let (list, d) = parse_name_list(text(src, s), s.start);
        diags.extend(d);
        list
    });
    let raw = text(src, path);
    let shown = raw.split('|').next().unwrap_or(raw).trim().to_string();
    Some(BlockKind::Using(Using {
        names,
        aliases,
        path: shown,
        path_span: path,
    }))
}

fn helper(src: &str, range: Span, diags: &mut Vec<Diagnostic>) -> Option<BlockKind> {
    let mut c = Cursor::new(src, range);
    let verb = if c.word("find") {
        HelperVerb::Find
    } else if c.word("compute") {
        HelperVerb::Compute
    } else if c.word("determine") {
        HelperVerb::Determine
    } else {
        return None;
    };
    let name = HELPERS
        .iter()
        .find_map(|(phrase, name)| c.word(phrase).then_some(*name))?;
    if !c.word("of") {
        return None;
    }
    let target = c.math()?;
    let interval = if c.word("on") { Some(c.math()?) } else { None };
    let at = if c.word("at") { Some(c.math()?) } else { None };
    if !c.finish_sentence() {
        return None;
    }
    let (target, d) = parse_expr(text(src, target), target.start);
    diags.extend(d);
    let interval = interval.map(|s| {
        let (n, d) = parse_interval(text(src, s), s.start);
        diags.extend(d);
        n
    });
    let at = at.map(|s| {
        let (cl, d) = parse_clause(text(src, s), s.start);
        diags.extend(d);
        cl
    });
    Some(BlockKind::Helper(Helper {
        verb,
        helper: name,
        target,
        interval,
        at,
    }))
}

fn analyse(src: &str, range: Span, diags: &mut Vec<Diagnostic>) -> Option<BlockKind> {
    let mut c = Cursor::new(src, range);
    if !(c.word("analyse") || c.word("analyze")) {
        return None;
    }
    let target = c.math()?;
    let interval = if c.word("on") { Some(c.math()?) } else { None };
    if !c.finish_sentence() {
        return None;
    }
    let (target, d) = parse_expr(text(src, target), target.start);
    diags.extend(d);
    let interval = interval.map(|s| {
        let (n, d) = parse_interval(text(src, s), s.start);
        diags.extend(d);
        n
    });
    Some(BlockKind::Analyse(Analyse { target, interval }))
}

fn solve(src: &str, range: Span, diags: &mut Vec<Diagnostic>) -> Option<BlockKind> {
    let mut c = Cursor::new(src, range);
    if !c.word("solve") {
        return None;
    }
    let equation = c.math()?;
    let unknowns = if c.word("for") { Some(c.math()?) } else { None };
    if !c.finish_sentence() {
        return None;
    }
    let equation = equation_list(src, equation, diags);
    let unknowns = unknowns.map(|s| {
        let (list, d) = parse_name_list(text(src, s), s.start);
        diags.extend(d);
        list
    });
    Some(BlockKind::Solve(Solve { equation, unknowns }))
}

/// `Assume $x > 0$.`, `Assume $n \in \mathbb{Z}$, $a \neq 0$ and $b > 1$.`
fn assume(src: &str, range: Span, diags: &mut Vec<Diagnostic>) -> Option<BlockKind> {
    let mut c = Cursor::new(src, range);
    if !c.word("assume") {
        return None;
    }
    let _ = c.word("that");
    let mut spans = vec![c.math()?];
    loop {
        if c.eat(',') {
            let _ = c.word("and");
        } else if !c.word("and") {
            break;
        }
        spans.push(c.math()?);
    }
    if !c.finish_sentence() {
        return None;
    }
    let mut conditions = Vec::with_capacity(spans.len());
    for s in spans {
        // `$x, y > 0$` inside one span is two conditions sharing the relation.
        let (node, d) = parse_expr(text(src, s), s.start);
        diags.extend(d);
        conditions.push(node);
    }
    Some(BlockKind::Assume(Assume { conditions }))
}

/// `Data: $T$ from [[measurements.csv]].`
fn data(src: &str, range: Span, diags: &mut Vec<Diagnostic>) -> Option<BlockKind> {
    let mut c = Cursor::new(src, range);
    if !c.word("data") || !c.eat(':') {
        return None;
    }
    let name = c.math()?;
    if !c.word("from") {
        return None;
    }
    let path = c.wiki_path()?;
    if !c.finish_sentence() {
        return None;
    }
    let (name, d) = parse_expr(text(src, name), name.start);
    diags.extend(d);
    let raw = text(src, path);
    let shown = raw.split('|').next().unwrap_or(raw).trim().to_string();
    Some(BlockKind::Data(DataImport {
        name,
        path: shown,
        path_span: path,
    }))
}

/// The equations of `Solve $…$`: one equation, or several separated by top-level
/// commas (`x + y = 3, x - y = 1`), which become a tuple of equations.
fn equation_list(src: &str, span: Span, diags: &mut Vec<Diagnostic>) -> Node {
    let parts = top_level_commas(text(src, span));
    if parts.len() < 2 {
        let (node, d) = parse_expr(text(src, span), span.start);
        diags.extend(d);
        return node;
    }
    let elems = parts
        .into_iter()
        .map(|(a, b)| {
            let piece = Span::new(span.start + a, span.start + b);
            let (node, d) = parse_expr(text(src, piece), piece.start);
            diags.extend(d);
            node
        })
        .collect();
    Node::new(
        NodeKind::Tuple {
            elems,
            bracket: Bracket::Paren,
            sized: false,
        },
        span,
    )
}

/// Byte ranges of `text` split at commas outside brackets, braces and environments.
fn top_level_commas(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut depth: i64 = 0;
    let mut parts = Vec::new();
    let mut start = 0;
    let mut k = 0;
    while k < bytes.len() {
        match bytes[k] {
            b'\\' => {
                let rest = &text[k + 1..];
                if rest.starts_with("begin") {
                    depth += 1;
                } else if rest.starts_with("end") {
                    depth -= 1;
                }
                k += 2;
                continue;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                parts.push((start, k));
                start = k + 1;
            }
            _ => {}
        }
        k += 1;
    }
    parts.push((start, bytes.len()));
    parts.retain(|(a, b)| !text[*a..*b].trim().is_empty());
    parts
}

/// `Let $a = 3$.`: a paragraph whose first words are `Let` and a maths span.
fn let_sentence(src: &str, range: Span, diags: &mut Vec<Diagnostic>) -> Option<BlockKind> {
    let mut c = Cursor::new(src, range);
    if !c.word("let") {
        return None;
    }
    c.math()?;
    let content = parse_inlines(src, range, diags);
    Some(BlockKind::Let(Let { content }))
}

/// Words that open a sentence form other than `Let`, lower case.
const OPENERS: &[&str] = &[
    "solve",
    "find",
    "compute",
    "determine",
    "analyse",
    "analyze",
    "using",
    "assume",
    "data",
];

/// Offsets in `text` where a sentence may start (0 and the first non-space
/// after each `.`, `!`, `?` or `:` that is outside `$…$` and code spans and is
/// followed by a space), and where one may end (just past such a `.`, and the
/// end of the text).
fn sentence_bounds(text: &str) -> (Vec<usize>, Vec<usize>) {
    let b = text.as_bytes();
    let mut starts = vec![0];
    let mut ends = Vec::new();
    let mut math = false;
    let mut code = false;
    let mut k = 0;
    while k < b.len() {
        match b[k] {
            b'\\' if !code => {
                k += 2;
                continue;
            }
            b'`' => code = !code,
            b'$' if !code => math = !math,
            c @ (b'.' | b'!' | b'?' | b':') if !math && !code => {
                let next = b.get(k + 1);
                if next.is_none_or(|n| n.is_ascii_whitespace()) {
                    if c == b'.' {
                        ends.push(k + 1);
                    }
                    let rest = &text[k + 1..];
                    let gap = rest.len() - rest.trim_start().len();
                    if k + 1 + gap < b.len() {
                        starts.push(k + 1 + gap);
                    }
                }
            }
            _ => {}
        }
        k += 1;
    }
    ends.push(b.len());
    ends.dedup();
    (starts, ends)
}

/// True when `text` opens with one of the sentence-form verbs.
fn opens_sentence(text: &str) -> bool {
    let word: String = text
        .chars()
        .take_while(char::is_ascii_alphabetic)
        .collect::<String>()
        .to_ascii_lowercase();
    OPENERS.contains(&word.as_str())
}

/// The first complete sentence form (not `Let`) in `src[range]` that starts
/// at a sentence boundary: `Roots. Solve $…$ for $x$.` gives the `Solve …`
/// part. Used to give such a sentence a line of its own inside a list item or
/// a paragraph.
pub(crate) fn find_sentence(src: &str, range: Span) -> Option<Span> {
    let text = src.get(range.start..range.end)?;
    let (starts, ends) = sentence_bounds(text);
    for &s in &starts {
        if !opens_sentence(&text[s..]) {
            continue;
        }
        for &e in ends.iter().filter(|&&e| e > s) {
            let span = Span::new(range.start + s, range.start + e);
            let mut scratch = Vec::new();
            match parse_sentence(src, span, &mut scratch) {
                None | Some(BlockKind::Let(_)) => {}
                Some(_) => return Some(span),
            }
        }
    }
    None
}

/// A sentence in `src[range]` that reads like a `Solve`, `Analyse` or helper
/// sentence (`Solve $…$`, `Find the domain of $…$`) but is not one Math++
/// recognises, so it would be passed over as prose: its span.
pub(crate) fn helper_lookalike(src: &str, range: Span) -> Option<Span> {
    let text = src.get(range.start..range.end)?;
    let (starts, _) = sentence_bounds(text);
    starts.iter().enumerate().find_map(|(n, &s)| {
        // The sentence runs to where the next one starts (or the end).
        let stop = starts.get(n + 1).copied().unwrap_or(text.len());
        let sentence = text[s..stop].trim_end();
        // `Solve $…$ with … :` introduces the display that does the work.
        if sentence.ends_with(':') {
            return None;
        }
        let mut c = Cursor::new(src, Span::new(range.start + s, range.start + stop));
        let looks = if c.word("solve") || c.word("analyse") || c.word("analyze") {
            c.math().is_some()
        } else if c.word("find") || c.word("compute") || c.word("determine") {
            HELPERS.iter().any(|(phrase, _)| c.word(phrase)) && c.word("of") && c.math().is_some()
        } else {
            false
        };
        looks.then(|| Span::new(range.start + s, range.start + s + sentence.len()))
    })
}
