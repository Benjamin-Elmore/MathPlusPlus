//! Numbers: exact big rationals, exact Gaussian rationals and approximate `f64` values.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

use num_bigint::BigInt;
use num_complex::Complex64;
use num_integer::Integer;
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};

use crate::error::{MathError, MathResult};

/// Largest result, in bits, that exact powers may produce.
const MAX_POW_BITS: u64 = 1 << 20;
/// Largest argument of an exact factorial.
pub const MAX_FACTORIAL: u64 = 20_000;
/// Trial-division bound used when extracting exact roots.
const TRIAL_DIVISION_LIMIT: u64 = 100_000;

/// A number. Exact values stay exact; any approximate operand makes the result approximate.
#[derive(Clone, Debug)]
pub enum Number {
    /// An exact rational (integers included).
    Rational(BigRational),
    /// An exact Gaussian rational `re + im·i` with `im ≠ 0`.
    Complex(BigRational, BigRational),
    /// An approximate real.
    Float(f64),
    /// An approximate complex value with `im ≠ 0`.
    ComplexFloat(f64, f64),
}

fn clean(x: f64) -> f64 {
    if x == 0.0 {
        0.0
    } else {
        x
    }
}

/// Converts a rational to `f64` (±∞ when out of range).
pub fn ratio_to_f64(q: &BigRational) -> f64 {
    q.to_f64().unwrap_or_else(|| {
        if q.is_negative() {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        }
    })
}

impl Number {
    /// An exact integer.
    pub fn int(n: i64) -> Number {
        Number::Rational(BigRational::from_integer(BigInt::from(n)))
    }

    /// An exact big integer.
    pub fn from_bigint(n: BigInt) -> Number {
        Number::Rational(BigRational::from_integer(n))
    }

    /// The exact fraction `p/q`.
    pub fn rational(p: i64, q: i64) -> MathResult<Number> {
        if q == 0 {
            return Err(MathError::DivisionByZero);
        }
        Ok(Number::Rational(BigRational::new(p.into(), q.into())))
    }

    /// Wraps a big rational.
    pub fn from_ratio(q: BigRational) -> Number {
        Number::Rational(q)
    }

    /// An exact complex value, real when `im = 0`.
    pub fn complex(re: BigRational, im: BigRational) -> Number {
        if im.is_zero() {
            Number::Rational(re)
        } else {
            Number::Complex(re, im)
        }
    }

    /// An approximate real.
    pub fn float(x: f64) -> Number {
        Number::Float(clean(x))
    }

    /// An approximate complex value, real when `im = 0`.
    pub fn complex_float(re: f64, im: f64) -> Number {
        if im == 0.0 {
            Number::Float(clean(re))
        } else {
            Number::ComplexFloat(clean(re), im)
        }
    }

    /// The imaginary unit.
    pub fn i() -> Number {
        Number::Complex(BigRational::zero(), BigRational::one())
    }

    /// Exact zero.
    pub fn zero() -> Number {
        Number::int(0)
    }

    /// Exact one.
    pub fn one() -> Number {
        Number::int(1)
    }

    /// Parses a decimal literal such as `12`, `0.25` or `1.5e-3` into an exact rational.
    pub fn parse_decimal(text: &str) -> Option<Number> {
        let (mantissa, exp) = match text.find(['e', 'E']) {
            Some(pos) => (&text[..pos], text[pos + 1..].parse::<i64>().ok()?),
            None => (text, 0),
        };
        let (int_part, frac_part) = match mantissa.find('.') {
            Some(pos) => (&mantissa[..pos], &mantissa[pos + 1..]),
            None => (mantissa, ""),
        };
        if int_part.is_empty() && frac_part.is_empty() {
            return None;
        }
        let digits = format!("{int_part}{frac_part}");
        if !digits.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let value: BigInt = digits.parse().ok()?;
        let scale = exp.checked_sub(i64::try_from(frac_part.len()).ok()?)?;
        if scale.unsigned_abs() > 100_000 {
            return None;
        }
        let ten = BigInt::from(10);
        let factor = num_traits::pow(ten, usize::try_from(scale.unsigned_abs()).ok()?);
        let q = if scale >= 0 {
            BigRational::from_integer(value * factor)
        } else {
            BigRational::new(value, factor)
        };
        Some(Number::Rational(q))
    }

    /// True for exact values.
    pub fn is_exact(&self) -> bool {
        matches!(self, Number::Rational(_) | Number::Complex(..))
    }

