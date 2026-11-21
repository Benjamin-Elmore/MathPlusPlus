//! The smaller tiles: variables (with live sliders), problems, graphs and the
//! placeholders for tools that are not built yet.

use std::ops::Range;

use egui::{Sense, Ui, Vec2};
use mpp_eval::{Binding, VarKind};
use mpp_render::Theme;
use mpp_syntax::Span;

use crate::app::IdeApp;
use crate::defs;
use crate::paint::c32;
use crate::typeset;

/// A variable whose value is a plain number in the file.
#[derive(Debug, Clone, PartialEq)]
pub struct NumberVar {
    pub name: String,
    pub value: f64,
    /// Where the number is written.
    pub range: Range<usize>,
}

/// The byte range and value of the plain number on the right of the `:=` in
/// the text of a definition. `None` when the right side is anything else.
pub fn numeric_rhs(src: &str, span: Span) -> Option<(Range<usize>, f64)> {
    let text = src.get(span.start..span.end.min(src.len()))?;
    let at = text.find(":=")? + 2;
    let rest = &text[at..];
    let lead = rest.len() - rest.trim_start().len();
    let body = &rest[lead..];
    let len = body
        .char_indices()
        .take_while(|(_, c)| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'))
        .map(|(i, c)| i + c.len_utf8())
        .last()?;
    let token = &body[..len];
    let after = body[len..].trim_matches(|c: char| c.is_whitespace() || ".,;$".contains(c));
    if !after.is_empty() {
        return None;
    }
    let value: f64 = token.parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    let start = span.start + at + lead;
    Some((start..start + len, value))
}

/// A number as it is written back into the file.
pub fn format_number(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" {
        "0".into()
    } else {
        s.to_string()
    }
}

/// A slider range around a value.
pub fn slider_range(v: f64) -> (f64, f64) {
    let m = v.abs().max(1.0);
    if v >= 0.0 {
        (0.0, m * 3.0)
    } else {
        (-m * 3.0, m * 3.0)
    }
}

/// What the variables tile remembers across frames.
#[derive(Debug, Default, Clone)]
pub struct VarEdit {
    /// The edit in progress: the number's place and the revision after our last write.
    pub live: Option<(String, Range<usize>, u64)>,
}

