//! The 3D camera: orbit, pan and zoom as a plain struct, and the projection.
//!
//! Points are in the figure's normalised box (`x`, `y` in -1..1, `z` in about
//! -0.8..0.8). `yaw` turns the box about the vertical axis, `pitch` is the elevation.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

/// A projected point: pixel position and distance from the viewer (larger is farther).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Projected {
    pub x: f32,
    pub y: f32,
    pub depth: f64,
}

/// An orbiting camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// Turn about the vertical axis, radians.
    pub yaw: f64,
    /// Elevation above the floor, radians, kept inside (-π/2, π/2).
    pub pitch: f64,
    /// 1.0 fits the box in the viewport.
    pub zoom: f64,
    /// Offset of the picture in pixels.
    pub pan: [f64; 2],
    /// 0 for the orthographic view of textbooks; up to 0.4 for gentle perspective.
    pub perspective: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Camera {
            yaw: -0.62,
            pitch: 0.52,
            zoom: 1.0,
            pan: [0.0, 0.0],
            perspective: 0.0,
        }
    }
}

const MAX_PITCH: f64 = FRAC_PI_2 - 0.02;

impl Camera {
    /// Back to the starting view.
    pub fn reset(&mut self) {
        *self = Camera::default();
    }

    /// Turns the view by a mouse drag of `dx`, `dy` pixels.
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw = wrap(self.yaw - f64::from(dx) * 0.01);
        self.pitch = (self.pitch + f64::from(dy) * 0.01).clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// Slides the picture by `dx`, `dy` pixels.
    pub fn pan_by(&mut self, dx: f32, dy: f32) {
        self.pan[0] += f64::from(dx);
        self.pan[1] += f64::from(dy);
    }

    /// Zooms by a factor (> 1 zooms in).
    pub fn zoom_by(&mut self, factor: f64) {
        if factor.is_finite() && factor > 0.0 {
            self.zoom = (self.zoom * factor).clamp(0.1, 30.0);
        }
    }

    /// A direction in the box as (right, up, away-from-viewer) components.
    pub fn view_vector(&self, v: [f64; 3]) -> [f64; 3] {
        let (sy, cy) = self.yaw.sin_cos();
        let (se, ce) = self.pitch.sin_cos();
        let x1 = v[0] * cy - v[1] * sy;
        let y1 = v[0] * sy + v[1] * cy;
        [x1, v[2] * ce + y1 * se, y1 * ce - v[2] * se]
    }

    /// The unit direction, in the box, that points from the viewer into the scene.
    pub fn forward(&self) -> [f64; 3] {
        let (sy, cy) = self.yaw.sin_cos();
        let (se, ce) = self.pitch.sin_cos();
        [sy * ce, cy * ce, -se]
    }

    /// Pixels per unit of the box in a viewport.
    pub fn scale(&self, width: f32, height: f32) -> f64 {
        0.30 * f64::from(width.min(height)) * self.zoom
    }

    /// Projects a point of the box into a `width` × `height` viewport.
    pub fn project(&self, p: [f64; 3], width: f32, height: f32) -> Projected {
        let [right, up, depth] = self.view_vector(p);
        let k = 1.0 / (1.0 + self.perspective.clamp(0.0, 0.4) * depth);
        let s = self.scale(width, height) * k;
        Projected {
            x: (f64::from(width) / 2.0 + self.pan[0] + s * right) as f32,
            y: (f64::from(height) / 2.0 + self.pan[1] - s * up) as f32,
            depth,
        }
    }
}

