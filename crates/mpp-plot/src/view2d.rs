//! Laying a 2D figure out as a [`PlotScene`].

use crate::color::Color;
use crate::contour::Bounds;
use crate::figure::{Figure, LayerData};
use crate::scene::{
    Anchor, Dash, Item, Label, Marker, MarkerKind, PlotScene, Polygon, Polyline, Pt, Rect,
};
use crate::theme::PlotTheme;
use crate::ticks::{format_tick, is_half_pi_multiple, pi_ticks, ticks};

const TICK_FONT: f32 = 12.0;
const NAME_FONT: f32 = 14.0;

/// Where the plot area sits in the viewport and which part of the plane it shows.
/// The IDE uses it to turn mouse positions into plane coordinates for panning,
/// zooming and hover read-outs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout2d {
    pub rect: Rect,
    pub window: Bounds,
}

impl Layout2d {
    /// The plot area for a viewport, showing `window` (grown to equal scales if asked).
    pub fn new(width: f32, height: f32, window: Bounds, equal_aspect: bool) -> Layout2d {
        Layout2d::with_footer(width, height, window, equal_aspect, 0.0)
    }

    /// Like [`Layout2d::new`], leaving `footer` pixels free under the axes for read-outs.
    pub fn with_footer(
        width: f32,
        height: f32,
        window: Bounds,
        equal_aspect: bool,
        footer: f32,
    ) -> Layout2d {
        let small = width < 260.0 || height < 200.0;
        let (l, r, t, b) = if small {
            (30.0, 8.0, 8.0, 22.0)
        } else {
            (46.0, 20.0, 18.0, 36.0)
        };
        let b = b + footer;
        let rect = Rect::new(l, t, (width - l - r).max(10.0), (height - t - b).max(10.0));
        let mut w = window;
        if equal_aspect && w.is_valid() {
            let sx = f64::from(rect.w) / w.width();
            let sy = f64::from(rect.h) / w.height();
            let s = sx.min(sy);
            let (nw, nh) = (f64::from(rect.w) / s, f64::from(rect.h) / s);
            let (cx, cy) = ((w.x0 + w.x1) / 2.0, (w.y0 + w.y1) / 2.0);
            w = Bounds::new(cx - nw / 2.0, cx + nw / 2.0, cy - nh / 2.0, cy + nh / 2.0);
        }
        Layout2d { rect, window: w }
    }

    /// Plane to pixels (f64 so clipping happens before precision is lost).
    pub fn to_px(&self, p: [f64; 2]) -> [f64; 2] {
        let w = &self.window;
        [
            f64::from(self.rect.x) + (p[0] - w.x0) / w.width() * f64::from(self.rect.w),
            f64::from(self.rect.bottom()) - (p[1] - w.y0) / w.height() * f64::from(self.rect.h),
        ]
    }

    /// Pixels to the plane.
    pub fn to_world(&self, px: Pt) -> [f64; 2] {
        let w = &self.window;
        [
            w.x0 + (f64::from(px[0]) - f64::from(self.rect.x)) / f64::from(self.rect.w) * w.width(),
            w.y0 + (f64::from(self.rect.bottom()) - f64::from(px[1])) / f64::from(self.rect.h)
                * w.height(),
        ]
    }

    /// The window after dragging by `dx`, `dy` pixels.
    pub fn panned(&self, dx: f32, dy: f32) -> Bounds {
        let w = &self.window;
        let (ux, uy) = (
            w.width() / f64::from(self.rect.w),
            w.height() / f64::from(self.rect.h),
        );
        let (mx, my) = (-f64::from(dx) * ux, f64::from(dy) * uy);
        Bounds::new(w.x0 + mx, w.x1 + mx, w.y0 + my, w.y1 + my)
    }

    /// The window after zooming by `factor` (> 1 zooms in) about a pixel position.
    pub fn zoomed(&self, factor: f64, about: Pt) -> Bounds {
        let f = if factor.is_finite() && factor > 1e-6 {
            factor
        } else {
            1.0
        };
        let c = self.to_world(about);
        let w = &self.window;
        Bounds::new(
            c[0] - (c[0] - w.x0) / f,
            c[0] + (w.x1 - c[0]) / f,
            c[1] - (c[1] - w.y0) / f,
            c[1] + (w.y1 - c[1]) / f,
        )
    }
}

