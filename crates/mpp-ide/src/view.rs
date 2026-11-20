//! The equation view: the typeset page you read and type into.
//!
//! Nothing here ever shows source text. Keys and clicks are mapped to source
//! offsets through the page's caret stops, and the editor changes the text behind
//! the scenes.

use std::ops::Range;
use std::sync::Arc;

use egui::{
    Align, Area, CursorIcon, Event, EventFilter, Frame, Key, Order, Pos2, Rect, Sense, Stroke, Ui,
    Vec2,
};
use mpp_render::Theme;
use mpp_syntax::document::{Block, BlockKind, Document, Figure};

use crate::copy::{clipboard_text, CopyFormat};
use crate::defs;
use crate::diagnostics::fixes::{self, Applied};
use crate::diagnostics::ui::{self as dui, DiagUi, FixMenu, MenuOutcome};
use crate::doc::{Doc, PageCache};
use crate::editor::symbols::{completions, live_replacement, word_at, Completion};
use crate::figure::FigureDrawer;
use crate::insert::{self, InsertUi};
use crate::page::BlockTag;
use crate::paint::{c32, paint_display_list, paint_text};
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
    /// Open a file (a quick fix: "Open mechanics.mpp").
    Open(std::path::PathBuf),
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
    /// The Insert menu and its settings.
    pub insert: &'a mut InsertUi,
    /// The quick-fix menu and the problem card kept open by F8.
    pub diag: &'a mut DiagUi,
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
    let out = egui::ScrollArea::vertical()
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
            cx.diag.origin = origin;

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

            // A click on the bulb opens the quick fixes instead of moving the caret.
            let bulb = bulb_target(doc, &pc0, origin);
            let bulb_clicked = bulb.as_ref().is_some_and(|(r, _)| {
                ui.input(|i| i.pointer.primary_pressed())
                    && resp
                        .interact_pointer_pos()
                        .or_else(|| resp.hover_pos())
                        .is_some_and(|p| r.contains(p))
            });
            let mut moved = false;
            if bulb_clicked {
                if let Some((r, span)) = bulb {
                    open_fix_menu(doc, &pc0, cx, span, r.right_bottom());
                }
            } else {
                moved = handle_pointer(ui, &resp, doc, &pc0, origin, &mut events);
                if moved {
                    cx.diag.peek = None;
                }
            }
            if focused && !cx.insert.menu.open && !cx.diag.menu_open() {
                moved |= handle_keys(ui, doc, &pc0, cx, &mut events);
            }
            if resp.dragged() || resp.clicked() {
                doc.highlight = None;
            }
            context_menu(&resp, doc, &pc0, origin, cx);
            if moved {
                // Leaving a template drops its empty optional parts.
                doc.editor.finalize_outside();
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
                let over_bulb = bulb_target(doc, &pc, origin)
                    .is_some_and(|(r, _)| resp.hover_pos().is_some_and(|p| r.contains(p)));
                ui.ctx().set_cursor_icon(if over_bulb {
                    CursorIcon::PointingHand
                } else {
                    CursorIcon::Text
                });
                if !cx.diag.menu_open() {
                    hover_card(ui, doc, &pc, origin, &theme, cx);
                }
            }
            peek_card(ui, doc, &pc, origin, &theme, cx);
            fix_menu(ui, doc, cx, &mut events, resp.id);
            completion_popup(ui, doc, &pc, origin, cx, &theme, caret_rect);
            if focused || cx.insert.menu.open {
                template_toolbar(ui, doc, &pc, origin, cx, resp.id);
            }
            insert_menu(ui, doc, cx, caret_rect, resp.id);
            (pc.page.height, pc.problems.clone())
        });
    // Problem marks beside the scrollbar: where each problem is on the page.
    let (page_h, problems) = out.inner;
    let content_h = out.content_size.y.max(page_h);
    if !problems.is_empty() {
        let r = out.inner_rect;
        let track = egui::Rect::from_min_max(
            egui::pos2(r.right() - 5.0, r.top() + 2.0),
            egui::pos2(r.right() - 1.0, r.bottom() - 2.0),
        );
        let painter = ui.painter_at(r);
        dui::paint_scrollbar_marks(&painter, track, content_h, &problems, &theme);
    }
    events
}

