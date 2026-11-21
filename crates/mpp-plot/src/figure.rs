//! A figure: its specification plus the sampled data for the current slider values.

use std::collections::BTreeMap;

use mpp_core::Expr;
use std::f64::consts::TAU;

use crate::calculus::{data_x_range, Notes};
use crate::compile::{eval1, eval2, eval_const, eval_real, eval_truth, Frame};
use crate::contour::{contour_lines, Bounds, Grid, RegionGrid};
use crate::error::PlotError;
use crate::helpers::{Bar, FieldItem, FieldKind, Solid};
use crate::sample::{robust_range, sample_curve, SampleOpts, Sampled, P};
use crate::spec::{BoxStats, FigureSpec, Geometry, Label, Slider};

/// A grid of 3D points, `points[v * nu + u]`. Undefined points are NaN.
#[derive(Clone, Debug, PartialEq)]
pub struct Mesh {
    pub nu: usize,
    pub nv: usize,
    pub points: Vec<[f64; 3]>,
}

impl Mesh {
    pub fn at(&self, u: usize, v: usize) -> [f64; 3] {
        self.points[v * self.nu + u]
    }
}

/// The sampled form of one layer.
#[derive(Clone, Debug, PartialEq)]
pub enum LayerData {
    /// Nothing to draw (the layer failed or is empty).
    Empty,
    /// A curve, in runs with gaps between.
    Curve(Sampled),
    /// Implicit curves as polylines.
    Lines(Vec<Vec<P>>),
    /// Level curves with their levels.
    Contours(Vec<(f64, Vec<Vec<P>>)>),
    /// A shaded region with its edges (and whether each edge is strict).
    Region {
        grid: RegionGrid,
        edges: Vec<(Vec<Vec<P>>, bool)>,
    },
    /// Isolated points.
    Points(Vec<P>),
    /// Histogram bars.
    Histogram { edges: Vec<f64>, counts: Vec<f64> },
    /// A 3D surface.
    Surface(Mesh),
    /// 3D curves in runs.
    Curve3(Vec<Vec<[f64; 3]>>),
    /// The tangent line through `point` with this slope, drawn across the window.
    Tangent { point: P, slope: f64 },
    /// Riemann strips.
    Bars(Vec<Bar>),
    /// Closed shaded outlines (the area between curves).
    Shaded(Vec<Vec<P>>),
    /// Dashed polylines (guide lines).
    DashedLines(Vec<Vec<P>>),
    /// A direction or vector field with solution curves. `cell` is the data spacing of
    /// the grid; `directed` curves get arrow heads.
    Field {
        items: Vec<FieldItem>,
        cell: P,
        kind: FieldKind,
        curves: Vec<Vec<P>>,
        directed: bool,
    },
    /// Text at points: position, LaTeX and plain text.
    Labels(Vec<(P, String, String)>),
    /// Box plots.
    BoxPlot(Vec<BoxStats>),
    /// Several pieces drawn in the layer's colour.
    Composite(Vec<LayerData>),
    /// A solid of revolution, or a plane on a surface.
    Solid(Solid),
}

/// How finely things are sampled.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quality {
    pub curve: SampleOpts,
    /// Cells per side of the grid behind implicit curves and contours.
    pub implicit_cells: usize,
    /// Cells per side of the grid behind shaded regions.
    pub region_cells: usize,
    /// Cells per side of 3D surfaces.
    pub surface_cells: usize,
    /// Samples along 3D curves.
    pub curve3_samples: usize,
}

impl Default for Quality {
    fn default() -> Self {
        Quality {
            curve: SampleOpts::default(),
            implicit_cells: 120,
            region_cells: 160,
            surface_cells: 60,
            curve3_samples: 480,
        }
    }
}

/// A figure ready to be drawn at any size.
#[derive(Clone, Debug)]
pub struct Figure {
    pub(crate) spec: FigureSpec,
    pub(crate) values: BTreeMap<String, f64>,
    pub(crate) data: Vec<LayerData>,
    pub(crate) window: Bounds,
    pub(crate) equal_aspect: bool,
    pub(crate) box3: [(f64, f64); 3],
    view: Option<Bounds>,
    pub(crate) quality: Quality,
    messages: Vec<String>,
    readouts: Vec<String>,
    /// Derivative chains of helper layers (`f`, `f'`, `f''` …), kept between slider moves.
    pub(crate) derivs: BTreeMap<usize, Vec<Expr>>,
}

