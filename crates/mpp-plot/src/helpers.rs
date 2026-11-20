//! The numbers behind the visual calculus helpers: strips, crossings, direction fields,
//! solution curves and solids of revolution. Everything here works on plain closures, so
//! it does not care where the functions came from, and every function returns `None` or
//! an empty result instead of failing.

use std::collections::BTreeMap;
use std::f64::consts::{PI, TAU};

use mpp_core::Expr;

use crate::compile::{eval1, Frame};
use crate::contour::Bounds;
use crate::figure::Mesh;
use crate::sample::P;
use crate::spec::RiemannRule;

/// One strip of a Riemann sum: heights at its left and right edges (equal for a
/// rectangle, different for a trapezoid).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bar {
    pub x0: f64,
    pub x1: f64,
    pub y0: f64,
    pub y1: f64,
}

/// What a field's short strokes mean.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    /// Unit-length strokes along `(1, dy/dx)`.
    Slope,
    /// Arrows whose length follows the size of the vector.
    Arrow,
}

/// One stroke or arrow of a field: where it is centred and its direction in data units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FieldItem {
    pub pos: P,
    pub dir: P,
}

/// A solid of revolution as drawn: surfaces, the lines that outline it and marked points.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Solid {
    pub parts: Vec<SolidPart>,
    pub lines: Vec<Vec<[f64; 3]>>,
    pub dots: Vec<[f64; 3]>,
    /// Whether the parts count for the size of the 3D box.
    pub in_box: bool,
}

/// One piece of a solid.
#[derive(Clone, Debug, PartialEq)]
pub struct SolidPart {
    pub mesh: Mesh,
    /// The slice that shows the method (a disk, a washer or a shell) is drawn in a second colour.
    pub highlight: bool,
    /// Draw the mesh lines (cut faces and slices) rather than a smooth skin.
    pub edged: bool,
}

/// How many pieces the sampled integrals use.
const SIMPSON_INTERVALS: usize = 2000;

/// Formats a number for a read-out: about four significant digits, a real minus sign.
pub fn fmt_num(v: f64) -> String {
    if !v.is_finite() {
        return "undefined".into();
    }
    if v == 0.0 {
        return "0".into();
    }
    let a = v.abs();
    let s = if !(1e-4..1e7).contains(&a) {
        let t = format!("{v:.3e}");
        match t.split_once('e') {
            Some((m, e)) => {
                let m = m.trim_end_matches('0').trim_end_matches('.');
                format!("{m}\u{d7}10^{e}")
            }
            None => t,
        }
    } else {
        let digits = (3.0 - a.log10().floor()).clamp(0.0, 9.0) as usize;
        let t = format!("{v:.digits$}");
        if t.contains('.') {
            t.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            t
        }
    };
    s.replace('-', "\u{2212}")
}

/// A count written with subscript digits (`3` as `₃`).
pub fn subscript(n: usize) -> String {
    n.to_string()
        .chars()
        .filter_map(|c| c.to_digit(10))
        .filter_map(|d| char::from_u32(0x2080 + d))
        .collect()
}

/// The slope of `f` at `x`: the symbolic derivative when there is one that is defined
/// there, otherwise a central difference.
pub fn slope_at(
    f: &Expr,
    df: Option<&Expr>,
    var: &str,
    fixed: &BTreeMap<String, f64>,
    x: f64,
) -> Option<f64> {
    if let Some(v) = df.and_then(|d| eval1(d, fixed, var, x)) {
        return Some(v);
    }
    let h = 1e-6 * x.abs().max(1.0);
    let at = |t: f64| eval1(f, fixed, var, t);
    match (at(x - h), at(x + h)) {
        (Some(a), Some(b)) => Some((b - a) / (2.0 * h)),
        _ => None,
    }
}

// ---- Riemann sums -----------------------------------------------------------------

