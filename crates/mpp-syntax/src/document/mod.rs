//! The document layer: the `.mpp` prose markup, written in house.
//!
//! [`parse_document`] splits a file into [`Block`]s (headings, paragraphs,
//! lists, tables, display maths, figures, imports, sentence forms, theorems …)
//! and reads the inline markup inside them. Every `$…$` and `$$…$$` span is
//! handed to [`crate::math::parse_math`] with its offset in the file, so every
//! span in the result indexes the whole source and the maths diagnostics are
//! part of [`Document::diagnostics`]. Nothing here panics on any input.

mod ast;
mod block;
mod cursor;
pub mod display;
pub mod dump;
mod figure;
mod inline;
mod list;
pub mod proof;
mod refs;
mod sentence;
mod table;

use std::collections::HashMap;

pub use ast::*;
pub use figure::split_keyword_list;
pub use refs::{cite_name, parse_reference_entries};

use crate::span::{Diagnostic, Span};
use block::{parse_blocks, split_lines, Ctx};

/// Parses a whole `.mpp` file.
pub fn parse_document(src: &str) -> Document {
    let lines = split_lines(src);
    let mut ctx = Ctx::new(src);
    let mut blocks = parse_blocks(&mut ctx, &lines, 0);
    ctx.finish();

    let labels = std::mem::take(&mut ctx.labels);
    let mut diagnostics = std::mem::take(&mut ctx.diags);
    for b in &mut blocks {
        resolve_block(b, &labels, &mut diagnostics);
    }
    assign_ids(&mut blocks, src);
    let mut doc = Document {
        blocks,
        diagnostics: Vec::new(),
    };
    // A `References from [[file]]` list is only known once the file is read, so the
    // keys are checked by [`citation_diagnostics`] then.
    if !doc.has_references_file() {
        diagnostics.extend(citation_diagnostics(&doc, &[]));
    }
    diagnostics.sort_by_key(|d| (d.span.start, d.span.end));
    doc.diagnostics = diagnostics;
    doc
}

/// The innermost block that contains the byte `offset`. An offset just past
/// the end of a block (a cursor at the end of a line) counts as inside it.
pub fn block_at(doc: &Document, offset: usize) -> Option<&Block> {
    find_block(&doc.blocks, offset)
}

fn find_block(blocks: &[Block], offset: usize) -> Option<&Block> {
    let hit = blocks
        .iter()
        .find(|b| b.span.start <= offset && offset < b.span.end)
        .or_else(|| {
            blocks
                .iter()
                .find(|b| b.span.start <= offset && offset == b.span.end)
        })?;
    if let BlockKind::Theorem(t) = &hit.kind {
        if let Some(inner) = find_block(&t.body, offset) {
            return Some(inner);
        }
    }
    Some(hit)
}

impl Document {
    /// Visits every block, nested ones included, in document order.
    pub fn walk<'a>(&'a self, f: &mut impl FnMut(&'a Block)) {
        fn go<'a>(blocks: &'a [Block], f: &mut impl FnMut(&'a Block)) {
            for b in blocks {
                f(b);
                if let BlockKind::Theorem(t) = &b.kind {
                    go(&t.body, f);
                }
            }
        }
        go(&self.blocks, f);
    }

    /// The errors (not warnings) among the diagnostics.
    pub fn errors(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == crate::span::Severity::Error)
    }
}

// ---- hashing and ids -------------------------------------------------------

