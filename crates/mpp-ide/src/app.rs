//! The application: menus, panels, tabs and the equation view.

use std::path::{Path, PathBuf};

use egui::{Context, Key, KeyboardShortcut, Modifiers, Ui};

use crate::copy::CopyFormat;
use crate::defs::{self, DefInfo};
use crate::doc::Doc;
use crate::explorer;
use crate::figure::{FigureDrawer, PlaceholderFigure};
use crate::files::Recent;
use crate::layout::{BODY_SIZE, MAX_SIZE, MIN_SIZE};
use crate::sample::WELCOME;
use crate::theme::{self, Mode};
use crate::view::{self, CompletionState, ViewCtx, ViewEvent};
use crate::workspace::Workspace;

const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
const ZOOM_IN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Equals);
const ZOOM_OUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Minus);
const ZOOM_RESET: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Num0);

/// The Math++ IDE.
pub struct IdeApp {
    pub docs: Vec<Doc>,
    pub active: usize,
    pub mode: Mode,
    pub size: f32,
    pub copy_format: CopyFormat,
    /// The read-only raw text pop-out. Off at every start and never saved.
    pub show_file_text: bool,
    pub recent: Recent,
    pub ws: Workspace,
    figures: Box<dyn FigureDrawer>,
    completion: Option<CompletionState>,
    clip: Option<(String, String)>,
    pub(crate) message: Option<String>,
    applied_mode: Option<Mode>,
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
            mode: Mode::Light,
            size: BODY_SIZE,
            copy_format: CopyFormat::Unicode,
            show_file_text: false,
            recent: Recent::default(),
            ws: Workspace::default(),
            figures: Box::new(PlaceholderFigure),
            completion: None,
            clip: None,
            message: None,
            applied_mode: None,
        }
    }

    /// An IDE with no documents open yet (the caller opens files).
    pub fn empty() -> Self {
        let mut a = IdeApp::new();
        a.docs.clear();
        a
    }

    /// Replaces the figure drawer (the plot crate plugs in here).
    pub fn set_figure_drawer(&mut self, drawer: Box<dyn FigureDrawer>) {
        self.figures = drawer;
    }

    /// Reads settings and the recent files from storage.
    pub fn restore(&mut self, storage: &dyn eframe::Storage) {
        if let Some(s) = storage.get_string("recent") {
            self.recent = Recent::from_stored(&s);
        }
        if let Some(s) = storage.get_string("dark") {
            self.mode = if s == "1" { Mode::Dark } else { Mode::Light };
        }
        if let Some(s) = storage
            .get_string("size")
            .and_then(|s| s.parse::<f32>().ok())
        {
            self.size = s.clamp(MIN_SIZE, MAX_SIZE);
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
        storage.set_string(
            "dark",
            if self.mode == Mode::Dark { "1" } else { "0" }.into(),
        );
        storage.set_string("size", self.size.to_string());
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

    fn goto_at(&mut self, offset: usize) {
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
        let (save, open, new, zin, zout, zreset, zdelta) = ctx.input_mut(|i| {
            (
                i.consume_shortcut(&SAVE),
                i.consume_shortcut(&OPEN),
                i.consume_shortcut(&NEW),
                i.consume_shortcut(&ZOOM_IN),
                i.consume_shortcut(&ZOOM_OUT),
                i.consume_shortcut(&ZOOM_RESET),
                i.zoom_delta(),
            )
        });
        if zreset {
            self.size = BODY_SIZE;
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
                ui.separator();
                if ui.button("Close tab").clicked() {
                    self.request_close(self.active);
                    ui.close();
                }
                if ui.button("Quit").clicked() {
                    ui.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| {
                let mut latex = self.copy_format == CopyFormat::Latex;
                if ui.checkbox(&mut latex, "Copy maths as LaTeX").changed() {
                    self.copy_format = if latex {
                        CopyFormat::Latex
                    } else {
                        CopyFormat::Unicode
                    };
                }
                ui.weak("Copying gives Unicode maths by default");
            });
            ui.menu_button("View", |ui| {
                let mut dark = self.mode == Mode::Dark;
                if ui.checkbox(&mut dark, "Dark theme").changed() {
                    self.mode = if dark { Mode::Dark } else { Mode::Light };
                }
                if ui.button("Larger text").clicked() {
                    self.size = (self.size + 1.0).min(MAX_SIZE);
                }
                if ui.button("Smaller text").clicked() {
                    self.size = (self.size - 1.0).max(MIN_SIZE);
                }
                ui.separator();
                if ui
                    .add(egui::Button::new("Side bar").shortcut_text("Ctrl+B"))
                    .clicked()
                {
                    self.toggle_side();
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
                ui.checkbox(&mut self.show_file_text, "Show file text");
            });
        });
    }

    /// Draws the whole window into `ui`.
    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if self.applied_mode != Some(self.mode) {
            theme::apply(&ctx, self.mode);
            self.applied_mode = Some(self.mode);
        }
        self.shortcuts(&ctx);
        let theme = self.mode.theme();

        egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        self.status_ui(ui);
        self.side_ui(ui);
        self.header_ui(ui);

        let central = egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(crate::paint::c32(theme.background)))
            .show(ui, |ui| {
                let (mode, size, copy_format) = (self.mode, self.size, self.copy_format);
                let Some(doc) = self.docs.get_mut(self.active) else {
                    ui.centered_and_justified(|ui| ui.weak("No file open"));
                    return;
                };
                let mut cx = ViewCtx {
                    mode,
                    size,
                    figures: self.figures.as_ref(),
                    completion: &mut self.completion,
                    clip: &mut self.clip,
                    copy_format,
                };
                let events = view::show(ui, doc, &mut cx);
                for e in events {
                    match e {
                        ViewEvent::GotoAt(o) => self.goto_at(o),
                        ViewEvent::OpenLinkAt(o) => self.open_link_at(o),
                        ViewEvent::Save => self.save(),
                    }
                }
            });
        if let Some(src) = central.response.dnd_release_payload::<PathBuf>() {
            self.add_import(&src);
        }

        self.file_text_window(&ctx);
        self.overlays(&ctx);
        if self.ws.explorer.poll() {
            self.reload_changed();
        }
        self.settle_preview();
        self.persist_settings();

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