const DEFAULT_RANGE: (f64, f64) = (-10.0, 10.0);

fn pad(lo: f64, hi: f64, frac: f64) -> (f64, f64) {
    if !(lo.is_finite() && hi.is_finite()) {
        return DEFAULT_RANGE;
    }
    if hi - lo <= f64::EPSILON * lo.abs().max(hi.abs()).max(1e-300) {
        let d = if lo == 0.0 { 1.0 } else { lo.abs() * 0.1 };
        return (lo - d, hi + d);
    }
    let d = (hi - lo) * frac;
    (lo - d, hi + d)
}

fn union(a: Option<(f64, f64)>, b: Option<(f64, f64)>) -> Option<(f64, f64)> {
    match (a, b) {
        (Some(a), Some(b)) => Some((a.0.min(b.0), a.1.max(b.1))),
        (a, b) => a.or(b),
    }
}

fn extent<'a>(points: impl Iterator<Item = &'a P>) -> Option<(f64, f64, f64, f64)> {
    points.fold(None, |acc, p| match acc {
        None => Some((p[0], p[0], p[1], p[1])),
        Some((a, b, c, d)) => Some((a.min(p[0]), b.max(p[0]), c.min(p[1]), d.max(p[1]))),
    })
}

impl Figure {
    /// Builds a figure from a specification, sampling it at the sliders' starting values.
    pub fn new(spec: FigureSpec) -> Result<Figure, PlotError> {
        if spec.layers.is_empty() {
            return Err(PlotError::Empty);
        }
        let values = spec
            .sliders
            .iter()
            .map(|s| (s.name.clone(), s.value))
            .collect();
        let mut fig = Figure {
            spec,
            values,
            data: Vec::new(),
            window: Bounds::new(-1.0, 1.0, -1.0, 1.0),
            equal_aspect: false,
            box3: [(-1.0, 1.0); 3],
            view: None,
            quality: Quality::default(),
            messages: Vec::new(),
            readouts: Vec::new(),
            derivs: BTreeMap::new(),
        };
        fig.build();
        Ok(fig)
    }

    /// Changes the sampling quality and re-samples.
    pub fn set_quality(&mut self, quality: Quality) {
        self.quality = quality;
        self.build();
    }

    /// The sliders this figure responds to.
    pub fn sliders(&self) -> Vec<Slider> {
        self.spec
            .sliders
            .iter()
            .map(|s| Slider {
                value: self.values.get(&s.name).copied().unwrap_or(s.value),
                ..s.clone()
            })
            .collect()
    }

    /// Sets slider values (by name; unknown names are ignored) and re-samples quickly.
    pub fn rebuild(&mut self, values: &BTreeMap<String, f64>) {
        for s in &self.spec.sliders {
            if let Some(v) = values.get(&s.name).filter(|v| v.is_finite()) {
                let v = if s.integer { v.round() } else { *v };
                self.values
                    .insert(s.name.clone(), v.clamp(s.min.min(s.max), s.max.max(s.min)));
            }
        }
        self.build();
    }

    /// Re-samples with the current values.
    pub fn rebuild_current(&mut self) {
        self.build();
    }

    /// Looks at another part of a 2D plot (pan and zoom), or back at the automatic
    /// window with `None`. The curves are re-sampled over the new window.
    pub fn set_view(&mut self, view: Option<Bounds>) {
        self.view = view.filter(Bounds::is_valid);
        self.build();
    }

    /// The 2D window used by the last build.
    pub fn window(&self) -> Bounds {
        self.window
    }

    /// True for a figure drawn with the 3D camera.
    pub fn is_3d(&self) -> bool {
        self.spec.layers.iter().any(|l| l.geometry.is_3d())
    }

    /// The specification.
    pub fn spec(&self) -> &FigureSpec {
        &self.spec
    }

    /// The caption, when the document gave one.
    pub fn caption(&self) -> Option<&str> {
        self.spec.caption.as_deref()
    }

    /// Notes for the author: unsupported items, layers that came out empty.
    pub fn warnings(&self) -> impl Iterator<Item = &str> {
        self.spec
            .warnings
            .iter()
            .chain(self.messages.iter())
            .map(String::as_str)
    }

    /// The sampled layers, in the order of the specification.
    pub fn layers(&self) -> &[LayerData] {
        &self.data
    }