fn wrap(a: f64) -> f64 {
    let a = a % TAU;
    if a > PI {
        a - TAU
    } else if a <= -PI {
        a + TAU
    } else {
        a
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VP: (f32, f32) = (640.0, 480.0);

    fn proj(c: &Camera, p: [f64; 3]) -> Projected {
        c.project(p, VP.0, VP.1)
    }

    #[test]
    fn origin_sits_at_the_centre_plus_pan() {
        let mut c = Camera::default();
        let o = proj(&c, [0.0; 3]);
        assert!((o.x - 320.0).abs() < 1e-4 && (o.y - 240.0).abs() < 1e-4);
        c.pan_by(10.0, -5.0);
        let o = proj(&c, [0.0; 3]);
        assert!((o.x - 330.0).abs() < 1e-4 && (o.y - 235.0).abs() < 1e-4);
    }

    #[test]
    fn a_full_turn_changes_nothing() {
        let mut c = Camera::default();
        let before = proj(&c, [0.3, -0.8, 0.5]);
        // 628 px of drag at 0.01 rad per px is almost 2π; finish the turn exactly.
        c.yaw += TAU;
        c.yaw = wrap(c.yaw);
        let after = proj(&c, [0.3, -0.8, 0.5]);
        assert!((before.x - after.x).abs() < 1e-3 && (before.y - after.y).abs() < 1e-3);
    }

    #[test]
    fn parallel_edges_stay_parallel_in_orthographic_view() {
        let c = Camera::default();
        let d = |a: [f64; 3], b: [f64; 3]| {
            let (p, q) = (proj(&c, a), proj(&c, b));
            (q.x - p.x, q.y - p.y)
        };
        let e1 = d([-1.0, -1.0, 0.0], [1.0, -1.0, 0.0]);
        let e2 = d([-1.0, 1.0, 0.5], [1.0, 1.0, 0.5]);
        assert!((e1.0 - e2.0).abs() < 1e-3 && (e1.1 - e2.1).abs() < 1e-3);
    }

    #[test]
    fn zoom_scales_distances_about_the_centre() {
        let mut c = Camera::default();
        let a = proj(&c, [0.5, 0.2, 0.1]);
        c.zoom_by(2.0);
        let b = proj(&c, [0.5, 0.2, 0.1]);
        assert!(((b.x - 320.0) - 2.0 * (a.x - 320.0)).abs() < 1e-3);
        assert!(((b.y - 240.0) - 2.0 * (a.y - 240.0)).abs() < 1e-3);
        let z = c.zoom;
        c.zoom_by(f64::NAN);
        c.zoom_by(-1.0);
        assert_eq!(c.zoom, z);
    }

    #[test]
    fn depth_grows_along_the_forward_direction() {
        let mut c = Camera::default();
        for (yaw, pitch) in [(0.0, 0.0), (1.0, 0.4), (-2.5, -0.7), (3.0, 1.2)] {
            c.yaw = yaw;
            c.pitch = pitch;
            let f = c.forward();
            let norm = f.iter().map(|v| v * v).sum::<f64>().sqrt();
            assert!((norm - 1.0).abs() < 1e-12);
            let p = [0.1, 0.2, 0.3];
            let q = [p[0] + 0.5 * f[0], p[1] + 0.5 * f[1], p[2] + 0.5 * f[2]];
            let (dp, dq) = (proj(&c, p).depth, proj(&c, q).depth);
            assert!((dq - dp - 0.5).abs() < 1e-9, "{yaw} {pitch}");
            // Moving along the view direction does not move the point on screen.
            let (a, b) = (proj(&c, p), proj(&c, q));
            assert!((a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3);
        }
    }

    #[test]
    fn level_view_keeps_verticals_vertical() {
        let c = Camera {
            pitch: 0.0,
            ..Camera::default()
        };
        let (a, b) = (proj(&c, [0.4, 0.4, -0.5]), proj(&c, [0.4, 0.4, 0.5]));
        assert!((a.x - b.x).abs() < 1e-3);
        assert!(b.y < a.y, "up is up the screen");
    }

    #[test]
    fn orbit_clamps_the_pitch_and_wraps_the_yaw() {
        let mut c = Camera::default();
        c.orbit(0.0, 10_000.0);
        assert!(c.pitch < FRAC_PI_2);
        c.orbit(0.0, -100_000.0);
        assert!(c.pitch > -FRAC_PI_2);
        c.orbit(-100_000.0, 0.0);
        assert!(c.yaw > -PI - 1e-9 && c.yaw <= PI + 1e-9);
    }

    #[test]
    fn perspective_shrinks_distant_points() {
        let c = Camera {
            perspective: 0.3,
            ..Camera::default()
        };
        let f = c.forward();
        let near = proj(&c, [-0.5 * f[0] + 0.3, -0.5 * f[1], -0.5 * f[2]]);
        let far = proj(&c, [0.5 * f[0] + 0.3, 0.5 * f[1], 0.5 * f[2]]);
        assert!((near.x - 320.0).abs() > (far.x - 320.0).abs());
    }
}
