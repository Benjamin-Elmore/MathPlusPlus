//! The typeset page: a document laid out as a column of blocks.
//!
//! A [`Page`] is rebuilt from the current text whenever the text, the width, the
//! theme or the evaluation result changes. Every block carries a display list
//! (glyphs, rules, hit boxes) in block-local pixels, and the page keeps one sorted
//! list of caret stops mapping screen positions to source offsets. Prose stops sit
//! between visible characters, so hidden markup (`#`, `*`, `$$`) is never a stop.

mod build;
mod flow;
mod mathcache;
pub mod paper;

use egui::{Pos2, Rect, Vec2};
use mpp_eval::NumberFormat;
use mpp_render::{CaretStop, DisplayList, HitBox, HitKind, Primitive, Theme};
use mpp_syntax::document::BlockId;
use mpp_syntax::Span;

pub(crate) use build::result_line_at;
pub use build::{build_page, column_width, note_words, scan_inlines, slider_row_height, PageInput};
pub use flow::{display_name, glyph_for, text_width};
pub use paper::{ExtraRefs, PaperInfo};

/// What kind of thing a block is, for the view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BlockTag {
    Prose,
    Math,
    /// A figure: the rectangle (block-local) the figure drawer fills.
    Figure(Rect),
    Rule,
}

/// How results are shown, beyond what the file says: the view's own choices.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShowState {
    /// Slider rows under each figure (a row per `:=` variable with a range).
    pub figure_rows: Vec<(BlockId, usize)>,
    /// Figures drawn in 3D (they get a taller box).
    pub figure_3d: Vec<BlockId>,
    /// Number formats picked for single results (right-click), by line span.
    pub formats: Vec<(Span, NumberFormat)>,
    /// Lines whose working is unfolded, by line span.
    pub expanded: Vec<Span>,
    /// Show the working of every query that has it, not only `= ?!` lines.
    pub show_steps: bool,
    /// View → Comments switched off: notes take no space at all.
    pub hide_comments: bool,
    /// Notes folded to one line, by block id (never saved in the file).
    pub folded: Vec<BlockId>,
    /// The caret is in the title block: its empty fields show grey labels
    /// ("Author", "Date") to type into; otherwise they take no space.
    pub editing_title: bool,
}

impl ShowState {
    /// The number format picked for the result of the line at `span`.
    pub fn format_for(&self, span: Span) -> Option<&NumberFormat> {
        self.formats
            .iter()
            .find(|(s, _)| *s == span)
            .map(|(_, f)| f)
    }

    /// Picks a number format for one line.
    pub fn set_format(&mut self, span: Span, fmt: NumberFormat) {
        self.formats.retain(|(s, _)| *s != span);
        self.formats.push((span, fmt));
    }

    /// True when the working of the line at `span` is unfolded.
    pub fn is_expanded(&self, span: Span) -> bool {
        self.expanded.contains(&span)
    }

    /// Folds or unfolds the working of one line.
    pub fn toggle_steps(&mut self, span: Span) {
        if let Some(i) = self.expanded.iter().position(|s| *s == span) {
            self.expanded.remove(i);
        } else {
            self.expanded.push(span);
        }
    }

    /// True when the note with this id is folded to one line.
    pub fn is_folded(&self, id: BlockId) -> bool {
        self.folded.contains(&id)
    }

    /// Folds or unfolds one note.
    pub fn toggle_fold(&mut self, id: BlockId) {
        if let Some(i) = self.folded.iter().position(|b| *b == id) {
            self.folded.remove(i);
        } else {
            self.folded.push(id);
        }
    }

    /// Slider rows under a figure.
    pub fn rows_for(&self, id: BlockId) -> usize {
        self.figure_rows
            .iter()
            .find(|(b, _)| *b == id)
            .map_or(0, |(_, n)| *n)
    }
}

/// A clickable fold line under an equation: unfolds the working of one line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepToggle {
    /// Page coordinates.
    pub rect: Rect,
    /// The span of the line whose working it folds.
    pub line: Span,
}

/// What a click on a link does.
#[derive(Debug, Clone, PartialEq)]
pub enum LinkTarget {
    /// Scroll to and highlight this part of the file (an equation, a reference
    /// entry, a footnote).
    Jump(Span),
    /// Fold or unfold a note.
    Fold(BlockId),
}

/// A clickable part of the page, with what hovering it shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub target: LinkTarget,
    /// Typeset (LaTeX) text of the hover card: the footnote, the reference entry,
    /// the full text of a folded note.
    pub tip: Option<String>,
}

/// A link and the rectangle it covers.
#[derive(Debug, Clone, PartialEq)]
pub struct PageLink {
    /// Page coordinates (block-local while a block is being laid out).
    pub rect: Rect,
    pub link: Link,
}

