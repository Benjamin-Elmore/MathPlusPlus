//! The paper features of the equation view: section and theorem numbers,
//! citations with their reference list, footnote marks and equation labels.
//!
//! [`PaperInfo::scan`] reads the whole document first (a citation may come before
//! its entry, a footnote before its note); the counters run while blocks are laid
//! out in order.

use std::cell::RefCell;
use std::collections::HashMap;

use mpp_syntax::document::{
    cite_name, Block, BlockKind, CiteStyle, Directive, DirectiveKind, Document, InlineKind,
    Reference, TheoremKind,
};
use mpp_syntax::Span;

use super::{Link, LinkTarget};
use crate::explorer::outline::inlines_latex;

/// Entries of `References from [[file]]` files, read by the document beside the
/// page, and the files that could not be read.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExtraRefs {
    pub entries: Vec<Reference>,
    pub missing: Vec<String>,
}

impl ExtraRefs {
    /// Reads the files of the document's `References from [[file]]` blocks,
    /// relative to `base` (the document's folder).
    pub fn read(doc: &Document, base: Option<&std::path::Path>) -> ExtraRefs {
        use mpp_eval::Importer;
        let mut files = vec![];
        doc.walk(&mut |b| {
            if let BlockKind::References(r) = &b.kind {
                if let Some((path, _)) = &r.from {
                    files.push(path.clone());
                }
            }
        });
        let mut out = ExtraRefs::default();
        if files.is_empty() {
            return out;
        }
        let importer = crate::eval::FsImporter::default();
        for path in files {
            match importer.read_data(base, &path) {
                Ok((_, text)) => out
                    .entries
                    .extend(mpp_syntax::document::parse_reference_entries(&text)),
                Err(_) => out.missing.push(path),
            }
        }
        out
    }
}

/// A footnote: its number (by first mark), where its note is, and the note typeset.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub number: usize,
    pub span: Span,
    pub tip: String,
}

#[derive(Debug, Default)]
struct Counters {
    sections: [u32; 6],
    theorems: u32,
    in_comment: bool,
}

/// What the whole document says about numbering, citations and footnotes.
#[derive(Debug, Default)]
pub struct PaperInfo {
    pub style: CiteStyle,
    /// Every known entry: written in the file, then read from files.
    pub refs: Vec<Reference>,
    /// Keys in listing order for the numbered style (cited first, as first cited).
    pub order: Vec<String>,
    /// `References from [[file]]` files that could not be read.
    pub missing_files: Vec<String>,
    /// Footnotes by name.
    pub notes: HashMap<String, Note>,
    /// Labelled equations by label: the span of their maths block.
    pub equations: HashMap<String, Span>,
    /// The first `References` block (where the list is drawn).
    pub ref_block: Option<Span>,
    pub number_sections: bool,
    /// The heading level that starts a section: 2 when a single `#` is the title.
    pub base_level: u8,
    counters: RefCell<Counters>,
}

impl PaperInfo {
    /// Reads the document (and the entries of its reference files).
    pub fn scan(doc: &Document, src: &str, extra: &ExtraRefs) -> PaperInfo {
        let mut p = PaperInfo {
            style: doc.cite_style(),
            refs: doc.reference_entries().into_iter().cloned().collect(),
            missing_files: extra.missing.clone(),
            ..PaperInfo::default()
        };
        p.refs.extend(extra.entries.iter().cloned());
        // `- [@key] …` items under a References heading define keys too.
        p.refs.extend(doc.reference_list_entries());
        let mut h1 = 0;
        let mut deeper = false;
        let mut defs: HashMap<String, (Span, String)> = HashMap::new();
        doc.walk(&mut |b: &Block| match &b.kind {
            BlockKind::Directive(Directive {
                kind: DirectiveKind::NumberSections,
                ..
            }) => p.number_sections = true,
            BlockKind::Heading { level, .. } => {
                if *level == 1 {
                    h1 += 1;
                } else {
                    deeper = true;
                }
            }
            BlockKind::FootnoteDef { name, content } => {
                defs.insert(name.clone(), (b.span, inlines_latex(content, src)));
            }
            BlockKind::DisplayMath(m) => {
                if let Some(l) = &m.label {
                    p.equations.insert(l.clone(), b.span);
                }
            }
            BlockKind::References(_) if p.ref_block.is_none() => p.ref_block = Some(b.span),
            _ => {}
        });
        p.base_level = if h1 == 1 && deeper { 2 } else { 1 };

        let mut order: Vec<String> = Vec::new();
        let mut marks: Vec<String> = Vec::new();
        doc.walk_inlines(&mut |i| match &i.kind {
            InlineKind::Cite(cites) => {
                for c in cites {
                    let known = p.refs.iter().any(|r| r.key == c.key);
                    if known && !order.contains(&c.key) {
                        order.push(c.key.clone());
                    }
                }
            }
            InlineKind::FootnoteRef(name) if !marks.contains(name) => marks.push(name.clone()),
            _ => {}
        });
        for r in &p.refs {
            if !order.contains(&r.key) {
                order.push(r.key.clone());
            }
        }
        p.order = order;
        for (i, name) in marks.iter().enumerate() {
            if let Some((span, tip)) = defs.get(name) {
                p.notes.insert(
                    name.clone(),
                    Note {
                        number: i + 1,
                        span: *span,
                        tip: tip.clone(),
                    },
                );
            }
        }
        p
    }

