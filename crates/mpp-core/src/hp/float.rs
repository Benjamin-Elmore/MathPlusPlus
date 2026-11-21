//! A binary floating-point number of any precision, `m · 2^e`, and the elementary
//! functions on it. Every operation takes the working precision `p` (bits kept in the
//! mantissa) and is accurate to a few units in the last place at that precision; the
//! driver in [`super`] reruns a whole evaluation at a higher precision to confirm digits.

use std::cell::RefCell;
use std::cmp::Ordering;

use num_bigint::{BigInt, Sign};
use num_integer::Integer;
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};

/// Largest binary exponent a value may reach (beyond it the evaluation gives up).
const MAX_EXP: i64 = 1 << 52;

/// Largest angle (in bits) reduced modulo π/2: reducing `x` takes π to about
/// `log₂|x|` more bits, so `sin(2^(10⁶))` or `cos(30000!)` is refused, not attempted.
pub const REDUCE_BITS: u64 = 1 << 17;

/// `m · 2^e`. Zero is `m = 0`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BigF {
    m: BigInt,
    e: i64,
}

/// Why a high-precision evaluation stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HpError {
    /// The expression uses something this evaluator does not handle.
    Unsupported,
    /// A value outside the domain (a pole, the log of a negative number …).
    Domain,
    /// A value too large or too small even for this format.
    Range,
    /// The query's time limit passed or it was cancelled.
    Stopped,
}

/// Stops the work once the query's budget (deadline or cancel flag) has run out.
pub fn tick() -> HpResult<()> {
    // Ticks come every few hundred big-number steps, which can each take long at a
    // huge precision: read the clock on every one so a deadline is not overshot.
    crate::budget::check_now().map_err(|_| HpError::Stopped)
}

/// True once the query's budget has run out (the work after it is not to be trusted).
pub fn stopped() -> bool {
    crate::budget::peek_tripped().is_some()
}

pub type HpResult<T> = Result<T, HpError>;

fn shl(x: &BigInt, n: i64) -> BigInt {
    if n >= 0 {
        x << n as u64
    } else {
        x >> (-n) as u64
    }
}

/// Divides by `2^n`, rounding to nearest.
fn shr_round(x: &BigInt, n: u64) -> BigInt {
    if n == 0 {
        return x.clone();
    }
    let half = BigInt::one() << (n - 1);
    if x.is_negative() {
        -((-x + half) >> n)
    } else {
        (x + half) >> n
    }
}

impl BigF {
    pub fn zero() -> BigF {
        BigF {
            m: BigInt::zero(),
            e: 0,
        }
    }

    pub fn one() -> BigF {
        BigF::from_int(BigInt::one())
    }

    pub fn from_int(m: BigInt) -> BigF {
        BigF { m, e: 0 }.normal()
    }

    pub fn from_i64(n: i64) -> BigF {
        BigF::from_int(BigInt::from(n))
    }

    /// `m · 2^e` exactly.
    pub fn from_parts(m: BigInt, e: i64) -> BigF {
        BigF { m, e }.normal()
    }

    /// The exact value of a finite `f64`.
    pub fn from_f64(x: f64) -> Option<BigF> {
        if !x.is_finite() {
            return None;
        }
        if x == 0.0 {
            return Some(BigF::zero());
        }
        let bits = x.to_bits();
        let exp = ((bits >> 52) & 0x7ff) as i64;
        let frac = bits & ((1u64 << 52) - 1);
        let (mant, e) = if exp == 0 {
            (frac, -1074)
        } else {
            (frac | (1u64 << 52), exp - 1075)
        };
        let m = BigInt::from(mant);
        Some(BigF::from_parts(if x < 0.0 { -m } else { m }, e))
    }

    /// A rational rounded to `p` bits.
    pub fn from_ratio(q: &BigRational, p: u64) -> BigF {
        if q.is_zero() {
            return BigF::zero();
        }
        let (n, d) = (q.numer(), q.denom());
        if d.is_one() {
            return BigF::from_int(n.clone()).round(p);
        }
        let shift = p as i64 + d.bits() as i64 - n.bits() as i64 + 2;
        let num = shl(n, shift.max(0));
        let den = shl(d, (-shift).max(0));
        BigF::from_parts(div_round(&num, &den), -shift).round(p)
    }

