//! Sampling the visual calculus helpers: what each one draws for the current slider
//! values and window. The geometry itself (strips, crossings, solution curves, solids)
//! is in [`crate::helpers`]; this module evaluates the figure's expressions for it.

use std::collections::BTreeMap;

use mpp_core::Expr;

use crate::compile::{eval1, eval_const};
use crate::contour::Bounds;
use crate::figure::{Figure, LayerData};
use crate::helpers::{self, fmt_num, subscript, FieldKind};
use crate::sample::{sample_curve, P};
use crate::spec::{AxisLine, FigureSpec, Geometry};

/// Messages and read-outs collected while sampling.
#[derive(Default)]
pub(crate) struct Notes {
    pub messages: Vec<String>,
    pub readouts: Vec<String>,
}

/// The cells of a field's grid.
const FIELD_CELLS: (usize, usize) = (22, 15);

/// The x range of the data of scatter plots, with a little room.
pub(crate) fn data_x_range(spec: &FigureSpec) -> Option<(f64, f64)> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for l in &spec.layers {
        if let Geometry::Scatter(ps) = &l.geometry {
            for p in ps {
                lo = lo.min(p[0]);
                hi = hi.max(p[0]);
            }
        }
    }
    if !(lo.is_finite() && hi.is_finite()) {
        return None;
    }
    let pad = if hi > lo { 0.04 * (hi - lo) } else { 1.0 };
    Some((lo - pad, hi + pad))
}

/// `y = m x + b` written the way a textbook would.
pub fn line_equation(lhs: &str, var: &str, m: f64, x0: f64, y0: f64) -> String {
    let b = y0 - m * x0;
    let zero = |v: f64| v.abs() < 1e-9 * (1.0 + m.abs() + y0.abs());
    let slope = if zero(m) {
        String::new()
    } else if (m - 1.0).abs() < 1e-12 {
        var.to_string()
    } else if (m + 1.0).abs() < 1e-12 {
        format!("\u{2212}{var}")
    } else {
        format!("{}{var}", fmt_num(m))
    };
    match (slope.is_empty(), zero(b)) {
        (true, _) => format!("{lhs} = {}", fmt_num(b)),
        (false, true) => format!("{lhs} = {slope}"),
        (false, false) if b < 0.0 => format!("{lhs} = {slope} \u{2212} {}", fmt_num(-b)),
        (false, false) => format!("{lhs} = {slope} + {}", fmt_num(b)),
    }
}

impl Figure {
    /// The chain `f, f′, f″, …` up to order `upto` (fewer when a derivative cannot be
    /// found), kept between slider moves.
    fn deriv_chain(&mut self, key: usize, f: &Expr, var: &str, upto: usize) -> Vec<Expr> {
        let chain = self.derivs.entry(key).or_insert_with(|| vec![f.clone()]);
        while chain.len() <= upto {
            let Some(last) = chain.last() else { break };
            match mpp_core::calc::diff(last, var, 1, None) {
                Ok(d) => chain.push(d),
                Err(_) => break,
            }
        }
        chain.clone()
    }

    fn number(&self, e: &Expr, fixed: &BTreeMap<String, f64>) -> Option<f64> {
        eval_const(e, fixed).ok().filter(|v| v.is_finite())
    }

    /// Where a helper wants the window to be when the author gave no range.
    pub(crate) fn window_hint(
        &self,
        spec: &FigureSpec,
        fixed: &BTreeMap<String, f64>,
    ) -> Option<(f64, f64)> {
        for l in &spec.layers {
            let around = |c: f64, half: f64| Some((c - half, c + half));
            let hint = match &l.geometry {
                Geometry::Tangent { at, .. } => self.number(at, fixed).and_then(|c| around(c, 3.5)),
                Geometry::Newton { x0, .. } => self.number(x0, fixed).and_then(|c| around(c, 3.5)),
                Geometry::Taylor { center, .. } => {
                    self.number(center, fixed).and_then(|c| around(c, 4.0))
                }
                Geometry::Riemann {
                    bounds: Some((a, b)),
                    ..
                }
                | Geometry::AreaBetween {
                    bounds: Some((a, b)),
                    ..
                } => {
                    let (a, b) = (self.number(a, fixed)?, self.number(b, fixed)?);
                    let pad = 0.15 * (b - a).abs();
                    Some((a.min(b) - pad, a.max(b) + pad))
                }
                Geometry::AreaBetween {
                    f,
                    g,
                    var,
                    bounds: None,
                } => {
                    let ff = |x: f64| eval1(f, fixed, var, x);
                    let gg = |x: f64| match g {
                        Some(g) => eval1(g, fixed, var, x),
                        None => Some(0.0),
                    };
                    let z = helpers::zeros(&|x| Some(ff(x)? - gg(x)?), -10.0, 10.0);
                    match (z.first(), z.last()) {
                        (Some(&a), Some(&b)) if z.len() >= 2 => {
                            let pad = 0.4 * (b - a);
                            Some((a - pad, b + pad))
                        }
                        (Some(&a), _) => around(a, 3.0),
                        _ => None,
                    }
                }
                _ => None,
            };
            if hint.is_some() {
                return hint;
            }
        }
        None
    }