/// The strips of a Riemann sum over `[a, b]` with `n` strips, and the sum.
pub fn riemann(
    f: &dyn Fn(f64) -> Option<f64>,
    rule: RiemannRule,
    n: usize,
    a: f64,
    b: f64,
) -> Option<(Vec<Bar>, f64)> {
    if !(a.is_finite() && b.is_finite()) || a == b {
        return None;
    }
    let n = n.clamp(1, 2000);
    let dx = (b - a) / n as f64;
    let mut bars = Vec::with_capacity(n);
    let mut sum = 0.0;
    for i in 0..n {
        let (x0, x1) = (a + dx * i as f64, a + dx * (i + 1) as f64);
        let (y0, y1) = match rule {
            RiemannRule::Left => {
                let y = f(x0);
                (y, y)
            }
            RiemannRule::Right => {
                let y = f(x1);
                (y, y)
            }
            RiemannRule::Mid => {
                let y = f((x0 + x1) / 2.0);
                (y, y)
            }
            RiemannRule::Trapezoid => (f(x0), f(x1)),
        };
        let (Some(y0), Some(y1)) = (y0, y1) else {
            continue;
        };
        sum += (y0 + y1) / 2.0 * dx;
        bars.push(Bar { x0, x1, y0, y1 });
    }
    Some((bars, sum))
}

/// Simpson's rule on `[a, b]`; points where `g` is undefined count as zero.
pub fn integrate(g: &dyn Fn(f64) -> Option<f64>, a: f64, b: f64) -> f64 {
    let n = SIMPSON_INTERVALS;
    let h = (b - a) / n as f64;
    let mut s = g(a).unwrap_or(0.0) + g(b).unwrap_or(0.0);
    for i in 1..n {
        let w = if i % 2 == 1 { 4.0 } else { 2.0 };
        s += w * g(a + h * i as f64).unwrap_or(0.0);
    }
    s * h / 3.0
}

// ---- area between curves ----------------------------------------------------------

/// The shaded region between two curves.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct AreaData {
    /// Closed outlines of each piece (the curves cross between pieces).
    pub polys: Vec<Vec<P>>,
    /// The area, `∫ |f − g|`.
    pub area: f64,
    /// Where the curves cross inside the region, as points on the first curve.
    pub crossings: Vec<P>,
    pub bounds: (f64, f64),
}

/// Zeros of `h` on `[a, b]`: sign changes found on a fine grid and refined by bisection.
/// A sign change across an asymptote (where `h` is huge at the "root") is not a zero.
pub fn zeros(h: &dyn Fn(f64) -> Option<f64>, a: f64, b: f64) -> Vec<f64> {
    const CELLS: usize = 1600;
    let mut out: Vec<f64> = Vec::new();
    if !(a.is_finite() && b.is_finite()) || b <= a {
        return out;
    }
    let step = (b - a) / CELLS as f64;
    let push = |out: &mut Vec<f64>, x: f64| {
        if out.last().is_none_or(|l| (x - l).abs() > step * 1e-3) {
            out.push(x);
        }
    };
    let mut prev: Option<(f64, f64)> = None;
    for i in 0..=CELLS {
        let x = if i == CELLS { b } else { a + step * i as f64 };
        let Some(v) = h(x).filter(|v| v.is_finite()) else {
            prev = None;
            continue;
        };
        if v == 0.0 {
            push(&mut out, x);
        } else if let Some((px, pv)) = prev {
            if pv * v < 0.0 {
                let (mut lo, mut hi, mut flo) = (px, x, pv);
                for _ in 0..80 {
                    let mid = 0.5 * (lo + hi);
                    match h(mid) {
                        Some(0.0) => {
                            lo = mid;
                            hi = mid;
                            break;
                        }
                        Some(m) if (m < 0.0) == (flo < 0.0) => {
                            lo = mid;
                            flo = m;
                        }
                        Some(_) => hi = mid,
                        None => break,
                    }
                }
                let root = snap(0.5 * (lo + hi));
                let scale = 1.0 + pv.abs().max(v.abs());
                if h(root).is_some_and(|r| r.abs() <= 1e-6 * scale) {
                    push(&mut out, root);
                }
            }
        }
        prev = Some((x, v));
    }
    out
}

/// Rounds a root that is a short decimal (to nine places), so `1.0000000000000002` is `1`.
fn snap(x: f64) -> f64 {
    let r = (x * 1e9).round() / 1e9;
    if (r - x).abs() < 1e-11 {
        r
    } else {
        x
    }
}

