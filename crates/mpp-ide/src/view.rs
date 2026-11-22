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
use mpp_syntax::document::BlockId;

use crate::copy::{clipboard_text, CopyFormat};
use crate::defs;
use crate::diagnostics::fixes::{self, Applied};
use crate::diagnostics::ui::{self as dui, DiagUi, FixMenu, MenuOutcome};
use crate::doc::{Doc, PageCache};
use crate::editor::symbols::{completions, word_at, Completion};
use crate::insert::{self, InsertUi};
use crate::page::{BlockTag, Side};
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
    /// A click landed here; if it is on an import, open that file.
    OpenLinkAt(usize),
    /// "Open in tile" on a figure (counting figures from 0).
    OpenFigureTile(usize),
    /// Export a figure (counting from 0) as PNG (`false`) or SVG (`true`).
    ExportFigure(usize, bool),
    Save,
    /// Open a file (a quick fix: "Open mechanics.mpp").
    Open(std::path::PathBuf),
    /// Prove… on a result (`Theorem 2`): the proof agent panel.
    Prove(String),
    /// Check against Lean… on a result: the alignment view.
    AlignLean(String),
    /// Insert → Conjecture: the proof agent proposes conjectures.
    Conjecture,
}

/// What the view needs from the app.
pub struct ViewCtx<'a> {
    pub mode: Mode,
    pub size: f32,
    pub completion: &'a mut Option<CompletionState>,
    /// The last copy made inside the app: (what other apps got, the source).
    pub clip: &'a mut Option<(String, String)>,
    pub copy_format: CopyFormat,
    /// The Insert menu and its settings.
    pub insert: &'a mut InsertUi,
    /// The quick-fix menu and the problem card kept open by F8.
    pub diag: &'a mut DiagUi,
    /// True for the one view that owns the Insert menu, the quick-fix menu,
    /// the F8 card and autocomplete (several views can be on screen in tiles).
    pub active: bool,
}

/// The step fold line under a screen point.
fn toggle_at(pc: &PageCache, origin: Pos2, pos: Pos2) -> Option<mpp_syntax::Span> {
    let local = pos - origin.to_vec2();
    pc.page
        .step_toggles
        .iter()
        .find(|t| t.rect.contains(local))
        .map(|t| t.line)
}

/// The evaluated line whose result box is under a screen point.
fn result_at(doc: &Doc, pc: &PageCache, origin: Pos2, pos: Pos2) -> Option<(BlockId, usize)> {
    let local = pos - origin.to_vec2();
    let (b, h) = pc.page.hit_at(local)?;
    if h.kind != mpp_render::HitKind::Result {
        return None;
    }
    let br = doc.eval.as_ref()?.block(b.id)?;
    let i = crate::page::result_line_at(br, h.span.start)?;
    Some((b.id, i))
}

/// A typeset sentence about a check: proved, agrees at points, or the
/// counterexample.
fn verdict_latex(v: &mpp_eval::Verdict) -> String {
    let num = |s: &str| {
        s.chars()
            .filter(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | 'e' | 'E' | '+'))
            .collect::<String>()
    };
    match v {
        mpp_eval::Verdict::Proved => "\\checkmark\\; \\text{proved symbolically}".into(),
        mpp_eval::Verdict::Holds { points } => {
            format!("\\checkmark\\; \\text{{agrees at {points} random points}}")
        }
        mpp_eval::Verdict::Fails { at, lhs, rhs } => {
            let point: Vec<String> = at
                .iter()
                .map(|(n, x)| format!("{} = {}", mpp_plot::view2d::name_latex(n), num(x)))
                .collect();
            let place = if point.is_empty() {
                String::new()
            } else {
                format!("\\text{{ at }} {}", point.join(",\\; "))
            };
            format!(
                "\\times{place}\\text{{: left side }} {}\\text{{, right side }} {}",
                num(lhs),
                num(rhs)
            )
        }
        mpp_eval::Verdict::Agrees { digits } => {
            format!("\\approx\\; \\text{{checked numerically to {digits} digits}}")
        }
        mpp_eval::Verdict::Unknown(_) => "\\text{could not decide}".into(),
    }
}

/// The card shown over a check's ✓ or ✗.
fn verdict_card(ui: &Ui, doc: &Doc, pc: &PageCache, origin: Pos2, theme: &Theme, size: f32) {
    let Some(pos) = ui.input(|i| i.pointer.hover_pos()) else {
        return;
    };
    let Some((block, i)) = result_at(doc, pc, origin, pos) else {
        return;
    };
    let Some(verdict) = doc
        .eval
        .as_ref()
        .and_then(|ev| ev.block(block))
        .and_then(|b| b.values.get(i))
        .and_then(|v| v.verdict.clone())
    else {
        return;
    };
    let tex = verdict_latex(&verdict);
    let theme = *theme;
    egui::Area::new(egui::Id::unique("verdict-card"))
        .order(egui::Order::Tooltip)
        .fixed_pos(pos + Vec2::new(12.0, 16.0))
        .show(ui.ctx(), |ui| {
            card_frame(ui).show(ui, |ui| {
                typeset::show(ui, &tex, size * 0.9, theme);
            });
        });
}

/// Follows a click on a link: jumps to an equation, a reference entry or a
/// footnote, or folds a note.
pub fn follow_link(doc: &mut Doc, target: &crate::page::LinkTarget) {
    match target {
        crate::page::LinkTarget::Jump(span) => {
            doc.editor.move_caret(span.start, false);
            doc.highlight = Some(*span);
            doc.scroll_to = Some(*span);
            doc.want_x = None;
        }
        crate::page::LinkTarget::Fold(id) => doc.show_mut().toggle_fold(*id),
    }
}

