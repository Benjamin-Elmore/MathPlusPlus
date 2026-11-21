//! The file importer: loads `.mpp` files, runs them and caches what they export.
//!
//! Where a file's text comes from goes through the [`SourceProvider`] trait, so a
//! future standard library (`std/…` paths embedded in the binary) is served by
//! [`LayeredSource`], which checks [`EmbeddedSources`] before the file system.
//!
//! Each loaded file is cached by its canonical path. An entry stays valid while
//! the modification time and size of the file and of every file it imports are
//! unchanged; when they differ the content hash decides, so touching a file
//! without editing it costs one read and no re-run.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use mpp_syntax::Severity;

use crate::holders::{Ctx, Holders};
use crate::import::{
    Exports, FileDiagnostic, ImportContext, ImportError, ImportErrorKind, ImportRequest, Importer,
};
use crate::resolve::{Lookup, Resolver};
use crate::session::Session;
use crate::value::Binding;

/// Imports nested deeper than this are refused.
const MAX_DEPTH: usize = 1000;

/// A file whose modification time is closer than this to the moment it was read
/// might still change without its time changing, so its content is compared.
const RACY_WINDOW: Duration = Duration::from_secs(2);

/// Cheap facts about a file that change when it is edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub modified: Option<SystemTime>,
    pub len: u64,
}

/// A file found by a [`SourceProvider`].
#[derive(Debug, Clone)]
pub struct Location {
    /// The identity of the file (its canonical path); the cache key.
    pub key: PathBuf,
    pub stamp: Stamp,
    /// The folder the file's own imports resolve against.
    pub dir: Option<PathBuf>,
}

/// Finds and reads the text of imported files.
pub trait SourceProvider: Send + Sync {
    /// Finds `path` (as written in an import) relative to `base`, without reading
    /// it. The error message names the path that was tried.
    fn locate(&self, base: Option<&Path>, path: &str) -> Result<Location, String>;
    /// Reads the file `key` found by [`SourceProvider::locate`].
    fn read(&self, key: &Path) -> Result<String, String>;
    /// The current stamp of `key`, or `None` when it no longer exists.
    fn stamp(&self, key: &Path) -> Option<Stamp>;
}

/// Files on disk; relative paths resolve against the importing file's folder
/// (or the current folder when there is none).
#[derive(Debug, Clone, Copy, Default)]
pub struct FsSource;

/// Drops the `\\?\` prefix Windows puts on canonical paths.
pub(crate) fn simplify_path(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path,
    }
}

/// The canonical form of a path, or the path as given when it does not exist.
pub(crate) fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path)
        .map(simplify_path)
        .unwrap_or_else(|_| path.to_path_buf())
}

fn stamp_of(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some(Stamp {
        modified: meta.modified().ok(),
        len: meta.len(),
    })
}

impl SourceProvider for FsSource {
    fn locate(&self, base: Option<&Path>, path: &str) -> Result<Location, String> {
        let wanted = Path::new(path);
        let full = if wanted.is_absolute() {
            wanted.to_path_buf()
        } else {
            match base {
                Some(dir) => dir.join(wanted),
                None => std::env::current_dir()
                    .map(|d| d.join(wanted))
                    .unwrap_or_else(|_| wanted.to_path_buf()),
            }
        };
        let key = std::fs::canonicalize(&full)
            .map(simplify_path)
            .map_err(|e| format!("cannot find the file {}: {e}", full.display()))?;
        let stamp = stamp_of(&key).ok_or_else(|| format!("cannot read {}", full.display()))?;
        if key.is_dir() {
            return Err(format!("{} is a folder, not a file", full.display()));
        }
        Ok(Location {
            dir: key.parent().map(Path::to_path_buf),
            key,
            stamp,
        })
    }

    fn read(&self, key: &Path) -> Result<String, String> {
        std::fs::read_to_string(key).map_err(|e| format!("cannot read {}: {e}", key.display()))
    }

    fn stamp(&self, key: &Path) -> Option<Stamp> {
        stamp_of(key)
    }
}

/// Files embedded in the binary, by import path (`std/trig.mpp`).
#[derive(Debug, Clone, Default)]
pub struct EmbeddedSources {
    files: BTreeMap<String, &'static str>,
}

impl EmbeddedSources {
    /// No files.
    pub fn new() -> Self {
        EmbeddedSources::default()
    }

    /// Adds a file under `path` (forward slashes, for example `std/trig.mpp`).
    pub fn with(mut self, path: &str, text: &'static str) -> Self {
        self.files.insert(path.to_string(), text);
        self
    }

    /// True when no file is embedded.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    fn name(path: &str) -> String {
        path.replace('\\', "/").trim_start_matches("./").to_string()
    }
}