    fn normal(mut self) -> BigF {
        if self.m.is_zero() {
            self.e = 0;
            return self;
        }
        let tz = self.m.trailing_zeros().unwrap_or(0);
        if tz > 0 {
            self.m >>= tz;
            self.e += tz as i64;
        }
        self
    }

    /// Keeps `p` significant bits, rounding to nearest.
    pub fn round(self, p: u64) -> BigF {
        let bits = self.m.bits();
        if bits <= p {
            return self;
        }
        let drop = bits - p;
        BigF {
            m: shr_round(&self.m, drop),
            e: self.e + drop as i64,
        }
        .normal()
    }

    pub fn is_zero(&self) -> bool {
        self.m.is_zero()
    }

    pub fn is_negative(&self) -> bool {
        self.m.is_negative()
    }

    pub fn sign(&self) -> Ordering {
        match self.m.sign() {
            Sign::Minus => Ordering::Less,
            Sign::NoSign => Ordering::Equal,
            Sign::Plus => Ordering::Greater,
        }
    }

    /// `k` with `2^(k−1) ≤ |x| < 2^k` (0 for zero).
    pub fn mag(&self) -> i64 {
        if self.m.is_zero() {
            0
        } else {
            self.e + self.m.bits() as i64
        }
    }

    pub fn exponent(&self) -> i64 {
        self.e
    }

    pub fn mantissa(&self) -> &BigInt {
        &self.m
    }

    pub fn neg(&self) -> BigF {
        BigF {
            m: -&self.m,
            e: self.e,
        }
    }

    pub fn abs(&self) -> BigF {
        BigF {
            m: self.m.abs(),
            e: self.e,
        }
    }

    fn check(self) -> HpResult<BigF> {
        if self.m.is_zero() || self.mag().abs() < MAX_EXP {
            Ok(self)
        } else {
            Err(HpError::Range)
        }
    }

    pub fn add(&self, o: &BigF, p: u64) -> BigF {
        if self.is_zero() {
            return o.clone().round(p);
        }
        if o.is_zero() {
            return self.clone().round(p);
        }
        // A term far below the other's last bit only matters as a sticky bit.
        let (big, small) = if self.mag() >= o.mag() {
            (self, o)
        } else {
            (o, self)
        };
        let gap = big.mag() - small.mag();
        if gap > p as i64 + 4 {
            let e = big.mag() - p as i64 - 8;
            let m = shl(&big.m, big.e - e) + small.m.signum();
            return BigF::from_parts(m, e).round(p);
        }
        let e = self.e.min(o.e);
        let m = shl(&self.m, self.e - e) + shl(&o.m, o.e - e);
        BigF::from_parts(m, e).round(p)
    }

    pub fn sub(&self, o: &BigF, p: u64) -> BigF {
        self.add(&o.neg(), p)
    }

    pub fn mul(&self, o: &BigF, p: u64) -> BigF {
        BigF {
            m: &self.m * &o.m,
            e: self.e + o.e,
        }
        .round(p)
        .normal()
    }

    pub fn mul_int(&self, n: i64, p: u64) -> BigF {
        BigF {
            m: &self.m * n,
            e: self.e,
        }
        .round(p)
        .normal()
    }

    /// `x · 2^n` exactly.
    pub fn ldexp(&self, n: i64) -> BigF {
        BigF {
            m: self.m.clone(),
            e: self.e + n,
        }
    }

    pub fn div(&self, o: &BigF, p: u64) -> HpResult<BigF> {
        if o.is_zero() {
            return Err(HpError::Domain);
        }
        if self.is_zero() {
            return Ok(BigF::zero());
        }
        let shift = p as i64 + o.m.bits() as i64 - self.m.bits() as i64 + 2;
        let num = shl(&self.m, shift.max(0));
        let den = shl(&o.m, (-shift).max(0));
        BigF::from_parts(div_round(&num, &den), self.e - o.e - shift)
            .round(p)
            .check()
    }

    pub fn recip(&self, p: u64) -> HpResult<BigF> {
        BigF::one().div(self, p)
    }

    pub fn sqrt(&self, p: u64) -> HpResult<BigF> {
        if self.is_negative() {
            return Err(HpError::Domain);
        }
        if self.is_zero() {
            return Ok(BigF::zero());
        }
        // Make the exponent even and the mantissa at least 2p + 2 bits long.
        let mut shift = (2 * p as i64 + 4 - self.m.bits() as i64).max(0);
        if (self.e - shift).is_odd() {
            shift += 1;
        }
        let m = &self.m << shift as u64;
        Ok(BigF::from_parts(m.sqrt(), (self.e - shift) / 2).round(p))
    }

