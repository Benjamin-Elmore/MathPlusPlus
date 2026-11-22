//! Answers in a fixed slot (U16, U17).
//!
//! A maths line that ends in `=` (`1.321321312 + 5 =`) gets its answer by itself, a
//! definition worked out from other names (`E := 2x + 5`) shows its value in grey, and
//! `x = 5` shows whether it defined `x` or checked it (✓ / ✗). All of these go in one
//! slot per line, outside the formula's own layout:
//!
//! - right after the line's end (after its `=`), or on a row of its own under the block
//!   when the line is long; the slot is there as soon as the line ends in `=`, so an
//!   answer arriving moves nothing (Design rule 5);
//! - while a new run is going, the last answers stay, dimmed;
//! - an error goes in the same slot, in red with a ⚠, with the whole message on hover;
//! - answers are never caret stops: a click copies one, Ctrl+click inserts it at the
//!   caret ([`click`]).
//!
//! Inline maths in a sentence that ends in `=` gets the same answer as a chip, in room
//! kept after it ([`inline_room`]).

use egui::{Pos2, Rect, Vec2};
use mpp_eval::{BlockResult, Evaluated, LineKind, LineResult as Answered};
use mpp_render::{
    layout_latex, Color, DisplayList, HitKind, LayoutOptions, LineResult, PathCmd, Primitive,
    ResultKind, ResultValue, Theme,
};
use mpp_syntax::document::{Block, BlockKind, Document};
use mpp_syntax::math::{Line, MathAst, Node, NodeKind, QueryMode, Statement};
use mpp_syntax::{Severity, Span};

use super::build::{prose_latex, PageInput};
use super::{Canvas, Link, LinkTarget, PageLink};
use crate::theme::answer_colors as colors;

// ---- What goes in a slot -----------------------------------------------------------

/// What the slot of one maths line shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    /// Nothing yet: the slot is kept for the answer.
    Empty,
    /// The answer to a line ending in `=`.
    Answer { latex: String, copy: String },
    /// A definition's value in grey (`= 15`).
    Preview { latex: String, copy: String },
    /// `x = 5` read as a definition, with the value when it was worked out.
    Defined { value: Option<(String, String)> },
    /// `x = 5` with `x` already defined, `R ≈ 35.3`, `R == 35.3`, `|R − 35.3| < 0.1`:
    /// does it hold? `left` is the left side as written (LaTeX) and its value, shown
    /// beside a ✗ (`✗ R = 35.31…`).
    Check {
        holds: Option<bool>,
        why: String,
        left: Option<(String, String)>,
    },
    /// The line's error: a short form for the slot and the whole message.
    Error { short: String, full: String },
}

/// The answer slot of one line of a maths block.
#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    /// Index of the line in the block.
    pub line: usize,
    /// The line ends in `=`: its slot is kept even while it is empty.
    pub open: bool,
    pub content: Content,
    /// From an earlier run of other text: shown dimmed.
    pub stale: bool,
    /// The end of the line in the run's text, for an answer: the slot is hit like a
    /// result in the formula (hover card, right-click formats).
    pub result: Option<Span>,
}

/// True when a line's text ends in a lone `=` (`1.32 + 5 =`), asking for its answer
/// without the `?`.
pub fn ends_open(text: &str) -> bool {
    let t = text.trim_end();
    t.ends_with('=') && !t.ends_with("==") && !t.ends_with(":=")
}

/// True when `line` of a display block has its answer drawn in the slot: a trailing
/// `=`, and also `= ?` and `≈ ?` (not `= ?!`, whose working is drawn under it). The
/// answer never goes in the formula, so it arriving never changes the line's height.
/// (An answer that is an equation or a table still goes in the formula: see [`rich`].)
pub fn is_slot_line(line: &Line, src: &str) -> bool {
    matches!(line.statement, Statement::Query { steps: false, .. }) || is_open_line(line, src)
}

/// True when an answer is drawn in the formula even on a slot line: an equation
/// (`L(x) = 25x − 54`, after an arrow) or a table under the line.
fn rich(v: &Answered) -> bool {
    matches!(
        v.value,
        Some(mpp_eval::Value::Relation(_) | mpp_eval::Value::Table(_))
    )
}

/// True when `line` asks for its answer with a trailing `=`.
pub fn is_open_line(line: &Line, src: &str) -> bool {
    let asks = matches!(
        line.statement,
        Statement::Query {
            mode: QueryMode::Exact,
            ..
        } | Statement::Definition {
            query: Some(QueryMode::Exact),
            ..
        }
    );
    asks && src
        .get(line.span.start..line.span.end)
        .is_some_and(ends_open)
}