/// The card over a link: a footnote's note, a citation's entry, a folded note.
fn link_card(ui: &Ui, pc: &PageCache, origin: Pos2, theme: &Theme, size: f32) {
    let Some(pos) = ui.input(|i| i.pointer.hover_pos()) else {
        return;
    };
    let Some(tip) = pc
        .page
        .link_at(pos - origin.to_vec2())
        .and_then(|l| l.link.tip.clone())
    else {
        return;
    };
    let theme = *theme;
    egui::Area::new(egui::Id::unique("mpp-link-card"))
        .order(egui::Order::Tooltip)
        .fixed_pos(pos + Vec2::new(12.0, 16.0))
        .show(ui.ctx(), |ui| {
            card_frame(ui).show(ui, |ui| {
                ui.set_max_width(size * 26.0);
                // A note's paragraphs come one per line.
                for line in tip.lines() {
                    typeset::show(ui, line, size * 0.85, theme);
                }
            });
        });
}

/// Draws the page and handles input. Returns the requests for the app.
pub fn show(ui: &mut Ui, doc: &mut Doc, cx: &mut ViewCtx<'_>) -> Vec<ViewEvent> {
    let mut events = vec![];
    let theme = cx.mode.theme();
    // A scroll that keeps the caret's line still (see `pin_shift`), taken up
    // in the frame after it was drawn, and the offset it needs room for.
    let pin_id = ui.scope_id().with("mpp-pin");
    let pending = ui.data_mut(|d| d.remove_temp::<f32>(pin_id.with("scroll")));
    let floor_id = pin_id.with("floor");
    let out = with_scroll(egui::ScrollArea::vertical(), pending)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let avail_w = ui.available_width();
            let avail_h = ui.available_height();
            let pc0 = doc.page(avail_w, cx.size, cx.mode);
            // The caret's line stays where it was drawn when the page grows or
            // shrinks above it (an answer arriving): the view scrolls instead.
            let scrolled = (ui.clip_rect().top() - ui.cursor().top()).max(0.0);
            let pinned = cx.active
                && doc
                    .caret_screen
                    .is_some_and(|r| r.intersects(ui.clip_rect()));
            let shift0 = pin_shift(doc, &pc0, pinned, scrolled);
            let floor = ui.data(|d| d.get_temp::<f32>(floor_id)).unwrap_or(0.0);
            let floor = if scrolled + 0.5 < floor { 0.0 } else { floor };
            let height = pc0
                .page
                .height
                .max(avail_h)
                .max(floor.max(scrolled + shift0) + avail_h);
            let (resp, painter) =
                ui.allocate_painter(Vec2::new(avail_w, height), Sense::click_and_drag());
            let col_left = ((avail_w - pc0.page.width) / 2.0).max(0.0);
            let base = resp.rect.min + Vec2::new(col_left, 0.0);
            let mut origin = base - Vec2::new(0.0, shift0);
            if cx.active {
                cx.diag.origin = origin;
            }

            let focused = take_focus(ui, &resp, std::mem::take(&mut doc.take_focus));

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
            let step_click = ui
                .input(|i| i.pointer.primary_pressed())
                .then(|| resp.interact_pointer_pos().or_else(|| resp.hover_pos()))
                .flatten()
                .and_then(|p| toggle_at(&pc0, origin, p));
            let link_click = ui
                .input(|i| i.pointer.primary_pressed())
                .then(|| resp.interact_pointer_pos().or_else(|| resp.hover_pos()))
                .flatten()
                .and_then(|p| pc0.page.link_at(p - origin.to_vec2()))
                .map(|l| l.link.target.clone());
            if let Some(line) = step_click {
                doc.show_mut().toggle_steps(line);
            } else if let Some(target) = link_click {
                // A pass that egui runs again (after a layout change) sees the same
                // press: follow it once per frame, or a fold would undo itself.
                let frame = ui.ctx().cumulative_frame_nr();
                let key = egui::Id::unique("mpp-link-frame");
                if ui.data(|d| d.get_temp::<u64>(key)) != Some(frame) {
                    ui.data_mut(|d| d.insert_temp(key, frame));
                    follow_link(doc, &target);
                }
                moved = true;
            } else if bulb_clicked {
                if let Some((r, span)) = bulb {
                    open_fix_menu(doc, &pc0, cx, span, r.right_bottom());
                }
            } else {
                moved = handle_pointer(ui, &resp, doc, &pc0, origin, &mut events);
                if resp.double_clicked() || resp.triple_clicked() {
                    if let Some(p) = resp.interact_pointer_pos() {
                        let at = p - origin.to_vec2();
                        moved |= crate::editor::select::multi_click(
                            &mut doc.editor,
                            &pc0.page,
                            at,
                            resp.triple_clicked(),
                        );
                    }
                }
                if moved {
                    cx.diag.peek = None;
                }
            }
            if focused && !cx.insert.menu.open && !cx.diag.menu_open() {
                moved |= handle_keys(ui, doc, &pc0, cx, &mut events);
            }
            if cx.active && !focused {
                f8_from_elsewhere(ui, doc, &pc0, cx, resp.id);
            }
            if resp.dragged() || resp.clicked() {
                doc.highlight = None;
            }
            context_menu(ui, &resp, doc, &pc0, origin, cx, &mut events);
            if moved {
                // Leaving a template drops its empty optional parts.
                doc.editor.finalize_outside();
            }

            // The text may have changed: draw the up-to-date page.
            let pc = doc.page(avail_w, cx.size, cx.mode);
            let shift = pin_shift(doc, &pc, pinned, scrolled);
            origin = base - Vec2::new(0.0, shift);
            paint_page(ui, &painter, doc, &pc, origin, cx, &theme, focused);
            figures_pass(ui, doc, &pc, origin, cx, resp.id, &mut events);
            spinners(ui, &painter, &pc, origin, &theme);

            let caret_rect = pc
                .page
                .caret_stop_on(doc.editor.caret(), doc.caret_side)
                .map(|s| {
                    Rect::from_min_max(
                        origin + Vec2::new(s.x - 2.0, s.top - 4.0),
                        origin + Vec2::new(s.x + 2.0, s.bottom + 4.0),
                    )
                });
            if cx.active {
                doc.caret_screen = caret_rect;
                let (caret, side) = (doc.editor.caret(), doc.caret_side);
                doc.pin = crate::page::Pin::new(&pc.page, doc.editor.text(), caret, side);
            }
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
                let over_link = resp
                    .hover_pos()
                    .is_some_and(|p| pc.page.link_at(p - origin.to_vec2()).is_some());
                ui.ctx().set_cursor_icon(if over_bulb || over_link {
                    CursorIcon::PointingHand
                } else {
                    CursorIcon::Text
                });
                if !cx.diag.menu_open() && !proof_mark_card(ui, &pc, origin, &theme, cx.size) {
                    hover_card(ui, doc, &pc, origin, &theme, cx);
                    verdict_card(ui, doc, &pc, origin, &theme, cx.size);
                    link_card(ui, &pc, origin, &theme, cx.size);
                }
            }
            if cx.active {
                peek_card(ui, doc, &pc, origin, &theme, cx);
                fix_menu(ui, doc, cx, &mut events, resp.id);
                completion_popup(ui, doc, &pc, origin, cx, &theme, caret_rect);
                command_list(ui, doc, &pc, cx, caret_rect);
                if focused || cx.insert.menu.open {
                    template_toolbar(ui, doc, &pc, origin, cx, resp.id);
                }
                insert_menu(ui, doc, cx, caret_rect, resp.id, &mut events);
            }
            // A menu or popup open now closes itself on the next Esc.
            let popup = ui.ctx().any_popup_open();
            ui.data_mut(|d| d.insert_temp(egui::Id::unique(POPUP_OPEN), popup));
            if ui.memory(|m| m.has_focus(resp.id)) {
                let filter = key_filter(doc, &pc, cx, popup);
                ui.memory_mut(|m| m.set_focus_lock_filter(resp.id, filter));
            }
            if shift != 0.0 {
                ui.data_mut(|d| d.insert_temp(floor_id, scrolled + shift));
            } else if floor == 0.0 {
                ui.data_mut(|d| d.remove::<f32>(floor_id));
            }
            (pc, shift)
        });
    let (pc, shift) = out.inner;
    if shift != 0.0 {
        let y = (out.state.clamped_offset().y + shift).max(0.0);
        ui.data_mut(|d| d.insert_temp(pin_id.with("scroll"), y));
        ui.ctx().request_repaint();
    }
    // Problem marks beside the scrollbar: where each problem is on the page.
    let (page_h, problems) = (pc.page.height, &pc.problems);
    let content_h = out.content_size.y.max(page_h);
    if !problems.is_empty() {
        let r = out.inner_rect;
        let track = egui::Rect::from_min_max(
            egui::pos2(r.right() - 5.0, r.top() + 2.0),
            egui::pos2(r.right() - 1.0, r.bottom() - 2.0),
        );
        let painter = ui.painter_at(r);
        dui::paint_scrollbar_marks(&painter, track, content_h, problems, &theme);
    }
    events
}