    /// `x^n` for an integer `n`.
    pub fn powi(&self, n: &BigInt, p: u64) -> HpResult<BigF> {
        if n.is_zero() {
            return Ok(BigF::one());
        }
        if self.is_zero() {
            return if n.is_negative() {
                Err(HpError::Domain)
            } else {
                Ok(BigF::zero())
            };
        }
        // log₂ of the result, from a short logarithm of the base (the magnitude alone
        // would refuse (1 + 10⁻²⁰)^(10²⁰)).
        let lg = if self.mag().abs() < 1 << 20 {
            ln(&self.abs(), 64)?.to_f64() / std::f64::consts::LN_2
        } else {
            self.mag() as f64
        };
        let nf = n.to_f64().unwrap_or(f64::INFINITY).abs();
        if lg.abs() * nf > MAX_EXP as f64 / 2.0 {
            return Err(HpError::Range);
        }
        let w = p + n.bits() + 16;
        let mut base = self.clone().round(w);
        let mut acc = BigF::one();
        let mut e = n.abs();
        while !e.is_zero() {
            tick()?;
            if e.is_odd() {
                acc = acc.mul(&base, w);
            }
            e >>= 1;
            if !e.is_zero() {
                base = base.mul(&base, w);
            }
        }
        let out = if n.is_negative() { acc.recip(w)? } else { acc };
        out.round(p).check()
    }

    pub fn cmp_abs(&self, o: &BigF) -> Ordering {
        self.abs().cmp_value(&o.abs())
    }

    pub fn cmp_value(&self, o: &BigF) -> Ordering {
        let (sa, sb) = (self.sign(), o.sign());
        if sa != sb {
            return sa.cmp(&sb);
        }
        if sa == Ordering::Equal {
            return Ordering::Equal;
        }
        let by_size = match self.mag().cmp(&o.mag()) {
            Ordering::Equal => {
                // Same magnitude, so aligning the mantissas costs little.
                let e = self.e.min(o.e);
                shl(&self.m.abs(), self.e - e).cmp(&shl(&o.m.abs(), o.e - e))
            }
            other => other,
        };
        if sa == Ordering::Less {
            by_size.reverse()
        } else {
            by_size
        }
    }

    /// The exact value as a rational (only for moderate exponents).
    pub fn to_ratio(&self) -> Option<BigRational> {
        if self.e.abs() > 1 << 20 {
            return None;
        }
        Some(if self.e >= 0 {
            BigRational::from_integer(&self.m << self.e as u64)
        } else {
            BigRational::new(self.m.clone(), BigInt::one() << (-self.e) as u64)
        })
    }

    pub fn to_f64(&self) -> f64 {
        if self.is_zero() {
            return 0.0;
        }
        let r = self.clone().round(60);
        let m = r.m.to_f64().unwrap_or(f64::NAN);
        let e = r.e.clamp(-4000, 4000) as i32;
        m * 2f64.powi(e.clamp(-1000, 1000)) * 2f64.powi(e - e.clamp(-1000, 1000))
    }

    /// The integer nearest to `x` (ties away from zero).
    pub fn round_int(&self) -> BigInt {
        if self.e >= 0 {
            return &self.m << self.e as u64;
        }
        shr_round(&self.m, (-self.e) as u64)
    }

    /// `⌊x⌋`.
    pub fn floor_int(&self) -> BigInt {
        if self.e >= 0 {
            return &self.m << self.e as u64;
        }
        self.m.div_floor(&(BigInt::one() << (-self.e) as u64))
    }

    /// The value in fixed point: `round(x · 2^w)`.
    fn fixed(&self, w: u64) -> BigInt {
        let s = self.e + w as i64;
        if s >= 0 {
            &self.m << s as u64
        } else {
            shr_round(&self.m, (-s) as u64)
        }
    }
}

/// `a · b / 2^w`, truncated toward zero (so a series of shrinking terms reaches 0).
fn mul_shr(a: &BigInt, b: &BigInt, w: u64) -> BigInt {
    let prod = a * b;
    if prod.is_negative() {
        -((-prod) >> w)
    } else {
        prod >> w
    }
}

fn div_round(n: &BigInt, d: &BigInt) -> BigInt {
    let (q, r) = n.div_mod_floor(d);
    if (&r << 1u32) >= d.abs() {
        q + 1
    } else {
        q
    }
}