    /// True while a comment's blocks are laid out (nothing in one is numbered or linked).
    pub fn in_comment(&self) -> bool {
        self.counters.borrow().in_comment
    }

    /// Marks the start or end of a comment; returns the previous state.
    pub fn set_in_comment(&self, on: bool) -> bool {
        std::mem::replace(&mut self.counters.borrow_mut().in_comment, on)
    }

    /// The opening bracket, separator and closing bracket of a citation.
    pub fn brackets(&self) -> (&'static str, &'static str, &'static str) {
        match self.style {
            CiteStyle::Numbered => ("[", ", ", "]"),
            CiteStyle::AuthorYear => ("(", "; ", ")"),
        }
    }

    fn reference(&self, key: &str) -> Option<&Reference> {
        self.refs.iter().find(|r| r.key == key)
    }

    /// Where a click on a citation of `r` goes: its entry when it is written in
    /// this file, otherwise the reference list.
    fn entry_target(&self, r: &Reference) -> Span {
        let in_file = self
            .ref_block
            .is_some_and(|b| b.start <= r.span.start && r.span.end <= b.end);
        if in_file {
            r.span
        } else {
            self.ref_block.unwrap_or(r.span)
        }
    }

    /// The label of one cited key (`3` or `Knuth 1984`) and its link, or `None`
    /// for a key nobody defined.
    pub fn cite(&self, key: &str) -> Option<(String, Link)> {
        let r = self.reference(key)?;
        let label = match self.style {
            CiteStyle::Numbered => {
                let n = self
                    .order
                    .iter()
                    .position(|k| k == key)
                    .map_or(0, |n| n + 1);
                n.to_string()
            }
            CiteStyle::AuthorYear => author_year(r),
        };
        let link = Link {
            target: LinkTarget::Jump(self.entry_target(r)),
            tip: Some(entry_latex(r)),
        };
        Some((label, link))
    }

    /// The entries in the order the list shows them, with their numbered label
    /// (`[1]`) or none (author-year).
    pub fn listing(&self) -> Vec<(Option<usize>, &Reference)> {
        let mut list: Vec<(Option<usize>, &Reference)> = self
            .order
            .iter()
            .enumerate()
            .filter_map(|(i, k)| self.reference(k).map(|r| (Some(i + 1), r)))
            .collect();
        if self.style == CiteStyle::AuthorYear {
            list.sort_by_key(|(_, r)| {
                (
                    cite_name(&r.author).to_lowercase(),
                    r.year.clone().unwrap_or_default(),
                )
            });
            for e in &mut list {
                e.0 = None;
            }
        }
        list
    }

    /// `2.1` before a heading when `%number sections` is on.
    pub fn heading_number(&self, level: u8) -> Option<String> {
        let mut c = self.counters.borrow_mut();
        if !self.number_sections || level < self.base_level || c.in_comment {
            return None;
        }
        let idx = usize::from(level - self.base_level).min(c.sections.len() - 1);
        c.sections[idx] += 1;
        for s in c.sections.iter_mut().skip(idx + 1) {
            *s = 0;
        }
        if idx == 0 {
            c.theorems = 0;
        }
        let parts: Vec<String> = c.sections[..=idx].iter().map(u32::to_string).collect();
        Some(parts.join("."))
    }

