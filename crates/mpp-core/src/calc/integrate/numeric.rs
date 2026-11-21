//! Numerical integration: globally adaptive Gauss–Kronrod 7–15 quadrature, with changes of
//! variable for infinite ranges and for singular endpoints.

use std::collections::BTreeMap;

use crate::error::{MathError, MathResult};
use crate::expr::Expr;

/// Default requested accuracy (absolute and relative).
pub const DEFAULT_TOLERANCE: f64 = 1e-10;
/// Most subintervals one quadrature may use.
const MAX_INTERVALS: usize = 2_000;

/// Kronrod nodes on [−1, 1] (non-negative half; the Gauss nodes are the odd entries).
const XGK: [f64; 8] = [
    0.991_455_371_120_812_6,
    0.949_107_912_342_758_5,
    0.864_864_423_359_769_1,
    0.741_531_185_599_394_4,
    0.586_087_235_467_691_1,
    0.405_845_151_377_397_2,
    0.207_784_955_007_898_5,
    0.0,
];
/// Kronrod weights.
const WGK: [f64; 8] = [
    0.022_935_322_010_529_22,
    0.063_092_092_629_978_55,
    0.104_790_010_322_250_2,
    0.140_653_259_715_525_9,
    0.169_004_726_639_267_9,
    0.190_350_578_064_785_4,
    0.204_432_940_075_298_9,
    0.209_482_141_084_727_8,
];
/// Gauss weights for the nodes `XGK[1]`, `XGK[3]`, `XGK[5]` and the centre.
const WG: [f64; 4] = [
    0.129_484_966_168_869_7,
    0.279_705_391_489_276_7,
    0.381_830_050_505_118_9,
    0.417_959_183_673_469_4,
];

/// One Gauss–Kronrod panel: `(Kronrod estimate, error estimate, estimate of ∫|f|)`, or
/// `None` when the integrand is not finite at a node.
fn gk15(f: &mut dyn FnMut(f64) -> Option<f64>, a: f64, b: f64) -> Option<(f64, f64, f64)> {
    let c = 0.5 * (a + b);
    let h = 0.5 * (b - a);
    let fc = f(c)?;
    let mut kronrod = WGK[7] * fc;
    let mut gauss = WG[3] * fc;
    let mut size = WGK[7] * fc.abs();
    for (j, x) in XGK.iter().enumerate().take(7) {
        let dx = h * x;
        let f1 = f(c - dx)?;
        let f2 = f(c + dx)?;
        kronrod += WGK[j] * (f1 + f2);
        size += WGK[j] * (f1.abs() + f2.abs());
        if j % 2 == 1 {
            gauss += WG[j / 2] * (f1 + f2);
        }
    }
    let k = kronrod * h;
    let g = gauss * h;
    if !k.is_finite() {
        return None;
    }
    // Rounding noise scales with ∫|f|, which matters when the values cancel.
    let abs = size * h.abs();
    Some((k, (k - g).abs().max(4e-15 * abs), abs))
}

#[derive(Clone, Copy)]
struct Panel {
    a: f64,
    b: f64,
    value: f64,
    err: f64,
    abs: f64,
}

/// A numerical integral: its value, an error estimate, and an estimate of `∫|f|` over
/// the same range (the scale that rounding and cancellation are measured against).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Quad {
    pub value: f64,
    pub error: f64,
    pub abs: f64,
}

impl Quad {
    fn add(self, o: Quad) -> Quad {
        Quad {
            value: self.value + o.value,
            error: self.error + o.error,
            abs: self.abs + o.abs,
        }
    }
}

/// Adaptive quadrature of `f` over the finite interval `[a, b]`, to the accuracy `tol`,
/// absolute or relative to `∫|f|` (for one piece of a larger sum).
pub(crate) fn adaptive(
    f: &mut dyn FnMut(f64) -> Option<f64>,
    a: f64,
    b: f64,
    tol: f64,
    max_intervals: usize,
) -> MathResult<(f64, f64)> {
    adaptive_at(f, a, b, (tol, tol), max_intervals, &|t| t).map(|q| (q.value, q.error))
}

