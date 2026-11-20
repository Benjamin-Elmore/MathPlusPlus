//! Layouts on disk: the current one per project and the named ones, kept in
//! the app-data folder (never inside the project).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Everything saved for one project.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutFile {
    /// The layout in use, as tree JSON.
    pub current: Option<String>,
    /// Layouts the user saved by name, as tree JSON.
    pub named: BTreeMap<String, String>,
}

fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// Where a project's layout file lives.
pub fn layout_path(data_dir: &Path, project: &Path) -> PathBuf {
    data_dir
        .join("layouts")
        .join(format!("{:016x}.json", fnv(&project.to_string_lossy())))
}

/// Reads a project's layout file (defaults when there is none).
pub fn load(data_dir: &Path, project: &Path) -> LayoutFile {
    std::fs::read_to_string(layout_path(data_dir, project))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Writes a project's layout file.
pub fn save(data_dir: &Path, project: &Path, file: &LayoutFile) -> std::io::Result<()> {
    let p = layout_path(data_dir, project);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(p, serde_json::to_string(file).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_are_kept_per_project() {
        let dir = std::env::temp_dir().join(format!("mpp-layouts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut f = LayoutFile::default();
        f.named.insert("Mine".into(), "{}".into());
        save(&dir, Path::new("/a"), &f).expect("save");
        assert_eq!(load(&dir, Path::new("/a")), f);
        assert_eq!(load(&dir, Path::new("/b")), LayoutFile::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
