//! Typeset maths kept between page layouts.
//!
//! Every keystroke lays the page out again, and typesetting the formulas is most of
//! that work. A formula whose text and layout options are unchanged (most of them: an
//! edit touches one block) is taken from here instead, with its source offsets moved
//! to where the text now starts. Entries not used by the last few layouts are dropped.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use mpp_render::{layout_math, DisplayList, LayoutOptions};
use mpp_syntax::math::MathAst;
use mpp_syntax::Span;

use super::{BlockTag, Canvas};

/// Layouts an entry may go unused before it is dropped.
const KEEP_FOR: u64 = 4;
/// Most entries kept, whatever their age.
const MAX_ENTRIES: usize = 20_000;

struct Entry {
    /// The maths text and the options (offsets relative to the text) it was laid out
    /// with; compared in full on a hit.
    key: String,
    /// Where the text started when it was laid out.
    start: usize,
    dl: DisplayList,
    used: u64,
}

/// A laid-out prose block: its canvas (offsets as laid out), height and tag.
struct BlockEntry {
    key: String,
    start: usize,
    canvas: Canvas,
    height: f32,
    tag: BlockTag,
    used: u64,
}

#[derive(Default)]
struct Cache {
    entries: HashMap<u64, Entry>,
    blocks: HashMap<u64, BlockEntry>,
    generation: u64,
}

fn hash_of(key: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut h);
    h.finish()
}

/// Moves every source offset of a canvas by `to - from`.
fn shift_canvas(c: &mut Canvas, from: usize, to: usize) {
    if from == to {
        return;
    }
    let mv = |o: usize| (o + to).wrapping_sub(from);
    for h in &mut c.hits {
        h.span = Span::new(mv(h.span.start), mv(h.span.end));
    }
    for s in &mut c.stops {
        s.offset = mv(s.offset);
    }
    for ch in &mut c.chars {
        *ch = Span::new(mv(ch.start), mv(ch.end));
    }
    for t in &mut c.toggles {
        t.line = Span::new(mv(t.line.start), mv(t.line.end));
    }
}

/// A prose block laid out before with the same `key` (its text and everything its
/// layout depends on, with offsets relative to `start`), moved to `start`.
pub(crate) fn cached_block(key: &str, start: usize) -> Option<(Canvas, f32, BlockTag)> {
    let hash = hash_of(key);
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        let now = c.generation;
        let e = c.blocks.get_mut(&hash).filter(|e| e.key == key)?;
        e.used = now;
        let mut canvas = e.canvas.clone();
        shift_canvas(&mut canvas, e.start, start);
        Some((canvas, e.height, e.tag))
    })
}

/// Remembers a prose block's layout for [`cached_block`].
pub(crate) fn store_block(key: String, start: usize, canvas: &Canvas, height: f32, tag: BlockTag) {
    let hash = hash_of(&key);
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        let used = c.generation;
        c.blocks.insert(
            hash,
            BlockEntry {
                key,
                start,
                canvas: canvas.clone(),
                height,
                tag,
                used,
            },
        );
    });
}

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(Cache::default());
}

/// Starts a page layout: entries used from now on count as fresh.
pub(crate) fn begin() {
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        c.generation += 1;
        let now = c.generation;
        c.entries.retain(|_, e| now - e.used <= KEEP_FOR);
        c.blocks.retain(|_, e| now - e.used <= KEEP_FOR);
        if c.entries.len() > MAX_ENTRIES {
            c.entries.clear();
        }
        if c.blocks.len() > MAX_ENTRIES {
            c.blocks.clear();
        }
    });
}

/// [`layout_math`] for the maths `text` that starts at `raw.start` in the file.
pub(crate) fn layout(text: &str, raw: Span, ast: &MathAst, opts: &LayoutOptions) -> DisplayList {
    let relative = LayoutOptions {
        slot_labels: opts
            .slot_labels
            .iter()
            .map(|(o, l)| (o.wrapping_sub(raw.start), l.clone()))
            .collect(),
        ..opts.clone()
    };
    let key = format!("{text}\u{0}{relative:?}");
    let hash = hash_of(&key);
    let hit = CACHE.with(|c| {
        let mut c = c.borrow_mut();
        let now = c.generation;
        let e = c.entries.get_mut(&hash).filter(|e| e.key == key)?;
        e.used = now;
        Some((e.start, e.dl.clone()))
    });
    if let Some((start, mut dl)) = hit {
        if start != raw.start {
            for h in &mut dl.hits {
                h.span = Span::new(
                    (h.span.start + raw.start).wrapping_sub(start),
                    (h.span.end + raw.start).wrapping_sub(start),
                );
            }
        }
        return dl;
    }
    let dl = layout_math(ast, opts);
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        let used = c.generation;
        c.entries.insert(
            hash,
            Entry {
                key,
                start: raw.start,
                dl: dl.clone(),
                used,
            },
        );
    });
    dl
}
