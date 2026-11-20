//! Problems in the document, shown live and in place, like a code editor shows
//! them: a red wavy underline under the exact typeset element that is wrong,
//! yellow for warnings, blue dots for information.
//!
//! Every message is plain English with typeset maths ([`rich::Rich`]); a
//! position is never shown as a line and column, and the `.mpp` text is never
//! shown. Problems come from three places:
//!
//! - the parser, for the current text at once;
//! - the evaluator, after the ~100 ms debounce. While a newer evaluation is
//!   running, the problems of blocks whose text did not change are kept (moved
//!   to where the block is now), so only the changed blocks lose and regain
//!   their marks;
//! - checks of the IDE's own: unused definitions, numeric-only checks, and a
//!   summary of the problems in each imported file on its import line
//!   ([`background`]).

pub mod background;
pub mod fixes;
pub mod map;
pub mod rich;
pub mod suggest;
pub mod ui;

use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;

use egui::Rect;
use mpp_eval::{Binding, Evaluated, LineKind, Verdict};
use mpp_syntax::document::{BlockId, BlockKind, Document};
use mpp_syntax::{Diagnostic, Severity, Span};

use crate::page::Page;
pub use background::Background;
use rich::{from_message, sentence, Rich, Seg};

/// How serious a problem is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    /// Red wavy underline: the maths cannot be worked out.
    Error,
    /// Yellow wavy underline: it can, but something is probably not meant.
    Warning,
    /// Blue dotted underline: something to know.
    Info,
}

impl Level {
    /// The level of a parser or evaluator diagnostic.
    pub fn of(s: Severity) -> Level {
        match s {
            Severity::Error => Level::Error,
            Severity::Warning => Level::Warning,
        }
    }

    /// The colour of its marks in a theme.
    pub fn color(self, theme: &mpp_render::Theme) -> mpp_render::Color {
        match self {
            Level::Error => theme.error,
            Level::Warning => theme.warning,
            Level::Info => theme.info,
        }
    }

    /// "error", "warning" or "note", with a plural.
    pub fn noun(self, n: usize) -> String {
        let w = match self {
            Level::Error => "error",
            Level::Warning => "warning",
            Level::Info => "note",
        };
        if n == 1 {
            format!("1 {w}")
        } else {
            format!("{n} {w}s")
        }
    }
}

/// What a problem is about (decides the message and the quick fixes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// An empty box (`\frac{1}{}`), with its label when the page knows it.
    EmptyBox { label: Option<String> },
    /// A name with no value. `at` is where it is written.
    Undefined {
        name: String,
        at: Option<Span>,
        /// Known names it may be a misspelling of.
        similar: Vec<String>,
        /// Files of the project that define it.
        defined_in: Vec<String>,
        /// Close names that other files of the project define: (name, file).
        similar_elsewhere: Vec<(String, String)>,
    },
    /// A bracket that is never closed. `open` is the opening bracket, `insert`
    /// where the closing one belongs.
    MissingBracket {
        open: Option<usize>,
        close: String,
        insert: usize,
    },
    /// Quantities with units that cannot be combined. (Units are read by the
    /// parser in a later version; nothing produces this yet.)
    UnitMismatch { from: String, to: String },
    /// A definition nothing uses.
    Unused { name: String },
    /// A result that holds only under an assumption. (Assumptions come in a
    /// later version; nothing produces this yet.)
    Assumption,
    /// A check that was tested with numbers only, not proved.
    NumericCheck { points: usize },
    /// Notation kept as written because it cannot be worked out yet.
    NotYet,
    /// Problems inside an imported file, or an imported file that is missing.
    Import {
        file: String,
        path: Option<PathBuf>,
        errors: usize,
        warnings: usize,
        missing: bool,
        /// Close file names when it is missing.
        similar: Vec<String>,
    },
    /// Any other parser or evaluator problem.
    Other,
}

/// One problem, placed on the page.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub span: Span,
    pub level: Level,
    pub kind: Kind,
    /// The message as Unicode text (for lists, tests and screen readers).
    pub message: String,
    /// The message as words and typeset maths.
    pub rich: Rich,
    /// What was expected at this point.
    pub expected: Option<Rich>,
    /// How to fix it.
    pub fix: Option<Rich>,
    /// Page rectangle of the typeset element (the union of `rects`).
    pub rect: Rect,
    /// The rectangles to underline, one per visual line.
    pub rects: Vec<Rect>,
    /// The span of the page block it is in.
    pub block: Option<Span>,
}

