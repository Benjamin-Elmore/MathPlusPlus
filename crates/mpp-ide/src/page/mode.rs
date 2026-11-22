//! Maths or prose, on the page: every maths block sits in an outlined, tinted
//! box with a ∑ marker in the left margin (a run of maths lines shares one box),
//! inline `$…$` maths gets a tinted chip, and the caret takes the maths colour
//! inside maths. Prose stays plain. All colours come from the theme.

use egui::{Color32, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2};
use mpp_render::FontId;

use super::{BlockTag, Page};
use crate::editor::mathline::LineMark;
use crate::theme::{c32, Palette};

/// One maths box: a run of maths blocks with no blank line between them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    /// The box, in page coordinates.
    pub rect: Rect,
    /// Where the ∑ marker is drawn and clicked (page coordinates, in the margin).
    pub marker: Rect,
    /// An offset inside the first block (a click on the marker flips that line).
    pub at: usize,
    /// From the start of the first block to the end of the last.
    pub start: usize,
    pub end: usize,
}

/// How far the box reaches out of the column on each side, in ems.
const OUTSET: f32 = 0.35;

/// The maths boxes of a page.
pub fn bands(page: &Page, size: f32) -> Vec<Band> {
    let mut out: Vec<Band> = Vec::new();
    let mut last_bottom = f32::MIN;
    for b in page.blocks.iter().filter(|b| b.tag == BlockTag::Math) {
        let rect = Rect::from_min_max(
            Pos2::new(-size * OUTSET, b.top),
            Pos2::new(page.width + size * OUTSET, b.top + b.height),
        );
        // The next block straight after this one (no blank line between).
        if let Some(prev) = out.last_mut() {
            if b.top - last_bottom < size * 1.2 && b.span.start >= prev.end {
                prev.rect = prev.rect.union(rect);
                prev.end = b.span.end;
                last_bottom = b.top + b.height;
                continue;
            }
        }
        last_bottom = b.top + b.height;
        let mid = first_line_middle(page, b.span.start, b.span.end).unwrap_or(b.top + size);
        let r = size * 0.55;
        let marker = Rect::from_center_size(
            Pos2::new(-size * (OUTSET + 0.8), mid),
            Vec2::new(2.0 * r, 2.0 * r),
        );
        out.push(Band {
            rect,
            marker,
            at: (b.span.start + 2).min(b.span.end),
            start: b.span.start,
            end: b.span.end,
        });
    }
    out
}

/// The middle of the first line of caret stops in `start..=end`.
fn first_line_middle(page: &Page, start: usize, end: usize) -> Option<f32> {
    let from = page.stops.partition_point(|s| s.offset < start);
    page.stops[from..]
        .iter()
        .take_while(|s| s.offset <= end)
        .map(|s| s.top)
        .fold(None, |m: Option<f32>, t| Some(m.map_or(t, |m| m.min(t))))
        .and_then(|top| {
            page.stops[from..]
                .iter()
                .take_while(|s| s.offset <= end)
                .find(|s| (s.top - top).abs() < 1.0)
                .map(|s| (s.top + s.bottom) / 2.0)
        })
}

/// The maths box whose ∑ marker is under a page position (with a little slack).
pub fn marker_at(page: &Page, p: Pos2, size: f32) -> Option<Band> {
    bands(page, size)
        .into_iter()
        .find(|b| b.marker.expand(size * 0.2).contains(p))
}

/// True when the caret at `offset` is in maths (a maths block or inline maths).
pub fn caret_in_maths(page: &Page, offset: usize) -> bool {
    page.in_math(offset)
}

/// The caret's colour: the maths colour inside maths, the ink colour in prose.
pub fn caret_color(page: &Page, offset: usize, pal: &Palette) -> Color32 {
    if caret_in_maths(page, offset) {
        c32(pal.maths_caret)
    } else {
        c32(pal.theme.text)
    }
}