/// An empty result standing in for the answer of an open line, so the formula draws
/// its `=` and no `?`; the answer itself is drawn in the slot.
pub fn placeholder(line: usize) -> LineResult {
    LineResult {
        line,
        value: ResultValue::Node(Node::new(NodeKind::Symbol(""), Span::point(0))),
        kind: ResultKind::Ghost,
        relation: None,
    }
}

/// The run the answers of a block come from.
struct Found<'e> {
    br: &'e BlockResult,
    /// The span of each of the block's lines in the text that was run.
    lines: Vec<Span>,
    stale: bool,
}

/// The block's results: from its own run, or (while a run of the edited text is still
/// going) from the block at the same place in the text that was run last.
fn found<'e>(
    ev: &'e Evaluated,
    src: &str,
    doc: &Document,
    b: &Block,
    ast: &MathAst,
) -> Option<Found<'e>> {
    let stale = ev.source() != src;
    if let Some(br) = ev.block(b.id) {
        let delta = b.span.start as isize - br.span.start as isize;
        let lines = ast
            .lines
            .iter()
            .map(|l| {
                Span::new(
                    l.span.start.saturating_add_signed(-delta),
                    l.span.end.saturating_add_signed(-delta),
                )
            })
            .collect();
        return Some(Found { br, lines, stale });
    }
    if !stale || doc.blocks.len() != ev.document.blocks.len() {
        return None;
    }
    let i = doc.blocks.iter().position(|x| x.id == b.id)?;
    let old = ev.document.blocks.get(i)?;
    let BlockKind::DisplayMath(om) = &old.kind else {
        return None;
    };
    let br = ev.block(old.id)?;
    let lines = om.ast.lines.iter().map(|l| l.span).collect();
    Some(Found { br, lines, stale })
}

/// The answer of an evaluated `= ?` line as LaTeX, with the text a copy gives.
pub(crate) fn answer_of(
    v: &Answered,
    input: &PageInput<'_>,
    span: Span,
) -> Option<(String, String)> {
    let (text, latex, approx) = match input.show.format_for(span) {
        Some(f) => v.reformat_all(f),
        None => (v.text.clone(), v.latex.clone(), v.decimal_beside()),
    };
    let decimal = matches!(
        v.kind,
        LineKind::Query {
            mode: QueryMode::Approx,
            ..
        }
    ) || v.shows_decimal_only();
    if let (true, Some(a)) = (decimal, &approx) {
        return Some((a.clone(), a.clone()));
    }
    let l = latex?;
    let copy = v
        .plain
        .clone()
        .or(text.clone())
        .unwrap_or_else(|| l.clone());
    match (&text, &approx) {
        (Some(t), Some(a)) if t != a => Some((format!("{l} \\approx {a}"), copy)),
        _ => Some((l, copy)),
    }
}

/// A definition's value as `= value` (or `≈ decimal`), with the text a copy gives.
fn preview_of(v: &Answered) -> Option<(String, String)> {
    let latex = v.latex.as_ref()?;
    let copy = v
        .plain
        .clone()
        .or_else(|| v.text.clone())
        .unwrap_or_else(|| latex.clone());
    match (&v.text, &v.decimal_beside()) {
        (Some(t), Some(a)) if t != a => Some((format!("= {latex} \\approx {a}"), copy)),
        _ => Some((format!("= {latex}"), copy)),
    }
}

/// The first error inside `span`, short and whole.
pub(crate) fn error_in(br: &BlockResult, span: Span) -> Option<Content> {
    // An error first; with none, a warning (`w isn't defined, so there is no value`)
    // explains why there is no answer.
    let inside = |d: &&mpp_syntax::Diagnostic| span.start <= d.span.start && d.span.end <= span.end;
    let d = br
        .diagnostics
        .iter()
        .filter(inside)
        .find(|d| d.severity == Severity::Error)
        .or_else(|| {
            br.diagnostics
                .iter()
                .filter(inside)
                .find(|d| d.severity == Severity::Warning)
        })?;
    Some(error_content(d))
}

/// A diagnostic as slot content: a short form for the slot and the whole message.
fn error_content(d: &mpp_syntax::Diagnostic) -> Content {
    let full = mpp_core::note::plain(&d.message).replace('`', "");
    let first = full.lines().next().unwrap_or("").trim();
    let short = if first.chars().count() > 44 {
        format!("{}…", first.chars().take(43).collect::<String>().trim_end())
    } else {
        first.to_string()
    };
    Content::Error { short, full }
}

/// What the hover over a check says about checks against numbers.
pub const CHECK_RULE: &str = "\\text{A check against a number passes when the value rounds to it as written: 35.31 passes R = 35.3 (within half a unit of the last digit). Write ≈ or approx for a looser check, to 3 significant figures.}";

