//! Small painted icons for the explorer and the activity bar (no icon fonts).

use egui::{epaint::PathStroke, Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2};

/// An expand/collapse arrow centred in `rect`.
pub fn arrow(painter: &Painter, rect: Rect, open: bool, color: Color32) {
    let c = rect.center();
    let s = rect.height().min(rect.width()) * 0.22;
    let pts = if open {
        vec![
            c + Vec2::new(-s, -s * 0.5),
            c + Vec2::new(s, -s * 0.5),
            c + Vec2::new(0.0, s),
        ]
    } else {
        vec![
            c + Vec2::new(-s * 0.5, -s),
            c + Vec2::new(-s * 0.5, s),
            c + Vec2::new(s, 0.0),
        ]
    };
    painter.add(Shape::convex_polygon(pts, color, PathStroke::NONE));
}

/// A folder.
pub fn folder(painter: &Painter, rect: Rect, color: Color32) {
    let r = rect.shrink2(Vec2::new(rect.width() * 0.12, rect.height() * 0.25));
    let tab = Rect::from_min_size(
        r.min - Vec2::new(0.0, 2.0),
        Vec2::new(r.width() * 0.45, 3.0),
    );
    painter.rect_filled(tab, 1.0, color);
    painter.rect_filled(r, 1.5, color);
}

/// A Math++ file: a rounded square holding an integral sign.
pub fn mpp_file(painter: &Painter, rect: Rect, color: Color32) {
    let r = rect.shrink(rect.height() * 0.1);
    painter.rect_stroke(r, 3.0, Stroke::new(1.3, color), egui::StrokeKind::Inside);
    painter.text(
        r.center(),
        egui::Align2::CENTER_CENTER,
        "\u{222b}",
        egui::FontId::proportional(r.height() * 0.95),
        color,
    );
}

/// Any other file, drawn as a plain page.
pub fn plain_file(painter: &Painter, rect: Rect, color: Color32) {
    let r = rect.shrink2(Vec2::new(rect.width() * 0.2, rect.height() * 0.12));
    painter.rect_stroke(r, 1.5, Stroke::new(1.1, color), egui::StrokeKind::Inside);
    for i in 1..=2 {
        let y = r.top() + r.height() * (0.3 + 0.25 * i as f32);
        painter.line_segment(
            [Pos2::new(r.left() + 3.0, y), Pos2::new(r.right() - 3.0, y)],
            Stroke::new(1.0, color),
        );
    }
}

/// A small chain marking a file the current file imports.
pub fn link(painter: &Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let r = rect.height() * 0.2;
    let w = Stroke::new(1.4, color);
    painter.circle_stroke(c - Vec2::new(r * 0.8, 0.0), r, w);
    painter.circle_stroke(c + Vec2::new(r * 0.8, 0.0), r, w);
}

/// A filled dot (the unsaved marker, drawn rather than typed).
pub fn dot(painter: &Painter, rect: Rect, color: Color32) {
    painter.circle_filled(rect.center(), rect.height().min(rect.width()) * 0.22, color);
}

/// A small plus at the bottom right of `rect` (the "new" badge).
pub fn plus(painter: &Painter, rect: Rect, color: Color32) {
    let c = rect.right_bottom() - Vec2::new(rect.width() * 0.22, rect.height() * 0.22);
    let s = rect.width() * 0.16;
    let st = Stroke::new(1.6, color);
    painter.line_segment([c - Vec2::new(s, 0.0), c + Vec2::new(s, 0.0)], st);
    painter.line_segment([c - Vec2::new(0.0, s), c + Vec2::new(0.0, s)], st);
}

/// A cross for close buttons.
pub fn cross(painter: &Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let s = rect.height().min(rect.width()) * 0.22;
    let st = Stroke::new(1.4, color);
    painter.line_segment([c + Vec2::new(-s, -s), c + Vec2::new(s, s)], st);
    painter.line_segment([c + Vec2::new(-s, s), c + Vec2::new(s, -s)], st);
}

