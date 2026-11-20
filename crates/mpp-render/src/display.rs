//! The output of layout: a flat list of drawing primitives in pixels, plus the
//! boxes the IDE needs to map a click or a caret to the source.
//!
//! Coordinates have their origin at the top-left corner of the formula's box with
//! y growing downwards; the baseline sits at `y = height`.

use mpp_syntax::Span;

use crate::font::FontId;

/// An sRGB colour with alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// An opaque colour.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Color { r, g, b, a: 255 }
    }

    /// `#rrggbb`, as used in SVG and CSS.
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// Opacity from 0 to 1.
    pub fn opacity(self) -> f32 {
        f32::from(self.a) / 255.0
    }
}

/// A rectangle: `x`, `y` is the top-left corner.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    /// True when the point lies inside or on the edge.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }

    /// Area of the rectangle.
    pub fn area(&self) -> f32 {
        self.w.max(0.0) * self.h.max(0.0)
    }
}

/// One step of a vector path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathCmd {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    QuadTo(f32, f32, f32, f32),
    CubicTo(f32, f32, f32, f32, f32, f32),
    Close,
}

/// A drawing primitive.
#[derive(Debug, Clone, PartialEq)]
pub enum Primitive {
    /// One glyph with its origin (left end of the baseline) at `x`, `y`.
    Glyph {
        font: FontId,
        glyph: u16,
        /// The character it shows (for text export and accessibility).
        ch: char,
        x: f32,
        y: f32,
        /// Font size in pixels (one em).
        size: f32,
        color: Color,
    },
    /// A filled rectangle (fraction bars, radical overbars).
    Rule { rect: Rect, color: Color },
    /// A vector path, filled or stroked.
    Path {
        cmds: Vec<PathCmd>,
        color: Color,
        /// Stroke width; `None` fills the path.
        stroke: Option<f32>,
    },
    /// A rectangle outline: the dotted box of an empty slot.
    Outline {
        rect: Rect,
        color: Color,
        dotted: bool,
    },
}

/// Index path from a statement line down to a node.
///
/// `[line, slot, child, child, …]`: `line` is the statement line, `slot` picks the
/// part of the statement (see [`crate::node_at_path`]), and each further index is
/// a position in [`mpp_syntax::math::visit::children`] order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodePath(pub Vec<u16>);

impl NodePath {
    /// The path extended by one child index.
    pub fn child(&self, index: u16) -> NodePath {
        let mut v = self.0.clone();
        v.push(index);
        NodePath(v)
    }
}

/// What a hit box stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitKind {
    /// A node of the source tree.
    Node,
    /// An empty slot (an error placeholder or a template box).
    Slot,
    /// A filled-in or automatic result; it has no source text.
    Result,
}

/// A box the IDE can click on.
#[derive(Debug, Clone, PartialEq)]
pub struct HitBox {
    pub rect: Rect,
    /// Baseline of the box, in the same coordinates as `rect`.
    pub baseline: f32,
    pub span: Span,
    pub path: NodePath,
    pub kind: HitKind,
    /// Nesting level: deeper boxes win a hit test.
    pub level: u16,
    /// The label of an empty slot.
    pub label: Option<String>,
}

/// A place the caret can stand.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaretStop {
    pub x: f32,
    pub top: f32,
    pub bottom: f32,
    /// Byte offset in the source the caret corresponds to.
    pub offset: usize,
}

/// A laid-out formula.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DisplayList {
    pub width: f32,
    /// Extent above the baseline.
    pub height: f32,
    /// Extent below the baseline.
    pub depth: f32,
    /// The baseline's y (equal to `height`).
    pub baseline: f32,
    /// Pixels per em at the base size.
    pub font_size: f32,
    pub items: Vec<Primitive>,
    pub hits: Vec<HitBox>,
}

impl DisplayList {
    /// Total height including depth.
    pub fn total_height(&self) -> f32 {
        self.height + self.depth
    }

    /// The innermost box under a point.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<&HitBox> {
        self.hits
            .iter()
            .filter(|h| h.rect.contains(x, y))
            .max_by(|a, b| {
                a.level.cmp(&b.level).then(
                    b.rect
                        .area()
                        .partial_cmp(&a.rect.area())
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
            })
    }

    /// Every caret position: both sides of each innermost node, the inside of
    /// each empty slot. Sorted by offset, then by x.
    pub fn caret_stops(&self) -> Vec<CaretStop> {
        let mut out = vec![];
        for (i, h) in self.hits.iter().enumerate() {
            if h.kind == HitKind::Result {
                continue;
            }
            let top = h.rect.y;
            let bottom = h.rect.y + h.rect.h;
            if h.kind == HitKind::Slot {
                out.push(CaretStop {
                    x: h.rect.x + h.rect.w / 2.0,
                    top,
                    bottom,
                    offset: h.span.start,
                });
                continue;
            }
            let has_child = self.hits.iter().enumerate().any(|(j, o)| {
                j != i
                    && o.level > h.level
                    && o.path.0.starts_with(&h.path.0)
                    && o.kind != HitKind::Result
            });
            if has_child {
                continue;
            }
            out.push(CaretStop {
                x: h.rect.x,
                top,
                bottom,
                offset: h.span.start,
            });
            out.push(CaretStop {
                x: h.rect.x + h.rect.w,
                top,
                bottom,
                offset: h.span.end,
            });
        }
        out.sort_by(|a, b| {
            a.offset
                .cmp(&b.offset)
                .then(a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal))
        });
        out.dedup_by(|a, b| a.offset == b.offset && (a.x - b.x).abs() < 0.01);
        out
    }

    /// The caret position for a source offset: the nearest stop at or before it.
    pub fn caret_at(&self, offset: usize) -> Option<CaretStop> {
        let stops = self.caret_stops();
        stops
            .iter()
            .rev()
            .find(|s| s.offset <= offset)
            .or_else(|| stops.first())
            .copied()
    }

    /// The slot boxes (empty template boxes) in source order, which is the
    /// Tab order of a template (bounds, then body, then variable).
    pub fn slots(&self) -> Vec<&HitBox> {
        let mut v: Vec<&HitBox> = self
            .hits
            .iter()
            .filter(|h| h.kind == HitKind::Slot)
            .collect();
        v.sort_by(|a, b| {
            a.span.start.cmp(&b.span.start).then(
                a.rect
                    .x
                    .partial_cmp(&b.rect.x)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
        });
        v
    }

    /// The text the formula shows, glyph by glyph (rough reading order).
    pub fn text(&self) -> String {
        self.items
            .iter()
            .filter_map(|p| match p {
                Primitive::Glyph { ch, .. } => Some(*ch),
                _ => None,
            })
            .collect()
    }
}