// ---- constants ----------------------------------------------------------------

thread_local! {
    static PI: RefCell<Option<(u64, BigF)>> = const { RefCell::new(None) };
    static LN2: RefCell<Option<(u64, BigF)>> = const { RefCell::new(None) };
}

fn cached(
    cell: &'static std::thread::LocalKey<RefCell<Option<(u64, BigF)>>>,
    p: u64,
    make: fn(u64) -> (BigF, bool),
) -> BigF {
    cell.with(|c| {
        if let Some((have, v)) = c.borrow().as_ref() {
            if *have >= p {
                return v.clone().round(p);
            }
        }
        let want = p.max(256) * 3 / 2;
        let (v, complete) = make(want);
        // A value cut short by the query's budget is wrong from where its series
        // stopped: the caller sees `stopped()` and discards it, and it is never kept
        // for a later query on this thread.
        if complete {
            *c.borrow_mut() = Some((want, v.clone()));
        }
        v.round(p)
    })
}

/// `Σ (−1)^j / ((2j+1) k^(2j+1))` in fixed point with `w` bits (`atan(1/k)`), with
/// whether the series was summed in full (false when the query's budget ran out).
fn atan_inv(k: u64, w: u64, alternate: bool) -> (BigInt, bool) {
    let k = BigInt::from(k);
    let k2 = &k * &k;
    let mut term = (BigInt::one() << w) / &k;
    let mut sum = BigInt::zero();
    let mut j: u64 = 0;
    while !term.is_zero() {
        // A huge precision (π for reducing an enormous angle) takes long: stop when the
        // query's budget runs out.
        if j % 512 == 511 && tick().is_err() {
            return (sum, false);
        }
        let t = &term / BigInt::from(2 * j + 1);
        if alternate && j % 2 == 1 {
            sum -= t;
        } else {
            sum += t;
        }
        term /= &k2;
        j += 1;
    }
    (sum, true)
}

fn make_pi(p: u64) -> (BigF, bool) {
    let w = p + 32;
    let (a, a_done) = atan_inv(5, w, true);
    let (b, b_done) = atan_inv(239, w, true);
    let v = a * 16 - b * 4;
    (BigF::from_parts(v, -(w as i64)).round(p), a_done && b_done)
}

fn make_ln2(p: u64) -> (BigF, bool) {
    // ln 2 = 2 atanh(1/3).
    let w = p + 32;
    let (a, done) = atan_inv(3, w, false);
    (BigF::from_parts(a * 2, -(w as i64)).round(p), done)
}

pub fn pi(p: u64) -> BigF {
    cached(&PI, p, make_pi)
}

pub fn ln2(p: u64) -> BigF {
    cached(&LN2, p, make_ln2)
}

// ---- exp and ln ---------------------------------------------------------------

pub fn exp(x: &BigF, p: u64) -> HpResult<BigF> {
    if x.is_zero() {
        return Ok(BigF::one());
    }
    if x.mag() > 62 {
        return Err(HpError::Range);
    }
    let w0 = p + 24 + x.mag().max(0) as u64;
    // x = k ln 2 + r with |r| ≤ ln 2 / 2.
    let l2 = ln2(w0 + 8);
    let k = x.div(&l2, w0)?.round_int();
    let r = x.sub(&l2.mul(&BigF::from_int(k.clone()), w0 + 8), w0);
    let s = ((p as f64).sqrt() as u64 / 2).max(4);
    let w = p + s + 32;
    // e^r = (e^(r / 2^s))^(2^s); the Taylor series runs on r / 2^s.
    let rr = r.fixed(w) >> s;
    let one = BigInt::one() << w;
    let mut sum = one.clone();
    let mut term = one;
    let mut n = 1u64;
    loop {
        term = mul_shr(&term, &rr, w) / BigInt::from(n);
        if term.is_zero() {
            break;
        }
        sum += &term;
        n += 1;
    }
    for _ in 0..s {
        sum = (&sum * &sum) >> w;
    }
    let k = k.to_i64().ok_or(HpError::Range)?;
    BigF::from_parts(sum, k - w as i64).round(p).check()
}

