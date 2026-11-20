//! The files panel: a flat list of the `.mpp` files in the project folder.
//! (A tree with folders replaces it later; keep this module's surface small.)

use std::path::{Path, PathBuf};

/// The `.mpp` files directly inside `dir`, sorted by name.
pub fn list_mpp(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut files: Vec<PathBuf> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "mpp"))
        .collect();
    files.sort_by_key(|p| p.file_name().map(|n| n.to_ascii_lowercase_lossy()));
    files
}

trait LowerLossy {
    fn to_ascii_lowercase_lossy(&self) -> String;
}

impl LowerLossy for std::ffi::OsStr {
    fn to_ascii_lowercase_lossy(&self) -> String {
        self.to_string_lossy().to_lowercase()
    }
}

/// A file's display name: no folders, no extension.
pub fn display_name(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".into())
}

/// Draws the list and returns the file the user clicked.
pub fn show(ui: &mut egui::Ui, files: &[PathBuf], current: Option<&Path>) -> Option<PathBuf> {
    let mut clicked = None;
    if files.is_empty() {
        ui.weak("No files here yet.");
    }
    for f in files {
        let selected = current == Some(f.as_path());
        if ui.selectable_label(selected, display_name(f)).clicked() {
            clicked = Some(f.clone());
        }
    }
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_have_no_extension() {
        assert_eq!(display_name(Path::new("a/b/mechanics.mpp")), "mechanics");
    }

    #[test]
    fn lists_only_mpp_files() {
        let dir = std::env::temp_dir().join(format!("mpp-ide-list-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("b.mpp"), "x");
        let _ = std::fs::write(dir.join("a.mpp"), "x");
        let _ = std::fs::write(dir.join("c.txt"), "x");
        let names: Vec<String> = list_mpp(&dir).iter().map(|p| display_name(p)).collect();
        assert_eq!(names, vec!["a", "b"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
