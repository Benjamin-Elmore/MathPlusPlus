//! Drawing problems on the page: underlines, the margin bar, scrollbar marks,
//! the hover card, the 💡 and the quick-fix menu.

use egui::{
    Align2, Area, Color32, Context, Frame, Key, Order, Painter, Pos2, Rect, Sense, Shape, Stroke,
    Ui, Vec2,
};
use mpp_render::Theme;
use mpp_syntax::Span;

use super::fixes::QuickFix;
use super::rich::{self, Rich};
use super::{Counts, Kind, Level, Problem};
use crate::page::Page;
use crate::paint::c32;
use crate::theme::Mode;

/// The quick-fix menu.
#[derive(Debug, Clone)]
pub struct FixMenu {
    pub fixes: Vec<QuickFix>,
    pub selected: usize,
    /// Where it opens (screen position).
    pub at: Pos2,
    /// The problem it fixes.
    pub problem: Span,
    /// Set on the frame it opens (the click that opened it is not a click away).
    pub fresh: bool,
}

/// Problem UI state of the equation view.
#[derive(Debug, Clone, Default)]
pub struct DiagUi {
    /// The open quick-fix menu.
    pub menu: Option<FixMenu>,
    /// A problem whose card stays open (after F8), until the next edit or click.
    pub peek: Option<Span>,
    /// Screen position of the page's top-left corner, from the last frame.
    pub origin: Pos2,
}

impl DiagUi {
    /// True while the quick-fix menu has the keyboard.
    pub fn menu_open(&self) -> bool {
        self.menu.is_some()
    }
}

/// A wavy line along the bottom of `rect` (errors and warnings).
pub fn paint_squiggle(painter: &Painter, rect: Rect, color: Color32) {
    let rect = widen(rect);
    let y = rect.bottom() + 1.5;
    let step = 2.5;
    let amp = 1.4;
    let mut pts = vec![];
    let mut x = rect.left();
    let mut up = false;
    while x <= rect.right() {
        pts.push(Pos2::new(x, y + if up { -amp } else { amp }));
        up = !up;
        x += step;
    }
    if pts.len() >= 2 {
        painter.add(Shape::line(pts, Stroke::new(1.3, color)));
    }
}

/// A dotted line along the bottom of `rect` (information).
pub fn paint_dotted(painter: &Painter, rect: Rect, color: Color32) {
    let rect = widen(rect);
    let y = rect.bottom() + 1.5;
    let mut x = rect.left();
    while x <= rect.right() {
        painter.circle_filled(Pos2::new(x, y), 0.9, color);
        x += 3.5;
    }
}

/// Tiny targets (a one-letter name, a thin box) get an underline that can be seen.
fn widen(rect: Rect) -> Rect {
    if rect.width() < 10.0 {
        rect.expand2(Vec2::new((10.0 - rect.width()) / 2.0, 0.0))
    } else {
        rect
    }
}

/// Underlines every problem; `off` moves page coordinates to the screen.
pub fn paint_marks(painter: &Painter, problems: &[Problem], off: Vec2, theme: &Theme) {
    // Draw notes first and errors last, so the worst mark is on top.
    let mut order: Vec<&Problem> = problems.iter().collect();
    order.sort_by_key(|p| std::cmp::Reverse(p.level));
    for p in order {
        let color = c32(p.level.color(theme));
        for r in &p.rects {
            let r = r.translate(off);
            match p.level {
                Level::Info => paint_dotted(painter, r, color),
                _ => paint_squiggle(painter, r, color),
            }
        }
    }
}

/// The worst level of the problems in each block, by block span.
pub fn block_levels(problems: &[Problem]) -> Vec<(Span, Level)> {
    let mut out: Vec<(Span, Level)> = vec![];
    for p in problems {
        let Some(b) = p.block else { continue };
        match out.iter_mut().find(|(s, _)| *s == b) {
            Some((_, l)) => *l = (*l).min(p.level),
            None => out.push((b, p.level)),
        }
    }
    out
}

/// The x of the margin bar: left of the column, inside the visible area.
pub fn margin_x(origin: Pos2, clip_left: f32) -> f32 {
    (origin.x - 9.0).max(clip_left + 3.0)
}

