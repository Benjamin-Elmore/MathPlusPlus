//! What a figure is made of, before any sampling: expressions and ranges.

use std::collections::BTreeMap;

use mpp_core::Expr;

use crate::sample::P;

/// A legend entry: plain text for fallbacks and LaTeX for the maths font.
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub plain: String,
    pub latex: String,
}

impl Label {
    /// A label from an expression.
    pub fn of(e: &Expr) -> Label {
        Label {
            plain: e.to_string(),
            latex: e.to_latex(),
        }
    }

    /// A label from plain text that is also valid LaTeX (names, numbers).
    pub fn text(s: &str) -> Label {
        Label {
            plain: s.to_string(),
            latex: s.to_string(),
        }
    }
}

/// One thing to draw. Expressions here already have user definitions inlined (see
/// [`crate::compile::Defs`]); free symbols are plot variables or slider names.
#[derive(Clone, Debug, PartialEq)]
pub enum Geometry {
    /// `y = f(var)`; `dep` names the vertical axis.
    Explicit { y: Expr, var: String, dep: String },
    /// `x = g(var)`, a curve over the vertical axis.
    ExplicitX { x: Expr, var: String },
    /// `(x(t), y(t))`.
    Parametric { x: Expr, y: Expr, var: String },
    /// `r(θ)`.
    Polar { r: Expr, var: String },
    /// `F(x, y) = 0`.
    Implicit { f: Expr, vars: [String; 2] },
    /// The set where `cond` holds. `boundary` lists `(lhs − rhs, strict)` for each
    /// inequality, so the edge is drawn solid or dashed.
    Region {
        cond: Expr,
        boundary: Vec<(Expr, bool)>,
        vars: [String; 2],
    },
    /// Level curves of `f`, `levels` of them.
    Contour {
        f: Expr,
        vars: [String; 2],
        levels: usize,
    },
    /// Marked points.
    Points(Vec<(Expr, Expr)>),
    /// Data points (scatter plot), already numbers.
    Scatter(Vec<P>),
    /// A histogram: bin edges (one more than counts) and heights.
    Histogram { edges: Vec<f64>, counts: Vec<f64> },
    /// `z = f(x, y)`.
    Surface { z: Expr, vars: [String; 2] },
    /// `(x(u, v), y(u, v), z(u, v))`.
    ParamSurface {
        x: Expr,
        y: Expr,
        z: Expr,
        vars: [String; 2],
    },
    /// `(x(t), y(t), z(t))`.
    Curve3 {
        x: Expr,
        y: Expr,
        z: Expr,
        var: String,
    },
}

impl Geometry {
    /// True for the 3D kinds.
    pub fn is_3d(&self) -> bool {
        matches!(
            self,
            Geometry::Surface { .. } | Geometry::ParamSurface { .. } | Geometry::Curve3 { .. }
        )
    }

    /// The expressions the geometry evaluates.
    pub fn exprs(&self) -> Vec<&Expr> {
        match self {
            Geometry::Explicit { y, .. } => vec![y],
            Geometry::ExplicitX { x, .. } => vec![x],
            Geometry::Parametric { x, y, .. } => vec![x, y],
            Geometry::Polar { r, .. } => vec![r],
            Geometry::Implicit { f, .. } | Geometry::Contour { f, .. } => vec![f],
            Geometry::Region { cond, .. } => vec![cond],
            Geometry::Points(pts) => pts.iter().flat_map(|(a, b)| [a, b]).collect(),
            Geometry::Scatter(_) | Geometry::Histogram { .. } => Vec::new(),
            Geometry::Surface { z, .. } => vec![z],
            Geometry::ParamSurface { x, y, z, .. } | Geometry::Curve3 { x, y, z, .. } => {
                vec![x, y, z]
            }
        }
    }
}

/// A geometry with its legend entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub geometry: Geometry,
    pub label: Option<Label>,
}

impl Layer {
    pub fn new(geometry: Geometry) -> Layer {
        Layer {
            geometry,
            label: None,
        }
    }

    pub fn labelled(geometry: Geometry, label: Label) -> Layer {
        Layer {
            geometry,
            label: Some(label),
        }
    }
}

/// A variable the reader can drag.
#[derive(Clone, Debug, PartialEq)]
pub struct Slider {
    pub name: String,
    pub min: f64,
    pub max: f64,
    pub value: f64,
}

impl Slider {
    /// A sensible drag step: a hundredth of the range, on a 1-2-5 grid.
    pub fn step(&self) -> f64 {
        crate::ticks::nice_step(self.max - self.min, 100)
    }
}

/// Everything needed to build a [`crate::Figure`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FigureSpec {
    pub layers: Vec<Layer>,
    /// Plot ranges by variable name, as expressions (they may use sliders and constants).
    pub ranges: BTreeMap<String, (Expr, Expr)>,
    pub sliders: Vec<Slider>,
    /// The caption as plain text.
    pub caption: Option<String>,
    /// Things the figure could not draw, for the author.
    pub warnings: Vec<String>,
}

impl FigureSpec {
    /// A range given by two numbers.
    pub fn set_range(&mut self, name: &str, lo: f64, hi: f64) {
        self.ranges
            .insert(name.to_string(), (Expr::float(lo), Expr::float(hi)));
    }
}
