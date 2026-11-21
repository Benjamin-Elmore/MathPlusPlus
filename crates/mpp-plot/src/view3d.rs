//! Drawing a 3D figure: depth-sorted shaded triangles, an axes box and ticks.

use crate::camera::{Camera, Projected};
use crate::color::Color;
use crate::figure::{Figure, LayerData, Mesh};
use crate::scene::{
    Anchor, Dash, Item, Label, Marker, MarkerKind, PlotScene, Polyline, Pt, Triangle,
};
use crate::spec::Geometry;
use crate::theme::PlotTheme;
use crate::ticks::{format_tick, ticks};
use crate::view2d::{draw_readouts, math_italic, name_latex};

/// Height of the box relative to its width and depth.
const BOX_Z: f64 = 0.8;

/// How surfaces are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceStyle {
    /// Filled and shaded, coloured by height.
    Shaded,
    /// Filled and shaded, with the mesh lines on top.
    ShadedWire,
    /// Mesh lines only (hidden lines are covered by the background fill).
    Wire,
}

/// What to draw besides the surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options3d {
    pub style: SurfaceStyle,
    /// The box, floor grid, ticks and axis names.
    pub axes: bool,
}

impl Default for Options3d {
    fn default() -> Self {
        Options3d {
            style: SurfaceStyle::Shaded,
            axes: true,
        }
    }
}

/// The light, in (right, up, towards the viewer) components.
const LIGHT: [f64; 3] = [-0.42, 0.56, 0.71];

impl Figure {
    /// The camera a 3D figure starts with.
    pub fn default_camera(&self) -> Camera {
        Camera::default()
    }

    /// Draws with the default camera and options.
    pub fn scene(&self, width: f32, height: f32, theme: &PlotTheme) -> PlotScene {
        if self.is_3d() {
            self.scene_3d(&self.default_camera(), width, height, theme)
        } else {
            self.scene_2d(width, height, theme)
        }
    }

    /// Draws a 3D figure from a camera.
    pub fn scene_3d(&self, cam: &Camera, width: f32, height: f32, theme: &PlotTheme) -> PlotScene {
        self.scene_3d_with(cam, width, height, theme, Options3d::default())
    }

    /// Maps a data point into the normalised box.
    pub fn normalize(&self, p: [f64; 3]) -> [f64; 3] {
        let h = self.half();
        let n = |v: f64, (lo, hi): (f64, f64)| 2.0 * (v - lo) / (hi - lo) - 1.0;
        [
            n(p[0], self.box3[0]) * h[0],
            n(p[1], self.box3[1]) * h[1],
            n(p[2], self.box3[2]) * h[2],
        ]
    }

    /// Draws a 3D figure with options.
    pub fn scene_3d_with(
        &self,
        cam: &Camera,
        width: f32,
        height: f32,
        theme: &PlotTheme,
        opts: Options3d,
    ) -> PlotScene {
        let (width, height) = (width.max(40.0), height.max(40.0));
        let mut scene = PlotScene::new(
            width,
            height,
            theme.background,
            &theme.font,
            &theme.math_font,
        );
        let proj = |p: [f64; 3]| cam.project(p, width, height);
        if opts.axes {
            self.draw_box(&mut scene, cam, &proj, theme);
        }

        // Surfaces, sorted far to near.
        let mut tris: Vec<(f64, Triangle)> = Vec::new();
        // A plane laid on a surface is drawn over it, so it shows even where it dips below.
        let mut overlay: Vec<(f64, Triangle)> = Vec::new();
        for (i, d) in self.data.iter().enumerate() {
            if let LayerData::Surface(mesh) = d {
                self.mesh_triangles(
                    mesh,
                    cam,
                    &proj,
                    theme,
                    opts.style,
                    theme.series(i),
                    &mut tris,
                );
            }
            if let LayerData::Solid(solid) = d {
                for part in &solid.parts {
                    let base = match (part.highlight, solid.in_box) {
                        (true, true) => theme.series(i + 1),
                        (true, false) => theme.series(i).with_alpha(170),
                        (false, _) => theme.series(i).lerp(theme.background, 0.18),
                    };
                    let target = if solid.in_box {
                        &mut tris
                    } else {
                        &mut overlay
                    };
                    self.flat_triangles(&part.mesh, cam, &proj, theme, (base, part.edged), target);
                }
            }
        }
        tris.sort_by(|a, b| b.0.total_cmp(&a.0));
        overlay.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (_, t) in tris.into_iter().chain(overlay) {
            scene.push(Item::Triangle(t));
        }

        // Solid outlines and marked points.
        for (i, d) in self.data.iter().enumerate() {
            if let LayerData::Solid(solid) = d {
                for run in &solid.lines {
                    let points: Vec<Pt> = run
                        .iter()
                        .filter(|p| p.iter().all(|v| v.is_finite()))
                        .map(|p| {
                            let q = proj(self.normalize(*p));
                            [q.x, q.y]
                        })
                        .collect();
                    scene.push(Item::Polyline(Polyline {
                        points,
                        color: theme.axes,
                        width: 1.6,
                        dash: Dash::Dashed(6.0, 4.0),
                    }));
                }
                for p in &solid.dots {
                    let q = proj(self.normalize(*p));
                    scene.push(Item::Marker(Marker {
                        pos: [q.x, q.y],
                        kind: MarkerKind::Circle,
                        color: theme.series(i),
                        size: 9.0,
                        filled: true,
                        hollow_fill: theme.background,
                    }));
                }
            }
        }

        // Curves.
        for (i, d) in self.data.iter().enumerate() {
            if let LayerData::Curve3(runs) = d {
                for run in runs {
                    let points: Vec<Pt> = run
                        .iter()
                        .map(|p| {
                            let q = proj(self.normalize(*p));
                            [q.x, q.y]
                        })
                        .collect();
                    scene.push(Item::Polyline(Polyline {
                        points,
                        color: theme.series(i),
                        width: 2.4,
                        dash: Dash::Solid,
                    }));
                }
            }
        }
        if opts.axes {
            self.draw_ticks(&mut scene, &proj, theme);
        }
        draw_readouts(&mut scene, theme, self.readouts(), 8.0, 8.0);
        scene
    }