/// Adaptive quadrature to the error `max(tol.0 · ∫|f|, tol.1)`: relative to `∫|f|`, so
/// that a value which is small because positive and negative parts cancel is not asked
/// for more than rounding allows, and one that is small because `f` is small is still
/// found to all its digits. `place` turns a point of `[a, b]` into the point of the
/// original variable that a message names (for an integral mapped onto another range).
fn adaptive_at(
    f: &mut dyn FnMut(f64) -> Option<f64>,
    a: f64,
    b: f64,
    (tol, atol): (f64, f64),
    max_intervals: usize,
    place: &dyn Fn(f64) -> f64,
) -> MathResult<Quad> {
    if a == b {
        return Ok(Quad::default());
    }
    let tol = if tol.is_finite() && tol > 0.0 {
        tol
    } else {
        DEFAULT_TOLERANCE
    };
    let mut panels: Vec<Panel> = Vec::new();
    let mut pending = vec![(a, b)];
    // Panels where the integrand blew up are split until they are tiny.
    let mut bad: Vec<(f64, f64)> = Vec::new();
    let mut evaluated = 0usize;
    while let Some((l, r)) = pending.pop() {
        crate::budget::check()?;
        evaluated += 1;
        if evaluated > 4 * max_intervals {
            break;
        }
        match gk15(f, l, r) {
            Some((value, err, abs)) => panels.push(Panel {
                a: l,
                b: r,
                value,
                err,
                abs,
            }),
            None => {
                let m = 0.5 * (l + r);
                if (r - l).abs() <= 1e-13 * (1.0 + m.abs()) {
                    bad.push((l, r));
                } else {
                    pending.push((l, m));
                    pending.push((m, r));
                }
            }
        }
    }
    if let Some(&(l, r)) = pending.last() {
        bad.push((l, r));
    }
    if let Some((l, r)) = bad.first() {
        return Err(MathError::Undetermined(format!(
            "whether the integral converges: the integrand is not finite near {}",
            crate::number::format_f64(place(0.5 * (l + r)), 6)
        )));
    }
    loop {
        crate::budget::check()?;
        let total: f64 = panels.iter().map(|p| p.value).sum();
        let err: f64 = panels.iter().map(|p| p.err).sum();
        let abs: f64 = panels.iter().map(|p| p.abs).sum();
        if !total.is_finite() {
            return Err(MathError::Undetermined(
                "the integral: the numerical sum overflowed".into(),
            ));
        }
        // Rounding in the panel sums is an error too: a total far below the size of its
        // parts (positive and negative pieces cancelling) is noise there.
        let done = |err: f64| Quad {
            value: total,
            error: err.max(4e-15 * abs),
            abs,
        };
        let wanted = (tol * abs).max(atol);
        if err <= wanted {
            return Ok(done(err));
        }
        if panels.len() >= max_intervals {
            return Err(not_settled(total, err));
        }
        let (i, worst) = panels
            .iter()
            .enumerate()
            .max_by(|x, y| x.1.err.total_cmp(&y.1.err))
            .map(|(i, p)| (i, *p))
            .ok_or_else(|| not_settled(total, err))?;
        let m = 0.5 * (worst.a + worst.b);
        if (worst.b - worst.a).abs() <= 1e-14 * (1.0 + m.abs()) {
            // Cannot split further: accept if the rest is good enough.
            if err <= 1e3 * wanted {
                return Ok(done(err));
            }
            return Err(not_settled(total, err));
        }
        let left = gk15(f, worst.a, m);
        let right = gk15(f, m, worst.b);
        match (left, right) {
            (Some((lv, le, la)), Some((rv, re, ra))) => {
                panels[i] = Panel {
                    a: worst.a,
                    b: m,
                    value: lv,
                    err: le,
                    abs: la,
                };
                panels.push(Panel {
                    a: m,
                    b: worst.b,
                    value: rv,
                    err: re,
                    abs: ra,
                });
            }
            _ => {
                return Err(MathError::Undetermined(format!(
                    "whether the integral converges: the integrand is not finite near {}",
                    crate::number::format_f64(place(m), 6)
                )))
            }
        }
    }
}

fn not_settled(total: f64, err: f64) -> MathError {
    MathError::Undetermined(format!(
        "whether the integral converges: the numerical value did not settle (about {} with estimated error {})",
        crate::number::format_f64(total, 8),
        crate::number::format_f64(err, 2)
    ))
}

/// Integrates `f` over `[a, b]` where either bound may be infinite: infinite ranges are
/// cut into pieces of every scale (see [`half_line`]), and singular endpoints are
/// smoothed by a polynomial change of variable. Returns `(value, error estimate)`.
pub(crate) fn quad(
    f: &mut dyn FnMut(f64) -> Option<f64>,
    a: f64,
    b: f64,
    tol: f64,
    max_intervals: usize,
) -> MathResult<(f64, f64)> {
    quad_full(f, a, b, tol, max_intervals).map(|q| (q.value, q.error))
}