    /// True for real values.
    pub fn is_real(&self) -> bool {
        matches!(self, Number::Rational(_) | Number::Float(_))
    }

    /// True when the value is zero.
    pub fn is_zero(&self) -> bool {
        match self {
            Number::Rational(q) => q.is_zero(),
            Number::Float(x) => *x == 0.0,
            _ => false,
        }
    }

    /// True when the value is exactly one (or `1.0`).
    pub fn is_one(&self) -> bool {
        match self {
            Number::Rational(q) => q.is_one(),
            Number::Float(x) => *x == 1.0,
            _ => false,
        }
    }

    /// True when the value is exactly −1 (or `−1.0`).
    pub fn is_minus_one(&self) -> bool {
        match self {
            Number::Rational(q) => *q == -BigRational::one(),
            Number::Float(x) => *x == -1.0,
            _ => false,
        }
    }

    /// The exact rational, if this is one.
    pub fn as_rational(&self) -> Option<&BigRational> {
        match self {
            Number::Rational(q) => Some(q),
            _ => None,
        }
    }

    /// The exact integer, if this is one.
    pub fn as_integer(&self) -> Option<BigInt> {
        match self {
            Number::Rational(q) if q.is_integer() => Some(q.to_integer()),
            _ => None,
        }
    }

    /// The exact integer as `i64`, if it fits.
    pub fn as_i64(&self) -> Option<i64> {
        self.as_integer().and_then(|n| n.to_i64())
    }

    /// True for exact integers.
    pub fn is_integer(&self) -> bool {
        matches!(self, Number::Rational(q) if q.is_integer())
    }

    /// Sign of a real value; `None` for complex values and NaN.
    pub fn sign(&self) -> Option<Ordering> {
        match self {
            Number::Rational(q) => Some(q.cmp(&BigRational::zero())),
            Number::Float(x) => x.partial_cmp(&0.0),
            _ => None,
        }
    }

    /// True for real values above zero.
    pub fn is_positive(&self) -> bool {
        self.sign() == Some(Ordering::Greater)
    }

    /// True for real values below zero.
    pub fn is_negative(&self) -> bool {
        self.sign() == Some(Ordering::Less)
    }

    /// Compares two real values; `None` if either is complex or NaN.
    pub fn cmp_real(&self, other: &Number) -> Option<Ordering> {
        match (self, other) {
            (Number::Rational(a), Number::Rational(b)) => Some(a.cmp(b)),
            _ if self.is_real() && other.is_real() => self.to_f64().partial_cmp(&other.to_f64()),
            _ => None,
        }
    }

    /// The value as `f64` (the real part of complex values).
    pub fn to_f64(&self) -> f64 {
        match self {
            Number::Rational(q) | Number::Complex(q, _) => ratio_to_f64(q),
            Number::Float(x) | Number::ComplexFloat(x, _) => *x,
        }
    }

    /// The value as an `f64` complex.
    pub fn to_complex64(&self) -> Complex64 {
        let (re, im) = self.float_parts();
        Complex64::new(re, im)
    }

    /// Converts to an approximate value.
    pub fn to_approx(&self) -> Number {
        let (re, im) = self.float_parts();
        Number::complex_float(re, im)
    }

    fn float_parts(&self) -> (f64, f64) {
        match self {
            Number::Rational(q) => (ratio_to_f64(q), 0.0),
            Number::Complex(re, im) => (ratio_to_f64(re), ratio_to_f64(im)),
            Number::Float(x) => (*x, 0.0),
            Number::ComplexFloat(re, im) => (*re, *im),
        }
    }

    fn exact_parts(&self) -> Option<(BigRational, BigRational)> {
        match self {
            Number::Rational(q) => Some((q.clone(), BigRational::zero())),
            Number::Complex(re, im) => Some((re.clone(), im.clone())),
            _ => None,
        }
    }

    /// Real part.
    pub fn re(&self) -> Number {
        match self {
            Number::Rational(q) | Number::Complex(q, _) => Number::Rational(q.clone()),
            Number::Float(x) | Number::ComplexFloat(x, _) => Number::float(*x),
        }
    }

    /// Imaginary part.
    pub fn im(&self) -> Number {
        match self {
            Number::Rational(_) => Number::zero(),
            Number::Complex(_, q) => Number::Rational(q.clone()),
            Number::Float(_) => Number::float(0.0),
            Number::ComplexFloat(_, y) => Number::float(*y),
        }
    }

