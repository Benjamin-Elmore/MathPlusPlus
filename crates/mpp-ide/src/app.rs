//! The application: menus, panels, tabs and the equation view.

use std::path::{Path, PathBuf};

use egui::{Context, Key, KeyboardShortcut, Modifiers, Ui};

use crate::copy::CopyFormat;
use crate::defs::{self, DefInfo};
use crate::doc::Doc;
use crate::explorer;
use crate::figure::{FigureDrawer, PlaceholderFigure};
use crate::files::Recent;
use crate::insert::{InsertSettings, InsertUi};
use crate::layout::{BODY_SIZE, MAX_SIZE, MIN_SIZE, SIDE_PANEL_WIDTH};
use crate::sample::WELCOME;
use crate::tabs::{self, Tab, TabAction};
use crate::theme::{self, Mode, ThemeChoice};
use crate::typeset;
use crate::view::{self, CompletionState, ViewCtx, ViewEvent};

const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
const ZOOM_IN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Equals);
const ZOOM_OUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Minus);
/// Ctrl+K then Ctrl+T switches the theme.
const CHORD_K: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::K);
const CHORD_T: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::T);
/// Shows or hides the Problems panel.
const PROBLEMS: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::M);
/// How long the second key of a chord may take.
const CHORD_SECONDS: f64 = 2.0;
/// Size of the status bar text (Latin Modern Sans).
const STATUS_SIZE: f32 = 14.0;

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
    pub recent: Recent,
    folder: Option<PathBuf>,
    files: Vec<PathBuf>,
    figures: Box<dyn FigureDrawer>,
    completion: Option<CompletionState>,
    clip: Option<(String, String)>,
    message: Option<String>,
    applied_mode: Option<Mode>,
    /// Set by Ctrl+K while waiting for the second key of a chord (time of press).
    chord_k: Option<f64>,
    system_theme: Option<egui::Theme>,
    /// The Insert menu, its recently used items and its settings.
    pub insert: InsertUi,
    /// The quick-fix menu and the problem card kept open by F8.
    pub diag: crate::diagnostics::ui::DiagUi,
    /// The Problems panel (Ctrl+Shift+M).
    pub problems: crate::problems::ProblemsView,
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
            recent: Recent::default(),
            folder: None,
            files: vec![],
            figures: Box::new(PlaceholderFigure),
            completion: None,
            clip: None,
            message: None,
            applied_mode: None,
            chord_k: None,
            system_theme: None,
            insert: InsertUi::default(),
            diag: Default::default(),
            problems: Default::default(),
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
        self.set_folder(path.parent());
        Ok(self.active)
    }

    fn set_folder(&mut self, dir: Option<&Path>) {
        self.folder = dir.map(Path::to_path_buf);
        self.refresh_files();
    }

    fn refresh_files(&mut self) {
        self.files = self
            .folder
            .as_deref()
            .map(explorer::list_mpp)
            .unwrap_or_default();
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
        self.refresh_files();
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
                self.set_folder(path.parent());
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

    fn close_tab(&mut self, i: usize) {
        if i < self.docs.len() {
            self.docs.remove(i);
            if self.active >= self.docs.len() {
                self.active = self.docs.len().saturating_sub(1);
            }
        }
    }

    /// Jumps to a definition: in this file, or in the file it came from.
    pub fn goto_definition(&mut self, info: &DefInfo) {
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
                match self.open_path(&path) {
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
        let (save, open, new, zin, zout) = ctx.input_mut(|i| {
            (
                i.consume_shortcut(&SAVE),
                i.consume_shortcut(&OPEN),
                i.consume_shortcut(&NEW),
                i.consume_shortcut(&ZOOM_IN),
                i.consume_shortcut(&ZOOM_OUT),
            )
        });
        let (chord_k, chord_t, now) = ctx.input_mut(|i| {
            (
                i.consume_shortcut(&CHORD_K),
                i.consume_shortcut(&CHORD_T),
                i.time,
            )
        });
        if ctx.input_mut(|i| i.consume_shortcut(&PROBLEMS)) {
            self.problems.toggle();
        }
        if chord_k {
            self.chord_k = Some(now);
        }
        if chord_t {
            if self.chord_k.is_some_and(|t| now - t <= CHORD_SECONDS) {
                self.set_theme_choice(self.theme_choice.next());
            }
            self.chord_k = None;
        }
        if self.chord_k.is_some_and(|t| now - t > CHORD_SECONDS) {
            self.chord_k = None;
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
                    self.close_tab(self.active);
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
                ui.separator();
                ui.checkbox(&mut self.show_file_text, "Show file text");
            });
            if crate::insert::menu::insert_button(ui).clicked() {
                self.insert.menu.open_at(None);
            }
        });
    }

    fn side_panel(&mut self, ui: &mut Ui) {
        let theme = self.mode.theme();
        ui.heading("Files");
        let current = self.doc().and_then(|d| d.path.clone());
        if let Some(f) = explorer::show(ui, &self.files, current.as_deref()) {
            if let Err(m) = self.open_path(&f) {
                self.message = Some(m);
            }
        }
        ui.add_space(10.0);
        ui.separator();
        ui.heading("Variables");
        let vars: Vec<DefInfo> = self
            .doc()
            .and_then(|d| d.eval.as_ref().map(|e| defs::all(e)))
            .unwrap_or_default();
        if vars.is_empty() {
            ui.weak("Nothing defined yet.");
        }
        let mut jump = None;
        egui::ScrollArea::vertical().id_salt("vars").show(ui, |ui| {
            for d in &vars {
                let r = typeset::show(ui, &defs::summary_latex(d), 17.0, theme);
                if r.clicked() {
                    jump = Some(d.clone());
                }
                ui.add_space(3.0);
            }
        });
        if let Some(d) = jump {
            self.goto_definition(&d);
        }
    }

    /// Draws the whole window into `ui`.
    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.shortcuts(&ctx);
        self.system_theme = ctx.system_theme();
        self.mode = self.theme_choice.resolve(self.system_theme);
        if self.applied_mode != Some(self.mode) {
            theme::apply(&ctx, self.mode);
            self.applied_mode = Some(self.mode);
        }
        let theme = self.mode.theme();

        egui::Panel::top("menu").show(ui, |ui| {
            self.menu(ui);
            if !self.docs.is_empty() {
                let tab_list: Vec<Tab> = self
                    .docs
                    .iter()
                    .map(|d| Tab {
                        title: d.title(),
                        dirty: d.is_dirty(),
                        errors: d.error_count(),
                    })
                    .collect();
                match tabs::show(ui, &tab_list, self.active, theme::c32(theme.error)) {
                    Some(TabAction::Select(i)) => self.active = i,
                    Some(TabAction::Close(i)) => self.close_tab(i),
                    None => {}
                }
            }
        });

        egui::Panel::bottom("status").show(ui, |ui| {
            let small = |s: &str| egui::RichText::new(s).font(theme::sans(STATUS_SIZE));
            ui.horizontal(|ui| {
                let evaluating = self.doc().is_some_and(Doc::evaluating);
                if evaluating {
                    ui.spinner();
                    ui.label(small("Working\u{2026}"));
                } else if let Some(d) = self.doc() {
                    let c = d.problem_counts();
                    let color = match c.worst() {
                        Some(crate::diagnostics::Level::Error) => theme::c32(theme.error),
                        Some(crate::diagnostics::Level::Warning) => theme::c32(theme.warning),
                        _ => ui.visuals().text_color(),
                    };
                    let text = if c.problems() == 0 {
                        "Ready".to_string()
                    } else {
                        c.summary()
                    };
                    // Clicking the count opens the Problems panel, like Ctrl+Shift+M.
                    if ui
                        .add(
                            egui::Label::new(small(&text).color(color)).sense(egui::Sense::click()),
                        )
                        .on_hover_text(small("Problems (Ctrl+Shift+M)"))
                        .clicked()
                    {
                        self.problems.toggle();
                    }
                }
                if let Some(m) = &self.message {
                    ui.separator();
                    ui.label(small(m).color(theme::c32(theme.error)));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let tip = format!(
                        "Theme: {} (Ctrl+K, then Ctrl+T, switches)",
                        self.theme_choice.label()
                    );
                    if theme::moon_button(ui, STATUS_SIZE)
                        .on_hover_text(small(&tip))
                        .clicked()
                    {
                        self.set_theme_choice(self.theme_choice.next());
                    }
                    ui.label(small(self.theme_choice.label()));
                });
            });
        });

        self.problems_panel(ui);

        egui::Panel::left("side")
            .default_size(SIDE_PANEL_WIDTH)
            .resizable(true)
            .show(ui, |ui| self.side_panel(ui));

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(theme::c32(theme.background)))
            .show(ui, |ui| {
                let (mode, size, copy_format) = (self.mode, self.size, self.copy_format);
                let Some(doc) = self.docs.get_mut(self.active) else {
                    ui.centered_and_justified(|ui| ui.weak("No file open"));
                    return;
                };
                doc.integral_limits = if self.insert.settings.integral_side {
                    mpp_render::IntegralLimits::Side
                } else {
                    mpp_render::IntegralLimits::AboveBelow
                };
                let mut cx = ViewCtx {
                    mode,
                    size,
                    figures: self.figures.as_ref(),
                    completion: &mut self.completion,
                    clip: &mut self.clip,
                    copy_format,
                    insert: &mut self.insert,
                    diag: &mut self.diag,
                };
                let events = view::show(ui, doc, &mut cx);
                for e in events {
                    match e {
                        ViewEvent::GotoAt(o) => self.goto_at(o),
                        ViewEvent::Save => self.save(),
                        ViewEvent::Open(p) => {
                            if let Err(m) = self.open_path(&p) {
                                self.message = Some(m);
                            }
                        }
                    }
                }
            });

        self.file_text_window(&ctx);

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

    /// Every problem of the open files and the files they import, grouped by
    /// file (what the Problems panel lists).
    pub fn project_problems(&mut self) -> Vec<crate::problems::FileProblems> {
        crate::problems::gather(&mut self.docs)
    }

    /// The error, warning and note counts of a file (for tab and explorer badges).
    pub fn problem_counts_for(&mut self, path: &Path) -> crate::diagnostics::Counts {
        let list = self.project_problems();
        crate::problems::counts_for(&list, path)
    }

    /// The number of errors in a file (0 when it is not open or imported).
    pub fn error_count(&mut self, path: &Path) -> usize {
        self.problem_counts_for(path).errors
    }

    /// Goes to a problem chosen in the Problems panel.
    pub fn jump_to(&mut self, jump: &crate::problems::Jump) {
        let (tab, span) = match jump {
            crate::problems::Jump::Open { tab, span } => (Some(*tab), *span),
            crate::problems::Jump::File { path, span } => match self.open_path(path) {
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

    /// The Problems panel at the bottom of the window, when open.
    fn problems_panel(&mut self, ui: &mut Ui) {
        if !self.problems.open {
            return;
        }
        let theme = self.mode.theme();
        let list = self.project_problems();
        let size = (self.size * 0.85).max(14.0);
        let mut jump = None;
        egui::Panel::bottom("problems")
            .resizable(true)
            .default_size(crate::problems::PANEL_HEIGHT)
            .show(ui, |ui| {
                jump = self.problems.ui(ui, &list, &theme, size);
            });
        if let Some(j) = jump {
            self.jump_to(&j);
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
