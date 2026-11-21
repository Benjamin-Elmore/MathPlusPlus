//! File → Export…: the current file as PDF, LaTeX or HTML, through the
//! `mpp-export` library, with the graphs at the sliders' current values.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use mpp_export::{export_evaluated, ExportOptions, Exported, Format, Loaded, PageSize};
use mpp_syntax::document::BlockId;

use crate::doc::Doc;

/// The dialog's state: the format and the options, kept between exports.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportDialog {
    pub open: bool,
    pub format: Format,
    pub options: ExportOptions,
    /// What the last export said (where it wrote, or why it failed).
    pub message: Option<String>,
}

impl Default for ExportDialog {
    fn default() -> Self {
        ExportDialog {
            open: false,
            format: Format::Pdf,
            options: ExportOptions::default(),
            message: None,
        }
    }
}

/// The slider values the reader set on each figure of the file.
pub fn slider_values(doc: &Doc) -> Vec<(BlockId, BTreeMap<String, f64>)> {
    doc.figures
        .list
        .iter()
        .filter(|f| !f.values.is_empty())
        .map(|f| (f.block, f.values.clone()))
        .collect()
}

/// Exports the file as it is evaluated now. `Err` says why not in a sentence.
pub fn export_doc(
    doc: &mut Doc,
    format: Format,
    options: &ExportOptions,
) -> Result<Exported, String> {
    doc.settle();
    let Some(ev) = doc.eval.clone() else {
        return Err("The file has not been worked out yet.".into());
    };
    let base_dir = doc
        .path
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_path_buf);
    let loaded = Loaded {
        evaluated: (*ev).clone(),
        importer: Arc::new(crate::eval::FsImporter::default()),
        base_dir,
    };
    let options = ExportOptions {
        slider_values: slider_values(doc),
        ..options.clone()
    };
    let name = doc.path.as_deref().and_then(Path::file_stem).map_or_else(
        || "document".to_string(),
        |s| s.to_string_lossy().to_string(),
    );
    export_evaluated(&loaded, &name, format, &options).map_err(|e| e.to_string())
}

/// Writes an export: the main file at `path`, the files that go beside it next to it.
pub fn write_export(path: &Path, exported: &Exported) -> Result<(), String> {
    std::fs::write(path, &exported.main)
        .map_err(|e| format!("Could not write {}: {e}", path.display()))?;
    let dir = path.parent().unwrap_or(Path::new("."));
    for (name, bytes) in &exported.files {
        let p = dir.join(name);
        std::fs::write(&p, bytes).map_err(|e| format!("Could not write {}: {e}", p.display()))?;
    }
    Ok(())
}

/// The format's name in the dialog.
pub fn format_label(f: Format) -> &'static str {
    match f {
        Format::Pdf => "PDF",
        Format::Tex => "LaTeX",
        Format::Html => "HTML",
    }
}

/// Draws the dialog. Returns true when the Export button was pressed.
pub fn dialog_ui(ctx: &egui::Context, d: &mut ExportDialog) -> bool {
    if !d.open {
        return false;
    }
    let mut go = false;
    let mut open = true;
    egui::Window::new("Export")
        .open(&mut open)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .resizable(false)
        .default_width(340.0)
        .show(ctx, |ui| {
            ui.label("Format");
            ui.horizontal(|ui| {
                for f in [Format::Pdf, Format::Tex, Format::Html] {
                    ui.radio_value(&mut d.format, f, format_label(f));
                }
            });
            ui.separator();
            let o = &mut d.options;
            ui.checkbox(&mut o.include_results, "Results");
            ui.checkbox(&mut o.include_steps, "Steps");
            ui.checkbox(&mut o.include_comments, "Comments");
            ui.checkbox(&mut o.dark, "Dark page");
            if d.format == Format::Pdf {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Page size");
                    ui.radio_value(&mut o.page, PageSize::A4, "A4");
                    ui.radio_value(&mut o.page, PageSize::Letter, "Letter");
                });
                ui.horizontal(|ui| {
                    ui.label("Margins");
                    ui.add(
                        egui::DragValue::new(&mut o.margin_mm)
                            .range(5.0..=60.0)
                            .speed(0.5)
                            .suffix(" mm"),
                    );
                });
            }
            ui.weak("Graphs are drawn at the sliders\u{2019} current values.");
            if let Some(m) = &d.message {
                ui.separator();
                ui.label(m);
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Export\u{2026}").clicked() {
                    go = true;
                }
                if ui.button("Close").clicked() {
                    d.open = false;
                }
            });
        });
    if !open {
        d.open = false;
    }
    go
}
