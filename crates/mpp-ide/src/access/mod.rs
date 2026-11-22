//! Accessibility: a spoken form of every formula on the page, handed to egui's
//! AccessKit so a screen reader (NVDA, Narrator) can read it. The page is
//! painted, not made of widgets, so each formula gets one invisible node over
//! its box, labelled with its spoken text ("x squared plus 5").

pub mod speech;

use egui::{Pos2, Rect, Role, Sense, Ui, WidgetInfo};

use crate::page::{BlockTag, Page};

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

/// Gives every formula that is on screen a screen-reader node: a maths block
/// over its box, and an inline `$…$` over its chip. `origin` is where the page
/// starts on screen.
pub fn annotate(ui: &Ui, page: &Page, text: &str, origin: Pos2) {
    let clip = ui.clip_rect();
    let off = origin.to_vec2();
    for b in page.blocks.iter().filter(|b| b.tag == BlockTag::Math) {
        let rect = Rect::from_min_max(
            Pos2::new(0.0, b.top),
            Pos2::new(page.width, b.top + b.height),
        )
        .translate(off);
        if rect.intersects(clip) {
            node(ui, rect, b.span, text);
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
            node(ui, rect, *span, text);
        }
    }
}

fn node(ui: &Ui, rect: Rect, span: mpp_syntax::Span, text: &str) {
    let Some(said) = spoken_at(text, span) else {
        return;
    };
    let id = ui.scope_id().with(("formula", span.start, span.end));
    let r = ui.interact(rect, id, Sense::hover());
    r.widget_info(|| WidgetInfo::labeled(Role::Math, true, said.clone()));
}
