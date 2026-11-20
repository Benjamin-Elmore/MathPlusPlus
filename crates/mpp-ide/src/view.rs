//! The equation view: the typeset page you read and type into.
//!
//! Nothing here ever shows source text. Keys and clicks are mapped to source
//! offsets through the page's caret stops, and the editor changes the text behind
//! the scenes.

use std::ops::Range;
use std::sync::Arc;

use egui::{
    Align, Area, Color32, CursorIcon, Event, EventFilter, Frame, Key, Order, Pos2, Rect, Sense,
    Stroke, Ui, Vec2,
};
use mpp_render::Theme;
use mpp_syntax::document::{Block, BlockKind, Document, Figure};
use mpp_syntax::Severity;

use crate::copy::{clipboard_text, CopyFormat};
use crate::defs;
use crate::doc::{Doc, PageCache};
use crate::editor::symbols::{completions, live_replacement, word_at, Completion};
use crate::figure::FigureDrawer;
use crate::page::BlockTag;
use crate::paint::{c32, paint_display_list};
use crate::theme::Mode;
use crate::typeset;

/// The autocomplete popup.
#[derive(Debug, Clone)]
pub struct CompletionState {
    pub range: Range<usize>,
    pub items: Vec<Completion>,
    pub selected: usize,
}

/// What the view asks the app to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewEvent {
    /// Go to the definition of the name at this source offset.
    GotoAt(usize),
    Save,
}

/// What the view needs from the app.
pub struct ViewCtx<'a> {
    pub mode: Mode,
    pub size: f32,
    pub figures: &'a dyn FigureDrawer,
    pub completion: &'a mut Option<CompletionState>,
    /// The last copy made inside the app: (what other apps got, the source).
    pub clip: &'a mut Option<(String, String)>,
    pub copy_format: CopyFormat,
}

fn find_block(doc: &Document, id: mpp_syntax::document::BlockId) -> Option<&Block> {
    let mut found = None;
    doc.walk(&mut |b| {
        if b.id == id && found.is_none() {
            found = Some(b);
        }
    });
    found
}

/// Draws the page and handles input. Returns the requests for the app.
pub fn show(ui: &mut Ui, doc: &mut Doc, cx: &mut ViewCtx<'_>) -> Vec<ViewEvent> {
    let mut events = vec![];
    let theme = cx.mode.theme();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let avail_w = ui.available_width();
            let avail_h = ui.available_height();
            let pc0 = doc.page(avail_w, cx.size, cx.mode);
            let (resp, painter) = ui.allocate_painter(
                Vec2::new(avail_w, pc0.page.height.max(avail_h)),
                Sense::click_and_drag(),
            );
            let col_left = ((avail_w - pc0.page.width) / 2.0).max(0.0);
            let origin = resp.rect.min + Vec2::new(col_left, 0.0);

            if ui.memory(|m| m.focused().is_none()) {
                ui.memory_mut(|m| m.request_focus(resp.id));
            }
            if resp.clicked() || resp.drag_started() {
                ui.memory_mut(|m| m.request_focus(resp.id));
            }
            let focused = ui.memory(|m| m.has_focus(resp.id));
            if focused {
                ui.memory_mut(|m| {
                    m.set_focus_lock_filter(
                        resp.id,
                        EventFilter {
                            tab: true,
                            horizontal_arrows: true,
                            vertical_arrows: true,
                            escape: false,
                        },
                    );
                });
            }

            let mut moved = handle_pointer(ui, &resp, doc, &pc0, origin, &mut events);
            if focused {
                moved |= handle_keys(ui, doc, &pc0, cx, &mut events);
            }
            if resp.dragged() || resp.clicked() {
                doc.highlight = None;
            }

            // The text may have changed: draw the up-to-date page.
            let pc = doc.page(avail_w, cx.size, cx.mode);
            paint_page(ui, &painter, doc, &pc, origin, cx, &theme, focused);

            let caret_rect = pc.page.caret_stop(doc.editor.caret()).map(|s| {
                Rect::from_min_max(
                    origin + Vec2::new(s.x - 2.0, s.top - 4.0),
                    origin + Vec2::new(s.x + 2.0, s.bottom + 4.0),
                )
            });
            if let Some(target) = doc.scroll_to.take() {
                if let Some(r) = pc.page.rect_of_span(target) {
                    ui.scroll_to_rect(
                        r.translate(origin.to_vec2()).expand(40.0),
                        Some(Align::Center),
                    );
                }
            } else if moved {
                if let Some(r) = caret_rect {
                    ui.scroll_to_rect(r.expand2(Vec2::new(0.0, 24.0)), None);
                }
            }

            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::Text);
                hover_card(ui, doc, &pc, origin, &theme, cx.size);
            }
            completion_popup(ui, doc, &pc, origin, cx, &theme, caret_rect);
        });
    events
}

