//! Problems found in the document, shown as wavy underlines under the typeset
//! element that is wrong. Messages are plain words; positions are never shown
//! as line and column numbers.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke};
use mpp_eval::Evaluated;
use mpp_syntax::document::Document;
use mpp_syntax::{Severity, Span};

use crate::page::Page;

/// One problem, placed on the page.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub span: Span,
    pub severity: Severity,
    pub message: String,
    /// Page-local rectangle of the typeset element it points at.
    pub rect: Rect,
}

/// Gathers the problems of the current text. Parser problems come from the
/// freshly parsed `doc`; evaluation problems are used only when the evaluation is
/// of this same text (`eval_fresh`), so a stale result never points at the wrong place.
pub fn collect(
    doc: &Document,
    eval: Option<&Evaluated>,
    eval_fresh: bool,
    page: &Page,
) -> Vec<Problem> {
    let mut seen: Vec<(Span, String)> = vec![];
    let mut out = vec![];
    let mut add = |d: &mpp_syntax::Diagnostic| {
        let key = (d.span, d.message.clone());
        if seen.contains(&key) {
            return;
        }
        seen.push(key);
        let Some(rect) = page.rect_of_span(d.span) else {
            return;
        };
        let mut message = d.message.clone();
        if !d.expected.is_empty() {
            message.push_str(&format!(" Expected {}.", d.expected.join(" or ")));
        }
        out.push(Problem {
            span: d.span,
            severity: d.severity,
            message,
            rect,
        });
    };
    doc.diagnostics.iter().for_each(&mut add);
    if let (Some(ev), true) = (eval, eval_fresh) {
        ev.all_diagnostics().into_iter().for_each(&mut add);
    }
    out
}

/// Draws a wavy line along the bottom of `rect`.
pub fn paint_squiggle(painter: &Painter, rect: Rect, color: Color32) {
    let rect = if rect.width() < 16.0 {
        rect.expand2(egui::Vec2::new((16.0 - rect.width()) / 2.0, 0.0))
    } else {
        rect
    };
    let y = rect.bottom() + 1.0;
    let step = 3.0;
    let amp = 1.6;
    let mut pts = vec![];
    let mut x = rect.left();
    let mut up = false;
    while x < rect.right() + step {
        pts.push(Pos2::new(
            x.min(rect.right() + 1.0),
            y + if up { -amp } else { amp },
        ));
        up = !up;
        x += step;
    }
    if pts.len() >= 2 {
        painter.add(Shape::line(pts, Stroke::new(1.3, color)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mpp_syntax::document::parse_document;

    #[test]
    fn collects_parser_problems_without_positions_in_the_text() {
        let src = "$$ \\int_0^ x dx $$\n";
        let doc = parse_document(src);
        let page = Page::default();
        // No page boxes: nothing can be placed, but nothing panics either.
        let _ = collect(&doc, None, false, &page);
    }
}