/// The region between `f` and `g` over `bounds`. With no bounds it is the region the
/// curves enclose (from the first crossing to the last in `range`), or all of `range`
/// when they cross fewer than twice.
pub fn area_between(
    f: &dyn Fn(f64) -> Option<f64>,
    g: &dyn Fn(f64) -> Option<f64>,
    bounds: Option<(f64, f64)>,
    range: (f64, f64),
) -> Option<AreaData> {
    let h = |x: f64| Some(f(x)? - g(x)?);
    let (a, b) = match bounds {
        Some((a, b)) => (a.min(b), a.max(b)),
        None => {
            let z = zeros(&h, range.0, range.1);
            match (z.first(), z.last()) {
                (Some(&a), Some(&b)) if z.len() >= 2 => (a, b),
                _ => range,
            }
        }
    };
    if !(a.is_finite() && b.is_finite()) || b <= a {
        return None;
    }
    let mut cuts = vec![a];
    cuts.extend(zeros(&h, a, b).into_iter().filter(|z| *z > a && *z < b));
    cuts.push(b);
    let mut data = AreaData {
        bounds: (a, b),
        ..AreaData::default()
    };
    for w in cuts.windows(2) {
        let (p, q) = (w[0], w[1]);
        if q - p < 1e-12 {
            continue;
        }
        data.area += integrate(&|x| Some(h(x)?.abs()), p, q);
        let m = 120usize;
        let mut top: Vec<P> = Vec::with_capacity(m + 1);
        let mut bottom: Vec<P> = Vec::with_capacity(m + 1);
        for k in 0..=m {
            let x = p + (q - p) * k as f64 / m as f64;
            if let (Some(y1), Some(y2)) = (f(x), g(x)) {
                top.push([x, y1]);
                bottom.push([x, y2]);
            }
        }
        if top.len() >= 2 {
            bottom.reverse();
            top.extend(bottom);
            data.polys.push(top);
        }
    }
    for &x in &cuts {
        if h(x).is_some_and(|v| v.abs() < 1e-7 * (1.0 + f(x).map_or(0.0, f64::abs))) {
            if let Some(y) = f(x) {
                data.crossings.push([x, y]);
            }
        }
    }
    Some(data)
}

// ---- fields and solution curves ---------------------------------------------------

/// Centres of the cells of an `nx` by `ny` grid over the window.
fn grid_centres(w: Bounds, nx: usize, ny: usize) -> Vec<P> {
    let mut out = Vec::with_capacity(nx * ny);
    for j in 0..ny {
        for i in 0..nx {
            out.push([
                w.x0 + w.width() * (i as f64 + 0.5) / nx as f64,
                w.y0 + w.height() * (j as f64 + 0.5) / ny as f64,
            ]);
        }
    }
    out
}

/// The grid size of a field over a window of this shape: about 22 strokes across.
pub fn field_cells(w: Bounds, aspect: f64) -> (usize, usize) {
    let nx = 22usize;
    let ny = ((nx as f64 / aspect.max(0.2)).round() as usize).clamp(8, 30);
    let _ = w;
    (nx, ny)
}

/// Slope strokes of `y' = m(x, y)` over a window.
pub fn slope_items(
    m: &dyn Fn(f64, f64) -> Option<f64>,
    w: Bounds,
    cells: (usize, usize),
) -> Vec<FieldItem> {
    grid_centres(w, cells.0, cells.1)
        .into_iter()
        .filter_map(|p| {
            let s = m(p[0], p[1])?;
            let dir = if s.abs() > 1e8 {
                [0.0, s.signum()]
            } else {
                [1.0, s]
            };
            Some(FieldItem { pos: p, dir })
        })
        .collect()
}

/// Arrows of a vector field over a window.
pub fn arrow_items(
    v: &dyn Fn(f64, f64) -> Option<P>,
    w: Bounds,
    cells: (usize, usize),
) -> Vec<FieldItem> {
    grid_centres(w, cells.0, cells.1)
        .into_iter()
        .filter_map(|p| {
            let d = v(p[0], p[1])?;
            (d[0].is_finite() && d[1].is_finite()).then_some(FieldItem { pos: p, dir: d })
        })
        .collect()
}