/// Where the 💡 is drawn (screen rectangle) and the problem it fixes: beside
/// the caret's block, when the problem there has a quick fix.
fn bulb_target(doc: &Doc, pc: &PageCache, origin: Pos2) -> Option<(Rect, mpp_syntax::Span)> {
    let caret = doc.editor.caret();
    let p = dui::problem_at(&pc.problems, &pc.page, caret)?;
    let fixes = fixes::quick_fixes(p, doc.editor.text(), &pc.doc);
    if fixes.is_empty() {
        return None;
    }
    let c = dui::bulb_center(&pc.page, p)? + origin.to_vec2();
    Some((
        Rect::from_center_size(c, Vec2::splat(dui::BULB_RADIUS * 2.8)),
        p.span,
    ))
}

/// Opens the quick-fix menu for the problem at `span`.
fn open_fix_menu(
    doc: &Doc,
    pc: &PageCache,
    cx: &mut ViewCtx<'_>,
    span: mpp_syntax::Span,
    at: Pos2,
) {
    let Some(p) = pc.problems.iter().find(|p| p.span == span) else {
        return;
    };
    let fixes = fixes::quick_fixes(p, doc.editor.text(), &pc.doc);
    if fixes.is_empty() {
        return;
    }
    cx.diag.menu = Some(FixMenu {
        fixes,
        selected: 0,
        at,
        problem: span,
        fresh: true,
    });
    *cx.completion = None;
}

/// The quick-fix menu, when open: applies the chosen fix to the text.
fn fix_menu(
    ui: &Ui,
    doc: &mut Doc,
    cx: &mut ViewCtx<'_>,
    events: &mut Vec<ViewEvent>,
    page_id: egui::Id,
) {
    let Some(menu) = cx.diag.menu.as_mut() else {
        return;
    };
    let theme = cx.mode.theme();
    let size = (cx.size * 0.85).max(13.0);
    match dui::show_menu(ui.ctx(), menu, &theme, cx.mode, size) {
        MenuOutcome::Pick(i) => {
            let fix = menu.fixes.get(i).cloned();
            cx.diag.menu = None;
            cx.diag.peek = None;
            if let Some(f) = fix {
                match fixes::apply(&mut doc.editor, &f) {
                    Applied::Done => doc.touched(),
                    Applied::Open(p) => events.push(ViewEvent::Open(p)),
                }
            }
            ui.memory_mut(|m| m.request_focus(page_id));
        }
        MenuOutcome::Close => {
            cx.diag.menu = None;
            ui.memory_mut(|m| m.request_focus(page_id));
        }
        MenuOutcome::Nothing => {}
    }
}

/// The card of the problem F8 moved to, under its typeset element.
fn peek_card(ui: &Ui, doc: &Doc, pc: &PageCache, origin: Pos2, theme: &Theme, cx: &ViewCtx<'_>) {
    let Some(span) = cx.diag.peek else {
        return;
    };
    if cx.diag.menu_open() {
        return;
    }
    let Some(p) = pc.problems.iter().find(|p| p.span == span) else {
        return;
    };
    let n = fixes::quick_fixes(p, doc.editor.text(), &pc.doc).len();
    let r = p.rect.translate(origin.to_vec2());
    dui::show_card(
        ui.ctx(),
        "mpp-peek-card",
        p,
        Pos2::new(r.left(), r.bottom() + 8.0),
        &dui::CardStyle {
            theme: *theme,
            mode: cx.mode,
            size: card_size(cx.size),
        },
        n,
    );
}