    /// Short facts the figure computed for its helpers (a Riemann sum, an area, a volume,
    /// the tangent line), for a note under the graph. The 2D and 3D scenes draw them too.
    pub fn readouts(&self) -> &[String] {
        &self.readouts
    }

    /// Legend entries: label and the layer index that gives the colour.
    pub fn legend(&self) -> Vec<(&Label, usize)> {
        self.spec
            .layers
            .iter()
            .enumerate()
            .filter_map(|(i, l)| l.label.as_ref().map(|lab| (lab, i)))
            .collect()
    }

    /// The slider and constant values in use.
    pub fn values(&self) -> &BTreeMap<String, f64> {
        &self.values
    }

    pub(crate) fn range(&self, name: &str) -> Option<(f64, f64)> {
        let (a, b) = self.spec.ranges.get(name)?;
        let lo = eval_const(a, &self.values).ok()?;
        let hi = eval_const(b, &self.values).ok()?;
        if !(lo.is_finite() && hi.is_finite()) || lo == hi {
            return None;
        }
        Some(if lo < hi { (lo, hi) } else { (hi, lo) })
    }

    fn build(&mut self) {
        self.messages.clear();
        self.readouts.clear();
        if self.is_3d() {
            self.build_3d();
        } else {
            self.build_2d();
        }
    }

    // ---- 2D ------------------------------------------------------------------