/// Clips a polyline (in pixels, f64) to a rectangle, splitting it where it leaves.
pub fn clip_polyline(points: &[[f64; 2]], r: Rect) -> Vec<Vec<Pt>> {
    let (x0, x1) = (f64::from(r.x), f64::from(r.right()));
    let (y0, y1) = (f64::from(r.y), f64::from(r.bottom()));
    let mut out: Vec<Vec<Pt>> = Vec::new();
    let mut cur: Vec<Pt> = Vec::new();
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        if !(a.iter().chain(b.iter()).all(|v| v.is_finite())) {
            if cur.len() >= 2 {
                out.push(std::mem::take(&mut cur));
            }
            cur.clear();
            continue;
        }
        // Liang-Barsky.
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let (mut t0, mut t1) = (0.0f64, 1.0f64);
        let mut inside = true;
        for (p, q) in [
            (-dx, a[0] - x0),
            (dx, x1 - a[0]),
            (-dy, a[1] - y0),
            (dy, y1 - a[1]),
        ] {
            if p == 0.0 {
                if q < 0.0 {
                    inside = false;
                    break;
                }
            } else {
                let t = q / p;
                if p < 0.0 {
                    t0 = t0.max(t);
                } else {
                    t1 = t1.min(t);
                }
            }
        }
        if !inside || t0 > t1 {
            if cur.len() >= 2 {
                out.push(std::mem::take(&mut cur));
            }
            cur.clear();
            continue;
        }
        let pa = [(a[0] + t0 * dx) as f32, (a[1] + t0 * dy) as f32];
        let pb = [(a[0] + t1 * dx) as f32, (a[1] + t1 * dy) as f32];
        if t0 > 0.0 && !cur.is_empty() {
            // Re-entered: a new piece.
            if cur.len() >= 2 {
                out.push(std::mem::take(&mut cur));
            }
            cur.clear();
        }
        if cur.is_empty() {
            cur.push(pa);
        }
        cur.push(pb);
        if t1 < 1.0 {
            out.push(std::mem::take(&mut cur));
            cur.clear();
        }
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out
}

/// LaTeX for a variable name (Greek letters become commands).
pub fn name_latex(name: &str) -> String {
    const GREEK: [(char, &str); 16] = [
        ('α', "\\alpha"),
        ('β', "\\beta"),
        ('γ', "\\gamma"),
        ('δ', "\\delta"),
        ('θ', "\\theta"),
        ('λ', "\\lambda"),
        ('μ', "\\mu"),
        ('π', "\\pi"),
        ('ρ', "\\rho"),
        ('σ', "\\sigma"),
        ('τ', "\\tau"),
        ('φ', "\\varphi"),
        ('ϕ', "\\phi"),
        ('ψ', "\\psi"),
        ('ω', "\\omega"),
        ('ε', "\\varepsilon"),
    ];
    name.chars()
        .map(|c| {
            GREEK
                .iter()
                .find(|(g, _)| *g == c)
                .map_or_else(|| c.to_string(), |(_, l)| format!("{l} "))
        })
        .collect::<String>()
        .trim_end()
        .to_string()
}

fn pt(p: [f64; 2]) -> Pt {
    [p[0] as f32, p[1] as f32]
}

impl Figure {
    /// The 2D layout this figure uses in a viewport.
    pub fn layout_2d(&self, width: f32, height: f32) -> Layout2d {
        let rows = self.readouts().len() as f32;
        let footer = if rows > 0.0 { rows * 17.0 + 4.0 } else { 0.0 };
        Layout2d::with_footer(width, height, self.window, self.equal_aspect, footer)
    }

