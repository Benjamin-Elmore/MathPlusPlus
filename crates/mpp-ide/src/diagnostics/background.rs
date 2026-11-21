//! Work done on the evaluation thread after each evaluation: the problems in
//! every imported file (summarised on the import line, listed in the Problems
//! panel) and an index of the names the other files of the folder define (so
//! "Undefined: θ" can say "import it from launch.mpp").
//!
//! Files are read and evaluated once and kept until they change on disk.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use mpp_eval::{Evaluated, NoImports, Session};
use mpp_syntax::document::parse_document;

use super::{gather, Inputs, Problem};
use crate::eval::FsImporter;

/// The problems of one imported file.
#[derive(Debug, Clone)]
pub struct ImportSummary {
    /// The path as written in the import (`mechanics.mpp`).
    pub written: String,
    /// The file on disk.
    pub path: PathBuf,
    /// The name shown to the user.
    pub display: String,
    /// Its problems, with spans in that file's text.
    pub problems: Vec<Problem>,
}

/// The names each `.mpp` file of the folder defines.
#[derive(Debug, Clone, Default)]
pub struct ProjectIndex {
    /// (name shown, path, names defined), sorted by name shown.
    pub files: Vec<(String, PathBuf, Vec<String>)>,
}

impl ProjectIndex {
    /// The files that define `name`.
    pub fn files_defining(&self, name: &str) -> Vec<String> {
        self.files
            .iter()
            .filter(|(_, _, names)| names.iter().any(|n| n == name))
            .map(|(d, _, _)| d.clone())
            .collect()
    }

    /// Every file name.
    pub fn file_names(&self) -> Vec<String> {
        self.files.iter().map(|(d, _, _)| d.clone()).collect()
    }

    /// The path of a file by the name shown.
    pub fn path_of(&self, display: &str) -> Option<&Path> {
        self.files
            .iter()
            .find(|(d, _, _)| d == display)
            .map(|(_, p, _)| p.as_path())
    }
}

/// What the evaluation thread found besides the evaluation itself.
#[derive(Debug, Clone, Default)]
pub struct Background {
    pub imports: Vec<ImportSummary>,
    pub index: ProjectIndex,
}

impl Background {
    /// The summary for an imported file by its path on disk.
    pub fn import_of(&self, path: &Path) -> Option<&ImportSummary> {
        self.imports.iter().find(|s| s.path == path)
    }
}

/// Most files the index reads from one folder.
const MAX_FILES: usize = 64;
/// Largest file the index reads.
const MAX_BYTES: u64 = 512 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Stamp {
    modified: Option<SystemTime>,
    len: u64,
}

fn stamp(path: &Path) -> Option<Stamp> {
    let m = std::fs::metadata(path).ok()?;
    Some(Stamp {
        modified: m.modified().ok(),
        len: m.len(),
    })
}

/// Read and evaluated files, kept until they change on disk.
#[derive(Default)]
pub struct FileCache {
    problems: HashMap<PathBuf, (Stamp, Arc<Vec<Problem>>)>,
    names: HashMap<PathBuf, (Stamp, Arc<Vec<String>>)>,
}

impl std::fmt::Debug for FileCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileCache")
            .field("files", &self.problems.len())
            .finish()
    }
}

/// The name of a file as shown to the user.
pub fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Resolves an import path the way the importer does.
pub fn resolve(base: Option<&Path>, written: &str) -> PathBuf {
    let mut p = base.unwrap_or_else(|| Path::new(".")).join(written);
    if p.extension().is_none() {
        p.set_extension("mpp");
    }
    p
}

impl FileCache {
    /// The problems of a file, evaluated with its own imports.
    pub fn problems_of(&mut self, path: &Path) -> Option<Arc<Vec<Problem>>> {
        let st = stamp(path)?;
        if let Some((s, p)) = self.problems.get(path) {
            if *s == st {
                return Some(p.clone());
            }
        }
        if st.len > MAX_BYTES {
            return None;
        }
        let text = std::fs::read_to_string(path).ok()?;
        let mut session = Session::new().with_importer(Arc::new(FsImporter::default()));
        session.set_base_dir(path.parent().map(Path::to_path_buf));
        let ev: &Evaluated = session.run_str(&text);
        let doc = parse_document(&text);
        let inputs = Inputs {
            src: &text,
            doc: &doc,
            eval: Some(ev),
            eval_fresh: true,
            background: None,
            quiet: None,
        };
        let found = Arc::new(gather(&inputs));
        self.problems
            .insert(path.to_path_buf(), (st, found.clone()));
        Some(found)
    }

    /// The names a file defines (read without its imports).
    pub fn names_of(&mut self, path: &Path) -> Option<Arc<Vec<String>>> {
        let st = stamp(path)?;
        if let Some((s, n)) = self.names.get(path) {
            if *s == st {
                return Some(n.clone());
            }
        }
        if st.len > MAX_BYTES {
            return None;
        }
        let text = std::fs::read_to_string(path).ok()?;
        let mut session = Session::new().with_importer(Arc::new(NoImports));
        let ev = session.run_str(&text);
        let mut names: Vec<String> = ev
            .blocks
            .iter()
            .flat_map(|b| b.defines.iter().cloned())
            .collect();
        names.sort();
        names.dedup();
        let names = Arc::new(names);
        self.names.insert(path.to_path_buf(), (st, names.clone()));
        Some(names)
    }
}

/// The `.mpp` files of a folder, sorted.
fn mpp_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut v: Vec<PathBuf> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "mpp"))
        .take(MAX_FILES)
        .collect();
    v.sort();
    v
}

/// Computes the import summaries and the project index for an evaluation of
/// the file at `this` (or of an unsaved text with `base` as its folder).
pub fn compute(
    ev: &Evaluated,
    base: Option<&Path>,
    this: Option<&Path>,
    cache: &mut FileCache,
) -> Background {
    let mut imports = vec![];
    for b in &ev.blocks {
        for i in b.imports.iter().filter(|i| i.loaded) {
            let path = resolve(base, &i.path);
            if imports.iter().any(|s: &ImportSummary| s.path == path) {
                continue;
            }
            let Some(problems) = cache.problems_of(&path) else {
                continue;
            };
            imports.push(ImportSummary {
                written: i.path.clone(),
                display: display_name(&path),
                path,
                problems: problems.as_ref().clone(),
            });
        }
    }
    let mut index = ProjectIndex::default();
    if let Some(dir) = base {
        for p in mpp_files(dir) {
            if this.is_some_and(|t| t == p) {
                continue;
            }
            if let Some(names) = cache.names_of(&p) {
                index
                    .files
                    .push((display_name(&p), p.clone(), names.as_ref().clone()));
            }
        }
    }
    Background { imports, index }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imported_files_are_summarised_and_indexed() {
        let dir = std::env::temp_dir().join(format!("mpp-bg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("lib.mpp"), "$$ (x + 1 $$\n\nLet $\\theta := 2$.\n").expect("w");
        let main = "![[lib.mpp]]\n\n$$ \\theta = ? $$\n";
        let mut s = Session::new().with_importer(Arc::new(FsImporter::default()));
        s.set_base_dir(Some(dir.clone()));
        let ev = s.run_str(main).clone();
        let mut cache = FileCache::default();
        let bg = compute(&ev, Some(&dir), None, &mut cache);
        assert_eq!(bg.imports.len(), 1);
        assert_eq!(bg.imports[0].display, "lib.mpp");
        assert!(!bg.imports[0].problems.is_empty());
        assert_eq!(bg.index.files_defining("θ"), ["lib.mpp"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
