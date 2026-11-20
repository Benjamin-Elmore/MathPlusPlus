//! The moon button of the status bar. Latin Modern has no moon glyph, so the
//! crescent is drawn as a shape in the current widget colours.

use egui::{Response, Sense, Ui, Vec2};

/// A square button showing a crescent moon.
pub fn moon_button(ui: &mut Ui, size: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size + 8.0), Sense::click());
    let visuals = ui.style().interact(&resp);
    let painter = ui.painter();
    painter.rect(
        rect,
        visuals.corner_radius,
        visuals.bg_fill,
        visuals.bg_stroke,
        egui::StrokeKind::Inside,
    );
    let r = size * 0.42;
    let c = rect.center() - Vec2::new(size * 0.04, 0.0);
    painter.circle_filled(c, r, visuals.fg_stroke.color);
    painter.circle_filled(
        c + Vec2::new(r * 0.55, -r * 0.35),
        r * 0.85,
        visuals.bg_fill,
    );
    resp
}
