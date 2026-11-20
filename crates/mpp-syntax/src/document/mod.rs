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
pub mod dump;
mod figure;
mod inline;
mod list;
mod sentence;
mod table;

use std::collections::HashMap;

pub use ast::*;

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
    diagnostics.sort_by_key(|d| (d.span.start, d.span.end));
    Document {
        blocks,
        diagnostics,
    }
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