    /// Draws a 2D figure into a pixel-space display list.
    pub fn scene_2d(&self, width: f32, height: f32, theme: &PlotTheme) -> PlotScene {
        let (width, height) = (width.max(40.0), height.max(40.0));
        let mut scene = PlotScene::new(
            width,
            height,
            theme.background,
            &theme.font,
            &theme.math_font,
        );
        let lay = self.layout_2d(width, height);
        let (rect, win) = (lay.rect, lay.window);

        // Shaded regions go first so grid and axes stay visible on top. Where several
        // overlap, the colours blend, so an intersection reads darker.
        let regions: Vec<(Color, &crate::contour::RegionGrid)> = self
            .data
            .iter()
            .enumerate()
            .filter_map(|(i, d)| match d {
                LayerData::Region { grid, .. } => {
                    Some((theme.series(i).with_alpha(theme.region_alpha), grid))
                }
                _ => None,
            })
            .collect();
        region_rects(&mut scene, &lay, &regions, theme.background);
        for (i, d) in self.data.iter().enumerate() {
            self.underlay_2d(&mut scene, &lay, d, theme.series(i));
        }

        // Axes and grid.
        let axis_y = if win.y0 <= 0.0 && win.y1 >= 0.0 {
            0.0
        } else {
            win.y0
        };
        let axis_x = if win.x0 <= 0.0 && win.x1 >= 0.0 {
            0.0
        } else {
            win.x0
        };
        let [ax_px, ay_px] = lay.to_px([axis_x, axis_y]);
        let (xt, xstep) = ticks(
            win.x0,
            win.x1,
            ((f64::from(rect.w) / 70.0) as usize).clamp(3, 12),
        );
        let (yt, ystep) = ticks(
            win.y0,
            win.y1,
            ((f64::from(rect.h) / 50.0) as usize).clamp(3, 10),
        );
        let x_pi = is_half_pi_multiple(win.x0) && is_half_pi_multiple(win.x1);
        let x_marks: Vec<(f64, String)> = match x_pi.then(|| pi_ticks(win.x0, win.x1)).flatten() {
            Some(m) => m,
            None => xt.iter().map(|&v| (v, format_tick(v, xstep))).collect(),
        };
        let categorical = self.data.iter().any(|d| matches!(d, LayerData::BoxPlot(_)));
        let x_marks = if categorical { Vec::new() } else { x_marks };
        let both_at_zero = axis_x == 0.0 && axis_y == 0.0;

        for (v, _) in &x_marks {
            let x = lay.to_px([*v, 0.0])[0] as f32;
            scene.line(vec![[x, rect.y], [x, rect.bottom()]], theme.grid, 1.0);
        }
        for v in &yt {
            let y = lay.to_px([0.0, *v])[1] as f32;
            scene.line(vec![[rect.x, y], [rect.right(), y]], theme.grid, 1.0);
        }

        // Curves and data.
        let mut markers: Vec<Marker> = Vec::new();
        for (i, d) in self.data.iter().enumerate() {
            self.draw_layer_2d(&mut scene, &lay, theme, d, theme.series(i), &mut markers);
        }
        // Axis lines, ticks and labels sit above the curves' fill but below markers.
        let (lx, rx, ty, by) = (rect.x, rect.right(), rect.y, rect.bottom());
        let axis_w = 1.4;
        scene.line(
            vec![[lx, ay_px as f32], [rx, ay_px as f32]],
            theme.axes,
            axis_w,
        );
        scene.line(
            vec![[ax_px as f32, by], [ax_px as f32, ty]],
            theme.axes,
            axis_w,
        );
        arrow_head(&mut scene, [rx, ay_px as f32], [1.0, 0.0], theme.axes);
        arrow_head(&mut scene, [ax_px as f32, ty], [0.0, -1.0], theme.axes);
        for (v, text) in &x_marks {
            let x = lay.to_px([*v, 0.0])[0] as f32;
            scene.line(
                vec![[x, ay_px as f32 - 3.5], [x, ay_px as f32 + 3.5]],
                theme.axes,
                1.0,
            );
            if both_at_zero && *v == 0.0 {
                continue;
            }
            let y = (ay_px as f32 + 17.0).min(height - 4.0);
            scene.text(tick_label([x, y], text, Anchor::Middle, theme));
        }
        for v in &yt {
            let y = lay.to_px([0.0, *v])[1] as f32;
            scene.line(
                vec![[ax_px as f32 - 3.5, y], [ax_px as f32 + 3.5, y]],
                theme.axes,
                1.0,
            );
            if both_at_zero && *v == 0.0 {
                continue;
            }
            let x = (ax_px as f32 - 8.0).max(30.0);
            scene.text(tick_label(
                [x, y + 4.0],
                &format_tick(*v, ystep),
                Anchor::End,
                theme,
            ));
        }
        if both_at_zero {
            scene.text(tick_label(
                [ax_px as f32 - 6.0, ay_px as f32 + 16.0],
                "0",
                Anchor::End,
                theme,
            ));
        }

        // Axis names.
        let (xn, yn) = self.axis_names();
        if !xn.is_empty() {
            scene.text(axis_name(
                [rx - 2.0, ay_px as f32 - 8.0],
                &xn,
                Anchor::End,
                theme,
            ));
        }
        if !yn.is_empty() {
            scene.text(axis_name(
                [ax_px as f32 + 9.0, ty + 10.0],
                &yn,
                Anchor::Start,
                theme,
            ));
        }

        for m in markers {
            scene.push(Item::Marker(m));
        }
        self.legend_2d(&mut scene, rect, theme);
        let n = self.readouts().len();
        for (k, line) in self.readouts().iter().enumerate() {
            scene.text(Label {
                pos: [rect.x + 4.0, height - 8.0 - 17.0 * (n - 1 - k) as f32],
                text: line.clone(),
                latex: None,
                anchor: Anchor::Start,
                size: 13.0,
                color: theme.text,
                rotation: 0.0,
                italic: false,
            });
        }
        scene
    }

