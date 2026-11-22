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
mod placed;
pub mod rich;
pub mod suggest;
pub mod ui;

use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;

use egui::Rect;
use mpp_eval::{Binding, Evaluated, LineKind, Verdict};
use mpp_syntax::document::{BlockId, BlockKind, DisplayMath, Document};
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
            Severity::Info => Level::Info,
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
    /// Quantities with units that cannot be combined, or a conversion between
    /// units of different kinds (`from` is the quantity's unit, `to` the other).
    UnitMismatch {
        from: String,
        to: String,
        /// A conversion (`… in s`) rather than a sum or a comparison.
        convert: bool,
    },
    /// A definition nothing uses.
    Unused { name: String },
    /// Letters side by side that spell no defined name (`velocity`): the grammar
    /// reads them as a product of one-letter names.
    LetterRun {
        word: String,
        /// The function the word is close to (`sin` for `sinn`).
        function: Option<String>,
    },
    /// A result that holds only under a condition (`x ≠ 0`), given as LaTeX.
    Assumption { conditions: Vec<String> },
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
    /// A proof step the checker could not confirm: wrong, a gap or an external
    /// claim, structure only, or cited.
    ProofStep { status: mpp_proof::StepStatus },
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
    /// Proof step statuses of exactly `src` (gaps, wrong steps and structure-only
    /// steps become marks; checked steps none).
    pub proof: &'a [crate::proof::StepMark],
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
    // In span order, each message once per span (the first copy kept). Sorting first
    // puts the copies side by side, so no set of every message is needed.
    out.sort_by_key(|d| (d.span.start, d.span.end));
    let mut kept: Vec<Diagnostic> = Vec::with_capacity(out.len());
    let mut group = 0;
    for d in out {
        if kept.last().is_some_and(|l| l.span != d.span) {
            group = kept.len();
        }
        if !kept[group..].iter().any(|k| k.message == d.message) {
            kept.push(d);
        }
    }
    kept
}

