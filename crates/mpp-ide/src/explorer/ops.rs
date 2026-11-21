//! File operations: create, rename, move, delete and reveal. Moves and
//! renames also work out what every other file's imports must become.

use std::path::{Path, PathBuf};

use super::links::{map_move, rewrite_links};
use super::tree::all_mpp;

/// A file whose text changes because of a move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    /// Where the file is before the move.
    pub old: PathBuf,
    /// Where it is afterwards.
    pub new: PathBuf,
    /// Its new text.
    pub text: String,
}

/// Checks a move or rename before it happens.
pub fn check_move(from: &Path, to: &Path) -> Result<(), String> {
    if from == to {
        return Err("That is already its name.".into());
    }
    if to.starts_with(from) {
        return Err("A folder cannot move into itself.".into());
    }
    if to.exists() {
        return Err("Something with that name already exists there.".into());
    }
    if !from.exists() {
        return Err("That file is gone.".into());
    }
    if to
        .file_name()
        .map(|n| n.to_string_lossy().trim().is_empty())
        .unwrap_or(true)
    {
        return Err("A name cannot be empty.".into());
    }
    Ok(())
}

/// The import rewrites that moving `from` to `to` causes, across every `.mpp`
/// file under `root`. `read` gives the current text of a file (an open, edited
/// one, or the one on disk).
pub fn plan_move(
    root: &Path,
    from: &Path,
    to: &Path,
    read: &dyn Fn(&Path) -> Option<String>,
) -> Vec<Planned> {
    let mut files = all_mpp(root);
    for extra in all_mpp(from) {
        if !files.contains(&extra) {
            files.push(extra);
        }
    }
    if from.is_file() && !files.iter().any(|f| f == from) {
        files.push(from.to_path_buf());
    }
    let map = |p: &Path| map_move(p, from, to);
    let mut out = vec![];
    for f in files {
        let Some(text) = read(&f) else {
            continue;
        };
        let new_file = map_move(&f, from, to);
        let (Some(old_dir), Some(new_dir)) = (f.parent(), new_file.parent()) else {
            continue;
        };
        if let Some(new_text) = rewrite_links(&text, old_dir, new_dir, &map) {
            out.push(Planned {
                old: f,
                new: new_file,
                text: new_text,
            });
        }
    }
    out
}

/// Moves a file or folder on disk (creating the target's folder).
pub fn move_on_disk(from: &Path, to: &Path) -> Result<(), String> {
    check_move(from, to)?;
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Could not move it: {e}"))?;
    }
    std::fs::rename(from, to).map_err(|e| format!("Could not move it: {e}"))
}

/// A name that does not exist yet in `dir`: `stem.ext`, then `stem 2.ext`...
pub fn unique_name(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let make = |n: u32| {
        let base = if n <= 1 {
            stem.to_string()
        } else {
            format!("{stem} {n}")
        };
        if ext.is_empty() {
            dir.join(base)
        } else {
            dir.join(format!("{base}.{ext}"))
        }
    };
    (1..)
        .map(make)
        .find(|p| !p.exists())
        .unwrap_or_else(|| dir.join(stem))
}

/// Checks a name typed for a new file or folder.
pub fn valid_name(name: &str) -> Result<(), String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("A name cannot be empty.".into());
    }
    if n.chars().any(|c| {
        matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()
    }) {
        return Err("A name cannot hold / \\ : * ? \" < > |".into());
    }
    if n.starts_with('.') || n.ends_with('.') {
        return Err("A name cannot start or end with a dot.".into());
    }
    Ok(())
}

/// Creates an empty `.mpp` file called `name` in `dir`.
pub fn create_file(dir: &Path, name: &str) -> Result<PathBuf, String> {
    valid_name(name)?;
    let mut n = name.trim().to_string();
    if !n.to_lowercase().ends_with(".mpp") {
        n.push_str(".mpp");
    }
    let p = dir.join(n);
    if p.exists() {
        return Err("A file with that name already exists.".into());
    }
    std::fs::write(&p, "").map_err(|e| format!("Could not create it: {e}"))?;
    Ok(p)
}

/// Creates a folder called `name` in `dir`.
pub fn create_dir(dir: &Path, name: &str) -> Result<PathBuf, String> {
    valid_name(name)?;
    let p = dir.join(name.trim());
    if p.exists() {
        return Err("A folder with that name already exists.".into());
    }
    std::fs::create_dir_all(&p).map_err(|e| format!("Could not create it: {e}"))?;
    Ok(p)
}

/// Sends a file or folder to the recycle bin.
pub fn delete(path: &Path) -> Result<(), String> {
    trash::delete(path).map_err(|e| format!("Could not move it to the recycle bin: {e}"))
}

/// Shows a file in the system's file manager.
pub fn reveal(path: &Path) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn();
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        let dir = path.parent().unwrap_or(path);
        let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mpp-ops-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("mkdir");
        d
    }

    #[test]
    fn rename_plans_every_import_rewrite() {
        let d = temp("plan");
        std::fs::write(d.join("a.mpp"), "![[m]]\n\nUsing $x$ from [[m.mpp]].\n").expect("w");
        std::fs::write(d.join("m.mpp"), "Let $x = 1$.\n").expect("w");
        std::fs::create_dir_all(d.join("sub")).expect("mk");
        std::fs::write(d.join("sub/b.mpp"), "![[../m]]\n").expect("w");
        let read = |p: &Path| std::fs::read_to_string(p).ok();
        let plan = plan_move(&d, &d.join("m.mpp"), &d.join("sub/mech.mpp"), &read);
        let a = plan.iter().find(|p| p.old == d.join("a.mpp")).expect("a");
        assert_eq!(
            a.text,
            "![[sub/mech]]\n\nUsing $x$ from [[sub/mech.mpp]].\n"
        );
        let b = plan
            .iter()
            .find(|p| p.old == d.join("sub/b.mpp"))
            .expect("b");
        assert_eq!(b.text, "![[mech]]\n");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn folder_moves_rewrite_inside_and_outside() {
        let d = temp("folder");
        std::fs::create_dir_all(d.join("lib")).expect("mk");
        std::fs::write(d.join("lib/one.mpp"), "![[two]]\n").expect("w");
        std::fs::write(d.join("lib/two.mpp"), "x").expect("w");
        std::fs::write(d.join("main.mpp"), "![[lib/one]]\n").expect("w");
        let read = |p: &Path| std::fs::read_to_string(p).ok();
        let plan = plan_move(&d, &d.join("lib"), &d.join("pkg/lib"), &read);
        assert_eq!(plan.len(), 1, "only main changes: {plan:?}");
        assert_eq!(plan[0].text, "![[pkg/lib/one]]\n");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn checks_catch_bad_moves() {
        let d = temp("check");
        std::fs::write(d.join("a.mpp"), "").expect("w");
        std::fs::write(d.join("b.mpp"), "").expect("w");
        assert!(check_move(&d.join("a.mpp"), &d.join("b.mpp")).is_err());
        assert!(check_move(&d, &d.join("x/y")).is_err());
        assert!(check_move(&d.join("a.mpp"), &d.join("c.mpp")).is_ok());
        assert!(valid_name("a/b").is_err());
        assert!(valid_name("good name").is_ok());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn creating_adds_the_extension_and_refuses_duplicates() {
        let d = temp("create");
        let p = create_file(&d, "notes").expect("create");
        assert!(p.ends_with("notes.mpp") && p.is_file());
        assert!(create_file(&d, "notes").is_err());
        assert_eq!(unique_name(&d, "notes", "mpp"), d.join("notes 2.mpp"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