    /// Samples one helper layer of a 2D figure; `None` for the layers that are not helpers.
    pub(crate) fn helper_2d(
        &mut self,
        i: usize,
        geometry: &Geometry,
        fixed: &BTreeMap<String, f64>,
        window: Bounds,
        notes: &mut Notes,
    ) -> Option<LayerData> {
        match geometry {
            Geometry::Tangent { f, var, at } => {
                let Some(x0) = self.number(at, fixed) else {
                    notes
                        .messages
                        .push("the tangent point is not a number".into());
                    return None;
                };
                let chain = self.deriv_chain(i, f, var, 1);
                let (Some(y0), Some(m)) = (
                    eval1(f, fixed, var, x0),
                    helpers::slope_at(f, chain.get(1), var, fixed, x0),
                ) else {
                    notes.messages.push(format!(
                        "the curve has no tangent at {var} = {}",
                        fmt_num(x0)
                    ));
                    return None;
                };
                notes.readouts.push(format!(
                    "tangent at {var} = {}: {}",
                    fmt_num(x0),
                    line_equation("y", var, m, x0, y0)
                ));
                Some(LayerData::Composite(vec![
                    LayerData::Tangent {
                        point: [x0, y0],
                        slope: m,
                    },
                    LayerData::Points(vec![[x0, y0]]),
                ]))
            }
            Geometry::Riemann {
                f,
                var,
                rule,
                n,
                bounds,
            } => {
                let (a, b) = match bounds {
                    Some((a, b)) => (self.number(a, fixed)?, self.number(b, fixed)?),
                    None => self.range(var).unwrap_or((window.x0, window.x1)),
                };
                let n = self.number(n, fixed)?.round().clamp(1.0, 2000.0) as usize;
                let fun = |x: f64| eval1(f, fixed, var, x);
                let Some((bars, sum)) = helpers::riemann(&fun, *rule, n, a, b) else {
                    notes
                        .messages
                        .push("the interval for the strips is empty".into());
                    return None;
                };
                let exact = helpers::integrate(&fun, a, b);
                notes.readouts.push(format!(
                    "{} sum with n = {n}: {}",
                    rule.name(),
                    fmt_num(sum)
                ));
                notes
                    .readouts
                    .push(format!("exact integral \u{2248} {}", fmt_num(exact)));
                Some(LayerData::Bars(bars))
            }
            Geometry::AreaBetween { f, g, var, bounds } => {
                let ff = |x: f64| eval1(f, fixed, var, x);
                let gg = |x: f64| match g {
                    Some(g) => eval1(g, fixed, var, x),
                    None => Some(0.0),
                };
                let bounds = match bounds {
                    Some((a, b)) => Some((self.number(a, fixed)?, self.number(b, fixed)?)),
                    None => None,
                };
                let range = self.range(var).unwrap_or((window.x0, window.x1));
                let Some(area) = helpers::area_between(&ff, &gg, bounds, range) else {
                    notes
                        .messages
                        .push("there is no region between the curves".into());
                    return None;
                };
                notes.readouts.push(format!(
                    "area between x = {} and x = {}: {}",
                    fmt_num(area.bounds.0),
                    fmt_num(area.bounds.1),
                    fmt_num(area.area)
                ));
                Some(LayerData::Composite(vec![
                    LayerData::Shaded(area.polys),
                    LayerData::Points(area.crossings),
                ]))
            }
            Geometry::SlopeField { dydx, vars, starts } => {
                let names = [vars[0].as_str(), vars[1].as_str()];
                let m = |x: f64, y: f64| helpers::eval_xy(dydx, fixed, names, x, y);
                let items = helpers::slope_items(&m, window, FIELD_CELLS);
                let starts = self.start_points(starts, fixed, window, false);
                let dir = |x: f64, y: f64| m(x, y).map(|s| [1.0, s]);
                let curves = starts
                    .into_iter()
                    .map(|s| helpers::trajectory(&dir, s, window))
                    .filter(|c| c.len() >= 2)
                    .collect();
                Some(LayerData::Field {
                    items,
                    cell: cell_of(window),
                    kind: FieldKind::Slope,
                    curves,
                    directed: false,
                })
            }
            Geometry::PhasePortrait {
                dx,
                dy,
                vars,
                starts,
            } => {
                let names = [vars[0].as_str(), vars[1].as_str()];
                let dir = |x: f64, y: f64| {
                    Some([
                        helpers::eval_xy(dx, fixed, names, x, y)?,
                        helpers::eval_xy(dy, fixed, names, x, y)?,
                    ])
                };
                let unit = |x: f64, y: f64| {
                    let d = dir(x, y)?;
                    let n = d[0].hypot(d[1]);
                    (n > 1e-12).then(|| [d[0] / n, d[1] / n])
                };
                let items = helpers::arrow_items(&unit, window, FIELD_CELLS);
                let starts = self.start_points(starts, fixed, window, true);
                let curves = starts
                    .into_iter()
                    .map(|s| helpers::trajectory(&dir, s, window))
                    .filter(|c| c.len() >= 2)
                    .collect();
                Some(LayerData::Field {
                    items,
                    cell: cell_of(window),
                    kind: FieldKind::Arrow,
                    curves,
                    directed: true,
                })
            }
            Geometry::VectorField { fx, fy, vars } => {
                let names = [vars[0].as_str(), vars[1].as_str()];
                let v = |x: f64, y: f64| {
                    Some([
                        helpers::eval_xy(fx, fixed, names, x, y)?,
                        helpers::eval_xy(fy, fixed, names, x, y)?,
                    ])
                };
                Some(LayerData::Field {
                    items: helpers::arrow_items(&v, window, FIELD_CELLS),
                    cell: cell_of(window),
                    kind: FieldKind::Arrow,
                    curves: Vec::new(),
                    directed: false,
                })
            }
            Geometry::GradientField { f, vars } => {
                let names = [vars[0].as_str(), vars[1].as_str()];
                let partial = |k: usize| mpp_core::calc::diff(f, &vars[k], 1, None).ok();
                let (px, py) = (partial(0), partial(1));
                let grad = |x: f64, y: f64| {
                    let h = 1e-6 * (window.width() + window.height());
                    let one = |p: &Option<Expr>, k: usize| match p
                        .as_ref()
                        .and_then(|e| helpers::eval_xy(e, fixed, names, x, y))
                    {
                        Some(v) => Some(v),
                        None => {
                            let at = |dx: f64, dy: f64| {
                                helpers::eval_xy(f, fixed, names, x + dx, y + dy)
                            };
                            let (dx, dy) = if k == 0 { (h, 0.0) } else { (0.0, h) };
                            Some((at(dx, dy)? - at(-dx, -dy)?) / (2.0 * h))
                        }
                    };
                    Some([one(&px, 0)?, one(&py, 1)?])
                };
                Some(LayerData::Field {
                    items: helpers::arrow_items(&grad, window, FIELD_CELLS),
                    cell: cell_of(window),
                    kind: FieldKind::Arrow,
                    curves: Vec::new(),
                    directed: false,
                })
            }
            Geometry::Taylor {
                f,
                var,
                center,
                degree,
            } => {
                let a = self.number(center, fixed)?;
                let want = self.number(degree, fixed)?.round().clamp(0.0, 20.0) as usize;
                let chain = self.deriv_chain(i, f, var, want);
                let n = want.min(chain.len().saturating_sub(1));
                if n < want {
                    notes.messages.push(format!(
                        "the derivatives stop at order {n}, so the polynomial has degree {n}"
                    ));
                }
                let Some(coeffs) = helpers::taylor_coefficients(&chain, var, fixed, a, n) else {
                    notes.messages.push(format!(
                        "the function or one of its derivatives is undefined at {var} = {}",
                        fmt_num(a)
                    ));
                    return None;
                };
                let (x0, x1) = (window.x0, window.x1);
                let mut poly = |x: f64| {
                    let y = helpers::horner(&coeffs, a, x);
                    y.is_finite().then_some([x, y])
                };
                let curve = sample_curve(&mut poly, x0, x1, self.quality.curve);
                notes.readouts.push(format!(
                    "Taylor polynomial T{}({var}) of degree {n} about {var} = {}",
                    subscript(n),
                    fmt_num(a)
                ));
                let mut parts = vec![LayerData::Curve(curve)];
                if let Some(y) = eval1(f, fixed, var, a) {
                    parts.push(LayerData::Points(vec![[a, y]]));
                }
                Some(LayerData::Composite(parts))
            }
            Geometry::Newton { f, var, x0, steps } => {
                let start = self.number(x0, fixed)?;
                let steps = self.number(steps, fixed)?.round().clamp(0.0, 30.0) as usize;
                let chain = self.deriv_chain(i, f, var, 1);
                let ff = |x: f64| eval1(f, fixed, var, x);
                let df = |x: f64| helpers::slope_at(f, chain.get(1), var, fixed, x);
                let it = helpers::newton_iterates(&ff, &df, start, steps);
                if it.len() < 2 {
                    notes.messages.push(format!(
                        "Newton's method cannot take a step from {var} = {}",
                        fmt_num(start)
                    ));
                    return None;
                }
                let mut tangents: Vec<Vec<P>> = Vec::new();
                let mut drops: Vec<Vec<P>> = Vec::new();
                for w in it.windows(2) {
                    tangents.push(vec![w[0], [w[1][0], 0.0]]);
                    drops.push(vec![[w[1][0], 0.0], w[1]]);
                }
                let labels = it
                    .iter()
                    .enumerate()
                    .map(|(k, p)| {
                        (
                            [p[0], 0.0],
                            format!("{var}_{{{k}}}"),
                            format!("{var}{}", subscript(k)),
                        )
                    })
                    .collect();
                let last = it.last().copied()?;
                notes.readouts.push(format!(
                    "{var}{} \u{2248} {} after {} step{}",
                    subscript(it.len() - 1),
                    fmt_num(last[0]),
                    it.len() - 1,
                    if it.len() == 2 { "" } else { "s" }
                ));
                Some(LayerData::Composite(vec![
                    LayerData::DashedLines(drops),
                    LayerData::Lines(tangents),
                    LayerData::Points(it),
                    LayerData::Labels(labels),
                ]))
            }
            _ => None,
        }
    }

