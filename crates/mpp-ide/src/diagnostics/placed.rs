//! Placements of problems kept between page layouts.
//!
//! Every keystroke places every problem of the document on the new page, and a long
//! file can have thousands. A problem whose page block is laid out as before (same
//! text, same size, same boxes) lands in the same place relative to that block, so its
//! rectangles are taken from here, moved to where the block now is, and only the
//! problems of the blocks that changed are placed again.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use egui::{Rect, Vec2};
use mpp_syntax::Span;

use super::{Kind, Problem};
use crate::page::{Page, PlacedBlock};

/// Placements an entry may go unused before it is dropped.
const KEEP_FOR: u64 = 3;
/// Most entries kept, whatever their age.
const MAX_ENTRIES: usize = 200_000;

/// Where a placed problem ended up, relative to its block (offsets from the block's
/// start, rectangles from its top).
#[derive(Clone)]
struct Placement {
    span: (usize, usize),
    at: Option<(usize, usize)>,
    label: Option<String>,
    rects: Vec<Rect>,
    used: u64,
}

#[derive(Default)]
struct Cache {
    entries: HashMap<u64, Placement>,
    generation: u64,
}

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(Cache::default());
}

/// The page blocks sorted by start, to find the block of an offset without scanning
/// the whole page for each of thousands of problems.
pub(super) struct BlockIndex<'p> {
    page: &'p Page,
    /// (start, index into `page.blocks`), sorted by start.
    order: Vec<(usize, usize)>,
    /// The greatest end among `order[..=i]`.
    max_end: Vec<usize>,
}

impl<'p> BlockIndex<'p> {
    pub(super) fn new(page: &'p Page) -> Self {
        let mut order: Vec<(usize, usize)> = page
            .blocks
            .iter()
            .enumerate()
            .map(|(i, b)| (b.span.start, i))
            .collect();
        order.sort_unstable();
        let mut max_end = Vec::with_capacity(order.len());
        let mut m = 0;
        for &(_, i) in &order {
            m = m.max(page.blocks[i].span.end);
            max_end.push(m);
        }
        BlockIndex {
            page,
            order,
            max_end,
        }
    }

    /// The same block as [`Page::block_at`]: the first in page order that holds
    /// `offset`.
    pub(super) fn block_at(&self, offset: usize) -> Option<&'p PlacedBlock> {
        let upto = self.order.partition_point(|(s, _)| *s <= offset);
        let mut best: Option<usize> = None;
        for k in (0..upto).rev() {
            if self.max_end[k] < offset {
                break;
            }
            let i = self.order[k].1;
            if offset <= self.page.blocks[i].span.end && best.is_none_or(|b| i < b) {
                best = Some(i);
            }
        }
        best.map(|i| &self.page.blocks[i])
    }
}

fn key_of(p: &Problem, b: &PlacedBlock, page_width: f32) -> u64 {
    let rel = |o: usize| o.wrapping_sub(b.span.start);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (b.id, b.span.len(), b.height.to_bits(), b.dl.hits.len()).hash(&mut h);
    page_width.to_bits().hash(&mut h);
    (rel(p.span.start), rel(p.span.end), p.level).hash(&mut h);
    p.message.hash(&mut h);
    std::mem::discriminant(&p.kind).hash(&mut h);
    match &p.kind {
        Kind::MissingBracket {
            open,
            close,
            insert,
        } => (open.map(rel), close, rel(*insert)).hash(&mut h),
        Kind::Undefined { name, .. } | Kind::Unused { name } => name.hash(&mut h),
        Kind::EmptyBox { label } => label.hash(&mut h),
        _ => {}
    }
    h.finish()
}

/// Places `problems` on `page` with `place_one`, reusing the placement of every
/// problem whose block is laid out as when it was last placed.
pub(super) fn place_cached(
    problems: &mut [Problem],
    page: &Page,
    mut place_one: impl FnMut(&mut Problem),
) {
    let index = BlockIndex::new(page);
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        c.generation += 1;
        let generation = c.generation;
        for p in problems.iter_mut() {
            let Some(b) = index.block_at(p.span.start) else {
                place_one(p);
                continue;
            };
            let key = key_of(p, b, page.width);
            if let Some(hit) = c.entries.get_mut(&key) {
                hit.used = generation;
                apply(p, hit, b);
                p.block = index.block_at(p.span.start).map(|b| b.span);
                continue;
            }
            place_one(p);
            let rel = |s: Span| {
                (
                    s.start.wrapping_sub(b.span.start),
                    s.end.wrapping_sub(b.span.start),
                )
            };
            let at = match &p.kind {
                Kind::Undefined { at, .. } => at.map(rel),
                _ => None,
            };
            let label = match &p.kind {
                Kind::EmptyBox { label } => label.clone(),
                _ => None,
            };
            let up = Vec2::new(0.0, -b.top);
            c.entries.insert(
                key,
                Placement {
                    span: rel(p.span),
                    at,
                    label,
                    rects: p.rects.iter().map(|r| r.translate(up)).collect(),
                    used: generation,
                },
            );
        }
        if c.entries.len() > MAX_ENTRIES / 2 {
            c.entries
                .retain(|_, e| generation.saturating_sub(e.used) < KEEP_FOR);
        }
        if c.entries.len() > MAX_ENTRIES {
            c.entries.clear();
        }
    });
}