/// The slots of a display maths block, line by line.
pub(crate) fn slots(input: &PageInput<'_>, b: &Block, ast: &MathAst) -> Vec<Slot> {
    let run = input
        .eval
        .and_then(|ev| found(ev, input.src, input.doc, b, ast));
    let mut out = vec![];
    for (i, line) in ast.lines.iter().enumerate() {
        let open = is_slot_line(line, input.src);
        let Some(run) = &run else {
            if open {
                out.push(Slot {
                    line: i,
                    open,
                    content: Content::Empty,
                    stale: false,
                    result: None,
                });
            }
            continue;
        };
        let Some(span) = run.lines.get(i).copied() else {
            continue;
        };
        let vals: Vec<&Answered> = run
            .br
            .values
            .iter()
            .filter(|v| span.start <= v.span.start && v.span.end <= span.end)
            .collect();
        let open = open && !vals.iter().any(|v| rich(v));
        // A name defined nowhere: say so, never echo the question as its answer.
        let missing = run.br.diagnostics.iter().find(|d| {
            span.start <= d.span.start
                && d.span.end <= span.end
                && d.message.contains("defined, so this cannot be worked out")
        });
        let content = if let (Some(d), false) = (missing, open) {
            error_content(d)
        } else if open {
            missing
                .map(error_content)
                .or_else(|| {
                    vals.iter()
                        .rev()
                        .find(|v| matches!(v.kind, LineKind::Query { .. }))
                        .and_then(|v| answer_of(v, input, line.span))
                        .map(|(latex, copy)| Content::Answer { latex, copy })
                        .or_else(|| error_in(run.br, span))
                })
                .unwrap_or(Content::Empty)
        } else if let Some(v) = vals.iter().find(|v| v.implicit) {
            Content::Defined {
                value: v.shows_value.then(|| preview_of(v)).flatten(),
            }
        } else if let Some(v) = vals.iter().find(|v| {
            v.kind == LineKind::Check
                && matches!(
                    line.statement,
                    Statement::Relation(_) | Statement::Check { .. }
                )
        }) {
            Content::Check {
                holds: v.verdict.as_ref().and_then(mpp_eval::Verdict::passed),
                why: v.verdict_text().unwrap_or_default(),
                left: left_side(line, input.src).zip(v.left.clone()),
            }
        } else if let Some((latex, copy)) = vals
            .iter()
            .find(|v| v.shows_value && matches!(v.kind, LineKind::Definition { .. }))
            .and_then(|v| preview_of(v))
        {
            Content::Preview { latex, copy }
        } else {
            continue;
        };
        let result = matches!(content, Content::Answer { .. }).then(|| Span::point(span.end));
        out.push(Slot {
            line: i,
            open,
            content,
            stale: run.stale,
            result,
        });
    }
    out
}

/// The left side of a check line as written, when it is short enough to repeat
/// beside the check's ✗ (`R`, `|R - 35.3|`).
fn left_side(line: &Line, src: &str) -> Option<String> {
    let span = match &line.statement {
        Statement::Check { lhs, .. } => lhs.span,
        Statement::Relation(n) => match &n.kind {
            NodeKind::Relation { first, .. } => first.span,
            _ => return None,
        },
        _ => return None,
    };
    let text = src.get(span.start..span.end)?.trim();
    (!text.is_empty() && text.chars().count() <= 24).then(|| text.to_string())
}

/// True when the line `index` of the block has its answer drawn in a slot (so the
/// formula itself gets no result for it).
pub(crate) fn takes(slots: &[Slot], index: usize) -> bool {
    slots.iter().any(|s| s.line == index)
}

// ---- Drawing -----------------------------------------------------------------------

/// Typeset `latex` in one colour at `size`.
fn typeset(latex: &str, size: f32, color: Color, theme: Theme) -> DisplayList {
    let opts = LayoutOptions {
        theme: Theme {
            text: color,
            ..theme
        },
        ..LayoutOptions::inline(size)
    };
    layout_latex(latex, &opts)
}

/// Typeset `latex`, made smaller (down to 60 %) when it is wider than `room`.
fn fitted(latex: &str, size: f32, color: Color, theme: Theme, room: f32) -> DisplayList {
    let dl = typeset(latex, size, color, theme);
    if dl.width <= room || dl.width <= 0.0 {
        return dl;
    }
    let scale = (room / dl.width).clamp(0.6, 1.0);
    typeset(latex, size * scale, color, theme)
}