impl SourceProvider for EmbeddedSources {
    fn locate(&self, _base: Option<&Path>, path: &str) -> Result<Location, String> {
        let name = Self::name(path);
        match self.files.get(&name) {
            Some(text) => Ok(Location {
                key: PathBuf::from(name),
                stamp: Stamp {
                    modified: None,
                    len: text.len() as u64,
                },
                dir: None,
            }),
            None => Err(format!("{path} is not an embedded file")),
        }
    }

    fn read(&self, key: &Path) -> Result<String, String> {
        let name = Self::name(&key.to_string_lossy());
        self.files
            .get(&name)
            .map(|t| t.to_string())
            .ok_or_else(|| format!("{name} is not an embedded file"))
    }

    fn stamp(&self, key: &Path) -> Option<Stamp> {
        let name = Self::name(&key.to_string_lossy());
        self.files.get(&name).map(|t| Stamp {
            modified: None,
            len: t.len() as u64,
        })
    }
}

/// Serves embedded files first and everything else from `fallback`.
#[derive(Clone)]
pub struct LayeredSource {
    embedded: EmbeddedSources,
    fallback: Arc<dyn SourceProvider>,
}

impl LayeredSource {
    /// `embedded` files shadow `fallback`.
    pub fn new(embedded: EmbeddedSources, fallback: Arc<dyn SourceProvider>) -> Self {
        LayeredSource { embedded, fallback }
    }

    /// Embedded files over the file system.
    pub fn over_files(embedded: EmbeddedSources) -> Self {
        LayeredSource::new(embedded, Arc::new(FsSource))
    }
}

impl SourceProvider for LayeredSource {
    fn locate(&self, base: Option<&Path>, path: &str) -> Result<Location, String> {
        match self.embedded.locate(base, path) {
            Ok(found) => Ok(found),
            Err(_) => self.fallback.locate(base, path),
        }
    }

    fn read(&self, key: &Path) -> Result<String, String> {
        if self.embedded.stamp(key).is_some() && !key.is_absolute() {
            self.embedded.read(key)
        } else {
            self.fallback.read(key)
        }
    }

    fn stamp(&self, key: &Path) -> Option<Stamp> {
        if !key.is_absolute() {
            if let Some(s) = self.embedded.stamp(key) {
                return Some(s);
            }
        }
        self.fallback.stamp(key)
    }
}

/// What the cache remembers about one file.
#[derive(Debug, Clone)]
struct Entry {
    stamp: Stamp,
    hash: u64,
    loaded_at: SystemTime,
    exports: Arc<Exports>,
}

fn hash_text(text: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

struct Inner {
    source: Arc<dyn SourceProvider>,
    cache: Mutex<HashMap<PathBuf, Entry>>,
}

/// Loads imported files from a [`SourceProvider`] (the file system by default),
/// caches them, and reports loops and missing files. Clones share one cache.
#[derive(Clone)]
pub struct FileImporter {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for FileImporter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileImporter")
            .field("cached_files", &self.cached_files().len())
            .finish()
    }
}

impl Default for FileImporter {
    fn default() -> Self {
        FileImporter::new()
    }
}

impl FileImporter {
    /// An importer reading files from disk, with the standard library (`std/…`) first.
    pub fn new() -> Self {
        FileImporter::with_source(Arc::new(LayeredSource::over_files(
            crate::stdlib::embedded(),
        )))
    }