    #[allow(clippy::too_many_arguments)]
    fn mesh_triangles(
        &self,
        mesh: &Mesh,
        cam: &Camera,
        proj: &dyn Fn([f64; 3]) -> Projected,
        theme: &PlotTheme,
        style: SurfaceStyle,
        wire_color: Color,
        out: &mut Vec<(f64, Triangle)>,
    ) {
        let (zlo, zhi) = self.box3[2];
        let height_t = |z: f64| {
            if zhi > zlo {
                (z - zlo) / (zhi - zlo)
            } else {
                0.5
            }
        };
        // Project every vertex once.
        let verts: Vec<Option<(Projected, [f64; 3], f64)>> = mesh
            .points
            .iter()
            .map(|p| {
                if p.iter().all(|v| v.is_finite()) {
                    let n = self.normalize(*p);
                    Some((proj(n), n, height_t(p[2])))
                } else {
                    None
                }
            })
            .collect();
        let at = |u: usize, v: usize| verts[v * mesh.nu + u];
        let edge = match style {
            SurfaceStyle::Shaded => None,
            SurfaceStyle::ShadedWire => Some((theme.wireframe, 0.5)),
            SurfaceStyle::Wire => Some((wire_color, 0.8)),
        };
        for v in 0..mesh.nv.saturating_sub(1) {
            for u in 0..mesh.nu.saturating_sub(1) {
                let quad = [at(u, v), at(u + 1, v), at(u + 1, v + 1), at(u, v + 1)];
                for tri in [[0usize, 1, 2], [0, 2, 3]] {
                    let (Some(a), Some(b), Some(c)) = (quad[tri[0]], quad[tri[1]], quad[tri[2]])
                    else {
                        continue;
                    };
                    let shade = lambert(cam, a.1, b.1, c.1);
                    let colors = [a, b, c].map(|(_, _, t)| match style {
                        SurfaceStyle::Wire => theme.background,
                        _ => theme.colormap(t).scaled(shade),
                    });
                    out.push((
                        (a.0.depth + b.0.depth + c.0.depth) / 3.0,
                        Triangle {
                            points: [[a.0.x, a.0.y], [b.0.x, b.0.y], [c.0.x, c.0.y]],
                            colors,
                            edge,
                        },
                    ));
                }
            }
        }
    }