/// FNV-1a over `bytes`, starting from `seed`.
fn fnv(seed: u64, bytes: &[u8]) -> u64 {
    let mut h = seed;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

fn assign_ids(blocks: &mut [Block], src: &str) {
    let mut seen: HashMap<u64, u64> = HashMap::new();
    assign(blocks, src, &mut seen);

    fn assign(blocks: &mut [Block], src: &str, seen: &mut HashMap<u64, u64>) {
        for b in blocks {
            let text = src
                .as_bytes()
                .get(b.span.start..b.span.end)
                .unwrap_or_default();
            let mut hash = fnv(FNV_OFFSET, text);
            if let BlockKind::DisplayMath(m) = &b.kind {
                if m.noeval {
                    hash = fnv(hash, b"%noeval");
                }
            }
            b.hash = hash;
            let n = seen.entry(hash).or_insert(0);
            b.id = BlockId(if *n == 0 {
                hash
            } else {
                fnv(hash, &n.to_le_bytes())
            });
            *n += 1;
            if let BlockKind::Theorem(t) = &mut b.kind {
                assign(&mut t.body, src, seen);
            }
        }
    }
}

// ---- equation references ---------------------------------------------------

fn resolve_block(b: &mut Block, labels: &HashMap<String, u32>, diags: &mut Vec<Diagnostic>) {
    let mut each = |inlines: &mut Vec<Inline>| resolve(inlines, labels, diags);
    match &mut b.kind {
        BlockKind::Heading { content, .. } | BlockKind::FootnoteDef { content, .. } => {
            each(content)
        }
        BlockKind::Paragraph(content) => each(content),
        BlockKind::Let(l) => each(&mut l.content),
        BlockKind::Quote(paragraphs) => {
            for p in paragraphs {
                each(p);
            }
        }
        BlockKind::List(list) => resolve_list(list, &mut each),
        BlockKind::Table(t) => {
            for cell in t.header.iter_mut().chain(t.rows.iter_mut().flatten()) {
                each(cell);
            }
        }
        BlockKind::Figure(f) => each(&mut f.caption),
        BlockKind::Theorem(t) => {
            if let Some(title) = &mut t.title {
                resolve(title, labels, diags);
            }
            for inner in &mut t.body {
                resolve_block(inner, labels, diags);
            }
        }
        _ => {}
    }
}

fn resolve_list(list: &mut List, each: &mut impl FnMut(&mut Vec<Inline>)) {
    for item in &mut list.items {
        each(&mut item.content);
        for child in &mut item.children {
            resolve_list(child, each);
        }
    }
}

fn resolve(inlines: &mut [Inline], labels: &HashMap<String, u32>, diags: &mut Vec<Diagnostic>) {
    for inline in inlines {
        let span: Span = inline.span;
        match &mut inline.kind {
            InlineKind::EqRef { label, number } => match labels.get(label.as_str()) {
                Some(n) => *number = Some(*n),
                None => {
                    diags.push(
                        Diagnostic::error(span, format!("no equation has the label `{label}`"))
                            .expecting(["a `\\label{…}` on a display equation"]),
                    );
                }
            },
            InlineKind::Emphasis(inner) | InlineKind::Strong(inner) => {
                resolve(inner, labels, diags)
            }
            InlineKind::Link { text, .. } => resolve(text, labels, diags),
            _ => {}
        }
    }
}

// ---- citations -------------------------------------------------------------

fn visit_block_inlines<'a>(b: &'a Block, f: &mut impl FnMut(&'a Inline)) {
    fn each<'a>(items: &'a [Inline], f: &mut impl FnMut(&'a Inline)) {
        for i in items {
            f(i);
            match &i.kind {
                InlineKind::Emphasis(inner) | InlineKind::Strong(inner) => each(inner, f),
                InlineKind::Link { text, .. } => each(text, f),
                _ => {}
            }
        }
    }
    fn list<'a>(l: &'a List, f: &mut impl FnMut(&'a Inline)) {
        for item in &l.items {
            each(&item.content, f);
            for child in &item.children {
                list(child, f);
            }
        }
    }
    match &b.kind {
        BlockKind::Heading { content, .. }
        | BlockKind::Paragraph(content)
        | BlockKind::FootnoteDef { content, .. } => each(content, f),
        BlockKind::Title(t) => each(&t.content, f),
        BlockKind::Let(l) => each(&l.content, f),
        BlockKind::Quote(ps) => {
            for p in ps {
                each(p, f);
            }
        }
        BlockKind::List(l) => list(l, f),
        BlockKind::Table(t) => {
            for cell in t.header.iter().chain(t.rows.iter().flatten()) {
                each(cell, f);
            }
        }
        BlockKind::Figure(fig) => each(&fig.caption, f),
        BlockKind::Theorem(t) => {
            if let Some(title) = &t.title {
                each(title, f);
            }
            for inner in &t.body {
                visit_block_inlines(inner, f);
            }
        }
        _ => {}
    }
}

impl Document {
    /// Visits every inline of the document, nested blocks included, in order.
    /// Comments are skipped: nothing in them counts.
    pub fn walk_inlines<'a>(&'a self, f: &mut impl FnMut(&'a Inline)) {
        for b in &self.blocks {
            visit_block_inlines(b, f);
        }
    }

    /// The entries of every `References` block written in this file, in order.
    pub fn reference_entries(&self) -> Vec<&Reference> {
        let mut out = Vec::new();
        self.walk(&mut |b| {
            if let BlockKind::References(r) = &b.kind {
                out.extend(r.entries.iter());
            }
        });
        out
    }

    /// The entries written as a list under a `References` (or `Bibliography`) heading,
    /// one item per entry starting with its key: `- [@key] Author, *Title*, year.` The
    /// text after the key is kept whole as the entry's `title`; `author` is empty.
    pub fn reference_list_entries(&self) -> Vec<Reference> {
        let mut out = Vec::new();
        let mut under_references = false;
        for b in &self.blocks {
            match &b.kind {
                BlockKind::Heading { content, .. } => {
                    under_references = is_references_heading(content);
                }
                BlockKind::List(list) if under_references => {
                    for item in &list.items {
                        if let Some(entry) = list_item_reference(item) {
                            out.push(entry);
                        }
                    }
                }
                BlockKind::Comment(_) | BlockKind::Directive(_) => {}
                _ => {}
            }
        }
        out
    }

    /// True when a `References from [[file]]` block names a file whose entries
    /// have to be read before the citations can be checked.
    pub fn has_references_file(&self) -> bool {
        let mut found = false;
        self.walk(&mut |b| {
            if matches!(&b.kind, BlockKind::References(r) if r.from.is_some()) {
                found = true;
            }
        });
        found
    }

    /// The `%cite` style in force (the first such directive), numbered by default.
    pub fn cite_style(&self) -> CiteStyle {
        self.blocks
            .iter()
            .find_map(|b| match &b.kind {
                BlockKind::Directive(Directive {
                    kind: DirectiveKind::Cite(s),
                    ..
                }) => Some(*s),
                _ => None,
            })
            .unwrap_or_default()
    }
}

/// True for a heading that reads "References" or "Bibliography" (any case, with or
/// without a section number in front).
fn is_references_heading(content: &[Inline]) -> bool {
    let mut text = String::new();
    for i in content {
        if let InlineKind::Text(t) = &i.kind {
            text.push_str(t);
        }
    }
    let word = text
        .trim()
        .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ' ')
        .to_lowercase();
    word == "references" || word == "bibliography"
}