/// A ⚠ sign: a triangle with an `!`, `h` high with its top-left at `(x, y)`.
fn warning_sign(x: f32, y: f32, h: f32, color: Color, out: &mut Vec<Primitive>) {
    let w = h * 1.1;
    let stroke = (h * 0.09).max(1.0);
    out.push(Primitive::Path {
        cmds: vec![
            PathCmd::MoveTo(x + w / 2.0, y),
            PathCmd::LineTo(x + w, y + h),
            PathCmd::LineTo(x, y + h),
            PathCmd::Close,
        ],
        color,
        stroke: Some(stroke),
    });
    let bar = mpp_render::Rect {
        x: x + w / 2.0 - stroke / 2.0,
        y: y + h * 0.32,
        w: stroke,
        h: h * 0.36,
    };
    out.push(Primitive::Rule { rect: bar, color });
    let dot = mpp_render::Rect {
        x: bar.x,
        y: y + h * 0.76,
        w: stroke,
        h: stroke,
    };
    out.push(Primitive::Rule { rect: dot, color });
}

/// A ✗ drawn as two strokes, `size` high on a baseline: never an operator between
/// empty boxes (a typeset `\times` has nothing on either side).
fn cross(size: f32, color: Color) -> DisplayList {
    let h = size * 0.62;
    let stroke = (size * 0.09).max(1.2);
    let line = |x0: f32, y0: f32, x1: f32, y1: f32| Primitive::Path {
        cmds: vec![PathCmd::MoveTo(x0, y0), PathCmd::LineTo(x1, y1)],
        color,
        stroke: Some(stroke),
    };
    let pad = size * 0.08;
    DisplayList {
        width: h + 2.0 * pad,
        height: h + pad,
        depth: 0.0,
        baseline: h + pad,
        font_size: size,
        items: vec![
            line(pad, pad, pad + h, pad + h),
            line(pad + h, pad, pad, pad + h),
        ],
        hits: vec![],
    }
}

/// What one slot draws, laid out with its top-left at the origin.
struct Chip {
    items: Vec<Primitive>,
    width: f32,
    /// Height above the baseline, and below it.
    height: f32,
    depth: f32,
    /// What a click copies, and what hovering shows (LaTeX lines).
    copy: Option<String>,
    tip: Option<String>,
    /// Hit like a result in the formula: the end of its line, and how far left of
    /// the chip the hit reaches (over the gap after the `=`).
    result: Option<(Span, f32)>,
}

impl Chip {
    fn from(dl: DisplayList) -> Self {
        Chip {
            width: dl.width,
            height: dl.height,
            depth: dl.depth,
            items: dl.items,
            copy: None,
            tip: None,
            result: None,
        }
    }

    /// Puts `other` after this chip, `gap` apart, on the same baseline.
    fn then(mut self, other: DisplayList, gap: f32) -> Self {
        let dx = self.width + gap;
        let dy = self.height - other.height;
        let (dy_self, dy_other) = if dy >= 0.0 { (0.0, dy) } else { (-dy, 0.0) };
        if dy_self > 0.0 {
            self.items = self
                .items
                .iter()
                .map(|p| super::shift_primitive(p, 0.0, dy_self))
                .collect();
        }
        self.items.extend(
            other
                .items
                .iter()
                .map(|p| super::shift_primitive(p, dx, dy_other)),
        );
        self.width = dx + other.width;
        self.height = self.height.max(other.height);
        self.depth = self.depth.max(other.depth);
        self
    }
}

/// What the hover over a definition's grey `= value` says.
pub const DEFINED_HERE: &str =
    "\\text{defined here: the value in grey after the line is worked out from the lines above}";

