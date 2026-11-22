//! The application: menus, panels, tabs and the equation view.

use std::path::{Path, PathBuf};

use egui::{Context, Key, KeyboardShortcut, Modifiers, Ui};

use crate::copy::CopyFormat;
use crate::defs::{self, DefInfo};
use crate::doc::Doc;
use crate::explorer;
use crate::files::Recent;
use crate::insert::{InsertSettings, InsertUi};
use crate::layout::{BODY_SIZE, MAX_SIZE, MIN_SIZE};
use crate::sample::WELCOME;
use crate::theme::{self, Mode, ThemeChoice};
use crate::view::CompletionState;
use crate::workspace::Workspace;

const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
const ZOOM_IN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Equals);
const ZOOM_OUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Minus);
const ZOOM_IN_PLUS: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Plus);
const ZOOM_RESET: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::ALT), Key::Num0);
/// Shows or hides the Problems tile.
const PROBLEMS: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::M);

/// The Math++ IDE.
pub struct IdeApp {
    pub docs: Vec<Doc>,
    pub active: usize,
    /// What the user chose (default: follow Windows).
    pub theme_choice: ThemeChoice,
    /// The mode drawn this frame: the choice resolved against the system theme.
    pub mode: Mode,
    pub size: f32,
    pub copy_format: CopyFormat,
    /// The read-only raw text pop-out. Off at every start and never saved.
    pub show_file_text: bool,
    /// Auto-evaluation, steps and the default number format, for every file.
    pub eval_settings: crate::eval::EvalSettings,
    pub recent: Recent,
    pub ws: Workspace,
    pub(crate) completion: Option<CompletionState>,
    pub(crate) clip: Option<(String, String)>,
    pub(crate) message: Option<String>,
    applied_mode: Option<Mode>,
    system_theme: Option<egui::Theme>,
    /// The Insert menu, its recently used items and its settings.
    pub insert: InsertUi,
    /// The quick-fix menu and the problem card kept open by F8.
    pub diag: crate::diagnostics::ui::DiagUi,
    /// The Problems tile's state (Ctrl+Shift+M).
    pub problems: crate::problems::ProblemsView,
    /// View → Comments: notes shown in the equation view (remembered per user).
    pub show_comments: bool,
    /// File → Export…
    pub export: crate::export::ExportDialog,
    /// The proof agent panel, Settings → Proof agent and the Lean alignment view.
    pub proof: crate::proof::ProofUi,
    /// What the window showed at the end of the last frame (see `screen_state`).
    last_screen: Option<u64>,
}

impl Default for IdeApp {
    fn default() -> Self {
        IdeApp::new()
    }
}

impl IdeApp {
    /// A new IDE showing the welcome page.
    pub fn new() -> Self {
        IdeApp {
            docs: vec![Doc::with_text(WELCOME, None)],
            active: 0,
            theme_choice: ThemeChoice::default(),
            mode: Mode::Light,
            size: BODY_SIZE,
            copy_format: CopyFormat::Unicode,
            show_file_text: false,
            eval_settings: crate::eval::EvalSettings::default(),
            recent: Recent::default(),
            ws: Workspace::default(),
            completion: None,
            clip: None,
            message: None,
            applied_mode: None,
            system_theme: None,
            insert: InsertUi::default(),
            diag: Default::default(),
            problems: Default::default(),
            show_comments: true,
            export: Default::default(),
            proof: Default::default(),
            last_screen: None,
        }
    }

    /// An IDE with no documents open yet (the caller opens files).
    pub fn empty() -> Self {
        let mut a = IdeApp::new();
        a.docs.clear();
        a
    }

    /// Sets the theme choice (Light, Dark or Follow Windows) and updates the mode.
    pub fn set_theme_choice(&mut self, choice: ThemeChoice) {
        self.theme_choice = choice;
        self.mode = choice.resolve(self.system_theme);
    }

