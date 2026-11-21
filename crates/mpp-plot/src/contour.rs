//! Marching squares for implicit curves `F(x, y) = 0` and level curves, plus the
//! boolean grid behind shaded inequalities.

use std::collections::HashMap;

use crate::sample::P;

/// A rectangle of the plane in world coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub x0: f64,
    pub x1: f64,
    pub y0: f64,
    pub y1: f64,
}

impl Bounds {
    pub fn new(x0: f64, x1: f64, y0: f64, y1: f64) -> Bounds {
        Bounds { x0, x1, y0, y1 }
    }

    pub fn width(&self) -> f64 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> f64 {
        self.y1 - self.y0
    }

    pub fn is_valid(&self) -> bool {
        [self.x0, self.x1, self.y0, self.y1]
            .iter()
            .all(|v| v.is_finite())
            && self.x1 > self.x0
            && self.y1 > self.y0
    }

    pub fn contains(&self, p: P) -> bool {
        p[0] >= self.x0 && p[0] <= self.x1 && p[1] >= self.y0 && p[1] <= self.y1
    }
}

/// Function values on a regular grid of `(nx + 1) × (ny + 1)` corners.
#[derive(Clone, Debug)]
pub struct Grid {
    pub bounds: Bounds,
    pub nx: usize,
    pub ny: usize,
    /// Row-major, `values[j * (nx + 1) + i]`; NaN where undefined.
    pub values: Vec<f64>,
}

impl Grid {
    /// Samples `f` at every corner.
    pub fn sample(
        bounds: Bounds,
        nx: usize,
        ny: usize,
        f: &mut dyn FnMut(f64, f64) -> Option<f64>,
    ) -> Grid {
        let (nx, ny) = (nx.clamp(1, 2000), ny.clamp(1, 2000));
        let mut values = Vec::with_capacity((nx + 1) * (ny + 1));
        for j in 0..=ny {
            let y = bounds.y0 + bounds.height() * j as f64 / ny as f64;
            for i in 0..=nx {
                let x = bounds.x0 + bounds.width() * i as f64 / nx as f64;
                values.push(f(x, y).filter(|v| v.is_finite()).unwrap_or(f64::NAN));
            }
        }
        Grid {
            bounds,
            nx,
            ny,
            values,
        }
    }

    fn at(&self, i: usize, j: usize) -> f64 {
        self.values[j * (self.nx + 1) + i]
    }

    fn pos(&self, i: usize, j: usize) -> P {
        [
            self.bounds.x0 + self.bounds.width() * i as f64 / self.nx as f64,
            self.bounds.y0 + self.bounds.height() * j as f64 / self.ny as f64,
        ]
    }

    /// Smallest and largest finite value.
    pub fn value_range(&self) -> Option<(f64, f64)> {
        let mut it = self.values.iter().copied().filter(|v| v.is_finite());
        let first = it.next()?;
        Some(it.fold((first, first), |(lo, hi), v| (lo.min(v), hi.max(v))))
    }
}

/// A cell edge, identified by the grid corner it starts at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum EdgeKey {
    /// From corner `(i, j)` to `(i + 1, j)`.
    H(usize, usize),
    /// From corner `(i, j)` to `(i, j + 1)`.
    V(usize, usize),
}

/// The level curves `f = level` as chains of points. `f` is used to polish each crossing
/// on the true function, and to drop false crossings at poles.
pub fn contour_lines(
    grid: &Grid,
    level: f64,
    f: &mut dyn FnMut(f64, f64) -> Option<f64>,
) -> Vec<Vec<P>> {
    let mut points: HashMap<EdgeKey, Option<P>> = HashMap::new();
    let mut segments: Vec<(EdgeKey, EdgeKey)> = Vec::new();

    for j in 0..grid.ny {
        for i in 0..grid.nx {
            let v = [
                grid.at(i, j) - level,
                grid.at(i + 1, j) - level,
                grid.at(i + 1, j + 1) - level,
                grid.at(i, j + 1) - level,
            ];
            if v.iter().any(|x| x.is_nan()) {
                continue;
            }
            let mask = v
                .iter()
                .enumerate()
                .fold(0u8, |m, (k, x)| if *x >= 0.0 { m | (1 << k) } else { m });
            let bottom = EdgeKey::H(i, j);
            let top = EdgeKey::H(i, j + 1);
            let left = EdgeKey::V(i, j);
            let right = EdgeKey::V(i + 1, j);
            let centre = (v[0] + v[1] + v[2] + v[3]) / 4.0 > 0.0;
            let pairs: &[(EdgeKey, EdgeKey)] = match mask {
                1 | 14 => &[(left, bottom)],
                2 | 13 => &[(bottom, right)],
                3 | 12 => &[(left, right)],
                4 | 11 => &[(right, top)],
                6 | 9 => &[(bottom, top)],
                7 | 8 => &[(left, top)],
                5 => {
                    if centre {
                        &[(left, top), (bottom, right)]
                    } else {
                        &[(left, bottom), (right, top)]
                    }
                }
                10 => {
                    if centre {
                        &[(left, bottom), (right, top)]
                    } else {
                        &[(left, top), (bottom, right)]
                    }
                }
                _ => &[],
            };
            for &(a, b) in pairs {
                let pa = *points
                    .entry(a)
                    .or_insert_with(|| crossing(grid, a, level, f));
                let pb = *points
                    .entry(b)
                    .or_insert_with(|| crossing(grid, b, level, f));
                if pa.is_some() && pb.is_some() {
                    segments.push((a, b));
                }
            }
        }
    }
    chain(&segments, &points)
}