/// Activity bar: the explorer (two stacked pages).
pub fn activity_explorer(painter: &Painter, rect: Rect, color: Color32) {
    let r = rect.shrink(rect.width() * 0.22);
    let st = Stroke::new(1.6, color);
    let back = Rect::from_min_size(r.min + Vec2::new(r.width() * 0.25, 0.0), r.size() * 0.75);
    let front = Rect::from_min_size(r.min + Vec2::new(0.0, r.height() * 0.25), r.size() * 0.75);
    painter.rect_stroke(back, 2.0, st, egui::StrokeKind::Inside);
    painter.rect_filled(front, 2.0, painter.ctx().global_style().visuals.panel_fill);
    painter.rect_stroke(front, 2.0, st, egui::StrokeKind::Inside);
}

/// Activity bar: a magnifier.
pub fn activity_search(painter: &Painter, rect: Rect, color: Color32) {
    let c = rect.center() - Vec2::new(2.0, 2.0);
    let r = rect.width() * 0.2;
    let st = Stroke::new(1.8, color);
    painter.circle_stroke(c, r, st);
    painter.line_segment(
        [
            c + Vec2::new(r * 0.75, r * 0.75),
            c + Vec2::new(r * 1.7, r * 1.7),
        ],
        st,
    );
}

/// Activity bar: the variables (two sliders).
pub fn activity_variables(painter: &Painter, rect: Rect, color: Color32) {
    let r = rect.shrink(rect.width() * 0.26);
    let st = Stroke::new(1.8, color);
    for (i, knob) in [0.3_f32, 0.7].iter().enumerate() {
        let y = r.top() + r.height() * (0.25 + 0.5 * i as f32);
        painter.line_segment([Pos2::new(r.left(), y), Pos2::new(r.right(), y)], st);
        painter.circle_filled(Pos2::new(r.left() + r.width() * knob, y), 3.2, color);
    }
}

/// Activity bar: an outline (indented lines).
pub fn activity_outline(painter: &Painter, rect: Rect, color: Color32) {
    let r = rect.shrink(rect.width() * 0.26);
    let st = Stroke::new(1.8, color);
    for (i, indent) in [0.0, 0.3, 0.3, 0.0].iter().enumerate() {
        let y = r.top() + r.height() * (i as f32 / 3.0);
        painter.line_segment(
            [
                Pos2::new(r.left() + r.width() * indent, y),
                Pos2::new(r.right(), y),
            ],
            st,
        );
    }
}

/// Activity bar: problems (a warning triangle with a mark).
pub fn activity_problems(painter: &Painter, rect: Rect, color: Color32) {
    let r = rect.shrink(rect.width() * 0.27);
    let st = Stroke::new(1.7, color);
    let top = Pos2::new(r.center().x, r.top());
    let (bl, br) = (r.left_bottom(), r.right_bottom());
    painter.line_segment([top, br], st);
    painter.line_segment([br, bl], st);
    painter.line_segment([bl, top], st);
    let x = r.center().x;
    painter.line_segment(
        [
            Pos2::new(x, r.top() + r.height() * 0.38),
            Pos2::new(x, r.top() + r.height() * 0.68),
        ],
        st,
    );
    painter.circle_filled(Pos2::new(x, r.top() + r.height() * 0.84), 1.2, color);
}

/// Activity bar: the tutorial (an open book).
pub fn activity_tutorial(painter: &Painter, rect: Rect, color: Color32) {
    let r = rect.shrink(rect.width() * 0.26);
    let st = Stroke::new(1.6, color);
    let mid_top = Pos2::new(r.center().x, r.top() + r.height() * 0.12);
    let mid_bottom = Pos2::new(r.center().x, r.bottom());
    for side in [r.left(), r.right()] {
        let outer_top = Pos2::new(side, r.top());
        let outer_bottom = Pos2::new(side, r.bottom() - r.height() * 0.12);
        painter.line_segment([outer_top, mid_top], st);
        painter.line_segment([outer_top, outer_bottom], st);
        painter.line_segment([outer_bottom, mid_bottom], st);
    }
    painter.line_segment([mid_top, mid_bottom], st);
}
