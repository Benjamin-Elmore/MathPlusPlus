//! "Did you mean...?": the closest names, symbols, units, helper phrases and
//! import file names to something that was not found.
//!
//! Matching is edit distance (with swapped neighbours counted as one slip) plus
//! aliases: `alpha`, `\alpha` and `α` are the same thing, so any of them finds
//! the others. The autocomplete and the quick fixes of the problems tile use
//! [`Suggester`].

use mpp_syntax::symbols::{lookup_char, lookup_latex, lookup_name, SYMBOLS};

/// How many suggestions are returned at most.
pub const MAX: usize = 3;

/// The number of character slips between two strings (a swap of neighbours is
/// one slip).
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = v;
        }
    }
    d[n][m]
}

/// How many slips are allowed for a word of `len` characters.
fn allowed(len: usize) -> usize {
    match len {
        0..=1 => 0,
        2..=3 => 1,
        4..=7 => 2,
        _ => 3,
    }
}

/// The symbol a spelling stands for: `alpha`, `\alpha` and `α` all give `α`.
pub fn canonical_symbol(s: &str) -> Option<&'static str> {
    let mut chars = s.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if let Some(sym) = lookup_char(c) {
            return Some(sym.unicode);
        }
    }
    if s.starts_with('\\') {
        return lookup_latex(s).map(|sym| sym.unicode);
    }
    lookup_name(s).map(|sym| sym.unicode)
}

/// A rank for a candidate (lower is better), or `None` when it is too far off.
fn score(unknown: &str, candidate: &str) -> Option<usize> {
    if unknown == candidate {
        return None;
    }
    let (u, c) = (unknown.to_lowercase(), candidate.to_lowercase());
    if u == c {
        return Some(0);
    }
    if let (Some(a), Some(b)) = (canonical_symbol(unknown), canonical_symbol(candidate)) {
        if a == b {
            return Some(0);
        }
    }
    let d = edit_distance(&u, &c);
    let len = u.chars().count().max(c.chars().count());
    (d <= allowed(len)).then_some(d + 1)
}

/// The best of `candidates` for `unknown`, closest first, at most [`MAX`].
pub fn rank(unknown: &str, candidates: &[String]) -> Vec<String> {
    let mut scored: Vec<(usize, usize, usize, &String)> = candidates
        .iter()
        .filter_map(|c| {
            score(unknown, c).map(|s| (s, edit_distance(unknown, c), c.chars().count(), c))
        })
        .collect();
    scored.sort();
    let mut out: Vec<String> = vec![];
    for (_, _, _, c) in scored {
        if !out.contains(c) {
            out.push(c.clone());
        }
        if out.len() == MAX {
            break;
        }
    }
    out
}

/// Units the IDE knows by default (the units lane adds the real table later).
pub const UNITS: [&str; 34] = [
    "m", "cm", "mm", "km", "s", "ms", "min", "h", "kg", "g", "mg", "N", "J", "W", "Pa", "kPa",
    "Hz", "V", "A", "K", "mol", "cd", "rad", "deg", "L", "mL", "ft", "in", "lb", "mph", "bar",
    "eV", "C", "F",
];

/// The sentence openings of the helper forms.
pub const PHRASES: [&str; 22] = [
    "Find the domain of",
    "Find the range of",
    "Find the intercepts of",
    "Find the critical points of",
    "Find the extrema of",
    "Find the asymptotes of",
    "Find the tangent line of",
    "Find the normal line of",
    "Compute the concavity of",
    "Compute the inflection points of",
    "Compute the linearisation of",
    "Determine the intervals of increase of",
    "Determine the local extrema of",
    "Determine the absolute extrema of",
    "Analyse",
    "Analyze",
    "Solve",
    "Using",
    "Let",
    "Figure:",
    "Find the intervals of increase of",
    "Compute the linearization of",
];

/// The matcher. It holds no state; the type exists so that the quick fixes can
/// take it as a plug point.
#[derive(Debug, Clone, Copy, Default)]
pub struct Suggester;

impl Suggester {
    /// Names (variables, functions) closest to an unknown one.
    pub fn names(&self, unknown: &str, known: &[String]) -> Vec<String> {
        let mut pool: Vec<String> = known.to_vec();
        // An unknown name may be a misspelt Greek letter: offer the letter.
        let symbols: Vec<String> = self.symbols(unknown);
        for s in symbols {
            if !pool.contains(&s) {
                pool.push(s);
            }
        }
        rank(unknown, &pool)
    }

    /// Symbols (as the letters themselves) closest to something typed:
    /// `lamda` and `\lamda` both give `λ`.
    pub fn symbols(&self, unknown: &str) -> Vec<String> {
        let bare = unknown.strip_prefix('\\').unwrap_or(unknown);
        let names: Vec<String> = SYMBOLS.iter().map(|s| s.name.to_string()).collect();
        rank(bare, &names)
            .into_iter()
            .filter_map(|n| lookup_name(&n).map(|s| s.unicode.to_string()))
            .collect()
    }

    /// Units closest to an unknown one (`known` adds the project's own).
    pub fn units(&self, unknown: &str, known: &[String]) -> Vec<String> {
        let mut pool: Vec<String> = UNITS.iter().map(|u| (*u).to_string()).collect();
        pool.extend(known.iter().cloned());
        // Units are case-sensitive (`m` is a metre, `M` is not): rank exact case first.
        let mut r = rank(unknown, &pool);
        r.sort_by_key(|c| usize::from(c.to_lowercase() != unknown.to_lowercase()));
        r
    }

