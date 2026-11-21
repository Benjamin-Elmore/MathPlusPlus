//! The folder tree: which rows are visible, in what order.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// One visible row of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub depth: usize,
    pub expanded: bool,
}

impl Row {
    /// True for a Math++ file.
    pub fn is_mpp(&self) -> bool {
        !self.is_dir && is_mpp(&self.path)
    }
}

/// True when the path is a `.mpp` file name.
pub fn is_mpp(p: &Path) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("mpp"))
}

/// The entries of one folder: folders first, then files, A to Z. Hidden
/// (dot) entries are left out.
pub fn read_dir_sorted(dir: &Path) -> Vec<(PathBuf, bool)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut v: Vec<(PathBuf, bool)> = rd
        .filter_map(Result::ok)
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| {
            let p = e.path();
            let d = p.is_dir();
            (p, d)
        })
        .collect();
    v.sort_by(|a, b| {
        b.1.cmp(&a.1).then_with(|| {
            let na = a.0.file_name().map(|n| n.to_string_lossy().to_lowercase());
            let nb = b.0.file_name().map(|n| n.to_string_lossy().to_lowercase());
            na.cmp(&nb)
        })
    });
    v
}

/// The visible rows below `root` given the expanded folders.
pub fn rows(root: &Path, expanded: &BTreeSet<PathBuf>) -> Vec<Row> {
    let mut out = vec![];
    walk(root, expanded, 0, &mut out);
    out
}

fn walk(dir: &Path, expanded: &BTreeSet<PathBuf>, depth: usize, out: &mut Vec<Row>) {
    for (path, is_dir) in read_dir_sorted(dir) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let open = is_dir && expanded.contains(&path);
        out.push(Row {
            path: path.clone(),
            name,
            is_dir,
            depth,
            expanded: open,
        });
        if open {
            walk(&path, expanded, depth + 1, out);
        }
    }
}

/// Every `.mpp` file below `root` (hidden folders skipped), in tree order.
pub fn all_mpp(root: &Path) -> Vec<PathBuf> {
    fn go(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
        if depth > 12 {
            return;
        }
        for (p, d) in read_dir_sorted(dir) {
            if d {
                go(&p, out, depth + 1);
            } else if is_mpp(&p) {
                out.push(p);
            }
        }
    }
    let mut v = vec![];
    go(root, &mut v, 0);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_first_then_files_a_to_z() {
        let dir = std::env::temp_dir().join(format!("mpp-tree-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("zeta")).expect("mkdir");
        std::fs::create_dir_all(dir.join("Alpha")).expect("mkdir");
        for f in ["b.mpp", "A.mpp", "c.txt", ".hidden"] {
            std::fs::write(dir.join(f), "x").expect("write");
        }
        let r = rows(&dir, &BTreeSet::new());
        let names: Vec<&str> = r.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["Alpha", "zeta", "A.mpp", "b.mpp", "c.txt"]);
        assert!(r[2].is_mpp() && !r[4].is_mpp());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
