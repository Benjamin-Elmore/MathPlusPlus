//! Drawing figures. The plot crate plugs in by implementing [`FigureDrawer`];
//! until then every figure is a typeset placeholder box.

use egui::{Pos2, Rect, Stroke, StrokeKind, Ui};
use mpp_render::{FontId, Theme};
use mpp_syntax::document::Figure;

use crate::paint::{c32, paint_text};

/// Draws the figure of a `Figure:` block inside `rect`.
pub trait FigureDrawer {
    fn draw(&self, ui: &Ui, rect: Rect, figure: &Figure, theme: &Theme);
}

/// The stand-in: a quiet box labelled "Figure".
#[derive(Debug, Clone, Copy, Default)]
pub struct PlaceholderFigure;

impl FigureDrawer for PlaceholderFigure {
    fn draw(&self, ui: &Ui, rect: Rect, _figure: &Figure, theme: &Theme) {
        let painter = ui.painter();
        painter.rect_filled(rect, 6.0, c32(theme.code_background));
        painter.rect_stroke(
            rect,
            6.0,
            Stroke::new(1.0, c32(theme.placeholder_outline)),
            StrokeKind::Inside,
        );
        let size = 22.0;
        let w = crate::page::text_width(FontId::TextItalic, "Figure") * size;
        paint_text(
            painter,
            "Figure",
            Pos2::new(rect.center().x - w / 2.0, rect.center().y + size * 0.3),
            size,
            FontId::TextItalic,
            c32(theme.placeholder_label),
        );
    }
}