#[allow(clippy::too_many_arguments)]
fn paint_page(
    ui: &Ui,
    painter: &egui::Painter,
    doc: &Doc,
    pc: &PageCache,
    origin: Pos2,
    cx: &ViewCtx<'_>,
    theme: &Theme,
    focused: bool,
) {
    painter.rect_filled(painter.clip_rect(), 0.0, c32(theme.background));
    let page = &pc.page;
    let off = origin.to_vec2();
    if let Some(span) = doc.highlight {
        if let Some(r) = page.rect_of_span(span) {
            let c = c32(theme.warning).gamma_multiply(0.28);
            painter.rect_filled(r.translate(off).expand(3.0), 4.0, c);
        }
    }
    // Selection.
    if let Some(sel) = doc.editor.selection() {
        let stops = &page.stops;
        let sel_color = ui.visuals().selection.bg_fill;
        for w in stops.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            let same_line = ((a.top + a.bottom) - (b.top + b.bottom)).abs() < 12.0;
            if a.offset >= sel.start && b.offset <= sel.end && same_line && b.x > a.x {
                let r = Rect::from_min_max(
                    origin + Vec2::new(a.x, a.top),
                    origin + Vec2::new(b.x, a.bottom),
                );
                painter.rect_filled(r, 0.0, sel_color);
            }
        }
    }
    let clip = painter.clip_rect();
    for b in &page.blocks {
        let top = origin.y + b.top;
        if top > clip.bottom() || top + b.height < clip.top() {
            continue;
        }
        let at = Pos2::new(origin.x, top);
        if let BlockTag::Figure(r) = b.tag {
            if let Some(Block {
                kind: BlockKind::Figure(f),
                ..
            }) = find_block(&pc.doc, b.id)
            {
                let rect = r.translate(at.to_vec2());
                let f: &Figure = f;
                cx.figures.draw(ui, rect, f, theme);
            }
        }
        paint_display_list(painter, &b.dl, at);
    }
    // Problems: wavy underlines under the typeset element.
    for p in &pc.problems {
        let color = match p.severity {
            Severity::Error => c32(theme.error),
            Severity::Warning => c32(theme.warning),
        };
        crate::diagnostics::paint_squiggle(painter, p.rect.translate(off), color);
    }
    // Caret.
    if focused {
        if let Some(s) = page.caret_stop(doc.editor.caret()) {
            let x = origin.x + s.x;
            painter.line_segment(
                [
                    Pos2::new(x, origin.y + s.top),
                    Pos2::new(x, origin.y + s.bottom),
                ],
                Stroke::new(1.6, c32(theme.text)),
            );
        }
    }
}