    /// `Theorem 3.`, or `Theorem 2.1.` with numbered sections; `Proof.` and
    /// `Remark.` are not numbered.
    pub fn theorem_label(&self, kind: TheoremKind) -> String {
        let mut c = self.counters.borrow_mut();
        if c.in_comment || !numbered(kind) {
            return format!("{}.", kind.as_str());
        }
        c.theorems += 1;
        let section = c.sections[0];
        if self.number_sections && section > 0 {
            format!("{} {section}.{}.", kind.as_str(), c.theorems)
        } else {
            format!("{} {}.", kind.as_str(), c.theorems)
        }
    }
}

/// Whether a theorem-like block gets a number.
pub fn numbered(kind: TheoremKind) -> bool {
    !matches!(kind, TheoremKind::Proof | TheoremKind::Remark)
}

/// Whether a theorem-like block has an italic body (theorem, lemma, corollary,
/// proposition), as in a paper.
pub fn italic_body(kind: TheoremKind) -> bool {
    matches!(
        kind,
        TheoremKind::Theorem
            | TheoremKind::Lemma
            | TheoremKind::Corollary
            | TheoremKind::Proposition
            | TheoremKind::Conjecture
    )
}

/// `Knuth 1984`.
pub fn author_year(r: &Reference) -> String {
    match &r.year {
        Some(y) => format!("{} {y}", cite_name(&r.author)),
        None => cite_name(&r.author),
    }
}

/// A reference entry as plain reading text: `Knuth, D. E. The TeXbook, 1984.`
pub fn entry_text(r: &Reference) -> (String, String, String) {
    let stop = if r.author.ends_with('.') { "" } else { "." };
    let head = format!("{}{stop} ", r.author);
    let mut tail = String::new();
    if let Some(y) = &r.year {
        tail.push_str(&format!(", {y}"));
    }
    tail.push('.');
    if let Some(l) = &r.link {
        tail.push(' ');
        tail.push_str(l);
    }
    (head, r.title.clone(), tail)
}

/// A reference entry typeset for a hover card.
pub fn entry_latex(r: &Reference) -> String {
    let (head, title, tail) = entry_text(r);
    format!(
        "\\text{{{}}}\\textit{{{}}}\\text{{{}}}",
        tex_text(&head),
        tex_text(&title),
        tex_text(&tail)
    )
}

/// Text that is safe inside `\text{…}`.
fn tex_text(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '{' | '}' | '\\' | '$'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mpp_syntax::document::parse_document;

    const SRC: &str = "%number sections\n\n# One\n\nSee [@b] and [@a].[^n]\n\n$$ x := 1 \\label{e} $$\n\n**Theorem.** True.\n\n# Two\n\n**Lemma.** Also.\n\n[^n]: A note.\n\nReferences\n[@a] Alpha, A. | First | 2001\n[@b] Beta, B. | Second | 1999\n";

    #[test]
    fn citations_number_in_order_of_first_use() {
        let doc = parse_document(SRC);
        let p = PaperInfo::scan(&doc, SRC, &ExtraRefs::default());
        assert_eq!(p.cite("b").map(|c| c.0).as_deref(), Some("1"));
        assert_eq!(p.cite("a").map(|c| c.0).as_deref(), Some("2"));
        assert!(p.cite("zzz").is_none());
        assert_eq!(p.notes.get("n").map(|n| n.number), Some(1));
        assert!(p.equations.contains_key("e"));
        let listing: Vec<&str> = p.listing().iter().map(|(_, r)| r.key.as_str()).collect();
        assert_eq!(listing, ["b", "a"]);
    }

    #[test]
    fn theorems_number_per_section() {
        let doc = parse_document(SRC);
        let p = PaperInfo::scan(&doc, SRC, &ExtraRefs::default());
        assert_eq!(p.heading_number(1).as_deref(), Some("1"));
        assert_eq!(p.theorem_label(TheoremKind::Theorem), "Theorem 1.1.");
        assert_eq!(p.theorem_label(TheoremKind::Proof), "Proof.");
        assert_eq!(p.heading_number(1).as_deref(), Some("2"));
        assert_eq!(p.theorem_label(TheoremKind::Lemma), "Lemma 2.1.");
    }

    #[test]
    fn author_year_lists_alphabetically() {
        let src = format!("%cite author-year\n\n{SRC}");
        let doc = parse_document(&src);
        let p = PaperInfo::scan(&doc, &src, &ExtraRefs::default());
        assert_eq!(p.cite("a").map(|c| c.0).as_deref(), Some("Alpha 2001"));
        let listing: Vec<&str> = p.listing().iter().map(|(_, r)| r.key.as_str()).collect();
        assert_eq!(listing, ["a", "b"]);
    }
}