/// A solution curve (or trajectory) through `start`, followed in both directions by
/// fourth-order Runge–Kutta steps of equal length on the screen, so steep curves are as
/// smooth as flat ones. The points run from the backward end to the forward end.
pub fn trajectory(dir: &dyn Fn(f64, f64) -> Option<P>, start: P, w: Bounds) -> Vec<P> {
    let (sx, sy) = (w.width(), w.height());
    let ds = 1.0 / 260.0;
    let outside = |p: P| {
        p[0] < w.x0 - 0.05 * sx
            || p[0] > w.x1 + 0.05 * sx
            || p[1] < w.y0 - 0.05 * sy
            || p[1] > w.y1 + 0.05 * sy
    };
    // Unit direction in screen proportions, back in data units.
    let unit = |p: P, sign: f64| -> Option<P> {
        let d = dir(p[0], p[1])?;
        let (u, v) = (d[0] / sx, d[1] / sy);
        let n = u.hypot(v);
        (n.is_finite() && n > 1e-12).then(|| [sign * u / n * sx, sign * v / n * sy])
    };
    let run = |sign: f64| -> Vec<P> {
        let mut out = Vec::new();
        let mut p = start;
        for _ in 0..900 {
            let Some(k1) = unit(p, sign) else { break };
            let at = |q: P, k: P, f: f64| [q[0] + k[0] * ds * f, q[1] + k[1] * ds * f];
            let Some(k2) = unit(at(p, k1, 0.5), sign) else {
                break;
            };
            let Some(k3) = unit(at(p, k2, 0.5), sign) else {
                break;
            };
            let Some(k4) = unit(at(p, k3, 1.0), sign) else {
                break;
            };
            p = [
                p[0] + ds * (k1[0] + 2.0 * k2[0] + 2.0 * k3[0] + k4[0]) / 6.0,
                p[1] + ds * (k1[1] + 2.0 * k2[1] + 2.0 * k3[1] + k4[1]) / 6.0,
            ];
            if outside(p) || !(p[0].is_finite() && p[1].is_finite()) {
                break;
            }
            out.push(p);
        }
        out
    };
    let mut back = run(-1.0);
    back.reverse();
    back.push(start);
    back.extend(run(1.0));
    back
}

/// Starting points for solution curves when the author gave none: a column through the
/// middle of a slope field, a ring around the middle of a phase portrait.
pub fn default_starts(w: Bounds, ring: bool) -> Vec<P> {
    let (cx, cy) = ((w.x0 + w.x1) / 2.0, (w.y0 + w.y1) / 2.0);
    if ring {
        let mut v = Vec::new();
        for (k, r, phase) in [(6usize, 0.85, 0.35), (4, 0.55, 1.0), (3, 0.28, 0.2)] {
            for i in 0..k {
                let t = TAU * i as f64 / k as f64 + phase;
                v.push([
                    cx + r * w.width() / 2.0 * t.cos(),
                    cy + r * w.height() / 2.0 * t.sin(),
                ]);
            }
        }
        v
    } else {
        (1..=7)
            .map(|k| [cx, w.y0 + w.height() * k as f64 / 8.0])
            .collect()
    }
}

// ---- Taylor and Newton ------------------------------------------------------------

/// Taylor coefficients `f⁽ᵏ⁾(a) / k!` from a chain of derivatives (index `k`).
pub fn taylor_coefficients(
    derivs: &[Expr],
    var: &str,
    fixed: &BTreeMap<String, f64>,
    center: f64,
    degree: usize,
) -> Option<Vec<f64>> {
    let mut out = Vec::new();
    let mut fact = 1.0;
    for k in 0..=degree {
        if k > 0 {
            fact *= k as f64;
        }
        let d = derivs.get(k)?;
        out.push(eval1(d, fixed, var, center)? / fact);
    }
    Some(out)
}

/// Evaluates `Σ c_k (x − a)^k`.
pub fn horner(coeffs: &[f64], a: f64, x: f64) -> f64 {
    let t = x - a;
    coeffs.iter().rev().fold(0.0, |acc, c| acc * t + c)
}

/// The iterates of Newton's method: `(xₖ, f(xₖ))` for `k = 0 …`, stopping early when the
/// slope is zero or the values stop being numbers.
pub fn newton_iterates(
    f: &dyn Fn(f64) -> Option<f64>,
    df: &dyn Fn(f64) -> Option<f64>,
    x0: f64,
    steps: usize,
) -> Vec<P> {
    let mut out = Vec::new();
    let mut x = x0;
    for _ in 0..=steps.min(60) {
        let Some(y) = f(x) else { break };
        out.push([x, y]);
        let Some(d) = df(x).filter(|d| d.abs() > 1e-14) else {
            break;
        };
        let next = x - y / d;
        if !next.is_finite() || next.abs() > 1e12 {
            break;
        }
        x = next;
    }
    out
}

