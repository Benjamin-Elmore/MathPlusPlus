//! The per-frame autosave and recovery work, and the restore dialog.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use egui::{Align2, Context, Vec2};
use mpp_render::Theme;

use super::compare::{compare, RowKind};
use super::{key_for, Recovered};
use crate::app::IdeApp;
use crate::doc::Doc;
use crate::paint::c32;
use crate::settings::AutosaveMode;
use crate::typeset;

/// Quiet time before the delay mode saves a file.
const AUTOSAVE_AFTER: Duration = Duration::from_secs(1);
/// How often recovery copies are refreshed.
const RECOVERY_EVERY: Duration = Duration::from_secs(3);

/// The restore question shown at start-up.
#[derive(Debug, Clone, Default)]
pub struct RecoveryDialog {
    pub files: Vec<Recovered>,
}

/// What the autosave side remembers.
#[derive(Debug)]
pub struct AutosaveState {
    /// The revision of each file's last recovery write.
    written: HashMap<u64, (String, u64)>,
    last_recovery: Instant,
    was_focused: bool,
    pub dialog: Option<RecoveryDialog>,
}

impl Default for AutosaveState {
    fn default() -> Self {
        AutosaveState {
            written: HashMap::new(),
            last_recovery: Instant::now(),
            was_focused: true,
            dialog: None,
        }
    }
}

impl IdeApp {
    /// Saves files by the autosave setting and keeps recovery copies fresh.
    /// Call once a frame.
    pub(crate) fn autosave_tick(&mut self, ctx: &Context) {
        let focus = ctx.input(|i| i.viewport().focused);
        let lost_focus = self.ws.autosave.was_focused && focus == Some(false);
        self.ws.autosave.was_focused = focus != Some(false);
        self.run_autosave(lost_focus);
        if self.ws.autosave.last_recovery.elapsed() >= RECOVERY_EVERY {
            self.write_recovery();
            self.ws.autosave.last_recovery = Instant::now();
        }
        let waiting = self.docs.iter().any(Doc::is_dirty);
        if waiting {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }

    /// One autosave pass (`lost_focus`: the window just lost focus).
    pub fn run_autosave(&mut self, lost_focus: bool) {
        let mode = self.autosave_mode();
        let mut failed = None;
        for d in &mut self.docs {
            let due = match mode {
                AutosaveMode::Off => false,
                AutosaveMode::Delay => d.idle_for() >= AUTOSAVE_AFTER,
                AutosaveMode::Focus => lost_focus,
            };
            if due && d.is_dirty() && d.path.is_some() {
                if let Err(e) = d.save() {
                    failed = Some(format!("Could not autosave {}: {e}", d.title()));
                }
            }
        }
        if let Some(m) = failed {
            self.message = Some(m);
        }
    }

    /// Writes recovery copies of files with unsaved changes, and drops the
    /// copies of files that are saved again. Needs the settings folder.
    pub fn write_recovery(&mut self) {
        let Some(dir) = self.ws.settings_dir.clone() else {
            return;
        };
        let mut live: Vec<u64> = vec![];
        for d in &self.docs {
            let key = key_for(d.path.as_deref(), d.id);
            live.push(d.id);
            let rev = d.editor.revision();
            if d.is_dirty() {
                let fresh = self
                    .ws
                    .autosave
                    .written
                    .get(&d.id)
                    .is_some_and(|(k, r)| *k == key && *r == rev);
                if !fresh {
                    let r = Recovered {
                        key: key.clone(),
                        path: d.path.clone(),
                        title: d.title(),
                        text: d.editor.text().to_string(),
                    };
                    if super::write(&dir, &key, &r).is_ok() {
                        self.ws.autosave.written.insert(d.id, (key, rev));
                    }
                }
            } else if let Some((old, _)) = self.ws.autosave.written.remove(&d.id) {
                super::remove(&dir, &old);
            }
        }
        // Files closed since the last pass.
        let gone: Vec<u64> = self
            .ws
            .autosave
            .written
            .keys()
            .filter(|id| !live.contains(id))
            .copied()
            .collect();
        for id in gone {
            if let Some((old, _)) = self.ws.autosave.written.remove(&id) {
                super::remove(&dir, &old);
            }
        }
    }

    /// Loads the copies left by a crash and opens the restore question.
    pub(crate) fn find_recovery(&mut self) {
        let Some(dir) = self.ws.settings_dir.clone() else {
            return;
        };
        let found = super::worth_restoring(&dir, super::load_all(&dir));
        if !found.is_empty() {
            self.ws.autosave.dialog = Some(RecoveryDialog { files: found });
        }
    }

    /// Puts the recovered text back as unsaved edits.
    pub fn restore_recovered(&mut self) {
        let Some(d) = self.ws.autosave.dialog.take() else {
            return;
        };
        for r in d.files {
            let text_end;
            match &r.path {
                Some(p) if p.is_file() => {
                    if self.open_file(p, false).is_err() {
                        continue;
                    }
                    let Some(doc) = self.docs.get_mut(self.active) else {
                        continue;
                    };
                    text_end = doc.editor.text().len();
                    doc.editor
                        .replace_node(mpp_syntax::Span::new(0, text_end), &r.text);
                    doc.touched();
                }
                _ => {
                    self.new_file();
                    if let Some(doc) = self.docs.get_mut(self.active) {
                        doc.editor
                            .replace_node(mpp_syntax::Span::new(0, 0), &r.text);
                        doc.touched();
                    }
                }
            }
        }
        // Their copies are written again as soon as the next pass runs.
        self.ws.autosave.written.clear();
    }

    /// Throws the recovered copies away.
    pub fn discard_recovered(&mut self) {
        if let (Some(d), Some(dir)) = (self.ws.autosave.dialog.take(), self.ws.settings_dir.clone())
        {
            for r in d.files {
                super::remove(&dir, &r.key);
            }
        }
    }

    /// Removes every recovery copy: the app is closing cleanly.
    pub fn clear_recovery_on_exit(&mut self) {
        // Unsaved edits at a clean exit are kept: the user chose to quit with
        // them, and saving or closing each file already asked.
        if self.docs.iter().any(Doc::is_dirty) {
            self.write_recovery();
        } else if let Some(dir) = self.ws.settings_dir.clone() {
            super::clear(&dir);
        }
    }

    /// The restore dialog.
    pub(crate) fn recovery_dialog(&mut self, ctx: &Context) {
        let Some(dlg) = self.ws.autosave.dialog.clone() else {
            return;
        };
        let theme = self.mode.theme();
        let mut answer = None;
        egui::Window::new("Restore unsaved changes")
            .collapsible(false)
            .resizable(true)
            .default_size([760.0, 460.0])
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                let n = dlg.files.len();
                ui.label(format!(
                    "Restore unsaved changes to {n} {}?",
                    if n == 1 { "file" } else { "files" }
                ));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Restore").clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Discard").clicked() {
                        answer = Some(false);
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for r in &dlg.files {
                        let disk = r
                            .path
                            .as_ref()
                            .and_then(|p| std::fs::read_to_string(p).ok())
                            .unwrap_or_default();
                        egui::CollapsingHeader::new(&r.title)
                            .default_open(n == 1)
                            .show(ui, |ui| compare_grid(ui, &disk, &r.text, &theme));
                    }
                });
            });
        match answer {
            Some(true) => self.restore_recovered(),
            Some(false) => self.discard_recovered(),
            None => {}
        }
    }
}

