//! The page as a text control for assistive tech (R1). The page is painted,
//! not a `TextEdit`, so its node is filled in by hand: a multi-line text input
//! named after the file, whose value is the caret's line (in maths, the spoken
//! formula with its answer), with the line as a text run and the caret and
//! selection on it, so NVDA echoes what is typed and reads the line.

use egui::text::{CCursor, CCursorRange};
use egui::text_selection::accesskit_text::update_accesskit_for_text_widget;
use egui::{accesskit, emath::TSTransform, Id, Response, Ui, WidgetInfo};

use crate::doc::{Doc, PageCache};

/// Where the frame notes the ids of the pages drawn in it.
const EDITORS: &str = "mpp-access-editors";

/// The ids of the pages (equation views) drawn this frame.
pub fn editor_ids(ctx: &egui::Context) -> Vec<Id> {
    let frame = ctx.cumulative_frame_nr();
    ctx.data(|d| d.get_temp::<(u64, Vec<Id>)>(Id::unique(EDITORS)))
        .filter(|(f, _)| *f + 1 >= frame)
        .map(|(_, v)| v)
        .unwrap_or_default()
}

/// True when `id` is a page (equation view).
pub fn is_editor(ctx: &egui::Context, id: Id) -> bool {
    editor_ids(ctx).contains(&id)
}

fn note_editor(ctx: &egui::Context, id: Id) {
    let frame = ctx.cumulative_frame_nr();
    ctx.data_mut(|d| {
        let e = d.get_temp_mut_or_default::<(u64, Vec<Id>)>(Id::unique(EDITORS));
        if e.0 != frame {
            *e = (frame, Vec::new());
        }
        if !e.1.contains(&id) {
            e.1.push(id);
        }
    });
}

/// The byte range of the line holding `at`.
pub fn line_range(text: &str, at: usize) -> std::ops::Range<usize> {
    let at = at.min(text.len());
    let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let end = text[at..].find('\n').map_or(text.len(), |i| at + i);
    start..end
}

/// What the page's node says as its value: the caret's line in prose, the
/// spoken formula (and what it shows) in maths.
pub fn value(doc: &Doc, pc: &PageCache) -> String {
    let text = doc.editor.text();
    let caret = doc.editor.caret();
    if pc.page.in_math(caret) {
        if let Some(b) = pc
            .page
            .block_at(caret)
            .filter(|b| b.tag == crate::page::BlockTag::Math)
        {
            if let Some(l) = super::answers::block_label(doc, pc, b.span.start) {
                return l;
            }
        }
        if let Some(span) = pc
            .page
            .math_ranges
            .iter()
            .find(|s| s.start <= caret && caret <= s.end)
        {
            if let Some(s) = super::spoken_at(text, *span) {
                return s;
            }
        }
    }
    let r = line_range(text, caret);
    spoken_line(doc, pc, r)
}

/// A prose line as it is read: each inline formula in it (with its `$`s) is
/// replaced by its spoken form and what the page shows after it (R19).
fn spoken_line(doc: &Doc, pc: &PageCache, r: std::ops::Range<usize>) -> String {
    let text = doc.editor.text();
    let mut spans: Vec<mpp_syntax::Span> = pc
        .page
        .math_ranges
        .iter()
        .filter(|s| r.start <= s.start && s.end <= r.end && s.start < s.end)
        .copied()
        .collect();
    spans.sort_by_key(|s| s.start);
    let mut out = String::new();
    let mut at = r.start;
    for span in spans {
        if span.start < at {
            continue;
        }
        // The delimiters round the maths, when the span leaves them out.
        let (mut lo, mut hi) = (span.start, span.end);
        if let Some(open) = ["$$", "\\(", "$"]
            .iter()
            .find(|o| lo >= at + o.len() && text[..lo].ends_with(**o))
        {
            lo -= open.len();
        }
        if let Some(close) = ["$$", "\\)", "$"]
            .iter()
            .find(|c| text[hi..r.end].starts_with(**c))
        {
            hi += close.len();
        }
        let Some(said) = super::answers::inline_label(doc, pc, span) else {
            continue;
        };
        out.push_str(&text[at..lo]);
        out.push_str(&said);
        at = hi;
    }
    out.push_str(&text[at..r.end]);
    out.trim_end_matches('\r').to_string()
}

/// Fills in the page's node: role, name, value, the caret's line and the caret.
pub fn editor_node(ui: &Ui, resp: &Response, doc: &Doc, pc: &PageCache) {
    note_editor(ui.ctx(), resp.id);
    let name = doc.title();
    let said = value(doc, pc);
    let place = crate::page::caret_path::describe(&pc.page, &pc.doc, doc.editor.caret());
    resp.widget_info(|| {
        let mut info = WidgetInfo::text_edit(true, said.clone(), said.clone(), "");
        info.label = Some(name.clone());
        info
    });
    ui.ctx().accesskit_node_builder(resp.id, |n| {
        n.set_role(accesskit::Role::MultilineTextInput);
        n.set_label(name.clone());
        n.set_value(said.clone());
        n.set_description(place.clone());
        n.add_action(accesskit::Action::Focus);
    });
    // The caret's line as a text run, with the caret and selection on it.
    let text = doc.editor.text();
    let caret = doc.editor.caret().min(text.len());
    let r = line_range(text, caret);
    let line = text[r.clone()].trim_end_matches('\r');
    let chars = |to: usize| {
        text[r.start..to.clamp(r.start, r.start + line.len())]
            .chars()
            .count()
    };
    let cursor = match doc.editor.selection() {
        Some(sel) if sel.start >= r.start && sel.end <= r.end => {
            let (a, b) = if caret == sel.start {
                (sel.end, sel.start)
            } else {
                (sel.start, sel.end)
            };
            CCursorRange::two(CCursor::new(chars(a)), CCursor::new(chars(b)))
        }
        _ => CCursorRange::one(CCursor::new(chars(caret))),
    };
    let galley = ui.ctx().fonts_mut(|f| {
        f.layout_no_wrap(
            line.to_string(),
            egui::FontId::proportional(14.0),
            crate::theme::CLEAR,
        )
    });
    let at = doc
        .caret_screen
        .map_or(resp.rect.min, |c| egui::pos2(resp.rect.left(), c.top()));
    update_accesskit_for_text_widget(
        ui.ctx(),
        resp.id,
        Some(cursor),
        TSTransform::from_translation(at.to_vec2()),
        &galley,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_holding_the_caret() {
        let t = "one\ntwo three\nfour";
        assert_eq!(&t[line_range(t, 0)], "one");
        assert_eq!(&t[line_range(t, 6)], "two three");
        assert_eq!(&t[line_range(t, t.len())], "four");
        assert_eq!(&t[line_range(t, 3)], "one");
    }
}
