//! Improper integrals of oscillating integrands over an infinite range, such as
//! `∫₀^∞ sin(x²) dx`: the range is cut at the zeros of the integrand, each piece is
//! integrated by Gauss–Kronrod quadrature, and the slowly converging (usually alternating)
//! series of pieces is summed with Wynn's ε-algorithm.
//!
//! The answer is accepted only when the pieces shrink, their signs alternate and the
//! accelerated sums agree with each other; otherwise the caller says it could not decide.

use super::numeric::{adaptive, quad, DEFAULT_TOLERANCE};

/// Most pieces (half-oscillations) summed.
const MAX_PIECES: usize = 400;
/// Pieces summed before the acceleration is first checked.
const MIN_PIECES: usize = 24;
/// Subintervals allowed per piece.
const PIECE_INTERVALS: usize = 200;
/// Grid points per expected gap when looking for the next zero.
const GRID: usize = 24;
/// Gaps scanned before deciding the integrand has stopped oscillating.
const MAX_GAPS: usize = 60;
/// Relative agreement required between successive accelerated sums.
const AGREE: f64 = 1e-9;

/// The result of summing an oscillating tail.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Tail {
    /// The value of `∫_start^∞ f`.
    pub value: f64,
    /// An error estimate.
    pub error: f64,
    /// How many pieces between zeros were summed.
    pub pieces: usize,
    /// The first few zeros used as cut points.
    pub first_zeros: Vec<f64>,
}

/// `∫_start^∞ f` for an oscillating `f`, or `None` when the method does not apply or its
/// checks fail.
pub(crate) fn oscillating_tail(f: &mut dyn FnMut(f64) -> Option<f64>, start: f64) -> Option<Tail> {
    let finite = |v: Option<f64>| v.filter(|y| y.is_finite());
    let mut g = |x: f64| finite(f(x));
    let zeros = zeros_from(&mut g, start)?;
    let (head, head_err) = quad(&mut g, start, zeros[0], DEFAULT_TOLERANCE, 2_000).ok()?;
    let mut sums = vec![head];
    let mut pieces = Vec::new();
    let mut error = head_err;
    let mut zeros = zeros;
    let mut estimates: Vec<f64> = Vec::new();
    let mut k = 0;
    while pieces.len() < MAX_PIECES {
        if k + 1 >= zeros.len() {
            let more = next_zeros(&mut g, &zeros, 32)?;
            zeros.extend(more);
        }
        let (p, e) = adaptive(
            &mut g,
            zeros[k],
            zeros[k + 1],
            DEFAULT_TOLERANCE * 0.01,
            PIECE_INTERVALS,
        )
        .ok()?;
        error += e;
        pieces.push(p);
        let last = *sums.last()?;
        sums.push(last + p);
        k += 1;
        if pieces.len() >= MIN_PIECES && pieces.len() % 4 == 0 {
            let window = &sums[sums.len().saturating_sub(40)..];
            let est = wynn(window)?;
            estimates.push(est);
            let n = estimates.len();
            if n >= 3 {
                let (a, b, c) = (estimates[n - 3], estimates[n - 2], estimates[n - 1]);
                let scale = 1.0 + c.abs();
                if (c - b).abs() <= AGREE * scale && (b - a).abs() <= 10.0 * AGREE * scale {
                    if !shrinking_alternating(&pieces) {
                        return None;
                    }
                    return Some(Tail {
                        value: c,
                        error: error + (c - b).abs() + (b - a).abs(),
                        pieces: pieces.len(),
                        first_zeros: zeros.iter().take(4).copied().collect(),
                    });
                }
            }
        }
    }
    None
}

/// True when the later pieces alternate in sign and their sizes tend to zero.
fn shrinking_alternating(pieces: &[f64]) -> bool {
    let n = pieces.len();
    if n < 8 {
        return false;
    }
    let tail = &pieces[n / 2..];
    let alternating = tail
        .windows(2)
        .all(|w| w[0] != 0.0 && w[1] != 0.0 && w[0].signum() != w[1].signum());
    let first = pieces[n / 4].abs();
    let last = pieces[n - 1].abs().max(pieces[n - 2].abs());
    alternating && last < first
}

/// The first two zeros of `g` after `start` (sign changes, refined by bisection).
fn zeros_from(g: &mut dyn FnMut(f64) -> Option<f64>, start: f64) -> Option<Vec<f64>> {
    // Look for a first sign change on a widening grid.
    let mut found = Vec::new();
    let mut step = 1e-2;
    let mut x = start;
    let mut prev = None;
    let mut tries = 0;
    while found.len() < 2 && tries < 200_000 {
        tries += 1;
        x += step;
        if tries % 2_000 == 0 {
            step *= 2.0;
        }
        let Some(y) = g(x) else {
            prev = None;
            continue;
        };
        if y == 0.0 {
            continue;
        }
        if let Some((px, py)) = prev {
            if sign_change(py, y) {
                found.push(bisect(g, px, x)?);
            }
        }
        prev = Some((x, y));
    }
    (found.len() == 2).then_some(found)
}