/// `2 atanh z` for `|z| ≤ 1/3`, in fixed point with `w` bits.
fn atanh_fixed(z: &BigInt, w: u64) -> BigInt {
    let z2 = (z * z) >> w;
    let mut term = z.clone();
    let mut sum = BigInt::zero();
    let mut n = 1u64;
    while !term.is_zero() {
        sum += &term / BigInt::from(n);
        term = mul_shr(&term, &z2, w);
        n += 2;
    }
    sum << 1
}

pub fn ln(x: &BigF, p: u64) -> HpResult<BigF> {
    if x.sign() != Ordering::Greater {
        return Err(HpError::Domain);
    }
    // x = y · 2^k with y in [3/4, 3/2).
    let bits = x.m.bits() as i64;
    let mut k = x.e + bits;
    let mut y = BigF::from_parts(x.m.clone(), -bits);
    if y.cmp_value(&BigF::from_parts(BigInt::from(3), -2)) == Ordering::Less {
        y = y.ldexp(1);
        k -= 1;
    }
    let w = p + 32;
    let yf = y.fixed(w);
    let one = BigInt::one() << w;
    let z = ((&yf - &one) << w) / (&yf + &one);
    let lny = BigF::from_parts(atanh_fixed(&z, w), -(w as i64));
    if k == 0 {
        return Ok(lny.round(p));
    }
    let kb = 64 - k.unsigned_abs().leading_zeros() as u64;
    let l2 = ln2(p + kb + 16);
    Ok(lny.add(&l2.mul_int(k, p + kb + 16), p))
}

// ---- trigonometry -------------------------------------------------------------

/// `(sin r, cos r)` for `|r| ≤ 1`, in fixed point with `w` bits.
fn sin_cos_fixed(r: &BigInt, w: u64) -> (BigInt, BigInt) {
    let r2 = (r * r) >> w;
    let one = BigInt::one() << w;
    // sin
    let mut term = r.clone();
    let mut s = r.clone();
    let mut n = 1u64;
    loop {
        term = -mul_shr(&term, &r2, w) / BigInt::from((n + 1) * (n + 2));
        if term.is_zero() {
            break;
        }
        s += &term;
        n += 2;
    }
    // cos
    let mut term = one.clone();
    let mut c = one;
    let mut n = 0u64;
    loop {
        term = -mul_shr(&term, &r2, w) / BigInt::from((n + 1) * (n + 2));
        if term.is_zero() {
            break;
        }
        c += &term;
        n += 2;
    }
    (s, c)
}

/// `(sin x, cos x)`.
pub fn sin_cos(x: &BigF, p: u64) -> HpResult<(BigF, BigF)> {
    if x.is_zero() {
        return Ok((BigF::zero(), BigF::one()));
    }
    if x.mag() > REDUCE_BITS as i64 {
        return Err(HpError::Range);
    }
    let extra = x.mag().max(0) as u64;
    let w0 = p + extra + 32;
    let half_pi = pi(w0 + 8).ldexp(-1);
    let n = x.div(&half_pi, w0)?.round_int();
    let r = x.sub(&half_pi.mul(&BigF::from_int(n.clone()), w0 + 8), w0);
    // Precision relative to the reduced argument (it may be tiny next to x).
    let w = p + 32 + (-r.mag()).max(0) as u64;
    let (s, c) = sin_cos_fixed(&r.fixed(w), w);
    let (s, c) = (
        BigF::from_parts(s, -(w as i64)),
        BigF::from_parts(c, -(w as i64)),
    );
    let q = n.mod_floor(&BigInt::from(4)).to_u8().unwrap_or(0);
    let (s, c) = match q {
        0 => (s, c),
        1 => (c, s.neg()),
        2 => (s.neg(), c.neg()),
        _ => (c.neg(), s),
    };
    Ok((s.round(p), c.round(p)))
}