/// The text size of problem cards and menus.
fn card_size(page_size: f32) -> f32 {
    (page_size * 0.85).max(14.0)
}

/// Right-click: puts the caret there and offers Insert.
fn context_menu(
    resp: &egui::Response,
    doc: &mut Doc,
    pc: &PageCache,
    origin: Pos2,
    cx: &mut ViewCtx<'_>,
) {
    if resp.secondary_clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            let at = pc.page.offset_at(p - origin.to_vec2());
            let inside_selection = at
                .zip(doc.editor.selection())
                .is_some_and(|(o, s)| s.start <= o && o <= s.end);
            if let (Some(o), false) = (at, inside_selection) {
                doc.editor.move_caret(o, false);
            }
        }
    }
    let at = resp.interact_pointer_pos();
    resp.context_menu(|ui| {
        if ui
            .button(egui::RichText::new("Insert\u{2026}").font(crate::theme::sans(14.0)))
            .clicked()
        {
            cx.insert.menu.open_at(at);
            ui.close();
        }
    });
}

/// The Insert menu, at the caret (or where it was opened).
fn insert_menu(
    ui: &Ui,
    doc: &mut Doc,
    cx: &mut ViewCtx<'_>,
    caret_rect: Option<Rect>,
    page_id: egui::Id,
) {
    if !cx.insert.menu.open {
        return;
    }
    let anchor = cx
        .insert
        .menu
        .at
        .or_else(|| caret_rect.map(|r| Pos2::new(r.left(), r.bottom() + 4.0)))
        .unwrap_or_else(|| ui.max_rect().left_top() + Vec2::new(40.0, 40.0));
    let recent = cx.insert.settings.recent.clone();
    match insert::menu::show(ui.ctx(), &mut cx.insert.menu, &recent, anchor, cx.mode) {
        insert::menu::Outcome::Insert { id, knobs } => {
            if let Some(t) = insert::find(&id) {
                let caret = doc.editor.caret();
                let bcx = cx.insert.settings.build_cx(doc.editor.text(), caret);
                let pc = doc.page(ui.available_width().max(200.0), cx.size, cx.mode);
                insert::insert(&mut doc.editor, &pc.page, t, knobs, &bcx);
                cx.insert.settings.touch(&id);
                doc.touched();
            }
            ui.memory_mut(|m| m.request_focus(page_id));
        }
        insert::menu::Outcome::Closed => ui.memory_mut(|m| m.request_focus(page_id)),
        insert::menu::Outcome::Nothing => {}
    }
}