/// The chip of a slot, at most `room` wide.
fn chip(slot: &Slot, size: f32, theme: Theme, room: f32) -> Option<Chip> {
    let ac = colors(&theme);
    let ink = |c: Color| if slot.stale { ac.stale } else { c };
    let hint = "\\text{click to copy, Ctrl+click to insert}";
    let chip = match &slot.content {
        Content::Empty => {
            // A run is going for a line with no answer yet: a quiet "working" mark.
            if !slot.stale {
                return None;
            }
            Chip::from(typeset("\\cdots", size, ac.stale, theme))
        }
        Content::Answer { latex, copy } => {
            let mut c = Chip::from(fitted(latex, size, ink(ac.answer), theme, room));
            c.copy = Some(copy.clone());
            c.result = slot.result.map(|r| (r, size * 0.35));
            c.tip = Some(format!("{latex}\n{hint}"));
            c
        }
        Content::Preview { latex, copy } => {
            let mut c = Chip::from(fitted(latex, size * 0.92, ink(ac.preview), theme, room));
            c.copy = Some(copy.clone());
            c.tip = Some(format!("{latex}\n{DEFINED_HERE}\n{hint}"));
            c
        }
        Content::Defined { value } => {
            let mark = typeset("\\text{defined}", size * 0.7, ink(ac.mark), theme);
            let tip = "\\text{read as a definition (the name was not defined yet);}\n\\text{the same name with = and a value on a later line checks it}";
            match value {
                Some((latex, copy)) => {
                    let room = (room - mark.width - size * 0.4).max(size);
                    let v = fitted(latex, size * 0.92, ink(ac.preview), theme, room);
                    let mut c = Chip::from(v).then(mark, size * 0.4);
                    c.copy = Some(copy.clone());
                    c.tip = Some(format!("{latex}\n{DEFINED_HERE}\n{tip}\n{hint}"));
                    c
                }
                None => {
                    let mut c = Chip::from(mark);
                    c.tip = Some(tip.to_string());
                    c
                }
            }
        }
        Content::Check { holds, why, left } => {
            let color = ink(match holds {
                Some(true) => ac.holds,
                Some(false) => ac.fails,
                None => ac.mark,
            });
            let mark = match holds {
                Some(true) => typeset("\\checkmark", size, color, theme),
                Some(false) => cross(size, color),
                None => typeset("?", size, color, theme),
            };
            let mut c = Chip::from(mark);
            // A failed check says what the value really is: ✗ R = 35.31194307.
            if let (Some(false), Some((name, value))) = (holds, left) {
                let shown = format!("{name} = {value}");
                let room = (room - c.width - size * 0.3).max(size);
                let v = fitted(&shown, size * 0.85, color, theme, room);
                c = c.then(v, size * 0.3);
                c.copy = Some(value.clone());
            }
            let mut tip = vec![];
            if !why.is_empty() {
                tip.push(prose_latex(&format!("check: {why}")));
            }
            tip.push(CHECK_RULE.to_string());
            c.tip = Some(tip.join("\n"));
            c
        }
        Content::Error { short, full } => {
            let color = ink(ac.error);
            let h = size * 0.72;
            let words = fitted(
                &prose_latex(short),
                size * 0.85,
                color,
                theme,
                (room - h * 1.5).max(size),
            );
            let mut items = vec![];
            let top = words.height - h * 0.95;
            warning_sign(0.0, top.max(0.0), h, color, &mut items);
            let dx = h * 1.1 + size * 0.25;
            items.extend(
                words
                    .items
                    .iter()
                    .map(|p| super::shift_primitive(p, dx, 0.0)),
            );
            Chip {
                items,
                width: dx + words.width,
                height: words.height.max(h),
                depth: words.depth,
                copy: Some(full.clone()),
                tip: Some(prose_latex(full)),
                result: None,
            }
        }
    };
    Some(chip)
}

/// Draws `chip` with its baseline at `(x, baseline)` and makes it clickable.
fn put(chip: Chip, x: f32, baseline: f32, c: &mut Canvas) {
    let top = baseline - chip.height;
    c.items
        .extend(chip.items.iter().map(|p| super::shift_primitive(p, x, top)));
    if chip.copy.is_some() || chip.tip.is_some() {
        let rect = Rect::from_min_size(
            Pos2::new(x, top),
            Vec2::new(chip.width.max(4.0), (chip.height + chip.depth).max(4.0)),
        );
        c.links.push(PageLink {
            rect,
            link: Link {
                target: LinkTarget::Answer(chip.copy.unwrap_or_default()),
                tip: chip.tip,
            },
        });
    }
    if let Some((span, left)) = chip.result {
        c.hits.push(mpp_render::HitBox {
            rect: mpp_render::Rect {
                x: x - left,
                y: top,
                w: chip.width.max(4.0) + left,
                h: (chip.height + chip.depth).max(4.0),
            },
            baseline,
            span,
            path: mpp_render::NodePath(vec![]),
            kind: HitKind::Result,
            level: 50,
            label: None,
        });
    }
}

/// The box of the first `=` (or `≔`) on the row with this baseline, left of `end`.
fn relation_at(dl: &DisplayList, baseline: f32, end: f32, size: f32) -> Option<Rect> {
    dl.items.iter().find_map(|p| match p {
        mpp_render::Primitive::Glyph {
            ch: '=' | '≔',
            x,
            y,
            ..
        } if (*y - baseline).abs() < size * 0.2 && *x < end => Some(Rect::from_min_max(
            Pos2::new(*x - size * 0.1, *y - size * 0.6),
            Pos2::new(*x + size * 0.8, *y + size * 0.2),
        )),
        _ => None,
    })
}

