//! The TeX-style box model: boxes with width, height and depth, nested by
//! explicit offsets. All lengths are in em at the base font size; y offsets grow
//! downwards (a positive `y` lowers a child below its parent's baseline).

use mpp_syntax::Span;

use crate::display::{Color, DisplayList, HitBox, HitKind, NodePath, Primitive, Rect};
use crate::font::FontId;

/// A glyph inside a box.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GlyphBox {
    pub font: FontId,
    pub glyph: u16,
    pub ch: char,
    /// Size in em of the base font size (1 in text style, ~0.7 in scripts).
    pub size: f32,
    pub color: Color,
}

/// Source information attached to a box, turned into a [`HitBox`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Hit {
    pub span: Span,
    pub path: NodePath,
    pub kind: HitKind,
    pub label: Option<String>,
}

/// What a box draws.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Content {
    /// Nothing (a kern or strut).
    Empty,
    Glyph(GlyphBox),
    /// Fills the whole box.
    Rule(Color),
    /// A dotted outline around the whole box.
    Slot(Color),
    /// Children at offsets from this box's origin.
    List(Vec<Placed>),
}

/// A child box at an offset.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Placed {
    pub x: f32,
    pub y: f32,
    pub b: LBox,
}

/// A box.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LBox {
    pub width: f32,
    pub height: f32,
    pub depth: f32,
    /// Italic correction of the box's last glyph, for placing superscripts.
    pub italic: f32,
    pub content: Content,
    pub hit: Option<Hit>,
}

impl LBox {
    /// An empty box of the given size.
    pub fn strut(width: f32, height: f32, depth: f32) -> LBox {
        LBox {
            width,
            height,
            depth,
            italic: 0.0,
            content: Content::Empty,
            hit: None,
        }
    }

    /// Horizontal space.
    pub fn kern(width: f32) -> LBox {
        LBox::strut(width, 0.0, 0.0)
    }

    /// A box with nothing in it.
    pub fn empty() -> LBox {
        LBox::kern(0.0)
    }

    /// A filled rectangle.
    pub fn rule(width: f32, height: f32, depth: f32, color: Color) -> LBox {
        LBox {
            content: Content::Rule(color),
            ..LBox::strut(width, height, depth)
        }
    }

    /// A box holding children at explicit offsets.
    pub fn with_children(width: f32, height: f32, depth: f32, children: Vec<Placed>) -> LBox {
        LBox {
            content: Content::List(children),
            ..LBox::strut(width, height, depth)
        }
    }

    /// Boxes side by side on a common baseline.
    pub fn hlist(items: Vec<LBox>) -> LBox {
        if items.len() == 1 {
            if let Some(only) = items.into_iter().next() {
                return only;
            }
            return LBox::empty();
        }
        let mut x = 0.0;
        let mut height: f32 = 0.0;
        let mut depth: f32 = 0.0;
        let mut italic = 0.0;
        let mut children = Vec::with_capacity(items.len());
        for b in items {
            height = height.max(b.height);
            depth = depth.max(b.depth);
            let w = b.width;
            italic = if w > 0.0 || !matches!(b.content, Content::Empty) {
                b.italic
            } else {
                italic
            };
            children.push(Placed { x, y: 0.0, b });
            x += w;
        }
        LBox {
            italic,
            ..LBox::with_children(x, height, depth, children)
        }
    }

    /// The box moved up by `amount` (down when negative).
    pub fn raised(self, amount: f32) -> LBox {
        if amount == 0.0 {
            return self;
        }
        let (w, h, d, italic) = (
            self.width,
            self.height + amount,
            self.depth - amount,
            self.italic,
        );
        LBox {
            italic,
            ..LBox::with_children(
                w,
                h.max(0.0),
                d.max(0.0),
                vec![Placed {
                    x: 0.0,
                    y: -amount,
                    b: self,
                }],
            )
        }
    }