    /// The starting points of solution curves: the author's, or sensible ones.
    fn start_points(
        &self,
        starts: &[(Expr, Expr)],
        fixed: &BTreeMap<String, f64>,
        window: Bounds,
        ring: bool,
    ) -> Vec<P> {
        let given: Vec<P> = starts
            .iter()
            .filter_map(|(a, b)| Some([self.number(a, fixed)?, self.number(b, fixed)?]))
            .collect();
        if given.is_empty() {
            helpers::default_starts(window, ring)
        } else {
            given
        }
    }

    /// Samples a solid of revolution.
    pub(crate) fn revolve_3d(
        &self,
        geometry: &Geometry,
        fixed: &BTreeMap<String, f64>,
        notes: &mut Notes,
    ) -> Option<LayerData> {
        let Geometry::Revolve {
            f,
            g,
            var,
            bounds,
            axis,
        } = geometry
        else {
            return None;
        };
        let (a, b) = match bounds {
            Some((a, b)) => (self.number(a, fixed)?, self.number(b, fixed)?),
            None => self.range(var).unwrap_or((0.0, 1.0)),
        };
        let (a, b) = (a.min(b), a.max(b));
        let (horizontal, c) = match axis {
            AxisLine::Horizontal(c) => (true, self.number(c, fixed)?),
            AxisLine::Vertical(c) => (false, self.number(c, fixed)?),
        };
        let ff = |x: f64| eval1(f, fixed, var, x);
        let default_g = if horizontal { c } else { 0.0 };
        let gg = |x: f64| match g {
            Some(g) => eval1(g, fixed, var, x),
            None => Some(default_g),
        };
        if b - a < 1e-12 {
            notes
                .messages
                .push("the interval to revolve is empty".into());
            return None;
        }
        let v = helpers::volume(&ff, &gg, horizontal, c, a, b);
        let method = match (horizontal, g.is_some()) {
            (false, _) => "shell",
            (true, false) => "disk",
            (true, true) => "washer",
        };
        let axis_text = if horizontal {
            format!("y = {}", fmt_num(c))
        } else {
            format!("{var} = {}", fmt_num(c))
        };
        notes.readouts.push(format!(
            "turned about {axis_text}, {method} method: volume \u{2248} {}",
            fmt_num(v)
        ));
        Some(LayerData::Solid(helpers::revolve(
            &ff, &gg, horizontal, c, a, b,
        )))
    }

