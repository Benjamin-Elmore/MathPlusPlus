//! Hover help (U25): the cards over an answer and over a red error.
//!
//! An answer's card shows its exact and decimal forms and a "Show steps" link.
//! An error's card says what is wrong in plain words and offers the quick fix
//! as a button ("x isn't defined", then "Define x"), so a fix is one click. The
//! cards can be clicked, so a card stays up while the pointer is on it or on
//! the way to it (the pointer crosses a small gap from the text to the card).

use egui::{Area, Context, Frame, Id, Order, Pos2, Rect, Ui, Vec2};
use mpp_render::Theme;
use mpp_syntax::{document::BlockId, Span};

use crate::diagnostics::fixes::{self, Applied, QuickFix};
use crate::diagnostics::rich;
use crate::diagnostics::ui as dui;
use crate::doc::{Doc, PageCache};
use crate::paint::c32;
use crate::theme::Mode;
use crate::typeset;
use crate::view::ViewEvent;

/// What a card is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Problem(Span),
    /// The answer of line `1` of block `0`.
    Answer(BlockId, usize),
}

/// The card on screen: what it is about, and the screen area in which the
/// pointer keeps it open.
#[derive(Debug, Clone, Copy)]
struct Open {
    target: Target,
    /// Where the card is.
    at: Pos2,
    keep: Rect,
}

fn sticky_id() -> Id {
    Id::NULL.with("mpp-hover-sticky")
}

/// The most fixes a card offers as buttons.
const MAX_BUTTONS: usize = 3;

/// Text size of a card.
fn card_size(page_size: f32) -> f32 {
    (page_size * 0.85).max(14.0)
}

/// The answer under a page point: its block and line.
fn answer_at(doc: &Doc, pc: &PageCache, local: Pos2) -> Option<(BlockId, usize)> {
    let (b, h) = pc.page.hit_at(local)?;
    if h.kind != mpp_render::HitKind::Result {
        return None;
    }
    let br = doc.eval.as_ref()?.block(b.id)?;
    let i = crate::page::result_line_at(br, h.span.start)?;
    let v = br.values.get(i)?;
    let shows_value = v.latex.is_some() || v.approx.is_some() || v.text.is_some();
    (shows_value && matches!(v.kind, mpp_eval::LineKind::Query { .. }) && v.verdict.is_none())
        .then_some((b.id, i))
}

/// The most serious problem under a page point.
fn problem_at(pc: &PageCache, local: Pos2) -> Option<&crate::diagnostics::Problem> {
    pc.problems
        .iter()
        .filter(|p| {
            p.rects
                .iter()
                .any(|r| r.expand2(Vec2::new(2.0, 4.0)).contains(local))
        })
        .min_by_key(|p| (p.level, p.span.len()))
}

/// The span of an answer's line in the current text, when the evaluation is of
/// this text (steps cannot be toggled for older results).
fn line_span(doc: &Doc, block: BlockId, i: usize) -> Option<Span> {
    let ev = doc.eval.as_ref()?;
    if ev.source() != doc.editor.text() {
        return None;
    }
    let v = ev.block(block)?.values.get(i)?;
    Some(v.span)
}

/// Where a `!` goes to make `= ?` into `= ?!`, or the text to put after a line
/// that only ends in `=`. None when the line cannot ask for its working.
fn steps_edit(text: &str, line: Span) -> Option<(usize, &'static str)> {
    let head = text.get(..line.end)?;
    let trimmed = head.trim_end();
    let at = trimmed.len();
    if trimmed.ends_with('?') {
        Some((at, "!"))
    } else if trimmed.ends_with('=') {
        Some((at, " ?!"))
    } else {
        None
    }
}

/// Shows the card for whatever is under the pointer, or the one the pointer is
/// still travelling to. True when a card is up (the page's other cards stay
/// away). `over_page` is whether the page itself is under the pointer.
#[allow(clippy::too_many_arguments)]
pub fn cards(
    ui: &Ui,
    doc: &mut Doc,
    pc: &PageCache,
    origin: Pos2,
    theme: &Theme,
    mode: Mode,
    page_size: f32,
    peek: Option<Span>,
    events: &mut Vec<ViewEvent>,
    over_page: bool,
) -> bool {
    let ctx = ui.ctx();
    let Some(pos) = ctx.pointer_hover_pos() else {
        ctx.data_mut(|d| d.remove::<Open>(sticky_id()));
        return false;
    };
    let kept: Option<Open> = ctx
        .data(|d| d.get_temp::<Open>(sticky_id()))
        .filter(|o| o.keep.contains(pos));
    let local = pos - origin.to_vec2();
    let target = if let Some(o) = kept {
        Some(o.target)
    } else if over_page && !ui.input(|i| i.pointer.any_down()) {
        match problem_at(pc, local) {
            Some(p) if peek != Some(p.span) => Some(Target::Problem(p.span)),
            Some(_) => None,
            None => answer_at(doc, pc, local).map(|(b, i)| Target::Answer(b, i)),
        }
    } else {
        None
    };
    let Some(target) = target else {
        ctx.data_mut(|d| d.remove::<Open>(sticky_id()));
        return false;
    };
    // The card sits a few pixels below the pointer when it first shows, and
    // stays where it was while the pointer travels to it.
    let at = kept.map_or(pos + Vec2::new(10.0, 16.0), |o| o.at);
    let shown = match target {
        Target::Problem(span) => problem_card(
            ctx,
            doc,
            pc,
            theme,
            mode,
            page_size,
            span,
            at,
            events,
            kept.is_some(),
        ),
        Target::Answer(block, i) => answer_card(ctx, doc, theme, page_size, block, i, at),
    };
    let Some(card) = shown else {
        ctx.data_mut(|d| d.remove::<Open>(sticky_id()));
        return false;
    };
    // Open while the pointer is on the card or between it and where it showed.
    let keep = if let Some(o) = kept {
        o.keep
    } else {
        card.union(Rect::from_center_size(pos, Vec2::splat(6.0)))
            .expand(4.0)
    };
    ctx.data_mut(|d| d.insert_temp(sticky_id(), Open { target, at, keep }));
    true
}