/// [`quad`], with the estimate of `∫|f|` as well.
pub(crate) fn quad_full(
    f: &mut dyn FnMut(f64) -> Option<f64>,
    a: f64,
    b: f64,
    tol: f64,
    max_intervals: usize,
) -> MathResult<Quad> {
    if a.is_nan() || b.is_nan() {
        return Err(MathError::Undefined("a bound".into()));
    }
    if a == b {
        return Ok(Quad::default());
    }
    if a > b {
        let q = quad_full(f, b, a, tol, max_intervals)?;
        return Ok(Quad {
            value: -q.value,
            ..q
        });
    }
    let tol = if tol.is_finite() && tol > 0.0 {
        tol
    } else {
        DEFAULT_TOLERANCE
    };
    let finite = |v: Option<f64>| v.filter(|y| y.is_finite());
    match (a.is_finite(), b.is_finite()) {
        (true, true) => {
            let mut g = |x: f64| finite(f(x));
            segment(&mut g, a, b, (tol, 0.0), max_intervals, &|t| t)
        }
        (true, false) => half_line(f, a, 1.0, tol, max_intervals),
        (false, true) => half_line(f, b, -1.0, tol, max_intervals),
        (false, false) => {
            let right = half_line(f, 0.0, 1.0, tol, max_intervals)?;
            let left = half_line(f, 0.0, -1.0, tol, max_intervals)?;
            Ok(right.add(left))
        }
    }
}

/// Smallest and largest power of two probed for the scale of a half-line integrand.
const PROBE: (i32, i32) = (-40, 1023);
/// Octaves whose sampled share is below this fraction of the largest are not needed for
/// the scale (they are still integrated).
const NEGLIGIBLE: f64 = 1e-18;
/// Negligible octaves in a row after which the probe stops looking further out.
const QUIET_OCTAVES: usize = 40;

/// `∫ f` over the half-line `x = a + dir·d`, `d ∈ [0, ∞)`.
///
/// A single map such as `x = a + t/(1 − t)` puts all of `[a + 10⁷, ∞)` into a sliver of
/// `t` beside 1, so an integrand that lives at the scale `10⁷` (`1/(x + 10⁷)²` from 0, or
/// `arctan x/x²` from `10⁷`) is never sampled where it matters. So the scale is found
/// first: `d·|f(a + dir·d)|`, the share of the octave around `d`, is probed at every power
/// of two. The range is then cut at the powers of two from the first octave that matters
/// to the last, each piece is integrated on its own, and the rest of the line is mapped
/// with the scale of the last octave: `d = D (1 + t/(1 − t))`.
fn half_line(
    f: &mut dyn FnMut(f64) -> Option<f64>,
    a: f64,
    dir: f64,
    tol: f64,
    max_intervals: usize,
) -> MathResult<Quad> {
    let finite = |v: Option<f64>| v.filter(|y| y.is_finite());
    let x_of = move |d: f64| a + dir * d;
    let mut shares: Vec<(i32, f64)> = Vec::new();
    let mut largest = 0.0f64;
    // Octaves in a row that were negligible: past enough of them the probe stops.
    let mut quiet = 0;
    for k in PROBE.0..=PROBE.1 {
        let d = 2f64.powi(k);
        let x = x_of(d);
        if !x.is_finite() {
            break;
        }
        if let Some(y) = finite(f(x)) {
            let share = (d * y.abs()).min(f64::MAX);
            shares.push((k, share));
            largest = largest.max(share);
            if largest == 0.0 || share > NEGLIGIBLE * largest {
                quiet = 0;
            } else {
                quiet += 1;
                if quiet > QUIET_OCTAVES {
                    break;
                }
            }
        }
    }
    let mut needed = shares
        .iter()
        .filter(|s| s.1 > NEGLIGIBLE * largest)
        .map(|s| s.0);
    let first = needed.next().unwrap_or(0);
    let last = needed.next_back().unwrap_or(first).max(first);
    if shares.len() > 1 && shares.last().is_some_and(|s| s.0 == last) {
        // Still not decaying where doubles end (∫_2^∞ dx/ln x): nothing to settle on.
        return Err(MathError::Undetermined(
            "whether the integral converges: the integrand does not die away within the range of floating-point numbers".into(),
        ));
    }
    let pieces = f64::from(last - first + 2);
    // ∫|f| ≈ ln 2 · Σ shares; each piece may add its part of the absolute error allowed.
    let scale: f64 = std::f64::consts::LN_2 * shares.iter().map(|s| s.1).sum::<f64>();
    let atol = if scale.is_finite() {
        tol * scale / pieces
    } else {
        0.0
    };
    let mut g = |x: f64| finite(f(x));
    let mut total = piece(
        &mut g,
        &x_of,
        0.0,
        2f64.powi(first),
        (tol, atol),
        max_intervals,
    )?;
    for k in first..last {
        let (l, r) = (2f64.powi(k), 2f64.powi(k + 1));
        total = total.add(piece(&mut g, &x_of, l, r, (tol, atol), max_intervals)?);
    }
    let big = 2f64.powi(last);
    let mut h = |t: f64| {
        let s = 1.0 - t;
        let y = g(x_of(big + big * t / s))?;
        Some(if y == 0.0 { 0.0 } else { y * big / (s * s) })
    };
    let tail = adaptive_at(&mut h, 0.0, 1.0, (tol, atol), max_intervals, &|t| {
        x_of(big + big * t / (1.0 - t))
    })?;
    Ok(total.add(tail))
}