    /// Sum.
    pub fn add(&self, other: &Number) -> Number {
        match (self.exact_parts(), other.exact_parts()) {
            (Some((a, b)), Some((c, d))) => Number::complex(a + c, b + d),
            _ => {
                let (a, b) = self.float_parts();
                let (c, d) = other.float_parts();
                Number::complex_float(a + c, b + d)
            }
        }
    }

    /// Difference.
    pub fn sub(&self, other: &Number) -> Number {
        self.add(&other.neg())
    }

    /// Negation.
    pub fn neg(&self) -> Number {
        match self {
            Number::Rational(q) => Number::Rational(-q),
            Number::Complex(re, im) => Number::Complex(-re, -im),
            Number::Float(x) => Number::float(-x),
            Number::ComplexFloat(re, im) => Number::complex_float(-re, -im),
        }
    }

    /// Product.
    pub fn mul(&self, other: &Number) -> Number {
        match (self, other) {
            (Number::Rational(a), Number::Rational(b)) => Number::Rational(a * b),
            _ => match (self.exact_parts(), other.exact_parts()) {
                (Some((a, b)), Some((c, d))) => {
                    Number::complex(&a * &c - &b * &d, &a * &d + &b * &c)
                }
                _ => {
                    let z = self.to_complex64() * other.to_complex64();
                    Number::complex_float(z.re, z.im)
                }
            },
        }
    }

    /// Reciprocal; errors on exact zero.
    pub fn recip(&self) -> MathResult<Number> {
        match self {
            Number::Rational(q) => {
                if q.is_zero() {
                    Err(MathError::DivisionByZero)
                } else {
                    Ok(Number::Rational(q.recip()))
                }
            }
            Number::Complex(a, b) => {
                let norm = a * a + b * b;
                Ok(Number::complex(a / &norm, -(b / &norm)))
            }
            _ => {
                let z = self.to_complex64();
                if z.re == 0.0 && z.im == 0.0 {
                    return Err(MathError::DivisionByZero);
                }
                let r = z.inv();
                Ok(Number::complex_float(r.re, r.im))
            }
        }
    }

    /// Quotient; errors on division by zero.
    pub fn div(&self, other: &Number) -> MathResult<Number> {
        Ok(self.mul(&other.recip()?))
    }

    /// Absolute value of a real number; `None` for complex values.
    pub fn abs_real(&self) -> Option<Number> {
        match self {
            Number::Rational(q) => Some(Number::Rational(q.abs())),
            Number::Float(x) => Some(Number::float(x.abs())),
            _ => None,
        }
    }

    /// Integer power, exact for exact bases.
    pub fn pow_int(&self, n: &BigInt) -> MathResult<Number> {
        if n.is_zero() {
            return Ok(if self.is_exact() {
                Number::one()
            } else {
                Number::float(1.0)
            });
        }
        if !self.is_exact() {
            let e = n.to_f64().unwrap_or(f64::INFINITY);
            let z = self.to_complex64();
            let r = if self.is_real() {
                Complex64::new(z.re.powf(e), 0.0)
            } else if let Some(k) = n.to_i32() {
                z.powi(k)
            } else {
                z.powf(e)
            };
            return Ok(Number::complex_float(r.re, r.im));
        }
        if self.is_zero() {
            return if n.is_negative() {
                Err(MathError::DivisionByZero)
            } else {
                Ok(Number::zero())
            };
        }
        if self.is_one() {
            return Ok(Number::one());
        }
        if self.is_minus_one() {
            return Ok(if n.is_even() {
                Number::one()
            } else {
                Number::int(-1)
            });
        }
        let base = if n.is_negative() {
            self.recip()?
        } else {
            self.clone()
        };
        let exp = n.abs();
        let base_bits = match &base {
            Number::Rational(q) => q.numer().bits().max(q.denom().bits()),
            Number::Complex(a, b) => {
                a.numer()
                    .bits()
                    .max(a.denom().bits())
                    .max(b.numer().bits())
                    .max(b.denom().bits())
                    + 1
            }
            _ => 1,
        };
        let exp_u = exp
            .to_u64()
            .ok_or_else(|| MathError::TooLarge("this power".into()))?;
        if base_bits.saturating_mul(exp_u) > MAX_POW_BITS {
            return Err(MathError::TooLarge("this power".into()));
        }
        Ok(match base {
            Number::Rational(q) => {
                let e =
                    usize::try_from(exp_u).map_err(|_| MathError::TooLarge("this power".into()))?;
                Number::Rational(BigRational::new(
                    num_traits::pow(q.numer().clone(), e),
                    num_traits::pow(q.denom().clone(), e),
                ))
            }
            other => {
                let mut result = Number::one();
                let mut b = other;
                let mut k = exp_u;
                while k > 0 {
                    if k & 1 == 1 {
                        result = result.mul(&b);
                    }
                    k >>= 1;
                    if k > 0 {
                        b = b.mul(&b);
                    }
                }
                result
            }
        })
    }

