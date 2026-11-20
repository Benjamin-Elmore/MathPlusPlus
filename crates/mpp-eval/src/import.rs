//! The import hook: `![[file.mpp]]` and `Using $a, f$ from [[file.mpp]].`
//!
//! The session asks its [`Importer`] for the names a file exports. The default,
//! [`NoImports`], loads nothing and the names a `Using` sentence lists stay without
//! values (with a warning). A file-system importer with caching, loop detection and
//! clash checks plugs in here.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use mpp_syntax::Span;

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

/// The names a file exports.
#[derive(Debug, Clone, Default)]
pub struct Exports {
    /// The file the names come from, as shown to the user.
    pub file: Arc<str>,
    pub bindings: BTreeMap<String, Arc<Binding>>,
}

/// Supplies the names of imported files.
pub trait Importer: Send + Sync {
    /// The exports of `request.path`, resolved against `base` (the importing
    /// file's folder), or a message saying why they cannot be loaded.
    fn import(&self, base: Option<&Path>, request: &ImportRequest) -> Result<Exports, String>;
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