/// Where the view notes whether a menu or popup was open at the end of the
/// last frame (Esc then closes it and does nothing else).
const POPUP_OPEN: &str = "mpp-popup-open";

/// Gives the page the keyboard: on a click, or when nothing else has it. When
/// the page let go of it on purpose (Esc with nothing open), only a click
/// brings it back, and the page never takes it from another widget (the side
/// column's search box, quick open, a dialog): a click elsewhere hands it over
/// as anywhere in egui. True when the page has it.
/// `force` is set when a file was just opened or its tab clicked (U5).
fn take_focus(ui: &Ui, resp: &egui::Response, force: bool) -> bool {
    let away_id = resp.id.with("mpp-keyboard-away");
    let pressed = ui.input(|i| i.pointer.any_pressed());
    // A menu or popup moves the keyboard about while it is open.
    let popup = ui.ctx().any_popup_open()
        || ui
            .data(|d| d.get_temp::<bool>(egui::Id::unique(POPUP_OPEN)))
            .unwrap_or(false);
    // Let go this frame by a key, not taken by a click, a menu or another widget.
    let let_go = !pressed
        && !popup
        && ui.memory(|m| {
            m.had_focus_last_frame(resp.id) && !m.has_focus(resp.id) && m.focused().is_none()
        });
    let away = let_go || ui.data(|d| d.get_temp::<bool>(away_id)).unwrap_or(false);
    if force || resp.clicked() || resp.drag_started() {
        ui.memory_mut(|m| m.request_focus(resp.id));
    } else if !away && ui.memory(|m| m.focused().is_none()) {
        ui.memory_mut(|m| m.request_focus(resp.id));
    }
    let focused = ui.memory(|m| m.has_focus(resp.id));
    ui.data_mut(|d| d.insert_temp(away_id, away && !focused));
    focused
}

/// Which keys the page keeps for itself next frame (the rest move the
/// keyboard to other widgets, as anywhere else). Tab only while there is a box
/// to go to or an autocomplete to accept; Esc only while it has something to
/// do (close a popup, leave a box, leave maths).
fn key_filter(doc: &Doc, pc: &PageCache, cx: &ViewCtx<'_>, menu: bool) -> EventFilter {
    let caret = doc.editor.caret();
    let in_template = doc.editor.session_at(caret).is_some();
    let popup = menu
        || cx.completion.is_some()
        || cx.diag.peek.is_some()
        || cx.insert.menu.open
        || cx.insert.commands.open
        || cx.diag.menu_open();
    EventFilter {
        tab: popup || in_template || !empty_slots(&pc.page, caret).is_empty(),
        horizontal_arrows: true,
        vertical_arrows: true,
        escape: popup || in_template || pc.page.in_math(caret),
    }
}