/// `∫ f` over `x = a + dir·d` for `d ∈ [l, r]`, as an integral in `d`.
fn piece(
    g: &mut dyn FnMut(f64) -> Option<f64>,
    x_of: &dyn Fn(f64) -> f64,
    l: f64,
    r: f64,
    tols: (f64, f64),
    max_intervals: usize,
) -> MathResult<Quad> {
    let mut h = |d: f64| g(x_of(d));
    segment(&mut h, l, r, tols, max_intervals, x_of)
}

/// Adaptive quadrature over the finite `[a, b]`, where the integrand may be singular at
/// an end: such an end is smoothed by `x = a + (b − a)(3t² − 2t³)`, which turns an
/// integrable `|x − a|^(−α)` into something bounded. `place` names points as in
/// [`adaptive_at`].
fn segment(
    f: &mut dyn FnMut(f64) -> Option<f64>,
    a: f64,
    b: f64,
    tols: (f64, f64),
    max_intervals: usize,
    place: &dyn Fn(f64) -> f64,
) -> MathResult<Quad> {
    let finite = |v: Option<f64>| v.filter(|y| y.is_finite());
    if finite(f(a)).is_some() && finite(f(b)).is_some() {
        return adaptive_at(f, a, b, tols, max_intervals, place);
    }
    // x = a + (b − a)(3t² − 2t³), dx = 6(b − a) t (1 − t) dt
    let w = b - a;
    let mut g = |t: f64| {
        let x = a + w * t * t * (3.0 - 2.0 * t);
        let dx = 6.0 * w * t * (1.0 - t);
        if dx == 0.0 {
            return Some(0.0);
        }
        finite(f(x)).map(|y| y * dx)
    };
    let smoothed = adaptive_at(&mut g, 0.0, 1.0, tols, max_intervals, &|t| {
        place(a + w * t * t * (3.0 - 2.0 * t))
    });
    // The map samples very close to the end, where a removable singularity such as
    // x/(eˣ − 1) at 0 can no longer be evaluated; the plain rule may still do.
    smoothed.or_else(|err| adaptive_at(f, a, b, tols, max_intervals, place).map_err(|_| err))
}

/// `∫_a^b f d(var)` numerically, with an error estimate.
///
/// Uses adaptive Gauss–Kronrod 7–15 with at most a few thousand subintervals. Bounds may be
/// `±∞`. Every other symbol in `f` must already have a value. An integral whose estimate does
/// not settle, or whose integrand is not finite inside the range, is reported as
/// [`MathError::Undetermined`] (divergence is only reported when it is proven, by the
/// definite-integral code).
pub fn numeric_integrate(f: &Expr, var: &str, a: f64, b: f64, tol: f64) -> MathResult<(f64, f64)> {
    if let Some(other) = f.free_symbols().iter().find(|s| &***s != var) {
        return Err(MathError::UnboundVariable(other.to_string()));
    }
    let mut g = sampler(f, var, a, b);
    quad(&mut g, a, b, tol, MAX_INTERVALS)
}

/// The integrand of `∫_a^b f d(var)` as a function of one `f64`, `None` where it is not
/// finite.
///
/// Far out on an infinite range `x/(e^x − 1)` can no longer be evaluated (e^x overflows)
/// although it has long been negligible: past the point where it is already below 1e-100
/// and then fails, a failed evaluation counts as 0.
pub(crate) fn sampler<'a>(
    f: &'a Expr,
    var: &'a str,
    a: f64,
    b: f64,
) -> impl FnMut(f64) -> Option<f64> + 'a {
    let at = move |x: f64| {
        let env: BTreeMap<String, f64> = [(var.to_string(), x)].into_iter().collect();
        f.eval_f64(&env).ok().filter(|v| v.is_finite())
    };
    let negligible_past = |from: f64, dir: f64| {
        let mut last_small = None;
        for k in 0..1100 {
            let x = from + dir * 2f64.powi(k);
            match at(x) {
                Some(v) if v.abs() < 1e-100 => last_small = Some(x),
                Some(_) => last_small = None,
                None => return last_small,
            }
        }
        None
    };
    let up = (b == f64::INFINITY && a.is_finite())
        .then(|| negligible_past(a.max(0.0), 1.0))
        .flatten();
    let down = (a == f64::NEG_INFINITY && b.is_finite())
        .then(|| negligible_past(b.min(0.0), -1.0))
        .flatten();
    move |x: f64| match at(x) {
        Some(v) => Some(v),
        None if up.is_some_and(|c| x > c) || down.is_some_and(|c| x < c) => Some(0.0),
        None => None,
    }
}