/// Where the curve crosses a cell edge, polished by bisection on the true function.
fn crossing(
    grid: &Grid,
    edge: EdgeKey,
    level: f64,
    f: &mut dyn FnMut(f64, f64) -> Option<f64>,
) -> Option<P> {
    let ((i0, j0), (i1, j1)) = match edge {
        EdgeKey::H(i, j) => ((i, j), (i + 1, j)),
        EdgeKey::V(i, j) => ((i, j), (i, j + 1)),
    };
    let (pa, pb) = (grid.pos(i0, j0), grid.pos(i1, j1));
    let (fa, fb) = (grid.at(i0, j0) - level, grid.at(i1, j1) - level);
    // A corner exactly on the curve is the crossing.
    if fa == 0.0 {
        return Some(pa);
    }
    if fb == 0.0 {
        return Some(pb);
    }
    // Linear guess, then bisect on the real function.
    let mut t = if fa == fb {
        0.5
    } else {
        (fa / (fa - fb)).clamp(0.0, 1.0)
    };
    let at = |t: f64| [pa[0] + (pb[0] - pa[0]) * t, pa[1] + (pb[1] - pa[1]) * t];
    let (mut lo, mut hi) = (0.0, 1.0);
    let mut flo = fa;
    for _ in 0..10 {
        let p = at(t);
        let v = f(p[0], p[1])? - level;
        if v == 0.0 {
            break;
        }
        if (v >= 0.0) == (flo >= 0.0) {
            lo = t;
            flo = v;
        } else {
            hi = t;
        }
        t = (lo + hi) / 2.0;
    }
    let p = at(t);
    // At a pole the sign flips but the value does not approach zero.
    let residual = f(p[0], p[1])? - level;
    if residual.abs() > 0.25 * (fa - fb).abs() {
        return None;
    }
    Some(p)
}

/// Joins segments that share an edge into polylines.
fn chain(segments: &[(EdgeKey, EdgeKey)], points: &HashMap<EdgeKey, Option<P>>) -> Vec<Vec<P>> {
    let mut at_edge: HashMap<EdgeKey, Vec<usize>> = HashMap::new();
    for (k, (a, b)) in segments.iter().enumerate() {
        at_edge.entry(*a).or_default().push(k);
        at_edge.entry(*b).or_default().push(k);
    }
    let mut used = vec![false; segments.len()];
    let pt = |e: EdgeKey| points.get(&e).copied().flatten();
    let mut lines = Vec::new();
    // Start from open ends first so open curves are one piece, then loops.
    let mut order: Vec<usize> = (0..segments.len()).collect();
    order.sort_by_key(|&k| {
        let (a, b) = segments[k];
        let ends = [a, b]
            .iter()
            .filter(|e| at_edge.get(e).map_or(0, Vec::len) == 1)
            .count();
        usize::from(ends == 0)
    });
    for start in order {
        if used[start] {
            continue;
        }
        used[start] = true;
        let (a, b) = segments[start];
        let mut chain: Vec<EdgeKey> = vec![a, b];
        for forward in [true, false] {
            loop {
                let end = if forward {
                    chain[chain.len() - 1]
                } else {
                    chain[0]
                };
                let next = at_edge
                    .get(&end)
                    .and_then(|v| v.iter().copied().find(|&k| !used[k]));
                let Some(k) = next else { break };
                used[k] = true;
                let (p, q) = segments[k];
                let other = if p == end { q } else { p };
                if forward {
                    chain.push(other);
                } else {
                    chain.insert(0, other);
                }
            }
        }
        let line: Vec<P> = chain.iter().filter_map(|e| pt(*e)).collect();
        if line.len() >= 2 {
            lines.push(line);
        }
    }
    lines
}