// ---- solids of revolution ---------------------------------------------------------

/// How far the solid is turned: three quarters of a turn, so the inside shows.
pub const CUTAWAY: f64 = 1.5 * PI;

/// The volume of the solid: disks and washers about `y = c`, shells about `x = c`.
pub fn volume(
    f: &dyn Fn(f64) -> Option<f64>,
    g: &dyn Fn(f64) -> Option<f64>,
    horizontal: bool,
    c: f64,
    a: f64,
    b: f64,
) -> f64 {
    if horizontal {
        PI * integrate(
            &|x| {
                let (r1, r2) = (f(x)? - c, g(x)? - c);
                Some((r1 * r1 - r2 * r2).abs())
            },
            a,
            b,
        )
    } else {
        TAU * integrate(&|x| Some((x - c).abs() * (f(x)? - g(x)?).abs()), a, b)
    }
}

/// Builds the surfaces of the solid made by turning the region between `f` and `g` over
/// `[a, b]` about `y = c` (`horizontal`) or `x = c`. Three quarters of the solid is drawn,
/// with the cut faces showing the region, and one slice (a disk or washer, or a shell) in
/// a second colour.
pub fn revolve(
    f: &dyn Fn(f64) -> Option<f64>,
    g: &dyn Fn(f64) -> Option<f64>,
    horizontal: bool,
    c: f64,
    a: f64,
    b: f64,
) -> Solid {
    let rot = |x: f64, y: f64, phi: f64| -> [f64; 3] {
        let (s, co) = phi.sin_cos();
        if horizontal {
            [x, (y - c) * s, c + (y - c) * co]
        } else {
            [c + (x - c) * co, (x - c) * s, y]
        }
    };
    let nan = [f64::NAN; 3];
    let (nx, nphi) = (64usize, 56usize);
    let xs: Vec<f64> = (0..=nx)
        .map(|i| a + (b - a) * i as f64 / nx as f64)
        .collect();
    let phis: Vec<f64> = (0..=nphi)
        .map(|j| CUTAWAY * j as f64 / nphi as f64)
        .collect();
    let mut solid = Solid {
        in_box: true,
        ..Solid::default()
    };

    // The outer and inner surfaces.
    let surface = |curve: &dyn Fn(f64) -> Option<f64>| -> Mesh {
        let mut points = Vec::with_capacity((nx + 1) * (nphi + 1));
        for &x in &xs {
            for &phi in &phis {
                points.push(curve(x).map_or(nan, |y| rot(x, y, phi)));
            }
        }
        Mesh {
            nu: nphi + 1,
            nv: nx + 1,
            points,
        }
    };
    solid.parts.push(SolidPart {
        mesh: surface(f),
        highlight: false,
        edged: false,
    });
    let g_is_axis = horizontal
        && (0..=4).all(|k| {
            let x = a + (b - a) * f64::from(k) / 4.0;
            g(x).is_some_and(|y| (y - c).abs() < 1e-12)
        });
    if !g_is_axis {
        solid.parts.push(SolidPart {
            mesh: surface(g),
            highlight: false,
            edged: false,
        });
    }

    // A strip across the region at `x`, turned through `phis`: an end cap, or a slice.
    let across = |x: f64, phis: &[f64]| -> Mesh {
        let steps = 6usize;
        let mut points = Vec::with_capacity((steps + 1) * phis.len());
        for k in 0..=steps {
            let t = k as f64 / steps as f64;
            for &phi in phis {
                points.push(match (f(x), g(x)) {
                    (Some(y1), Some(y2)) => rot(x, y2 + t * (y1 - y2), phi),
                    _ => nan,
                });
            }
        }
        Mesh {
            nu: phis.len(),
            nv: steps + 1,
            points,
        }
    };
    for x in [a, b] {
        solid.parts.push(SolidPart {
            mesh: across(x, &phis),
            highlight: false,
            edged: false,
        });
    }

    // The two cut faces: the region itself at the start and the end of the turn.
    let cut = |phi: f64| -> Mesh {
        let steps = 6usize;
        let mut points = Vec::with_capacity((nx + 1) * (steps + 1));
        for k in 0..=steps {
            let t = k as f64 / steps as f64;
            for &x in &xs {
                points.push(match (f(x), g(x)) {
                    (Some(y1), Some(y2)) => rot(x, y2 + t * (y1 - y2), phi),
                    _ => nan,
                });
            }
        }
        Mesh {
            nu: nx + 1,
            nv: steps + 1,
            points,
        }
    };
    for phi in [0.0, CUTAWAY] {
        solid.parts.push(SolidPart {
            mesh: cut(phi),
            highlight: false,
            edged: true,
        });
    }

    // The slice that shows the method.
    let xm = (a + b) / 2.0;
    let full: Vec<f64> = (0..=nphi).map(|j| TAU * j as f64 / nphi as f64).collect();
    solid.parts.push(SolidPart {
        mesh: across(xm, &full),
        highlight: true,
        edged: true,
    });

    // The axis of revolution.
    let pad = 0.12 * (b - a);
    solid.lines.push(if horizontal {
        vec![[a - pad, 0.0, c], [b + pad, 0.0, c]]
    } else {
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for &x in &xs {
            for y in [f(x), g(x)].into_iter().flatten() {
                lo = lo.min(y);
                hi = hi.max(y);
            }
        }
        let room = 0.12 * (hi - lo).abs();
        vec![[c, 0.0, lo - room], [c, 0.0, hi + room]]
    });
    solid
}

