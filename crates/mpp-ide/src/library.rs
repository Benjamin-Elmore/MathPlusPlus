//! The standard library as the IDE sees it: the entries of `std/` for the
//! Standard library tile, and their names for autocomplete and did-you-mean.

use std::sync::OnceLock;

use mpp_eval::stdlib::{self, Entry, EntryKind};
use mpp_syntax::math::{parse_math, DefHead, Statement};

/// One entry of the library with what the tile needs.
#[derive(Debug, Clone)]
pub struct LibEntry {
    pub entry: Entry,
    /// The name a document uses (`A_circle`), for functions and constants.
    pub key: Option<String>,
    /// The file name without `std/` and `.mpp` (`trig`).
    pub file: String,
}

impl LibEntry {
    /// The text to insert at the caret: a call with empty boxes for a function,
    /// the name for a constant, the statement for an identity.
    pub fn insert_text(&self) -> String {
        match self.entry.kind {
            EntryKind::Function => {
                let head = self.entry.name.as_str();
                match head.find('(') {
                    Some(open) => {
                        let params = head[open + 1..].trim_end_matches(')');
                        let boxes = vec!["{}"; params.split(',').count()].join(", ");
                        format!("{}({boxes})", &head[..open])
                    }
                    None => head.to_string(),
                }
            }
            EntryKind::Constant => self
                .entry
                .name
                .split("\\equiv")
                .next()
                .unwrap_or(&self.entry.name)
                .split(":=")
                .next()
                .unwrap_or(&self.entry.name)
                .trim()
                .to_string(),
            EntryKind::Identity | EntryKind::Theorem => self.entry.name.clone(),
        }
    }

    /// The entry typeset for the list (LaTeX), with `\overset{?}{=}` read as `=`.
    pub fn latex(&self) -> String {
        match self.entry.kind {
            EntryKind::Theorem => mixed(&self.entry.name),
            _ => self.entry.name.replace("\\overset{?}{=}", "="),
        }
    }
}

/// Prose with `$…$` maths as one LaTeX line: the words in `\text{…}`, the maths
/// typeset (a description such as "the positive root of $x^2 = x + 1$").
pub fn mixed(s: &str) -> String {
    let mut out = String::new();
    for (i, part) in s.split('$').enumerate() {
        if i % 2 == 1 {
            if !part.trim().is_empty() {
                out.push_str(&format!("{{{part}}}"));
            }
        } else {
            let t: String = part
                .chars()
                .map(|c| match c {
                    '\\' | '{' | '}' | '_' | '^' | '`' | '*' | '#' | '%' | '&' | '~' => ' ',
                    c => c,
                })
                .collect();
            if !t.trim().is_empty() {
                out.push_str(&format!("\\text{{{t}}}"));
            }
        }
    }
    out
}

impl LibEntry {
    /// The head a `Using …` sentence names (`A_{\text{circle}}` for a function).
    pub fn head_latex(&self) -> String {
        let t = self.insert_text();
        t.split('(').next().unwrap_or(&t).trim().to_string()
    }

    /// The description as LaTeX (see [`mixed`]).
    pub fn description_latex(&self) -> String {
        mixed(&self.entry.description)
    }
}

/// Text that can sit in `\text{…}` with no markup showing.
pub fn plain(s: &str) -> String {
    let mut out = String::new();
    let mut in_math = false;
    for ch in s.chars() {
        match ch {
            '$' => in_math = !in_math,
            '\\' | '{' | '}' | '_' | '^' | '`' | '*' | '#' | '%' | '&' | '~' => out.push(' '),
            c => out.push(c),
        }
    }
    let _ = in_math;
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The binding name of a definition head (`A_{\text{circle}}(r)` → its key).
fn key_of(head: &str) -> Option<String> {
    let src = format!("{head} := 0");
    let (ast, _) = parse_math(&src, 0);
    let line = ast.lines.first()?;
    match &line.statement {
        Statement::Definition { head, .. } => match head {
            DefHead::Name(n) | DefHead::Function { name: n, .. } => mpp_eval::names::node_key(n),
            DefHead::Recurrence { name, .. } => mpp_eval::names::node_key(name),
            #[allow(unreachable_patterns)]
            _ => None,
        },
        _ => None,
    }
}

/// Every entry of the library, read once.
pub fn entries() -> &'static [LibEntry] {
    static ENTRIES: OnceLock<Vec<LibEntry>> = OnceLock::new();
    ENTRIES.get_or_init(|| {
        stdlib::index()
            .into_iter()
            .map(|entry| {
                let key = match entry.kind {
                    EntryKind::Function | EntryKind::Constant => {
                        let head = entry
                            .name
                            .split("\\equiv")
                            .next()
                            .unwrap_or(&entry.name)
                            .split(":=")
                            .next()
                            .unwrap_or(&entry.name)
                            .trim()
                            .to_string();
                        key_of(&head)
                    }
                    _ => None,
                };
                let file = entry
                    .file
                    .trim_start_matches("std/")
                    .trim_end_matches(".mpp")
                    .to_string();
                LibEntry { entry, key, file }
            })
            .collect()
    })
}

/// The names the library defines, for autocomplete and did-you-mean.
pub fn names() -> &'static [String] {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let mut v: Vec<String> = entries().iter().filter_map(|e| e.key.clone()).collect();
        v.sort();
        v.dedup();
        v
    })
}

/// The file of the library that defines `name`, if any.
pub fn file_of(name: &str) -> Option<&'static str> {
    entries()
        .iter()
        .find(|e| e.key.as_deref() == Some(name))
        .map(|e| e.entry.file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_library_has_named_functions() {
        assert!(!entries().is_empty());
        assert!(!names().is_empty());
        let f = entries()
            .iter()
            .find(|e| e.entry.kind == EntryKind::Function)
            .expect("a function");
        assert!(f.insert_text().contains("{}"));
        assert!(f.key.is_some());
        assert!(file_of(f.key.as_deref().unwrap_or("")).is_some());
    }

    #[test]
    fn plain_text_has_no_markup() {
        assert_eq!(plain("The $x^2$ \\textbf{bold}"), "The x 2 textbf bold");
    }
}