    /// Triangles of a mesh in one colour, shaded by the angle to the light.
    fn flat_triangles(
        &self,
        mesh: &Mesh,
        cam: &Camera,
        proj: &dyn Fn([f64; 3]) -> Projected,
        theme: &PlotTheme,
        paint: (Color, bool),
        out: &mut Vec<(f64, Triangle)>,
    ) {
        let (base, edged) = paint;
        let verts: Vec<Option<(Projected, [f64; 3])>> = mesh
            .points
            .iter()
            .map(|p| {
                p.iter().all(|v| v.is_finite()).then(|| {
                    let n = self.normalize(*p);
                    (proj(n), n)
                })
            })
            .collect();
        let at = |u: usize, v: usize| verts.get(v * mesh.nu + u).copied().flatten();
        let edge = edged.then_some((theme.axes.with_alpha(110), 0.6));
        for v in 0..mesh.nv.saturating_sub(1) {
            for u in 0..mesh.nu.saturating_sub(1) {
                let quad = [at(u, v), at(u + 1, v), at(u + 1, v + 1), at(u, v + 1)];
                for tri in [[0usize, 1, 2], [0, 2, 3]] {
                    let (Some(a), Some(b), Some(c)) = (quad[tri[0]], quad[tri[1]], quad[tri[2]])
                    else {
                        continue;
                    };
                    let colour = base.scaled(lambert(cam, a.1, b.1, c.1));
                    out.push((
                        (a.0.depth + b.0.depth + c.0.depth) / 3.0,
                        Triangle {
                            points: [[a.0.x, a.0.y], [b.0.x, b.0.y], [c.0.x, c.0.y]],
                            colors: [colour; 3],
                            edge,
                        },
                    ));
                }
            }
        }
    }

    /// Half the box size along each axis: a flat-topped box for `z = f(x, y)`, and one in
    /// proportion to the data for curves and parametric surfaces so a sphere stays round.
    pub fn half(&self) -> [f64; 3] {
        let has_surface = self
            .spec
            .layers
            .iter()
            .any(|l| matches!(l.geometry, Geometry::Surface { .. }));
        if has_surface {
            return [1.0, 1.0, BOX_Z];
        }
        let ext = self.box3.map(|(lo, hi)| (hi - lo).abs());
        let m = ext.iter().copied().fold(0.0, f64::max);
        if m <= 0.0 {
            return [1.0, 1.0, BOX_Z];
        }
        ext.map(|e| (e / m).max(0.12))
    }

    /// The twelve edges of the box and the floor grid.
    fn draw_box(
        &self,
        scene: &mut PlotScene,
        _cam: &Camera,
        proj: &dyn Fn([f64; 3]) -> Projected,
        theme: &PlotTheme,
    ) {
        let h = self.half();
        let line = |a: [f64; 3], b: [f64; 3]| -> Vec<Pt> {
            let (p, q) = (proj(a), proj(b));
            vec![[p.x, p.y], [q.x, q.y]]
        };
        let z0 = -h[2];
        // Floor grid.
        for k in 0..2 {
            let (t, _) = ticks(self.box3[k].0, self.box3[k].1, 5);
            for v in t {
                let n =
                    (2.0 * (v - self.box3[k].0) / (self.box3[k].1 - self.box3[k].0) - 1.0) * h[k];
                let (a, b) = if k == 0 {
                    ([n, -h[1], z0], [n, h[1], z0])
                } else {
                    ([-h[0], n, z0], [h[0], n, z0])
                };
                scene.line(line(a, b), theme.grid, 1.0);
            }
        }
        let corners = |i: usize| {
            [
                if i & 1 == 0 { -h[0] } else { h[0] },
                if i & 2 == 0 { -h[1] } else { h[1] },
                if i & 4 == 0 { -h[2] } else { h[2] },
            ]
        };
        for i in 0..8usize {
            for bit in [1usize, 2, 4] {
                if i & bit == 0 {
                    scene.line(line(corners(i), corners(i | bit)), theme.legend_border, 1.0);
                }
            }
        }
    }