/// A bar in the left margin beside each block that has an error or warning.
pub fn paint_margin(
    painter: &Painter,
    page: &Page,
    problems: &[Problem],
    origin: Pos2,
    theme: &Theme,
) {
    for (span, level) in block_levels(problems) {
        if level == Level::Info {
            continue;
        }
        let Some(b) = page.blocks.iter().find(|b| b.span == span) else {
            continue;
        };
        let x = margin_x(origin, painter.clip_rect().left());
        let r = Rect::from_min_max(
            Pos2::new(x - 1.5, origin.y + b.top + 1.0),
            Pos2::new(x + 1.5, origin.y + b.top + b.height.max(8.0) - 1.0),
        );
        painter.rect_filled(r, 1.5, c32(level.color(theme)));
    }
}

/// "2 errors in mechanics.mpp" after an import line with problems.
pub fn paint_import_notes(
    painter: &Painter,
    problems: &[Problem],
    off: Vec2,
    theme: &Theme,
    size: f32,
    max_x: f32,
) {
    for p in problems {
        if !matches!(p.kind, Kind::Import { .. }) {
            continue;
        }
        let Some(last) = p.rects.last() else { continue };
        let r = last.translate(off);
        let size = size * 0.8;
        let w = rich::measure(&p.rich, size, *theme, 600.0).x;
        let x = (r.right() + size).min(max_x - w).max(r.left());
        let y = if x < r.right() {
            r.bottom() + 4.0
        } else {
            r.top() + (r.height() - size) / 2.0
        };
        rich::paint(
            painter,
            &p.rich,
            Pos2::new(x, y),
            size,
            c32(p.level.color(theme)),
            *theme,
            600.0,
        );
    }
}

/// Marks beside the scrollbar for every problem: where it is in the whole page.
pub fn paint_scrollbar_marks(
    painter: &Painter,
    track: Rect,
    page_height: f32,
    problems: &[Problem],
    theme: &Theme,
) {
    if page_height <= 0.0 || track.height() <= 0.0 {
        return;
    }
    let mut order: Vec<&Problem> = problems.iter().collect();
    order.sort_by_key(|p| std::cmp::Reverse(p.level));
    for p in order {
        if !p.rect.is_positive() && p.rect.height() <= 0.0 {
            continue;
        }
        let t = (p.rect.center().y / page_height).clamp(0.0, 1.0);
        let y = track.top() + t * track.height();
        let r = Rect::from_center_size(
            Pos2::new(track.center().x, y),
            Vec2::new(track.width(), 3.0),
        );
        painter.rect_filled(r, 1.0, c32(p.level.color(theme)));
    }
}

/// The icon of a level: a circle with ×, a triangle with !, a circle with i.
pub fn paint_level_icon(painter: &Painter, center: Pos2, r: f32, level: Level, theme: &Theme) {
    let color = c32(level.color(theme));
    let bg = c32(theme.background);
    match level {
        Level::Error => {
            painter.circle_filled(center, r, color);
            let d = r * 0.42;
            let s = Stroke::new(r * 0.28, bg);
            painter.line_segment([center + Vec2::new(-d, -d), center + Vec2::new(d, d)], s);
            painter.line_segment([center + Vec2::new(-d, d), center + Vec2::new(d, -d)], s);
        }
        Level::Warning => {
            let pts = vec![
                center + Vec2::new(0.0, -r),
                center + Vec2::new(r * 1.05, r * 0.8),
                center + Vec2::new(-r * 1.05, r * 0.8),
            ];
            painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
            let s = Stroke::new(r * 0.24, bg);
            painter.line_segment(
                [
                    center + Vec2::new(0.0, -r * 0.4),
                    center + Vec2::new(0.0, r * 0.25),
                ],
                s,
            );
            painter.circle_filled(center + Vec2::new(0.0, r * 0.52), r * 0.13, bg);
        }
        Level::Info => {
            painter.circle_filled(center, r, color);
            let s = Stroke::new(r * 0.26, bg);
            painter.line_segment(
                [
                    center + Vec2::new(0.0, -r * 0.1),
                    center + Vec2::new(0.0, r * 0.55),
                ],
                s,
            );
            painter.circle_filled(center + Vec2::new(0.0, -r * 0.45), r * 0.14, bg);
        }
    }
}

/// Draws a light bulb (the quick-fix sign) centred at `c`.
pub fn paint_bulb(painter: &Painter, c: Pos2, r: f32, color: Color32, ink: Color32) {
    painter.circle_filled(c + Vec2::new(0.0, -r * 0.25), r * 0.7, color);
    let base = Rect::from_center_size(c + Vec2::new(0.0, r * 0.6), Vec2::new(r * 0.75, r * 0.45));
    painter.rect_filled(base, 1.0, ink);
    painter.line_segment(
        [
            c + Vec2::new(-r * 0.35, r * 0.95),
            c + Vec2::new(r * 0.35, r * 0.95),
        ],
        Stroke::new(1.0, ink),
    );
}