    /// Draws one layer of a 2D figure.
    fn draw_layer_2d(
        &self,
        scene: &mut PlotScene,
        lay: &Layout2d,
        theme: &PlotTheme,
        d: &LayerData,
        color: Color,
        markers: &mut Vec<Marker>,
    ) {
        let rect = lay.rect;
        match d {
            LayerData::Curve(s) => {
                for seg in &s.segments {
                    let px: Vec<[f64; 2]> = seg.iter().map(|p| lay.to_px(*p)).collect();
                    for piece in clip_polyline(&px, rect) {
                        scene.push(Item::Polyline(Polyline {
                            points: piece,
                            color,
                            width: 2.2,
                            dash: Dash::Solid,
                        }));
                    }
                }
                for j in &s.jumps {
                    let mut dot = |y: f64, filled: bool| {
                        let p = pt(lay.to_px([j.x, y]));
                        if rect.contains(p) {
                            markers.push(Marker {
                                pos: p,
                                kind: MarkerKind::Circle,
                                color,
                                size: 8.0,
                                filled,
                                hollow_fill: theme.background,
                            });
                        }
                    };
                    dot(j.left, j.left_closed);
                    dot(j.right, j.right_closed);
                    if let Some(v) = j.isolated {
                        dot(v, true);
                    }
                }
            }
            LayerData::Lines(lines) => {
                draw_lines(scene, lay, lines, color, 2.2, Dash::Solid);
            }
            LayerData::Contours(levels) => {
                let n = levels.len().max(2) - 1;
                for (k, (_, lines)) in levels.iter().enumerate() {
                    let c = theme.colormap(k as f64 / n as f64);
                    draw_lines(scene, lay, lines, c, 1.6, Dash::Solid);
                }
            }
            LayerData::Region { edges, .. } => {
                for (lines, strict) in edges {
                    let dash = if *strict {
                        Dash::Dashed(6.0, 4.0)
                    } else {
                        Dash::Solid
                    };
                    draw_lines(scene, lay, lines, color, 2.0, dash);
                }
            }
            LayerData::Points(ps) => {
                for p in ps {
                    let q = pt(lay.to_px(*p));
                    if rect.contains(q) {
                        markers.push(Marker {
                            pos: q,
                            kind: MarkerKind::Circle,
                            color,
                            size: 8.0,
                            filled: true,
                            hollow_fill: theme.background,
                        });
                    }
                }
            }
            LayerData::Histogram { edges, counts } => {
                let fill = color.with_alpha(150).over(theme.background);
                for (k, h) in counts.iter().enumerate() {
                    let (Some(&a), Some(&b)) = (edges.get(k), edges.get(k + 1)) else {
                        continue;
                    };
                    let (p0, p1) = (lay.to_px([a, 0.0]), lay.to_px([b, *h]));
                    scene.push(Item::Polygon(Polygon {
                        points: vec![
                            pt([p0[0], p0[1]]),
                            pt([p1[0], p0[1]]),
                            pt([p1[0], p1[1]]),
                            pt([p0[0], p1[1]]),
                        ],
                        fill: Some(fill),
                        stroke: Some((color, 1.2)),
                    }));
                }
            }
            LayerData::Tangent { point, slope } => {
                let w = &lay.window;
                let y_at = |x: f64| point[1] + slope * (x - point[0]);
                let px = [lay.to_px([w.x0, y_at(w.x0)]), lay.to_px([w.x1, y_at(w.x1)])];
                for piece in clip_polyline(&px, rect) {
                    scene.push(Item::Polyline(Polyline {
                        points: piece,
                        color,
                        width: 2.0,
                        dash: Dash::Solid,
                    }));
                }
            }
            LayerData::DashedLines(lines) => {
                draw_lines(scene, lay, lines, color, 1.5, Dash::Dashed(5.0, 4.0));
            }
            LayerData::Field {
                items,
                cell,
                kind,
                curves,
                directed,
            } => self.draw_field(
                scene, lay, theme, items, *cell, *kind, curves, *directed, color,
            ),
            LayerData::Labels(labels) => {
                let mut last = f32::NEG_INFINITY;
                for (p, latex, plain) in labels {
                    let q = lay.to_px(*p);
                    let q = [q[0] as f32, q[1] as f32 + 36.0];
                    if q[0] >= rect.x
                        && q[0] <= rect.right()
                        && q[1] <= rect.bottom() + 30.0
                        && (q[0] - last).abs() > 26.0
                    {
                        last = q[0];
                        scene.text(Label {
                            pos: q,
                            text: plain.clone(),
                            latex: Some(latex.clone()),
                            anchor: Anchor::Middle,
                            size: 13.0,
                            color: theme.text,
                            rotation: 0.0,
                            italic: false,
                        });
                    }
                }
            }
            LayerData::BoxPlot(stats) => {
                self.draw_boxes(scene, lay, theme, stats, markers);
            }
            LayerData::Composite(parts) => {
                for p in parts {
                    self.draw_layer_2d(scene, lay, theme, p, color, markers);
                }
            }
            LayerData::Bars(_)
            | LayerData::Shaded(_)
            | LayerData::Solid(_)
            | LayerData::Empty
            | LayerData::Surface(_)
            | LayerData::Curve3(_) => {}
        }
    }