    /// Approximate power with an arbitrary exponent (principal branch).
    pub fn pow_approx(&self, exp: &Number) -> Number {
        let b = self.to_complex64();
        let e = exp.to_complex64();
        if b.im == 0.0 && e.im == 0.0 && (b.re >= 0.0 || e.re.fract() == 0.0) {
            return Number::float(b.re.powf(e.re));
        }
        if b.re == 0.0 && b.im == 0.0 {
            return Number::float(if e.re > 0.0 { 0.0 } else { f64::NAN });
        }
        let r = b.powc(e);
        Number::complex_float(r.re, r.im)
    }

    /// Decimal text with `sig` significant digits (trailing zeros trimmed).
    pub fn to_decimal(&self, sig: usize) -> String {
        match self {
            Number::Rational(q) => rational_to_sig_digits(q, sig),
            Number::Float(x) => format_f64(*x, sig),
            Number::Complex(re, im) => join_complex(
                rational_to_sig_digits(re, sig),
                &Number::Rational(im.clone()),
                sig,
            ),
            Number::ComplexFloat(re, im) => {
                join_complex(format_f64(*re, sig), &Number::float(*im), sig)
            }
        }
    }
}

fn join_complex(re: String, im: &Number, sig: usize) -> String {
    let neg = im.is_negative();
    let mag = im.abs_real().unwrap_or_else(|| im.clone()).to_decimal(sig);
    let im_text = if mag == "1" {
        "i".to_string()
    } else {
        format!("{mag}*i")
    };
    if re == "0" {
        if neg {
            format!("-{im_text}")
        } else {
            im_text
        }
    } else {
        format!("{re} {} {im_text}", if neg { "-" } else { "+" })
    }
}

/// Places a decimal point in `digits` (value = 0.d1d2… × 10^(k+1)) and trims zeros.
fn place_point(neg: bool, digits: &str, k: i64) -> String {
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    if digits == "0" {
        return "0".into();
    }
    let n = i64::try_from(digits.len()).unwrap_or(i64::MAX);
    let body = if (-7..21).contains(&k) {
        if k < 0 {
            let zeros = usize::try_from(-k - 1).unwrap_or(0);
            format!("0.{}{}", "0".repeat(zeros), digits)
        } else if k + 1 >= n {
            let zeros = usize::try_from(k + 1 - n).unwrap_or(0);
            format!("{}{}", digits, "0".repeat(zeros))
        } else {
            let split = usize::try_from(k + 1).unwrap_or(0);
            format!("{}.{}", &digits[..split], &digits[split..])
        }
    } else {
        let (first, rest) = digits.split_at(1);
        if rest.is_empty() {
            format!("{first}e{k}")
        } else {
            format!("{first}.{rest}e{k}")
        }
    };
    if neg {
        format!("-{body}")
    } else {
        body
    }
}

/// Formats an `f64` with `sig` significant digits.
pub fn format_f64(x: f64, sig: usize) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "∞".into() } else { "-∞".into() };
    }
    if x == 0.0 {
        return "0".into();
    }
    let sig = sig.clamp(1, 17);
    let text = format!("{:.*e}", sig - 1, x.abs());
    let Some((mantissa, exp)) = text.split_once('e') else {
        return text;
    };
    let k: i64 = exp.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    place_point(x < 0.0, &digits, k)
}

/// Exact rational to decimal text with `sig` significant digits (rounded half away from zero).
pub fn rational_to_sig_digits(q: &BigRational, sig: usize) -> String {
    if q.is_zero() {
        return "0".into();
    }
    let (neg, digits, k) = rational_sig_parts(q, sig);
    place_point(neg, &digits, k)
}