/// The empty boxes on the page other than the one at `caret`, in order.
fn empty_slots(page: &crate::page::Page, caret: usize) -> Vec<usize> {
    let mut slots: Vec<usize> = page
        .blocks
        .iter()
        .flat_map(|b| b.dl.hits.iter())
        .filter(|h| h.kind == mpp_render::HitKind::Slot)
        .map(|h| h.span.start)
        .filter(|&o| o != caret)
        .collect();
    slots.sort_unstable();
    slots.dedup();
    slots
}

/// An arrow key at the edge of the document: the page has nowhere to move the
/// caret, so the key moves the keyboard to the widget that way, as it does
/// everywhere else. Up on the first line first goes to the line's start, and
/// Down on the last line to its end.
fn edge_release(
    doc: &Doc,
    pc: &PageCache,
    key: Key,
    mods: egui::Modifiers,
) -> Option<egui::FocusDirection> {
    use egui::FocusDirection as F;
    if mods.any() || doc.editor.selection().is_some() {
        return None;
    }
    let page = &pc.page;
    let (caret, side) = (doc.editor.caret(), doc.caret_side);
    let (lo, hi) = page.line_extent_on(caret, side).unwrap_or((caret, caret));
    match key {
        Key::ArrowLeft => page
            .step_on(caret, side, false)
            .is_none()
            .then_some(F::Left),
        Key::ArrowRight => page
            .step_on(caret, side, true)
            .is_none()
            .then_some(F::Right),
        Key::ArrowUp => {
            (page.vertical_on(caret, side, true, None).is_none() && caret == lo).then_some(F::Up)
        }
        Key::ArrowDown => {
            (page.vertical_on(caret, side, false, None).is_none() && caret == hi).then_some(F::Down)
        }
        _ => None,
    }
}

/// Esc does one thing, the first of: close a popup, leave the template box,
/// leave the maths (the caret goes just after it). True when the caret moved.
fn escape(doc: &mut Doc, page: &crate::page::Page, cx: &mut ViewCtx<'_>) -> bool {
    if cx.completion.take().is_some() || cx.diag.peek.take().is_some() {
        return false;
    }
    let caret = doc.editor.caret();
    let target = match doc.editor.session_at(caret) {
        Some(s) => Some(s.range.end),
        None => leave_maths(doc.editor.text(), page, caret),
    };
    let Some(t) = target else {
        return false;
    };
    doc.editor.move_caret(t, false);
    doc.caret_side = Side::Before;
    doc.want_x = None;
    true
}

/// Just after the maths holding `caret`: past the closing `$` of inline maths,
/// or the end of a display block.
fn leave_maths(text: &str, page: &crate::page::Page, caret: usize) -> Option<usize> {
    let r = page
        .math_ranges
        .iter()
        .filter(|r| r.start <= caret && caret <= r.end)
        .min_by_key(|r| r.end - r.start)?;
    if let Some(b) = page.block_at(caret).filter(|b| b.tag == BlockTag::Math) {
        return Some(b.span.end);
    }
    let rest = text.get(r.end..).unwrap_or("");
    let close = ["$$", "\\]", "\\)", "$"]
        .iter()
        .find(|c| rest.starts_with(**c))
        .map_or(0, |c| c.len());
    Some(r.end + close)
}

/// The scroll area, moved to `y` when a scroll is pending.
fn with_scroll(area: egui::ScrollArea, y: Option<f32>) -> egui::ScrollArea {
    match y {
        Some(y) => area.vertical_scroll_offset(y),
        None => area,
    }
}

/// How far to scroll so the caret's line stays where it was last drawn
/// (never above the top of the page).
fn pin_shift(doc: &Doc, pc: &PageCache, pinned: bool, scrolled: f32) -> f32 {
    let shift = doc
        .pin
        .filter(|_| pinned)
        .and_then(|p| p.shift(&pc.page, doc.editor.text()))
        .unwrap_or(0.0);
    if shift.abs() < 0.5 {
        0.0
    } else {
        shift.max(-scrolled)
    }
}

/// Draws the figures of the page and their sliders, and handles their mouse.
fn figures_pass(
    ui: &mut Ui,
    doc: &mut Doc,
    pc: &PageCache,
    origin: Pos2,
    cx: &ViewCtx<'_>,
    view_id: egui::Id,
    events: &mut Vec<ViewEvent>,
) {
    let clip = ui.clip_rect();
    let mut index = 0;
    for b in &pc.page.blocks {
        let BlockTag::Figure(r) = b.tag else {
            continue;
        };
        let n = index;
        index += 1;
        let rect = r.translate(Vec2::new(origin.x, origin.y + b.top));
        let rows = pc.doc_rows(b.id);
        let full =
            rect.with_max_y(rect.bottom() + rows as f32 * crate::page::slider_row_height(cx.size));
        if !full.intersects(clip) {
            continue;
        }
        let Some(st) = doc.figures.of_block(b.id) else {
            continue;
        };
        let id = view_id.with(("figure", n));
        match crate::figure::figure_ui(ui, rect, id, st, cx.mode, false) {
            Some(crate::figure::FigureAction::OpenInTile) => {
                events.push(ViewEvent::OpenFigureTile(n))
            }
            Some(crate::figure::FigureAction::ExportPng) => {
                events.push(ViewEvent::ExportFigure(n, false))
            }
            Some(crate::figure::FigureAction::ExportSvg) => {
                events.push(ViewEvent::ExportFigure(n, true))
            }
            None => {}
        }
        if rows > 0 {
            let top = Pos2::new(rect.left(), rect.bottom() + cx.size * 0.2);
            if crate::figure::sliders_ui(ui, top, rect.width(), cx.size, st, cx.mode) {
                ui.ctx().request_repaint();
            }
        }
    }
}