    /// An importer reading through `source`.
    pub fn with_source(source: Arc<dyn SourceProvider>) -> Self {
        FileImporter {
            inner: Arc::new(Inner {
                source,
                cache: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Finds an imported file. A name written without an extension (`[[constants]]`,
    /// as in a wiki link) also finds `constants.mpp`.
    fn locate_import(&self, base: Option<&Path>, written: &str) -> Result<Location, ImportError> {
        let source = &self.inner.source;
        let found = source.locate(base, written).or_else(|first| {
            if Path::new(written).extension().is_none() {
                source
                    .locate(base, &format!("{written}.mpp"))
                    .map_err(|_| first)
            } else {
                Err(first)
            }
        });
        found.map_err(|why| ImportError::new(ImportErrorKind::Missing, why))
    }

    /// The files currently cached, sorted.
    pub fn cached_files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = self.lock().keys().cloned().collect();
        files.sort();
        files
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, Entry>> {
        self.inner
            .cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// True when the cached entry for `key` still matches the file.
    fn entry_is_fresh(&self, key: &Path, entry: &Entry) -> bool {
        let Some(now) = self.inner.source.stamp(key) else {
            return false;
        };
        if now == entry.stamp {
            let settled = match (entry.stamp.modified, entry.loaded_at) {
                (Some(modified), loaded) => loaded
                    .duration_since(modified)
                    .is_ok_and(|age| age > RACY_WINDOW),
                (None, _) => false,
            };
            if settled {
                return true;
            }
        }
        match self.inner.source.read(key) {
            Ok(text) => hash_text(&text) == entry.hash,
            Err(_) => false,
        }
    }

    /// The cached exports for `key`, when they and everything they import are
    /// still current.
    fn cached(&self, key: &Path) -> Option<Arc<Exports>> {
        let (entry, deps) = {
            let cache = self.lock();
            let entry = cache.get(key)?.clone();
            let deps: Vec<(PathBuf, Entry)> = entry
                .exports
                .files
                .iter()
                .filter(|f| f.as_path() != key)
                .map(|f| cache.get(f).cloned().map(|e| (f.clone(), e)))
                .collect::<Option<_>>()?;
            (entry, deps)
        };
        let fresh =
            self.entry_is_fresh(key, &entry) && deps.iter().all(|(k, e)| self.entry_is_fresh(k, e));
        fresh.then(|| entry.exports.clone())
    }

    fn display(&self, key: &Path, root: Option<&Path>) -> String {
        let shown = match root {
            Some(root) => {
                let root = canonical(root);
                key.strip_prefix(&root).unwrap_or(key)
            }
            None => key,
        };
        shown
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    }

    fn load(
        &self,
        ctx: &ImportContext<'_>,
        loc: &Location,
        display: &str,
    ) -> Result<Exports, ImportError> {
        let text = self
            .inner
            .source
            .read(&loc.key)
            .map_err(|m| ImportError::new(ImportErrorKind::Missing, m))?;
        let loaded_at = SystemTime::now();
        let hash = hash_text(&text);

        let mut chain = ctx.chain.to_vec();
        chain.push(loc.key.clone());
        let mut session = Session::for_import(
            ctx.holders.clone(),
            Arc::new(self.clone()),
            loc.dir.clone(),
            ctx.root_dir.map(Path::to_path_buf),
            chain,
            display.into(),
        );
        let ev = session.run_nested(&text);

        let path: Arc<Path> = Arc::from(loc.key.as_path());
        let file: Arc<str> = Arc::from(display);
        // A file reachable along several import paths (a diamond) brings the same
        // diagnostics more than once; keep one copy of each, so the lists stay as long as
        // the number of distinct problems rather than doubling per layer.
        let mut seen: HashSet<(Arc<str>, usize, usize, String)> = HashSet::new();
        let mut diagnostics: Vec<FileDiagnostic> = Vec::new();
        let mut push = |d: FileDiagnostic, out: &mut Vec<FileDiagnostic>| {
            let key = (
                d.file.clone(),
                d.diagnostic.span.start,
                d.diagnostic.span.end,
                d.diagnostic.message.clone(),
            );
            if seen.insert(key) {
                out.push(d);
            }
        };
        for d in ev.all_diagnostics() {
            push(
                FileDiagnostic {
                    file: file.clone(),
                    diagnostic: d.clone(),
                },
                &mut diagnostics,
            );
        }
        let mut files = vec![loc.key.clone()];
        // A set beside the list: a long import chain lists every file below, and a
        // linear search per file made loading the chain take cubic time.
        let mut listed: HashSet<&Path> = HashSet::new();
        listed.insert(&loc.key);
        let mut uncacheable = false;
        for block in &ev.blocks {
            for import in &block.imports {
                for d in &import.diagnostics {
                    push(d.clone(), &mut diagnostics);
                }
                for f in &import.files {
                    if listed.insert(f) {
                        files.push(f.clone());
                    }
                }
                uncacheable |= import.uncacheable;
            }
        }
        // Every distinct error in the file and the files it imports, without the
        // "n errors in …" summaries that only point at them.
        let error_count = diagnostics
            .iter()
            .filter(|d| {
                d.diagnostic.severity == Severity::Error
                    && !crate::import::is_import_summary(&d.diagnostic)
            })
            .count();

        let mut bindings = BTreeMap::new();
        for (name, binding) in &ev.bindings {
            if binding.origin().file.is_some() || matches!(**binding, Binding::Imported { .. }) {
                continue;
            }
            let closed = close_over(binding, &ev.bindings, ctx.holders);
            let mut b = (*closed).clone();
            let origin = b.origin_mut();
            origin.file = Some(file.clone());
            origin.path = Some(path.clone());
            origin.line = Some(ev.line_of(origin.span.start));
            bindings.insert(name.clone(), Arc::new(b));
        }

        let exports = Exports {
            file,
            path: Some(path),
            bindings,
            diagnostics,
            error_count,
            files,
            uncacheable,
        };
        if !uncacheable {
            self.lock().insert(
                loc.key.clone(),
                Entry {
                    stamp: loc.stamp,
                    hash,
                    loaded_at,
                    exports: Arc::new(exports.clone()),
                },
            );
        }
        Ok(exports)
    }
}

/// Names a file defines resolve inside the file, so an imported `E := ½ m v²`
/// keeps the file's `m` even if the importing document defines its own.
/// Names the file does not define stay free and take their value where the
/// definition is used.
fn close_over(
    binding: &Arc<Binding>,
    file: &BTreeMap<String, Arc<Binding>>,
    holders: &Arc<Holders>,
) -> Arc<Binding> {
    struct FileScope<'a>(&'a BTreeMap<String, Arc<Binding>>);
    impl Lookup for FileScope<'_> {
        fn lookup(&self, name: &str) -> Option<Arc<Binding>> {
            self.0.get(name).cloned()
        }
    }
    let scope = FileScope(file);
    let ctx = Ctx::new(holders);
    match &**binding {
        Binding::Variable {
            kind,
            value,
            origin,
        } if !binding.is_constant() => {
            let mut resolver = Resolver::new(&scope, &ctx, BTreeMap::new());
            match resolver.run(value) {
                Ok(closed) => Arc::new(Binding::Variable {
                    kind: *kind,
                    value: closed,
                    origin: origin.clone(),
                }),
                Err(_) => binding.clone(),
            }
        }
        Binding::Function {
            params,
            body,
            origin,
        } => {
            let mut resolver = Resolver::new(&scope, &ctx, BTreeMap::new());
            resolver.mask(params.iter().cloned());
            match resolver.run(body) {
                Ok(closed) => Arc::new(Binding::Function {
                    params: params.clone(),
                    body: closed,
                    origin: origin.clone(),
                }),
                Err(_) => binding.clone(),
            }
        }
        _ => binding.clone(),
    }
}

impl Importer for FileImporter {
    fn read_data(&self, base: Option<&Path>, path: &str) -> Result<(PathBuf, String), String> {
        crate::import::read_data_with(&*self.inner.source, base, path)
    }

    fn import(&self, base: Option<&Path>, request: &ImportRequest) -> Result<Exports, String> {
        let holders = Arc::new(Holders::new());
        let ctx = ImportContext {
            base,
            root_dir: base,
            chain: &[],
            holders: &holders,
        };
        self.import_in(&ctx, request).map_err(|e| e.message)
    }

    fn import_in(
        &self,
        ctx: &ImportContext<'_>,
        request: &ImportRequest,
    ) -> Result<Exports, ImportError> {
        let loc = self.locate_import(ctx.base, &request.path)?;
        let root = ctx.root_dir.or(ctx.base);
        let display = self.display(&loc.key, root);

        if ctx.chain.contains(&loc.key) {
            let mut names: Vec<String> = ctx.chain.iter().map(|k| self.display(k, root)).collect();
            names.push(display);
            return Err(ImportError::new(
                ImportErrorKind::Loop,
                format!("import loop: {}", names.join(" → ")),
            ));
        }
        if ctx.chain.len() >= MAX_DEPTH {
            return Err(ImportError::new(
                ImportErrorKind::Other,
                format!("imports are nested more than {MAX_DEPTH} files deep"),
            ));
        }

        if let Some(hit) = self.cached(&loc.key) {
            let mut exports = (*hit).clone();
            exports.file = Arc::from(display.as_str());
            return Ok(exports);
        }
        self.load(ctx, &loc, &display)
    }

    fn invalidate(&self, path: Option<&Path>) {
        let mut cache = self.lock();
        let Some(path) = path else {
            cache.clear();
            return;
        };
        let mut keys: HashSet<PathBuf> = HashSet::new();
        keys.insert(path.to_path_buf());
        keys.insert(canonical(path));
        // A deleted file cannot be canonicalised, but its folder can.
        if let (Some(dir), Some(name)) = (path.parent(), path.file_name()) {
            keys.insert(canonical(dir).join(name));
        }
        cache.retain(|k, _| !keys.contains(k));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_files_come_first() {
        let embedded = EmbeddedSources::new().with("std/a.mpp", "$$ x := 1 $$");
        let layered = LayeredSource::over_files(embedded);
        let loc = layered.locate(None, "std/a.mpp").expect("embedded");
        assert_eq!(loc.key, PathBuf::from("std/a.mpp"));
        assert_eq!(layered.read(&loc.key).as_deref(), Ok("$$ x := 1 $$"));
        assert!(layered.locate(None, "std/missing.mpp").is_err());
    }

    #[test]
    fn simplify_strips_the_verbatim_prefix() {
        assert_eq!(
            simplify_path(PathBuf::from(r"\\?\C:\a\b.mpp")),
            PathBuf::from(r"C:\a\b.mpp")
        );
        assert_eq!(
            simplify_path(PathBuf::from(r"\\?\UNC\srv\a")),
            PathBuf::from(r"\\?\UNC\srv\a")
        );
    }
}
