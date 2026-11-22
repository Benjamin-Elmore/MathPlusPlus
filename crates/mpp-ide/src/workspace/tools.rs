//! The IDE tools of the Edit and Help menus: undo and history, autosave
//! setting, did-you-mean for missing files, and the tutorial entry.

use std::path::Path;
use std::time::Instant;

use egui::{Context, Ui};
use mpp_syntax::document::parse_document;

use crate::app::IdeApp;
use crate::copy::CopyFormat;
use crate::explorer::search::block_latex;
use crate::history::{ago, first_difference};
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

    /// The history of the current file, newest first: index, age, and what to
    /// typeset for the block that changed. These are the editor's own undo
    /// steps (a template insert or a word of typing is one step), so the list,
    /// Ctrl+Z and Ctrl+Y walk the same history.
    pub fn history_entries(&self) -> Vec<(usize, String, String)> {
        let Some(d) = self.doc() else {
            return vec![];
        };
        // One state more than is listed: the oldest listed one is compared with it.
        let states = d.editor.history_window(MENU_STEPS + 1);
        let now = Instant::now();
        let current = d.editor.history_position();
        (0..states.len())
            .rev()
            .take(MENU_STEPS)
            .map(|j| {
                let (i, s) = (states[j].0, &states[j].1);
                let when = match s.when {
                    _ if i == current => "Now".to_string(),
                    Some(t) if i > 0 => ago(t, now),
                    _ => "As opened".to_string(),
                };
                let offset = if i == 0 {
                    0
                } else {
                    first_difference(&states[j - 1].1.text, &s.text)
                };
                let mut latex = String::new();
                parse_document(&s.text).walk(&mut |b| {
                    if latex.is_empty() && b.span.start <= offset && offset <= b.span.end {
                        latex = block_latex(b, &s.text);
                    }
                });
                if latex.is_empty() {
                    latex = "\\text{(empty)}".into();
                }
                (i, when, latex)
            })
            .collect()
    }

    /// Goes back (or forward) to a history step; Ctrl+Y and Ctrl+Z carry on
    /// from there.
    pub fn restore_history_step(&mut self, step: usize) -> bool {
        let Some(d) = self.docs.get_mut(self.active) else {
            return false;
        };
        let ok = d.editor.go_to_history(step);
        if ok {
            d.touched();
        }
        ok
    }

    /// Removes the recovery copy of a file that is being closed.
    pub(crate) fn forget_recovery(&mut self, i: usize) {
        let (Some(dir), Some(d)) = (self.ws.settings_dir.clone(), self.docs.get(i)) else {
            return;
        };
        let owner = self.ws.autosave.owner.clone();
        crate::autosave::remove(
            &dir,
            &crate::autosave::key_for(d.path.as_deref(), d.id),
            &owner,
        );
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
        if ui.button("Cheat sheet	F1").clicked() {
            self.show_cheat_sheet(ctx);
            ui.close();
        }
        self.hints_menu(ui);
    }
}