fn sign_change(a: f64, b: f64) -> bool {
    (a < 0.0 && b > 0.0) || (a > 0.0 && b < 0.0)
}

/// `count` more zeros after the last of `zeros`, scanning with a step set by the last gap.
fn next_zeros(
    g: &mut dyn FnMut(f64) -> Option<f64>,
    zeros: &[f64],
    count: usize,
) -> Option<Vec<f64>> {
    let n = zeros.len();
    let mut last = zeros[n - 1];
    let mut gap = (zeros[n - 1] - zeros[n - 2]).max(1e-9);
    let mut out = Vec::with_capacity(count);
    while out.len() < count {
        let step = gap / GRID as f64;
        // Start just past the last zero so it is not found again.
        let mut x = last + 0.5 * step;
        let mut prev = (x, g(x)?);
        let mut next = None;
        for _ in 0..GRID * MAX_GAPS {
            x += step;
            let y = g(x)?;
            if y == 0.0 {
                next = Some(x);
                break;
            }
            if sign_change(prev.1, y) {
                next = Some(bisect(g, prev.0, x)?);
                break;
            }
            prev = (x, y);
        }
        let z = next?;
        gap = z - last;
        last = z;
        out.push(z);
    }
    Some(out)
}

fn bisect(g: &mut dyn FnMut(f64) -> Option<f64>, mut a: f64, mut b: f64) -> Option<f64> {
    let mut fa = g(a)?;
    for _ in 0..200 {
        let m = 0.5 * (a + b);
        if m <= a || m >= b {
            break;
        }
        let fm = g(m)?;
        if fm == 0.0 {
            return Some(m);
        }
        if (fm < 0.0) == (fa < 0.0) {
            a = m;
            fa = fm;
        } else {
            b = m;
        }
    }
    Some(0.5 * (a + b))
}

/// Wynn's ε-algorithm: the limit estimate from the highest even column of the table.
pub(crate) fn wynn(s: &[f64]) -> Option<f64> {
    let n = s.len();
    if n == 0 {
        return None;
    }
    let mut prev: Vec<f64> = vec![0.0; n + 1];
    let mut cur: Vec<f64> = s.to_vec();
    let mut best = s[n - 1];
    let mut column = 0;
    while cur.len() > 1 {
        let mut next = Vec::with_capacity(cur.len() - 1);
        for j in 0..cur.len() - 1 {
            let d = cur[j + 1] - cur[j];
            if d == 0.0 || !d.is_finite() {
                return Some(if column % 2 == 0 { cur[j + 1] } else { best });
            }
            next.push(prev[j + 1] + 1.0 / d);
        }
        column += 1;
        if column % 2 == 0 {
            match next.last() {
                Some(v) if v.is_finite() => best = *v,
                _ => return Some(best),
            }
        }
        prev = cur;
        cur = next;
    }
    Some(best)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wynn_sums_an_alternating_series() {
        // 1 − 1/2 + 1/3 − … = ln 2
        let mut s = Vec::new();
        let mut acc = 0.0;
        for k in 1..=20 {
            acc += if k % 2 == 1 { 1.0 } else { -1.0 } / k as f64;
            s.push(acc);
        }
        let v = wynn(&s).unwrap();
        assert!((v - 2f64.ln()).abs() < 1e-12, "{v}");
    }

    #[test]
    fn dirichlet_and_fresnel_tails() {
        let t = oscillating_tail(&mut |x: f64| Some(x.sin() / x), 0.0).unwrap();
        assert!(
            (t.value - std::f64::consts::FRAC_PI_2).abs() < 1e-8,
            "{t:?}"
        );
        let t = oscillating_tail(&mut |x: f64| Some((x * x).sin()), 0.0).unwrap();
        let fresnel = (std::f64::consts::PI / 8.0).sqrt();
        assert!((t.value - fresnel).abs() < 1e-8, "{t:?}");
        let t = oscillating_tail(&mut |x: f64| Some((x * x).cos()), 0.0).unwrap();
        assert!((t.value - fresnel).abs() < 1e-8, "{t:?}");
    }

    #[test]
    fn non_oscillating_or_growing_tails_are_refused() {
        assert!(oscillating_tail(&mut |x: f64| Some(x * x.sin()), 0.0).is_none());
        assert!(oscillating_tail(&mut |x: f64| Some((-x).exp()), 0.0).is_none());
    }
}