fn handle_pointer(
    ui: &Ui,
    resp: &egui::Response,
    doc: &mut Doc,
    pc: &Arc<PageCache>,
    origin: Pos2,
    events: &mut Vec<ViewEvent>,
) -> bool {
    let (pressed, shift, command) = ui.input(|i| {
        (
            i.pointer.primary_pressed(),
            i.modifiers.shift,
            i.modifiers.command,
        )
    });
    let Some(pos) = resp.interact_pointer_pos().or_else(|| resp.hover_pos()) else {
        return false;
    };
    let local = pos - origin.to_vec2();
    if pressed && resp.hovered() {
        if let Some(o) = pc.page.offset_at(local) {
            if command {
                events.push(ViewEvent::GotoAt(o));
            }
            doc.editor.move_caret(o, shift);
            doc.want_x = None;
            return true;
        }
    } else if resp.dragged() {
        if let Some(o) = pc.page.offset_at(local) {
            doc.editor.move_caret(o, true);
            return true;
        }
    }
    false
}

/// Applies typed text one character at a time (so live replacement sees each).
fn type_text(doc: &mut Doc, pc: &PageCache, cx: &mut ViewCtx<'_>, text: &str) {
    let in_math = pc.page.in_math(doc.editor.caret());
    for ch in text.chars().filter(|c| !c.is_control()) {
        let s = ch.to_string();
        doc.editor.insert_text(&s);
        if in_math {
            let caret = doc.editor.caret();
            if let Some(r) = live_replacement(doc.editor.text(), caret, &s) {
                doc.editor.replace_inline(r.range, r.with);
            }
        }
    }
    doc.touched();
    doc.want_x = None;
    update_completion(doc, in_math, cx);
}

fn update_completion(doc: &Doc, in_math: bool, cx: &mut ViewCtx<'_>) {
    *cx.completion = None;
    if !in_math {
        return;
    }
    let text = doc.editor.text();
    let caret = doc.editor.caret();
    let Some(range) = word_at(text, caret) else {
        return;
    };
    let names: Vec<String> = doc
        .eval
        .as_ref()
        .map(|e| e.bindings.keys().cloned().collect())
        .unwrap_or_default();
    let items = completions(&text[range.clone()], names.iter().map(String::as_str), 6);
    if !items.is_empty() {
        *cx.completion = Some(CompletionState {
            range,
            items,
            selected: 0,
        });
    }
}

fn accept_completion(doc: &mut Doc, cx: &mut ViewCtx<'_>) -> bool {
    let Some(c) = cx.completion.take() else {
        return false;
    };
    let Some(item) = c.items.get(c.selected) else {
        return false;
    };
    doc.editor.replace_node(
        mpp_syntax::Span::new(c.range.start, c.range.end),
        &item.insert,
    );
    doc.touched();
    true
}

fn copy_selection(doc: &Doc, pc: &PageCache, cx: &mut ViewCtx<'_>, ui: &Ui) {
    let Some(sel) = doc.editor.selection() else {
        return;
    };
    let Some(src) = doc.editor.text().get(sel.clone()) else {
        return;
    };
    let all_maths = pc.page.in_math(sel.start) && pc.page.in_math(sel.end);
    let out = clipboard_text(src, all_maths, cx.copy_format);
    *cx.clip = Some((out.clone(), src.to_string()));
    ui.ctx().copy_text(out);
}

fn handle_keys(
    ui: &mut Ui,
    doc: &mut Doc,
    pc: &Arc<PageCache>,
    cx: &mut ViewCtx<'_>,
    events: &mut Vec<ViewEvent>,
) -> bool {
    let input = ui.input(|i| i.events.clone());
    let mut moved = false;
    for ev in input {
        match ev {
            Event::Text(t) => {
                type_text(doc, pc, cx, &t);
                moved = true;
            }
            Event::Paste(s) => {
                let src = match cx.clip.as_ref() {
                    Some((shown, source)) if *shown == s => source.clone(),
                    _ => s,
                };
                doc.editor.insert_text(&src.replace('\r', ""));
                doc.touched();
                *cx.completion = None;
                moved = true;
            }
            Event::Copy => copy_selection(doc, pc, cx, ui),
            Event::Cut => {
                copy_selection(doc, pc, cx, ui);
                let c = doc.editor.caret();
                doc.editor.delete(c, false);
                doc.touched();
                moved = true;
            }
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                moved |= key_pressed(doc, pc, cx, events, key, modifiers);
            }
            _ => {}
        }
    }
    moved
}