    /// Fills drawn under the grid: Riemann strips and the area between curves.
    fn underlay_2d(&self, scene: &mut PlotScene, lay: &Layout2d, d: &LayerData, color: Color) {
        match d {
            LayerData::Bars(bars) => {
                for b in bars {
                    let pts = [[b.x0, 0.0], [b.x1, 0.0], [b.x1, b.y1], [b.x0, b.y0]]
                        .map(|p| pt(lay.to_px(p)));
                    scene.push(Item::Polygon(Polygon {
                        points: pts.to_vec(),
                        fill: Some(color.with_alpha(90)),
                        stroke: Some((color, 1.2)),
                    }));
                }
            }
            LayerData::Shaded(polys) => {
                for poly in polys {
                    scene.push(Item::Polygon(Polygon {
                        points: poly.iter().map(|p| pt(lay.to_px(*p))).collect(),
                        fill: Some(color.with_alpha(95)),
                        stroke: None,
                    }));
                }
            }
            LayerData::Composite(parts) => {
                for p in parts {
                    self.underlay_2d(scene, lay, p, color);
                }
            }
            _ => {}
        }
    }

    /// Slope strokes, arrows and the solution curves of a field.
    #[allow(clippy::too_many_arguments)]
    fn draw_field(
        &self,
        scene: &mut PlotScene,
        lay: &Layout2d,
        theme: &PlotTheme,
        items: &[crate::helpers::FieldItem],
        cell: [f64; 2],
        kind: crate::helpers::FieldKind,
        curves: &[Vec<[f64; 2]>],
        directed: bool,
        color: Color,
    ) {
        use crate::helpers::FieldKind;
        let (sx, sy) = (
            f64::from(lay.rect.w) / lay.window.width(),
            f64::from(lay.rect.h) / lay.window.height(),
        );
        let reach = (cell[0] * sx).min(cell[1] * sy) * 0.82;
        let vmax = items
            .iter()
            .map(|it| it.dir[0].hypot(it.dir[1]))
            .fold(0.0, f64::max);
        let stroke = theme.axes.with_alpha(150);
        for it in items {
            let (dx, dy) = (it.dir[0] * sx, -it.dir[1] * sy);
            let n = dx.hypot(dy);
            if !(n.is_finite() && n > 1e-12) {
                continue;
            }
            let (ux, uy) = (dx / n, dy / n);
            let c = lay.to_px(it.pos);
            match kind {
                FieldKind::Slope => {
                    let h = reach / 2.0;
                    scene.push(Item::Polyline(Polyline {
                        points: vec![
                            pt([c[0] - ux * h, c[1] - uy * h]),
                            pt([c[0] + ux * h, c[1] + uy * h]),
                        ],
                        color: stroke,
                        width: 1.4,
                        dash: Dash::Solid,
                    }));
                }
                FieldKind::Arrow => {
                    let mag = if vmax > 0.0 {
                        it.dir[0].hypot(it.dir[1]) / vmax
                    } else {
                        0.0
                    };
                    let len = reach * 1.1 * mag.clamp(0.2, 1.0);
                    let h = len / 2.0;
                    let (tail, tip) = (
                        [c[0] - ux * h, c[1] - uy * h],
                        [c[0] + ux * h, c[1] + uy * h],
                    );
                    let col = if directed { stroke } else { color };
                    scene.push(Item::Polyline(Polyline {
                        points: vec![pt(tail), pt(tip)],
                        color: col,
                        width: 1.5,
                        dash: Dash::Solid,
                    }));
                    arrow_head(scene, pt(tip), [ux as f32, uy as f32], col);
                }
            }
        }
        draw_lines(scene, lay, curves, color, 2.2, Dash::Solid);
        if directed {
            for c in curves {
                let k = c.len() * 55 / 100;
                if let (Some(a), Some(b)) = (c.get(k), c.get(k + 1)) {
                    let (pa, pb) = (lay.to_px(*a), lay.to_px(*b));
                    let (dx, dy) = (pb[0] - pa[0], pb[1] - pa[1]);
                    let n = dx.hypot(dy);
                    if n > 1e-9 && lay.rect.contains(pt(pb)) {
                        arrow_head(scene, pt(pb), [(dx / n) as f32, (dy / n) as f32], color);
                    }
                }
            }
        }
    }