/// The colour of the bulb.
pub fn bulb_color(mode: Mode) -> Color32 {
    c32(mode.palette().bulb)
}

/// Radius of the bulb on the page.
pub const BULB_RADIUS: f32 = 6.5;

/// Where the 💡 of a problem goes (page coordinates): left of the maths when
/// the block has room there (display maths is centred), otherwise just above
/// the start of the faulty element.
pub fn bulb_center(page: &Page, p: &Problem) -> Option<Pos2> {
    let block = page.blocks.iter().find(|b| Some(b.span) == p.block)?;
    let ink_left = block
        .dl
        .hits
        .iter()
        .map(|h| h.rect.x)
        .fold(f32::INFINITY, f32::min);
    if ink_left.is_finite() && ink_left >= 4.0 * BULB_RADIUS {
        Some(Pos2::new(ink_left - 2.2 * BULB_RADIUS, p.rect.center().y))
    } else {
        Some(Pos2::new(
            p.rect.left() + BULB_RADIUS * 0.5,
            p.rect.top() - BULB_RADIUS * 1.2,
        ))
    }
}

/// The problem the caret is on: one whose span holds the caret, otherwise the
/// first one in the caret's block.
pub fn problem_at<'a>(problems: &'a [Problem], page: &Page, caret: usize) -> Option<&'a Problem> {
    problems
        .iter()
        .filter(|p| p.span.start <= caret && caret <= p.span.end)
        .min_by_key(|p| (p.level, p.span.len()))
        .or_else(|| {
            let b = page.block_at(caret)?;
            problems
                .iter()
                .filter(|p| p.block == Some(b.span))
                .min_by_key(|p| (p.level, p.span.start.abs_diff(caret)))
        })
}

/// The next (or previous) problem after the caret, wrapping around the file.
/// A caret inside a problem counts as being at that problem.
pub fn next_problem(problems: &[Problem], caret: usize, forward: bool) -> Option<&Problem> {
    let mut v: Vec<&Problem> = problems.iter().collect();
    v.sort_by_key(|p| (p.span.start, p.level));
    v.dedup_by_key(|p| p.span.start);
    let current = v
        .iter()
        .position(|p| p.span.start <= caret && caret <= p.span.end.max(p.span.start + 1));
    if forward {
        let after = |p: &&&Problem| p.span.start > caret;
        match current {
            Some(i) => v.get(i + 1).or(v.first()).copied(),
            None => v.iter().find(after).or(v.first()).copied(),
        }
    } else {
        match current {
            Some(0) => v.last().copied(),
            Some(i) => v.get(i - 1).copied(),
            None => v
                .iter()
                .rev()
                .find(|p| p.span.start < caret)
                .or(v.last())
                .copied(),
        }
    }
}

/// Where the caret goes for a problem: the first caret stop inside its span
/// (an opening bracket has no stop of its own), else the stop nearest its box.
pub fn caret_target(page: &Page, p: &Problem) -> usize {
    page.stops
        .iter()
        .map(|s| s.offset)
        .filter(|o| p.span.start <= *o && *o <= p.span.end)
        .min()
        .or_else(|| page.offset_at(p.rect.left_center()))
        .unwrap_or(p.span.start)
}

/// The words for a level in a card's header.
fn level_title(level: Level) -> &'static str {
    match level {
        Level::Error => "Error",
        Level::Warning => "Warning",
        Level::Info => "Note",
    }
}

/// Width of a hover card.
const CARD_WIDTH: f32 = 440.0;

/// The contents of a problem card: icon and message, what was expected, how to
/// fix it, and the quick-fix hint.
pub fn card_contents(ui: &mut Ui, p: &Problem, theme: &Theme, mode: Mode, size: f32, fixes: usize) {
    let ink = rich::ink(theme);
    let muted = c32(theme.muted);
    ui.set_max_width(CARD_WIDTH);
    ui.horizontal(|ui| {
        let (r, painter) = ui.allocate_painter(Vec2::splat(size), Sense::hover());
        paint_level_icon(&painter, r.rect.center(), size * 0.36, p.level, theme);
        let title = Rich::default().i(level_title(p.level).to_string());
        rich::show(ui, &title, size * 0.8, c32(p.level.color(theme)), *theme);
    });
    ui.add_space(2.0);
    rich::show(ui, &p.rich, size, ink, *theme);
    if let Some(e) = &p.expected {
        ui.add_space(4.0);
        rich::show(ui, e, size * 0.85, muted, *theme);
    }
    if let Some(f) = &p.fix {
        ui.add_space(4.0);
        let line = Rich::default().i("How to fix: ".to_string()).and(f.clone());
        rich::show(ui, &line, size * 0.85, muted, *theme);
    }
    if fixes > 0 {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let (r, painter) = ui.allocate_painter(Vec2::splat(size * 0.9), Sense::hover());
            paint_bulb(
                &painter,
                r.rect.center(),
                size * 0.32,
                bulb_color(mode),
                muted,
            );
            let hint = Rich::text(if fixes == 1 {
                "1 quick fix: press Ctrl+. or click the bulb".to_string()
            } else {
                format!("{fixes} quick fixes: press Ctrl+. or click the bulb")
            });
            rich::show(ui, &hint, size * 0.8, muted, *theme);
        });
    }
}

