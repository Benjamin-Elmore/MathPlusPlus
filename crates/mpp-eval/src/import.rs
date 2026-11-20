//! The import hook: `![[file.mpp]]` and `Using $a, f$ from [[file.mpp]].`
//!
//! The session asks its [`Importer`] for the names a file exports. The default
//! importer is a [`crate::files::FileImporter`]; [`NoImports`] loads nothing and
//! leaves the names a `Using` sentence lists without values (with a warning).

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mpp_syntax::{Diagnostic, Span};

use crate::holders::Holders;
use crate::value::Binding;

/// One import a block asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportRequest {
    /// The path as written (relative to the importing file).
    pub path: String,
    pub span: Span,
    /// `(name in the file, name here)` pairs, or `None` to import every name.
    pub names: Option<Vec<(String, String)>>,
}

/// A problem found inside an imported file, with the file it is in. Its span is a
/// byte range of that file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiagnostic {
    /// The file as shown to the user.
    pub file: Arc<str>,
    pub diagnostic: Diagnostic,
}

impl fmt::Display for FileDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.file, self.diagnostic.message)
    }
}

/// The names a file exports.
#[derive(Debug, Clone, Default)]
pub struct Exports {
    /// The file the names come from, as shown to the user.
    pub file: Arc<str>,
    /// The resolved path of the file, when it is a real file.
    pub path: Option<Arc<Path>>,
    /// Every top-level definition of the file.
    pub bindings: BTreeMap<String, Arc<Binding>>,
    /// Everything reported while running the file, and the files it imports.
    pub diagnostics: Vec<FileDiagnostic>,
    /// How many errors the file itself has (the summary on the import line).
    pub error_count: usize,
    /// Every file that went into these exports (itself first), for watching.
    pub files: Vec<PathBuf>,
    /// The result depends on who imported the file (an import loop, or a file that
    /// could not be loaded), so it must not be cached.
    pub uncacheable: bool,
}

/// Why an import failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportErrorKind {
    /// The importer does not load files at all (a warning, not an error).
    NotLoaded,
    /// The file could not be found or read.
    Missing,
    /// The file imports itself, directly or through other files.
    Loop,
    /// Anything else (imports nested too deeply).
    Other,
}

/// An import that could not be carried out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportError {
    pub kind: ImportErrorKind,
    pub message: String,
}

impl ImportError {
    /// An error of `kind`.
    pub fn new(kind: ImportErrorKind, message: impl Into<String>) -> Self {
        ImportError {
            kind,
            message: message.into(),
        }
    }

    /// "The importer does not load files."
    pub fn not_loaded(message: impl Into<String>) -> Self {
        ImportError::new(ImportErrorKind::NotLoaded, message)
    }
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ImportError {}

/// Where an import is made from.
pub struct ImportContext<'a> {
    /// The folder of the importing file; relative paths resolve against it.
    pub base: Option<&'a Path>,
    /// The folder of the document the whole import tree started from, which file
    /// names shown to the user are made relative to.
    pub root_dir: Option<&'a Path>,
    /// The files being loaded right now, outermost first (the import loop check).
    pub chain: &'a [PathBuf],
    /// The holder registry of the importing session, shared with imported files.
    pub holders: &'a Arc<Holders>,
}

/// Supplies the names of imported files.
pub trait Importer: Send + Sync {
    /// The exports of `request.path`, resolved against `base` (the importing
    /// file's folder), or a message saying why they cannot be loaded.
    fn import(&self, base: Option<&Path>, request: &ImportRequest) -> Result<Exports, String>;

    /// Like [`Importer::import`] with the whole context. The default forwards to
    /// `import`; the file importer overrides it to check loops and share holders.
    fn import_in(
        &self,
        ctx: &ImportContext<'_>,
        request: &ImportRequest,
    ) -> Result<Exports, ImportError> {
        self.import(ctx.base, request)
            .map_err(ImportError::not_loaded)
    }

    /// Forgets what is cached for the file at `path`, or for every file when
    /// `None`, so the next import reads it again.
    fn invalidate(&self, _path: Option<&Path>) {}
}

/// An importer that loads nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoImports;

impl Importer for NoImports {
    fn import(&self, _base: Option<&Path>, request: &ImportRequest) -> Result<Exports, String> {
        Err(format!(
            "imports are not loaded yet, so the names from {} have no values",
            request.path
        ))
    }
}
