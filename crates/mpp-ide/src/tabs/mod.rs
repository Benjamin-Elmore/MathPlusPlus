//! Tabs: the strip above the page, back/forward history and breadcrumbs.

pub mod breadcrumbs;
pub mod nav;

use egui::{Color32, FontId, PointerButton, Rect, Role, Sense, Ui, Vec2, WidgetInfo};
use mpp_render::Theme;

use crate::explorer::icons;
use crate::explorer::panel::text_galley;
use crate::paint::c32;

/// What the user did in the strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabAction {
    Select(usize),
    Close(usize),
    /// A tab was dragged to a new place.
    Move {
        from: usize,
        to: usize,
    },
}

/// One tab to draw.
pub struct Tab {
    pub title: String,
    pub dirty: bool,
    pub errors: usize,
    /// A preview tab (shown slanted); it is replaced by the next preview.
    pub preview: bool,
}

const TAB_H: f32 = 30.0;

/// Draws the tabs; returns what happened.
pub fn show(ui: &mut Ui, tabs: &[Tab], active: usize, theme: &Theme) -> Option<TabAction> {
    let mut action = None;
    let ink = c32(theme.text);
    let muted = c32(theme.muted);
    let bg = c32(theme.background);
    egui::ScrollArea::horizontal()
        .id_salt("tab-strip")
        .auto_shrink([false, true])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 1.0;
                for (i, t) in tabs.iter().enumerate() {
                    let color = if t.errors > 0 { c32(theme.error) } else { ink };
                    let sample = ui.painter().layout_no_wrap(
                        t.title.clone(),
                        FontId::proportional(15.0),
                        color,
                    );
                    let width = sample.size().x + 62.0;
                    let (rect, resp) =
                        ui.allocate_exact_size(Vec2::new(width, TAB_H), Sense::click_and_drag());
                    resp.widget_info(|| {
                        WidgetInfo::selected(
                            Role::Tab,
                            true,
                            i == active,
                            format!("tab {}", t.title),
                        )
                    });
                    let p = ui.painter_at(rect);
                    let selected = i == active;
                    let fill = if selected {
                        bg
                    } else if resp.hovered() {
                        c32(theme.code_background)
                    } else {
                        c32(theme.code_background).gamma_multiply(0.6)
                    };
                    p.rect_filled(rect, 0.0, fill);
                    if selected {
                        p.line_segment(
                            [rect.left_top(), rect.right_top()],
                            egui::Stroke::new(2.0, c32(theme.link)),
                        );
                    }
                    let galley = text_galley(&p, &t.title, 15.0, color, t.preview);
                    p.galley(
                        egui::pos2(rect.left() + 12.0, rect.center().y - galley.size().y / 2.0),
                        galley,
                        color,
                    );
                    let close = Rect::from_center_size(
                        egui::pos2(rect.right() - 14.0, rect.center().y),
                        Vec2::splat(18.0),
                    );
                    let over_close = ui
                        .input(|inp| inp.pointer.latest_pos())
                        .is_some_and(|pos| close.contains(pos));
                    if t.dirty && !(resp.hovered() && over_close) {
                        icons::dot(&p, close, ink);
                    } else if selected || resp.hovered() {
                        icons::cross(&p, close, if over_close { ink } else { muted });
                    }
                    if resp.dragged() {
                        resp.dnd_set_drag_payload(i);
                    }
                    if let Some(from) = resp.dnd_release_payload::<usize>() {
                        action = Some(TabAction::Move { from: *from, to: i });
                    }
                    if resp.dnd_hover_payload::<usize>().is_some() {
                        p.line_segment(
                            [rect.left_top(), rect.left_bottom()],
                            egui::Stroke::new(2.0, c32(theme.link)),
                        );
                    }
                    if resp.clicked_by(PointerButton::Middle) {
                        action = Some(TabAction::Close(i));
                    } else if resp.clicked() {
                        let on_x = resp
                            .interact_pointer_pos()
                            .is_some_and(|pos| close.contains(pos));
                        action = Some(if on_x {
                            TabAction::Close(i)
                        } else {
                            TabAction::Select(i)
                        });
                    }
                }
            });
        });
    let line_y = ui.min_rect().bottom();
    ui.painter().line_segment(
        [
            egui::pos2(ui.min_rect().left(), line_y),
            egui::pos2(ui.max_rect().right(), line_y),
        ],
        egui::Stroke::new(1.0, c32(theme.rule)),
    );
    let _: Color32 = ink;
    action
}

/// Where a dragged tab ends up when `from` is dropped on `to`: the new order
/// of indices and the new active index.
pub fn reorder(len: usize, from: usize, to: usize, active: usize) -> (Vec<usize>, usize) {
    let mut order: Vec<usize> = (0..len).collect();
    if from >= len || to >= len || from == to {
        return (order, active);
    }
    let item = order.remove(from);
    order.insert(to, item);
    let new_active = order.iter().position(|&o| o == active).unwrap_or(0);
    (order, new_active)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dragging_a_tab_keeps_the_active_one_active() {
        let (order, active) = reorder(4, 0, 2, 0);
        assert_eq!(order, vec![1, 2, 0, 3]);
        assert_eq!(active, 2);
        let (order, active) = reorder(3, 2, 0, 1);
        assert_eq!(order, vec![2, 0, 1]);
        assert_eq!(active, 2);
        assert_eq!(reorder(3, 5, 0, 1).0, vec![0, 1, 2]);
    }
}