    fn build_2d(&mut self) {
        let spec = self.spec.clone();
        let fixed = self.values.clone();
        let q = self.quality;
        let n = spec.layers.len();
        let mut data = vec![LayerData::Empty; n];
        let mut messages = Vec::new();

        // Ranges the author gave.
        let mut x_given: Option<(f64, f64)> = None;
        let mut y_given: Option<(f64, f64)> = None;
        let mut has_explicit = false;
        for layer in &spec.layers {
            match &layer.geometry {
                Geometry::Explicit { var, dep, .. } => {
                    has_explicit = true;
                    x_given = union(x_given, self.range(var));
                    y_given = y_given.or_else(|| self.range(dep));
                }
                Geometry::ExplicitX { .. } => has_explicit = true,
                Geometry::Implicit { vars, .. }
                | Geometry::Region { vars, .. }
                | Geometry::Contour { vars, .. }
                | Geometry::SlopeField { vars, .. }
                | Geometry::PhasePortrait { vars, .. }
                | Geometry::VectorField { vars, .. }
                | Geometry::GradientField { vars, .. } => {
                    x_given = x_given.or_else(|| self.range(&vars[0]));
                    y_given = y_given.or_else(|| self.range(&vars[1]));
                }
                _ => {}
            }
        }
        x_given = x_given.or_else(|| self.range("x"));
        y_given = y_given.or_else(|| self.range("y"));
        let author_gave_range = x_given.is_some() || y_given.is_some();
        if x_given.is_none() && self.view.is_none() {
            // Nothing says where to look: a helper (a tangent, an area, …) or the data
            // itself does.
            x_given = self
                .window_hint(&spec, &fixed)
                .or_else(|| data_x_range(&spec));
        }
        if let Some(v) = self.view {
            x_given = Some((v.x0, v.x1));
            y_given = Some((v.y0, v.y1));
        }

        // Layers that choose their own extent.
        let mut free: Option<(f64, f64, f64, f64)> = None;
        let grow = |e: Option<(f64, f64, f64, f64)>, free: &mut Option<(f64, f64, f64, f64)>| {
            if let Some(e) = e {
                *free = Some(match *free {
                    None => e,
                    Some(f) => (f.0.min(e.0), f.1.max(e.1), f.2.min(e.2), f.3.max(e.3)),
                });
            }
        };
        for (i, layer) in spec.layers.iter().enumerate() {
            match &layer.geometry {
                Geometry::Parametric { x, y, var } => {
                    let (t0, t1) = self.range(var).unwrap_or((0.0, TAU));
                    let mut f =
                        |t: f64| Some([eval1(x, &fixed, var, t)?, eval1(y, &fixed, var, t)?]);
                    let s = sample_curve(&mut f, t0, t1, q.curve);
                    grow(extent(s.points()), &mut free);
                    data[i] = LayerData::Curve(s);
                }
                Geometry::Polar { r, var } => {
                    let (t0, t1) = self.range(var).unwrap_or((0.0, TAU));
                    let mut f = |t: f64| {
                        let r = eval1(r, &fixed, var, t)?;
                        Some([r * t.cos(), r * t.sin()])
                    };
                    let s = sample_curve(&mut f, t0, t1, q.curve);
                    grow(extent(s.points()), &mut free);
                    data[i] = LayerData::Curve(s);
                }
                Geometry::ExplicitX { x, var } => {
                    let (t0, t1) = self.range(var).or(y_given).unwrap_or(DEFAULT_RANGE);
                    let mut f = |t: f64| Some([eval1(x, &fixed, var, t)?, t]);
                    let s = sample_curve(&mut f, t0, t1, q.curve);
                    grow(extent(s.points()), &mut free);
                    data[i] = LayerData::Curve(s);
                }
                Geometry::Points(pts) => {
                    let ps: Vec<P> = pts
                        .iter()
                        .filter_map(|(a, b)| {
                            Some([eval_const(a, &fixed).ok()?, eval_const(b, &fixed).ok()?])
                        })
                        .filter(|p| p.iter().all(|v| v.is_finite()))
                        .collect();
                    grow(extent(ps.iter()), &mut free);
                    data[i] = LayerData::Points(ps);
                }
                Geometry::Scatter(ps) => {
                    grow(extent(ps.iter()), &mut free);
                    data[i] = LayerData::Points(ps.clone());
                }
                Geometry::BoxPlot(stats) => {
                    let lo = stats
                        .iter()
                        .flat_map(|s| s.outliers.iter().copied().chain([s.low]))
                        .fold(f64::INFINITY, f64::min);
                    let hi = stats
                        .iter()
                        .flat_map(|s| s.outliers.iter().copied().chain([s.high]))
                        .fold(f64::NEG_INFINITY, f64::max);
                    if lo.is_finite() && hi.is_finite() {
                        let k = stats.len() as f64;
                        let half = (k / 2.0 + 0.5).max(1.5);
                        grow(
                            Some(((k + 1.0) / 2.0 - half, (k + 1.0) / 2.0 + half, lo, hi)),
                            &mut free,
                        );
                    }
                    data[i] = LayerData::BoxPlot(stats.clone());
                }
                Geometry::Histogram { edges, counts } => {
                    if let (Some(&lo), Some(&hi)) = (edges.first(), edges.last()) {
                        let top = counts.iter().copied().fold(0.0, f64::max);
                        grow(Some((lo, hi, 0.0, top)), &mut free);
                    }
                    data[i] = LayerData::Histogram {
                        edges: edges.clone(),
                        counts: counts.clone(),
                    };
                }
                _ => {}
            }
        }

        // Explicit curves y = f(x).
        let mut explicit_x: Option<(f64, f64)> = None;
        for (i, layer) in spec.layers.iter().enumerate() {
            if let Geometry::Explicit { y, var, .. } = &layer.geometry {
                let (x0, x1) = self
                    .view
                    .map(|v| (v.x0, v.x1))
                    .or_else(|| self.range(var))
                    .or(x_given)
                    .unwrap_or(DEFAULT_RANGE);
                explicit_x = union(explicit_x, Some((x0, x1)));
                let mut f = |x: f64| Some([x, eval1(y, &fixed, var, x)?]);
                data[i] = LayerData::Curve(sample_curve(&mut f, x0, x1, q.curve));
            }
        }

        // Probe implicit layers for a window when nothing else says where to look.
        let needs_probe = x_given.is_none() && y_given.is_none() && free.is_none() && !has_explicit;
        if needs_probe {
            for layer in &spec.layers {
                let (f, vars) = match &layer.geometry {
                    Geometry::Implicit { f, vars } | Geometry::Contour { f, vars, .. } => (f, vars),
                    Geometry::Region { boundary, vars, .. } => match boundary.first() {
                        Some((f, _)) => (f, vars),
                        None => continue,
                    },
                    _ => continue,
                };
                grow(probe(f, vars, &fixed), &mut free);
            }
        }

        // The window.
        let y_data = {
            let mut values = Vec::new();
            for (d, l) in data.iter().zip(&spec.layers) {
                if let (LayerData::Curve(s), Geometry::Explicit { var, .. }) = (d, &l.geometry) {
                    let (x0, x1) = self
                        .view
                        .map(|v| (v.x0, v.x1))
                        .or_else(|| self.range(var))
                        .or(x_given)
                        .unwrap_or(DEFAULT_RANGE);
                    one_per_bucket(s, x0, x1, &mut values);
                }
            }
            robust_range(values.into_iter())
        };
        let x_range = x_given
            .or_else(|| {
                union(
                    explicit_x,
                    free.map(|f| {
                        if explicit_x.is_some() {
                            (f.0, f.1)
                        } else {
                            pad(f.0, f.1, 0.08)
                        }
                    }),
                )
            })
            .unwrap_or(DEFAULT_RANGE);
        let y_range = y_given
            .or_else(|| {
                let from_free = free.map(|f| (f.2, f.3));
                let combined = union(y_data, from_free);
                combined.map(|(a, b)| pad(a, b, if y_data.is_some() { 0.06 } else { 0.08 }))
            })
            .unwrap_or(DEFAULT_RANGE);
        let (x_range, y_range) = (
            pad(x_range.0, x_range.1, 0.0),
            pad(y_range.0, y_range.1, 0.0),
        );
        let window = Bounds::new(x_range.0, x_range.1, y_range.0, y_range.1);

        // Layers that need the window.
        for (i, layer) in spec.layers.iter().enumerate() {
            match &layer.geometry {
                Geometry::Implicit { f, vars } => {
                    let names = [vars[0].as_str(), vars[1].as_str()];
                    let mut ev = |x: f64, y: f64| eval2(f, &fixed, names, x, y);
                    let grid = Grid::sample(window, q.implicit_cells, q.implicit_cells, &mut ev);
                    data[i] = LayerData::Lines(contour_lines(&grid, 0.0, &mut ev));
                }
                Geometry::Contour { f, vars, levels } => {
                    let names = [vars[0].as_str(), vars[1].as_str()];
                    let mut ev = |x: f64, y: f64| eval2(f, &fixed, names, x, y);
                    let grid = Grid::sample(window, q.implicit_cells, q.implicit_cells, &mut ev);
                    let mut out = Vec::new();
                    if let Some((lo, hi)) = grid.value_range() {
                        let k = (*levels).clamp(1, 40);
                        for j in 1..=k {
                            let level = lo + (hi - lo) * j as f64 / (k + 1) as f64;
                            out.push((level, contour_lines(&grid, level, &mut ev)));
                        }
                    }
                    data[i] = LayerData::Contours(out);
                }
                Geometry::Region {
                    cond,
                    boundary,
                    vars,
                } => {
                    let names = [vars[0].as_str(), vars[1].as_str()];
                    let mut holds = |x: f64, y: f64| {
                        eval_truth(
                            cond,
                            &Frame {
                                fixed: &fixed,
                                vars: &[(names[0], x), (names[1], y)],
                            },
                        )
                    };
                    let grid =
                        RegionGrid::sample(window, q.region_cells, q.region_cells, &mut holds);
                    let edges = boundary
                        .iter()
                        .map(|(g, strict)| {
                            let mut ev = |x: f64, y: f64| eval2(g, &fixed, names, x, y);
                            let gr =
                                Grid::sample(window, q.implicit_cells, q.implicit_cells, &mut ev);
                            (contour_lines(&gr, 0.0, &mut ev), *strict)
                        })
                        .collect();
                    data[i] = LayerData::Region { grid, edges };
                }
                _ => {}
            }
        }

        // The visual calculus helpers draw over the window the curves chose.
        let mut notes = Notes::default();
        for (i, layer) in spec.layers.iter().enumerate() {
            if let Some(d) = self.helper_2d(i, &layer.geometry, &fixed, window, &mut notes) {
                data[i] = d;
            }
        }
        messages.append(&mut notes.messages);

        for (i, d) in data.iter().enumerate() {
            let empty = match d {
                LayerData::Curve(s) => s.segments.is_empty(),
                LayerData::Lines(l) => l.is_empty(),
                LayerData::Empty => true,
                _ => false,
            };
            if empty && !spec.layers[i].geometry.is_helper() {
                if let Some(l) = &spec.layers[i].label {
                    messages.push(format!("nothing to draw for {} in this window", l.plain));
                }
            }
        }
        let plain_axes = spec.layers.iter().any(|l| {
            matches!(
                l.geometry,
                Geometry::Scatter(_) | Geometry::Histogram { .. } | Geometry::BoxPlot(_)
            )
        });
        self.equal_aspect =
            !has_explicit && !author_gave_range && !plain_axes && self.view.is_none();
        self.window = window;
        self.data = data;
        self.readouts = notes.readouts;
        self.messages = messages;
    }

