//! The IDE tools of the Edit and Help menus: undo and history, autosave
//! setting, did-you-mean for missing files, and the tutorial entry.

use std::path::Path;
use std::time::Instant;

use egui::{Context, Ui};
use mpp_syntax::document::parse_document;
use mpp_syntax::Span;

use crate::app::IdeApp;
use crate::copy::CopyFormat;
use crate::explorer::search::block_latex;
use crate::history::{ago, Timeline};
use crate::settings::AutosaveMode;
use crate::suggest::Suggester;
use crate::typeset;

/// How many history entries the menu lists.
const MENU_STEPS: usize = 25;

impl IdeApp {
    /// Steps the current file back (Ctrl+Z).
    pub fn undo(&mut self) -> bool {
        let Some(d) = self.docs.get_mut(self.active) else {
            return false;
        };
        let ok = d.editor.undo();
        if ok {
            d.touched();
        }
        ok
    }

    /// Steps the current file forward (Ctrl+Y).
    pub fn redo(&mut self) -> bool {
        let Some(d) = self.docs.get_mut(self.active) else {
            return false;
        };
        let ok = d.editor.redo();
        if ok {
            d.touched();
        }
        ok
    }

    /// Adds the files' changes to their histories. Call once a frame.
    pub(crate) fn record_history(&mut self) {
        let now = Instant::now();
        for d in &self.docs {
            let rev = d.editor.revision();
            let text = d.editor.text();
            self.ws
                .history
                .entry(d.id)
                .and_modify(|t| {
                    t.observe(rev, text, now);
                })
                .or_insert_with(|| Timeline::new(text, rev, now));
        }
        let ids: Vec<u64> = self.docs.iter().map(|d| d.id).collect();
        self.ws.history.retain(|id, _| ids.contains(id));
    }

    /// The history of the current file, newest first: index, age, and what to
    /// typeset for the block that changed.
    pub fn history_entries(&self) -> Vec<(usize, String, String)> {
        let Some(d) = self.doc() else {
            return vec![];
        };
        let Some(tl) = self.ws.history.get(&d.id) else {
            return vec![];
        };
        let now = Instant::now();
        let last = tl.steps().len().saturating_sub(1);
        tl.steps()
            .iter()
            .enumerate()
            .rev()
            .take(MENU_STEPS)
            .map(|(i, s)| {
                let when = if i == 0 {
                    "As opened".to_string()
                } else {
                    ago(s.when, now)
                };
                let parsed = parse_document(&s.text);
                let mut latex = String::new();
                parsed.walk(&mut |b| {
                    if latex.is_empty() && b.span.start <= s.offset && s.offset <= b.span.end {
                        latex = block_latex(b, &s.text);
                    }
                });
                if i == last && latex.is_empty() {
                    latex = "\\text{latest}".into();
                }
                (i, when, latex)
            })
            .collect()
    }

    /// Restores the current file to a history step (as one undoable edit).
    pub fn restore_history_step(&mut self, step: usize) -> bool {
        let Some(d) = self.docs.get(self.active) else {
            return false;
        };
        let Some(text) = self
            .ws
            .history
            .get(&d.id)
            .and_then(|t| t.text_at(step))
            .map(str::to_string)
        else {
            return false;
        };
        let Some(d) = self.docs.get_mut(self.active) else {
            return false;
        };
        let end = d.editor.text().len();
        let caret = d.editor.caret();
        d.editor.replace_node(Span::new(0, end), &text);
        d.editor.move_caret(caret.min(text.len()), false);
        d.touched();
        true
    }

    /// Removes the recovery copy of a file that is being closed.
    pub(crate) fn forget_recovery(&mut self, i: usize) {
        let (Some(dir), Some(d)) = (self.ws.settings_dir.clone(), self.docs.get(i)) else {
            return;
        };
        crate::autosave::remove(&dir, &crate::autosave::key_for(d.path.as_deref(), d.id));
    }

    /// The message for an import whose file is missing, with the closest real names.
    pub(crate) fn missing_import_message(&self, target: &Path) -> String {
        let name = target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let root = self.project_root().map(Path::to_path_buf);
        let known: Vec<String> = self
            .ws
            .explorer
            .all_files()
            .iter()
            .map(|p| {
                root.as_deref()
                    .and_then(|r| p.strip_prefix(r).ok())
                    .unwrap_or(p)
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        let s = Suggester.files(&name, &known);
        match s.first() {
            Some(first) => format!("The file {name} could not be found. Did you mean {first}?"),
            None => format!("The file {name} could not be found."),
        }
    }

    /// The Edit menu.
    pub(crate) fn edit_menu(&mut self, ui: &mut Ui) {
        if ui
            .add(egui::Button::new("Undo").shortcut_text("Ctrl+Z"))
            .clicked()
        {
            self.undo();
            ui.close();
        }
        if ui
            .add(egui::Button::new("Redo").shortcut_text("Ctrl+Y"))
            .clicked()
        {
            self.redo();
            ui.close();
        }
        let theme = self.mode.theme();
        let entries = self.history_entries();
        let mut restore = None;
        ui.menu_button("History", |ui| {
            if entries.len() < 2 {
                ui.weak("No changes yet.");
            }
            for (i, when, latex) in &entries {
                ui.horizontal(|ui| {
                    ui.set_min_width(260.0);
                    ui.label(egui::RichText::new(when).weak());
                    let r = typeset::show(ui, latex, 15.0, theme);
                    if r.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if r.clicked() {
                        restore = Some(*i);
                        ui.close();
                    }
                });
            }
        });
        if let Some(i) = restore {
            self.restore_history_step(i);
        }
        ui.separator();
        ui.menu_button("Autosave", |ui| {
            for (m, label) in [
                (AutosaveMode::Off, "Off"),
                (AutosaveMode::Delay, "After a second of quiet"),
                (AutosaveMode::Focus, "When the window loses focus"),
            ] {
                if ui.radio(self.autosave_mode() == m, label).clicked() {
                    self.set_autosave_mode(m);
                    ui.close();
                }
            }
        });
        ui.separator();
        let mut latex = self.copy_format == CopyFormat::Latex;
        if ui.checkbox(&mut latex, "Copy maths as LaTeX").changed() {
            self.copy_format = if latex {
                CopyFormat::Latex
            } else {
                CopyFormat::Unicode
            };
        }
        ui.weak("Copying gives Unicode maths by default");
    }

    /// The Help menu.
    pub(crate) fn help_menu(&mut self, ui: &mut Ui, ctx: &Context) {
        if ui.button("Tutorial").clicked() {
            self.open_tutorial(Some(ctx));
            ui.close();
        }
    }
}