/// The entry in a reference-list item that starts with `[@key]`.
fn list_item_reference(item: &ListItem) -> Option<Reference> {
    let mut parts = item
        .content
        .iter()
        .skip_while(|i| matches!(&i.kind, InlineKind::Text(t) if t.trim().is_empty()));
    let first = parts.next()?;
    let InlineKind::Cite(cites) = &first.kind else {
        return None;
    };
    let [cite] = cites.as_slice() else {
        return None;
    };
    let mut rest = String::new();
    for i in parts {
        inline_plain_text(i, &mut rest);
    }
    Some(Reference {
        key: cite.key.clone(),
        author: String::new(),
        title: rest.trim().trim_start_matches(':').trim().to_string(),
        year: None,
        link: None,
        span: item.span,
    })
}

/// Appends the plain text of an inline (markup dropped, maths kept as written).
pub fn inline_plain_text(i: &Inline, out: &mut String) {
    match &i.kind {
        InlineKind::Text(t) => out.push_str(t),
        InlineKind::SoftBreak => out.push(' '),
        InlineKind::Emphasis(inner) | InlineKind::Strong(inner) => {
            for x in inner {
                inline_plain_text(x, out);
            }
        }
        InlineKind::Link { text, .. } => {
            for x in text {
                inline_plain_text(x, out);
            }
        }
        InlineKind::Code(c) => out.push_str(c),
        InlineKind::Escape(c) => out.push(*c),
        InlineKind::Cite(cites) => {
            let keys: Vec<String> = cites.iter().map(|c| format!("@{}", c.key)).collect();
            out.push('[');
            out.push_str(&keys.join("; "));
            out.push(']');
        }
        InlineKind::WikiLink { path, alias } => out.push_str(alias.as_deref().unwrap_or(path)),
        InlineKind::FootnoteRef(n) => {
            out.push_str("[^");
            out.push_str(n);
            out.push(']');
        }
        InlineKind::EqRef { label, number } => match number {
            Some(n) => out.push_str(&format!("({n})")),
            None => out.push_str(&format!("({label})")),
        },
        InlineKind::Math(_) => out.push_str("$…$"),
    }
}

/// Checks every `[@key]` against the entries written in the file plus `extra`
/// (the entries read from a `References from [[file]]` file): an unknown key is an
/// error, and so is a key defined twice.
pub fn citation_diagnostics(doc: &Document, extra: &[Reference]) -> Vec<Diagnostic> {
    let mut known: HashMap<&str, ()> = HashMap::new();
    let mut diags = Vec::new();
    let listed = doc.reference_list_entries();
    for r in doc
        .reference_entries()
        .into_iter()
        .chain(extra.iter())
        .chain(listed.iter())
    {
        if known.insert(r.key.as_str(), ()).is_some() {
            diags.push(Diagnostic::error(
                r.span,
                format!("the reference key `{}` is defined twice", r.key),
            ));
        }
    }
    let mut names: Vec<&str> = known.keys().copied().collect();
    names.sort_unstable();
    doc.walk_inlines(&mut |i| {
        if let InlineKind::Cite(cites) = &i.kind {
            for c in cites {
                if !known.contains_key(c.key.as_str()) {
                    let d =
                        Diagnostic::error(c.span, format!("no reference has the key `{}`", c.key));
                    diags.push(if names.is_empty() {
                        d.expecting(["an entry in a `References` block"])
                    } else {
                        d.expecting(names.iter().map(|n| format!("`@{n}`")))
                    });
                }
            }
        }
    });
    diags
}
