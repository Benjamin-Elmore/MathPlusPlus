//! Source spans to typeset boxes.
//!
//! A problem points at a span of the `.mpp` text; the page shows typeset maths.
//! These functions find the exact boxes on the page that show that span: the
//! empty box a parser error is about, the node of a name (a subscript, the
//! integrand of a template), the opening bracket that is never closed, or the
//! words of a prose sentence, one rectangle per visual line.

use egui::{Pos2, Rect, Vec2};
use mpp_render::{HitBox, HitKind};
use mpp_syntax::Span;

use crate::page::{Page, PlacedBlock};

fn page_rect(b: &PlacedBlock, h: &HitBox) -> Rect {
    Rect::from_min_size(
        Pos2::new(h.rect.x, h.rect.y + b.top),
        Vec2::new(h.rect.w.max(0.0), h.rect.h.max(0.0)),
    )
}

/// The maths boxes (not results) of the blocks that overlap `near`, with their
/// blocks. Looking only there keeps placing thousands of problems linear in the page
/// rather than quadratic.
fn hits(page: &Page, near: Span) -> impl Iterator<Item = (&PlacedBlock, &HitBox)> {
    page.blocks
        .iter()
        .filter(move |b| b.span.start <= near.end && near.start <= b.span.end)
        .flat_map(|b| {
            b.dl.hits
                .iter()
                .filter(|h| h.kind != HitKind::Result)
                .map(move |h| (b, h))
        })
}

/// The empty box at a problem's span: a slot that starts inside it (the `}` of
/// an empty `{}`), or that a zero-width span points at.
pub fn slot_at(page: &Page, span: Span) -> Option<(Rect, Option<String>)> {
    hits(page, span)
        .filter(|(_, h)| h.kind == HitKind::Slot)
        .filter(|(_, h)| {
            let at = h.span.start;
            if span.is_empty() {
                at == span.start
            } else {
                span.start <= at && at < span.end.max(span.start + 1)
            }
        })
        .max_by_key(|(_, h)| h.level)
        .map(|(b, h)| (page_rect(b, h), h.label.clone()))
}

/// The box of the node whose source is exactly `span` (the deepest one when
/// nested nodes share a span, e.g. `{θ}` and `θ`).
pub fn node_exact(page: &Page, span: Span) -> Option<Rect> {
    hits(page, span)
        .filter(|(_, h)| h.kind == HitKind::Node && h.span == span)
        .max_by_key(|(_, h)| h.level)
        .map(|(b, h)| page_rect(b, h))
}

/// The smallest node box that covers the whole of `span`.
pub fn node_covering(page: &Page, span: Span) -> Option<Rect> {
    hits(page, span)
        .filter(|(_, h)| {
            h.kind == HitKind::Node
                && !h.span.is_empty()
                && h.span.start <= span.start
                && span.end <= h.span.end
        })
        .min_by_key(|(_, h)| (h.span.len(), u16::MAX - h.level))
        .map(|(b, h)| page_rect(b, h))
}

/// The outermost node boxes inside `span` (a box whose parent is also inside is
/// left out), one rectangle per group of boxes on the same visual line.
pub fn nodes_inside(page: &Page, span: Span) -> Vec<Rect> {
    let inside: Vec<(&PlacedBlock, &HitBox)> = hits(page, span)
        .filter(|(_, h)| !h.span.is_empty() && h.span.start >= span.start && h.span.end <= span.end)
        .collect();
    let outer: Vec<Rect> = inside
        .iter()
        .filter(|(_, h)| {
            !inside.iter().any(|(_, o)| {
                o.level < h.level && o.span.start <= h.span.start && h.span.end <= o.span.end
            })
        })
        .map(|(b, h)| page_rect(b, h))
        .collect();
    merge_lines(outer)
}