/// A nonzero rational as `(negative, digits, k)`: `digits` holds exactly `sig` significant
/// digits (rounded half away from zero) and the value is `0.d1d2… × 10^(k+1)`.
pub fn rational_sig_parts(q: &BigRational, sig: usize) -> (bool, String, i64) {
    let sig = sig.max(1);
    let neg = q.is_negative();
    let a = q.abs();
    // Estimate k = floor(log10 a) from digit counts, then correct it.
    let num_digits = a.numer().to_string().len() as i64;
    let den_digits = a.denom().to_string().len() as i64;
    let mut k = num_digits - den_digits;
    let pow10 = |e: i64| -> BigRational {
        let p = num_traits::pow(
            BigInt::from(10),
            usize::try_from(e.unsigned_abs()).unwrap_or(0),
        );
        if e >= 0 {
            BigRational::from_integer(p)
        } else {
            BigRational::new(BigInt::one(), p)
        }
    };
    while a < pow10(k) {
        k -= 1;
    }
    while a >= pow10(k + 1) {
        k += 1;
    }
    let shift = i64::try_from(sig).unwrap_or(17) - 1 - k;
    let scaled = &a * pow10(shift);
    let half = BigRational::new(BigInt::one(), BigInt::from(2));
    let mut rounded = (scaled + half).floor().to_integer();
    let limit = num_traits::pow(BigInt::from(10), sig);
    if rounded >= limit {
        rounded /= BigInt::from(10);
        k += 1;
    }
    (neg, rounded.to_string(), k)
}

/// Exact rational to decimal text with exactly `places` digits after the point.
pub fn rational_to_fixed(q: &BigRational, places: usize) -> String {
    let neg = q.is_negative();
    let scale = num_traits::pow(BigInt::from(10), places);
    let half = BigRational::new(BigInt::one(), BigInt::from(2));
    let scaled = (q.abs() * BigRational::from_integer(scale.clone()) + half)
        .floor()
        .to_integer();
    let (int_part, frac_part) = scaled.div_rem(&scale);
    let sign = if neg && !scaled.is_zero() { "-" } else { "" };
    if places == 0 {
        return format!("{sign}{int_part}");
    }
    let frac = frac_part.to_string();
    format!("{sign}{int_part}.{}{frac}", "0".repeat(places - frac.len()))
}

/// Writes `r^(1/n)` (for `r ≥ 0`) as `outside · inside^(1/index)` with `inside` an integer.
///
/// Example: `8^(1/2) = 2 · 2^(1/2)`, `(1/2)^(1/2) = (1/2) · 2^(1/2)`.
pub fn rational_root_parts(r: &BigRational, n: u32) -> (BigRational, BigInt, u32) {
    let den = r.denom().clone();
    let n_usize = usize::try_from(n).unwrap_or(1);
    // r^(1/n) = (num · den^(n-1))^(1/n) / den.
    let radicand = r.numer() * num_traits::pow(den.clone(), n_usize.saturating_sub(1));
    let (out, inside, index) = integer_root_parts(&radicand, n);
    (BigRational::new(out, den), inside, index)
}

/// Writes `m^(1/n)` (for `m ≥ 0`) as `outside · inside^(1/index)`.
pub fn integer_root_parts(m: &BigInt, n: u32) -> (BigInt, BigInt, u32) {
    if m.is_zero() || m.is_one() || n <= 1 {
        return (m.clone(), BigInt::one(), 1);
    }
    let n_usize = n as usize;
    let root = m.nth_root(n);
    if num_traits::pow(root.clone(), n_usize) == *m {
        return (root, BigInt::one(), 1);
    }
    let mut outside = BigInt::one();
    let mut kept = BigInt::one();
    let mut rest = m.clone();
    let mut p: u64 = 2;
    while p <= TRIAL_DIVISION_LIMIT && !rest.is_one() {
        let bp = BigInt::from(p);
        if &bp * &bp > rest {
            break;
        }
        let mut count = 0u32;
        while (&rest % &bp).is_zero() {
            rest /= &bp;
            count += 1;
        }
        if count >= n {
            outside *= num_traits::pow(bp.clone(), (count / n) as usize);
        }
        if !count.is_multiple_of(n) {
            kept *= num_traits::pow(bp, (count % n) as usize);
        }
        p += if p == 2 { 1 } else { 2 };
    }
    // The leftover may still be a perfect power.
    if !rest.is_one() {
        let r = rest.nth_root(n);
        if num_traits::pow(r.clone(), n_usize) == rest {
            outside *= r;
            rest = BigInt::one();
        }
    }
    reduce_index(outside, kept * rest, n)
}