    // ---- 3D ------------------------------------------------------------------

    fn build_3d(&mut self) {
        let spec = self.spec.clone();
        let fixed = self.values.clone();
        let q = self.quality;
        let mut data = vec![LayerData::Empty; spec.layers.len()];
        let mut messages = Vec::new();
        let mut notes = Notes::default();
        for (i, layer) in spec.layers.iter().enumerate() {
            match &layer.geometry {
                Geometry::Surface { z, vars } => {
                    let (x0, x1) = self.range(&vars[0]).unwrap_or((-5.0, 5.0));
                    let (y0, y1) = self.range(&vars[1]).unwrap_or((-5.0, 5.0));
                    let names = [vars[0].as_str(), vars[1].as_str()];
                    let n = q.surface_cells.clamp(2, 400);
                    let mut points = Vec::with_capacity((n + 1) * (n + 1));
                    for j in 0..=n {
                        let y = y0 + (y1 - y0) * j as f64 / n as f64;
                        for k in 0..=n {
                            let x = x0 + (x1 - x0) * k as f64 / n as f64;
                            let zv = eval2(z, &fixed, names, x, y).unwrap_or(f64::NAN);
                            points.push([x, y, zv]);
                        }
                    }
                    clip_spikes(&mut points);
                    data[i] = LayerData::Surface(Mesh {
                        nu: n + 1,
                        nv: n + 1,
                        points,
                    });
                }
                Geometry::ParamSurface { x, y, z, vars } => {
                    let (u0, u1) = self.range(&vars[0]).unwrap_or((0.0, TAU));
                    let (v0, v1) = self.range(&vars[1]).unwrap_or((0.0, TAU));
                    let n = q.surface_cells.clamp(2, 400);
                    let mut points = Vec::with_capacity((n + 1) * (n + 1));
                    for j in 0..=n {
                        let v = v0 + (v1 - v0) * j as f64 / n as f64;
                        for k in 0..=n {
                            let u = u0 + (u1 - u0) * k as f64 / n as f64;
                            let frame = Frame {
                                fixed: &fixed,
                                vars: &[(vars[0].as_str(), u), (vars[1].as_str(), v)],
                            };
                            let p = match (
                                eval_real(x, &frame),
                                eval_real(y, &frame),
                                eval_real(z, &frame),
                            ) {
                                (Some(a), Some(b), Some(c)) => [a, b, c],
                                _ => [f64::NAN; 3],
                            };
                            points.push(p);
                        }
                    }
                    data[i] = LayerData::Surface(Mesh {
                        nu: n + 1,
                        nv: n + 1,
                        points,
                    });
                }
                Geometry::Curve3 { x, y, z, var } => {
                    let (t0, t1) = self.range(var).unwrap_or((0.0, TAU));
                    let n = q.curve3_samples.clamp(2, 20_000);
                    let mut runs: Vec<Vec<[f64; 3]>> = Vec::new();
                    let mut cur = Vec::new();
                    for k in 0..=n {
                        let t = t0 + (t1 - t0) * k as f64 / n as f64;
                        let frame = Frame {
                            fixed: &fixed,
                            vars: &[(var.as_str(), t)],
                        };
                        match (
                            eval_real(x, &frame),
                            eval_real(y, &frame),
                            eval_real(z, &frame),
                        ) {
                            (Some(a), Some(b), Some(c)) => cur.push([a, b, c]),
                            _ => {
                                if cur.len() >= 2 {
                                    runs.push(std::mem::take(&mut cur));
                                }
                                cur.clear();
                            }
                        }
                    }
                    if cur.len() >= 2 {
                        runs.push(cur);
                    }
                    data[i] = LayerData::Curve3(runs);
                }
                Geometry::Revolve { .. } => {
                    if let Some(d) = self.revolve_3d(&layer.geometry, &fixed, &mut notes) {
                        data[i] = d;
                    }
                }
                // Planes wait until the box around the surface is known.
                Geometry::TangentPlane { .. } => {}
                _ => messages.push("a 2D item was left out of the 3D figure".to_string()),
            }
        }
        // The box that encloses all of it.
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        let mut take = |p: &[f64; 3]| {
            if p.iter().all(|v| v.is_finite()) {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
        };
        for d in &data {
            match d {
                LayerData::Surface(m) => m.points.iter().for_each(&mut take),
                LayerData::Curve3(runs) => runs.iter().flatten().for_each(&mut take),
                LayerData::Solid(s) if s.in_box => {
                    s.parts
                        .iter()
                        .flat_map(|p| p.mesh.points.iter())
                        .for_each(&mut take);
                    s.lines.iter().flatten().for_each(&mut take);
                }
                _ => {}
            }
        }
        let mut box3 = [(-1.0, 1.0); 3];
        for k in 0..3 {
            if lo[k].is_finite() && hi[k].is_finite() {
                box3[k] = pad(lo[k], hi[k], 0.0);
            }
        }
        self.box3 = box3;
        for (i, layer) in spec.layers.iter().enumerate() {
            if let Geometry::TangentPlane { .. } = &layer.geometry {
                if let Some(d) = self.plane_3d(&layer.geometry, &fixed, box3[2], &mut notes) {
                    data[i] = d;
                }
            }
        }
        messages.append(&mut notes.messages);
        self.readouts = notes.readouts;
        self.data = data;
        self.messages = messages;
    }
}

/// Takes about one sampled y per horizontal bucket, so that the refinement points piled up
/// near an asymptote do not count more than the rest of the curve.
fn one_per_bucket(s: &Sampled, x0: f64, x1: f64, out: &mut Vec<f64>) {
    const BUCKETS: usize = 512;
    if x1.partial_cmp(&x0) != Some(std::cmp::Ordering::Greater) {
        return;
    }
    let mut seen = vec![false; BUCKETS];
    for p in s.points() {
        if p[0] < x0 || p[0] > x1 {
            continue;
        }
        let k = (((p[0] - x0) / (x1 - x0)) * BUCKETS as f64) as usize;
        let k = k.min(BUCKETS - 1);
        if !seen[k] {
            seen[k] = true;
            out.push(p[1]);
        }
    }
}

/// Removes points far outside the bulk of the surface (poles), so they do not squash
/// the rest of the plot.
fn clip_spikes(points: &mut [[f64; 3]]) {
    let Some((lo, hi)) = robust_range(points.iter().map(|p| p[2])) else {
        return;
    };
    let margin = (hi - lo) * 0.05;
    for p in points.iter_mut() {
        if p[2].is_finite() && (p[2] < lo - margin || p[2] > hi + margin) {
            p[2] = f64::NAN;
        }
    }
}

/// Looks for where an implicit curve lives by scanning a wide grid for sign changes.
fn probe(
    f: &mpp_core::Expr,
    vars: &[String; 2],
    fixed: &BTreeMap<String, f64>,
) -> Option<(f64, f64, f64, f64)> {
    let names = [vars[0].as_str(), vars[1].as_str()];
    let b = Bounds::new(-24.0, 24.0, -24.0, 24.0);
    let mut ev = |x: f64, y: f64| eval2(f, fixed, names, x, y);
    let g = Grid::sample(b, 192, 192, &mut ev);
    let mut e: Option<(f64, f64, f64, f64)> = None;
    let (dx, dy) = (b.width() / g.nx as f64, b.height() / g.ny as f64);
    for j in 0..g.ny {
        for i in 0..g.nx {
            let v = [
                g.values[j * (g.nx + 1) + i],
                g.values[j * (g.nx + 1) + i + 1],
                g.values[(j + 1) * (g.nx + 1) + i],
                g.values[(j + 1) * (g.nx + 1) + i + 1],
            ];
            if v.iter().any(|x| x.is_nan()) {
                continue;
            }
            let pos = v.iter().filter(|x| **x > 0.0).count();
            if pos == 0 || pos == 4 {
                continue;
            }
            let (x, y) = (b.x0 + dx * i as f64, b.y0 + dy * j as f64);
            e = Some(match e {
                None => (x, x + dx, y, y + dy),
                Some(c) => (c.0.min(x), c.1.max(x + dx), c.2.min(y), c.3.max(y + dy)),
            });
        }
    }
    e.map(|(x0, x1, y0, y1)| {
        let (xs, ys) = pad(x0, x1, 0.25);
        let (ys0, ys1) = pad(y0, y1, 0.25);
        (xs, ys, ys0, ys1)
    })
}
