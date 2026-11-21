//! The optional paper features of the page: formatted comments, the title block,
//! numbered sections and theorems, citations with their reference list. A
//! document that uses none of them renders exactly as it would without this file.

use std::collections::HashMap;
use std::fmt::Write;

use mpp_syntax::document::{
    cite_name, parse_reference_entries, Block, BlockKind, Cite, CiteStyle, Comment, Directive,
    DirectiveKind, Document, InlineKind, Reference, References, TheoremKind, TitleField, TitleKind,
};

use super::{esc, plain_text, safe_url, slug, Renderer, ResultProvider};

/// What the whole document says about sections, citations and the title block,
/// read before any block is drawn (a citation may come before its entry).
#[derive(Debug, Default)]
pub(super) struct Paper {
    number_sections: bool,
    /// The heading level that starts a section: 2 when a single `#` is the title.
    base_level: u8,
    /// The running section numbers, by level from `base_level`.
    counters: [u32; 6],
    style: CiteStyle,
    /// Every known entry.
    refs: Vec<Reference>,
    /// Keys in listing order (numbered style): cited ones first, as first cited.
    order: Vec<String>,
    titles: Vec<TitleField>,
    title_done: bool,
    refs_done: bool,
    /// Footnote text by name, for the hover note.
    notes: HashMap<String, String>,
    /// `References from [[file]]` files the provider could not supply.
    missing_files: Vec<String>,
}

impl Paper {
    pub(super) fn scan(doc: &Document, results: &dyn ResultProvider) -> Paper {
        let mut p = Paper {
            style: doc.cite_style(),
            ..Paper::default()
        };
        doc.walk(&mut |b: &Block| match &b.kind {
            BlockKind::Directive(Directive {
                kind: DirectiveKind::NumberSections,
                ..
            }) => p.number_sections = true,
            BlockKind::FootnoteDef { name, content } => {
                p.notes.insert(name.clone(), plain_text(content));
            }
            BlockKind::References(r) => {
                p.refs.extend(r.entries.iter().cloned());
                if let Some((path, _)) = &r.from {
                    match results.references_source(path) {
                        Some(text) => p.refs.extend(parse_reference_entries(&text)),
                        None => p.missing_files.push(path.clone()),
                    }
                }
            }
            _ => {}
        });
        // `- [@key] …` items under a References heading define keys too.
        p.refs.extend(doc.reference_list_entries());
        p.base_level = doc.section_base_level();
        p.titles = doc
            .blocks
            .iter()
            .filter_map(|b| match &b.kind {
                BlockKind::Title(t) => Some(t.clone()),
                _ => None,
            })
            .collect();

        let mut order: Vec<String> = Vec::new();
        {
            let known: HashMap<&str, ()> = p.refs.iter().map(|r| (r.key.as_str(), ())).collect();
            doc.walk_inlines(&mut |i| {
                if let InlineKind::Cite(cites) = &i.kind {
                    for c in cites {
                        if known.contains_key(c.key.as_str()) && !order.contains(&c.key) {
                            order.push(c.key.clone());
                        }
                    }
                }
            });
        }
        for r in &p.refs {
            if !order.contains(&r.key) {
                order.push(r.key.clone());
            }
        }
        p.order = order;
        p
    }

    /// The title text, for the page title.
    pub(super) fn title_text(&self) -> Option<String> {
        self.titles
            .iter()
            .find(|t| t.kind == TitleKind::Title)
            .map(|t| plain_text(&t.content))
    }

    /// The hover text of a footnote.
    pub(super) fn note_text(&self, name: &str) -> Option<&str> {
        self.notes.get(name).map(String::as_str)
    }

    fn reference(&self, key: &str) -> Option<&Reference> {
        self.refs.iter().find(|r| r.key == key)
    }

    fn number_of(&self, key: &str) -> Option<usize> {
        self.order.iter().position(|k| k == key).map(|n| n + 1)
    }
}

