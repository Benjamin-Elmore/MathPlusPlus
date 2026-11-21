//! Evaluated documents out into the world: the static HTML page, PDF and LaTeX.
//!
//! [`export_path`] and [`export_evaluated`] are the library API the command line
//! and the IDE's File → Export… call. Every format reads the same evaluated
//! document: results are filled in, graphs are drawn at their current slider
//! values, and the options say what to leave out.

#![forbid(unsafe_code)]

mod pdf;
mod provider;
mod tex;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mpp_eval::{Evaluated, FileImporter, Importer, Session};
use mpp_render::{HtmlOptions, Theme};
use mpp_syntax::document::BlockId;

pub use pdf::{pdf_pages_svg, PageSize};
pub use provider::EvalProvider;

/// The export formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Pdf,
    Tex,
    Html,
}

impl Format {
    /// Reads `pdf`, `tex`/`latex` or `html`.
    pub fn parse(s: &str) -> Option<Format> {
        match s.to_ascii_lowercase().as_str() {
            "pdf" => Some(Format::Pdf),
            "tex" | "latex" => Some(Format::Tex),
            "html" | "htm" => Some(Format::Html),
            _ => None,
        }
    }

    /// The file extension.
    pub fn extension(self) -> &'static str {
        match self {
            Format::Pdf => "pdf",
            Format::Tex => "tex",
            Format::Html => "html",
        }
    }
}

/// What to put in an export.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportOptions {
    /// Fill in the answers of `= ?` queries (default yes).
    pub include_results: bool,
    /// Show the working of `= ?!` queries (default no).
    pub include_steps: bool,
    /// Show formatted comments as notes (default no).
    pub include_comments: bool,
    /// Dark page and graphs; the default is light, which prints well.
    pub dark: bool,
    /// Paper size of a PDF.
    pub page: PageSize,
    /// Margin of a PDF in millimetres.
    pub margin_mm: f32,
    /// Slider values set in the editor, by figure: graphs are drawn at them
    /// (empty: the values written in the file).
    pub slider_values: Vec<(BlockId, BTreeMap<String, f64>)>,
}

impl Default for ExportOptions {
    fn default() -> Self {
        ExportOptions {
            include_results: true,
            include_steps: false,
            include_comments: false,
            dark: false,
            page: PageSize::A4,
            margin_mm: 25.0,
            slider_values: Vec::new(),
        }
    }
}

/// An export: the main file and the files that go beside it (a `.tex` file's figures).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Exported {
    pub main: Vec<u8>,
    pub files: Vec<(String, Vec<u8>)>,
}

/// Why an export failed.
#[derive(Debug)]
pub enum ExportError {
    Io(String),
    Pdf(String),
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportError::Io(m) | ExportError::Pdf(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for ExportError {}

/// A document read from a file and run, with what exports need.
pub struct Loaded {
    pub evaluated: Evaluated,
    pub importer: Arc<dyn Importer>,
    pub base_dir: Option<PathBuf>,
}

/// Reads and runs the file at `path` (imports resolve against its folder).
pub fn load(path: &Path) -> Result<Loaded, ExportError> {
    let src = mpp_eval::read_text(path)
        .map_err(|e| ExportError::Io(format!("could not read {}: {e}", path.display())))?;
    let mut session = Session::new().with_importer(Arc::new(FileImporter::new()));
    session.set_file(path);
    let evaluated = session.run_str(&src).clone();
    Ok(Loaded {
        evaluated,
        importer: session.importer(),
        base_dir: path.parent().map(Path::to_path_buf),
    })
}

/// Runs the file at `path` and exports it.
pub fn export_path(
    path: &Path,
    format: Format,
    opts: &ExportOptions,
) -> Result<Exported, ExportError> {
    let loaded = load(path)?;
    let name = path.file_stem().map_or_else(
        || "document".to_string(),
        |s| s.to_string_lossy().to_string(),
    );
    export_evaluated(&loaded, &name, format, opts)
}

/// Exports an evaluated document. `name` is the file stem (the figures of a
/// `.tex` file are named after it).
pub fn export_evaluated(
    loaded: &Loaded,
    name: &str,
    format: Format,
    opts: &ExportOptions,
) -> Result<Exported, ExportError> {
    let mut provider = EvalProvider::new(
        &loaded.evaluated,
        loaded.importer.clone(),
        loaded.base_dir.clone(),
        opts.dark,
        opts.include_results,
    );
    for (id, values) in &opts.slider_values {
        provider.set_slider_values(*id, values);
    }
    match format {
        Format::Html => Ok(Exported {
            main: html_page(&loaded.evaluated, &provider, opts).into_bytes(),
            files: vec![],
        }),
        Format::Tex => Ok(tex::export(&loaded.evaluated, &provider, name, opts)),
        Format::Pdf => Ok(Exported {
            main: pdf::export(&loaded.evaluated, &provider, opts)?,
            files: vec![],
        }),
    }
}

/// The static page: one self-contained file.
pub fn html_page(ev: &Evaluated, provider: &EvalProvider<'_>, opts: &ExportOptions) -> String {
    let html_opts = HtmlOptions {
        theme: if opts.dark { Theme::DARK } else { Theme::LIGHT },
        dark_theme: None,
        include_comments: opts.include_comments,
        include_steps: opts.include_steps,
        ..HtmlOptions::default()
    };
    mpp_render::render_html_with(&ev.document, provider, &html_opts)
}

/// The page for `mpp render`: results and graphs, adapting to the reader's light or dark setting.
pub fn render_page(loaded: &Loaded) -> String {
    let provider = EvalProvider::new(
        &loaded.evaluated,
        loaded.importer.clone(),
        loaded.base_dir.clone(),
        false,
        true,
    );
    mpp_render::render_html_with(
        &loaded.evaluated.document,
        &provider,
        &HtmlOptions {
            include_comments: true,
            ..HtmlOptions::default()
        },
    )
}