/// The plane touching `z = f(x, y)` at `(x0, y0)`, as a mesh over a square of half-width
/// `half` around the point, with the heights kept inside `z_range`.
pub fn tangent_plane(z0: f64, grad: [f64; 2], at: P, half: [f64; 2], z_range: (f64, f64)) -> Mesh {
    let n = 24usize;
    let mut points = Vec::with_capacity((n + 1) * (n + 1));
    for j in 0..=n {
        let y = at[1] - half[1] + 2.0 * half[1] * j as f64 / n as f64;
        for i in 0..=n {
            let x = at[0] - half[0] + 2.0 * half[0] * i as f64 / n as f64;
            let z = z0 + grad[0] * (x - at[0]) + grad[1] * (y - at[1]);
            let z = if z >= z_range.0 && z <= z_range.1 {
                z
            } else {
                f64::NAN
            };
            points.push([x, y, z]);
        }
    }
    Mesh {
        nu: n + 1,
        nv: n + 1,
        points,
    }
}

/// Evaluates a function of two variables.
pub fn eval_xy(
    e: &Expr,
    fixed: &BTreeMap<String, f64>,
    names: [&str; 2],
    x: f64,
    y: f64,
) -> Option<f64> {
    crate::compile::eval_real(
        e,
        &Frame {
            fixed,
            vars: &[(names[0], x), (names[1], y)],
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_read_well() {
        assert_eq!(fmt_num(2.0), "2");
        assert_eq!(fmt_num(-0.5), "\u{2212}0.5");
        assert_eq!(fmt_num(std::f64::consts::PI), "3.142");
        assert_eq!(fmt_num(1234.5678), "1235");
        assert_eq!(fmt_num(0.0), "0");
        assert_eq!(fmt_num(f64::NAN), "undefined");
        assert_eq!(subscript(12), "\u{2081}\u{2082}");
    }

    #[test]
    fn left_sum_of_x_squared() {
        let f = |x: f64| Some(x * x);
        let (bars, sum) = riemann(&f, RiemannRule::Left, 4, 0.0, 2.0).unwrap();
        assert_eq!(bars.len(), 4);
        // 0.5 (0 + 0.25 + 1 + 2.25)
        assert!((sum - 1.75).abs() < 1e-12);
        let (_, mid) = riemann(&f, RiemannRule::Mid, 4, 0.0, 2.0).unwrap();
        assert!((mid - 2.625).abs() < 1e-12);
        let (_, trap) = riemann(&f, RiemannRule::Trapezoid, 4, 0.0, 2.0).unwrap();
        assert!((trap - 2.75).abs() < 1e-12);
    }

    #[test]
    fn simpson_is_exact_for_cubics() {
        let v = integrate(&|x| Some(x * x * x), 0.0, 2.0);
        assert!((v - 4.0).abs() < 1e-9);
    }

    #[test]
    fn zeros_skip_asymptotes() {
        let z = zeros(&|x| Some(x * x - 1.0), -3.0, 3.0);
        assert_eq!(z.len(), 2);
        assert!((z[0] + 1.0).abs() < 1e-9 && (z[1] - 1.0).abs() < 1e-9);
        // 1/x changes sign at 0 but has no zero.
        assert!(zeros(&|x| Some(1.0 / x), -1.0, 1.0).is_empty());
    }

    #[test]
    fn area_between_a_parabola_and_a_line() {
        let f = |x: f64| Some(x);
        let g = |x: f64| Some(x * x);
        let a = area_between(&f, &g, None, (-3.0, 3.0)).unwrap();
        assert!((a.area - 1.0 / 6.0).abs() < 1e-9);
        assert_eq!(a.polys.len(), 1);
        assert_eq!(a.crossings.len(), 2);
        assert_eq!(a.bounds.0, 0.0);
    }

    #[test]
    fn trajectories_follow_the_field() {
        // y' = y has solutions c e^x; through (0, 1) the curve passes (1, e).
        let w = Bounds::new(-2.0, 2.0, -1.0, 8.0);
        let t = trajectory(&|_, y| Some([1.0, y]), [0.0, 1.0], w);
        let near = t
            .iter()
            .min_by(|p, q| (p[0] - 1.0).abs().total_cmp(&(q[0] - 1.0).abs()))
            .unwrap();
        assert!((near[1] - std::f64::consts::E).abs() < 0.02, "{near:?}");
        assert!(t.windows(2).all(|p| p[1][0] > p[0][0] - 1e-9));
    }

    #[test]
    fn newton_finds_the_root_of_x_squared_minus_two() {
        let it = newton_iterates(&|x| Some(x * x - 2.0), &|x| Some(2.0 * x), 2.0, 6);
        let last = it.last().unwrap();
        assert!((last[0] - 2f64.sqrt()).abs() < 1e-9);
        assert_eq!(it[0], [2.0, 2.0]);
    }

    #[test]
    fn volumes_of_revolution() {
        // y = x on [0, 1] about the x axis: a cone, pi/3.
        let id = |x: f64| Some(x);
        let zero = |_: f64| Some(0.0);
        let v = volume(&id, &zero, true, 0.0, 0.0, 1.0);
        assert!((v - PI / 3.0).abs() < 1e-9);
        // The same region as shells about the y axis: 2 pi int x (x) dx = 2 pi / 3.
        let s = volume(&id, &zero, false, 0.0, 0.0, 1.0);
        assert!((s - 2.0 * PI / 3.0).abs() < 1e-9);
        // Washer: y = 2 over y = x on [0, 1]: pi int (4 - x^2) = pi (4 - 1/3).
        let two = |_: f64| Some(2.0);
        let w = volume(&two, &id, true, 0.0, 0.0, 1.0);
        assert!((w - PI * (4.0 - 1.0 / 3.0)).abs() < 1e-9);
    }

    #[test]
    fn washer_rings_are_a_ring_of_radii() {
        let two = |_: f64| Some(2.0);
        let id = |x: f64| Some(x);
        let s = revolve(&two, &id, true, 0.0, 0.0, 1.0);
        // The highlighted slice at x = 0.5 runs from radius 0.5 to 2.
        let slice = s.parts.iter().find(|p| p.highlight).unwrap();
        let radius = |p: &[f64; 3]| p[1].hypot(p[2]);
        let radii: Vec<f64> = slice.mesh.points.iter().map(radius).collect();
        let lo = radii.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = radii.iter().copied().fold(0.0, f64::max);
        assert!((lo - 0.5).abs() < 1e-9 && (hi - 2.0).abs() < 1e-9);
        assert!(slice.mesh.points.iter().all(|p| (p[0] - 0.5).abs() < 1e-12));
    }

    #[test]
    fn horner_matches_the_polynomial() {
        // 1 + 2 (x - 1) + 3 (x - 1)^2 at x = 3 is 1 + 4 + 12.
        assert!((horner(&[1.0, 2.0, 3.0], 1.0, 3.0) - 17.0).abs() < 1e-12);
    }
}
