//! Autosave and crash recovery.
//!
//! Autosave writes a file in place (after a second of quiet, or when the window
//! loses focus, or never: a setting). Recovery copies are separate: every few
//! seconds the unsaved text of each changed file goes to the app-data folder
//! (never into the project), and is removed when the file is saved or closed.
//! After a crash the copies are still there, and start-up offers to restore them
//! with a typeset side-by-side compare.

pub mod compare;
mod tick;

pub use tick::{AutosaveState, RecoveryDialog};

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// An unsaved file found after a crash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recovered {
    /// What the copy is filed under (see [`key_for`]).
    pub key: String,
    /// The file's path, or `None` for a file that never had a name.
    pub path: Option<PathBuf>,
    /// The name shown for it.
    pub title: String,
    /// The unsaved text.
    pub text: String,
}

fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// The recovery key of a file: its path, or a stand-in for an unnamed one.
pub fn key_for(path: Option<&Path>, id: u64) -> String {
    path.map_or_else(
        || format!("untitled-{id}"),
        |p| p.to_string_lossy().into_owned(),
    )
}

/// Where recovery copies live.
pub fn recovery_dir(data: &Path) -> PathBuf {
    data.join("recovery")
}

fn file_of(data: &Path, key: &str) -> PathBuf {
    recovery_dir(data).join(format!("{:016x}.json", fnv(key)))
}

/// Writes (or replaces) the recovery copy for `key`.
pub fn write(data: &Path, key: &str, r: &Recovered) -> std::io::Result<()> {
    std::fs::create_dir_all(recovery_dir(data))?;
    std::fs::write(
        file_of(data, key),
        serde_json::to_string(r).unwrap_or_default(),
    )
}

/// Removes the recovery copy for `key`, if there is one.
pub fn remove(data: &Path, key: &str) {
    let _ = std::fs::remove_file(file_of(data, key));
}

/// Every recovery copy on disk.
pub fn load_all(data: &Path) -> Vec<Recovered> {
    let Ok(rd) = std::fs::read_dir(recovery_dir(data)) else {
        return vec![];
    };
    let mut out: Vec<Recovered> = rd
        .filter_map(Result::ok)
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|s| serde_json::from_str(&s).ok())
        .collect();
    out.sort_by(|a, b| a.title.cmp(&b.title));
    out
}

/// Deletes every recovery copy.
pub fn clear(data: &Path) {
    let _ = std::fs::remove_dir_all(recovery_dir(data));
}

/// Keeps only the copies that differ from what is on disk (a copy that equals
/// its file is stale and is deleted).
pub fn worth_restoring(data: &Path, all: Vec<Recovered>) -> Vec<Recovered> {
    all.into_iter()
        .filter(|r| {
            let same = r
                .path
                .as_ref()
                .and_then(|p| std::fs::read_to_string(p).ok())
                .is_some_and(|disk| disk == r.text);
            if same || r.text.trim().is_empty() && r.path.is_none() {
                remove(data, &r.key);
                false
            } else {
                true
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_are_written_read_and_removed() {
        let dir = std::env::temp_dir().join(format!("mpp-recovery-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let r = Recovered {
            key: "/p/a.mpp".into(),
            path: Some(PathBuf::from("/p/a.mpp")),
            title: "a".into(),
            text: "x".into(),
        };
        write(&dir, "/p/a.mpp", &r).expect("write");
        assert_eq!(load_all(&dir), vec![r.clone()]);
        write(
            &dir,
            "/p/a.mpp",
            &Recovered {
                text: "y".into(),
                ..r
            },
        )
        .expect("replace");
        assert_eq!(load_all(&dir).len(), 1);
        remove(&dir, "/p/a.mpp");
        assert!(load_all(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