/// The controls of the template the caret is in (mode, rows, infinity buttons).
fn template_toolbar(
    ui: &Ui,
    doc: &mut Doc,
    pc: &PageCache,
    origin: Pos2,
    cx: &mut ViewCtx<'_>,
    page_id: egui::Id,
) {
    let caret = doc.editor.caret();
    let Some(s) = doc.editor.session_at(caret) else {
        return;
    };
    let controls = insert::controls(&s.template, s.knobs);
    if controls.is_empty() {
        return;
    }
    let (id, range) = (s.id, s.range.clone());
    let Some(r) = pc
        .page
        .rect_of_span(mpp_syntax::Span::new(range.start, range.end))
    else {
        return;
    };
    let rect = r.translate(origin.to_vec2());
    if let Some(i) = insert::menu::toolbar(ui.ctx(), &controls, rect) {
        let bcx = cx.insert.settings.build_cx(doc.editor.text(), caret);
        insert::apply(&mut doc.editor, id, &controls[i].action, &bcx);
        doc.touched();
        ui.memory_mut(|m| m.request_focus(page_id));
    }
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
            let c = c32(cx.mode.palette().highlight);
            painter.rect_filled(r.translate(off).expand(3.0), 4.0, c);
        }
    }
    // The template box the caret is in is lightly highlighted.
    if focused {
        let caret = doc.editor.caret();
        let current = doc.editor.session_at(caret).and_then(|s| {
            let b = &s.boxes[s.box_at(caret)?];
            Some(mpp_syntax::Span::new(b.range.start, b.range.end))
        });
        if let Some(r) = current.and_then(|span| page.rect_of_span(span)) {
            let c = c32(cx.mode.palette().row_selected);
            painter.rect_filled(r.translate(off).expand(2.0), 3.0, c);
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
    // Problems: underlines under the typeset element, a bar in the margin,
    // the import summaries and the bulb of the caret's problem.
    dui::paint_marks(painter, &pc.problems, off, theme);
    dui::paint_margin(painter, page, &pc.problems, origin, theme);
    dui::paint_import_notes(
        painter,
        &pc.problems,
        off,
        theme,
        cx.size,
        painter.clip_rect().right() - 8.0,
    );
    if focused {
        if let Some((r, _)) = bulb_target(doc, pc, origin) {
            dui::paint_bulb(
                painter,
                r.center(),
                dui::BULB_RADIUS,
                dui::bulb_color(cx.mode),
                c32(theme.muted),
            );
        }
    }
    // Unfilled template boxes: a typeset hint beside the equation, no answer.
    for b in &page.blocks {
        if b.tag != BlockTag::Math {
            continue;
        }
        let span = b.span.start..b.span.end;
        let Some(hint) = insert::fill_in_hint(&doc.editor, page, &span) else {
            continue;
        };
        // The right edge of the ink: boxes, plus glyphs without one (the `?`).
        let glyphs = b.dl.items.iter().filter_map(|p| match p {
            mpp_render::Primitive::Glyph { x, size, .. } => Some(x + size * 0.75),
            mpp_render::Primitive::Rule { rect, .. } => Some(rect.x + rect.w),
            _ => None,
        });
        let right =
            b.dl.hits
                .iter()
                .map(|h| h.rect.x + h.rect.w)
                .chain(glyphs)
                .fold(0.0f32, f32::max);
        let size = cx.size * 0.85;
        let w = crate::page::text_width(mpp_render::FontId::TextItalic, &hint) * size;
        let x = (right + cx.size).min(page.width - w).max(0.0);
        let y = origin.y + b.top + b.height / 2.0 + size * 0.3;
        paint_text(
            painter,
            &hint,
            Pos2::new(origin.x + x, y),
            size,
            mpp_render::FontId::TextItalic,
            c32(theme.placeholder_label),
        );
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
        if in_math && cx.insert.settings.shortcuts {
            let caret = doc.editor.caret();
            let bcx = cx.insert.settings.build_cx(doc.editor.text(), caret);
            if insert::shortcut(&mut doc.editor, ch, &bcx) {
                continue;
            }
        }
        if in_math {
            let caret = doc.editor.caret();
            if let Some(r) = live_replacement(doc.editor.text(), caret, &s) {
                doc.editor.replace_inline(r.range, r.with);
            }
        }
    }
    doc.touched();
    doc.want_x = None;
    cx.diag.peek = None;
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
            Key::Period => {
                let target = dui::problem_at(&pc.problems, page, caret).map(|p| (p.span, p.rect));
                if let Some((span, r)) = target {
                    let at = egui::pos2(r.left(), r.bottom() + 6.0);
                    cx.diag.peek = None;
                    let origin = cx.diag.origin;
                    open_fix_menu(doc, pc, cx, span, at + origin.to_vec2());
                }
                return false;
            }
            Key::Space => {
                cx.insert.menu.open_at(None);
                *cx.completion = None;
                return false;
            }
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
        Key::F8 => {
            if let Some(p) = dui::next_problem(&pc.problems, caret, !shift) {
                doc.editor.move_caret(dui::caret_target(page, p), false);
                doc.scroll_to = Some(p.span);
                cx.diag.peek = Some(p.span);
                doc.want_x = None;
            }
            false
        }
        Key::F12 => {
            events.push(ViewEvent::GotoAt(caret));
            false
        }
        Key::Escape => {
            *cx.completion = None;
            false
        }
        Key::Tab => {
            if accept_completion(doc, cx) {
                return true;
            }
            if !doc.editor.tab(!shift) {
                tab_to_slot(doc, page, !shift);
            }
            doc.want_x = None;
            true
        }
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
            if !fwd && in_math && doc.editor.backspace_in_template() {
                doc.touched();
                *cx.completion = None;
                return true;
            }
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

/// Tab outside a tracked template: the next (or previous) empty box on the page.
fn tab_to_slot(doc: &mut Doc, page: &crate::page::Page, forward: bool) {
    let caret = doc.editor.caret();
    let mut slots: Vec<usize> = page
        .blocks
        .iter()
        .flat_map(|b| b.dl.hits.iter())
        .filter(|h| h.kind == mpp_render::HitKind::Slot)
        .map(|h| h.span.start)
        .collect();
    slots.sort_unstable();
    slots.dedup();
    let target = if forward {
        slots.into_iter().find(|&o| o > caret)
    } else {
        slots.into_iter().rev().find(|&o| o < caret)
    };
    if let Some(o) = target {
        doc.editor.move_caret(o, false);
    }
}

fn card_frame(ui: &Ui) -> Frame {
    Frame::popup(ui.style()).inner_margin(egui::Margin::symmetric(10, 8))
}

fn hover_card(ui: &Ui, doc: &Doc, pc: &PageCache, origin: Pos2, theme: &Theme, cx: &ViewCtx<'_>) {
    let ctx = ui.ctx();
    let Some(pos) = ctx.pointer_hover_pos() else {
        return;
    };
    if ui.input(|i| i.pointer.any_down()) {
        return;
    }
    let local = pos - origin.to_vec2();
    // The most serious problem under the pointer (its underline or its element).
    let problem = pc
        .problems
        .iter()
        .filter(|p| {
            p.rects
                .iter()
                .any(|r| r.expand2(Vec2::new(2.0, 4.0)).contains(local))
        })
        .min_by_key(|p| (p.level, p.span.len()));
    if let Some(p) = problem {
        if cx.diag.peek == Some(p.span) {
            return;
        }
        let n = crate::diagnostics::fixes::quick_fixes(p, doc.editor.text(), &pc.doc).len();
        dui::show_card(
            ctx,
            "mpp-hover-card",
            p,
            pos + Vec2::new(14.0, 18.0),
            &dui::CardStyle {
                theme: *theme,
                mode: cx.mode,
                size: card_size(cx.size),
            },
            n,
        );
        return;
    }
    let def = pc.page.hit_at(local).and_then(|(_, h)| {
        let ev = doc.eval.as_ref()?;
        defs::definition_at(ev, doc.editor.text(), h.span.start)
    });
    let Some(d) = def else {
        return;
    };
    let size = cx.size * 0.9;
    Area::new(egui::Id::unique("mpp-hover-card"))
        .order(Order::Tooltip)
        .fixed_pos(pos + Vec2::new(14.0, 18.0))
        .interactable(false)
        .show(ctx, |ui| {
            card_frame(ui).show(ui, |ui| {
                ui.set_max_width(420.0);
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
                    // Reserve a slot so the highlight is painted behind the row.
                    let backdrop = ui.painter().add(egui::Shape::Noop);
                    let row = ui.horizontal(|ui| {
                        ui.set_min_width(190.0);
                        typeset::show(ui, &item.latex, 18.0, *theme);
                        ui.add_space(10.0);
                        ui.weak(&item.label);
                    });
                    let r = row.response.rect.expand2(Vec2::new(4.0, 1.0));
                    if selected {
                        ui.painter().set(
                            backdrop,
                            egui::Shape::rect_filled(r, 3.0, c32(cx.mode.palette().row_selected)),
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