fn compare_grid(ui: &mut egui::Ui, left: &str, right: &str, theme: &Theme) {
    let rows = compare(left, right);
    egui::Grid::new(ui.next_auto_id())
        .num_columns(2)
        .spacing([24.0, 6.0])
        .striped(false)
        .show(ui, |ui| {
            ui.label(egui::RichText::new("On disk").color(c32(theme.muted)));
            ui.label(egui::RichText::new("Unsaved copy").color(c32(theme.muted)));
            ui.end_row();
            for row in rows.iter().take(200) {
                for side in [&row.left, &row.right] {
                    let tint = match row.kind {
                        RowKind::Same => None,
                        RowKind::Changed => Some(c32(theme.warning)),
                        RowKind::Added => Some(c32(theme.result)),
                        RowKind::Removed => Some(c32(theme.error)),
                    };
                    let frame = egui::Frame::NONE
                        .inner_margin(egui::Margin::symmetric(4, 2))
                        .fill(tint.map_or(egui::Color32::TRANSPARENT, |c| c.gamma_multiply(0.16)));
                    frame.show(ui, |ui| {
                        ui.set_min_width(300.0);
                        match side {
                            Some(l) => {
                                typeset::show(ui, l, 16.0, *theme);
                            }
                            None => {
                                ui.label("");
                            }
                        }
                    });
                }
                ui.end_row();
            }
        });
}