/// One laid-out block.
#[derive(Debug, Clone)]
pub struct PlacedBlock {
    pub id: BlockId,
    pub span: Span,
    /// Top of the block on the page.
    pub top: f32,
    pub height: f32,
    /// Glyphs and rules, block-local (origin at the column's left edge and `top`).
    pub dl: DisplayList,
    pub tag: BlockTag,
}

/// A laid-out document.
#[derive(Debug, Clone, Default)]
pub struct Page {
    pub width: f32,
    pub height: f32,
    pub blocks: Vec<PlacedBlock>,
    /// Caret stops in page coordinates, sorted by offset then x.
    pub stops: Vec<CaretStop>,
    /// The source range of every visible prose character, sorted.
    pub chars: Vec<Span>,
    /// Source ranges that are maths (edited structurally; live symbol replacement applies).
    pub math_ranges: Vec<Span>,
    pub theme: Option<Theme>,
    /// Fold lines of worked steps, in page coordinates.
    pub step_toggles: Vec<StepToggle>,
    /// Where a spinner turns while an answer is being worked out (page coordinates).
    pub spinners: Vec<Pos2>,
    /// Citations, footnote marks, equation references and note folds.
    pub links: Vec<PageLink>,
}

/// Drawing primitives and boxes being collected for one block.
#[derive(Debug, Clone, Default)]
pub struct Canvas {
    pub items: Vec<Primitive>,
    pub hits: Vec<HitBox>,
    pub stops: Vec<CaretStop>,
    /// Source ranges of the visible prose characters.
    pub chars: Vec<Span>,
    /// Fold lines of worked steps (block-local).
    pub toggles: Vec<StepToggle>,
    /// Spinners for answers still being worked out (block-local).
    pub spinners: Vec<Pos2>,
    /// Links (block-local).
    pub links: Vec<PageLink>,
}

impl Canvas {
    /// Adds a display list, moved by `(dx, dy)`, with its caret stops.
    pub fn merge(&mut self, dl: &DisplayList, dx: f32, dy: f32, with_stops: bool) {
        self.items
            .extend(dl.items.iter().map(|p| shift_primitive(p, dx, dy)));
        for h in &dl.hits {
            let mut h = h.clone();
            h.rect.x += dx;
            h.rect.y += dy;
            h.baseline += dy;
            self.hits.push(h);
        }
        if with_stops {
            for s in dl.caret_stops() {
                self.stops.push(CaretStop {
                    x: s.x + dx,
                    top: s.top + dy,
                    bottom: s.bottom + dy,
                    offset: s.offset,
                });
            }
        }
    }
}

/// A primitive moved by `(dx, dy)`.
pub fn shift_primitive(p: &Primitive, dx: f32, dy: f32) -> Primitive {
    use mpp_render::{PathCmd, Rect as R};
    let mv = |r: &R| R {
        x: r.x + dx,
        y: r.y + dy,
        w: r.w,
        h: r.h,
    };
    match p {
        Primitive::Glyph {
            font,
            glyph,
            ch,
            x,
            y,
            size,
            color,
        } => Primitive::Glyph {
            font: *font,
            glyph: *glyph,
            ch: *ch,
            x: x + dx,
            y: y + dy,
            size: *size,
            color: *color,
        },
        Primitive::Rule { rect, color } => Primitive::Rule {
            rect: mv(rect),
            color: *color,
        },
        Primitive::Outline {
            rect,
            color,
            dotted,
        } => Primitive::Outline {
            rect: mv(rect),
            color: *color,
            dotted: *dotted,
        },
        Primitive::Path {
            cmds,
            color,
            stroke,
        } => Primitive::Path {
            cmds: cmds
                .iter()
                .map(|c| match *c {
                    PathCmd::MoveTo(x, y) => PathCmd::MoveTo(x + dx, y + dy),
                    PathCmd::LineTo(x, y) => PathCmd::LineTo(x + dx, y + dy),
                    PathCmd::QuadTo(a, b, x, y) => PathCmd::QuadTo(a + dx, b + dy, x + dx, y + dy),
                    PathCmd::CubicTo(a, b, c2, d, x, y) => {
                        PathCmd::CubicTo(a + dx, b + dy, c2 + dx, d + dy, x + dx, y + dy)
                    }
                    PathCmd::Close => PathCmd::Close,
                })
                .collect(),
            color: *color,
            stroke: *stroke,
        },
    }
}

fn cmp_f(a: f32, b: f32) -> std::cmp::Ordering {
    a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
}

impl Page {
    /// True when the offset is inside a maths range.
    pub fn in_math(&self, offset: usize) -> bool {
        self.math_ranges
            .iter()
            .any(|r| r.start <= offset && offset <= r.end)
    }

