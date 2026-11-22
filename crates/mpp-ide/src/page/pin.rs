//! Keeping the caret's line still on screen while the page is laid out again.
//!
//! The page is rebuilt when an answer arrives, a note appears or the text
//! changes. Anything that grows above the caret pushes its line down the page.
//! A [`Pin`] remembers where the caret's line was drawn (by the bottom of the
//! block above it); after the rebuild [`Pin::shift`] says how far that line
//! moved, and the view scrolls by as much, so the line the person is typing on
//! stays where it was, and so do the lines above it.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::{Page, Side};

/// Where the caret's line was on the page when it was last drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pin {
    /// What is pinned: the start of the last block above the caret's line, or
    /// (when there is none) the first offset on the caret's line.
    pub anchor: usize,
    /// Where that is on the page: the bottom of the block above, or the top of
    /// the caret's line.
    pub top: f32,
    /// True when the block above the caret's line is pinned.
    above: bool,
    /// A hash of the text before the anchor: when it changed, the anchor may
    /// point at other text and the pin no longer holds.
    prefix: u64,
}

fn hash_prefix(text: &str, anchor: usize) -> u64 {
    let mut h = DefaultHasher::new();
    text.get(..anchor).unwrap_or(text).hash(&mut h);
    h.finish()
}

impl Pin {
    /// The pin for a caret at `caret` (on `side`) on `page`, laid out from `text`.
    ///
    /// The bottom of the block above the caret's line is pinned rather than the
    /// line itself: what is typed on the line (a `$` that makes it a maths box,
    /// a fraction that makes it taller) never moves the lines above it, while
    /// anything growing above it (an answer, a note) still leaves it where it is.
    pub fn new(page: &Page, text: &str, caret: usize, side: Side) -> Option<Pin> {
        let cur = page.caret_stop_on(caret, side)?;
        let cy = (cur.top + cur.bottom) / 2.0;
        let first = page
            .stops
            .iter()
            .filter(|s| ((s.top + s.bottom) / 2.0 - cy).abs() < 6.0)
            .min_by(|a, b| super::cmp_f(a.x, b.x).then(a.offset.cmp(&b.offset)))?;
        let above = page
            .blocks
            .iter()
            .filter(|b| b.span.end <= first.offset && b.top + b.height <= first.top + 0.5)
            .max_by_key(|b| b.span.start);
        Some(match above {
            Some(b) => Pin {
                anchor: b.span.start,
                top: b.top + b.height,
                above: true,
                prefix: hash_prefix(text, b.span.start),
            },
            None => Pin {
                anchor: first.offset,
                top: first.top,
                above: false,
                prefix: hash_prefix(text, first.offset),
            },
        })
    }

    /// How far the pinned line moved down `page` (laid out from `text`) since
    /// the pin was taken. `None` when the text before it changed (an undo, a
    /// paste above): then the caret is simply scrolled into view.
    pub fn shift(&self, page: &Page, text: &str) -> Option<f32> {
        if self.anchor > text.len() || hash_prefix(text, self.anchor) != self.prefix {
            return None;
        }
        if self.above {
            let b = page.blocks.iter().find(|b| b.span.start == self.anchor)?;
            return Some(b.top + b.height - self.top);
        }
        let now = page.caret_stop_on(self.anchor, Side::After)?;
        Some(now.top - self.top)
    }
}

/// How far the line holding `caret` moved between two layouts of `text`.
#[cfg(test)]
pub(crate) fn pin_shift(old: &Page, new: &Page, text: &str, caret: usize) -> Option<f32> {
    Pin::new(old, text, caret, Side::Before)?.shift(new, text)
}