    /// The helper sentence openings closest to what was typed.
    pub fn phrases(&self, unknown: &str) -> Vec<String> {
        let pool: Vec<String> = PHRASES.iter().map(|p| (*p).to_string()).collect();
        let lead = unknown.trim().trim_end_matches(['.', ':']);
        let mut out = rank(lead, &pool);
        // A sentence start also matches the beginning of a longer phrase.
        let lower = lead.to_lowercase();
        for p in PHRASES {
            if lower.len() >= 3
                && p.to_lowercase().starts_with(&lower)
                && !out.iter().any(|o| o == p)
            {
                out.push((*p).to_string());
            }
        }
        out.truncate(MAX);
        out
    }

    /// Import file names closest to a missing one (the extension is ignored).
    pub fn files(&self, unknown: &str, known: &[String]) -> Vec<String> {
        let stem = |s: &str| {
            let t = s.rsplit(['/', '\\']).next().unwrap_or(s);
            t.strip_suffix(".mpp").unwrap_or(t).to_string()
        };
        let want = stem(unknown);
        let pairs: Vec<(String, &String)> = known.iter().map(|k| (stem(k), k)).collect();
        let stems: Vec<String> = pairs.iter().map(|(s, _)| s.clone()).collect();
        rank(&want, &stems)
            .into_iter()
            .filter_map(|s| {
                pairs
                    .iter()
                    .find(|(st, _)| *st == s)
                    .map(|(_, k)| (*k).clone())
            })
            .collect()
    }
}

/// The live-problem cards and quick fixes ask this matcher too. Only names
/// that are actually defined come back (a quick fix must not offer a name that
/// has no value), with the look-alike Greek variants (θ and ϑ) first.
impl crate::diagnostics::suggest::Suggest for Suggester {
    fn names(&self, unknown: &str, known: &[String]) -> Vec<String> {
        let basic = crate::diagnostics::suggest::BasicSuggest;
        let mut out = crate::diagnostics::suggest::Suggest::names(&basic, unknown, known);
        for n in Suggester::names(self, unknown, known) {
            if known.contains(&n) && !out.contains(&n) {
                out.push(n);
            }
        }
        out.truncate(MAX);
        out
    }

    fn files(&self, unknown: &str, known: &[String]) -> Vec<String> {
        Suggester::files(self, unknown, known)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugs_into_the_live_problems() {
        use crate::diagnostics::suggest::Suggest;
        let s: &dyn Suggest = &Suggester;
        let k = v(&["ϑ", "velocity", "mass"]);
        assert_eq!(s.names("θ", &k), ["ϑ"]);
        assert_eq!(s.names("velcoity", &k), ["velocity"]);
        // A Greek letter that is not defined is never offered.
        assert!(s.names("alpah", &k).is_empty());
        assert_eq!(
            s.files("mechanic.mpp", &v(&["mechanics.mpp", "launch.mpp"])),
            ["mechanics.mpp"]
        );
    }

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn distance_counts_a_swap_as_one() {
        assert_eq!(edit_distance("teh", "the"), 1);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("", "ab"), 2);
    }

    #[test]
    fn names_rank_the_closest_first() {
        let known = v(&["velocity", "velocities", "mass", "vel"]);
        let r = Suggester.names("velocty", &known);
        assert_eq!(r[0], "velocity");
        assert!(!r.contains(&"mass".to_string()));
        assert!(Suggester.names("zzzz", &known).is_empty());
    }

    #[test]
    fn greek_and_latex_spellings_are_the_same_thing() {
        let known = v(&["\u{03b1}", "beta"]);
        assert_eq!(Suggester.names("alpha", &known)[0], "\u{03b1}");
        assert_eq!(Suggester.names("\\alpha", &known)[0], "\u{03b1}");
        assert_eq!(canonical_symbol("alpha"), canonical_symbol("\u{03b1}"));
        let s = Suggester.symbols("lamda");
        assert_eq!(s[0], "\u{03bb}", "{s:?}");
        assert_eq!(Suggester.symbols("\\lamda")[0], "\u{03bb}");
    }

    #[test]
    fn units_prefer_the_same_case() {
        let r = Suggester.units("kgs", &[]);
        assert_eq!(r[0], "kg", "{r:?}");
        assert!(Suggester.units("Kg", &[]).contains(&"kg".to_string()));
        assert!(Suggester.units("qqqq", &[]).is_empty());
    }

    #[test]
    fn helper_phrases_complete_and_correct() {
        assert!(Suggester.phrases("Solv").contains(&"Solve".to_string()));
        let r = Suggester.phrases("Find the domian of");
        assert_eq!(r[0], "Find the domain of", "{r:?}");
        assert!(Suggester
            .phrases("Analy")
            .iter()
            .any(|p| p.starts_with("Analy")));
    }

    #[test]
    fn missing_import_files_find_the_real_name() {
        let known = v(&["physics/mechanics.mpp", "waves.mpp", "calculus/area.mpp"]);
        let r = Suggester.files("mechanis", &known);
        assert_eq!(r[0], "physics/mechanics.mpp");
        assert_eq!(Suggester.files("wavs.mpp", &known)[0], "waves.mpp");
        assert!(Suggester.files("unrelated-name", &known).is_empty());
    }
}
