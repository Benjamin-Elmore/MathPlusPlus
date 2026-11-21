//! Floating-point summation: compensated sums of long finite ranges, and accelerated values
//! of convergent infinite series (direct summation for fast decay, Wynn's ε-algorithm for
//! alternating series, Euler–Maclaurin tails for slow algebraic decay).

use num_bigint::BigInt;
use num_traits::ToPrimitive;

use super::term_f64;
use crate::calc::diff::diff;
use crate::calc::integrate::{integrate_definite, numeric_integrate};
use crate::error::{MathError, MathResult};
use crate::expr::Expr;

/// Most terms used by direct summation of a fast-converging series.
const MAX_DIRECT: u64 = 200_000;
/// Partial sums fed to Wynn's ε-algorithm.
const WYNN_TERMS: usize = 40;
/// Terms summed before the Euler–Maclaurin tail takes over.
const EM_HEAD: f64 = 60.0;
/// Terms used when only brute force is left.
const BRUTE_TERMS: u64 = 100_000;

/// Compensated (Kahan–Neumaier) accumulator.
#[derive(Default)]
struct Kahan {
    sum: f64,
    comp: f64,
}

impl Kahan {
    fn add(&mut self, x: f64) {
        let t = self.sum + x;
        if self.sum.abs() >= x.abs() {
            self.comp += (self.sum - t) + x;
        } else {
            self.comp += (x - t) + self.sum;
        }
        self.sum = t;
    }

    fn value(&self) -> f64 {
        self.sum + self.comp
    }
}

fn bad_term(k: &str, n: f64) -> MathError {
    MathError::Undefined(format!("the term at {k} = {n}"))
}

/// `Σ_{k=lo}^{hi} t` in floating point.
pub(crate) fn kahan_range(t: &Expr, k: &str, lo: &BigInt, hi: &BigInt) -> MathResult<f64> {
    let lo = lo
        .to_f64()
        .ok_or_else(|| MathError::TooLarge("the lower bound".into()))?;
    let hi = hi
        .to_f64()
        .ok_or_else(|| MathError::TooLarge("the upper bound".into()))?;
    let mut acc = Kahan::default();
    let mut n = lo;
    while n <= hi {
        crate::budget::check()?;
        acc.add(term_f64(t, k, n).ok_or_else(|| bad_term(k, n))?);
        n += 1.0;
    }
    Ok(acc.value())
}

/// How a numeric value was obtained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Method {
    Direct,
    Wynn,
    EulerMaclaurin,
    Brute,
}

impl Method {
    pub(crate) fn describe(self) -> &'static str {
        match self {
            Method::Direct => "terms added until they no longer change the sum",
            Method::Wynn => "partial sums accelerated with Wynn's ε-algorithm",
            Method::EulerMaclaurin => {
                "60 terms added, the tail estimated by the Euler–Maclaurin formula"
            }
            Method::Brute => "100000 terms added (the tail is below the last digits shown)",
        }
    }
}

/// Fast decay: add until the terms stop mattering.
fn direct(t: &Expr, k: &str, a: f64) -> MathResult<f64> {
    let mut acc = Kahan::default();
    let mut small = 0;
    let mut n = a;
    for _ in 0..MAX_DIRECT {
        crate::budget::check()?;
        let Some(v) = term_f64(t, k, n) else {
            // Overflowing factorials far out: fine once the terms are already negligible.
            if small > 0 {
                return Ok(acc.value());
            }
            return Err(bad_term(k, n));
        };
        acc.add(v);
        if v.abs() <= 1e-17 * acc.value().abs().max(1e-300) {
            small += 1;
            if small >= 8 {
                return Ok(acc.value());
            }
        } else {
            small = 0;
        }
        n += 1.0;
    }
    Err(MathError::Inconclusive(
        "the numeric sum (it did not settle)".into(),
    ))
}

/// Wynn's ε-algorithm on a sequence of partial sums.
pub(crate) fn wynn_epsilon(s: &[f64]) -> Option<f64> {
    let mut best = *s.last()?;
    let mut prev = vec![0.0; s.len() + 1];
    let mut cur = s.to_vec();
    let mut column = 0usize;
    while cur.len() >= 2 {
        let mut next = Vec::with_capacity(cur.len() - 1);
        for i in 0..cur.len() - 1 {
            let d = cur[i + 1] - cur[i];
            if d == 0.0 || !d.is_finite() {
                return Some(if column.is_multiple_of(2) {
                    cur[i + 1]
                } else {
                    best
                });
            }
            next.push(prev[i + 1] + 1.0 / d);
        }
        column += 1;
        prev = cur;
        cur = next;
        if column.is_multiple_of(2) {
            if let Some(v) = cur.last().copied().filter(|v| v.is_finite()) {
                best = v;
            }
        }
    }
    Some(best)
}