impl Problem {
    /// A problem that is not placed on a page yet.
    pub fn new(span: Span, level: Level, kind: Kind, rich: Rich) -> Problem {
        Problem {
            span,
            level,
            kind,
            message: rich.plain(),
            rich,
            expected: None,
            fix: None,
            rect: Rect::NOTHING,
            rects: vec![],
            block: None,
        }
    }

    fn expecting(mut self, e: Option<Rich>) -> Problem {
        self.expected = e.filter(|e| !e.is_empty());
        self
    }

    fn fixing(mut self, f: Rich) -> Problem {
        self.fix = Some(f);
        self
    }

    /// The same problem `delta` bytes further on.
    pub fn shifted(mut self, delta: isize) -> Problem {
        let mv = |o: usize| o.saturating_add_signed(delta);
        self.span = Span::new(mv(self.span.start), mv(self.span.end));
        match &mut self.kind {
            Kind::Undefined { at: Some(s), .. } => *s = Span::new(mv(s.start), mv(s.end)),
            Kind::MissingBracket { open, insert, .. } => {
                *open = open.map(mv);
                *insert = mv(*insert);
            }
            _ => {}
        }
        self
    }
}

/// Error, warning and note counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub errors: usize,
    pub warnings: usize,
    pub infos: usize,
}

impl Counts {
    /// Counts a list of problems.
    pub fn of<'a>(problems: impl IntoIterator<Item = &'a Problem>) -> Counts {
        let mut c = Counts::default();
        for p in problems {
            c.add(p.level, 1);
        }
        c
    }

    /// Adds `n` problems of a level.
    pub fn add(&mut self, level: Level, n: usize) {
        match level {
            Level::Error => self.errors += n,
            Level::Warning => self.warnings += n,
            Level::Info => self.infos += n,
        }
    }

    /// The sum of two counts.
    pub fn plus(self, o: Counts) -> Counts {
        Counts {
            errors: self.errors + o.errors,
            warnings: self.warnings + o.warnings,
            infos: self.infos + o.infos,
        }
    }

    /// Errors plus warnings (notes are not counted as problems).
    pub fn problems(&self) -> usize {
        self.errors + self.warnings
    }

    /// The worst level present.
    pub fn worst(&self) -> Option<Level> {
        if self.errors > 0 {
            Some(Level::Error)
        } else if self.warnings > 0 {
            Some(Level::Warning)
        } else if self.infos > 0 {
            Some(Level::Info)
        } else {
            None
        }
    }

    /// "2 errors, 1 warning", or "No problems".
    pub fn summary(&self) -> String {
        let mut parts = vec![];
        if self.errors > 0 {
            parts.push(Level::Error.noun(self.errors));
        }
        if self.warnings > 0 {
            parts.push(Level::Warning.noun(self.warnings));
        }
        if parts.is_empty() {
            "No problems".into()
        } else {
            parts.join(", ")
        }
    }
}

/// What the problems of a document are gathered from.
pub struct Inputs<'a> {
    pub src: &'a str,
    pub doc: &'a Document,
    pub eval: Option<&'a Evaluated>,
    /// True when `eval` is of exactly `src`.
    pub eval_fresh: bool,
    /// Imported-file summaries and the project's names, when computed.
    pub background: Option<&'a Background>,
    /// Problems inside this range are not shown: the template the caret is
    /// filling in, whose empty boxes show their labels and a "fill in" hint.
    pub quiet: Option<Range<usize>>,
}

/// The block spans of a document by id.
fn block_spans(doc: &Document) -> HashMap<BlockId, Span> {
    let mut m = HashMap::new();
    doc.walk(&mut |b| {
        m.insert(b.id, b.span);
    });
    m
}