fn key_pressed(
    doc: &mut Doc,
    pc: &PageCache,
    cx: &mut ViewCtx<'_>,
    events: &mut Vec<ViewEvent>,
    key: Key,
    mods: egui::Modifiers,
) -> bool {
    let page = &pc.page;
    let caret = doc.editor.caret();
    let in_math = page.in_math(caret);
    let shift = mods.shift;
    if mods.command {
        match key {
            Key::A => {
                let n = doc.editor.text().len();
                doc.editor.select(0..n);
                return true;
            }
            Key::Z => {
                let ok = if shift {
                    doc.editor.redo()
                } else {
                    doc.editor.undo()
                };
                if ok {
                    doc.touched();
                }
                return ok;
            }
            Key::Y => {
                if doc.editor.redo() {
                    doc.touched();
                }
                return true;
            }
            Key::S => {
                events.push(ViewEvent::Save);
                return false;
            }
            Key::Home => {
                if let Some((lo, _)) = page.offset_extent() {
                    doc.editor.move_caret(lo, shift);
                }
                return true;
            }
            Key::End => {
                if let Some((_, hi)) = page.offset_extent() {
                    doc.editor.move_caret(hi, shift);
                }
                return true;
            }
            _ => return false,
        }
    }
    match key {
        Key::F12 => {
            events.push(ViewEvent::GotoAt(caret));
            false
        }
        Key::Escape => {
            *cx.completion = None;
            false
        }
        Key::Tab => accept_completion(doc, cx),
        Key::Enter => {
            if accept_completion(doc, cx) {
                return true;
            }
            doc.editor.insert_text("\n");
            doc.touched();
            true
        }
        Key::Backspace | Key::Delete => {
            let fwd = key == Key::Delete;
            if doc.editor.selection().is_some() {
                doc.editor.delete(caret, false);
            } else if in_math {
                let Some(t) = page.offset_step(caret, fwd) else {
                    return false;
                };
                doc.editor.delete(t, true);
            } else {
                let c = if fwd {
                    page.char_after(caret)
                } else {
                    page.char_before(caret)
                };
                let Some(c) = c else {
                    return false;
                };
                doc.editor.delete_range(c.start..c.end);
            }
            doc.touched();
            *cx.completion = None;
            doc.want_x = None;
            true
        }
        Key::ArrowLeft | Key::ArrowRight => {
            let fwd = key == Key::ArrowRight;
            doc.want_x = None;
            *cx.completion = None;
            if let (Some(sel), false) = (doc.editor.selection(), shift) {
                doc.editor
                    .move_caret(if fwd { sel.end } else { sel.start }, false);
            } else if let Some(t) = page.offset_step(caret, fwd) {
                doc.editor.move_caret(t, shift);
            }
            true
        }
        Key::ArrowUp | Key::ArrowDown => {
            let up = key == Key::ArrowUp;
            if let Some(c) = cx.completion.as_mut() {
                let n = c.items.len().max(1);
                c.selected = if up {
                    (c.selected + n - 1) % n
                } else {
                    (c.selected + 1) % n
                };
                return false;
            }
            if doc.want_x.is_none() {
                doc.want_x = page.caret_stop(caret).map(|s| s.x);
            }
            if let Some(t) = page.offset_vertical(caret, up, doc.want_x) {
                doc.editor.move_caret(t, shift);
            }
            true
        }
        Key::Home | Key::End => {
            if let Some((lo, hi)) = page.line_extent(caret) {
                doc.editor
                    .move_caret(if key == Key::Home { lo } else { hi }, shift);
            }
            doc.want_x = None;
            true
        }
        _ => false,
    }
}