    /// The box with source information attached.
    pub fn with_hit(mut self, hit: Hit) -> LBox {
        if self.hit.is_some() {
            // Keep the inner hit and wrap so both survive.
            let (w, h, d, italic) = (self.width, self.height, self.depth, self.italic);
            let mut outer = LBox::with_children(
                w,
                h,
                d,
                vec![Placed {
                    x: 0.0,
                    y: 0.0,
                    b: self,
                }],
            );
            outer.italic = italic;
            outer.hit = Some(hit);
            return outer;
        }
        self.hit = Some(hit);
        self
    }

    /// The glyph when the box is exactly one glyph (possibly wrapped).
    pub fn single_glyph(&self) -> Option<&GlyphBox> {
        match &self.content {
            Content::Glyph(g) => Some(g),
            Content::List(children) if children.len() == 1 => {
                let c = &children[0];
                if c.x == 0.0 && c.y == 0.0 {
                    c.b.single_glyph()
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Total vertical extent.
    pub fn total(&self) -> f32 {
        self.height + self.depth
    }

    /// Draws the box with its origin (baseline left end) at `x`, `y` (em).
    pub fn emit(&self, x: f32, y: f32, px: f32, level: u16, out: &mut DisplayList) {
        let mut level = level;
        if let Some(hit) = &self.hit {
            out.hits.push(HitBox {
                rect: Rect {
                    x: x * px,
                    y: (y - self.height) * px,
                    w: self.width * px,
                    h: self.total() * px,
                },
                baseline: y * px,
                span: hit.span,
                path: hit.path.clone(),
                kind: hit.kind,
                level,
                label: hit.label.clone(),
            });
            level = level.saturating_add(1);
        }
        match &self.content {
            Content::Empty => {}
            Content::Glyph(g) => out.items.push(Primitive::Glyph {
                font: g.font,
                glyph: g.glyph,
                ch: g.ch,
                x: x * px,
                y: y * px,
                size: g.size * px,
                color: g.color,
            }),
            Content::Rule(color) => out.items.push(Primitive::Rule {
                rect: Rect {
                    x: x * px,
                    y: (y - self.height) * px,
                    w: self.width * px,
                    h: self.total() * px,
                },
                color: *color,
            }),
            Content::Slot(color) => out.items.push(Primitive::Outline {
                rect: Rect {
                    x: x * px,
                    y: (y - self.height) * px,
                    w: self.width * px,
                    h: self.total() * px,
                },
                color: *color,
                dotted: true,
            }),
            Content::List(children) => {
                for c in children {
                    c.b.emit(x + c.x, y + c.y, px, level, out);
                }
            }
        }
    }
}

/// Lays boxes out top to bottom, centred on a common axis, with each box's
/// baseline at the given offset (positive is down) from the result's baseline.
pub(crate) fn vstack_centered(items: Vec<(LBox, f32)>) -> LBox {
    let width = items.iter().map(|(b, _)| b.width).fold(0.0, f32::max);
    let mut height: f32 = 0.0;
    let mut depth: f32 = 0.0;
    let mut children = vec![];
    for (b, y) in items {
        height = height.max(b.height - y);
        depth = depth.max(b.depth + y);
        children.push(Placed {
            x: (width - b.width) / 2.0,
            y,
            b,
        });
    }
    LBox::with_children(width, height.max(0.0), depth.max(0.0), children)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hlist_adds_widths_and_takes_max_extents() {
        let b = LBox::hlist(vec![LBox::strut(1.0, 0.5, 0.1), LBox::strut(2.0, 0.7, 0.0)]);
        assert_eq!(b.width, 3.0);
        assert_eq!(b.height, 0.7);
        assert_eq!(b.depth, 0.1);
    }

    #[test]
    fn raised_moves_extents() {
        let b = LBox::strut(1.0, 0.5, 0.2).raised(0.3);
        assert!((b.height - 0.8).abs() < 1e-6);
        assert_eq!(b.depth, 0.0);
    }
}