    /// Box plots: boxes, medians, whiskers, outliers and the names of the data below.
    fn draw_boxes(
        &self,
        scene: &mut PlotScene,
        lay: &Layout2d,
        theme: &PlotTheme,
        stats: &[crate::spec::BoxStats],
        markers: &mut Vec<Marker>,
    ) {
        for (k, s) in stats.iter().enumerate() {
            let c = (k + 1) as f64;
            let hw = 0.28;
            let at = |x: f64, y: f64| pt(lay.to_px([x, y]));
            let col = theme.series(k);
            scene.push(Item::Polygon(Polygon {
                points: vec![
                    at(c - hw, s.q1),
                    at(c + hw, s.q1),
                    at(c + hw, s.q3),
                    at(c - hw, s.q3),
                ],
                fill: Some(col.with_alpha(90)),
                stroke: Some((col, 1.6)),
            }));
            scene.push(Item::Polyline(Polyline {
                points: vec![at(c - hw, s.median), at(c + hw, s.median)],
                color: col,
                width: 3.0,
                dash: Dash::Solid,
            }));
            for (from, to) in [(s.q1, s.low), (s.q3, s.high)] {
                scene.push(Item::Polyline(Polyline {
                    points: vec![at(c, from), at(c, to)],
                    color: col,
                    width: 1.6,
                    dash: Dash::Solid,
                }));
                scene.push(Item::Polyline(Polyline {
                    points: vec![at(c - hw / 2.0, to), at(c + hw / 2.0, to)],
                    color: col,
                    width: 1.6,
                    dash: Dash::Solid,
                }));
            }
            for o in &s.outliers {
                markers.push(Marker {
                    pos: at(c, *o),
                    kind: MarkerKind::Circle,
                    color: col,
                    size: 7.0,
                    filled: false,
                    hollow_fill: theme.background,
                });
            }
            let q = lay.to_px([c, lay.window.y0]);
            scene.text(Label {
                pos: [q[0] as f32, lay.rect.bottom() + 17.0],
                text: math_italic(&s.label),
                latex: Some(name_latex(&s.label)),
                anchor: Anchor::Middle,
                size: NAME_FONT,
                color: theme.text,
                rotation: 0.0,
                italic: false,
            });
        }
    }

    /// Names of the horizontal and vertical axes.
    pub fn axis_names(&self) -> (String, String) {
        if let Some((x, y)) = &self.spec.axis_names {
            return (x.clone(), y.clone());
        }
        use crate::spec::Geometry::*;
        for l in &self.spec.layers {
            match &l.geometry {
                Explicit { var, dep, .. } => return (var.clone(), dep.clone()),
                ExplicitX { var, .. } => return ("x".into(), var.clone()),
                Implicit { vars, .. } | Region { vars, .. } | Contour { vars, .. } => {
                    return (vars[0].clone(), vars[1].clone())
                }
                _ => {}
            }
        }
        ("x".into(), "y".into())
    }

    fn legend_2d(&self, scene: &mut PlotScene, rect: Rect, theme: &PlotTheme) {
        let entries = self.legend();
        if entries.len() < 2 {
            return;
        }
        let size = 13.0f32;
        let row = 19.0f32;
        let text_w = entries
            .iter()
            .map(|(l, _)| l.plain.chars().count())
            .max()
            .unwrap_or(0) as f32
            * size
            * 0.52;
        let (w, h) = (text_w + 46.0, row * entries.len() as f32 + 8.0);
        let x = rect.right() - w - 8.0;
        let y = rect.y + 8.0;
        scene.push(Item::Polygon(Polygon {
            points: vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]],
            fill: Some(theme.legend_background),
            stroke: Some((theme.legend_border, 1.0)),
        }));
        for (k, (label, layer)) in entries.iter().enumerate() {
            let cy = y + 4.0 + row * k as f32 + row / 2.0;
            let color = theme.series(*layer);
            scene.push(Item::Polyline(Polyline {
                points: vec![[x + 8.0, cy], [x + 30.0, cy]],
                color,
                width: 2.4,
                dash: Dash::Solid,
            }));
            scene.text(Label {
                pos: [x + 38.0, cy + size * 0.33],
                text: label.plain.clone(),
                latex: Some(label.latex.clone()),
                anchor: Anchor::Start,
                size,
                color: theme.text,
                rotation: 0.0,
                italic: false,
            });
        }
    }
}

