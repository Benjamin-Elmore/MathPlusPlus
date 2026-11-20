//! The display list a figure draws as: pixel-space primitives with no GUI attached.

use crate::color::Color;

/// A point in pixels, origin at the top left, y down.
pub type Pt = [f32; 2];

/// A pixel rectangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn contains(&self, p: Pt) -> bool {
        p[0] >= self.x && p[0] <= self.right() && p[1] >= self.y && p[1] <= self.bottom()
    }
}

/// How a line is dashed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dash {
    Solid,
    /// Dash and gap lengths in pixels.
    Dashed(f32, f32),
}

/// A connected line.
#[derive(Clone, Debug, PartialEq)]
pub struct Polyline {
    pub points: Vec<Pt>,
    pub color: Color,
    pub width: f32,
    pub dash: Dash,
}

/// A filled polygon, optionally outlined.
#[derive(Clone, Debug, PartialEq)]
pub struct Polygon {
    pub points: Vec<Pt>,
    pub fill: Option<Color>,
    pub stroke: Option<(Color, f32)>,
}

/// A triangle with a colour at each corner (flat-shaded as the average in SVG).
#[derive(Clone, Debug, PartialEq)]
pub struct Triangle {
    pub points: [Pt; 3],
    pub colors: [Color; 3],
    /// Mesh edge colour and width for a wireframe.
    pub edge: Option<(Color, f32)>,
}

impl Triangle {
    /// The average of the three corner colours.
    pub fn flat_color(&self) -> Color {
        let [a, b, c] = self.colors;
        let avg = |f: fn(&Color) -> u8| {
            ((u16::from(f(&a)) + u16::from(f(&b)) + u16::from(f(&c))) / 3) as u8
        };
        Color {
            r: avg(|c| c.r),
            g: avg(|c| c.g),
            b: avg(|c| c.b),
            a: avg(|c| c.a),
        }
    }
}

/// The shape of a marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerKind {
    Circle,
    Square,
    Diamond,
    Cross,
}

/// A point marker. A hollow `Circle` is the open dot at a jump; a filled one is closed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Marker {
    pub pos: Pt,
    pub kind: MarkerKind,
    pub color: Color,
    /// Diameter in pixels.
    pub size: f32,
    /// False draws an outline filled with `hollow_fill` (an open dot).
    pub filled: bool,
    /// What an open marker is filled with (the background).
    pub hollow_fill: Color,
}

/// Where a label sits relative to its position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Start,
    Middle,
    End,
}

/// A text label.
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    /// The position of the text baseline at its anchor.
    pub pos: Pt,
    /// Plain Unicode text, always present (used for SVG and as a fallback).
    pub text: String,
    /// When set this is a maths label: the IDE typesets this LaTeX with the maths font
    /// instead of drawing `text`.
    pub latex: Option<String>,
    pub anchor: Anchor,
    /// Font size in pixels.
    pub size: f32,
    pub color: Color,
    /// Rotation about `pos` in degrees (counter-clockwise).
    pub rotation: f32,
    /// Draw maths symbols in the italic maths style.
    pub italic: bool,
}

impl Label {
    /// True when this label should be typeset from its LaTeX.
    pub fn is_math(&self) -> bool {
        self.latex.is_some()
    }
}

/// One drawing command.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Polyline(Polyline),
    Polygon(Polygon),
    Triangle(Triangle),
    Marker(Marker),
    Text(Label),
    /// Everything up to the matching [`Item::ClipEnd`] is clipped to this rectangle.
    ClipStart(Rect),
    ClipEnd,
}

/// A whole figure for one viewport.
#[derive(Clone, Debug, PartialEq)]
pub struct PlotScene {
    pub width: f32,
    pub height: f32,
    pub background: Color,
    pub items: Vec<Item>,
    /// The font families the theme asked for.
    pub font: String,
    pub math_font: String,
}

impl PlotScene {
    pub fn new(width: f32, height: f32, background: Color, font: &str, math_font: &str) -> Self {
        PlotScene {
            width,
            height,
            background,
            items: Vec::new(),
            font: font.to_string(),
            math_font: math_font.to_string(),
        }
    }

    pub fn push(&mut self, item: Item) {
        self.items.push(item);
    }

    /// Adds a solid line.
    pub fn line(&mut self, points: Vec<Pt>, color: Color, width: f32) {
        self.push(Item::Polyline(Polyline {
            points,
            color,
            width,
            dash: Dash::Solid,
        }));
    }

    /// Adds a text label.
    pub fn text(&mut self, label: Label) {
        self.push(Item::Text(label));
    }

    /// Counts items of each kind, for tests: (polylines, polygons, triangles, markers, labels).
    pub fn counts(&self) -> (usize, usize, usize, usize, usize) {
        let mut c = (0, 0, 0, 0, 0);
        for i in &self.items {
            match i {
                Item::Polyline(_) => c.0 += 1,
                Item::Polygon(_) => c.1 += 1,
                Item::Triangle(_) => c.2 += 1,
                Item::Marker(_) => c.3 += 1,
                Item::Text(_) => c.4 += 1,
                Item::ClipStart(_) | Item::ClipEnd => {}
            }
        }
        c
    }

    /// Every label in the scene.
    pub fn labels(&self) -> impl Iterator<Item = &Label> {
        self.items.iter().filter_map(|i| match i {
            Item::Text(l) => Some(l),
            _ => None,
        })
    }
}