    /// Ticks and names on the box edges nearest the viewer.
    fn draw_ticks(
        &self,
        scene: &mut PlotScene,
        proj: &dyn Fn([f64; 3]) -> Projected,
        theme: &PlotTheme,
    ) {
        let h = self.half();
        let z0 = -h[2];
        let centre = proj([0.0, 0.0, 0.0]);
        let names = self.axis_names_3d();
        // Which side of the floor is nearest: the one with the smaller depth.
        let near = |a: [f64; 3], b: [f64; 3]| proj(a).depth <= proj(b).depth;
        let ey = if near([0.0, -h[1], z0], [0.0, h[1], z0]) {
            -h[1]
        } else {
            h[1]
        };
        let ex = if near([-h[0], 0.0, z0], [h[0], 0.0, z0]) {
            -h[0]
        } else {
            h[0]
        };
        // Outward direction on screen for a point of the box.
        let out_dir = |p: [f64; 3]| {
            let q = proj(p);
            let (dx, dy) = (q.x - centre.x, q.y - centre.y);
            let n = dx.hypot(dy);
            if n > 1e-3 {
                [dx / n, dy / n]
            } else {
                [0.0, 1.0]
            }
        };
        let label = |pos: Pt, text: &str, anchor: Anchor, latex: Option<String>, size: f32| Label {
            pos,
            text: text.to_string(),
            latex,
            anchor,
            size,
            color: theme.text,
            rotation: 0.0,
            italic: false,
        };
        let anchor_for = |d: [f32; 2]| {
            if d[0] > 0.35 {
                Anchor::Start
            } else if d[0] < -0.35 {
                Anchor::End
            } else {
                Anchor::Middle
            }
        };
        // Tick labels already placed, so two never print over each other.
        let mut placed: Vec<Pt> = Vec::new();
        let crowded = |placed: &mut Vec<Pt>, pos: Pt| {
            if placed
                .iter()
                .any(|o| (o[0] - pos[0]).abs() < 26.0 && (o[1] - pos[1]).abs() < 12.0)
            {
                return true;
            }
            placed.push(pos);
            false
        };
        for (k, axis_name) in names.iter().enumerate().take(2) {
            let (lo, hi) = self.box3[k];
            let (t, step) = ticks(lo, hi, 5);
            for v in &t {
                let n = (2.0 * (v - lo) / (hi - lo) - 1.0) * h[k];
                let p = if k == 0 { [n, ey, z0] } else { [ex, n, z0] };
                let d = out_dir(p);
                let q = proj(p);
                let pos = [q.x + d[0] * 9.0, q.y + d[1] * 9.0 + 4.0];
                if crowded(&mut placed, pos) {
                    continue;
                }
                scene.line(
                    vec![[q.x, q.y], [q.x + d[0] * 5.0, q.y + d[1] * 5.0]],
                    theme.axes,
                    1.0,
                );
                scene.text(label(
                    pos,
                    &format_tick(*v, step),
                    anchor_for(d),
                    None,
                    11.0,
                ));
            }
            let mid = if k == 0 { [0.0, ey, z0] } else { [ex, 0.0, z0] };
            let d = out_dir(mid);
            let q = proj(mid);
            let pos = [q.x + d[0] * 34.0, q.y + d[1] * 34.0 + 5.0];
            scene.text(Label {
                italic: false,
                ..label(
                    pos,
                    &math_italic(axis_name),
                    Anchor::Middle,
                    Some(name_latex(axis_name)),
                    14.0,
                )
            });
        }
        // The vertical axis goes on the leftmost vertical edge.
        let mut best = (f32::INFINITY, [-h[0], -h[1]]);
        for cx in [-h[0], h[0]] {
            for cy in [-h[1], h[1]] {
                let q = proj([cx, cy, z0]);
                if q.x < best.0 {
                    best = (q.x, [cx, cy]);
                }
            }
        }
        let [cx, cy] = best.1;
        let (lo, hi) = self.box3[2];
        let (t, step) = ticks(lo, hi, 5);
        for v in &t {
            let n = (2.0 * (v - lo) / (hi - lo) - 1.0) * h[2];
            let q = proj([cx, cy, n]);
            let pos = [q.x - 9.0, q.y + 4.0];
            if crowded(&mut placed, pos) {
                continue;
            }
            scene.line(vec![[q.x, q.y], [q.x - 5.0, q.y]], theme.axes, 1.0);
            scene.text(label(pos, &format_tick(*v, step), Anchor::End, None, 11.0));
        }
        let top = proj([cx, cy, h[2]]);
        scene.text(Label {
            italic: false,
            ..label(
                [top.x - 10.0, top.y - 12.0],
                &math_italic(&names[2]),
                Anchor::End,
                Some(name_latex(&names[2])),
                14.0,
            )
        });
    }

    /// Names of the three axes.
    pub fn axis_names_3d(&self) -> [String; 3] {
        for l in &self.spec.layers {
            if let Geometry::Surface { vars, .. } = &l.geometry {
                return [vars[0].clone(), vars[1].clone(), "z".into()];
            }
        }
        ["x".into(), "y".into(), "z".into()]
    }
}

/// Brightness factor from the angle between a face and the light (two-sided).
fn lambert(cam: &Camera, a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f32 {
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len < 1e-12 {
        return 1.0;
    }
    let n = cam.view_vector([n[0] / len, n[1] / len, n[2] / len]);
    // view_vector's third component points away from the viewer; the light is towards it.
    let d = (n[0] * LIGHT[0] + n[1] * LIGHT[1] - n[2] * LIGHT[2]).abs();
    (0.62 + 0.5 * d) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_towards_the_light_are_brighter() {
        let cam = Camera {
            yaw: 0.0,
            pitch: 0.0,
            ..Camera::default()
        };
        // A face whose normal points at the viewer (-y in the box at yaw 0).
        let facing = lambert(&cam, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
        // An edge-on face.
        let edge_on = lambert(&cam, [0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
        assert!(facing >= 0.62 && edge_on >= 0.62);
        assert!(facing > edge_on - 0.2);
    }
}