/// Where line `index` of a laid-out block ends: the x after it and its baseline.
fn line_end(dl: &DisplayList, line: &Line, open: bool) -> Option<(f32, f32)> {
    if open {
        if let Some(h) = dl.hits.iter().find(|h| {
            h.kind == HitKind::Result
                && line.span.start <= h.span.start
                && h.span.start <= line.span.end
        }) {
            return Some((h.rect.x, h.baseline));
        }
    }
    dl.hits
        .iter()
        .filter(|h| {
            h.kind != HitKind::Result
                && line.span.start <= h.span.start
                && h.span.end <= line.span.end
        })
        .max_by(|a, b| {
            (a.rect.x + a.rect.w)
                .partial_cmp(&(b.rect.x + b.rect.w))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|h| (h.rect.x + h.rect.w, h.baseline))
}

/// Draws the slots of a display block laid out as `dl` at `(x0, top)` in a column `w`
/// wide. Returns the height of the rows kept under the block for long lines.
#[allow(clippy::too_many_arguments)]
pub(crate) fn place(
    input: &PageInput<'_>,
    ast: &MathAst,
    numbered: bool,
    slots: &[Slot],
    dl: &DisplayList,
    x0: f32,
    top: f32,
    w: f32,
    c: &mut Canvas,
) -> f32 {
    let size = input.size;
    let gap = size * 0.3;
    let right = w - if numbered { size * 2.5 } else { 0.0 };
    let row = size * 1.5;
    let mut below = 0.0;
    for slot in slots {
        let Some(line) = ast.lines.get(slot.line) else {
            continue;
        };
        let Some((end, baseline)) = line_end(dl, line, slot.open) else {
            continue;
        };
        let x = end + gap;
        // A long line ending in `=` keeps a row of its own under the block, so its
        // answer has room and still moves nothing when it arrives.
        if slot.open && x > right * 0.6 {
            let y = dl.total_height() + below + size * 0.25;
            below += row;
            if let Some(ch) = chip(slot, size, input.theme, right - size) {
                let at = (end - ch.width).clamp(0.0, (right - ch.width).max(0.0));
                put(ch, x0 + at, top + y + ch_baseline(size), c);
            }
            continue;
        }
        if let Some(ch) = chip(slot, size, input.theme, (right - x).max(size * 2.0)) {
            // The `=` of a definition explains itself on hover too, as its value
            // and "defined" mark do (S11, E17).
            if matches!(
                slot.content,
                Content::Defined { .. } | Content::Preview { .. }
            ) {
                if let Some(eq) = relation_at(dl, baseline, end, size) {
                    c.links.push(PageLink {
                        rect: eq.translate(Vec2::new(x0, top)),
                        link: Link {
                            target: LinkTarget::Tip,
                            tip: ch.tip.clone(),
                        },
                    });
                }
            }
            put(ch, x0 + x, top + baseline, c);
        }
    }
    below
}

/// The baseline of a row of its own under a block, from the row's top.
fn ch_baseline(size: f32) -> f32 {
    size * 1.05
}

// ---- Inline maths ------------------------------------------------------------------

/// The index of the last line of inline maths when it asks for its answer: it
/// ends in `=` (`$1 + 2 =$`), `= ?` or `≈ ?` (T-N2).
pub(crate) fn inline_open(ast: &MathAst, src: &str) -> Option<usize> {
    let i = ast.lines.len().checked_sub(1)?;
    is_slot_line(&ast.lines[i], src).then_some(i)
}

/// The room kept after inline maths that ends in `=`, for its answer chip.
pub(crate) fn inline_room(size: f32) -> f32 {
    size * 6.0
}

/// Draws the answer chips of the inline maths in a laid-out prose block, in the room
/// kept after each (see [`inline_room`]).
pub(crate) fn inline_chips(input: &PageInput<'_>, b: &Block, c: &mut Canvas) {
    if matches!(b.kind, BlockKind::DisplayMath(_)) {
        return;
    }
    let ends: Vec<(usize, f32, f32, f32)> = c
        .hits
        .iter()
        .filter(|h| h.kind == HitKind::Result)
        .map(|h| (h.span.start, h.rect.x, h.baseline, h.rect.h))
        .collect();
    if ends.is_empty() {
        return;
    }
    let Some(ev) = input.eval else {
        return;
    };
    let Some(br) = ev.block(b.id) else {
        return;
    };
    let stale = ev.source() != input.src;
    let delta = b.span.start as isize - br.span.start as isize;
    let size = input.size;
    for (end, x, baseline, _) in ends {
        let old_end = end.saturating_add_signed(-delta);
        let asks = |v: &&Answered| v.open || matches!(v.kind, LineKind::Query { steps: false, .. });
        let Some(v) = br
            .values
            .iter()
            .filter(asks)
            .find(|v| v.span.end == old_end)
        else {
            continue;
        };
        let span = Span::new(v.span.start.saturating_add_signed(delta), end);
        let content = answer_of(v, input, span)
            .map(|(latex, copy)| Content::Answer { latex, copy })
            .or_else(|| error_in(br, v.span))
            .unwrap_or(Content::Empty);
        let slot = Slot {
            line: 0,
            open: true,
            content,
            stale,
            result: None,
        };
        let room = inline_room(size) - size * 0.3;
        if let Some(ch) = chip(&slot, size * 0.95, input.theme, room) {
            put(ch, x + size * 0.2, baseline, c);
        }
    }
}

// ---- Page fingerprint --------------------------------------------------------------

/// Adds what the slots show to the page's evaluation fingerprint (see
/// [`super::build::eval_view`]): their answers, and whether the run is of the text on
/// screen, so a dimmed answer is drawn again when its run is done.
pub(crate) fn view_hash(doc: &Document, ev: &Evaluated, h: &mut impl std::hash::Hasher) {
    use std::hash::Hash;
    let mut any = false;
    doc.walk(&mut |b| {
        let Some(br) = ev.block(b.id) else {
            return;
        };
        for v in &br.values {
            if v.open || v.shows_value || v.implicit {
                any = true;
                b.id.hash(h);
                (
                    v.span.start.wrapping_sub(br.span.start),
                    v.open,
                    v.shows_value,
                    v.implicit,
                )
                    .hash(h);
                (&v.text, &v.latex, &v.approx).hash(h);
            }
        }
    });
    if any {
        ev.source().hash(h);
    }
}

// ---- Clicks ------------------------------------------------------------------------

/// The key under which the last answer clicked is kept in egui's memory (for tests
/// and the status line).
pub const LAST_ANSWER: &str = "mpp-last-answer";

/// A click on an answer: copies it, or with Ctrl inserts it at the caret.
pub fn click(ctx: &egui::Context, doc: &mut crate::doc::Doc, text: &str, insert: bool) {
    if text.is_empty() {
        return;
    }
    if insert {
        doc.editor.insert_text(text);
        doc.touched();
    } else {
        ctx.copy_text(text.to_string());
    }
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique(LAST_ANSWER), text.to_string()));
}