fn card_frame(ui: &Ui) -> Frame {
    Frame::popup(ui.style()).inner_margin(egui::Margin::symmetric(10, 8))
}

fn hover_card(ui: &Ui, doc: &Doc, pc: &PageCache, origin: Pos2, theme: &Theme, size: f32) {
    let ctx = ui.ctx();
    let Some(pos) = ctx.pointer_hover_pos() else {
        return;
    };
    if ui.input(|i| i.pointer.any_down()) {
        return;
    }
    let local = pos - origin.to_vec2();
    let problem = pc
        .problems
        .iter()
        .find(|p| p.rect.expand(3.0).contains(local));
    let def = if problem.is_none() {
        pc.page.hit_at(local).and_then(|(_, h)| {
            let ev = doc.eval.as_ref()?;
            defs::definition_at(ev, doc.editor.text(), h.span.start)
        })
    } else {
        None
    };
    if problem.is_none() && def.is_none() {
        return;
    }
    let size = size * 0.9;
    Area::new(egui::Id::unique("mpp-hover-card"))
        .order(Order::Tooltip)
        .fixed_pos(pos + Vec2::new(14.0, 18.0))
        .interactable(false)
        .show(ctx, |ui| {
            card_frame(ui).show(ui, |ui| {
                ui.set_max_width(420.0);
                if let Some(p) = problem {
                    let color = if p.severity == Severity::Error {
                        c32(theme.error)
                    } else {
                        c32(theme.warning)
                    };
                    ui.label(egui::RichText::new(&p.message).color(color).size(16.0));
                }
                if let Some(d) = &def {
                    typeset::show(ui, &d.definition, size, *theme);
                    if let Some(v) = &d.value {
                        ui.add_space(4.0);
                        typeset::show(ui, &format!("= {v}"), size, *theme);
                    }
                    if let Some(f) = &d.file {
                        ui.add_space(2.0);
                        ui.weak(format!(
                            "from {}",
                            crate::explorer::display_name(std::path::Path::new(f))
                        ));
                    }
                }
            });
        });
}

fn completion_popup(
    ui: &mut Ui,
    doc: &mut Doc,
    pc: &PageCache,
    origin: Pos2,
    cx: &mut ViewCtx<'_>,
    theme: &Theme,
    caret_rect: Option<Rect>,
) {
    let Some(state) = cx.completion.clone() else {
        return;
    };
    let _ = (pc, origin);
    let Some(cr) = caret_rect else {
        return;
    };
    let mut pick = None;
    Area::new(egui::Id::unique("mpp-completion"))
        .order(Order::Foreground)
        .fixed_pos(Pos2::new(cr.left(), cr.bottom() + 2.0))
        .show(ui.ctx(), |ui| {
            card_frame(ui).show(ui, |ui| {
                for (i, item) in state.items.iter().enumerate() {
                    let selected = i == state.selected;
                    let row = ui.horizontal(|ui| {
                        ui.set_min_width(190.0);
                        typeset::show(ui, &item.latex, 18.0, *theme);
                        ui.add_space(10.0);
                        ui.weak(&item.label);
                    });
                    let r = row.response.rect.expand2(Vec2::new(4.0, 1.0));
                    if selected {
                        ui.painter().rect_filled(
                            r,
                            3.0,
                            ui.visuals().selection.bg_fill.gamma_multiply(0.6),
                        );
                    }
                    if ui
                        .interact(r, ui.scope_id().with(("mpp-comp", i)), Sense::click())
                        .clicked()
                    {
                        pick = Some(i);
                    }
                }
            });
        });
    if let Some(i) = pick {
        if let Some(c) = cx.completion.as_mut() {
            c.selected = i;
        }
        accept_completion(doc, cx);
    }
}

/// A colour used for the page edge shadow in the app (kept here with the view's colours).
pub fn edge_color(theme: &Theme) -> Color32 {
    c32(theme.rule).gamma_multiply(0.15)
}