/// Spinners beside answers that are still being worked out.
fn spinners(ui: &Ui, painter: &egui::Painter, pc: &PageCache, origin: Pos2, theme: &Theme) {
    if pc.page.spinners.is_empty() {
        return;
    }
    let t = ui.input(|i| i.time) as f32;
    for p in &pc.page.spinners {
        let c = origin + p.to_vec2();
        let r = 6.0;
        let start = t * 5.0;
        let pts: Vec<Pos2> = (0..=20)
            .map(|k| {
                let a = start + k as f32 / 20.0 * std::f32::consts::PI * 1.5;
                c + Vec2::new(a.cos(), a.sin()) * r
            })
            .collect();
        painter.add(egui::Shape::line(
            pts,
            Stroke::new(1.8, c32(theme.auto_result)),
        ));
    }
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(30));
}

/// The figure (counting figures from 0) under a screen point, if any.
fn figure_at(pc: &PageCache, origin: Pos2, pos: Pos2) -> Option<usize> {
    let local = pos - origin.to_vec2();
    pc.page
        .blocks
        .iter()
        .filter_map(|b| match b.tag {
            BlockTag::Figure(r) => Some(r.translate(Vec2::new(0.0, b.top))),
            _ => None,
        })
        .position(|r| r.contains(local))
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

/// F8 and Shift+F8 while the keyboard is somewhere else (the explorer, a tab):
/// the active view moves to the next problem and takes the keyboard. A text
/// box with the keyboard (rename, search) keeps its F8.
fn f8_from_elsewhere(
    ui: &Ui,
    doc: &mut Doc,
    pc: &PageCache,
    cx: &mut ViewCtx<'_>,
    page_id: egui::Id,
) {
    let typing = ui
        .memory(|m| m.focused())
        .is_some_and(|id| egui::text_edit::TextEditState::load(ui.ctx(), id).is_some());
    if typing || cx.insert.menu.open || cx.diag.menu_open() {
        return;
    }
    let forward = ui.input_mut(|i| {
        if i.consume_key(egui::Modifiers::NONE, Key::F8) {
            Some(true)
        } else if i.consume_key(egui::Modifiers::SHIFT, Key::F8) {
            Some(false)
        } else {
            None
        }
    });
    let Some(forward) = forward else {
        return;
    };
    if let Some(p) = dui::next_problem(&pc.problems, doc.editor.caret(), forward) {
        doc.editor.move_caret(dui::caret_target(&pc.page, p), false);
        doc.scroll_to = Some(p.span);
        cx.diag.peek = Some(p.span);
        doc.want_x = None;
    }
    ui.memory_mut(|m| m.request_focus(page_id));
}

/// The text size of problem cards and menus.
fn card_size(page_size: f32) -> f32 {
    (page_size * 0.85).max(14.0)
}

/// Right-click: puts the caret there and offers Insert.
fn context_menu(
    ui: &Ui,
    resp: &egui::Response,
    doc: &mut Doc,
    pc: &PageCache,
    origin: Pos2,
    cx: &mut ViewCtx<'_>,
    events: &mut Vec<ViewEvent>,
) {
    let fig_key = resp.id.with("figure-menu");
    let result_key = resp.id.with("result-menu");
    if resp.secondary_clicked() {
        let line = resp
            .interact_pointer_pos()
            .and_then(|p| result_at(doc, pc, origin, p));
        ui.data_mut(|d| d.insert_temp(result_key, line));
        let fig = resp
            .interact_pointer_pos()
            .and_then(|p| figure_at(pc, origin, p));
        ui.data_mut(|d| d.insert_temp(fig_key, fig));
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
        let line = ui
            .data(|d| d.get_temp::<Option<(BlockId, usize)>>(result_key))
            .flatten();
        if let Some((block, i)) = line {
            if result_menu(ui, doc, block, i) {
                ui.close();
            }
            ui.separator();
        }
        if ui
            .button(egui::RichText::new("Insert\u{2026}").font(crate::theme::sans(14.0)))
            .clicked()
        {
            cx.insert.menu.open_at(at);
            ui.close();
        }
        if doc.editor.selection().is_some() {
            // Copy as LaTeX, as plain text or as code (copy.rs).
            if let Some(format) = crate::copy::copy_menu(ui) {
                copy_selection_as(doc, pc, cx, ui, format);
                ui.close();
            }
        }
        let result = crate::proof::result_at(&pc.doc, doc.editor.caret());
        if let Some(name) = result {
            ui.separator();
            if ui
                .button(egui::RichText::new("Prove\u{2026}").font(crate::theme::sans(14.0)))
                .on_hover_text(format!("Ask the proof agent to prove {name}"))
                .clicked()
            {
                events.push(ViewEvent::Prove(name.clone()));
                ui.close();
            }
            if ui
                .button(
                    egui::RichText::new("Check against Lean\u{2026}")
                        .font(crate::theme::sans(14.0)),
                )
                .clicked()
            {
                events.push(ViewEvent::AlignLean(name));
                ui.close();
            }
        }
        let fig = ui.data(|d| d.get_temp::<Option<usize>>(fig_key)).flatten();
        if let Some(idx) = fig {
            if ui
                .button(egui::RichText::new("Open in tile").font(crate::theme::sans(14.0)))
                .clicked()
            {
                events.push(ViewEvent::OpenFigureTile(idx));
                ui.close();
            }
        }
    });
}