/// Lowers the root index when `inside` is a perfect power: `4^(1/4) = 2^(1/2)`.
fn reduce_index(outside: BigInt, inside: BigInt, n: u32) -> (BigInt, BigInt, u32) {
    if inside.is_one() {
        return (outside, inside, 1);
    }
    for d in (2..=n).rev() {
        if n.is_multiple_of(d) {
            let r = inside.nth_root(d);
            if num_traits::pow(r.clone(), d as usize) == inside {
                return (outside, r, n / d);
            }
        }
    }
    (outside, inside, n)
}

/// Greatest common divisor of two rationals (`gcd(a/b, c/d) = gcd(a, c)/lcm(b, d)`).
pub fn gcd_rational(a: &BigRational, b: &BigRational) -> BigRational {
    BigRational::new(a.numer().gcd(b.numer()), a.denom().lcm(b.denom()))
}

/// Least common multiple of two rationals.
pub fn lcm_rational(a: &BigRational, b: &BigRational) -> BigRational {
    if a.is_zero() || b.is_zero() {
        return BigRational::zero();
    }
    BigRational::new(a.numer().lcm(b.numer()), a.denom().gcd(b.denom()))
}

/// `n!` for `0 ≤ n ≤ MAX_FACTORIAL`.
pub fn factorial(n: &BigInt) -> MathResult<BigInt> {
    if n.is_negative() {
        return Err(MathError::Domain {
            function: "factorial".into(),
            detail: "the argument is a negative integer".into(),
        });
    }
    let k = n
        .to_u64()
        .filter(|k| *k <= MAX_FACTORIAL)
        .ok_or_else(|| MathError::TooLarge(format!("{n}!")))?;
    Ok(product_range(1, k))
}

fn product_range(lo: u64, hi: u64) -> BigInt {
    if lo > hi {
        return BigInt::one();
    }
    if hi - lo < 16 {
        return (lo..=hi).fold(BigInt::one(), |acc, k| acc * k);
    }
    let mid = lo + (hi - lo) / 2;
    product_range(lo, mid) * product_range(mid + 1, hi)
}

/// Generalised binomial coefficient `C(n, k)` for rational `n` and integer `k`.
pub fn binomial(n: &BigRational, k: &BigInt) -> MathResult<BigRational> {
    if k.is_negative() {
        return Ok(BigRational::zero());
    }
    if n.is_integer() && !n.is_negative() && k > &n.to_integer() {
        return Ok(BigRational::zero());
    }
    // Use symmetry for integer n to keep the loop short.
    let mut k = k.clone();
    if n.is_integer() && !n.is_negative() {
        let alt = n.to_integer() - &k;
        if alt < k {
            k = alt;
        }
    }
    let kk = k
        .to_u64()
        .filter(|v| *v <= MAX_FACTORIAL)
        .ok_or_else(|| MathError::TooLarge("this binomial coefficient".into()))?;
    let mut acc = BigRational::one();
    for j in 0..kk {
        let j = BigRational::from_integer(BigInt::from(j));
        acc = acc * (n - &j) / (j + BigRational::one());
    }
    Ok(acc)
}

/// Divisors of `|m|` (positive), or `None` when `m` is too large to factor by trial division.
pub fn divisors(m: &BigInt) -> Option<Vec<BigInt>> {
    let m = m.abs();
    if m.is_zero() {
        return None;
    }
    let mut factors: Vec<(BigInt, u32)> = Vec::new();
    let mut rest = m;
    let mut p: u64 = 2;
    while BigInt::from(p) * BigInt::from(p) <= rest {
        if p > TRIAL_DIVISION_LIMIT {
            return None;
        }
        let bp = BigInt::from(p);
        let mut count = 0;
        while (&rest % &bp).is_zero() {
            rest /= &bp;
            count += 1;
        }
        if count > 0 {
            factors.push((bp, count));
        }
        p += if p == 2 { 1 } else { 2 };
    }
    if !rest.is_one() {
        factors.push((rest, 1));
    }
    let mut divs = vec![BigInt::one()];
    for (p, e) in factors {
        let mut next = Vec::with_capacity(divs.len() * (e as usize + 1));
        for d in &divs {
            let mut pk = BigInt::one();
            for _ in 0..=e {
                next.push(d * &pk);
                pk *= &p;
            }
        }
        divs = next;
        if divs.len() > 100_000 {
            return None;
        }
    }
    divs.sort();
    Some(divs)
}

