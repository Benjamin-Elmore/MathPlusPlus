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

/// One Gauss–Kronrod panel: `(Kronrod estimate, error estimate)`, or `None` when the
/// integrand is not finite at a node.
fn gk15(f: &mut dyn FnMut(f64) -> Option<f64>, a: f64, b: f64) -> Option<(f64, f64)> {
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
    Some((k, (k - g).abs().max(4e-15 * size * h.abs())))
}

#[derive(Clone, Copy)]
struct Panel {
    a: f64,
    b: f64,
    value: f64,
    err: f64,
}

/// Adaptive quadrature of `f` over the finite interval `[a, b]`.
pub(crate) fn adaptive(
    f: &mut dyn FnMut(f64) -> Option<f64>,
    a: f64,
    b: f64,
    tol: f64,
    max_intervals: usize,
) -> MathResult<(f64, f64)> {
    adaptive_at(f, a, b, tol, max_intervals, &|t| t)
}

/// [`adaptive`], with `place` turning a point of `[a, b]` into the point of the
/// original variable that a message names (for an integral mapped onto `[0, 1]`).
fn adaptive_at(
    f: &mut dyn FnMut(f64) -> Option<f64>,
    a: f64,
    b: f64,
    tol: f64,
    max_intervals: usize,
    place: &dyn Fn(f64) -> f64,
) -> MathResult<(f64, f64)> {
    if a == b {
        return Ok((0.0, 0.0));
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
            Some((value, err)) => panels.push(Panel {
                a: l,
                b: r,
                value,
                err,
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
        if !total.is_finite() {
            return Err(MathError::Undetermined(
                "the integral: the numerical sum overflowed".into(),
            ));
        }
        if err <= tol.max(tol * total.abs()) {
            // Rounding in the panel sums is an error too: a total far below the size
            // of its parts (positive and negative pieces cancelling) is noise there.
            let parts: f64 = panels.iter().map(|p| p.value.abs()).sum();
            return Ok((total, err.max(4e-15 * parts)));
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
            if err <= 1e3 * tol.max(tol * total.abs()) {
                let parts: f64 = panels.iter().map(|p| p.value.abs()).sum();
                return Ok((total, err.max(4e-15 * parts)));
            }
            return Err(not_settled(total, err));
        }
        let left = gk15(f, worst.a, m);
        let right = gk15(f, m, worst.b);
        match (left, right) {
            (Some((lv, le)), Some((rv, re))) => {
                panels[i] = Panel {
                    a: worst.a,
                    b: m,
                    value: lv,
                    err: le,
                };
                panels.push(Panel {
                    a: m,
                    b: worst.b,
                    value: rv,
                    err: re,
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
/// mapped to finite ones, and singular endpoints are smoothed by a polynomial change of
/// variable. Returns `(value, error estimate)`.
pub(crate) fn quad(
    f: &mut dyn FnMut(f64) -> Option<f64>,
    a: f64,
    b: f64,
    tol: f64,
    max_intervals: usize,
) -> MathResult<(f64, f64)> {
    if a.is_nan() || b.is_nan() {
        return Err(MathError::Undefined("a bound".into()));
    }
    if a == b {
        return Ok((0.0, 0.0));
    }
    if a > b {
        let (v, e) = quad(f, b, a, tol, max_intervals)?;
        return Ok((-v, e));
    }
    let finite = |v: Option<f64>| v.filter(|y| y.is_finite());
    match (a.is_finite(), b.is_finite()) {
        (true, true) => {
            let singular = finite(f(a)).is_none() || finite(f(b)).is_none();
            if singular {
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
                adaptive_at(&mut g, 0.0, 1.0, tol, max_intervals, &|t| {
                    a + w * t * t * (3.0 - 2.0 * t)
                })
            } else {
                adaptive(&mut |x| finite(f(x)), a, b, tol, max_intervals)
            }
        }
        (true, false) => {
            // x = a + t/(1 − t)
            let mut g = |t: f64| {
                let s = 1.0 - t;
                let x = a + t / s;
                let y = finite(f(x))?;
                Some(if y == 0.0 { 0.0 } else { y / (s * s) })
            };
            adaptive_at(&mut g, 0.0, 1.0, tol, max_intervals, &|t| a + t / (1.0 - t))
        }
        (false, true) => {
            // x = b − t/(1 − t)
            let mut g = |t: f64| {
                let s = 1.0 - t;
                let x = b - t / s;
                let y = finite(f(x))?;
                Some(if y == 0.0 { 0.0 } else { y / (s * s) })
            };
            adaptive_at(&mut g, 0.0, 1.0, tol, max_intervals, &|t| b - t / (1.0 - t))
        }
        (false, false) => {
            // x = t/(1 − t²)
            let mut g = |t: f64| {
                let s = 1.0 - t * t;
                let x = t / s;
                let y = finite(f(x))?;
                Some(if y == 0.0 {
                    0.0
                } else {
                    y * (1.0 + t * t) / (s * s)
                })
            };
            adaptive_at(&mut g, -1.0, 1.0, tol, max_intervals, &|t| {
                t / (1.0 - t * t)
            })
        }
    }
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
