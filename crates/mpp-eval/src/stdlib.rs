//! The standard library: `.mpp` formula sheets embedded in the binary.
//!
//! The files live in the repository's `std/` folder and are embedded at build time
//! (see `build.rs`). They import like any file, `![[std/trig.mpp]]` or
//! `Using $\varphi$ from [[std/constants.mpp]].`, because the default
//! [`crate::FileImporter`] reads through [`embedded`] before the file system.
//!
//! [`index`] lists what each file offers (constants, functions, identities and
//! stated theorems, with the one-line description written above each), for the
//! IDE's library browser, search and autocomplete.

use mpp_syntax::document::{parse_document, Block, BlockKind, Inline};
use mpp_syntax::math::Statement;

use crate::files::EmbeddedSources;

include!(concat!(env!("OUT_DIR"), "/stdlib_files.rs"));

/// Every embedded file as (import path, text), in name order (`std/algebra.mpp`, …).
pub fn files() -> &'static [(&'static str, &'static str)] {
    FILES
}

/// The text of the embedded file `path` (`std/trig.mpp`).
pub fn source(path: &str) -> Option<&'static str> {
    let wanted = path.replace('\\', "/");
    let wanted = wanted.trim_start_matches("./");
    FILES.iter().find(|(p, _)| *p == wanted).map(|(_, t)| *t)
}

/// The standard library as a [`crate::SourceProvider`] layer.
pub fn embedded() -> EmbeddedSources {
    FILES
        .iter()
        .fold(EmbeddedSources::new(), |acc, (path, text)| {
            acc.with(path, text)
        })
}

/// What a library entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// `φ \equiv …`: a named constant.
    Constant,
    /// `A_{\text{circle}}(r) := …`: a formula to call.
    Function,
    /// `… \overset{?}{=} …`: an identity or value the library checks.
    Identity,
    /// A `**Theorem.**` block: stated, not computed.
    Theorem,
}

/// One entry of the library index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The import path of the file (`std/trig.mpp`).
    pub file: &'static str,
    /// The entry as written: the definition head (`A_{\text{circle}}(r)`), the whole
    /// identity, or the theorem's first sentence.
    pub name: String,
    pub kind: EntryKind,
    /// The prose line written above it (empty when there is none).
    pub description: String,
    /// The 1-based line in the file.
    pub line: usize,
}

/// Every constant, function, identity and theorem in the library, file by file.
pub fn index() -> Vec<Entry> {
    let mut out = Vec::new();
    for (file, text) in FILES {
        index_file(file, text, &mut out);
    }
    out
}

fn index_file(file: &'static str, text: &str, out: &mut Vec<Entry>) {
    let doc = parse_document(text);
    let line_of = |at: usize| text.get(..at).map_or(1, |s| s.matches('\n').count() + 1);
    let mut description = String::new();
    for block in &doc.blocks {
        match &block.kind {
            BlockKind::Paragraph(inlines) => description = plain(text, inlines),
            BlockKind::DisplayMath(m) => {
                for line in &m.ast.lines {
                    let src = text
                        .get(line.span.start..line.span.end)
                        .unwrap_or("")
                        .trim();
                    let (name, kind) = match &line.statement {
                        Statement::Definition { .. } => {
                            let head = src
                                .split(":=")
                                .next()
                                .and_then(|h| h.split("\\equiv").next())
                                .unwrap_or(src)
                                .trim();
                            let kind = if head.ends_with(')') {
                                EntryKind::Function
                            } else {
                                EntryKind::Constant
                            };
                            (head.to_string(), kind)
                        }
                        Statement::Check { .. } => (src.to_string(), EntryKind::Identity),
                        _ => continue,
                    };
                    out.push(Entry {
                        file,
                        name,
                        kind,
                        description: description.clone(),
                        line: line_of(line.span.start),
                    });
                }
            }
            BlockKind::Theorem(t) => {
                let body = theorem_text(text, &t.body);
                let name = body.split(". ").next().unwrap_or(&body).trim().to_string();
                out.push(Entry {
                    file,
                    name,
                    kind: EntryKind::Theorem,
                    description: description.clone(),
                    line: line_of(block.span.start),
                });
            }
            _ => {}
        }
    }
}

/// The source text of a run of inlines, on one line.
fn plain(text: &str, inlines: &[Inline]) -> String {
    let (Some(first), Some(last)) = (inlines.first(), inlines.last()) else {
        return String::new();
    };
    let raw = text.get(first.span.start..last.span.end).unwrap_or("");
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn theorem_text(text: &str, body: &[Block]) -> String {
    let mut parts = Vec::new();
    for b in body {
        if let BlockKind::Paragraph(inlines) = &b.kind {
            let mut p = plain(text, inlines);
            if let Some(rest) = p.strip_prefix("**Theorem.**") {
                p = rest.trim().to_string();
            }
            parts.push(p);
        }
    }
    let joined = parts.join(" ");
    // Text and maths only: drop emphasis markers so a hover reads cleanly.
    joined.replace("**", "").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_embedded_in_order() {
        let names: Vec<&str> = files().iter().map(|(p, _)| *p).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
        assert!(names
            .iter()
            .all(|n| n.starts_with("std/") && n.ends_with(".mpp")));
        assert!(source("std/constants.mpp").is_some());
        assert!(source("./std/constants.mpp").is_some());
        assert!(source("std/none.mpp").is_none());
        assert!(!embedded().is_empty());
    }
}