impl Renderer<'_> {
    /// `2.1 ` before a heading when `%number sections` is on, otherwise nothing.
    pub(super) fn heading_number(&mut self, level: u8) -> String {
        let p = &mut self.paper;
        if !p.number_sections || level < p.base_level || self.in_comment {
            return String::new();
        }
        let idx = usize::from(level - p.base_level).min(p.counters.len() - 1);
        p.counters[idx] += 1;
        for c in p.counters.iter_mut().skip(idx + 1) {
            *c = 0;
        }
        let text: Vec<String> = p.counters[..=idx].iter().map(u32::to_string).collect();
        if idx == 0 {
            self.theorem_count = 0;
        }
        format!("<span class=\"sec-num\">{}</span> ", text.join("."))
    }

    /// `Theorem 3.` or, with numbered sections, `Theorem 2.1.`
    pub(super) fn theorem_label(&mut self, kind: TheoremKind) -> String {
        if self.in_comment {
            return format!("{}.", kind.as_str());
        }
        self.theorem_count += 1;
        let section = self.paper.counters[0];
        if self.paper.number_sections && section > 0 {
            format!("{} {section}.{}.", kind.as_str(), self.theorem_count)
        } else {
            format!("{} {}.", kind.as_str(), self.theorem_count)
        }
    }

    /// A comment as a note: a soft tinted box with a pencil marker. Nothing
    /// without `include_comments`.
    pub(super) fn comment(&mut self, c: &Comment, out: &mut String) {
        if !self.opts.include_comments {
            return;
        }
        let saved = std::mem::replace(&mut self.in_comment, true);
        let class = if c.is_commented_out_maths() {
            "note off"
        } else {
            "note"
        };
        let _ = writeln!(
            out,
            "<aside class=\"{class}\"><span class=\"note-mark\" aria-hidden=\"true\">✎</span><div class=\"note-body\">"
        );
        self.blocks(&c.blocks, out);
        out.push_str("</div></aside>\n");
        self.in_comment = saved;
    }

    /// The title, authors, date and abstract, drawn once where the first of them is.
    pub(super) fn title_block(&mut self, out: &mut String) {
        if self.paper.title_done {
            return;
        }
        self.paper.title_done = true;
        let fields = self.paper.titles.clone();
        out.push_str("<header class=\"title-block\">\n");
        for kind in TitleKind::ALL {
            for f in fields.iter().filter(|f| f.kind == *kind) {
                let html = self.inlines(&f.content);
                match kind {
                    TitleKind::Title => {
                        let _ = writeln!(out, "<h1 class=\"doc-title\">{html}</h1>");
                    }
                    TitleKind::Author => {
                        let _ = writeln!(out, "<p class=\"doc-author\">{html}</p>");
                    }
                    TitleKind::Date => {
                        let _ = writeln!(out, "<p class=\"doc-date\">{html}</p>");
                    }
                    TitleKind::Abstract => {
                        let _ = writeln!(
                            out,
                            "<div class=\"abstract\"><p class=\"abstract-head\">Abstract</p><p>{html}</p></div>"
                        );
                    }
                }
            }
        }
        out.push_str("</header>\n");
    }

    /// `[1]`, `[1, 3]` or `(Knuth 1984)`; a key nobody defined shows as `[?]`.
    pub(super) fn cite(&mut self, cites: &[Cite]) -> String {
        if self.in_comment {
            let keys: Vec<String> = cites.iter().map(|c| format!("@{}", c.key)).collect();
            return esc(&format!("[{}]", keys.join("; ")));
        }
        let author_year = self.paper.style == CiteStyle::AuthorYear;
        let mut parts = Vec::new();
        for c in cites {
            let id = esc(&slug(&c.key));
            let part = match self.paper.reference(&c.key) {
                None => format!(
                    "<span class=\"cite-missing\" title=\"no reference has the key {}\">?</span>",
                    esc(&c.key)
                ),
                Some(r) if author_year => format!(
                    "<a class=\"cite\" href=\"#ref-{id}\">{}</a>",
                    esc(&author_year_text(r))
                ),
                Some(_) => {
                    let n = self.paper.number_of(&c.key).unwrap_or(0);
                    format!("<a class=\"cite\" href=\"#ref-{id}\">{n}</a>")
                }
            };
            parts.push(part);
        }
        if author_year {
            format!("({})", parts.join("; "))
        } else {
            format!("[{}]", parts.join(", "))
        }
    }

    /// The generated list of references, at the first `References` block.
    pub(super) fn references_block(&mut self, r: &References, out: &mut String) {
        if self.paper.refs_done || self.in_comment {
            return;
        }
        self.paper.refs_done = true;
        let mut notes = String::new();
        for path in &self.paper.missing_files {
            let _ = writeln!(
                notes,
                "<p class=\"import\">The references in {} are not loaded.</p>",
                esc(path)
            );
        }
        let author_year = self.paper.style == CiteStyle::AuthorYear;
        let mut list: Vec<(usize, &Reference)> = self
            .paper
            .order
            .iter()
            .enumerate()
            .filter_map(|(i, k)| self.paper.reference(k).map(|r| (i + 1, r)))
            .collect();
        if list.is_empty() && notes.is_empty() && r.entries.is_empty() {
            return;
        }
        if author_year {
            list.sort_by_key(|(_, r)| {
                (
                    cite_name(&r.author).to_lowercase(),
                    r.year.clone().unwrap_or_default(),
                )
            });
        }
        out.push_str("<section class=\"references\">\n<h2>References</h2>\n");
        out.push_str(&notes);
        let style = if author_year {
            "author-year"
        } else {
            "numbered"
        };
        let _ = writeln!(out, "<ol class=\"refs {style}\">");
        for (n, r) in list {
            let id = esc(&slug(&r.key));
            let label = if author_year {
                String::new()
            } else {
                format!("<span class=\"ref-num\">[{n}]</span> ")
            };
            let stop = if r.author.ends_with('.') { "" } else { "." };
            let mut line = format!("{label}{}{stop} <em>{}</em>", esc(&r.author), esc(&r.title));
            if let Some(y) = &r.year {
                let _ = write!(line, ", {}", esc(y));
            }
            line.push('.');
            if let Some(l) = &r.link {
                let _ = write!(line, " <a href=\"{}\">{}</a>", esc(&safe_url(l)), esc(l));
            }
            let _ = writeln!(out, "<li id=\"ref-{id}\">{line}</li>");
        }
        out.push_str("</ol>\n</section>\n");
    }
}

fn author_year_text(r: &Reference) -> String {
    match &r.year {
        Some(y) => format!("{} {y}", cite_name(&r.author)),
        None => cite_name(&r.author),
    }
}