fn alternating(t: &Expr, k: &str, a: f64) -> MathResult<f64> {
    let mut partial = Vec::with_capacity(WYNN_TERMS);
    let mut acc = Kahan::default();
    for i in 0..WYNN_TERMS {
        crate::budget::check()?;
        let n = a + i as f64;
        acc.add(term_f64(t, k, n).ok_or_else(|| bad_term(k, n))?);
        partial.push(acc.value());
    }
    wynn_epsilon(&partial).ok_or_else(|| MathError::Inconclusive("the numeric sum".into()))
}

/// `∫_N^∞ t` from an antiderivative, when the integrator finds one.
fn exact_tail(t: &Expr, k: &str, n: f64) -> Option<f64> {
    let lower = Expr::int(n as i64);
    integrate_definite(t, k, &lower, &Expr::infinity(), None)
        .ok()?
        .eval_f64(&crate::eval::EmptyEnv)
        .ok()
        .filter(|v| v.is_finite())
}

/// `Σ_{k=a}^{N−1} t + ∫_N^∞ t + t(N)/2 − t′(N)/12 + t‴(N)/720 − t⁽⁵⁾(N)/30240`.
fn euler_maclaurin(t: &Expr, k: &str, a: f64) -> MathResult<f64> {
    let n_head = a + EM_HEAD;
    let mut acc = Kahan::default();
    let mut n = a;
    while n < n_head {
        crate::budget::check()?;
        acc.add(term_f64(t, k, n).ok_or_else(|| bad_term(k, n))?);
        n += 1.0;
    }
    let tail = exact_tail(t, k, n_head).map_or_else(
        || numeric_integrate(t, k, n_head, f64::INFINITY, 1e-13).map(|r| r.0),
        Ok,
    )?;
    let d1 = diff(t, k, 1, None)?;
    let d3 = diff(&d1, k, 2, None)?;
    let d5 = diff(&d3, k, 2, None)?;
    let at = |e: &Expr| term_f64(e, k, n_head).ok_or_else(|| bad_term(k, n_head));
    let correction = at(t)? / 2.0 - at(&d1)? / 12.0 + at(&d3)? / 720.0 - at(&d5)? / 30240.0;
    acc.add(tail);
    acc.add(correction);
    Ok(acc.value())
}

fn brute(t: &Expr, k: &str, a: f64) -> MathResult<f64> {
    let mut acc = Kahan::default();
    let mut n = a;
    for _ in 0..BRUTE_TERMS {
        crate::budget::check()?;
        acc.add(term_f64(t, k, n).ok_or_else(|| bad_term(k, n))?);
        n += 1.0;
    }
    Ok(acc.value())
}

/// The value of a convergent `Σ_{k=a}^{∞} t`, choosing the method from the test that proved
/// convergence.
pub(crate) fn infinite(t: &Expr, k: &str, a: f64, test: &str) -> MathResult<(f64, Method)> {
    match test {
        "geometric series" | "ratio test" | "root test" => {
            direct(t, k, a).map(|v| (v, Method::Direct))
        }
        "alternating series test" => alternating(t, k, a).map(|v| (v, Method::Wynn)),
        _ => match euler_maclaurin(t, k, a) {
            Ok(v) if v.is_finite() => Ok((v, Method::EulerMaclaurin)),
            _ => brute(t, k, a).map(|v| (v, Method::Brute)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wynn_accelerates_alternating_harmonic() {
        let mut s = Vec::new();
        let mut acc = 0.0;
        for n in 1..=30 {
            acc += if n % 2 == 1 { 1.0 } else { -1.0 } / n as f64;
            s.push(acc);
        }
        let v = wynn_epsilon(&s).unwrap_or(f64::NAN);
        assert!((v - std::f64::consts::LN_2).abs() < 1e-12, "{v}");
        assert_eq!(wynn_epsilon(&[]), None);
    }

    #[test]
    fn kahan_keeps_small_terms() {
        let mut k = Kahan::default();
        k.add(1e16);
        for _ in 0..10 {
            k.add(1.0);
        }
        k.add(-1e16);
        assert_eq!(k.value(), 10.0);
    }
}