    /// The block whose span holds the offset.
    pub fn block_at(&self, offset: usize) -> Option<&PlacedBlock> {
        self.blocks
            .iter()
            .find(|b| b.span.start <= offset && offset <= b.span.end)
    }

    /// The stop to draw the caret at for a source offset: the one with the
    /// greatest offset not past it (the first stop when there is none).
    pub fn caret_stop(&self, offset: usize) -> Option<CaretStop> {
        let mut best: Option<&CaretStop> = None;
        for s in &self.stops {
            if s.offset <= offset {
                match best {
                    Some(b) if b.offset > s.offset => {}
                    _ => best = Some(s),
                }
            }
        }
        best.or_else(|| self.stops.first()).copied()
    }

    /// The source offset nearest to a page position.
    pub fn offset_at(&self, p: Pos2) -> Option<usize> {
        self.stops
            .iter()
            .map(|s| {
                let dy = if p.y < s.top {
                    s.top - p.y
                } else if p.y > s.bottom {
                    p.y - s.bottom
                } else {
                    0.0
                };
                (dy * 4.0 + (s.x - p.x).abs(), s.offset)
            })
            .min_by(|a, b| cmp_f(a.0, b.0))
            .map(|(_, o)| o)
    }

    /// The offset one visual line above (`up`) or below the caret, at about the
    /// same x.
    pub fn offset_vertical(&self, offset: usize, up: bool, want_x: Option<f32>) -> Option<usize> {
        let cur = self.caret_stop(offset)?;
        let cy = (cur.top + cur.bottom) / 2.0;
        let x = want_x.unwrap_or(cur.x);
        let cand: Vec<&CaretStop> = self
            .stops
            .iter()
            .filter(|s| {
                let y = (s.top + s.bottom) / 2.0;
                if up {
                    y < cy - 3.0
                } else {
                    y > cy + 3.0
                }
            })
            .collect();
        let line_y = if up {
            cand.iter()
                .map(|s| (s.top + s.bottom) / 2.0)
                .fold(f32::MIN, f32::max)
        } else {
            cand.iter()
                .map(|s| (s.top + s.bottom) / 2.0)
                .fold(f32::MAX, f32::min)
        };
        cand.into_iter()
            .filter(|s| ((s.top + s.bottom) / 2.0 - line_y).abs() < 6.0)
            .min_by(|a, b| cmp_f((a.x - x).abs(), (b.x - x).abs()))
            .map(|s| s.offset)
    }

    /// The previous or next offset whose stop is somewhere else on the page
    /// (offsets that differ only by hidden markup share a position and are skipped).
    pub fn offset_step(&self, offset: usize, forward: bool) -> Option<usize> {
        let cur = self.caret_stop(offset);
        let same = |s: &CaretStop| {
            cur.is_some_and(|c| {
                (s.x - c.x).abs() < 0.5 && ((s.top + s.bottom) - (c.top + c.bottom)).abs() < 4.0
            })
        };
        let cands = self.stops.iter().filter(|s| !same(s));
        if forward {
            cands.map(|s| s.offset).filter(|&o| o > offset).min()
        } else {
            cands.map(|s| s.offset).filter(|&o| o < offset).max()
        }
    }

    /// The visible prose character ending at or before `offset`.
    pub fn char_before(&self, offset: usize) -> Option<Span> {
        self.chars
            .iter()
            .filter(|c| c.end <= offset)
            .max_by_key(|c| c.end)
            .copied()
    }

    /// The visible prose character starting at or after `offset`.
    pub fn char_after(&self, offset: usize) -> Option<Span> {
        self.chars
            .iter()
            .filter(|c| c.start >= offset)
            .min_by_key(|c| c.start)
            .copied()
    }

    /// The first and last offsets with a stop.
    pub fn offset_extent(&self) -> Option<(usize, usize)> {
        let lo = self.stops.iter().map(|s| s.offset).min()?;
        let hi = self.stops.iter().map(|s| s.offset).max()?;
        Some((lo, hi))
    }

    /// The first and last stop offsets on the caret's visual line.
    pub fn line_extent(&self, offset: usize) -> Option<(usize, usize)> {
        let cur = self.caret_stop(offset)?;
        let cy = (cur.top + cur.bottom) / 2.0;
        let on_line = |s: &&CaretStop| ((s.top + s.bottom) / 2.0 - cy).abs() < 6.0;
        let first = self
            .stops
            .iter()
            .filter(on_line)
            .min_by(|a, b| cmp_f(a.x, b.x))?;
        let last = self
            .stops
            .iter()
            .filter(on_line)
            .max_by(|a, b| cmp_f(a.x, b.x))?;
        Some((first.offset, last.offset))
    }

