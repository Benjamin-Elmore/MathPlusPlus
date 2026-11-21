//! High-precision evaluation for `≈` answers and checks.
//!
//! A closed real expression is evaluated in binary floating point at a working
//! precision, then again at a higher one; the digits are kept once two runs agree to
//! more digits than are wanted. Values far outside the `f64` range (`200!`, `e^1000`)
//! and differences of nearly equal numbers (`π − 3.14159265358979`) come out right
//! this way, where `f64` gives `∞` or rounding noise.

mod eval;
pub mod float;
mod special;

use std::collections::HashMap;

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Zero};

use crate::expr::Expr;
use eval::Eval;
pub use float::BigF;
pub use num_rational::BigRational as Rational;

/// Most bits tried before giving up.
const MAX_BITS: u64 = 1 << 14;

/// A decimal value: `±d₁.d₂d₃… × 10^k`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decimal {
    pub neg: bool,
    /// Significant digits (the first is nonzero).
    pub digits: String,
    /// The power of ten of the first digit.
    pub k: i64,
}

impl Decimal {
    /// The decimal rounded to `sig` significant digits (half away from zero).
    pub fn rounded(&self, sig: usize) -> Decimal {
        let sig = sig.max(1);
        if self.digits.len() <= sig {
            let mut d = self.clone();
            d.digits
                .extend(std::iter::repeat_n('0', sig - self.digits.len()));
            return d;
        }
        let head: BigInt = self.digits[..sig].parse().unwrap_or_default();
        let up = self.digits.as_bytes()[sig] >= b'5';
        let n = if up { head + 1 } else { head };
        let mut text = n.to_string();
        let mut k = self.k;
        if text.len() > sig {
            text.truncate(sig);
            k += 1;
        }
        Decimal {
            neg: self.neg,
            digits: text,
            k,
        }
    }

    /// The exact rational value of these digits (only for moderate exponents).
    pub fn to_ratio(&self) -> Option<BigRational> {
        let n: BigInt = self.digits.parse().ok()?;
        let shift = self.k - (self.digits.len() as i64 - 1);
        if shift.abs() > 100_000 {
            return None;
        }
        let ten = num_traits::pow(BigInt::from(10), shift.unsigned_abs() as usize);
        let q = if shift >= 0 {
            BigRational::from_integer(n * ten)
        } else {
            BigRational::new(n, ten)
        };
        Some(if self.neg { -q } else { q })
    }
}

/// The outcome of a high-precision evaluation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Approx {
    /// The value, with at least the requested digits confirmed (a few more follow).
    Value {
        value: Decimal,
        /// An input was a floating-point number, so only about 15 digits mean anything.
        inexact: bool,
    },
    /// Zero to every digit tried: the value is below `10^below` (relative to the size
    /// of the terms that cancelled).
    Zero { below: i64 },
}

/// `x` as decimal digits: `sig` significant digits, rounded to nearest.
pub fn to_decimal(x: &BigF, sig: usize) -> Option<Decimal> {
    if x.is_zero() {
        return None;
    }
    let neg = x.is_negative();
    let a = x.abs();
    let sig = sig.max(1);
    let log10_2 = std::f64::consts::LOG10_2;
    let mut k = ((a.mag() - 1) as f64 * log10_2).floor() as i64;
    if a.exponent().abs() > 200_000 || k.abs() > 60_000 {
        return log_decimal(&a, sig).map(|mut d| {
            d.neg = neg;
            d
        });
    }
    let limit_hi = num_traits::pow(BigInt::from(10), sig);
    let limit_lo = num_traits::pow(BigInt::from(10), sig - 1);
    for _ in 0..4 {
        let t = sig as i64 - 1 - k;
        let mut num = a.mantissa().clone();
        let mut den = BigInt::one();
        let e = a.exponent();
        if e >= 0 {
            num <<= e as u64;
        } else {
            den <<= (-e) as u64;
        }
        let p10 = num_traits::pow(BigInt::from(10), t.unsigned_abs() as usize);
        if t >= 0 {
            num *= p10;
        } else {
            den *= p10;
        }
        let d: BigInt = (&num * 2 + &den) / (&den * 2);
        if d >= limit_hi {
            k += 1;
        } else if d < limit_lo {
            k -= 1;
        } else {
            return Some(Decimal {
                neg,
                digits: d.to_string(),
                k,
            });
        }
    }
    None
}

/// Digits of a value with a huge exponent, through `log₁₀`.
fn log_decimal(a: &BigF, sig: usize) -> Option<Decimal> {
    let w = (sig as f64 * 3.33) as u64 + 64 + 64;
    let ln10 = float::ln(&BigF::from_i64(10), w + 64).ok()?;
    let l = float::ln(a, w + 64).ok()?.div(&ln10, w + 64).ok()?;
    let k = l.floor_int();
    let frac = l.sub(&BigF::from_int(k.clone()), w);
    let mant = float::exp(&frac.mul(&ln10, w), w).ok()?;
    let mut d = to_decimal(&mant, sig)?;
    d.k += i64::try_from(k).ok()?;
    Some(d)
}

/// Evaluates a closed real expression (free names take the exact values in `vars`)
/// to at least `sig` confirmed significant digits. `None` when the expression has
/// parts this evaluator does not handle, is not real, or the digits never settle.
pub fn approx(e: &Expr, vars: &HashMap<String, BigRational>, sig: usize) -> Option<Approx> {
    let sig = sig.clamp(1, 2000);
    let start = (sig as f64 * 3.33) as u64 + 48;
    let mut p = start;
    let mut prev: Option<BigF> = None;
    let mut quiet = 0;
    while p <= MAX_BITS.max(start * 4) {
        let mut ev = Eval::new(vars, p);
        let v = ev.ev(e).ok()?;
        // Bits below `2^floor` may be cancellation noise.
        let scale = if ev.scale == i64::MIN { 0 } else { ev.scale };
        let floor = scale - p as i64 + 24;
        let noise = v.is_zero() || (ev.scale != i64::MIN && v.mag() < floor);
        if noise {
            quiet += 1;
            prev = None;
            if quiet >= 2 && p >= start * 4 {
                let below = (floor as f64 * std::f64::consts::LOG10_2) as i64;
                return Some(Approx::Zero { below });
            }
        } else {
            quiet = 0;
            if let Some(pv) = &prev {
                let diff = v.sub(pv, p);
                let tol = v.mag() - ((sig as f64 + 3.0) * 3.33) as i64;
                if diff.is_zero() || diff.mag() < tol {
                    return Some(Approx::Value {
                        value: to_decimal(&v, sig + 3)?,
                        inexact: ev.inexact,
                    });
                }
            }
            prev = Some(v);
        }
        p = if prev.is_some() && quiet == 0 && p == start {
            p + p / 2
        } else {
            p * 2
        };
    }
    None
}

/// [`approx`] of a closed expression.
pub fn approx_closed(e: &Expr, sig: usize) -> Option<Approx> {
    approx(e, &HashMap::new(), sig)
}

/// The exact value of a rational as decimal digits (`None` for zero).
pub fn rational_digits(q: &BigRational, sig: usize) -> Option<Decimal> {
    if q.is_zero() {
        return None;
    }
    let (neg, digits, k) = crate::number::rational_sig_parts(q, sig);
    Some(Decimal { neg, digits, k })
}

#[cfg(test)]
mod tests;
