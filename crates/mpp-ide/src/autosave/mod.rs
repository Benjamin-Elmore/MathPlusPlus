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
    /// The window (see [`instance_id`]) that wrote the copy; empty for copies written
    /// before copies had owners.
    #[serde(default)]
    pub owner: String,
}

/// A name for this window, different for every window and every start: recovery
/// copies carry it, so one window never offers or deletes another's live copies.
pub fn instance_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "{:x}-{:x}-{n}",
        std::process::id(),
        nanos & 0xffff_ffff_ffff
    )
}

/// The lock file a running window holds open (and locked) for as long as it runs.
fn lock_file(data: &Path, owner: &str) -> PathBuf {
    recovery_dir(data).join(format!("{owner}.lock"))
}

/// Creates and locks this window's lock file; the lock goes when the file handle is
/// dropped (also when the process dies).
pub fn hold_lock(data: &Path, owner: &str) -> Option<std::fs::File> {
    std::fs::create_dir_all(recovery_dir(data)).ok()?;
    let f = std::fs::File::create(lock_file(data, owner)).ok()?;
    f.try_lock().ok()?;
    Some(f)
}

/// True while the window that wrote copies as `owner` is still running.
pub fn owner_alive(data: &Path, owner: &str) -> bool {
    if owner.is_empty() {
        return false;
    }
    let path = lock_file(data, owner);
    let Ok(f) = std::fs::OpenOptions::new().write(true).open(&path) else {
        return false;
    };
    match f.try_lock() {
        Ok(()) => {
            // Nobody holds it: that window is gone.
            let _ = f.unlock();
            drop(f);
            let _ = std::fs::remove_file(&path);
            false
        }
        Err(_) => true,
    }
}

/// Removes the lock files of windows that are gone (a crash with nothing unsaved
/// leaves its lock behind, and no copy ever names it). `own` is kept.
pub fn sweep_stale_locks(data: &Path, own: &str) {
    let Ok(rd) = std::fs::read_dir(recovery_dir(data)) else {
        return;
    };
    for e in rd.filter_map(Result::ok) {
        let path = e.path();
        if path.extension().is_none_or(|x| x != "lock") {
            continue;
        }
        let Some(owner) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
            continue;
        };
        if owner != own {
            // Deletes the file when nobody holds its lock.
            owner_alive(data, &owner);
        }
    }
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

fn file_of(data: &Path, key: &str, owner: &str) -> PathBuf {
    if owner.is_empty() {
        recovery_dir(data).join(format!("{:016x}.json", fnv(key)))
    } else {
        recovery_dir(data).join(format!("{:016x}-{owner}.json", fnv(key)))
    }
}

/// Writes (or replaces) the recovery copy for `key` made by `r.owner`. The text goes to
/// a temporary file first and is then renamed into place, so a crash part way through
/// never leaves a cut-off copy.
pub fn write(data: &Path, key: &str, r: &Recovered) -> std::io::Result<()> {
    std::fs::create_dir_all(recovery_dir(data))?;
    let target = file_of(data, key, &r.owner);
    let temp = target.with_extension("json.tmp");
    std::fs::write(&temp, serde_json::to_string(r).unwrap_or_default())?;
    std::fs::rename(&temp, &target)
}

/// Removes the recovery copy for `key` made by `owner`, if there is one.
pub fn remove(data: &Path, key: &str, owner: &str) {
    let _ = std::fs::remove_file(file_of(data, key, owner));
}

/// Removes every copy `owner` made, and its lock file (a clean exit).
pub fn clear_own(data: &Path, owner: &str) {
    let Ok(rd) = std::fs::read_dir(recovery_dir(data)) else {
        return;
    };
    let suffix = format!("-{owner}.json");
    for e in rd.filter_map(Result::ok) {
        if e.file_name().to_string_lossy().ends_with(&suffix) {
            let _ = std::fs::remove_file(e.path());
        }
    }
    let _ = std::fs::remove_file(lock_file(data, owner));
}

/// Every recovery copy on disk.
pub fn load_all(data: &Path) -> Vec<Recovered> {
    let Ok(rd) = std::fs::read_dir(recovery_dir(data)) else {
        return vec![];
    };
    let mut out: Vec<Recovered> = rd
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|s| serde_json::from_str(&s).ok())
        .collect();
    out.sort_by(|a, b| a.title.cmp(&b.title));
    out
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
                remove(data, &r.key, &r.owner);
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
            owner: "w1".into(),
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
        remove(&dir, "/p/a.mpp", "w1");
        assert!(load_all(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lock_files_of_windows_that_are_gone_are_removed_at_start_up() {
        let dir = std::env::temp_dir().join(format!("mpp-locks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let live = hold_lock(&dir, "live").expect("lock");
        let mine = hold_lock(&dir, "mine").expect("lock");
        // A window that crashed: its lock file is there but nobody holds it.
        std::fs::write(lock_file(&dir, "crashed"), "").expect("write");
        sweep_stale_locks(&dir, "mine");
        assert!(!lock_file(&dir, "crashed").exists());
        assert!(lock_file(&dir, "live").exists());
        assert!(lock_file(&dir, "mine").exists());
        drop((live, mine));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
