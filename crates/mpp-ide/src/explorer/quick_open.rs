//! Ctrl+P: pick a file by typing part of its name.

use std::path::{Path, PathBuf};

/// A fuzzy score for `query` against `name` (higher is better); `None` when
/// the letters of the query do not appear in order.
pub fn score(query: &str, name: &str) -> Option<i32> {
    let q: Vec<char> = query
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if q.is_empty() {
        return Some(0);
    }
    let n: Vec<char> = name.to_lowercase().chars().collect();
    let mut qi = 0;
    let mut total = 0;
    let mut last: Option<usize> = None;
    for (i, c) in n.iter().enumerate() {
        if qi < q.len() && *c == q[qi] {
            total += 1;
            if last == Some(i.wrapping_sub(1)) {
                total += 4;
            }
            if i == 0 || !n[i - 1].is_alphanumeric() {
                total += 3;
            }
            last = Some(i);
            qi += 1;
        }
    }
    if qi < q.len() {
        return None;
    }
    let lower = name.to_lowercase();
    let qs: String = q.iter().collect();
    if lower.starts_with(&qs) {
        total += 10;
    } else if lower.contains(&qs) {
        total += 6;
    }
    Some(total * 10 - n.len() as i32)
}

/// The best matches for `query` among `files`, shown relative to `root`.
pub fn rank(root: &Path, files: &[PathBuf], query: &str, limit: usize) -> Vec<PathBuf> {
    let mut scored: Vec<(i32, &PathBuf)> = files
        .iter()
        .filter_map(|f| {
            let stem = f.file_stem()?.to_string_lossy().into_owned();
            let rel = f
                .strip_prefix(root)
                .unwrap_or(f)
                .to_string_lossy()
                .into_owned();
            let s = score(query, &stem)
                .map(|s| s + 5)
                .or_else(|| score(query, &rel))?;
            Some((s, f))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    scored
        .into_iter()
        .take(limit)
        .map(|(_, f)| f.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_beats_scattered_matches() {
        let root = Path::new("/p");
        let files: Vec<PathBuf> = ["/p/mechanics.mpp", "/p/calculus/more.mpp", "/p/waves.mpp"]
            .iter()
            .map(PathBuf::from)
            .collect();
        let r = rank(root, &files, "mech", 5);
        assert_eq!(r[0], PathBuf::from("/p/mechanics.mpp"));
        assert!(rank(root, &files, "zzz", 5).is_empty());
        assert_eq!(rank(root, &files, "", 5).len(), 3);
        assert!(score("mc", "mechanics").is_some());
        assert!(score("cm", "mechanics").is_none());
    }
}