/// Paints the maths boxes, their markers and the inline chips (under the text).
/// The box holding the caret gets a slightly stronger outline.
pub fn paint_maths(
    painter: &Painter,
    page: &Page,
    origin: Pos2,
    pal: &Palette,
    size: f32,
    caret: Option<usize>,
) {
    let clip = painter.clip_rect();
    let off = origin.to_vec2();
    let tint = c32(pal.maths_tint);
    for r in &page.chips {
        let r = r.translate(off).expand2(Vec2::new(2.0, 1.0));
        if r.intersects(clip) {
            painter.rect_filled(r, 3.0, tint);
        }
    }
    let outline = c32(pal.maths_outline);
    for band in bands(page, size) {
        let rect = band.rect.translate(off);
        if !rect.intersects(clip) {
            continue;
        }
        let here = caret.is_some_and(|c| band.start <= c && c <= band.end);
        let width = if here { 1.6 } else { 1.0 };
        painter.rect(
            rect,
            size * 0.3,
            tint,
            Stroke::new(width, outline),
            StrokeKind::Inside,
        );
        let m = band.marker.translate(off);
        let glyph = size * 0.95;
        let w = super::text_width(FontId::Math, "\u{2211}") * glyph;
        let x = (m.center().x - w / 2.0).max(clip.left() + 1.0);
        crate::paint::paint_text(
            painter,
            "\u{2211}",
            Pos2::new(x, m.center().y + glyph * 0.3),
            glyph,
            FontId::Math,
            c32(pal.maths_marker),
        );
    }
}

/// The Insert catalogue's id of "Maths line".
pub const MATHS_LINE: &str = "text.mathsline";

/// After a frame's input: auto-detect on the line the caret was on at the start
/// of the frame (see [`crate::editor::Editor::maths_after_frame`]). True when
/// the line became maths.
pub fn auto_detect(ui: &egui::Ui, doc: &mut crate::doc::Doc, was: LineMark) -> bool {
    let enter = ui.input(|i| {
        i.events.iter().any(|e| {
            matches!(e, egui::Event::Key { key: egui::Key::Enter, pressed: true, modifiers, .. }
                if !modifiers.shift && !modifiers.command)
        })
    });
    let done = doc.editor.maths_after_frame(&mut doc.maths, was, enter);
    if done {
        doc.touched();
    }
    done
}

/// The hint on the empty line of an empty file.
pub const EMPTY_HINT: &str = "Type text, or start with = or $ for maths";

/// Paints [`EMPTY_HINT`] beside the caret while the file is empty.
pub fn paint_empty_hint(
    painter: &Painter,
    doc: &crate::doc::Doc,
    page: &Page,
    origin: Pos2,
    theme: &mpp_render::Theme,
    size: f32,
) {
    if !doc.editor.text().trim().is_empty() {
        return;
    }
    let Some(s) = page.caret_stop(doc.editor.caret()) else {
        return;
    };
    let at = Pos2::new(
        origin.x + s.x + size * 0.3,
        origin.y + s.bottom - size * 0.3,
    );
    crate::paint::paint_text(
        painter,
        EMPTY_HINT,
        at,
        size * 0.9,
        FontId::TextItalic,
        c32(theme.placeholder_label),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::{build_page, ExtraRefs, PageInput, ShowState};
    use crate::theme::Mode;
    use mpp_syntax::document::parse_document;

    fn page_of(src: &str) -> Page {
        let doc = parse_document(src);
        let show = ShowState::default();
        let refs = ExtraRefs::default();
        let proof = crate::proof::ProofView::default();
        build_page(&PageInput {
            src,
            doc: &doc,
            eval: None,
            theme: Mode::Light.theme(),
            size: 16.0,
            width: 700.0,
            integral_limits: mpp_render::IntegralLimits::AboveBelow,
            slot_labels: &[],
            show: &show,
            extra_refs: &refs,
            proof: &proof,
        })
    }

    #[test]
    fn every_maths_block_gets_a_box_and_prose_none() {
        let p = page_of("Some words.\n\n$$ x := 5 $$\n\nMore words.\n\n$$ $$\n");
        let b = bands(&p, 16.0);
        assert_eq!(b.len(), 2, "two maths boxes, the empty one too");
        assert!(b[0].marker.right() < 0.0, "the marker is in the margin");
        assert!(b[0].rect.left() < 0.0 && b[0].rect.right() > p.width);
    }

    #[test]
    fn inline_maths_gets_a_chip() {
        let p = page_of("Let $x$ be the speed and $y + 1$ the rest.\n");
        assert_eq!(p.chips.len(), 2);
        assert!(bands(&p, 16.0).is_empty());
    }

    #[test]
    fn a_click_on_the_marker_finds_its_box() {
        let p = page_of("$$ x := 5 $$\n");
        let b = bands(&p, 16.0)[0];
        let hit = marker_at(&p, b.marker.center(), 16.0).expect("marker");
        assert_eq!(hit.start, 0);
        assert!(marker_at(&p, Pos2::new(100.0, b.marker.center().y), 16.0).is_none());
    }
}