/// The name in the mathematical italic letters of Unicode (`x` as `𝑥`), so an axis letter
/// is slanted whatever font the reader has, even one with no italic of its own.
pub fn math_italic(name: &str) -> String {
    name.chars()
        .map(|c| {
            let base = match c {
                'h' => return '\u{210E}',
                'A'..='Z' => 0x1D434 + (c as u32 - 'A' as u32),
                'a'..='z' => 0x1D44E + (c as u32 - 'a' as u32),
                // Greek small letters alpha to omega.
                '\u{3b1}'..='\u{3c9}' => 0x1D6FC + (c as u32 - 0x3B1),
                _ => return c,
            };
            char::from_u32(base).unwrap_or(c)
        })
        .collect()
}

/// Draws short read-outs in a box in the top left corner of the plot.
pub fn draw_readouts(scene: &mut PlotScene, theme: &PlotTheme, lines: &[String], x: f32, y: f32) {
    if lines.is_empty() {
        return;
    }
    let size = 12.5f32;
    let row = 17.0f32;
    let chars = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as f32;
    let (w, h) = (chars * size * 0.5 + 18.0, row * lines.len() as f32 + 8.0);
    scene.push(Item::Polygon(Polygon {
        points: vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]],
        fill: Some(theme.legend_background),
        stroke: Some((theme.legend_border, 1.0)),
    }));
    for (k, line) in lines.iter().enumerate() {
        scene.text(Label {
            pos: [x + 9.0, y + 4.0 + row * (k as f32 + 0.5) + size * 0.33],
            text: line.clone(),
            latex: None,
            anchor: Anchor::Start,
            size,
            color: theme.text,
            rotation: 0.0,
            italic: false,
        });
    }
}

fn tick_label(pos: Pt, text: &str, anchor: Anchor, theme: &PlotTheme) -> Label {
    Label {
        pos,
        text: text.to_string(),
        latex: None,
        anchor,
        size: TICK_FONT,
        color: theme.text,
        rotation: 0.0,
        italic: false,
    }
}

fn axis_name(pos: Pt, name: &str, anchor: Anchor, theme: &PlotTheme) -> Label {
    Label {
        pos,
        text: if name.chars().count() == 1 {
            math_italic(name)
        } else {
            name.to_string()
        },
        latex: Some(name_latex(name)),
        anchor,
        size: NAME_FONT,
        color: theme.text,
        rotation: 0.0,
        italic: false,
    }
}

fn arrow_head(scene: &mut PlotScene, tip: Pt, dir: [f32; 2], color: Color) {
    let (len, half) = (8.0f32, 3.2f32);
    let base = [tip[0] - dir[0] * len, tip[1] - dir[1] * len];
    let n = [-dir[1], dir[0]];
    scene.push(Item::Polygon(Polygon {
        points: vec![
            tip,
            [base[0] + n[0] * half, base[1] + n[1] * half],
            [base[0] - n[0] * half, base[1] - n[1] * half],
        ],
        fill: Some(color),
        stroke: None,
    }));
}

fn draw_lines(
    scene: &mut PlotScene,
    lay: &Layout2d,
    lines: &[Vec<[f64; 2]>],
    color: Color,
    width: f32,
    dash: Dash,
) {
    for line in lines {
        let px: Vec<[f64; 2]> = line.iter().map(|p| lay.to_px(*p)).collect();
        for piece in clip_polyline(&px, lay.rect) {
            scene.push(Item::Polyline(Polyline {
                points: piece,
                color,
                width,
                dash,
            }));
        }
    }
}