/// Joins rectangles that sit on the same line into one.
fn merge_lines(mut rects: Vec<Rect>) -> Vec<Rect> {
    rects.sort_by(|a, b| {
        a.top()
            .partial_cmp(&b.top())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut out: Vec<Rect> = vec![];
    for r in rects {
        match out
            .iter_mut()
            .find(|o| (o.center().y - r.center().y).abs() < o.height().max(r.height()) * 0.6)
        {
            Some(o) => *o = o.union(r),
            None => out.push(r),
        }
    }
    out
}

/// The opening bracket of the group that starts at `open`: the part of the
/// group's box to the left of its contents.
pub fn opener(page: &Page, open: usize) -> Option<Rect> {
    let (b, group) = hits(page, Span::point(open))
        .filter(|(_, h)| h.kind == HitKind::Node && h.span.start == open && !h.span.is_empty())
        .min_by_key(|(_, h)| h.level)?;
    let first_child = b
        .dl
        .hits
        .iter()
        .filter(|h| h.level > group.level && h.span.start > open && h.span.end <= group.span.end)
        .map(|h| h.rect.x)
        .fold(f32::INFINITY, f32::min);
    let r = page_rect(b, group);
    let right = if first_child.is_finite() && first_child > r.left() + 1.0 {
        first_child
    } else {
        // No contents (`(` alone): the bracket is about half an em wide.
        r.left() + (b.dl.font_size * 0.45).min(r.width())
    };
    Some(Rect::from_min_max(r.min, Pos2::new(right, r.bottom())))
}

/// The words of prose inside `span`, one rectangle per visual line, measured
/// from the caret stops at the start and end of each visible character.
pub fn prose(page: &Page, span: Span) -> Vec<Rect> {
    let mut rects = vec![];
    // Characters are sorted by start and stops by offset: search, do not scan.
    let first = page.chars.partition_point(|c| c.start < span.start);
    let stops_at = |o: usize| {
        let i = page.stops.partition_point(|s| s.offset < o);
        page.stops[i..].iter().take_while(move |s| s.offset == o)
    };
    for c in page.chars[first..]
        .iter()
        .take_while(|c| c.start <= span.end)
        .filter(|c| c.end <= span.end)
    {
        let a = stops_at(c.start).next();
        let b =
            stops_at(c.end).find(|s| a.is_some_and(|a| (s.top - a.top).abs() < 2.0 && s.x > a.x));
        if let (Some(a), Some(b)) = (a, b) {
            rects.push(Rect::from_min_max(
                Pos2::new(a.x, a.top),
                Pos2::new(b.x, b.bottom),
            ));
        }
    }
    merge_lines(rects)
}

/// The whole visible line(s) of `span` measured from its caret stops, one
/// rectangle per visual line (an import line: its words and its link).
pub fn line_ink(page: &Page, span: Span) -> Vec<Rect> {
    let first = page.stops.partition_point(|s| s.offset < span.start);
    let rects: Vec<Rect> = page.stops[first..]
        .iter()
        .take_while(|s| s.offset <= span.end)
        .map(|s| Rect::from_min_max(Pos2::new(s.x, s.top), Pos2::new(s.x, s.bottom)))
        .collect();
    merge_lines(rects)
        .into_iter()
        .filter(|r| r.width() > 0.5)
        .collect()
}

/// A thin box at the caret position of `offset` (a zero-width problem with
/// nothing typeset at it).
pub fn point(page: &Page, offset: usize) -> Option<Rect> {
    let s = page.caret_stop(offset)?;
    Some(Rect::from_min_max(
        Pos2::new(s.x - 1.0, s.top),
        Pos2::new(s.x + 7.0, s.bottom),
    ))
}

/// Every rectangle that shows `span`, best match first: an empty box, the node
/// with exactly that source, the nodes inside it, the prose words, the
/// smallest node around it, and as a last resort the caret position.
pub fn rects_for_span(page: &Page, span: Span) -> Vec<Rect> {
    if let Some((r, _)) = slot_at(page, span) {
        return vec![r];
    }
    if let Some(r) = node_exact(page, span) {
        return vec![r];
    }
    if !span.is_empty() {
        let maths = nodes_inside(page, span);
        if !maths.is_empty() {
            let words = prose(page, span);
            return merge_lines(maths.into_iter().chain(words).collect());
        }
        let words = prose(page, span);
        if !words.is_empty() {
            return words;
        }
        if let Some(r) = node_covering(page, span) {
            return vec![r];
        }
    }
    point(page, span.start).into_iter().collect()
}

/// Parses the source of a box as one expression. A subscript's box can stop
/// just before its closing brace (`x_{θ`), so that brace is tried too.
fn parse_node(src: &str, span: Span) -> Option<mpp_syntax::math::Node> {
    let parse = |end: usize| {
        let text = src.get(span.start..end)?;
        let (node, diags) = mpp_syntax::math::parse_expr(text, span.start);
        diags.is_empty().then_some(node)
    };
    parse(span.end).or_else(|| {
        (src.get(span.end..span.end + 1) == Some("}"))
            .then(|| parse(span.end + 1))
            .flatten()
    })
}

/// The nodes on the page that are names, inside `within`: (span, key, box).
/// A name inside another name (the `θ` of `x_θ`) is not a name of its own.
fn named_nodes(page: &Page, src: &str, within: Span) -> Vec<(Span, String, Rect)> {
    let mut all: Vec<(Span, String, Rect, u16)> = vec![];
    for (b, h) in hits(page, within) {
        if h.kind != HitKind::Node
            || h.span.is_empty()
            || h.span.start < within.start
            || h.span.end > within.end
            || h.span.len() > 64
        {
            continue;
        }
        let Some(node) = parse_node(src, h.span) else {
            continue;
        };
        if let Some(key) = mpp_eval::names::node_key(&node) {
            all.push((h.span, key, page_rect(b, h), h.level));
        }
    }
    // Keep a name unless a larger name holds it; among boxes with the same
    // source (`{θ}` and `θ`) keep the deepest.
    let mut out: Vec<(Span, String, Rect)> = all
        .iter()
        .filter(|(span, _, _, level)| {
            let inside_bigger = all.iter().any(|(s, _, _, _)| {
                s.start <= span.start && span.end <= s.end && s.len() > span.len()
            });
            let deeper_twin = all.iter().any(|(s, _, _, l)| s == span && l > level);
            !inside_bigger && !deeper_twin
        })
        .map(|(s, k, r, _)| (*s, k.clone(), *r))
        .collect();
    out.sort_by_key(|(s, _, _)| s.start);
    out.dedup_by(|a, b| a.0 == b.0);
    out
}

/// The nodes on the page whose source spells the name `key` (as the evaluator
/// keys it: `θ`, `x_θ`, `v_0`), inside `within`, in source order.
pub fn name_nodes(page: &Page, src: &str, within: Span, key: &str) -> Vec<(Span, Rect)> {
    named_nodes(page, src, within)
        .into_iter()
        .filter(|(_, k, _)| k == key)
        .map(|(s, _, r)| (s, r))
        .collect()
}