/// The parser and evaluator diagnostics for the current text. Evaluation
/// problems of blocks that did not change since the last evaluation are kept,
/// moved to where the block is now.
fn raw_diagnostics(inp: &Inputs<'_>) -> Vec<Diagnostic> {
    let mut out: Vec<Diagnostic> = inp.doc.diagnostics.clone();
    if let Some(ev) = inp.eval {
        if inp.eval_fresh {
            out.extend(ev.all_diagnostics().into_iter().cloned());
        } else {
            let now = block_spans(inp.doc);
            for b in &ev.blocks {
                let Some(new) = now.get(&b.id) else {
                    continue;
                };
                let delta = new.start as isize - b.span.start as isize;
                for d in &b.diagnostics {
                    let mut d = d.clone();
                    d.span = Span::new(
                        d.span.start.saturating_add_signed(delta),
                        d.span.end.saturating_add_signed(delta),
                    );
                    out.push(d);
                }
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|d| seen.insert((d.span, d.message.clone())));
    out.sort_by_key(|d| (d.span.start, d.span.end));
    out
}

/// The names the evaluator has values for.
fn known_names(ev: Option<&Evaluated>) -> Vec<String> {
    ev.map(|e| {
        e.bindings
            .iter()
            .filter(|(_, b)| !matches!(***b, Binding::Imported { .. }))
            .map(|(k, _)| k.clone())
            .collect()
    })
    .unwrap_or_default()
}

/// The names inside backticks in a message (`` `x`, `θ` have no values ``).
fn backticked(msg: &str) -> Vec<String> {
    msg.split('`')
        .enumerate()
        .filter(|(i, _)| i % 2 == 1)
        .map(|(_, s)| s.to_string())
        .collect()
}

/// The opening bracket that `close` would match, scanning back from `before`.
fn unmatched_opener(src: &str, before: usize, open: &str, close: &str) -> Option<usize> {
    let head = src.get(..before)?;
    let floor = head.rfind("$$").or_else(|| head.rfind('$')).unwrap_or(0);
    let mut depth = 0usize;
    let mut i = before;
    while i > floor {
        let Some(c) = head[..i].chars().next_back() else {
            break;
        };
        i -= c.len_utf8();
        let rest = &head[i..];
        // `\{` and `\left(` are still brackets; a backslash before `{` of a
        // command argument is not, but those are balanced anyway.
        if rest.starts_with(close) {
            depth += 1;
        } else if rest.starts_with(open) {
            if depth == 0 {
                return Some(i);
            }
            depth -= 1;
        }
    }
    None
}

/// Where a closing bracket goes when the parser reports its opener only: the
/// end of the maths it is in.
fn end_of_maths(src: &str, from: usize) -> usize {
    let rest = src.get(from..).unwrap_or("");
    let line_end = rest.find('\n').unwrap_or(rest.len());
    let line = &rest[..line_end];
    let cut = line
        .find("$$")
        .or_else(|| line.find('$'))
        .unwrap_or(line.len());
    from + line[..cut].trim_end().len()
}

fn is_empty_box(src: &str, span: Span) -> bool {
    let at = span.start;
    (at > 0 && src.get(at - 1..at + 1) == Some("{}"))
        || (span.is_empty()
            && src.get(at..at + 1) == Some("}")
            && at > 0
            && src.get(at - 1..at) == Some("{"))
}

/// The expected list of a parser diagnostic as words and maths.
fn expected_of(d: &Diagnostic) -> Option<Rich> {
    if d.expected.is_empty() {
        return None;
    }
    let joined = d.expected.join(" or ");
    let r = from_message(&joined);
    Some(sentence(Rich::text("Expected ").and(lower_first(r))))
}

fn lower_first(mut r: Rich) -> Rich {
    if let Some(Seg::Text(t)) = r.0.first_mut() {
        let mut cs = t.chars();
        if let Some(c) = cs.next() {
            *t = c.to_lowercase().chain(cs).collect();
        }
    }
    if let Some(Seg::Text(t)) = r.0.last_mut() {
        if t.ends_with('.') {
            t.pop();
        }
    }
    r
}

/// Words for a template box label (`upper` → "upper bound").
pub fn box_name(label: &str) -> String {
    crate::insert::long_label(label)
}

/// The project names and files a missing name could come from.
fn origins(name: &str, bg: Option<&Background>) -> Vec<String> {
    bg.map(|b| b.index.files_defining(name)).unwrap_or_default()
}

/// Turns one diagnostic into problems (an unbound list gives one per name).
pub fn classify(
    src: &str,
    d: &Diagnostic,
    known: &[String],
    bg: Option<&Background>,
) -> Vec<Problem> {
    let level = Level::of(d.severity);
    let msg = d.message.as_str();
    // Names without values.
    if msg.contains("no value") {
        let names = backticked(msg);
        return names
            .into_iter()
            .map(|name| undefined(d.span, &name, known, bg))
            .collect();
    }
    // Brackets never closed.
    if let Some(rest) = msg.strip_prefix("expected `") {
        if let Some((close, tail)) = rest.split_once("` to close the `") {
            let open = tail.trim_end_matches('`');
            let opener = unmatched_opener(src, d.span.start, open, close);
            return vec![missing_bracket(d, opener, close, d.span.start)];
        }
    }
    if msg == "this `{` is never closed" {
        let insert = end_of_maths(src, d.span.end);
        return vec![missing_bracket(d, Some(d.span.start), "}", insert)];
    }
    if is_empty_box(src, d.span) && level == Level::Error {
        return vec![Problem::new(
            d.span,
            Level::Error,
            Kind::EmptyBox { label: None },
            Rich::text("This box is empty."),
        )
        .expecting(expected_of(d))
        .fixing(Rich::text(
            "Click the box (or press Tab to reach it) and type what goes there.",
        ))];
    }
    if msg.contains("cannot be computed yet") {
        return vec![
            Problem::new(d.span, Level::Info, Kind::NotYet, from_message(msg)).fixing(Rich::text(
                "Nothing to fix: it is worked out once this kind of maths is supported.",
            )),
        ];
    }
    if let Some(file) = msg
        .strip_prefix("the file ")
        .and_then(|r| r.strip_suffix(" could not be opened"))
    {
        let similar = bg
            .map(|b| suggest::suggester().files(file, &b.index.file_names()))
            .unwrap_or_default();
        let mut fix = Rich::text("Check the file name");
        if let Some(s) = similar.first() {
            fix = fix
                .t(", or choose ")
                .i(s.clone())
                .t(" from the quick fixes");
        }
        return vec![Problem::new(
            d.span,
            Level::Error,
            Kind::Import {
                file: file.to_string(),
                path: None,
                errors: 0,
                warnings: 0,
                missing: true,
                similar,
            },
            Rich::text("Can't find ").i(file.to_string()).t("."),
        )
        .fixing(sentence(fix))];
    }
    vec![Problem::new(d.span, level, Kind::Other, from_message(msg)).expecting(expected_of(d))]
}

fn missing_bracket(d: &Diagnostic, open: Option<usize>, close: &str, insert: usize) -> Problem {
    let brace = close == "}";
    let rich = if brace {
        Rich::text("This part is never closed.")
    } else {
        Rich::text(format!("Missing closing bracket {close}."))
    };
    let expected = if brace {
        Rich::text("Expected the end of the group.")
    } else {
        Rich::text(format!(
            "Expected a closing {close} to match the opening one."
        ))
    };
    let span = open.map_or(d.span, |o| {
        let len = if brace { 1 } else { close.len().max(1) };
        Span::new(o, o + len)
    });
    Problem::new(
        span,
        Level::Error,
        Kind::MissingBracket {
            open,
            close: close.to_string(),
            insert,
        },
        rich,
    )
    .expecting(Some(expected))
    .fixing(Rich::text(
        "Press Ctrl+. and choose \u{201c}Close the bracket\u{201d}.",
    ))
}

fn undefined(span: Span, name: &str, known: &[String], bg: Option<&Background>) -> Problem {
    let sugg = suggest::suggester();
    let similar = sugg.names(name, known);
    let defined_in = origins(name, bg);
    // Close names defined in other files of the folder (not imported yet).
    let mut similar_elsewhere: Vec<(String, String)> = vec![];
    if let Some(b) = bg {
        for (file, _, names) in &b.index.files {
            for s in sugg.names(name, names) {
                if !similar.contains(&s) && !similar_elsewhere.iter().any(|(n, _)| *n == s) {
                    similar_elsewhere.push((s, file.clone()));
                }
            }
        }
    }
    let mut rich = Rich::text("Undefined: ").m(name.to_string()).t(".");
    let mut options: Vec<Rich> = similar
        .iter()
        .take(2)
        .map(|s| Rich::default().m(s.clone()))
        .collect();
    let room = 2usize.saturating_sub(options.len());
    for (s, f) in similar_elsewhere.iter().take(room) {
        options.push(Rich::default().m(s.clone()).t(" from ").i(f.clone()));
    }
    let import = defined_in
        .first()
        .map(|f| Rich::text("import it from ").i(f.clone()));
    if !options.is_empty() {
        // "Did you mean ϑ, or import it from launch.mpp?"
        rich = rich.t(" Did you mean ");
        let n = options.len() + usize::from(import.is_some());
        for (i, o) in options.into_iter().chain(import).enumerate() {
            if i > 0 {
                rich = rich.t(if i + 1 == n { ", or " } else { ", " });
            }
            rich = rich.and(o);
        }
        rich = rich.t("?");
    } else if let Some(f) = defined_in.first() {
        // "It is defined in launch.mpp: import it?"
        rich = rich.t(" It is defined in ").i(f.clone()).t(": import it?");
    }
    let expected = Rich::text("Expected a value for ")
        .m(name.to_string())
        .t(", so that there is a number to give.");
    let mut fix = Rich::text("Define it (for example ")
        .m(format!("{name} := 1"))
        .t(")");
    if let Some(f) = defined_in.first() {
        fix = fix.t(", or import it from ").i(f.clone());
    }
    Problem::new(
        span,
        Level::Error,
        Kind::Undefined {
            name: name.to_string(),
            at: None,
            similar,
            defined_in,
            similar_elsewhere,
        },
        rich,
    )
    .expecting(Some(expected))
    .fixing(fix.t("."))
}

/// The IDE's own checks on an evaluation: definitions nothing uses, checks
/// tested with numbers only, checks it could not decide.
pub fn checks(doc: &Document, ev: &Evaluated) -> Vec<Problem> {
    let mut out = vec![];
    for b in &ev.blocks {
        for line in &b.values {
            match (&line.kind, &line.verdict) {
                (LineKind::Check, Some(Verdict::Holds { points })) => out.push(
                    Problem::new(
                        line.span,
                        Level::Warning,
                        Kind::NumericCheck { points: *points },
                        Rich::text("Checked with numbers only."),
                    )
                    .expecting(Some(Rich::text(format!(
                        "A proof. The two sides agree at {points} random points, which makes it very likely, but it was not proved."
                    ))))
                    .fixing(Rich::text(
                        "Simplify one side by hand, or leave it: the check is very probably right.",
                    )),
                ),
                (LineKind::Check, Some(Verdict::Unknown(why))) => out.push(
                    Problem::new(
                        line.span,
                        Level::Info,
                        Kind::Other,
                        Rich::text("Could not decide whether the two sides are equal."),
                    )
                    .expecting(Some(from_message(why))),
                ),
                _ => {}
            }
        }
    }
    // Definitions nothing uses. A file with nothing to work out (no `?`, no
    // check, no figure) is a library for other files, so it is left alone.
    let consumes = ev.blocks.iter().any(|b| {
        b.values.iter().any(|l| {
            matches!(
                l.kind,
                LineKind::Query { .. } | LineKind::Check | LineKind::Figure
            )
        })
    }) || doc_has_figure(doc);
    if consumes {
        let used: std::collections::HashSet<&str> = ev
            .blocks
            .iter()
            .flat_map(|b| b.uses.iter().map(String::as_str))
            .collect();
        for (name, binding) in &ev.bindings {
            let origin = binding.origin();
            if origin.file.is_some() || matches!(**binding, Binding::Imported { .. }) {
                continue;
            }
            if used.contains(name.as_str()) {
                continue;
            }
            out.push(
                Problem::new(
                    origin.span,
                    Level::Warning,
                    Kind::Unused { name: name.clone() },
                    Rich::text("Unused: ")
                        .m(name.clone())
                        .t(" is defined but nothing uses it."),
                )
                .fixing(Rich::text(
                    "Use it in a formula, or delete the definition if it is not needed.",
                )),
            );
        }
    }
    out
}

fn doc_has_figure(doc: &Document) -> bool {
    let mut any = false;
    doc.walk(&mut |b| any |= matches!(b.kind, BlockKind::Figure(_)));
    any
}

/// The import lines of a document: (path as written, span of the line).
pub fn import_lines(doc: &Document) -> Vec<(String, Span)> {
    let mut out = vec![];
    doc.walk(&mut |b| match &b.kind {
        BlockKind::EmbedImport { path, .. } => out.push((path.clone(), b.span)),
        BlockKind::Using(u) => out.push((u.path.clone(), b.span)),
        _ => {}
    });
    out
}

/// The problems summarised on each import line.
fn import_problems(doc: &Document, bg: &Background) -> Vec<Problem> {
    let mut out = vec![];
    let lines = import_lines(doc);
    for s in &bg.imports {
        let counts = Counts::of(&s.problems);
        if counts.problems() == 0 {
            continue;
        }
        let Some((_, span)) = lines.iter().find(|(p, _)| *p == s.written) else {
            continue;
        };
        let level = if counts.errors > 0 {
            Level::Error
        } else {
            Level::Warning
        };
        out.push(
            Problem::new(
                *span,
                level,
                Kind::Import {
                    file: s.display.clone(),
                    path: Some(s.path.clone()),
                    errors: counts.errors,
                    warnings: counts.warnings,
                    missing: false,
                    similar: vec![],
                },
                Rich::text(format!("{} in ", counts.summary())).i(s.display.clone()),
            )
            .fixing(
                Rich::text("Open ")
                    .i(s.display.clone())
                    .t(" to fix them (Ctrl+. on this line, or click them in the Problems panel)."),
            ),
        );
    }
    out
}

/// Every problem of a document, page-independent (`rects` empty), in source order.
pub fn gather(inp: &Inputs<'_>) -> Vec<Problem> {
    let known = known_names(inp.eval);
    let mut out: Vec<Problem> = raw_diagnostics(inp)
        .iter()
        .flat_map(|d| classify(inp.src, d, &known, inp.background))
        .collect();
    if let (Some(ev), true) = (inp.eval, inp.eval_fresh) {
        out.extend(checks(inp.doc, ev));
    }
    if let Some(bg) = inp.background {
        out.extend(import_problems(inp.doc, bg));
    }
    if let Some(q) = &inp.quiet {
        out.retain(|p| !(q.start <= p.span.start && p.span.end <= q.end));
    }
    out.sort_by_key(|p| (p.span.start, p.level, p.span.end));
    out
}

/// Places problems on a page: the exact boxes to underline.
pub fn place(mut problems: Vec<Problem>, page: &Page, src: &str) -> Vec<Problem> {
    for p in &mut problems {
        let rects = match &mut p.kind {
            Kind::Undefined { name, at, .. } => {
                let found = map::name_nodes(page, src, block_span(page, p.span), name);
                // The first place the name is written in the expression the
                // evaluator pointed at (or in its block).
                let pick = found
                    .iter()
                    .find(|(s, _)| p.span.start <= s.start && s.end <= p.span.end.max(s.end))
                    .or(found.first());
                match pick {
                    Some((s, r)) => {
                        *at = Some(*s);
                        p.span = *s;
                        vec![*r]
                    }
                    None => map::rects_for_span(page, p.span),
                }
            }
            Kind::MissingBracket { open: Some(o), .. } => match map::opener(page, *o) {
                Some(r) if src.get(*o..*o + 1) != Some("{") => vec![r],
                // A brace is not drawn: underline what it holds.
                _ => {
                    let inside = Span::new(*o + 1, p.span.end.max(*o + 1));
                    let r = map::nodes_inside(
                        page,
                        Span::new(inside.start, block_span(page, p.span).end),
                    );
                    if r.is_empty() {
                        map::rects_for_span(page, p.span)
                    } else {
                        r
                    }
                }
            },
            Kind::EmptyBox { label } => match map::slot_at(page, p.span) {
                Some((r, l)) => {
                    *label = l.filter(|l| !l.is_empty());
                    vec![r]
                }
                None => map::rects_for_span(page, p.span),
            },
            Kind::Unused { name } => {
                let found = map::name_nodes(page, src, p.span, name);
                match found.first() {
                    Some((s, r)) => {
                        p.span = *s;
                        vec![*r]
                    }
                    None => map::rects_for_span(page, p.span),
                }
            }
            Kind::Import { .. } => {
                let r = map::line_ink(page, p.span);
                if r.is_empty() {
                    map::rects_for_span(page, p.span)
                } else {
                    r
                }
            }
            _ => map::rects_for_span(page, p.span),
        };
        if let Kind::EmptyBox { label: Some(l) } = &p.kind {
            p.rich = Rich::text("Empty box: ").i(box_name(l)).t(".");
            p.message = p.rich.plain();
        }
        p.rect = rects
            .iter()
            .copied()
            .reduce(|a, b| a.union(b))
            .unwrap_or(Rect::NOTHING);
        p.rects = rects;
        p.block = page.block_at(p.span.start).map(|b| b.span);
    }
    problems.retain(|p| !p.rects.is_empty());
    problems
}

/// The span of the page block holding `span` (or `span` itself).
fn block_span(page: &Page, span: Span) -> Span {
    page.block_at(span.start).map_or(span, |b| b.span)
}

/// Gathers and places the problems of the current text.
pub fn collect(inp: &Inputs<'_>, page: &Page) -> Vec<Problem> {
    place(gather(inp), page, inp.src)
}

#[cfg(test)]
mod tests;
