//! Watching imported files: a change on disk invalidates the importer's cache
//! and tells the caller, who then calls [`crate::Session::update`] again.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::files::canonical;
use crate::import::Importer;
use crate::result::Evaluated;

type Watched = Arc<Mutex<HashSet<PathBuf>>>;

/// Watches files and calls [`Importer::invalidate`] when one changes.
pub struct FileWatcher {
    watcher: RecommendedWatcher,
    watched: Watched,
}

impl std::fmt::Debug for FileWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileWatcher")
            .field("files", &self.files().len())
            .finish()
    }
}

impl FileWatcher {
    /// A watcher that invalidates `importer` (take it from
    /// [`crate::Session::importer`]) and then calls `on_change` with the path of
    /// each watched file that changed, was replaced or was removed.
    /// `on_change` runs on the watcher's thread.
    pub fn new(
        importer: Arc<dyn Importer>,
        on_change: impl Fn(&Path) + Send + 'static,
    ) -> notify::Result<Self> {
        let watched: Watched = Arc::new(Mutex::new(HashSet::new()));
        let seen = watched.clone();
        let watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
            let Ok(event) = event else { return };
            if !matches!(
                event.kind,
                EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
            ) {
                return;
            }
            for path in &event.paths {
                let key = canonical_or_sibling(path);
                let hit = seen
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .contains(&key);
                if hit {
                    importer.invalidate(Some(path));
                    on_change(path);
                }
            }
        })?;
        Ok(FileWatcher { watcher, watched })
    }

    /// Starts watching `path`. The folder is watched so that editors that save by
    /// replacing the file are noticed too.
    pub fn watch(&mut self, path: impl AsRef<Path>) -> notify::Result<()> {
        let key = canonical_or_sibling(path.as_ref());
        let first = self
            .watched
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.clone());
        if first {
            if let Some(dir) = key.parent() {
                self.watcher.watch(dir, RecursiveMode::NonRecursive)?;
            }
        }
        Ok(())
    }

    /// Watches every file `evaluated` imported, directly or indirectly.
    pub fn watch_imports(&mut self, evaluated: &Evaluated) -> notify::Result<()> {
        for file in evaluated.imported_files() {
            self.watch(file)?;
        }
        Ok(())
    }

    /// The files being watched.
    pub fn files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = self
            .watched
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned()
            .collect();
        files.sort();
        files
    }
}

/// The canonical path, or (for a file that was just removed) its folder's
/// canonical path joined with its name.
fn canonical_or_sibling(path: &Path) -> PathBuf {
    if path.exists() {
        return canonical(path);
    }
    match (path.parent(), path.file_name()) {
        (Some(dir), Some(name)) => canonical(dir).join(name),
        _ => path.to_path_buf(),
    }
}
