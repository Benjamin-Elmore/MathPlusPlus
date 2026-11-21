//! Live symbol replacement (`alpha` becomes α) and autocomplete candidates.

use std::ops::Range;

use mpp_syntax::symbols::{lookup_char, lookup_name, SymbolKind, SYMBOLS};

use crate::suggest::Suggester;

/// A text replacement the editor should apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacement {
    pub range: Range<usize>,
    pub with: &'static str,
}

/// The ASCII letters just before `end`, as a byte range.
fn word_before(text: &str, end: usize) -> Range<usize> {
    let mut start = end;
    for (i, c) in text[..end].char_indices().rev() {
        if c.is_ascii_alphabetic() {
            start = i;
        } else {
            break;
        }
    }
    start..end
}

fn replaceable(kind: SymbolKind) -> bool {
    !matches!(
        kind,
        SymbolKind::Function | SymbolKind::Font | SymbolKind::Accent | SymbolKind::Delimiter
    )
}

/// True when some other symbol name continues `word` (so `alpha` might still
/// become something longer and must wait for a delimiter).
fn has_longer_name(word: &str) -> bool {
    SYMBOLS
        .iter()
        .any(|s| s.name.len() > word.len() && s.name.starts_with(word))
}

/// The replacement to apply after `typed` was inserted and the caret moved to
/// `caret`. A Greek letter is replaced as soon as its name is complete and
/// unambiguous; other symbols wait for the delimiter that ends the word.
pub fn live_replacement(text: &str, caret: usize, typed: &str) -> Option<Replacement> {
    if caret > text.len() || !text.is_char_boundary(caret) {
        return None;
    }
    let last = typed.chars().next_back()?;
    if last.is_ascii_alphabetic() {
        let w = word_before(text, caret);
        let word = &text[w.clone()];
        if word.len() < 2 || text[..w.start].ends_with('\\') {
            return None;
        }
        let sym = lookup_name(word)?;
        if sym.kind == SymbolKind::Letter && !has_longer_name(word) {
            return Some(Replacement {
                range: w,
                with: sym.unicode,
            });
        }
        None
    } else {
        if last.is_ascii_alphanumeric() || last == '\\' {
            return None;
        }
        let before = caret.checked_sub(last.len_utf8())?;
        let w = word_before(text, before);
        let word = &text[w.clone()];
        if word.len() < 2 || text[..w.start].ends_with('\\') {
            return None;
        }
        let sym = lookup_name(word)?;
        if replaceable(sym.kind) && sym.unicode != word {
            return Some(Replacement {
                range: w,
                with: sym.unicode,
            });
        }
        None
    }
}

/// One autocomplete entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// What is shown typeset (LaTeX).
    pub latex: String,
    /// The name as typed.
    pub label: String,
    /// What replaces the typed word.
    pub insert: String,
}

/// The word being typed before `caret`, with a `\` if it has one.
pub fn word_at(text: &str, caret: usize) -> Option<Range<usize>> {
    if caret > text.len() || !text.is_char_boundary(caret) {
        return None;
    }
    let w = word_before(text, caret);
    if w.is_empty() {
        return None;
    }
    let start = if text[..w.start].ends_with('\\') {
        w.start - 1
    } else {
        w.start
    };
    Some(start..w.end)
}

/// Candidates for a typed prefix: symbol names, then names in scope.
pub fn completions<'a>(
    prefix: &str,
    scope: impl IntoIterator<Item = &'a str>,
    limit: usize,
) -> Vec<Completion> {
    let p = prefix.strip_prefix('\\').unwrap_or(prefix);
    if p.len() < 2 {
        return vec![];
    }
    let scope: Vec<String> = scope.into_iter().map(str::to_string).collect();
    let mut out = vec![];
    for name in &scope {
        if name.starts_with(p) && name != p {
            out.push(Completion {
                latex: name.to_string(),
                label: name.to_string(),
                insert: name.to_string(),
            });
        }
    }
    for s in SYMBOLS {
        if out.len() >= limit * 2 {
            break;
        }
        if s.name.starts_with(p) && s.name != p && replaceable(s.kind) {
            out.push(Completion {
                latex: s.latex.to_string(),
                label: s.name.to_string(),
                insert: s.unicode.to_string(),
            });
        }
    }
    if out.is_empty() && p.chars().count() >= 3 {
        misspelt(p, &scope, &mut out);
    }
    out.truncate(limit);
    out
}

/// Nothing starts with what was typed: offer the closest names and symbols
/// instead (`lamda` offers λ, `velcoity` offers `velocity`).
fn misspelt(p: &str, scope: &[String], out: &mut Vec<Completion>) {
    let s = Suggester;
    for name in s.names(p, scope) {
        if scope.contains(&name) {
            out.push(Completion {
                latex: name.clone(),
                label: name.clone(),
                insert: name,
            });
        }
    }
    for letter in s.symbols(p) {
        let mut chars = letter.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            continue;
        };
        let Some(sym) = lookup_char(c).filter(|sym| replaceable(sym.kind)) else {
            continue;
        };
        if !out.iter().any(|o| o.insert == sym.unicode) {
            out.push(Completion {
                latex: sym.latex.to_string(),
                label: sym.name.to_string(),
                insert: sym.unicode.to_string(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(text: &str) -> Option<String> {
        // Type the text one character at a time, applying replacements.
        let mut s = String::new();
        for c in text.chars() {
            s.push(c);
            if let Some(r) = live_replacement(&s, s.len(), &c.to_string()) {
                s.replace_range(r.range, r.with);
            }
        }
        Some(s)
    }

    #[test]
    fn alpha_becomes_a_letter() {
        assert_eq!(typed("alpha").as_deref(), Some("α"));
        assert_eq!(typed("2 alpha + beta").as_deref(), Some("2 α + β"));
    }

    #[test]
    fn longer_names_wait() {
        // `epsilon` and `varepsilon` both exist; `var` alone is not a letter.
        assert_eq!(typed("epsilon").as_deref(), Some("ϵ"));
        assert_eq!(typed("varepsilon").as_deref(), Some("ε"));
    }

    #[test]
    fn latex_commands_and_plain_words_are_left_alone() {
        assert_eq!(typed("\\alpha").as_deref(), Some("\\alpha"));
        assert_eq!(typed("a").as_deref(), Some("a"));
    }

    #[test]
    fn relations_replace_on_a_delimiter() {
        assert_eq!(typed("x in R").as_deref(), Some("x ∈ R"));
    }

    #[test]
    fn misspellings_offer_the_closest_name_or_symbol() {
        let c = completions("lamda", ["x"], 8);
        assert!(c.iter().any(|c| c.insert == "\u{3bb}"), "{c:?}");
        let c = completions("velcoity", ["velocity", "mass"], 8);
        assert_eq!(c.first().map(|c| c.insert.as_str()), Some("velocity"));
    }

    #[test]
    fn completion_finds_symbols_and_names() {
        let c = completions("alp", ["alpha_1", "beta"], 8);
        assert!(c.iter().any(|e| e.label == "alpha_1"));
        assert!(c.iter().any(|e| e.insert == "α"));
        assert!(!c.iter().any(|e| e.label == "beta"));
    }

    #[test]
    fn word_range_includes_the_backslash() {
        assert_eq!(word_at("x + \\alp", 8), Some(4..8));
        assert_eq!(word_at("x + alp", 7), Some(4..7));
        assert_eq!(word_at("x + ", 4), None);
    }
}