/// A frame for a card.
fn frame(ui: &Ui) -> Frame {
    Frame::popup(ui.style()).inner_margin(egui::Margin::symmetric(12, 10))
}

/// The card over a red error or other problem, with its fixes as buttons.
#[allow(clippy::too_many_arguments)]
fn problem_card(
    ctx: &Context,
    doc: &mut Doc,
    pc: &PageCache,
    theme: &Theme,
    mode: Mode,
    page_size: f32,
    span: Span,
    at: Pos2,
    events: &mut Vec<ViewEvent>,
    keeping: bool,
) -> Option<Rect> {
    let p = pc.problems.iter().find(|p| p.span == span)?;
    let list = fixes::quick_fixes(p, doc.editor.text(), &pc.doc);
    let size = card_size(page_size);
    let mut chosen: Option<QuickFix> = None;
    let r = Area::new(Id::NULL.with("mpp-problem-card"))
        .order(Order::Tooltip)
        .fixed_pos(at)
        .show(ctx, |ui| {
            frame(ui).show(ui, |ui| {
                dui::card_contents(ui, p, theme, mode, size, 0);
                if !list.is_empty() {
                    ui.add_space(6.0);
                }
                for f in list.iter().take(MAX_BUTTONS) {
                    let row = ui.horizontal(|ui| {
                        ui.set_min_width(200.0);
                        rich::show(ui, &f.title, size, c32(theme.link), *theme)
                    });
                    let resp = row.inner;
                    let label = f.title.plain();
                    resp.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Button, true, label.clone())
                    });
                    if resp.hovered() {
                        ui.painter().rect_filled(
                            row.response.rect,
                            3.0,
                            c32(theme.link).gamma_multiply(0.14),
                        );
                    }
                    if resp
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        chosen = Some(f.clone());
                    }
                }
            })
        })
        .response
        .rect;
    let _ = keeping;
    if let Some(f) = chosen {
        match fixes::apply(&mut doc.editor, &f) {
            Applied::Done => doc.touched(),
            Applied::Open(path) => events.push(ViewEvent::Open(path)),
        }
        ctx.data_mut(|d| d.remove::<Open>(sticky_id()));
        ctx.request_repaint();
        return None;
    }
    Some(r)
}

/// The card over an answer: exact and decimal forms, how to copy, and a link
/// that shows the working.
fn answer_card(
    ctx: &Context,
    doc: &mut Doc,
    theme: &Theme,
    page_size: f32,
    block: BlockId,
    i: usize,
    at: Pos2,
) -> Option<Rect> {
    let ev = doc.eval.clone()?;
    let v = ev.block(block)?.values.get(i)?;
    let exact = v.latex.clone();
    // An exact fraction has no decimal of its own on the page: work it out here.
    let decimal = v.approx.clone().or_else(|| {
        let e = v.value.as_ref()?.expr();
        e.is_constant()
            .then(|| mpp_eval::numeric::approximate(&e).ok())
            .flatten()
    });
    let line = line_span(doc, block, i);
    let has_steps = v.steps.is_some();
    let size = card_size(page_size);
    let unfolded = line.is_some_and(|l| doc.show().is_expanded(l));
    let can_ask = line.is_some_and(|l| steps_edit(doc.editor.text(), l).is_some());
    let steps_label = if unfolded && has_steps {
        "Hide steps"
    } else {
        "Show steps"
    };
    let mut steps_clicked = false;
    let theme = *theme;
    let r = Area::new(Id::NULL.with("mpp-answer-card"))
        .order(Order::Tooltip)
        .fixed_pos(at)
        .show(ctx, |ui| {
            frame(ui).show(ui, |ui| {
                ui.set_max_width(420.0);
                let muted = c32(theme.muted);
                if let Some(e) = &exact {
                    ui.label(egui::RichText::new("Exact").size(size * 0.8).color(muted));
                    typeset::show(ui, e, size, theme);
                }
                if let Some(d) = decimal.as_ref().filter(|d| exact.as_ref() != Some(d)) {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("Decimal").size(size * 0.8).color(muted));
                    typeset::show(ui, d, size, theme);
                }
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Click the answer to copy, Ctrl+click to insert")
                        .size(size * 0.75)
                        .color(muted),
                );
                if line.is_some() && (has_steps || can_ask) {
                    ui.add_space(4.0);
                    let b = ui.add(
                        egui::Button::new(
                            egui::RichText::new(steps_label)
                                .size(size * 0.9)
                                .color(c32(theme.link)),
                        )
                        .frame(false),
                    );
                    if b.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        steps_clicked = true;
                    }
                }
            })
        })
        .response
        .rect;
    if steps_clicked {
        if let Some(l) = line {
            if has_steps {
                doc.show_mut().toggle_steps(l);
            } else if let Some((at, ins)) = steps_edit(doc.editor.text(), l) {
                // The line asks for its working: `= ?` becomes `= ?!`.
                doc.editor.replace_node(Span::point(at), ins);
                doc.touched();
            }
        }
        ctx.data_mut(|d| d.remove::<Open>(sticky_id()));
        ctx.request_repaint();
        return None;
    }
    Some(r)
}