/// Which cells of a grid satisfy a condition (sampled at cell centres).
#[derive(Clone, Debug, PartialEq)]
pub struct RegionGrid {
    pub bounds: Bounds,
    pub nx: usize,
    pub ny: usize,
    /// Row-major, `inside[j * nx + i]`.
    pub inside: Vec<bool>,
}

impl RegionGrid {
    pub fn sample(
        bounds: Bounds,
        nx: usize,
        ny: usize,
        holds: &mut dyn FnMut(f64, f64) -> bool,
    ) -> RegionGrid {
        let (nx, ny) = (nx.clamp(1, 2000), ny.clamp(1, 2000));
        let mut inside = Vec::with_capacity(nx * ny);
        for j in 0..ny {
            let y = bounds.y0 + bounds.height() * (j as f64 + 0.5) / ny as f64;
            for i in 0..nx {
                let x = bounds.x0 + bounds.width() * (i as f64 + 0.5) / nx as f64;
                inside.push(holds(x, y));
            }
        }
        RegionGrid {
            bounds,
            nx,
            ny,
            inside,
        }
    }

    /// Runs of consecutive inside cells in row `j`, as `(first, one past last)`.
    pub fn runs(&self, j: usize) -> Vec<(usize, usize)> {
        let row = &self.inside[j * self.nx..(j + 1) * self.nx];
        let mut out = Vec::new();
        let mut start = None;
        for (i, &v) in row.iter().enumerate() {
            match (v, start) {
                (true, None) => start = Some(i),
                (false, Some(s)) => {
                    out.push((s, i));
                    start = None;
                }
                _ => {}
            }
        }
        if let Some(s) = start {
            out.push((s, self.nx));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circle(x: f64, y: f64) -> Option<f64> {
        Some(x * x + y * y)
    }

    #[test]
    fn circle_points_satisfy_the_equation() {
        let b = Bounds::new(-3.0, 3.0, -3.0, 3.0);
        let mut f = circle;
        let grid = Grid::sample(b, 60, 60, &mut f);
        let lines = contour_lines(&grid, 4.0, &mut f);
        assert_eq!(lines.len(), 1, "one closed curve");
        let line = &lines[0];
        assert!(line.len() > 50);
        for p in line {
            let r2 = p[0] * p[0] + p[1] * p[1];
            assert!((r2 - 4.0).abs() < 1e-3, "{p:?} -> {r2}");
        }
        // Closed: first and last points meet.
        let (a, z) = (line[0], line[line.len() - 1]);
        assert!((a[0] - z[0]).hypot(a[1] - z[1]) < 0.2);
    }

    #[test]
    fn hyperbola_has_two_branches() {
        let b = Bounds::new(-4.0, 4.0, -4.0, 4.0);
        let mut f = |x: f64, y: f64| Some(x * x - y * y);
        let grid = Grid::sample(b, 80, 80, &mut f);
        let lines = contour_lines(&grid, 1.0, &mut f);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn poles_do_not_make_false_curves() {
        // y = 1/x as F = x*y - 1 has no pole, but F = y - 1/x does.
        let b = Bounds::new(-4.0, 4.0, -4.0, 4.0);
        let mut f = |x: f64, y: f64| (x != 0.0).then(|| y - 1.0 / x);
        let grid = Grid::sample(b, 81, 81, &mut f);
        let lines = contour_lines(&grid, 0.0, &mut f);
        for line in &lines {
            for p in line {
                assert!((p[1] - 1.0 / p[0]).abs() < 0.05, "{p:?}");
            }
        }
        assert!(!lines.is_empty());
    }

    #[test]
    fn undefined_regions_are_skipped() {
        let b = Bounds::new(-2.0, 2.0, -2.0, 2.0);
        let mut f = |_x: f64, _y: f64| None;
        let grid = Grid::sample(b, 10, 10, &mut f);
        assert!(contour_lines(&grid, 0.0, &mut f).is_empty());
        assert!(grid.value_range().is_none());
    }

    #[test]
    fn region_runs() {
        let b = Bounds::new(0.0, 4.0, 0.0, 1.0);
        let mut holds = |x: f64, _y: f64| !(1.0..2.0).contains(&x);
        let g = RegionGrid::sample(b, 4, 1, &mut holds);
        assert_eq!(g.runs(0), vec![(0, 1), (2, 4)]);
    }
}
