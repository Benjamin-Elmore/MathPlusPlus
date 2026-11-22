//! Accessibility: a spoken form of every formula on the page, handed to egui's
//! AccessKit so a screen reader (NVDA, Narrator) can read it. The page is
//! painted, not made of widgets, so each formula gets one invisible node over
//! its box, labelled with its spoken text and what it shows ("R equals
//! 35.31194307", "check fails, …"), and the page itself is a text control
//! ([`editor`]). Live announcements, F6 and "where am I" are in [`live`].

pub mod answers;
pub mod editor;
pub mod live;
pub mod speech;

use egui::{accesskit, Id, Pos2, Rect, Role, Sense, Ui, WidgetInfo};

use crate::doc::{Doc, PageCache};
use crate::page::BlockTag;

/// What `$$ … $$` or `$ … $` holds, and where it starts in the file.
fn inner(src: &str, at: usize) -> (&str, usize) {
    let lead = src.len() - src.trim_start_matches('$').len();
    let body = src.trim_matches('$');
    (body, at + lead)
}

/// The spoken form of the maths at `span` of the file text.
pub fn spoken_at(text: &str, span: mpp_syntax::Span) -> Option<String> {
    let src = text.get(span.start..span.end)?;
    let (body, base) = inner(src, span.start);
    let said = speech::speak_math(body, base);
    (!said.is_empty()).then_some(said)
}

/// The label of the ∑ marker's button.
pub const MARKER_LABEL: &str = "Maths line: switch to prose";

/// Where the frame notes ∑ markers a screen reader clicked (their offsets).
const MARKER_CLICKS: &str = "mpp-access-marker-clicks";

/// Offsets of the maths lines whose ∑ marker a screen reader clicked this
/// frame (the app switches them to prose).
pub fn take_marker_clicks(ctx: &egui::Context) -> Vec<usize> {
    ctx.data_mut(|d| d.remove_temp::<Vec<usize>>(Id::unique(MARKER_CLICKS)))
        .unwrap_or_default()
}

/// Gives every formula that is on screen a screen-reader node: a maths block
/// over its box, and an inline `$…$` over its chip; and every ∑ marker a
/// button. `origin` is where the page starts on screen.
pub fn annotate(ui: &Ui, doc: &Doc, pc: &PageCache, origin: Pos2, size: f32) {
    let page = &pc.page;
    let text = doc.editor.text();
    let clip = ui.clip_rect();
    let off = origin.to_vec2();
    for b in page.blocks.iter().filter(|b| b.tag == BlockTag::Math) {
        let rect = Rect::from_min_max(
            Pos2::new(0.0, b.top),
            Pos2::new(page.width, b.top + b.height),
        )
        .translate(off);
        if rect.intersects(clip) {
            let said =
                answers::block_label(doc, pc, b.span.start).or_else(|| spoken_at(text, b.span));
            node(ui, rect, b.span, said);
        }
    }
    for span in &page.math_ranges {
        if page.blocks.iter().any(|b| {
            b.tag == BlockTag::Math && b.span.start <= span.start && span.end <= b.span.end
        }) {
            continue;
        }
        let Some(rect) = page.rect_of_span(*span) else {
            continue;
        };
        let rect = rect.translate(off);
        if rect.intersects(clip) {
            node(ui, rect, *span, answers::inline_label(doc, pc, *span));
        }
    }
    for band in crate::page::mode::bands(page, size) {
        let rect = band.marker.translate(off);
        if !rect.intersects(clip) {
            continue;
        }
        // A hover-only widget: the page keeps the pointer (a click on the
        // marker is the page's own), the screen reader gets a button inside
        // the page, beside the maths it switches (R17).
        let id = ui.scope_id().with(("maths-marker", band.start));
        let _ = ui.interact(rect, id, Sense::hover());
        ui.ctx().accesskit_node_builder(id, |n| {
            n.set_role(Role::Button);
            n.set_label(MARKER_LABEL);
            n.set_bounds(accesskit::Rect {
                x0: rect.min.x.into(),
                y0: rect.min.y.into(),
                x1: rect.max.x.into(),
                y1: rect.max.y.into(),
            });
            n.add_action(accesskit::Action::Click);
        });
        let clicked = ui.input(|i| {
            i.accesskit_action_requests(id, accesskit::Action::Click)
                .next()
                .is_some()
        });
        if clicked {
            ui.ctx().data_mut(|d| {
                d.get_temp_mut_or_default::<Vec<usize>>(Id::unique(MARKER_CLICKS))
                    .push(band.at);
            });
        }
    }
}

fn node(ui: &Ui, rect: Rect, span: mpp_syntax::Span, said: Option<String>) {
    let Some(said) = said else {
        return;
    };
    let id = ui.scope_id().with(("formula", span.start, span.end));
    let r = ui.interact(rect, id, Sense::hover());
    r.widget_info(|| WidgetInfo::labeled(Role::Math, true, said.clone()));
}