impl PartialEq for Number {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Number {}

impl PartialOrd for Number {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Number {
    /// A total structural order (used for canonical sorting, not for maths comparisons).
    fn cmp(&self, other: &Self) -> Ordering {
        fn rank(n: &Number) -> u8 {
            match n {
                Number::Rational(_) => 0,
                Number::Complex(..) => 1,
                Number::Float(_) => 2,
                Number::ComplexFloat(..) => 3,
            }
        }
        match (self, other) {
            (Number::Rational(a), Number::Rational(b)) => a.cmp(b),
            (Number::Complex(a, b), Number::Complex(c, d)) => a.cmp(c).then_with(|| b.cmp(d)),
            (Number::Float(a), Number::Float(b)) => a.total_cmp(b),
            (Number::ComplexFloat(a, b), Number::ComplexFloat(c, d)) => {
                a.total_cmp(c).then_with(|| b.total_cmp(d))
            }
            _ => rank(self).cmp(&rank(other)),
        }
    }
}

impl Hash for Number {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Number::Rational(q) => {
                0u8.hash(state);
                q.hash(state);
            }
            Number::Complex(a, b) => {
                1u8.hash(state);
                a.hash(state);
                b.hash(state);
            }
            Number::Float(x) => {
                2u8.hash(state);
                x.to_bits().hash(state);
            }
            Number::ComplexFloat(a, b) => {
                3u8.hash(state);
                a.to_bits().hash(state);
                b.to_bits().hash(state);
            }
        }
    }
}

impl fmt::Display for Number {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Number::Rational(q) => {
                if q.is_integer() {
                    write!(f, "{}", q.numer())
                } else {
                    write!(f, "{}/{}", q.numer(), q.denom())
                }
            }
            Number::Float(x) => write!(f, "{}", format_f64(*x, 10)),
            Number::Complex(re, im) => {
                let re_text = Number::Rational(re.clone()).to_string();
                let neg = im.is_negative();
                let mag = im.abs();
                let im_text = if mag.is_one() {
                    "i".to_string()
                } else if mag.is_integer() {
                    format!("{}*i", mag.numer())
                } else if mag.numer().is_one() {
                    // `i/2`, not `1*i/2`.
                    format!("i/{}", mag.denom())
                } else {
                    format!("{}*i/{}", mag.numer(), mag.denom())
                };
                if re.is_zero() {
                    write!(f, "{}{im_text}", if neg { "-" } else { "" })
                } else {
                    write!(f, "{re_text} {} {im_text}", if neg { "-" } else { "+" })
                }
            }
            Number::ComplexFloat(..) => write!(f, "{}", self.to_decimal(10)),
        }
    }
}

impl From<i64> for Number {
    fn from(n: i64) -> Self {
        Number::int(n)
    }
}

impl From<BigInt> for Number {
    fn from(n: BigInt) -> Self {
        Number::from_bigint(n)
    }
}