/// The right-click items of a result: exact or decimal, more or fewer digits,
/// and keeping what is shown. True when an item was chosen.
fn result_menu(ui: &mut Ui, doc: &mut Doc, block: BlockId, i: usize) -> bool {
    let Some(v) = doc
        .eval
        .as_ref()
        .and_then(|ev| ev.block(block))
        .and_then(|b| b.values.get(i))
        .cloned()
    else {
        return false;
    };
    let item = |ui: &mut Ui, text: &str| {
        ui.button(egui::RichText::new(text).font(crate::theme::sans(14.0)))
            .clicked()
    };
    let numeric = v.value.is_some() && v.verdict.is_none();
    let current = doc
        .show()
        .format_for(v.span)
        .copied()
        .or_else(|| v.format().copied())
        .unwrap_or_default();
    let mut picked = None;
    if numeric {
        let decimal = current.style != mpp_core::display::Style::Exact;
        if item(
            ui,
            if decimal {
                "Show exactly"
            } else {
                "Show as a decimal"
            },
        ) {
            picked = Some(mpp_eval::numfmt::toggle_exact_decimal(&current));
        }
        if item(ui, "More digits") {
            picked = Some(mpp_eval::numfmt::more_digits(&current));
        }
        if item(ui, "Fewer digits") {
            picked = Some(mpp_eval::numfmt::fewer_digits(&current));
        }
    }
    if let Some(f) = picked {
        doc.show_mut().set_format(v.span, f);
        return true;
    }
    let keepable = v.auto || doc.show().format_for(v.span).is_some();
    if keepable && item(ui, "Keep this result   Ctrl+Enter") {
        doc.keep_result_at(v.span.start);
        return true;
    }
    false
}

/// The Insert menu, at the caret (or where it was opened).
fn insert_menu(
    ui: &Ui,
    doc: &mut Doc,
    cx: &mut ViewCtx<'_>,
    caret_rect: Option<Rect>,
    page_id: egui::Id,
    events: &mut Vec<ViewEvent>,
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
    cx.insert.menu.citations =
        crate::doc_settings::cite_choices(&mpp_syntax::document::parse_document(doc.editor.text()));
    match insert::menu::show(ui.ctx(), &mut cx.insert.menu, &recent, anchor, cx.mode) {
        insert::menu::Outcome::Cite { key } => {
            doc.editor.insert_text(&crate::doc_settings::citation(&key));
            cx.insert.settings.touch("doc.citation");
            doc.touched();
            ui.memory_mut(|m| m.request_focus(page_id));
        }
        insert::menu::Outcome::Insert { id, knobs } => {
            if let Some(t) = insert::find(&id) {
                let caret = doc.editor.caret();
                let bcx = cx.insert.settings.build_cx(doc.editor.text(), caret);
                let pc = doc.page(ui.available_width().max(200.0), cx.size, cx.mode);
                insert::insert(&mut doc.editor, &pc.page, t, knobs, &bcx);
                cx.insert.settings.touch(&id);
                doc.touched();
                if id == "doc.conjecture" {
                    events.push(ViewEvent::Conjecture);
                }
            }
            ui.memory_mut(|m| m.request_focus(page_id));
        }
        insert::menu::Outcome::Closed => ui.memory_mut(|m| m.request_focus(page_id)),
        insert::menu::Outcome::Nothing => {}
    }
}