fn apply(p: &mut Problem, hit: &Placement, b: &PlacedBlock) {
    let base = b.span.start;
    let abs = move |(s, e): (usize, usize)| Span::new(s.wrapping_add(base), e.wrapping_add(base));
    p.span = Span::new(hit.span.0.wrapping_add(base), hit.span.1.wrapping_add(base));
    match &mut p.kind {
        Kind::Undefined { at, .. } => *at = hit.at.map(abs),
        Kind::EmptyBox { label } => {
            if let Some(l) = &hit.label {
                *label = Some(l.clone());
                p.rich = super::Rich::text("Empty box: ")
                    .i(super::box_name(l))
                    .t(".");
                p.message = p.rich.plain();
            }
        }
        _ => {}
    }
    let down = Vec2::new(0.0, b.top);
    p.rects = hit.rects.iter().map(|r| r.translate(down)).collect();
    p.rect = p
        .rects
        .iter()
        .copied()
        .reduce(|a, b| a.union(b))
        .unwrap_or(Rect::NOTHING);
}

/// Problems made from one diagnostic, relative to the start of the block it is in.
struct Classified {
    problems: Vec<Problem>,
    used: u64,
}

#[derive(Default)]
struct ClassifyCache {
    entries: HashMap<u64, Classified>,
    generation: u64,
}

thread_local! {
    static CLASSIFIED: RefCell<ClassifyCache> = RefCell::new(ClassifyCache::default());
}

/// Moves every source offset a problem holds by `delta` (before it is placed).
fn shift_problem(p: &mut Problem, delta: isize) {
    let mv = |o: usize| o.wrapping_add_signed(delta);
    p.span = Span::new(mv(p.span.start), mv(p.span.end));
    match &mut p.kind {
        Kind::MissingBracket { open, insert, .. } => {
            *open = open.map(mv);
            *insert = mv(*insert);
        }
        Kind::Undefined { at, .. } => *at = at.map(|s| Span::new(mv(s.start), mv(s.end))),
        _ => {}
    }
}

/// Classifies diagnostics with `classify`, reusing the problems of a diagnostic seen
/// before in a block with the same text at the same place in that block. `context`
/// stands for everything else classifying reads (the known names, the project).
pub(super) fn classify_cached(
    doc: &mpp_syntax::document::Document,
    raw: &[mpp_syntax::Diagnostic],
    context: u64,
    mut classify: impl FnMut(&mpp_syntax::Diagnostic) -> Vec<Problem>,
) -> Vec<Problem> {
    let blocks = &doc.blocks;
    CLASSIFIED.with(|c| {
        let mut c = c.borrow_mut();
        c.generation += 1;
        let generation = c.generation;
        let mut out = Vec::with_capacity(raw.len());
        for d in raw {
            // Top-level blocks are in source order and do not overlap.
            let i = blocks.partition_point(|b| b.span.start <= d.span.start);
            let block = i
                .checked_sub(1)
                .map(|i| &blocks[i])
                .filter(|b| d.span.end <= b.span.end);
            let Some(b) = block else {
                out.extend(classify(d));
                continue;
            };
            let start = b.span.start;
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (context, b.hash, b.span.len()).hash(&mut h);
            (d.span.start - start, d.span.end - start).hash(&mut h);
            (&d.message, &d.expected, d.severity).hash(&mut h);
            let key = h.finish();
            let delta = start as isize;
            if let Some(hit) = c.entries.get_mut(&key) {
                hit.used = generation;
                out.extend(hit.problems.iter().cloned().map(|mut p| {
                    shift_problem(&mut p, delta);
                    p
                }));
                continue;
            }
            let made = classify(d);
            let rel = made
                .iter()
                .cloned()
                .map(|mut p| {
                    shift_problem(&mut p, -delta);
                    p
                })
                .collect();
            c.entries.insert(
                key,
                Classified {
                    problems: rel,
                    used: generation,
                },
            );
            out.extend(made);
        }
        if c.entries.len() > MAX_ENTRIES / 2 {
            c.entries
                .retain(|_, e| generation.saturating_sub(e.used) < KEEP_FOR);
        }
        if c.entries.len() > MAX_ENTRIES {
            c.entries.clear();
        }
        out
    })
}