    /// Samples the plane touching a surface.
    pub(crate) fn plane_3d(
        &self,
        geometry: &Geometry,
        fixed: &BTreeMap<String, f64>,
        z_range: (f64, f64),
        notes: &mut Notes,
    ) -> Option<LayerData> {
        let Geometry::TangentPlane { z, vars, at } = geometry else {
            return None;
        };
        let names = [vars[0].as_str(), vars[1].as_str()];
        let (x0, y0) = (self.number(&at.0, fixed)?, self.number(&at.1, fixed)?);
        let z0 = helpers::eval_xy(z, fixed, names, x0, y0);
        let part = |k: usize| -> Option<f64> {
            if let Ok(d) = mpp_core::calc::diff(z, &vars[k], 1, None) {
                if let Some(v) = helpers::eval_xy(&d, fixed, names, x0, y0) {
                    return Some(v);
                }
            }
            let h = 1e-6;
            let (dx, dy) = if k == 0 { (h, 0.0) } else { (0.0, h) };
            let at = |s: f64| helpers::eval_xy(z, fixed, names, x0 + s * dx, y0 + s * dy);
            Some((at(1.0)? - at(-1.0)?) / (2.0 * h))
        };
        let (Some(z0), Some(zx), Some(zy)) = (z0, part(0), part(1)) else {
            notes
                .messages
                .push("the surface has no tangent plane at that point".into());
            return None;
        };
        let (rx, ry) = (
            self.range(&vars[0]).unwrap_or((-5.0, 5.0)),
            self.range(&vars[1]).unwrap_or((-5.0, 5.0)),
        );
        let half = [0.22 * (rx.1 - rx.0), 0.22 * (ry.1 - ry.0)];
        let pad = 0.02 * (z_range.1 - z_range.0);
        let mesh = helpers::tangent_plane(
            z0,
            [zx, zy],
            [x0, y0],
            half,
            (z_range.0 - pad, z_range.1 + pad),
        );
        let term = |c: f64, v: &str, p: f64| -> String {
            if c.abs() < 1e-12 {
                String::new()
            } else if p < 0.0 {
                format!(
                    " {} {}({v} + {})",
                    if c < 0.0 { "\u{2212}" } else { "+" },
                    fmt_num(c.abs()),
                    fmt_num(-p)
                )
            } else {
                format!(
                    " {} {}({v} \u{2212} {})",
                    if c < 0.0 { "\u{2212}" } else { "+" },
                    fmt_num(c.abs()),
                    fmt_num(p)
                )
            }
        };
        notes.readouts.push(format!(
            "tangent plane at ({}, {}): z = {}{}{}",
            fmt_num(x0),
            fmt_num(y0),
            fmt_num(z0),
            term(zx, &vars[0], x0),
            term(zy, &vars[1], y0)
        ));
        Some(LayerData::Solid(helpers::Solid {
            parts: vec![helpers::SolidPart {
                mesh,
                highlight: true,
                edged: true,
            }],
            lines: Vec::new(),
            dots: vec![[x0, y0, z0]],
            in_box: false,
        }))
    }
}

/// Data spacing of a field's grid.
fn cell_of(w: Bounds) -> P {
    [
        w.width() / FIELD_CELLS.0 as f64,
        w.height() / FIELD_CELLS.1 as f64,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_read_like_a_textbook() {
        assert_eq!(line_equation("y", "x", 2.0, 1.0, 1.0), "y = 2x \u{2212} 1");
        assert_eq!(line_equation("y", "x", 1.0, 0.0, 0.0), "y = x");
        assert_eq!(line_equation("y", "x", 0.0, 3.0, 5.0), "y = 5");
        assert_eq!(line_equation("y", "x", -1.0, 0.0, 2.0), "y = \u{2212}x + 2");
    }
}
