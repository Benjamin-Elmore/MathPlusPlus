//! The Math++ IDE.
//!
//! One view: the typeset equation view. You type straight into the page; the
//! `.mpp` text lives behind it and is never shown unless View → Show file text
//! is switched on (off at every start).

#![forbid(unsafe_code)]

mod app;
pub mod copy;
pub mod defs;
pub mod diagnostics;
mod doc;
pub mod editor;
pub mod eval;
pub mod explorer;
pub mod figure;
pub mod files;
pub mod layout;
pub mod page;
pub mod paint;
mod sample;
pub mod tabs;
pub mod theme;
pub mod typeset;
pub mod view;

pub use app::IdeApp;
pub use doc::{Doc, PageCache};
pub use sample::WELCOME;

/// The eframe wrapper around [`IdeApp`].
pub struct EframeApp(pub IdeApp);

impl EframeApp {
    /// Builds the app for a window: installs fonts, restores settings, opens `paths`.
    pub fn new(cc: &eframe::CreationContext<'_>, paths: &[std::path::PathBuf]) -> Self {
        let mut app = IdeApp::new();
        if let Some(s) = cc.storage {
            app.restore(s);
        }
        theme::install_fonts(&cc.egui_ctx);
        app.mode = app.theme_choice.resolve(cc.egui_ctx.system_theme());
        theme::apply(&cc.egui_ctx, app.mode);
        if !paths.is_empty() {
            app.docs.clear();
            for p in paths {
                if let Err(m) = app.open_path(p) {
                    eprintln!("{m}");
                }
            }
            if app.docs.is_empty() {
                app.new_file();
            }
        }
        EframeApp(app)
    }
}

impl eframe::App for EframeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui(ui);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.0.save_state(storage);
    }
}
