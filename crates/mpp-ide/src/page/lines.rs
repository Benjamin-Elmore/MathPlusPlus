//! Moving the caret by lines (E15, R15): Up and Down go to the line above or
//! below, keeping the goal column, and never box by box inside one line's
//! fractions, sums and scripts (Left, Right and Tab walk those). Home and End go
//! to the ends of the line; PageUp and PageDown move by a screenful.
//!
//! A line is a row of a maths block (rows are parted by `\`) or a visual line
//! of prose. Inside a maths row the caret lands on the row's outer level (not in
//! a numerator or a limit) when it can.

use mpp_render::CaretStop;

use super::{BlockTag, Page, Side};

fn mid(s: &CaretStop) -> f32 {
    (s.top + s.bottom) / 2.0
}

fn cmp_f(a: f32, b: f32) -> std::cmp::Ordering {
    a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
}

/// Which line a stop is on: a maths block's row, or a prose stop's own line.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Line {
    Row { block: usize, row: usize },
    Visual(f32),
}

impl Page {
    /// The maths block holding `offset` (its index), if it is in one.
    fn maths_block(&self, offset: usize) -> Option<usize> {
        self.blocks
            .iter()
            .position(|b| b.tag == BlockTag::Math && b.span.start <= offset && offset <= b.span.end)
    }

    fn line_of(&self, text: &str, s: &CaretStop) -> Line {
        match self.maths_block(s.offset) {
            Some(i) => {
                let start = self.blocks[i].span.start.min(text.len());
                let end = s.offset.clamp(start, text.len());
                let row = text
                    .get(start..end)
                    .map_or(0, |t| t.matches("\\\\").count());
                Line::Row { block: i, row }
            }
            None => Line::Visual(mid(s)),
        }
    }

    fn on_line(&self, text: &str, line: Line, s: &CaretStop) -> bool {
        match (line, self.line_of(text, s)) {
            (Line::Row { .. }, l) => l == line,
            (Line::Visual(y), Line::Visual(_)) => (mid(s) - y).abs() < 6.0,
            (Line::Visual(_), Line::Row { .. }) => false,
        }
    }

    /// The stops on `line`; in a maths row, those on its outer level (outside
    /// every `{…}`) when there are any.
    fn line_stops(&self, text: &str, line: Line) -> Vec<CaretStop> {
        let all: Vec<CaretStop> = self
            .stops
            .iter()
            .filter(|s| self.on_line(text, line, s))
            .copied()
            .collect();
        let Line::Row { block, .. } = line else {
            return all;
        };
        let start = self.blocks[block].span.start;
        let outer: Vec<CaretStop> = all
            .iter()
            .filter(|s| depth(text, start, s.offset) == 0)
            .copied()
            .collect();
        if outer.is_empty() {
            all
        } else {
            outer
        }
    }

    /// The caret's line and how far it reaches up and down on the page.
    fn caret_line(&self, text: &str, offset: usize, side: Side) -> Option<(Line, f32, f32)> {
        let cur = self.caret_stop_on(offset, side)?;
        let line = self.line_of(text, &cur);
        let (mut top, mut bottom) = (cur.top, cur.bottom);
        if let Line::Row { .. } = line {
            for s in self.stops.iter().filter(|s| self.line_of(text, s) == line) {
                top = top.min(s.top);
                bottom = bottom.max(s.bottom);
            }
        }
        Some((line, top, bottom))
    }

    /// The offset (and side) on the line above (`up`) or below the caret's,
    /// nearest to `want_x`. None when there is no line there.
    pub fn line_vertical(
        &self,
        text: &str,
        offset: usize,
        side: Side,
        up: bool,
        want_x: Option<f32>,
    ) -> Option<(usize, Side)> {
        let cur = self.caret_stop_on(offset, side)?;
        let (_, top, bottom) = self.caret_line(text, offset, side)?;
        let x = want_x.unwrap_or(cur.x);
        let next = self
            .stops
            .iter()
            .filter(|s| if up { mid(s) < top } else { mid(s) > bottom })
            .min_by(|a, b| {
                let (da, db) = if up {
                    (top - mid(a), top - mid(b))
                } else {
                    (mid(a) - bottom, mid(b) - bottom)
                };
                cmp_f(da, db)
            })?;
        let line = self.line_of(text, next);
        self.nearest_on(text, line, x)
    }

    fn nearest_on(&self, text: &str, line: Line, x: f32) -> Option<(usize, Side)> {
        self.line_stops(text, line)
            .into_iter()
            .min_by(|a, b| cmp_f((a.x - x).abs(), (b.x - x).abs()))
            .map(|s| (s.offset, self.side_of(&s)))
    }

    /// The first and last offsets of the caret's line (Home and End).
    pub fn line_ends(&self, text: &str, offset: usize, side: Side) -> Option<(usize, usize)> {
        let (line, ..) = self.caret_line(text, offset, side)?;
        let stops = self.line_stops(text, line);
        let lo = stops.iter().map(|s| s.offset).min()?;
        let hi = stops.iter().map(|s| s.offset).max()?;
        Some((lo, hi))
    }

    /// The offset a screenful (`height`) above or below the caret, at about
    /// `want_x`: the first or last line when there is no screenful left.
    pub fn page_vertical(
        &self,
        text: &str,
        offset: usize,
        side: Side,
        up: bool,
        height: f32,
        want_x: Option<f32>,
    ) -> Option<(usize, Side)> {
        let cur = self.caret_stop_on(offset, side)?;
        let x = want_x.unwrap_or(cur.x);
        let y = if up {
            mid(&cur) - height
        } else {
            mid(&cur) + height
        };
        let target = self
            .stops
            .iter()
            .min_by(|a, b| cmp_f((mid(a) - y).abs(), (mid(b) - y).abs()))?;
        if (mid(target) - mid(&cur)).abs() < 1.0 {
            return None;
        }
        let line = self.line_of(text, target);
        self.nearest_on(text, line, x)
    }
}

/// How many `{…}` groups are open at `offset`, counted from `start`.
fn depth(text: &str, start: usize, offset: usize) -> usize {
    let Some(t) = text.get(start..offset.min(text.len())) else {
        return 0;
    };
    let mut d: isize = 0;
    let mut esc = false;
    for c in t.chars() {
        match c {
            '\\' => {
                esc = !esc;
                continue;
            }
            '{' if !esc => d += 1,
            '}' if !esc => d -= 1,
            _ => {}
        }
        esc = false;
    }
    d.max(0) as usize
}