impl From<BigRational> for Number {
    fn from(q: BigRational) -> Self {
        Number::Rational(q)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(p: i64, d: i64) -> BigRational {
        BigRational::new(p.into(), d.into())
    }

    #[test]
    fn exact_arithmetic() {
        let a = Number::rational(1, 3).unwrap();
        let b = Number::rational(1, 6).unwrap();
        assert_eq!(a.add(&b), Number::rational(1, 2).unwrap());
        assert_eq!(a.mul(&b), Number::rational(1, 18).unwrap());
        assert_eq!(a.div(&b).unwrap(), Number::int(2));
        assert_eq!(a.div(&Number::zero()), Err(MathError::DivisionByZero));
        assert!(Number::rational(1, 0).is_err());
    }

    #[test]
    fn gaussian_arithmetic() {
        let i = Number::i();
        assert_eq!(i.mul(&i), Number::int(-1));
        let z = Number::complex(q(1, 1), q(1, 1));
        assert_eq!(z.mul(&z), Number::complex(q(0, 1), q(2, 1)));
        assert_eq!(z.recip().unwrap(), Number::complex(q(1, 2), q(-1, 2)));
        assert_eq!(i.pow_int(&BigInt::from(4)).unwrap(), Number::one());
        assert_eq!(i.pow_int(&BigInt::from(-1)).unwrap(), i.neg());
    }

    #[test]
    fn approx_is_contagious() {
        let a = Number::float(0.5);
        let b = Number::int(2);
        assert_eq!(a.add(&b), Number::float(2.5));
        assert!(!a.mul(&b).is_exact());
    }

    #[test]
    fn integer_powers() {
        let two = Number::int(2);
        assert_eq!(two.pow_int(&BigInt::from(10)).unwrap(), Number::int(1024));
        assert_eq!(
            two.pow_int(&BigInt::from(-2)).unwrap(),
            Number::rational(1, 4).unwrap()
        );
        assert!(two.pow_int(&BigInt::from(10_000_000)).is_err());
        assert_eq!(
            Number::int(-1).pow_int(&BigInt::from(10_000_001)).unwrap(),
            Number::int(-1)
        );
        assert!(Number::zero().pow_int(&BigInt::from(-1)).is_err());
    }

    #[test]
    fn roots() {
        assert_eq!(
            integer_root_parts(&BigInt::from(8), 2),
            (2.into(), 2.into(), 2)
        );
        assert_eq!(
            integer_root_parts(&BigInt::from(16), 2),
            (4.into(), 1.into(), 1)
        );
        assert_eq!(
            integer_root_parts(&BigInt::from(4), 4),
            (1.into(), 2.into(), 2)
        );
        assert_eq!(
            integer_root_parts(&BigInt::from(72), 2),
            (6.into(), 2.into(), 2)
        );
        assert_eq!(
            integer_root_parts(&BigInt::from(54), 3),
            (3.into(), 2.into(), 3)
        );
        assert_eq!(rational_root_parts(&q(4, 9), 2), (q(2, 3), 1.into(), 1));
        assert_eq!(rational_root_parts(&q(1, 2), 2), (q(1, 2), 2.into(), 2));
        assert_eq!(
            integer_root_parts(&BigInt::from(7), 2),
            (1.into(), 7.into(), 2)
        );
    }

    #[test]
    fn decimals() {
        assert_eq!(rational_to_sig_digits(&q(1, 3), 5), "0.33333");
        assert_eq!(rational_to_sig_digits(&q(2, 3), 3), "0.667");
        assert_eq!(rational_to_sig_digits(&q(-1, 8), 10), "-0.125");
        assert_eq!(rational_to_sig_digits(&q(999, 1), 2), "1000");
        assert_eq!(rational_to_sig_digits(&q(1, 1_000_000_000), 3), "1e-9");
        assert_eq!(rational_to_fixed(&q(1, 3), 4), "0.3333");
        assert_eq!(rational_to_fixed(&q(-5, 2), 0), "-3");
        assert_eq!(rational_to_fixed(&q(1, 40), 3), "0.025");
        assert_eq!(format_f64(std::f64::consts::PI, 10), "3.141592654");
        assert_eq!(format_f64(1e30, 4), "1e30");
        assert_eq!(format_f64(-0.5, 10), "-0.5");
    }

    #[test]
    fn parse_decimals() {
        assert_eq!(
            Number::parse_decimal("0.25"),
            Some(Number::rational(1, 4).unwrap())
        );
        assert_eq!(Number::parse_decimal("12"), Some(Number::int(12)));
        assert_eq!(
            Number::parse_decimal("1.5e-3"),
            Some(Number::rational(3, 2000).unwrap())
        );
        assert_eq!(Number::parse_decimal("2e3"), Some(Number::int(2000)));
        assert_eq!(Number::parse_decimal("x"), None);
        assert_eq!(Number::parse_decimal("."), None);
    }

    #[test]
    fn combinatorics() {
        assert_eq!(factorial(&BigInt::from(5)).unwrap(), BigInt::from(120));
        assert!(factorial(&BigInt::from(-1)).is_err());
        assert!(factorial(&BigInt::from(10_000_000)).is_err());
        assert_eq!(binomial(&q(5, 1), &BigInt::from(2)).unwrap(), q(10, 1));
        assert_eq!(binomial(&q(5, 1), &BigInt::from(7)).unwrap(), q(0, 1));
        assert_eq!(binomial(&q(1, 2), &BigInt::from(2)).unwrap(), q(-1, 8));
        assert_eq!(gcd_rational(&q(12, 1), &q(18, 1)), q(6, 1));
        assert_eq!(lcm_rational(&q(4, 1), &q(6, 1)), q(12, 1));
        assert_eq!(
            divisors(&BigInt::from(12)).unwrap(),
            vec![1, 2, 3, 4, 6, 12]
                .into_iter()
                .map(BigInt::from)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn display() {
        assert_eq!(Number::rational(-3, 4).unwrap().to_string(), "-3/4");
        assert_eq!(Number::i().to_string(), "i");
        assert_eq!(Number::complex(q(1, 1), q(-2, 1)).to_string(), "1 - 2*i");
        assert_eq!(Number::float(0.1 + 0.2).to_string(), "0.3");
    }
}