/// The command list at `\` and `/` (U22), under the caret.
fn command_list(ui: &Ui, doc: &mut Doc, pc: &PageCache, cx: &mut ViewCtx<'_>, caret: Option<Rect>) {
    cx.insert
        .commands
        .refresh(doc.editor.text(), doc.editor.caret());
    let Some(cr) = caret else {
        return;
    };
    let at = Pos2::new(cr.left(), cr.bottom() + 2.0);
    if let Some(i) = insert::commands::popup(ui.ctx(), &cx.insert.commands, at, cx.mode) {
        cx.insert.commands.selected = i;
        if insert::commands::accept(&mut doc.editor, &pc.page, cx.insert) {
            doc.touched();
        }
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
    let mut controls = insert::controls(&s.template, s.knobs);
    insert::mark_current(&mut controls, &s.contents(doc.editor.text()));
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
    // The box being typed in, which the toolbar must not cover (an empty box in a
    // line of prose is not inside the template's own boxes).
    let r = match pc.page.rect_of_span(mpp_syntax::Span::point(caret)) {
        Some(c) if (c.center().y - r.center().y).abs() < r.height() => r.union(c),
        _ => r,
    };
    // A figure line starts with words that have no box of their own
    // ("Parametric", "Slope field"): the toolbar must not cover them either.
    let r = if insert::find(&s.template).is_some_and(|t| t.is_figure()) {
        pc.page
            .line_ink(mpp_syntax::Span::new(range.start, range.end), r)
    } else {
        r
    };
    let rect = r.translate(origin.to_vec2());
    if let Some(i) = insert::menu::toolbar(ui.ctx(), &controls, rect, ui.clip_rect()) {
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
    if focused {
        crate::editor::brackets::paint(painter, page, origin, &doc.editor, cx.size, cx.mode);
    }
    let clip = painter.clip_rect();
    for b in &page.blocks {
        let top = origin.y + b.top;
        if top > clip.bottom() || top + b.height < clip.top() {
            continue;
        }
        let at = Pos2::new(origin.x, top);
        paint_display_list(painter, &b.dl, at);
    }
    // Problems: underlines under the typeset element, a bar in the margin,
    // the import summaries and the bulb of the caret's problem.
    dui::paint_marks(painter, &pc.problems, off, theme);
    dui::paint_margin(painter, page, &pc.problems, origin, theme);
    crate::proof::marks::paint_marks(painter, page, &pc.proof.steps, origin, theme, cx.size);
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
        // Only the equations on screen (a long file has hundreds, and finding a hint
        // reads the editor's template boxes each time).
        let top = origin.y + b.top;
        if b.tag != BlockTag::Math || top > clip.bottom() || top + b.height < clip.top() {
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
        // Beside the equation when there is room; otherwise on its own line in the
        // gap under the block, so it never lands on the words or runs off the page.
        let beside = right + cx.size + w <= page.width;
        let x = if beside {
            right + cx.size
        } else {
            (page.width - w).clamp(0.0, cx.size)
        };
        let y = if beside {
            origin.y + b.top + b.height / 2.0 + size * 0.3
        } else {
            origin.y + b.top + b.height + size * 0.62
        };
        paint_text(
            painter,
            &hint,
            Pos2::new(origin.x + x, y),
            size,
            mpp_render::FontId::TextItalic,
            c32(theme.placeholder_label),
        );
    }
    // The one-time hint of the last edit, under the caret.
    if let (Some(h), Some(s)) = (doc.editor.hint(), page.caret_stop(doc.editor.caret())) {
        let at = Pos2::new(origin.x + s.x + 12.0, origin.y + s.bottom + 6.0);
        crate::paint::paint_text(
            painter,
            h,
            at,
            cx.size * 0.8,
            mpp_render::FontId::TextItalic,
            c32(theme.placeholder_label),
        );
    }
    // Caret.
    if focused {
        if let Some(s) = page.caret_stop_on(doc.editor.caret(), doc.caret_side) {
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
        if let Some((o, side)) = pc.page.side_at(local) {
            if command {
                events.push(ViewEvent::GotoAt(o));
            } else if !shift {
                events.push(ViewEvent::OpenLinkAt(o));
            }
            doc.editor.move_caret(o, shift);
            doc.caret_side = side;
            doc.want_x = None;
            return true;
        }
    } else if resp.dragged() {
        if let Some((o, side)) = pc.page.side_at(local) {
            doc.editor.move_caret(o, true);
            doc.caret_side = side;
            return true;
        }
    }
    false
}

/// Applies typed text one character at a time (so live replacement sees each).
fn type_text(doc: &mut Doc, pc: &PageCache, cx: &mut ViewCtx<'_>, text: &str) {
    let in_math = pc.page.in_math(doc.editor.caret());
    for ch in text.chars().filter(|c| !c.is_control()) {
        if in_math && doc.editor.bracket_before(ch) {
            // Brackets (editor/brackets.rs): wrap a selection, fill a ghost closer.
            continue;
        }
        if in_math {
            // Both ways of typing, converted as you type (editor/convert.rs).
            let typing = crate::editor::convert::TypingCx {
                build: cx
                    .insert
                    .settings
                    .build_cx(doc.editor.text(), doc.editor.caret()),
                shortcuts: cx.insert.settings.shortcuts,
            };
            doc.editor.type_maths(ch, &typing);
            doc.editor.bracket_after(ch);
            continue;
        }
        doc.editor.before_type(ch);
        doc.editor.insert_text(&ch.to_string());
        doc.editor.after_type(ch);
    }
    doc.touched();
    doc.want_x = None;
    doc.caret_side = crate::page::Side::Before;
    cx.diag.peek = None;
    update_completion(doc, in_math, cx);
    let caret = doc.editor.caret();
    cx.insert
        .commands
        .typed(doc.editor.text(), caret, in_math, text.chars().next_back());
    if cx.insert.commands.open {
        *cx.completion = None;
    }
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
    let mut names: Vec<String> = doc
        .eval
        .as_ref()
        .map(|e| e.user_names())
        .unwrap_or_default();
    // Names from the standard library come after the file's own.
    names.extend(crate::library::names().iter().cloned());
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
    let format = cx.copy_format;
    copy_selection_as(doc, pc, cx, ui, format);
}

fn copy_selection_as(doc: &Doc, pc: &PageCache, cx: &mut ViewCtx<'_>, ui: &Ui, format: CopyFormat) {
    let Some(sel) = doc.editor.selection() else {
        return;
    };
    let Some(src) = doc.editor.text().get(sel.clone()) else {
        return;
    };
    let all_maths = pc.page.in_math(sel.start) && pc.page.in_math(sel.end);
    let out = clipboard_text(src, all_maths, format);
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
    let revision = doc.editor.revision();
    let menu = ui
        .data(|d| d.get_temp::<bool>(egui::Id::unique(POPUP_OPEN)))
        .unwrap_or(false);
    for ev in input {
        // The go-to-definition highlight lasts until the next key or edit.
        if matches!(
            ev,
            Event::Text(_) | Event::Paste(_) | Event::Cut | Event::Key { pressed: true, .. }
        ) && !matches!(ev, Event::Key { key: Key::F12, .. })
        {
            doc.highlight = None;
        }
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
                if key == Key::Escape && menu {
                    // The menu that was open closes on this Esc; nothing else.
                    continue;
                }
                let horizontal = matches!(key, Key::ArrowLeft | Key::ArrowRight);
                if horizontal && !modifiers.any() && doc.editor.arrow_out(key == Key::ArrowRight) {
                    // At the edge of a template box: next box, or out (editor/exits.rs).
                    *cx.completion = None;
                    doc.caret_side = Side::Before;
                    doc.want_x = None;
                    moved = true;
                    continue;
                }
                if cx.completion.is_none() && !cx.insert.commands.open {
                    if let Some(dir) = edge_release(doc, pc, key, modifiers) {
                        ui.memory_mut(|m| m.move_focus(dir));
                        continue;
                    }
                }
                moved |= key_pressed(doc, pc, cx, events, key, modifiers);
            }
            _ => {}
        }
    }
    if doc.editor.revision() != revision {
        // After an edit the caret stays with what was just typed or kept.
        doc.caret_side = crate::page::Side::Before;
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
    if !mods.command && !mods.alt {
        use crate::insert::commands::{accept, KeyOutcome};
        match cx.insert.commands.key(key, shift) {
            KeyOutcome::Accept => {
                if accept(&mut doc.editor, page, cx.insert) {
                    doc.touched();
                    doc.want_x = None;
                }
                return true;
            }
            KeyOutcome::Handled => return false,
            KeyOutcome::Ignored => {}
        }
    }
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
                    doc.caret_side = Side::After;
                }
                doc.want_x = None;
                return true;
            }
            Key::End => {
                if let Some((_, hi)) = page.offset_extent() {
                    doc.editor.move_caret(hi, shift);
                    doc.caret_side = Side::Before;
                }
                doc.want_x = None;
                return true;
            }
            Key::Enter => return doc.keep_result_at(caret),
            Key::Slash => return toggle_comment(doc),
            Key::Num0 | Key::Num1 | Key::Num2 | Key::Num3 => {
                let level = match key {
                    Key::Num1 => 1,
                    Key::Num2 => 2,
                    Key::Num3 => 3,
                    _ => 0,
                };
                let done = doc.editor.set_heading(level, in_math);
                if done {
                    doc.touched();
                    doc.want_x = None;
                }
                return done;
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
        Key::Escape => escape(doc, page, cx),
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
            if note_enter(doc) {
                return true;
            }
            doc.editor.enter(in_math, shift);
            doc.touched();
            true
        }
        Key::Backspace | Key::Delete => {
            let fwd = key == Key::Delete;
            if !fwd && !in_math && doc.editor.backspace_block(false) {
                doc.touched();
                *cx.completion = None;
                doc.want_x = None;
                return true;
            }
            if !fwd && in_math && doc.editor.backspace_pair() {
                doc.touched();
                *cx.completion = None;
                return true;
            }
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
                doc.caret_side = if fwd { Side::Before } else { Side::After };
            } else if shift
                && in_math
                && crate::editor::select::shift_arrow(&mut doc.editor, page, fwd)
            {
                doc.caret_side = if fwd { Side::Before } else { Side::After };
            } else if let Some((t, side)) = page.step_on(caret, doc.caret_side, fwd) {
                doc.editor.move_caret(t, shift);
                doc.caret_side = side;
            }
            true
        }
        Key::ArrowUp if mods.alt && shift && in_math => {
            crate::editor::select::grow_selection(&mut doc.editor, page);
            doc.want_x = None;
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
                doc.want_x = page.caret_stop_on(caret, doc.caret_side).map(|s| s.x);
            }
            if let Some((t, side)) = page.vertical_on(caret, doc.caret_side, up, doc.want_x) {
                doc.editor.move_caret(t, shift);
                doc.caret_side = side;
            } else if let Some((lo, hi)) = page.line_extent_on(caret, doc.caret_side) {
                // No line above (below): to the start (end) of this one.
                doc.editor.move_caret(if up { lo } else { hi }, shift);
                doc.caret_side = if up { Side::After } else { Side::Before };
            }
            true
        }
        Key::Home | Key::End => {
            if let Some((lo, hi)) = page.line_extent_on(caret, doc.caret_side) {
                let home = key == Key::Home;
                doc.editor.move_caret(if home { lo } else { hi }, shift);
                doc.caret_side = if home { Side::After } else { Side::Before };
            }
            doc.want_x = None;
            true
        }
        _ => false,
    }
}