    /// Reads settings and the recent files from storage.
    pub fn restore(&mut self, storage: &dyn eframe::Storage) {
        if let Some(s) = storage.get_string("recent") {
            self.recent = Recent::from_stored(&s);
        }
        // The old "dark" key (0 or 1) is read when no "theme" was saved yet.
        let legacy = storage.get_string("dark").map(|s| {
            if s == "1" {
                ThemeChoice::Dark
            } else {
                ThemeChoice::Light
            }
        });
        let saved = storage
            .get_string("theme")
            .and_then(|s| ThemeChoice::from_stored(&s));
        if let Some(c) = saved.or(legacy) {
            self.set_theme_choice(c);
        }
        if let Some(s) = storage
            .get_string("size")
            .and_then(|s| s.parse::<f32>().ok())
        {
            self.size = s.clamp(MIN_SIZE, MAX_SIZE);
        }
        if let Some(s) = storage.get_string("insert") {
            self.insert.settings = InsertSettings::from_stored(&s);
        }
        if let Some(s) = storage.get_string("comments") {
            self.show_comments = s != "0";
        }
        if let Some(s) = storage.get_string("copy_latex") {
            self.copy_format = if s == "1" {
                CopyFormat::Latex
            } else {
                CopyFormat::Unicode
            };
        }
    }

    /// Writes settings and the recent files. The raw-text pop-out is not saved.
    pub fn save_state(&self, storage: &mut dyn eframe::Storage) {
        storage.set_string("recent", self.recent.to_stored());
        storage.set_string("theme", self.theme_choice.to_stored().into());
        storage.set_string("size", self.size.to_string());
        storage.set_string("insert", self.insert.settings.to_stored());
        storage.set_string(
            "comments",
            if self.show_comments { "1" } else { "0" }.into(),
        );
        storage.set_string(
            "copy_latex",
            if self.copy_format == CopyFormat::Latex {
                "1"
            } else {
                "0"
            }
            .into(),
        );
    }

    /// The last message shown in the status bar (an error), if any.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// The document being edited.
    pub fn doc(&self) -> Option<&Doc> {
        self.docs.get(self.active)
    }

    /// The document being edited, mutably.
    pub fn doc_mut(&mut self) -> Option<&mut Doc> {
        self.docs.get_mut(self.active)
    }

    /// Opens a file (or switches to it when it is already open).
    pub fn open_path(&mut self, path: &Path) -> Result<usize, String> {
        if let Some(i) = self
            .docs
            .iter()
            .position(|d| d.path.as_deref() == Some(path))
        {
            self.active = i;
            return Ok(i);
        }
        let doc = Doc::open(path).map_err(|e| format!("Could not open {}: {e}", path.display()))?;
        self.docs.push(doc);
        self.active = self.docs.len() - 1;
        self.recent.touch(path);
        self.adopt_folder(path.parent());
        Ok(self.active)
    }

    /// With no project folder yet, the folder of the file just opened becomes it.
    fn adopt_folder(&mut self, dir: Option<&Path>) {
        if self.ws.explorer.root.is_none() {
            if let Some(d) = dir {
                self.open_folder(d, None);
            }
        } else {
            self.ws.explorer.mark_stale();
        }
    }

    /// A new empty file.
    pub fn new_file(&mut self) {
        self.docs.push(Doc::blank());
        self.active = self.docs.len() - 1;
        self.name_untitled();
    }

    /// Saves the current file, asking for a name when it has none.
    pub fn save(&mut self) {
        let Some(doc) = self.docs.get_mut(self.active) else {
            return;
        };
        if doc.path.is_none() {
            self.save_as();
            return;
        }
        if let Err(e) = doc.save() {
            self.message = Some(format!("Could not save: {e}"));
        } else {
            self.message = None;
        }
        self.ws.explorer.mark_stale();
    }

    /// Saves under a new name chosen in a dialog.
    pub fn save_as(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Math++", &["mpp"])
            .set_file_name("Untitled.mpp")
            .save_file()
        else {
            return;
        };
        self.save_to(&path);
    }

    /// Saves the current file under `path`.
    pub fn save_to(&mut self, path: &Path) {
        let Some(doc) = self.docs.get_mut(self.active) else {
            return;
        };
        match doc.save_as(path) {
            Ok(()) => {
                self.recent.touch(path);
                self.adopt_folder(path.parent());
                self.message = None;
            }
            Err(e) => self.message = Some(format!("Could not save: {e}")),
        }
    }

