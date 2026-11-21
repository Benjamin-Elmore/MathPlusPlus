//! The status marks beside proof steps (✓ ≈ ⊢ ◇ 📖 ⚠ ✗), drawn as shapes in the
//! left margin of the equation view, and the card shown when one is hovered.
//! They are preview only: nothing here is ever written into the file.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2};
use mpp_proof::StepStatus;
use mpp_render::Theme;

use super::StepMark;
use crate::page::Page;
use crate::paint::c32;

/// Where the mark of each step goes on screen (`origin` is the column's top left).
pub fn mark_rects<'a>(
    page: &Page,
    steps: &'a [StepMark],
    origin: Pos2,
    clip_left: f32,
    size: f32,
) -> Vec<(Rect, &'a StepMark)> {
    // In the column's left margin (at least one em wide), clear of the side bar's
    // resize handle.
    let r = (size * 0.36).max(5.0);
    let x = (origin.x - size * 0.66).max(clip_left + r + 6.0);
    let mut out = vec![];
    for s in steps {
        let rects = crate::diagnostics::map::rects_for_span(page, s.span);
        let Some(first) = rects.first() else {
            continue;
        };
        let cy = origin.y + first.center().y;
        out.push((
            Rect::from_center_size(Pos2::new(x, cy), Vec2::splat(r * 2.0)),
            s,
        ));
    }
    out
}

/// Draws every step's mark.
pub fn paint_marks(
    painter: &Painter,
    page: &Page,
    steps: &[StepMark],
    origin: Pos2,
    theme: &Theme,
    size: f32,
) {
    for (rect, s) in mark_rects(page, steps, origin, painter.clip_rect().left(), size) {
        let color = c32(super::status_color(s.status, theme));
        paint_status(painter, rect.center(), rect.width() / 2.0, s.status, color);
    }
}

/// One status mark as a shape of radius `r` around `c`.
pub fn paint_status(painter: &Painter, c: Pos2, r: f32, status: StepStatus, color: Color32) {
    let w = (r * 0.24).max(1.4);
    let stroke = Stroke::new(w, color);
    let p = |u: f32, v: f32| Pos2::new(c.x + u * r, c.y + v * r);
    match status {
        StepStatus::Symbolic => {
            painter.add(Shape::line(
                vec![p(-0.8, 0.05), p(-0.25, 0.6), p(0.85, -0.7)],
                stroke,
            ));
        }
        StepStatus::Lean => {
            painter.line_segment([p(-0.6, -0.8), p(-0.6, 0.8)], stroke);
            painter.line_segment([p(-0.6, 0.0), p(0.8, 0.0)], stroke);
        }
        StepStatus::Numeric => {
            for dy in [-0.32, 0.32] {
                let pts: Vec<Pos2> = (0..=12)
                    .map(|i| {
                        let t = i as f32 / 12.0;
                        let u = -0.85 + 1.7 * t;
                        p(u, dy - 0.18 * (t * std::f32::consts::TAU).sin())
                    })
                    .collect();
                painter.add(Shape::line(pts, stroke));
            }
        }
        StepStatus::Structure => {
            painter.add(Shape::closed_line(
                vec![p(0.0, -0.85), p(0.75, 0.0), p(0.0, 0.85), p(-0.75, 0.0)],
                stroke,
            ));
        }
        StepStatus::Cited => {
            // An open book: two pages meeting at the spine.
            let left = vec![p(0.0, -0.45), p(-0.9, -0.7), p(-0.9, 0.6), p(0.0, 0.8)];
            let right = vec![p(0.0, -0.45), p(0.9, -0.7), p(0.9, 0.6), p(0.0, 0.8)];
            painter.add(Shape::closed_line(left, stroke));
            painter.add(Shape::closed_line(right, stroke));
        }
        StepStatus::Gap | StepStatus::External => {
            painter.add(Shape::closed_line(
                vec![p(0.0, -0.9), p(0.95, 0.8), p(-0.95, 0.8)],
                stroke,
            ));
            painter.line_segment([p(0.0, -0.35), p(0.0, 0.25)], stroke);
            painter.circle_filled(p(0.0, 0.55), w * 0.75, color);
        }
        StepStatus::Wrong => {
            painter.line_segment([p(-0.7, -0.7), p(0.7, 0.7)], stroke);
            painter.line_segment([p(-0.7, 0.7), p(0.7, -0.7)], stroke);
        }
    }
}

/// The card shown over a mark: the status, the checker's one-line detail, the
/// counterexample of a wrong step and each relation checked.
pub fn mark_card(ctx: &egui::Context, at: Pos2, s: &StepMark, theme: &Theme, size: f32) {
    egui::Area::new(egui::Id::unique("mpp-proof-mark-card"))
        .order(egui::Order::Tooltip)
        .fixed_pos(at)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.set_max_width(460.0);
                    let step = s
                        .number
                        .map_or_else(|| "This step".to_string(), |n| format!("Step {n}"));
                    let head = format!("{step}: {} {}", s.status.mark(), s.status.name());
                    let tint = Theme {
                        text: super::status_color(s.status, theme),
                        ..*theme
                    };
                    crate::typeset::show(ui, &super::prose_latex(&head), size, tint);
                    if !s.detail.is_empty() {
                        crate::typeset::show(
                            ui,
                            &super::prose_latex(&s.detail),
                            size * 0.9,
                            *theme,
                        );
                    }
                    // Only what the detail does not already say.
                    if let Some(cx) = s
                        .counterexample
                        .as_ref()
                        .filter(|c| !s.detail.contains(c.as_str()))
                    {
                        crate::typeset::show(
                            ui,
                            &super::prose_latex(&format!("Counterexample: {cx}")),
                            size * 0.9,
                            *theme,
                        );
                    }
                    let new_claims = s.claims.iter().filter(|(_, o)| {
                        let said = o.split_once(": ").map_or(o.as_str(), |(_, rest)| rest);
                        !s.detail.contains(said)
                    });
                    for (latex, outcome) in new_claims {
                        ui.horizontal(|ui| {
                            crate::typeset::show(ui, latex, size * 0.85, *theme);
                            let muted = Theme {
                                text: theme.muted,
                                ..*theme
                            };
                            crate::typeset::show(
                                ui,
                                &super::prose_latex(outcome),
                                size * 0.8,
                                muted,
                            );
                        });
                    }
                });
        });
}