impl IdeApp {
    /// Every changeable variable of the current file, with its value; plain
    /// numbers get a slider and an input box.
    pub(crate) fn variables_tile(&mut self, ui: &mut Ui, theme: &Theme) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            ui.label(
                egui::RichText::new("VARIABLES")
                    .size(13.5)
                    .color(c32(theme.muted)),
            );
        });
        ui.add_space(4.0);
        if self.docs.get(self.active).is_none() {
            ui.weak("  No file open.");
            return;
        }
        let (rows, fresh, src, rev) = self.variable_rows();
        // While a drag is going on the evaluation is stale, but we know where
        // our own write put the number.
        let live = self.ws.vars.live.clone();
        if rows.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.weak("Nothing defined yet.");
            });
        }
        let mut change: Option<(String, Range<usize>, f64, bool)> = None;
        egui::ScrollArea::vertical()
            .id_salt("variables-tile")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (info, num, changeable) in &rows {
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        typeset::show(ui, &defs::summary_latex(info), 17.0, *theme);
                    });
                    let mut num = num.clone();
                    if let Some((n, range, r)) = &live {
                        if *r == rev && *n == info.name {
                            if let Some(v) = num.as_mut() {
                                v.range = range.clone();
                            } else if let Some(text) = src.get(range.clone()) {
                                if let Ok(value) = text.parse::<f64>() {
                                    num = Some(NumberVar {
                                        name: n.clone(),
                                        value,
                                        range: range.clone(),
                                    });
                                }
                            }
                        }
                    }
                    if let Some(v) = num {
                        let (lo, hi) = slider_range(v.value);
                        let mut x = v.value;
                        ui.horizontal(|ui| {
                            ui.add_space(12.0);
                            let w = (ui.available_width() - 96.0).max(60.0);
                            ui.spacing_mut().slider_width = w;
                            let s = ui.add(egui::Slider::new(&mut x, lo..=hi).show_value(false));
                            s.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::Role::Slider,
                                    true,
                                    format!("{} slider", v.name),
                                )
                            });
                            let d = ui.add(egui::DragValue::new(&mut x).speed((hi - lo) / 400.0));
                            d.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::Role::SpinButton,
                                    true,
                                    format!("{} value", v.name),
                                )
                            });
                            let started = s.drag_started() || d.drag_started() || d.gained_focus();
                            if (s.changed() || d.changed()) && x.is_finite() {
                                change = Some((v.name.clone(), v.range.clone(), x, started));
                            }
                        });
                    } else if *changeable && !fresh {
                        ui.horizontal(|ui| {
                            ui.add_space(12.0);
                            ui.weak("Working\u{2026}");
                        });
                    }
                    ui.add_space(6.0);
                }
                // Variables that are not plain numbers are listed without a control.
                let _ = Sense::hover();
                let _ = Vec2::ZERO;
            });
        if let Some((name, range, x, first)) = change {
            self.write_variable(&name, range, x, first);
        }
    }

    /// The changeable variables of the current file: their description, the
    /// plain number behind them (when there is one), and whether they are `:=`
    /// definitions of this file. Also whether the evaluation is of the text on
    /// screen, the text, and its revision.
    #[allow(clippy::type_complexity)]
    fn variable_rows(
        &self,
    ) -> (
        Vec<(defs::DefInfo, Option<NumberVar>, bool)>,
        bool,
        String,
        u64,
    ) {
        let Some(doc) = self.docs.get(self.active) else {
            return (vec![], false, String::new(), 0);
        };
        let fresh = doc.eval_fresh();
        let src = doc.editor.text().to_string();
        let rev = doc.editor.revision();
        let mut rows = vec![];
        if let Some(ev) = doc.eval.as_ref() {
            for (name, b) in ev.bindings.iter() {
                let Binding::Variable { kind, origin, .. } = &**b else {
                    continue;
                };
                if mpp_eval::is_internal_name(name) {
                    continue;
                }
                let local = origin.file.is_none();
                let Some(info) = defs::describe(ev, name) else {
                    continue;
                };
                let editable = *kind == VarKind::Variable && local && fresh;
                let num = if editable {
                    numeric_rhs(&src, origin.span).map(|(range, value)| NumberVar {
                        name: name.clone(),
                        value,
                        range,
                    })
                } else {
                    None
                };
                rows.push((info, num, *kind == VarKind::Variable && local));
            }
        }
        (rows, fresh, src, rev)
    }

    /// Sets the number of a `:=` variable in the current file (the variables
    /// tile does this as a slider moves). False when the variable is not a plain
    /// number or the evaluation is out of date.
    pub fn set_variable(&mut self, name: &str, value: f64) -> bool {
        let (rows, ..) = self.variable_rows();
        let Some(v) = rows
            .into_iter()
            .find_map(|(_, n, _)| n.filter(|n| n.name == name))
        else {
            return false;
        };
        self.write_variable(name, v.range, value, true);
        true
    }

    /// Writes a new number into the file (one undo step per drag).
    fn write_variable(&mut self, name: &str, range: Range<usize>, value: f64, first: bool) {
        let text = format_number(value);
        let continuing = self.ws.vars.live.as_ref().is_some_and(|(n, r, rev)| {
            n == name
                && *r == range
                && self
                    .docs
                    .get(self.active)
                    .is_some_and(|d| d.editor.revision() == *rev)
        });
        let Some(doc) = self.docs.get_mut(self.active) else {
            return;
        };
        let caret = doc.editor.caret();
        if range.end > doc.editor.text().len() {
            return;
        }
        if continuing && !first {
            doc.editor.replace_inline(range.clone(), &text);
        } else {
            doc.editor
                .replace_node(Span::new(range.start, range.end), &text);
            doc.editor.move_caret(caret, false);
        }
        doc.touched();
        let rev = doc.editor.revision();
        self.ws.vars.live = Some((name.to_string(), range.start..range.start + text.len(), rev));
    }

    /// The Problems tile: every problem of the open files and the files they
    /// import, grouped by file and typeset; a click goes there.
    pub(crate) fn problems_tile(&mut self, ui: &mut Ui, theme: &Theme) {
        let list = self.project_problems();
        let size = (self.size * 0.85).max(14.0);
        let mut jump = None;
        egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                jump = self.problems.ui(ui, &list, theme, size);
            });
        if let Some(j) = jump {
            self.note_place();
            self.jump_to(&j);
            self.note_place();
        }
    }

    /// A figure pinned in its own tile: live (it redraws as the file changes and
    /// shares its camera and sliders with the page), with scroll to zoom.
    pub(crate) fn graph_tile(&mut self, ui: &mut Ui, doc: u64, figure: usize, theme: &Theme) {
        let (mode, size) = (self.mode, self.size);
        let Some(d) = self.docs.iter_mut().find(|d| d.id == doc) else {
            return;
        };
        let Some(st) = d.figures.list.get_mut(figure) else {
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new("This figure is not in the file any more.")
                    .color(c32(theme.muted)),
            );
            return;
        };
        let rows = st.rows() as f32 * crate::page::slider_row_height(size);
        let area = ui.available_rect_before_wrap().shrink(14.0);
        let rect = area.with_max_y((area.bottom() - rows).max(area.top() + 20.0));
        if rect.width() > 20.0 && rect.height() > 20.0 {
            let id = egui::Id::unique(("graph-tile", doc, figure));
            let action = crate::figure::figure_ui(ui, rect, id, st, mode, true);
            if rows > 0.0 {
                let top = egui::pos2(rect.left(), rect.bottom() + size * 0.2);
                crate::figure::sliders_ui(ui, top, rect.width(), size, st, mode);
            }
            match action {
                Some(crate::figure::FigureAction::ExportPng) => {
                    self.export_figure_of(doc, figure, false)
                }
                Some(crate::figure::FigureAction::ExportSvg) => {
                    self.export_figure_of(doc, figure, true)
                }
                _ => {}
            }
        }
    }

    /// Asks where to save figure `figure` of document `idx` and writes it.
    pub(crate) fn export_figure(&mut self, idx: usize, figure: usize, svg: bool) {
        if let Some(id) = self.docs.get(idx).map(|d| d.id) {
            self.export_figure_of(id, figure, svg);
        }
    }

    fn export_figure_of(&mut self, doc: u64, figure: usize, svg: bool) {
        let (ext, label) = if svg {
            ("svg", "SVG image")
        } else {
            ("png", "PNG image")
        };
        let Some(path) = rfd::FileDialog::new()
            .add_filter(label, &[ext])
            .set_file_name(format!("figure-{}.{ext}", figure + 1))
            .save_file()
        else {
            return;
        };
        let mode = self.mode;
        let result = self
            .docs
            .iter()
            .find(|d| d.id == doc)
            .and_then(|d| d.figures.list.get(figure))
            .map(|st| crate::figure::export(st, &path, mode));
        if let Some(Err(m)) = result {
            self.message = Some(m);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_number_after_the_definition_sign() {
        let src = "Let $m := 3.5$.";
        let (range, v) = numeric_rhs(src, Span::new(5, 13)).expect("number");
        assert_eq!(&src[range], "3.5");
        assert_eq!(v, 3.5);
        assert!(numeric_rhs("$$ a := b + 1 $$", Span::new(3, 13)).is_none());
        assert!(numeric_rhs("$$ a = 2 $$", Span::new(3, 8)).is_none());
    }

    #[test]
    fn numbers_are_written_tidily() {
        assert_eq!(format_number(2.0), "2");
        assert_eq!(format_number(-0.5), "-0.5");
        assert_eq!(format_number(0.1234567), "0.123457");
        assert_eq!(slider_range(5.0), (0.0, 15.0));
    }
}