/// Fills the cells of the region grids with opaque rectangles (each cell the blend of
/// the regions that contain it), merging rows with identical runs so there are no seams.
fn region_rects(
    scene: &mut PlotScene,
    lay: &Layout2d,
    regions: &[(Color, &crate::contour::RegionGrid)],
    background: Color,
) {
    let Some((_, first)) = regions.first() else {
        return;
    };
    let (nx, ny, bounds) = (first.nx, first.ny, first.bounds);
    let usable: Vec<&(Color, &crate::contour::RegionGrid)> = regions
        .iter()
        .filter(|(_, g)| g.nx == nx && g.ny == ny)
        .collect();
    let cell_x = |i: usize| bounds.x0 + bounds.width() * i as f64 / nx as f64;
    let cell_y = |j: usize| bounds.y0 + bounds.height() * j as f64 / ny as f64;
    let colour_at = |i: usize, j: usize| -> Option<Color> {
        let mut c: Option<Color> = None;
        for (fill, g) in &usable {
            if g.inside[j * nx + i] {
                c = Some(fill.over(c.unwrap_or(background)));
            }
        }
        c
    };
    // Runs of equal colour in a row, as (first, one past last, colour).
    let runs = |j: usize| -> Vec<(usize, usize, Color)> {
        let mut out: Vec<(usize, usize, Color)> = Vec::new();
        for i in 0..nx {
            match (colour_at(i, j), out.last_mut()) {
                (Some(c), Some(last)) if last.1 == i && last.2 == c => last.1 = i + 1,
                (Some(c), _) => out.push((i, i + 1, c)),
                (None, _) => {}
            }
        }
        out
    };
    let mut emit = |run: (usize, usize, Color), j0: usize, j1: usize| {
        let a = lay.to_px([cell_x(run.0), cell_y(j1)]);
        let b = lay.to_px([cell_x(run.1), cell_y(j0)]);
        let (x0, x1) = (
            a[0].max(f64::from(lay.rect.x)),
            b[0].min(f64::from(lay.rect.right())),
        );
        let (y0, y1) = (
            a[1].max(f64::from(lay.rect.y)),
            b[1].min(f64::from(lay.rect.bottom())),
        );
        if x1 > x0 && y1 > y0 {
            let e = 0.4;
            scene.push(Item::Polygon(Polygon {
                points: vec![
                    pt([x0, y0 - e]),
                    pt([x1, y0 - e]),
                    pt([x1, y1 + e]),
                    pt([x0, y1 + e]),
                ],
                fill: Some(run.2),
                stroke: None,
            }));
        }
    };
    let mut open: Vec<((usize, usize, Color), usize)> = Vec::new();
    for j in 0..ny {
        let row = runs(j);
        let mut next = Vec::new();
        for (run, start) in open.drain(..) {
            if row.contains(&run) {
                next.push((run, start));
            } else {
                emit(run, start, j);
            }
        }
        for run in row {
            if !next.iter().any(|(r, _)| *r == run) {
                next.push((run, j));
            }
        }
        open = next;
    }
    for (run, start) in open {
        emit(run, start, ny);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_splits_where_the_line_leaves() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        let line = [
            [-5.0, 5.0],
            [5.0, 5.0],
            [5.0, 50.0],
            [8.0, 50.0],
            [8.0, 5.0],
        ];
        let pieces = clip_polyline(&line, r);
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[0][0], [0.0, 5.0]);
        assert!(pieces
            .iter()
            .flatten()
            .all(|p| p[0] >= 0.0 && p[0] <= 10.0 && p[1] >= 0.0 && p[1] <= 10.0));
        assert!(clip_polyline(&[[f64::NAN, 0.0], [1.0, 1.0]], r).is_empty());
        assert!(clip_polyline(&[[-9.0, -9.0], [-8.0, 50.0]], r).is_empty());
    }

    #[test]
    fn layout_round_trips() {
        let l = Layout2d::new(600.0, 400.0, Bounds::new(-2.0, 3.0, -1.0, 4.0), false);
        let p = l.to_px([1.0, 2.0]);
        let w = l.to_world([p[0] as f32, p[1] as f32]);
        assert!((w[0] - 1.0).abs() < 1e-4 && (w[1] - 2.0).abs() < 1e-4);
        let z = l.zoomed(2.0, [l.rect.x + l.rect.w / 2.0, l.rect.y + l.rect.h / 2.0]);
        assert!((z.width() - 2.5).abs() < 1e-6);
        let pan = l.panned(10.0, 0.0);
        assert!(pan.x0 < l.window.x0);
    }

    #[test]
    fn equal_aspect_makes_units_square() {
        let l = Layout2d::new(600.0, 400.0, Bounds::new(-1.0, 1.0, -1.0, 1.0), true);
        let sx = f64::from(l.rect.w) / l.window.width();
        let sy = f64::from(l.rect.h) / l.window.height();
        assert!((sx - sy).abs() < 1e-9);
        assert!(l.window.x0 <= -1.0 && l.window.y0 <= -1.0);
    }

    #[test]
    fn greek_names() {
        assert_eq!(name_latex("θ"), "\\theta");
        assert_eq!(name_latex("x"), "x");
    }
}
