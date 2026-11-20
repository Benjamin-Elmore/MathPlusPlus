//! Links between `.mpp` files: `![[file]]`, `from [[file]]` and `[[file|alias]]`.
//! Finding them, resolving them against a folder, and rewriting them when a
//! file or folder moves.

use std::ops::Range;
use std::path::{Component, Path, PathBuf};

/// One written file reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// Byte range of the path as written (without brackets or alias).
    pub range: Range<usize>,
    /// The path as written.
    pub path: String,
}

/// Every file reference in `text`, in order. Fenced code is skipped.
pub fn scan_links(text: &str) -> Vec<Link> {
    let mut out = vec![];
    let mut in_fence = false;
    let mut line_start = 0;
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        } else if !in_fence {
            scan_line(line, line_start, &mut out);
        }
        line_start += line.len();
    }
    out
}

fn scan_line(line: &str, base: usize, out: &mut Vec<Link>) {
    let mut i = 0;
    while let Some(p) = line[i..].find("[[") {
        let start = i + p + 2;
        let rest = &line[start..];
        let Some(end) = rest.find("]]") else {
            break;
        };
        let inner = &rest[..end];
        let path_len = inner.find('|').unwrap_or(inner.len());
        let raw = &inner[..path_len];
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            let lead = raw.len() - raw.trim_start().len();
            let s = base + start + lead;
            out.push(Link {
                range: s..s + trimmed.len(),
                path: trimmed.to_string(),
            });
        }
        i = start + end + 2;
    }
}

/// Removes `.` and resolves `..` without touching the disk.
pub fn normalize(p: &Path) -> PathBuf {
    let mut out: Vec<Component> = vec![];
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.last(), Some(Component::Normal(_))) {
                    out.pop();
                } else {
                    out.push(c);
                }
            }
            _ => out.push(c),
        }
    }
    out.iter().collect()
}

/// The file a written path points at, from a file in `dir`.
pub fn resolve(dir: &Path, written: &str) -> PathBuf {
    let mut p = dir.join(written);
    if p.extension().is_none() {
        p.set_extension("mpp");
    }
    normalize(&p)
}

/// The path to write for `target`, from a file in `dir` (forward slashes).
/// `keep_ext` keeps the `.mpp` extension.
pub fn relative(dir: &Path, target: &Path, keep_ext: bool) -> String {
    let dir = normalize(dir);
    let target = normalize(target);
    let a: Vec<_> = dir.components().collect();
    let b: Vec<_> = target.components().collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut parts: Vec<String> = vec![];
    if common == 0 {
        parts.push(target.to_string_lossy().replace('\\', "/"));
    } else {
        for _ in common..a.len() {
            parts.push("..".into());
        }
        for c in &b[common..] {
            parts.push(c.as_os_str().to_string_lossy().into_owned());
        }
    }
    let mut s = parts.join("/");
    if !keep_ext {
        if let Some(t) = s.strip_suffix(".mpp") {
            s = t.to_string();
        }
    }
    s
}

/// Maps `p` through a move of `from` to `to` (a file or a whole folder).
pub fn map_move(p: &Path, from: &Path, to: &Path) -> PathBuf {
    match p.strip_prefix(from) {
        Ok(rest) if rest.as_os_str().is_empty() => to.to_path_buf(),
        Ok(rest) => to.join(rest),
        Err(_) => p.to_path_buf(),
    }
}

/// The text of a file after the links in it are updated for a move. `old_dir`
/// and `new_dir` are the folder of the file before and after it moves; `map`
/// sends each link target to where it ends up. `None` when nothing changes.
pub fn rewrite_links(
    text: &str,
    old_dir: &Path,
    new_dir: &Path,
    map: &dyn Fn(&Path) -> PathBuf,
) -> Option<String> {
    let mut out = text.to_string();
    let mut changed = false;
    for l in scan_links(text).into_iter().rev() {
        if l.path.contains("://") {
            continue;
        }
        let target = resolve(old_dir, &l.path);
        let new_target = map(&target);
        let keep_ext = l.path.ends_with(".mpp");
        let new = relative(new_dir, &new_target, keep_ext);
        if new != l.path && out.is_char_boundary(l.range.start) && out.is_char_boundary(l.range.end)
        {
            out.replace_range(l.range.clone(), &new);
            changed = true;
        }
    }
    changed.then_some(out)
}

/// The files `text` imports (resolved against `dir`), missing or not.
pub fn targets(text: &str, dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = vec![];
    for l in scan_links(text) {
        if l.path.contains("://") {
            continue;
        }
        let p = resolve(dir, &l.path);
        if !v.contains(&p) {
            v.push(p);
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_all_three_forms() {
        let t =
            "![[a/b]]\n\nUsing $x$ from [[c.mpp]].\n\nSee [[d|the d file]].\n```\n[[no]]\n```\n";
        let ps: Vec<String> = scan_links(t).into_iter().map(|l| l.path).collect();
        assert_eq!(ps, vec!["a/b", "c.mpp", "d"]);
        let l = &scan_links(t)[0];
        assert_eq!(&t[l.range.clone()], "a/b");
    }

    #[test]
    fn relative_paths_climb_and_descend() {
        let dir = Path::new("/p/a");
        assert_eq!(relative(dir, Path::new("/p/a/x.mpp"), false), "x");
        assert_eq!(relative(dir, Path::new("/p/b/y.mpp"), true), "../b/y.mpp");
        assert_eq!(relative(dir, Path::new("/p/a/c/z.mpp"), false), "c/z");
    }

    #[test]
    fn moving_the_target_rewrites_the_link() {
        let dir = Path::new("/p");
        let from = PathBuf::from("/p/m.mpp");
        let to = PathBuf::from("/p/phys/m.mpp");
        let map = |p: &Path| map_move(p, &from, &to);
        let out = rewrite_links("![[m]]\n", dir, dir, &map).expect("changed");
        assert_eq!(out, "![[phys/m]]\n");
    }

    #[test]
    fn moving_the_importer_keeps_the_target() {
        let from = PathBuf::from("/p/a.mpp");
        let to = PathBuf::from("/p/sub/a.mpp");
        let map = |p: &Path| map_move(p, &from, &to);
        let out = rewrite_links("![[b.mpp]]\n", Path::new("/p"), Path::new("/p/sub"), &map)
            .expect("changed");
        assert_eq!(out, "![[../b.mpp]]\n");
    }
}