/// The look of a problem card.
#[derive(Debug, Clone, Copy)]
pub struct CardStyle {
    pub theme: Theme,
    pub mode: Mode,
    /// Text size of the message.
    pub size: f32,
}

/// Shows a problem card at a screen position.
pub fn show_card(
    ctx: &Context,
    id: &str,
    p: &Problem,
    at: Pos2,
    style: &CardStyle,
    fixes: usize,
) -> Rect {
    Area::new(egui::Id::unique(id))
        .order(Order::Tooltip)
        .fixed_pos(at)
        .interactable(false)
        .show(ctx, |ui| {
            Frame::popup(ui.style())
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    card_contents(ui, p, &style.theme, style.mode, style.size, fixes)
                });
        })
        .response
        .rect
}

/// What the quick-fix menu did this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuOutcome {
    Pick(usize),
    Close,
    Nothing,
}

/// The quick-fix menu: arrows choose, Enter applies, Escape closes.
pub fn show_menu(
    ctx: &Context,
    menu: &mut FixMenu,
    theme: &Theme,
    mode: Mode,
    size: f32,
) -> MenuOutcome {
    let n = menu.fixes.len();
    if n == 0 {
        return MenuOutcome::Close;
    }
    let (up, down, enter, esc) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
            i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
            i.consume_key(egui::Modifiers::NONE, Key::Enter),
            i.consume_key(egui::Modifiers::NONE, Key::Escape),
        )
    });
    if up {
        menu.selected = (menu.selected + n - 1) % n;
    }
    if down {
        menu.selected = (menu.selected + 1) % n;
    }
    if enter {
        return MenuOutcome::Pick(menu.selected);
    }
    if esc {
        return MenuOutcome::Close;
    }
    let mut out = MenuOutcome::Nothing;
    let row_bg = c32(mode.palette().row_selected);
    let ink = rich::ink(theme);
    let resp = Area::new(egui::Id::unique("mpp-quick-fix-menu"))
        .order(Order::Foreground)
        .fixed_pos(menu.at)
        .pivot(Align2::LEFT_TOP)
        .show(ctx, |ui| {
            Frame::popup(ui.style())
                .inner_margin(egui::Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (r, painter) = ui.allocate_painter(Vec2::splat(size), Sense::hover());
                        paint_bulb(
                            &painter,
                            r.rect.center(),
                            size * 0.33,
                            bulb_color(mode),
                            c32(theme.muted),
                        );
                        let t = Rich::default().i("Quick fixes".to_string());
                        rich::show(ui, &t, size * 0.8, c32(theme.muted), *theme);
                    });
                    ui.add_space(2.0);
                    for (i, f) in menu.fixes.iter().enumerate() {
                        let backdrop = ui.painter().add(Shape::Noop);
                        let row = ui.horizontal(|ui| {
                            ui.set_min_width(240.0);
                            ui.add_space(4.0);
                            rich::show(ui, &f.title, size, ink, *theme);
                        });
                        let r = row.response.rect.expand2(Vec2::new(4.0, 2.0));
                        let hit =
                            ui.interact(r, ui.scope_id().with(("mpp-fix", i)), Sense::click());
                        if hit.hovered() {
                            menu.selected = i;
                        }
                        if i == menu.selected {
                            ui.painter()
                                .set(backdrop, Shape::rect_filled(r, 3.0, row_bg));
                        }
                        if hit.clicked() {
                            out = MenuOutcome::Pick(i);
                        }
                    }
                });
        });
    let fresh = std::mem::take(&mut menu.fresh);
    if out == MenuOutcome::Nothing && !fresh && resp.response.clicked_elsewhere() {
        return MenuOutcome::Close;
    }
    out
}

/// The status bar text for a set of counts.
pub fn status_text(c: &Counts) -> String {
    c.summary()
}