/// Tab outside a tracked template: the next (or previous) empty box on the page.
/// Ctrl+/: comments the current block (or the selected blocks) out, or takes
/// the comment markers off a note.
pub fn toggle_comment(doc: &mut Doc) -> bool {
    let caret = doc.editor.caret();
    let sel = doc.editor.selection();
    let Some((range, new)) = crate::editor::comment::toggle(doc.editor.text(), caret, sel) else {
        return false;
    };
    doc.editor
        .replace_node(mpp_syntax::Span::new(range.start, range.end), &new);
    doc.touched();
    doc.want_x = None;
    true
}

/// Enter at the end of a `% ` note line: a line with words continues the note on
/// a new `% ` line; an empty note line ends the note (its marker goes, leaving a
/// blank line).
fn note_enter(doc: &mut Doc) -> bool {
    let text = doc.editor.text();
    let caret = doc.editor.caret();
    let start = text[..caret].rfind('\n').map_or(0, |i| i + 1);
    let end = text[caret..].find('\n').map_or(text.len(), |i| caret + i);
    let line = &text[start..end];
    if !(line == "%" || line.starts_with("% ")) || caret != end {
        return false;
    }
    if line.trim_end() == "%" {
        doc.editor
            .replace_node(mpp_syntax::Span::new(start, end), "");
        doc.editor.insert_text("\n");
    } else {
        doc.editor.insert_text("\n% ");
    }
    doc.touched();
    true
}

fn tab_to_slot(doc: &mut Doc, page: &crate::page::Page, forward: bool) {
    let caret = doc.editor.caret();
    let slots = empty_slots(page, caret);
    let target = if forward {
        slots.into_iter().find(|&o| o > caret)
    } else {
        slots.into_iter().rev().find(|&o| o < caret)
    };
    if let Some(o) = target {
        doc.editor.move_caret(o, false);
    }
}

/// The card of the proof status mark under the pointer, if any. True when shown.
fn proof_mark_card(ui: &Ui, pc: &PageCache, origin: Pos2, theme: &Theme, size: f32) -> bool {
    let Some(pos) = ui.ctx().pointer_hover_pos() else {
        return false;
    };
    let clip = ui.clip_rect().left();
    let marks = crate::proof::marks::mark_rects(&pc.page, &pc.proof.steps, origin, clip, size);
    let Some((_, s)) = marks.iter().find(|(r, _)| r.expand(3.0).contains(pos)) else {
        return false;
    };
    crate::proof::marks::mark_card(
        ui.ctx(),
        pos + Vec2::new(14.0, 18.0),
        s,
        theme,
        card_size(size),
    );
    true
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
                // `g := 9.81` needs no second line saying `= 9.81`.
                let repeated = |v: &String| d.definition.ends_with(&format!(" {v}"));
                if let Some(v) = d.value.as_ref().filter(|v| !repeated(v)) {
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