/// Where a name was defined, for its hover card: `defined on line 3`.
pub fn defined_where(src: &str, span: Span) -> String {
    let line = src
        .get(..span.start.min(src.len()))
        .map_or(0, |s| s.matches('\n').count())
        + 1;
    format!("defined on line {line} (Ctrl+click to go there)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::{build_page, ExtraRefs, Page, PageInput, ShowState};
    use mpp_eval::Session;
    use mpp_render::IntegralLimits;
    use mpp_syntax::document::parse_document;

    fn page_with(src: &str, eval: Option<&Evaluated>) -> Page {
        let doc = parse_document(src);
        build_page(&PageInput {
            src,
            doc: &doc,
            eval,
            theme: Theme::LIGHT,
            size: 18.0,
            width: 900.0,
            integral_limits: IntegralLimits::AboveBelow,
            slot_labels: &[],
            show: &ShowState::default(),
            extra_refs: &ExtraRefs::default(),
            proof: &crate::proof::ProofView::default(),
        })
    }

    fn run(src: &str) -> Evaluated {
        Session::new().run_str(src).clone()
    }

    /// Every answer link on the page: what a click copies, and the hover text.
    fn answers(page: &Page) -> Vec<(String, String)> {
        page.links
            .iter()
            .filter_map(|l| match &l.link.target {
                LinkTarget::Answer(t) => Some((t.clone(), l.link.tip.clone().unwrap_or_default())),
                _ => None,
            })
            .collect()
    }

    fn tops(page: &Page) -> Vec<(f32, f32)> {
        page.blocks.iter().map(|b| (b.top, b.height)).collect()
    }

    fn has_color(page: &Page, block: usize, want: Color, path: bool) -> bool {
        page.blocks[block].dl.items.iter().any(|p| match p {
            Primitive::Glyph { color, .. } => !path && *color == want,
            Primitive::Path { color, .. } => path && *color == want,
            _ => false,
        })
    }

    const VIDEO: &str =
        "# Test\n\n$$ x := 5 $$\n\n$$ E := 2x + 5 $$\n\n$$ 1.321321312 + 5 = $$\n\nAfter.\n";

    #[test]
    fn bens_lines_show_their_answers_and_nothing_moves_when_they_arrive() {
        let ev = run(VIDEO);
        let before = page_with(VIDEO, None);
        let after = page_with(VIDEO, Some(&ev));
        let got = answers(&after);
        assert!(
            got.iter().any(|(c, t)| c == "15" && t.contains("= 15")),
            "{got:?}"
        );
        assert!(got.iter().any(|(c, _)| c == "6.321321312"), "{got:?}");
        // `x := 5` is a plain number: no grey value after it.
        assert_eq!(got.len(), 2, "{got:?}");
        assert_eq!(
            tops(&before),
            tops(&after),
            "a line moved when the answers arrived"
        );
        // The answer is in its own colour, and never a place for the caret.
        assert!(has_color(&after, 3, colors(&Theme::LIGHT).answer, false));
        let end = VIDEO.find(" = $$").expect("the open line") + 2;
        assert!(after
            .stops
            .iter()
            .all(|s| s.offset <= end || s.offset > end + 4));
    }

    #[test]
    fn a_long_line_keeps_its_answer_row_before_the_answer_comes() {
        let sum = vec!["1"; 60].join(" + ");
        let long = format!("$$ {sum} = $$\n\nAfter.\n");
        let ev = run(&long);
        assert_eq!(
            tops(&page_with(&long, None)),
            tops(&page_with(&long, Some(&ev)))
        );
        let got = answers(&page_with(&long, Some(&ev)));
        assert!(got.iter().any(|(c, _)| c == "60"), "{got:?}");
        // The row is kept under the line: taller than the same line asking nothing.
        let plain = format!("$$ {sum} $$\n\nAfter.\n");
        assert!(page_with(&long, None).blocks[0].height > page_with(&plain, None).blocks[0].height);
    }

    #[test]
    fn an_old_answer_stays_dimmed_while_the_new_run_goes() {
        let old = "$$ 1.321321312 + 5 = $$\n";
        let new = "$$ 1.321321312 + 50 = $$\n";
        let page = page_with(new, Some(&run(old)));
        assert!(answers(&page).iter().any(|(c, _)| c == "6.321321312"));
        let ac = colors(&Theme::LIGHT);
        assert!(has_color(&page, 0, ac.stale, false), "not dimmed");
        assert!(!has_color(&page, 0, ac.answer, false));
        // Once the run is of the text on screen, it is drawn in the answer colour.
        let fresh = page_with(new, Some(&run(new)));
        assert!(answers(&fresh).iter().any(|(c, _)| c == "51.32132131"));
        assert!(has_color(&fresh, 0, ac.answer, false));
        assert_eq!(tops(&page), tops(&fresh));
    }

    #[test]
    fn an_error_goes_in_the_same_slot_with_its_message_on_hover() {
        let src = r"$$ 5 \text{ m} + 2 \text{ s} = $$";
        let bad = page_with(src, Some(&run(src)));
        let before = page_with(src, None);
        let got = answers(&bad);
        assert_eq!(got.len(), 1, "{got:?}");
        assert!(got[0].0.contains("can't add"), "{got:?}");
        assert!(got[0].1.contains("add"), "{got:?}");
        // The ⚠ sign, in red.
        assert!(has_color(&bad, 0, Theme::LIGHT.error, true));
        // No extra line for the error.
        assert_eq!(tops(&bad), tops(&before));
    }

    #[test]
    fn inline_maths_ending_in_equals_gets_a_chip() {
        let src = "The sum $2 + 3 =$ is small.\n";
        let got = answers(&page_with(src, Some(&run(src))));
        let copies: Vec<&str> = got.iter().map(|(c, _)| c.as_str()).collect();
        assert_eq!(copies, ["5"]);
        // The room is kept before the answer comes: nothing after it moves.
        assert_eq!(
            tops(&page_with(src, None)),
            tops(&page_with(src, Some(&run(src))))
        );
    }

    /// T-N2, P-N3: inline maths in a sentence asking with `= ?` (or `≈ ?`) is
    /// answered as `=` is.
    #[test]
    fn inline_maths_asking_with_a_question_mark_gets_a_chip() {
        for (src, want) in [
            ("$$ v = 20 $$\n\nHalf is $v/2 = ?$ m/s.\n", "10"),
            ("$$ v = 20 $$\n\nHalf is $\\frac{v}{2} = ?$ m/s.\n", "10"),
            ("The sum $2 + 3 = ?$ is small.\n", "5"),
            ("Then $1/4 \\approx ?$ is small.\n", "0.25"),
        ] {
            let got = answers(&page_with(src, Some(&run(src))));
            assert!(got.iter().any(|(c, _)| c == want), "{src}: {got:?}");
            assert_eq!(
                tops(&page_with(src, None)),
                tops(&page_with(src, Some(&run(src)))),
                "{src}"
            );
        }
    }

    #[test]
    fn x_equals_5_shows_defined_or_its_check() {
        let src = "$$ y = 7 $$\n\n$$ x := 5 $$\n\n$$ x = 6 $$\n\n$$ x = 5 $$\n";
        let got = answers(&page_with(src, Some(&run(src))));
        let tips: Vec<&str> = got.iter().map(|(_, t)| t.as_str()).collect();
        assert_eq!(tips.len(), 3, "{tips:?}");
        assert!(tips[0].contains("definition"), "{tips:?}");
        assert!(
            tips[1].contains("check") && tips[1].contains('✗'),
            "{tips:?}"
        );
        assert!(tips[2].contains('✓'), "{tips:?}");
    }

    #[test]
    fn a_trailing_equals_asks_but_definitions_and_checks_do_not() {
        assert!(ends_open("1.321321312 + 5 ="));
        assert!(ends_open("x + 1 =  "));
        assert!(!ends_open("x = ?"));
        assert!(!ends_open("x :="));
        assert!(!ends_open("x =="));
        assert!(!ends_open("x = 5"));
    }
}