/// `atan x`.
pub fn atan(x: &BigF, p: u64) -> HpResult<BigF> {
    if x.is_zero() {
        return Ok(BigF::zero());
    }
    let w = p + 40;
    if x.mag() > 0 && x.cmp_abs(&BigF::one()) == Ordering::Greater {
        // atan x = ±π/2 − atan(1/x).
        let inv = atan(&x.recip(w)?, w)?;
        let hp = pi(w).ldexp(-1);
        let hp = if x.is_negative() { hp.neg() } else { hp };
        return Ok(hp.sub(&inv, p));
    }
    // Halve the angle until it is small: atan x = 2 atan(x / (1 + √(1 + x²))).
    let mut y = x.clone().round(w);
    let mut doublings = 0;
    while y.mag() > -10 {
        let d = BigF::one().add(&y.mul(&y, w), w).sqrt(w)?;
        y = y.div(&BigF::one().add(&d, w), w)?;
        doublings += 1;
    }
    let wf = w + (-y.mag()).max(0) as u64;
    let yf = y.fixed(wf);
    let y2 = (&yf * &yf) >> wf;
    let mut term = yf.clone();
    let mut sum = BigInt::zero();
    let mut n = 1u64;
    let mut sign = true;
    while !term.is_zero() {
        let t = &term / BigInt::from(n);
        if sign {
            sum += t;
        } else {
            sum -= t;
        }
        term = mul_shr(&term, &y2, wf);
        n += 2;
        sign = !sign;
    }
    Ok(BigF::from_parts(sum, doublings - wf as i64).round(p))
}

/// `asin x` for `|x| ≤ 1`.
pub fn asin(x: &BigF, p: u64) -> HpResult<BigF> {
    let w = p + 40;
    let one = BigF::one();
    match x.cmp_abs(&one) {
        Ordering::Greater => Err(HpError::Domain),
        Ordering::Equal => {
            let hp = pi(p).ldexp(-1);
            Ok(if x.is_negative() { hp.neg() } else { hp })
        }
        Ordering::Less => {
            let c = one.sub(&x.mul(x, 2 * w), w).sqrt(w)?;
            atan(&x.div(&c, w)?, p)
        }
    }
}

pub fn atan2(y: &BigF, x: &BigF, p: u64) -> HpResult<BigF> {
    let w = p + 16;
    if x.is_zero() {
        return match y.sign() {
            Ordering::Equal => Err(HpError::Domain),
            Ordering::Greater => Ok(pi(p).ldexp(-1)),
            Ordering::Less => Ok(pi(p).ldexp(-1).neg()),
        };
    }
    let a = atan(&y.div(x, w)?, w)?;
    if !x.is_negative() {
        return Ok(a.round(p));
    }
    let pi = pi(w);
    Ok(if y.is_negative() {
        a.sub(&pi, p)
    } else {
        a.add(&pi, p)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(x: &BigF, want: f64) -> bool {
        (x.to_f64() - want).abs() <= 1e-15 * want.abs().max(1.0)
    }

    #[test]
    fn constants_and_functions() {
        assert!(close(&pi(200), std::f64::consts::PI));
        assert!(close(&ln2(200), std::f64::consts::LN_2));
        let half = BigF::from_ratio(&BigRational::new(1.into(), 2.into()), 200);
        assert!(close(&exp(&half, 200).unwrap(), 0.5f64.exp()));
        assert!(close(&ln(&BigF::from_i64(10), 200).unwrap(), 10f64.ln()));
        let (s, c) = sin_cos(&BigF::from_i64(100), 200).unwrap();
        assert!(close(&s, 100f64.sin()) && close(&c, 100f64.cos()));
        assert!(close(&atan(&BigF::from_i64(3), 200).unwrap(), 3f64.atan()));
        assert!(close(&asin(&half, 200).unwrap(), 0.5f64.asin()));
        assert!(close(&BigF::from_i64(2).sqrt(200).unwrap(), 2f64.sqrt()));
        let big = BigF::from_i64(10).powi(&BigInt::from(400), 200).unwrap();
        assert!(close(&ln(&big, 200).unwrap(), 400.0 * 10f64.ln()));
    }

    /// A π or ln 2 cut short by a stopped query is never kept: a later query on the
    /// same thread gets the right digits, as a fresh thread does.
    #[test]
    fn a_stopped_constant_is_not_kept_for_later_queries() {
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;
        let fresh = std::thread::spawn(|| (pi(13_000), ln2(13_000)))
            .join()
            .expect("thread");
        std::thread::spawn(move || {
            {
                // A cancelled budget: the series stop at their first tick.
                let cancel = Arc::new(AtomicBool::new(true));
                let _guard = crate::budget::install(crate::budget::Budget::new(cancel, None));
                let _ = pi(400_000);
                let _ = ln2(400_000);
                assert!(stopped());
            }
            let _ = crate::budget::take_tripped();
            assert_eq!(pi(13_000), fresh.0, "π kept from a stopped run");
            assert_eq!(ln2(13_000), fresh.1, "ln 2 kept from a stopped run");
        })
        .join()
        .expect("thread");
    }
}
