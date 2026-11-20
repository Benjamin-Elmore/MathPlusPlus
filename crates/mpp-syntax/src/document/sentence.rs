//! The sentence forms of grammar §3: `Using … from`, the helpers, `Analyse`,
//! `Solve` and the `Let` convenience definition.
//!
//! A sentence is recognised by its shape alone (words and `$…$` spans). Only
//! once the shape fits is the maths parsed, so prose that merely starts with
//! `Using $x$ …` stays prose and reports nothing.

use super::ast::*;
use super::cursor::Cursor;
use super::inline::parse_inlines;
use crate::math::{parse_clause, parse_expr, parse_interval, parse_name_list};
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
    let (equation, d) = parse_expr(text(src, equation), equation.start);
    diags.extend(d);
    let unknowns = unknowns.map(|s| {
        let (list, d) = parse_name_list(text(src, s), s.start);
        diags.extend(d);
        list
    });
    Some(BlockKind::Solve(Solve { equation, unknowns }))
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