    /// `line` (page coordinates) widened to every glyph drawn on the same line by
    /// the blocks overlapping `span`, including words that have no box of their
    /// own (a figure line's "Parametric" heading).
    pub fn line_ink(&self, span: Span, line: Rect) -> Rect {
        let mut out = line;
        for b in &self.blocks {
            if b.span.end < span.start || b.span.start > span.end {
                continue;
            }
            for p in &b.dl.items {
                if let mpp_render::Primitive::Glyph { x, y, size, .. } = *p {
                    let y = y + b.top;
                    if y >= line.top() && y <= line.bottom() + 0.3 * size {
                        let g = Rect::from_min_max(Pos2::new(x, y - 0.7 * size), Pos2::new(x, y));
                        out = out.union(g);
                    }
                }
            }
        }
        out
    }

    /// The page rectangle covered by a source span (union of the boxes inside it),
    /// falling back to the caret position.
    pub fn rect_of_span(&self, span: Span) -> Option<Rect> {
        let mut out: Option<Rect> = None;
        for b in &self.blocks {
            let origin = Vec2::new(0.0, b.top);
            for h in &b.dl.hits {
                if h.kind == HitKind::Result {
                    continue;
                }
                let inside = if span.is_empty() {
                    h.span.start <= span.start
                        && span.start <= h.span.end
                        && h.kind == HitKind::Slot
                } else {
                    h.span.start >= span.start && h.span.end <= span.end && !h.span.is_empty()
                };
                if inside {
                    let r = Rect::from_min_size(
                        Pos2::new(h.rect.x, h.rect.y) + origin,
                        Vec2::new(h.rect.w, h.rect.h),
                    );
                    out = Some(out.map_or(r, |o| o.union(r)));
                }
            }
        }
        if out.is_none() {
            let a = self.caret_stop(span.start)?;
            let b = self.caret_stop(span.end).unwrap_or(a);
            return Some(Rect::from_min_max(
                Pos2::new(a.x.min(b.x), a.top.min(b.top)),
                Pos2::new(a.x.max(b.x).max(a.x + 4.0), a.bottom.max(b.bottom)),
            ));
        }
        out
    }

    /// The link under a page position.
    pub fn link_at(&self, p: Pos2) -> Option<&PageLink> {
        self.links.iter().find(|l| l.rect.expand(1.0).contains(p))
    }

    /// The innermost node box under a page position, with the block it is in.
    pub fn hit_at(&self, p: Pos2) -> Option<(&PlacedBlock, &HitBox)> {
        let b = self
            .blocks
            .iter()
            .find(|b| p.y >= b.top - 2.0 && p.y <= b.top + b.height + 2.0)?;
        let h = b.dl.hit_test(p.x, p.y - b.top)?;
        Some((b, h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stop(x: f32, y: f32, offset: usize) -> CaretStop {
        CaretStop {
            x,
            top: y,
            bottom: y + 10.0,
            offset,
        }
    }

    fn page() -> Page {
        Page {
            stops: vec![
                stop(0.0, 0.0, 0),
                stop(10.0, 0.0, 1),
                stop(20.0, 0.0, 2),
                stop(0.0, 20.0, 3),
                stop(10.0, 20.0, 4),
            ],
            ..Page::default()
        }
    }

    #[test]
    fn click_maps_to_the_nearest_offset() {
        let p = page();
        assert_eq!(p.offset_at(Pos2::new(11.0, 4.0)), Some(1));
        assert_eq!(p.offset_at(Pos2::new(1.0, 22.0)), Some(3));
    }

    #[test]
    fn caret_steps_over_missing_offsets() {
        let mut p = page();
        p.stops.retain(|s| s.offset != 1);
        assert_eq!(p.offset_step(0, true), Some(2));
        assert_eq!(p.offset_step(2, false), Some(0));
        assert_eq!(p.caret_stop(1).map(|s| s.offset), Some(0));
    }

    #[test]
    fn up_and_down_keep_the_column() {
        let p = page();
        assert_eq!(p.offset_vertical(1, false, None), Some(4));
        assert_eq!(p.offset_vertical(4, true, None), Some(1));
        assert_eq!(p.offset_vertical(4, false, None), None);
    }

    #[test]
    fn visible_characters_skip_markup() {
        let mut p = page();
        p.chars = vec![Span::new(0, 1), Span::new(2, 3)];
        assert_eq!(p.char_before(3), Some(Span::new(2, 3)));
        assert_eq!(p.char_before(2), Some(Span::new(0, 1)));
        assert_eq!(p.char_after(1), Some(Span::new(2, 3)));
    }

    #[test]
    fn line_ends() {
        let p = page();
        assert_eq!(p.line_extent(1), Some((0, 2)));
    }
}