/// The names the evaluator has values for.
pub(crate) fn known_names(ev: Option<&Evaluated>) -> Vec<String> {
    ev.map(|e| {
        e.bindings
            .iter()
            .filter(|(k, b)| {
                !matches!(***b, Binding::Imported { .. }) && !mpp_eval::is_internal_name(k)
            })
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
    // Brackets the parser names as code read as words: "a number, a name or a
    // bracketed expression", never a bare "(".
    let words: Vec<String> = d
        .expected
        .iter()
        .map(|e| {
            match e.as_str() {
                "`(`" | "`[`" => "a bracketed expression",
                "`{`" => "a group in braces",
                "`)`" | "`]`" => "a closing bracket",
                "`}`" => "a closing brace",
                other => other,
            }
            .to_string()
        })
        .collect();
    let joined = match words.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} or {last}", rest.join(", ")),
        _ => words.join(""),
    };
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
    // The evaluator's "2 errors in mechanics.mpp" on an import line: the IDE's own
    // summary of the imported file (with its warnings, and a link) replaces it.
    if bg.is_some_and(|b| !b.imports.is_empty()) && is_import_summary(msg) {
        return vec![];
    }
    if let Some(p) = unit_mismatch(d, msg) {
        return vec![p];
    }
    // Letters read as a product: the evaluator's warning, with the IDE's quick fixes.
    if let Some((word, function)) = mpp_eval::letter_runs::parse_message(msg) {
        let spaced: Vec<String> = word.chars().map(String::from).collect();
        let ask = match &function {
            Some(f) => Rich::text(" of single letters. Did you mean ")
                .m(format!(r"\mathrm{{{f}}}"))
                .t("?"),
            None => Rich::text(" of single letters. Did you mean the name ")
                .m(format!(r"\mathrm{{{word}}}"))
                .t("?"),
        };
        return vec![Problem::new(
            d.span,
            Level::Warning,
            Kind::LetterRun { word, function },
            Rich::text("Read as the product ")
                .m(spaced.join(r" \cdot "))
                .and(ask),
        )
        .fixing(Rich::text(
            "Press Ctrl+. to use it as one name, or to keep the product.",
        ))];
    }
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
    let missing_file = msg
        .strip_prefix("the file ")
        .and_then(|r| r.strip_suffix(" could not be opened"))
        .map(str::to_string)
        .or_else(|| {
            // The file importer names the resolved path; the page shows the name as
            // it was written on the import line.
            msg.starts_with("cannot find the file ")
                .then(|| written_import_name(src, d.span))
                .flatten()
        });
    if let Some(file) = missing_file.as_deref() {
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

/// Units in a sentence, upright.
fn unit_latex(u: &str) -> String {
    if u == "a plain number" {
        "1".into()
    } else {
        format!("\\mathrm{{{u}}}")
    }
}

/// "can't add m and s", "can't convert km to s": units that do not fit together.
fn unit_mismatch(d: &Diagnostic, msg: &str) -> Option<Problem> {
    let rest = msg.strip_prefix("can't ")?;
    let (verb, pair) = rest.split_once(' ')?;
    let convert = verb == "convert";
    let (from, to) = if convert {
        pair.split_once(" to ")?
    } else {
        pair.split_once(" and ")?
    };
    let (from, to) = (from.trim().to_string(), to.trim().to_string());
    let rich = if convert {
        Rich::text("Units of different kinds: ")
            .m(unit_latex(&from))
            .t(" cannot be written in ")
            .m(unit_latex(&to))
            .t(".")
    } else {
        Rich::text(format!("Units don't match: can't {verb} "))
            .m(unit_latex(&from))
            .t(" and ")
            .m(unit_latex(&to))
            .t(".")
    };
    let fix = if convert {
        Rich::text("Press Ctrl+. to convert to ")
            .m(unit_latex(&from))
            .t(" instead, or pick a unit of the same kind.")
    } else {
        Rich::text("Check the units: both sides need the same kind of unit (lengths with lengths, times with times).")
    };
    Some(
        Problem::new(
            d.span,
            Level::Error,
            Kind::UnitMismatch { from, to, convert },
            rich,
        )
        .fixing(fix),
    )
}

/// True for the evaluator's summary of an imported file's errors (`3 errors in a.mpp`).
fn is_import_summary(msg: &str) -> bool {
    let digits = msg.chars().take_while(char::is_ascii_digit).count();
    digits > 0
        && (msg[digits..].starts_with(" error in ") || msg[digits..].starts_with(" errors in "))
}

/// The file name written on an import line, from the span the evaluator reports
/// (the path itself, or the whole line).
fn written_import_name(src: &str, span: Span) -> Option<String> {
    let text = src.get(span.start..span.end)?;
    let inner = match (text.find("[["), text.find("]]")) {
        (Some(a), Some(b)) if a + 2 <= b => &text[a + 2..b],
        _ => text,
    };
    let name = inner.split('|').next().unwrap_or(inner).trim();
    (!name.is_empty()).then(|| name.to_string())
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
    let mut defined_in = origins(name, bg);
    // A name the standard library defines can be imported from it.
    if let Some(f) = crate::library::file_of(name) {
        if !defined_in.iter().any(|d| d == f) {
            defined_in.push(f.to_string());
        }
    }
    // Close names defined in other files of the folder (not imported yet).
    let mut similar_elsewhere: Vec<(String, String)> = vec![];
    // ... and close names of the standard library ("did you mean A_circle from std/geometry.mpp").
    if similar.is_empty() && defined_in.is_empty() {
        for s in sugg
            .names(name, crate::library::names())
            .into_iter()
            .take(2)
        {
            if let Some(f) = crate::library::file_of(&s) {
                similar_elsewhere.push((s, f.to_string()));
            }
        }
    }
    if let Some(b) = bg {
        for (file, _, names) in &b.index.files {
            for s in sugg.names(name, names) {
                if !similar.contains(&s) && !similar_elsewhere.iter().any(|(n, _)| *n == s) {
                    similar_elsewhere.push((s, file.clone()));
                }
            }
        }
    }
    let mut rich = Rich::default().m(name.to_string()).t(" isn't defined.");
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
                (LineKind::Query { .. }, _) if !line.conditions.is_empty() => {
                    let conditions: Vec<String> =
                        line.conditions.iter().map(|c| c.to_latex()).collect();
                    let mut rich = Rich::text("Holds when ");
                    for (i, c) in conditions.iter().enumerate() {
                        if i > 0 {
                            rich = rich.t(" and ");
                        }
                        rich = rich.m(c.clone());
                    }
                    out.push(
                        Problem::new(
                            line.span,
                            Level::Warning,
                            Kind::Assumption {
                                conditions: conditions.clone(),
                            },
                            rich.t("."),
                        )
                        .fixing(Rich::text(
                            "Add an assumption (Assume …) if you know which case applies.",
                        )),
                    );
                }
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
            if origin.file.is_some()
                || matches!(**binding, Binding::Imported { .. })
                || mpp_eval::is_internal_name(name)
            {
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
    let context = classify_context(&known, inp.background);
    let raw = raw_diagnostics(inp);
    let mut out: Vec<Problem> = placed::classify_cached(inp.doc, &raw, context, |d| {
        classify(inp.src, d, &known, inp.background)
    });
    if let (Some(ev), true) = (inp.eval, inp.eval_fresh) {
        out.extend(checks(inp.doc, ev));
    }
    out.extend(prose_hints(inp.src, inp.doc, &known));
    if let Some(bg) = inp.background {
        out.extend(import_problems(inp.doc, bg));
    }
    out.extend(proof_problems(inp.proof));
    if let Some(q) = &inp.quiet {
        out.retain(|p| !(q.start <= p.span.start && p.span.end <= q.end));
    }
    out.sort_by_key(|p| (p.span.start, p.level, p.span.end));
    out
}

/// Prose lines that were meant as a question (P7, P8): a defined name on its own or
/// `print(R)` (type `R =` to see its value), and SymPy or Python forms of an
/// integral or a sum (the form written here). Notes, never errors.
pub(crate) fn prose_hints(src: &str, doc: &Document, known: &[String]) -> Vec<Problem> {
    let mut out = vec![];
    doc.walk(&mut |b| {
        if !matches!(b.kind, BlockKind::Paragraph(_)) {
            return;
        }
        let Some(text) = src.get(b.span.start..b.span.end) else {
            return;
        };
        let mut at = b.span.start;
        for line in text.split('\n') {
            let span = Span::new(at, at + line.len());
            at += line.len() + 1;
            if let Some(hint) = prose_hint(line.trim(), known) {
                out.push(Problem::new(span, Level::Info, Kind::Other, hint));
            }
        }
    });
    out
}

/// The hint for one prose line, if it reads as a question in another tool's form.
fn prose_hint(line: &str, known: &[String]) -> Option<Rich> {
    let name = line
        .strip_prefix("print(")
        .and_then(|r| r.strip_suffix(')'))
        .or_else(|| line.strip_prefix("print "))
        .unwrap_or(line)
        .trim();
    if !name.is_empty()
        && name.chars().all(|c| c.is_alphanumeric() || c == '_')
        && known.iter().any(|k| k == name)
    {
        return Some(
            Rich::text("To see the value of ")
                .m(name.to_string())
                .t(", type ")
                .m(format!("{name} ="))
                .t(" on a line of its own."),
        );
    }
    if line.contains("integrate(") {
        return Some(
            Rich::text("Integrals are written int(f, x, a, b) here: ")
                .m("int(x^2, x, 0, 3) =".to_string())
                .t(" (SymPy's integrate(f, (x, a, b)))."),
        );
    }
    if line.contains("sum(") && line.contains(" for ") && line.contains(" in ") {
        return Some(
            Rich::text("Sums are written sum(f, n, a, b) here, with b included: ")
                .m("sum(1/n^2, n, 1, 10) =".to_string())
                .t(" (Python's range(1, 11) stops before 11)."),
        );
    }
    None
}

/// A fingerprint of what classifying a diagnostic reads besides the diagnostic and
/// its block: the known names and the project's files and names.
fn classify_context(known: &[String], bg: Option<&Background>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    known.hash(&mut h);
    if let Some(bg) = bg {
        for s in &bg.imports {
            (&s.path, &s.written, s.problems.len()).hash(&mut h);
        }
        bg.index.files.hash(&mut h);
    }
    h.finish()
}

/// The lines of a maths block (by index) with a run of letters that spells no
/// defined name: their answer would be the product of the letters, which is not
/// what was meant, so the page shows the warning (from the evaluator) and no answer.
pub(crate) fn lines_with_letter_runs(m: &DisplayMath, known: &[String]) -> Vec<usize> {
    let is_known = |w: &str| known.iter().any(|k| k == w);
    m.ast
        .lines
        .iter()
        .enumerate()
        .filter(|(_, line)| !mpp_eval::letter_runs::hits(line, &is_known).is_empty())
        .map(|(i, _)| i)
        .collect()
}

/// The proof steps that get a mark: a wrong step is an error, a gap or an external
/// claim a warning, structure-only and cited steps notes.
fn proof_problems(steps: &[crate::proof::StepMark]) -> Vec<Problem> {
    use mpp_proof::model::Squiggle;
    let mut out = vec![];
    for s in steps {
        let Some(sq) = s.status.squiggle() else {
            continue;
        };
        let level = match sq {
            Squiggle::Error => Level::Error,
            Squiggle::Warning => Level::Warning,
            Squiggle::Info => Level::Info,
        };
        let step = s
            .number
            .map_or_else(|| "This step".to_string(), |n| format!("Step {n}"));
        // The status in words (the mark itself is drawn in the margin); the detail
        // often starts with the same word ("wrong: …"), which is said once.
        let name = s.status.name();
        let mut text = format!("{step} of the proof of {}: {name}", s.result);
        let detail = s.detail.trim();
        let detail = detail
            .strip_prefix(name)
            .map_or(detail, |d| d.trim_start_matches([':', ' ']));
        if !detail.is_empty() {
            text.push_str(" \u{2014} ");
            text.push_str(detail);
        }
        let mut p = Problem::new(
            s.span,
            level,
            Kind::ProofStep { status: s.status },
            from_message(&text),
        );
        if let Some(cx) = &s.counterexample {
            p = p.fixing(from_message(&format!("Counterexample: {cx}")));
        }
        out.push(p);
    }
    out
}

/// Places problems on a page: the exact boxes to underline.
pub fn place(mut problems: Vec<Problem>, page: &Page, src: &str) -> Vec<Problem> {
    placed::place_cached(&mut problems, page, |p| place_one(p, page, src));
    problems.retain(|p| !p.rects.is_empty());
    problems
}

/// Places one problem on the page.
fn place_one(p: &mut Problem, page: &Page, src: &str) {
    {
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
                let r = map::line_ink(page, block_span(page, p.span));
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