    fn open_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Math++", &["mpp"])
            .pick_file()
        {
            if let Err(m) = self.open_path(&path) {
                self.message = Some(m);
            }
        }
    }

    fn open_folder_dialog(&mut self, ctx: &Context) {
        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
            self.open_folder(&dir, Some(ctx));
            self.show_side(crate::settings::SideView::Explorer);
        }
    }

    /// Jumps to a definition: in this file, or in the file it came from.
    pub fn goto_definition(&mut self, info: &DefInfo) {
        self.note_place();
        self.goto_definition_inner(info);
        self.note_place();
    }

    fn goto_definition_inner(&mut self, info: &DefInfo) {
        match &info.file {
            None => {
                if let Some(doc) = self.docs.get_mut(self.active) {
                    doc.editor.move_caret(info.span.start, false);
                    doc.highlight = Some(info.span);
                    doc.scroll_to = Some(info.span);
                }
            }
            Some(f) => {
                let path = PathBuf::from(f);
                match self.open_file(&path, false) {
                    Ok(i) => {
                        if let Some(doc) = self.docs.get_mut(i) {
                            doc.editor.move_caret(info.span.start, false);
                            doc.highlight = Some(info.span);
                            doc.scroll_to = Some(info.span);
                        }
                    }
                    Err(m) => self.message = Some(m),
                }
            }
        }
    }

    pub(crate) fn goto_at(&mut self, offset: usize) {
        let info = self.doc().and_then(|d| {
            let ev = d.eval.as_ref()?;
            defs::definition_at(ev, d.editor.text(), offset)
        });
        if let Some(info) = info {
            self.goto_definition(&info);
        }
    }

    /// The last copy made inside the app: what other applications received, and
    /// the source it came from.
    pub fn last_copy(&self) -> Option<(&str, &str)> {
        self.clip.as_ref().map(|(a, b)| (a.as_str(), b.as_str()))
    }

    /// Finishes any pending evaluation of the current file (used by tests).
    pub fn settle(&mut self) {
        if let Some(d) = self.docs.get_mut(self.active) {
            d.settle();
        }
    }

    /// The autocomplete entries on show, as (typeset LaTeX, label) pairs.
    pub fn completion_items(&self) -> Vec<(String, String)> {
        self.completion
            .as_ref()
            .map(|c| {
                c.items
                    .iter()
                    .map(|i| (i.latex.clone(), i.label.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The laid-out page of the active file at a column width (for checks of what
    /// is drawn where).
    pub fn page_at(&mut self, width: f32) -> Option<std::sync::Arc<crate::PageCache>> {
        let (size, mode) = (self.size, self.mode);
        self.docs
            .get_mut(self.active)
            .map(|d| d.page(width, size, mode))
    }

    /// The text of the typeset page: every glyph shown on it, block by block.
    /// (This is what a reader sees, never the source.)
    pub fn visible_text(&mut self, width: f32) -> String {
        let (size, mode) = (self.size, self.mode);
        let Some(doc) = self.docs.get_mut(self.active) else {
            return String::new();
        };
        let pc = doc.page(width, size, mode);
        pc.page
            .blocks
            .iter()
            .map(|b| b.dl.text())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn shortcuts(&mut self, ctx: &Context) {
        self.workspace_shortcuts(ctx);
        self.tile_shortcuts(ctx);
        let (save, open, new, zin, zout, zreset, zdelta) = ctx.input_mut(|i| {
            (
                i.consume_shortcut(&SAVE),
                i.consume_shortcut(&OPEN),
                i.consume_shortcut(&NEW),
                i.consume_shortcut(&ZOOM_IN) | i.consume_shortcut(&ZOOM_IN_PLUS),
                i.consume_shortcut(&ZOOM_OUT),
                i.consume_shortcut(&ZOOM_RESET),
                i.zoom_delta(),
            )
        });
        if zreset {
            self.size = BODY_SIZE;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&PROBLEMS)) {
            self.toggle_problems();
        }
        if (zdelta - 1.0).abs() > 0.001 {
            self.size = (self.size * zdelta).clamp(MIN_SIZE, MAX_SIZE);
        }
        if save {
            self.save();
        }
        if open {
            self.open_dialog();
        }
        if new {
            self.new_file();
        }
        if zin {
            self.size = (self.size + 1.0).min(MAX_SIZE);
        }
        if zout {
            self.size = (self.size - 1.0).max(MIN_SIZE);
        }
    }

    fn menu(&mut self, ui: &mut Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New").clicked() {
                    self.new_file();
                    ui.close();
                }
                if ui.button("Open\u{2026}").clicked() {
                    self.open_dialog();
                    ui.close();
                }
                if ui.button("Open Folder\u{2026}").clicked() {
                    self.open_folder_dialog(ui.ctx());
                    ui.close();
                }
                ui.menu_button("Open recent", |ui| {
                    let paths: Vec<PathBuf> = self.recent.paths().to_vec();
                    if paths.is_empty() {
                        ui.weak("Nothing yet");
                    }
                    for p in paths {
                        if ui.button(explorer::display_name(&p)).clicked() {
                            if let Err(m) = self.open_path(&p) {
                                self.message = Some(m);
                            }
                            ui.close();
                        }
                    }
                });
                ui.separator();
                if ui.button("Save").clicked() {
                    self.save();
                    ui.close();
                }
                if ui.button("Save as\u{2026}").clicked() {
                    self.save_as();
                    ui.close();
                }
                if ui.button("Export\u{2026}").clicked() {
                    self.export.open = true;
                    self.export.message = None;
                    ui.close();
                }
                ui.separator();
                if ui.button("Close tab").clicked() {
                    self.request_close(self.active);
                    ui.close();
                }
                if ui.button("Quit").clicked() {
                    ui.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| self.edit_menu(ui));
            ui.menu_button("Proof", |ui| self.proof_menu(ui));
            ui.menu_button("View", |ui| {
                ui.menu_button("Theme", |ui| {
                    for c in ThemeChoice::ALL {
                        if ui.radio(self.theme_choice == c, c.label()).clicked() {
                            self.set_theme_choice(c);
                        }
                    }
                    ui.weak("Ctrl+K, then Ctrl+T, switches theme");
                });
                if ui.button("Larger text").clicked() {
                    self.size = (self.size + 1.0).min(MAX_SIZE);
                }
                if ui.button("Smaller text").clicked() {
                    self.size = (self.size - 1.0).max(MIN_SIZE);
                }
                ui.separator();
                self.layout_menu(ui);
                ui.separator();
                if ui
                    .add(egui::Button::new("Side bar").shortcut_text("Ctrl+B"))
                    .clicked()
                {
                    self.toggle_side();
                    ui.close();
                }
                if ui
                    .add(egui::Button::new("Problems").shortcut_text("Ctrl+Shift+M"))
                    .clicked()
                {
                    self.toggle_problems();
                    ui.close();
                }
                if ui
                    .add(egui::Button::new("Go to file\u{2026}").shortcut_text("Ctrl+P"))
                    .clicked()
                {
                    self.ws.quick.open = true;
                    ui.close();
                }
                ui.separator();
                ui.menu_button("Insert settings", |ui| {
                    let s = &mut self.insert.settings;
                    ui.label("Integral bounds");
                    ui.radio_value(&mut s.integral_side, false, "Above and below the sign");
                    ui.radio_value(&mut s.integral_side, true, "At the side of the sign");
                    ui.separator();
                    ui.checkbox(
                        &mut s.shortcuts,
                        "Typing shortcuts: int + space for \u{222b}, / for a fraction",
                    );
                    ui.checkbox(&mut s.fourier_k, "Fourier transforms use k (not \u{3c9})");
                });
                self.document_menu(ui);
                ui.separator();
                ui.checkbox(&mut self.eval_settings.auto_eval, "Auto-evaluate");
                ui.checkbox(&mut self.eval_settings.show_steps, "Show steps");
                ui.checkbox(&mut self.show_comments, "Comments");
                ui.menu_button("Numbers", |ui| {
                    use mpp_core::display::Style;
                    let style = &mut self.eval_settings.number_format.style;
                    ui.radio_value(style, Style::Exact, "Exact (fractions and roots)");
                    ui.radio_value(style, Style::Decimal, "Decimals");
                });
                ui.separator();
                ui.checkbox(&mut self.show_file_text, "Show file text");
            });
            let ctx = ui.ctx().clone();
            ui.menu_button("Help", |ui| self.help_menu(ui, &ctx));
            if crate::insert::menu::insert_button(ui).clicked() {
                self.insert.menu.open_at(None);
            }
        });
    }

    /// Gives every open file the evaluator settings (each re-evaluates on a change).
    pub fn apply_eval_settings(&mut self) {
        let s = self.eval_settings;
        let notes = self.show_comments;
        for d in &mut self.docs {
            d.set_eval_settings(s);
            d.set_show_comments(notes);
        }
    }

    /// Draws the whole window into `ui`.
    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.name_untitled();
        self.shortcuts(&ctx);
        self.system_theme = ctx.system_theme();
        self.mode = self.theme_choice.resolve(self.system_theme);
        if self.applied_mode != Some(self.mode) {
            theme::apply(&ctx, self.mode);
            self.applied_mode = Some(self.mode);
        }
        let theme = self.mode.theme();
        self.apply_eval_settings();
        self.hints_in();

        egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        self.status_ui(ui);
        self.side_ui(ui);

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(crate::paint::c32(theme.background)))
            .show(ui, |ui| self.tiles_ui(ui));
        self.problems.open = self.has_pane(&crate::layout::Pane::Problems);

        self.file_text_window(&ctx);
        if crate::export::dialog_ui(&ctx, &mut self.export) {
            self.export_dialog_save();
        }
        self.overlays(&ctx);
        self.proof_windows(&ctx);
        if self.ws.explorer.poll() {
            self.reload_changed();
        }
        self.settle_preview();
        self.autosave_tick(&ctx);
        self.hints_out();
        self.persist_settings();
        if !ctx.input(|i| i.pointer.any_down()) {
            self.persist_layout();
        }

        // Whatever this frame changed after the part of the window showing it was
        // drawn (a file opened from a menu or a panel, a panel switched, a menu
        // closed) is drawn by the very next frame, without waiting for input.
        let now = self.screen_state(&ctx);
        if self.last_screen != Some(now) {
            self.last_screen = Some(now);
            ctx.request_repaint();
        }

        let mut wait: Option<std::time::Duration> = None;
        for d in &mut self.docs {
            if let Some(w) = d.pump() {
                wait = Some(wait.map_or(w, |x| x.min(w)));
            }
        }
        if let Some(w) = wait {
            ctx.request_repaint_after(w);
        }
    }

    /// A digest of what the window shows: open files and their names, the
    /// active file, the side column, the layout, menus and dialogs.
    fn screen_state(&self, ctx: &Context) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for d in &self.docs {
            (d.id, &d.path, d.untitled, d.is_dirty()).hash(&mut h);
        }
        self.active.hash(&mut h);
        (self.ws.side_visible, self.ws.side_view).hash(&mut h);
        (self.ws.quick.open, self.ws.pending_close, &self.message).hash(&mut h);
        (self.ws.tiles.focused, self.ws.tiles.maximised).hash(&mut h);
        if let Some(t) = &self.ws.tiles.tree {
            t.active_tiles().hash(&mut h);
        }
        (egui::Popup::is_any_open(ctx), self.insert.menu.open).hash(&mut h);
        h.finish()
    }

    /// Gives every file without a name its own "Untitled-N" (the lowest number
    /// no open file uses).
    pub(crate) fn name_untitled(&mut self) {
        for i in 0..self.docs.len() {
            if self.docs[i].path.is_none() && self.docs[i].untitled == 0 {
                let n = (1..)
                    .find(|n| {
                        self.docs
                            .iter()
                            .all(|d| d.path.is_some() || d.untitled != *n)
                    })
                    .unwrap_or(1);
                self.docs[i].untitled = n;
            }
        }
    }

    /// Asks where to save the export, then writes it.
    fn export_dialog_save(&mut self) {
        let format = self.export.format;
        let stem = self
            .doc()
            .and_then(|d| d.path.as_deref())
            .and_then(Path::file_stem)
            .map_or_else(
                || "Untitled".to_string(),
                |s| s.to_string_lossy().to_string(),
            );
        let ext = format.extension();
        let Some(path) = rfd::FileDialog::new()
            .add_filter(crate::export::format_label(format), &[ext])
            .set_file_name(format!("{stem}.{ext}"))
            .save_file()
        else {
            return;
        };
        let msg = match self.export_to(&path) {
            Ok(()) => format!("Wrote {}", explorer::display_name(&path)),
            Err(e) => e,
        };
        self.export.message = Some(msg);
    }

    /// Exports the current file to `path` with the dialog's format and options.
    pub fn export_to(&mut self, path: &Path) -> Result<(), String> {
        let format = self.export.format;
        let options = self.export.options.clone();
        let Some(doc) = self.docs.get_mut(self.active) else {
            return Err("No file is open.".into());
        };
        let exported = crate::export::export_doc(doc, format, &options)?;
        crate::export::write_export(path, &exported)
    }

    /// Shows or hides the Problems tile (Ctrl+Shift+M).
    pub fn toggle_problems(&mut self) {
        self.toggle_pane(crate::layout::Pane::Problems);
        self.problems.open = self.has_pane(&crate::layout::Pane::Problems);
    }

    /// Every problem of the open files and the files they import, grouped by
    /// file (what the Problems panel lists).
    pub fn project_problems(&mut self) -> Vec<crate::problems::FileProblems> {
        crate::problems::gather(&mut self.docs)
    }

    /// The error, warning and note counts of a file (for tab and explorer badges).
    pub fn problem_counts_for(&mut self, path: &Path) -> crate::diagnostics::Counts {
        crate::problems::file_counts(&mut self.docs)
            .into_iter()
            .filter(|(p, _)| p == path)
            .map(|(_, c)| c)
            .fold(
                crate::diagnostics::Counts::default(),
                crate::diagnostics::Counts::plus,
            )
    }

    /// The counts of every file with problems (open files and their imports).
    pub fn file_counts(&mut self) -> Vec<(PathBuf, crate::diagnostics::Counts)> {
        crate::problems::file_counts(&mut self.docs)
    }

    /// The number of errors in a file (0 when it is not open or imported).
    pub fn error_count(&mut self, path: &Path) -> usize {
        self.problem_counts_for(path).errors
    }

    /// Goes to a problem chosen in the Problems panel.
    pub fn jump_to(&mut self, jump: &crate::problems::Jump) {
        let (tab, span) = match jump {
            crate::problems::Jump::Open { tab, span } => (Some(*tab), *span),
            crate::problems::Jump::File { path, span } => match self.open_file(path, false) {
                Ok(i) => (Some(i), *span),
                Err(m) => {
                    self.message = Some(m);
                    (None, *span)
                }
            },
        };
        let Some(i) = tab.filter(|i| *i < self.docs.len()) else {
            return;
        };
        self.active = i;
        if let Some(doc) = self.docs.get_mut(i) {
            doc.editor.move_caret(span.start, false);
            doc.scroll_to = Some(span);
        }
        self.diag.peek = Some(span);
    }

    fn file_text_window(&mut self, ctx: &Context) {
        if !self.show_file_text {
            return;
        }
        let text = self
            .doc()
            .map(|d| d.editor.text().to_string())
            .unwrap_or_default();
        let mut open = true;
        egui::Window::new("File text")
            .open(&mut open)
            .default_size([480.0, 360.0])
            .show(ctx, |ui| {
                ui.weak("Read only");
                egui::ScrollArea::both().show(ui, |ui| {
                    let mut shown: &str = &text;
                    ui.add(
                        egui::TextEdit::multiline(&mut shown)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .desired_rows(14),
                    );
                });
            });
        if !open {
            self.show_file_text = false;
        }
    }
}
