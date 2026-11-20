//! "Did you mean …?" for unknown names and file names.
//!
//! The quick fixes and hover cards ask a [`Suggest`] for the closest known
//! names. [`BasicSuggest`] is a small built-in matcher (edit distance plus the
//! Greek variant letters); the full matcher with LaTeX aliases, units and
//! helper phrases replaces it through [`set_suggester`] without touching the
//! callers.

use std::sync::{Arc, OnceLock, RwLock};

/// Finds the known names closest to an unknown one.
pub trait Suggest: Send + Sync {
    /// Known names that the user probably meant by `unknown`, best first.
    fn names(&self, unknown: &str, known: &[String]) -> Vec<String>;

    /// Known file names that the user probably meant by `unknown`, best first.
    fn files(&self, unknown: &str, known: &[String]) -> Vec<String> {
        self.names(unknown, known)
    }
}

/// The built-in matcher: Greek variant letters first, then names within a
/// small edit distance (ignoring case), at most three.
#[derive(Debug, Clone, Copy, Default)]
pub struct BasicSuggest;

/// Pairs of Greek letters that look alike or are variants of each other.
const GREEK_VARIANTS: [(char, char); 8] = [
    ('θ', 'ϑ'),
    ('ε', 'ϵ'),
    ('φ', 'ϕ'),
    ('ρ', 'ϱ'),
    ('π', 'ϖ'),
    ('σ', 'ς'),
    ('κ', 'ϰ'),
    ('Θ', 'ϴ'),
];

fn variant(c: char) -> Option<char> {
    GREEK_VARIANTS.iter().find_map(|&(a, b)| {
        if c == a {
            Some(b)
        } else if c == b {
            Some(a)
        } else {
            None
        }
    })
}

/// The edit distance between two strings, counted in characters.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

impl Suggest for BasicSuggest {
    fn names(&self, unknown: &str, known: &[String]) -> Vec<String> {
        let swapped: String = unknown.chars().map(|c| variant(c).unwrap_or(c)).collect();
        let lower = unknown.to_lowercase();
        let len = unknown.chars().count();
        let limit = match len {
            0..=1 => 0,
            2..=4 => 1,
            _ => 2,
        };
        let mut scored: Vec<(usize, &String)> = known
            .iter()
            .filter(|k| k.as_str() != unknown)
            .filter_map(|k| {
                if swapped != unknown && *k == swapped {
                    return Some((0, k));
                }
                let d = edit_distance(&lower, &k.to_lowercase());
                (d <= limit).then_some((d + 1, k))
            })
            .collect();
        scored.sort();
        scored.dedup_by(|a, b| a.1 == b.1);
        scored.into_iter().take(3).map(|(_, k)| k.clone()).collect()
    }

    fn files(&self, unknown: &str, known: &[String]) -> Vec<String> {
        let stem = |s: &str| s.trim_end_matches(".mpp").to_lowercase();
        let u = stem(unknown);
        let mut scored: Vec<(usize, &String)> = known
            .iter()
            .filter_map(|k| {
                let d = edit_distance(&u, &stem(k));
                (d <= 2.max(u.chars().count() / 4)).then_some((d, k))
            })
            .collect();
        scored.sort();
        scored.into_iter().take(3).map(|(_, k)| k.clone()).collect()
    }
}

fn slot() -> &'static RwLock<Arc<dyn Suggest>> {
    static SLOT: OnceLock<RwLock<Arc<dyn Suggest>>> = OnceLock::new();
    SLOT.get_or_init(|| RwLock::new(Arc::new(BasicSuggest)))
}

/// Replaces the matcher used by every hover card and quick fix.
pub fn set_suggester(s: Arc<dyn Suggest>) {
    if let Ok(mut g) = slot().write() {
        *g = s;
    }
}

/// The matcher in use.
pub fn suggester() -> Arc<dyn Suggest> {
    match slot().read() {
        Ok(g) => g.clone(),
        Err(_) => Arc::new(BasicSuggest),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn greek_variants_come_first() {
        let k = known(&["ϑ", "x", "θ_0"]);
        assert_eq!(BasicSuggest.names("θ", &k), ["ϑ"]);
    }

    #[test]
    fn close_spellings_are_found() {
        let k = known(&["velocity", "mass", "v_0"]);
        assert_eq!(BasicSuggest.names("velocty", &k), ["velocity"]);
        assert!(BasicSuggest.names("q", &k).is_empty());
        assert_eq!(BasicSuggest.names("v_1", &k), ["v_0"]);
    }

    #[test]
    fn file_names_ignore_the_extension() {
        let k = known(&["mechanics.mpp", "launch.mpp"]);
        assert_eq!(BasicSuggest.files("mechanic.mpp", &k), ["mechanics.mpp"]);
    }

    #[test]
    fn distance() {
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("", "ab"), 2);
    }
}
