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
            latex: if s.contains(' ') {
                format!("{}text{{{s}}}", '\x5c')
            } else {
                s.to_string()
            },
        }
    }
}

/// The rule a Riemann sum uses to pick the height of each strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiemannRule {
    Left,
    Right,
    Mid,
    /// Trapezoids: the average of the left and right heights, drawn as slanted tops.
    Trapezoid,
}

impl RiemannRule {
    /// The word used in the figure keyword and in read-outs.
    pub fn name(self) -> &'static str {
        match self {
            RiemannRule::Left => "left",
            RiemannRule::Right => "right",
            RiemannRule::Mid => "midpoint",
            RiemannRule::Trapezoid => "trapezoid",
        }
    }

    /// The rule as a figure's read-out names it ("Midpoint rule").
    pub fn readout_name(self) -> &'static str {
        match self {
            RiemannRule::Left => "Left Riemann sum",
            RiemannRule::Right => "Right Riemann sum",
            RiemannRule::Mid => "Midpoint rule",
            RiemannRule::Trapezoid => "Trapezoid rule",
        }
    }
}

/// The line a region is turned about.
#[derive(Clone, Debug, PartialEq)]
pub enum AxisLine {
    /// `y = c`: strips along the axis make disks or washers.
    Horizontal(Expr),
    /// `x = c`: strips across the axis make cylindrical shells.
    Vertical(Expr),
}

/// The five-number summary of one box in a box plot.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxStats {
    pub label: String,
    pub low: f64,
    pub q1: f64,
    pub median: f64,
    pub q3: f64,
    pub high: f64,
    pub outliers: Vec<f64>,
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
    /// The tangent line to `y = f(var)` at `var = at`.
    Tangent { f: Expr, var: String, at: Expr },
    /// Riemann strips under `y = f(var)`; `n` is the number of strips.
    Riemann {
        f: Expr,
        var: String,
        rule: RiemannRule,
        n: Expr,
        bounds: Option<(Expr, Expr)>,
    },
    /// The region between `f` and `g` (the axis when `g` is `None`), split where the
    /// curves cross. Without bounds it is the region the curves enclose.
    AreaBetween {
        f: Expr,
        g: Option<Expr>,
        var: String,
        bounds: Option<(Expr, Expr)>,
    },
    /// The solid made by turning the region between `f` and `g` about a line.
    Revolve {
        f: Expr,
        g: Option<Expr>,
        var: String,
        bounds: Option<(Expr, Expr)>,
        axis: AxisLine,
    },
    /// Direction field of `y' = dydx(x, y)` with solution curves through `starts`.
    SlopeField {
        dydx: Expr,
        vars: [String; 2],
        starts: Vec<(Expr, Expr)>,
    },
    /// Phase portrait of `x' = dx, y' = dy`, with trajectories through `starts`.
    PhasePortrait {
        dx: Expr,
        dy: Expr,
        vars: [String; 2],
        starts: Vec<(Expr, Expr)>,
    },
    /// Arrows of the vector field `(fx, fy)`.
    VectorField {
        fx: Expr,
        fy: Expr,
        vars: [String; 2],
    },
    /// Arrows of the gradient of `f`.
    GradientField { f: Expr, vars: [String; 2] },
    /// The plane touching `z = f(x, y)` at `at`.
    TangentPlane {
        z: Expr,
        vars: [String; 2],
        at: (Expr, Expr),
    },
    /// The Taylor polynomial of `f` of the given degree about `center`.
    Taylor {
        f: Expr,
        var: String,
        center: Expr,
        degree: Expr,
    },
    /// Newton's method on `f` from `x0`, drawn for `steps` iterations.
    Newton {
        f: Expr,
        var: String,
        x0: Expr,
        steps: Expr,
    },
    /// Box plots, already summarised.
    BoxPlot(Vec<BoxStats>),
}

impl Geometry {
    /// True for the 3D kinds.
    pub fn is_3d(&self) -> bool {
        matches!(
            self,
            Geometry::Surface { .. }
                | Geometry::ParamSurface { .. }
                | Geometry::Curve3 { .. }
                | Geometry::Revolve { .. }
                | Geometry::TangentPlane { .. }
        )
    }

    /// True for the visual calculus helpers, which draw over another layer.
    pub fn is_helper(&self) -> bool {
        matches!(
            self,
            Geometry::Tangent { .. }
                | Geometry::Riemann { .. }
                | Geometry::AreaBetween { .. }
                | Geometry::Revolve { .. }
                | Geometry::SlopeField { .. }
                | Geometry::PhasePortrait { .. }
                | Geometry::VectorField { .. }
                | Geometry::GradientField { .. }
                | Geometry::TangentPlane { .. }
                | Geometry::Taylor { .. }
                | Geometry::Newton { .. }
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
            Geometry::Scatter(_) | Geometry::Histogram { .. } | Geometry::BoxPlot(_) => Vec::new(),
            Geometry::Tangent { f, at, .. } => vec![f, at],
            Geometry::Riemann { f, n, bounds, .. } => {
                let mut v = vec![f, n];
                if let Some((a, b)) = bounds {
                    v.extend([a, b]);
                }
                v
            }
            Geometry::AreaBetween { f, g, bounds, .. } | Geometry::Revolve { f, g, bounds, .. } => {
                let mut v = vec![f];
                v.extend(g.iter());
                if let Some((a, b)) = bounds {
                    v.extend([a, b]);
                }
                if let Geometry::Revolve { axis, .. } = self {
                    match axis {
                        AxisLine::Horizontal(c) | AxisLine::Vertical(c) => v.push(c),
                    }
                }
                v
            }
            Geometry::SlopeField { dydx, starts, .. } => {
                let mut v = vec![dydx];
                v.extend(starts.iter().flat_map(|(a, b)| [a, b]));
                v
            }
            Geometry::PhasePortrait { dx, dy, starts, .. } => {
                let mut v = vec![dx, dy];
                v.extend(starts.iter().flat_map(|(a, b)| [a, b]));
                v
            }
            Geometry::VectorField { fx, fy, .. } => vec![fx, fy],
            Geometry::GradientField { f, .. } => vec![f],
            Geometry::TangentPlane { z, at, .. } => vec![z, &at.0, &at.1],
            Geometry::Taylor {
                f, center, degree, ..
            } => vec![f, center, degree],
            Geometry::Newton { f, x0, steps, .. } => vec![f, x0, steps],
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
    /// Only whole numbers make sense (a number of strips, a degree).
    pub integer: bool,
}

impl Slider {
    /// A sensible drag step: a hundredth of the range, on a 1-2-5 grid; 1 for a count.
    pub fn step(&self) -> f64 {
        if self.integer {
            return 1.0;
        }
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
    /// Names of the horizontal and vertical axes when the data gives them (`t`, `x`).
    pub axis_names: Option<(String, String)>,
}

impl FigureSpec {
    /// A range given by two numbers.
    pub fn set_range(&mut self, name: &str, lo: f64, hi: f64) {
        self.ranges
            .insert(name.to_string(), (Expr::float(lo), Expr::float(hi)));
    }
}
