//! More special functions at high precision: erf and erfc, digamma, polygamma, Euler's
//! γ, Bessel `J_n`, the sine and cosine integrals, the complete elliptic integrals
//! `K` and `E` (modulus convention) and the principal branch of Lambert `W`.
//!
//! Each works at a few guard bits past the precision asked for. Series whose terms
//! cancel (Bessel, Si, Ci, erfc) are summed with extra bits for the cancellation; any
//! loss left over shows up as disagreement between two precisions in the driver, which
//! then retries with more bits.

use std::cmp::Ordering;

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, ToPrimitive};

use super::float::{exp, ln, pi, sin_cos, tick, BigF, HpError, HpResult};
use super::special::{bernoulli_number, hurwitz};

/// Largest argument (in absolute value) of the cancelling series.
const MAX_SERIES_ARG: f64 = 2000.0;

fn small(n: i64) -> BigF {
    BigF::from_i64(n)
}

/// `2/√π`.
fn two_over_sqrt_pi(w: u64) -> HpResult<BigF> {
    pi(w).sqrt(w)?.recip(w).map(|v| v.ldexp(1))
}

/// `erf x` (all real `x`).
pub fn erf(x: &BigF, p: u64) -> HpResult<BigF> {
    if x.is_zero() {
        return Ok(BigF::zero());
    }
    let a = x.abs();
    let af = a.to_f64();
    let neg = x.is_negative();
    // erfc a < e^(−a²): below the last bit, erf is ±1.
    if af * af * std::f64::consts::LOG2_E > p as f64 + 8.0 {
        let one = BigF::one();
        return Ok(if neg { one.neg() } else { one });
    }
    let v = erf_positive(&a, p)?;
    Ok(if neg { v.neg() } else { v })
}

/// `erf a` for `a > 0` by the series of positive terms
/// `erf a = (2/√π) e^(−a²) Σ 2ⁿ a^(2n+1) / (1·3·…·(2n+1))`.
fn erf_positive(a: &BigF, p: u64) -> HpResult<BigF> {
    let af = a.to_f64();
    let terms = (2.0 * af * af + p as f64).max(8.0);
    let w = p + 24 + terms.log2().ceil() as u64;
    let a2 = a.mul(a, w);
    let two_a2 = a2.ldexp(1);
    let mut term = a.clone().round(w);
    let mut sum = term.clone();
    let mut n = 1i64;
    loop {
        tick()?;
        term = term.mul(&two_a2, w).div(&small(2 * n + 1), w)?;
        sum = sum.add(&term, w);
        if n as f64 > af * af && (term.is_zero() || term.mag() < sum.mag() - w as i64 - 2) {
            break;
        }
        n += 1;
        if n > 4_000_000 {
            return Err(HpError::Unsupported);
        }
    }
    let damp = exp(&a2.neg(), w)?;
    Ok(two_over_sqrt_pi(w)?.mul(&damp, w).mul(&sum, p))
}

/// `erfc x = 1 − erf x`, with extra bits for the cancellation when `x > 0`.
pub fn erfc(x: &BigF, p: u64) -> HpResult<BigF> {
    if x.sign() != Ordering::Greater {
        return Ok(BigF::one().sub(&erf(x, p + 8)?, p));
    }
    let xf = x.to_f64();
    let lost = xf * xf * std::f64::consts::LOG2_E + 8.0;
    if lost > 40_000.0 {
        return Err(HpError::Unsupported);
    }
    let w = p + lost as u64 + 16;
    let e = erf_positive(&x.abs(), w)?;
    Ok(BigF::one().sub(&e, w).round(p))
}

/// The digamma function `ψ(x) = Γ′(x)/Γ(x)` for real `x` (not a pole).
pub fn digamma(x: &BigF, p: u64) -> HpResult<BigF> {
    let w = p + 32 + x.mag().max(0) as u64;
    let half = BigF::from_parts(BigInt::one(), -1);
    if x.cmp_value(&half) == Ordering::Less {
        // Reflection: ψ(x) = ψ(1 − x) − π cot(πx).
        let wx = w + x.mag().max(0) as u64;
        let (s, c) = sin_cos(&pi(wx).mul(x, wx), w)?;
        if s.is_zero() || s.mag() < -(w as i64) / 2 {
            return Err(HpError::Domain);
        }
        let cot = c.div(&s, w)?;
        let r = digamma(&BigF::one().sub(x, wx), w)?;
        return Ok(r.sub(&pi(w).mul(&cot, w), p));
    }
    // Shift up: ψ(x) = ψ(x + N) − Σ_{k<N} 1/(x + k).
    let x0 = (w / 8 + 12) as f64;
    let mut y = x.clone().round(w);
    let mut acc = BigF::zero();
    if y.mag() < 62 {
        let start = y.to_f64();
        if start < x0 {
            let n = (x0 - start).ceil() as i64;
            for _ in 0..n {
                tick()?;
                acc = acc.add(&y.recip(w)?, w);
                y = y.add(&BigF::one(), w);
            }
        }
    }
    // ψ(y) ≈ ln y − 1/(2y) − Σ B_2k / (2k y^(2k)).
    let inv = y.recip(w)?;
    let inv2 = inv.mul(&inv, w);
    let mut s = ln(&y, w)?.sub(&inv.ldexp(-1), w);
    let mut pw = inv2.clone();
    let limit = s.mag().max(0) - w as i64 - 4;
    let mut done = false;
    for k in 1..400usize {
        let b = bernoulli_number(2 * k).ok_or(HpError::Unsupported)?;
        let c = b / BigRational::from_integer(BigInt::from(2 * k));
        let term = BigF::from_ratio(&c, w).mul(&pw, w);
        s = s.sub(&term, w);
        if term.is_zero() || term.mag() < limit {
            done = true;
            break;
        }
        pw = pw.mul(&inv2, w);
    }
    if !done {
        return Err(HpError::Unsupported);
    }
    Ok(s.sub(&acc, p))
}

/// Euler's constant `γ = −ψ(1)`.
pub fn euler_gamma(p: u64) -> HpResult<BigF> {
    Ok(digamma(&BigF::one(), p + 8)?.neg().round(p))
}

/// `ψ⁽ᵐ⁾(x)` for a whole `m ≥ 0` and `x > 0`: `(−1)^(m+1) m! ζ(m+1, x)` for `m ≥ 1`.
pub fn polygamma(m: &BigF, x: &BigF, p: u64) -> HpResult<BigF> {
    let m = whole(m).ok_or(HpError::Unsupported)?;
    if m == 0 {
        return digamma(x, p);
    }
    if !(0..=1000).contains(&m) {
        return Err(HpError::Unsupported);
    }
    if x.sign() != Ordering::Greater {
        return Err(HpError::Unsupported);
    }
    let w = p + 16;
    let z = hurwitz(&small(m + 1), x, w)?;
    let mut f = BigInt::one();
    for j in 2..=m {
        f *= j;
    }
    let v = z.mul(&BigF::from_int(f), p);
    Ok(if m % 2 == 0 { v.neg() } else { v })
}

/// The integer value of `x` when it is a (small) whole number.
fn whole(x: &BigF) -> Option<i64> {
    if x.exponent() < 0 {
        return None;
    }
    x.round_int().to_i64()
}

/// The number of bits the terms of a series in `x` may cancel (about `|x| log₂ e`).
fn cancel_bits(x: &BigF) -> HpResult<u64> {
    let xf = x.abs().to_f64();
    if xf > MAX_SERIES_ARG {
        return Err(HpError::Unsupported);
    }
    Ok((xf * std::f64::consts::LOG2_E * 1.05) as u64 + 24)
}

/// Bessel `J_n(x)` for a whole order `n` by its power series.
pub fn besselj(n: &BigF, x: &BigF, p: u64) -> HpResult<BigF> {
    let n = whole(n).ok_or(HpError::Unsupported)?;
    if n.abs() > 100_000 {
        return Err(HpError::Unsupported);
    }
    let sign_flip = n < 0 && n % 2 != 0;
    let n = n.abs();
    if x.is_zero() {
        return Ok(if n == 0 { BigF::one() } else { BigF::zero() });
    }
    let w = p + cancel_bits(x)? + 16;
    let h = x.ldexp(-1).round(w);
    let h2 = h.mul(&h, w).neg();
    // First term (x/2)^n / n!.
    let mut term = h.powi(&BigInt::from(n), w)?;
    for j in 2..=n {
        term = term.div(&small(j), w)?;
    }
    let mut sum = term.clone();
    let mut big = term.mag();
    let xf = x.abs().to_f64();
    let mut k = 1i64;
    loop {
        tick()?;
        term = term.mul(&h2, w).div(&small(k * (k + n)), w)?;
        sum = sum.add(&term, w);
        big = big.max(term.mag());
        if k as f64 > xf && (term.is_zero() || term.mag() < big - w as i64 - 2) {
            break;
        }
        k += 1;
    }
    let v = sum.round(p);
    Ok(if sign_flip { v.neg() } else { v })
}

/// The sine integral `Si x = Σ (−1)^k x^(2k+1) / ((2k+1)(2k+1)!)`.
pub fn si(x: &BigF, p: u64) -> HpResult<BigF> {
    if x.is_zero() {
        return Ok(BigF::zero());
    }
    let w = p + cancel_bits(x)? + 16;
    let x2 = x.mul(x, w).neg();
    let mut t = x.clone().round(w);
    let mut sum = t.clone();
    let mut big = sum.mag();
    let xf = x.abs().to_f64();
    let mut k = 0i64;
    loop {
        tick()?;
        t = t.mul(&x2, w).div(&small((2 * k + 2) * (2 * k + 3)), w)?;
        let term = t.div(&small(2 * k + 3), w)?;
        sum = sum.add(&term, w);
        big = big.max(term.mag());
        k += 1;
        if k as f64 > xf && (term.is_zero() || term.mag() < big - w as i64 - 2) {
            break;
        }
    }
    Ok(sum.round(p))
}

/// The cosine integral `Ci x = γ + ln x + Σ_{k≥1} (−1)^k x^(2k) / (2k (2k)!)` for `x > 0`.
pub fn ci(x: &BigF, p: u64) -> HpResult<BigF> {
    if x.sign() != Ordering::Greater {
        return Err(HpError::Domain);
    }
    let w = p + cancel_bits(x)? + 16;
    let x2 = x.mul(x, w).neg();
    let mut u = BigF::one();
    let mut sum = euler_gamma(w)?.add(&ln(x, w)?, w);
    let mut big = sum.mag().max(0);
    let xf = x.to_f64();
    let mut k = 1i64;
    loop {
        tick()?;
        u = u.mul(&x2, w).div(&small((2 * k - 1) * (2 * k)), w)?;
        let term = u.div(&small(2 * k), w)?;
        sum = sum.add(&term, w);
        big = big.max(term.mag());
        if k as f64 > xf && (term.is_zero() || term.mag() < big - w as i64 - 2) {
            break;
        }
        k += 1;
    }
    Ok(sum.round(p))
}

/// The arithmetic–geometric mean steps from `(1, k')`: returns `(AGM, Σ 2^(n−1) c_n²)`.
fn agm(kp: &BigF, k: &BigF, w: u64) -> HpResult<(BigF, BigF)> {
    let mut a = BigF::one();
    let mut b = kp.clone();
    let mut c = k.clone();
    // n = 0 term: 2^(−1) c₀².
    let mut sum = c.mul(&c, w).ldexp(-1);
    let mut pow = 0i64;
    for _ in 0..200 {
        if c.is_zero() || c.mag() < -(w as i64) / 2 - 4 {
            return Ok((a, sum));
        }
        let a1 = a.add(&b, w).ldexp(-1);
        let b1 = a.mul(&b, w).sqrt(w)?;
        c = a.sub(&b, w).ldexp(-1);
        a = a1;
        b = b1;
        pow += 1;
        sum = sum.add(&c.mul(&c, w).ldexp(pow - 1), w);
    }
    Err(HpError::Unsupported)
}

/// `(k, k')` for a modulus `|k| < 1`.
fn modulus(k: &BigF, w: u64) -> HpResult<BigF> {
    let one = BigF::one();
    if k.cmp_abs(&one) != Ordering::Less {
        return Err(HpError::Domain);
    }
    one.sub(&k.mul(k, w), w).sqrt(w)
}

/// The complete elliptic integral of the first kind `K(k) = π / (2 AGM(1, √(1−k²)))`.
pub fn ellipk(k: &BigF, p: u64) -> HpResult<BigF> {
    let w = p + 32 + (-k.mag()).max(0) as u64;
    let kp = modulus(k, w)?;
    let (m, _) = agm(&kp, k, w)?;
    pi(w).div(&m.ldexp(1), p)
}

/// The complete elliptic integral of the second kind `E(k) = K(k) (1 − Σ 2^(n−1) c_n²)`.
pub fn ellipe(k: &BigF, p: u64) -> HpResult<BigF> {
    let w = p + 48;
    let kp = modulus(k, w)?;
    let (m, s) = agm(&kp, k, w)?;
    let kk = pi(w).div(&m.ldexp(1), w)?;
    Ok(kk.mul(&BigF::one().sub(&s, w), p))
}

/// The principal branch `W₀(x)` of Lambert's function, `x ≥ −1/e`, by Newton steps from
/// a float start.
pub fn lambert_w0(x: &BigF, p: u64) -> HpResult<BigF> {
    if x.is_zero() {
        return Ok(BigF::zero());
    }
    let xf = x.to_f64();
    if !xf.is_finite() || xf.abs() > 1e300 {
        return Err(HpError::Unsupported);
    }
    let w = p + 32;
    // −1/e ≤ x: test in high precision; the branch point itself is −1.
    let minus_inv_e = exp(&BigF::one(), w)?.recip(w)?.neg();
    match x.cmp_value(&minus_inv_e) {
        Ordering::Less => return Err(HpError::Domain),
        Ordering::Equal => return Ok(small(-1)),
        Ordering::Greater => {}
    }
    // Near the branch point the float start and Newton both struggle.
    if x.sub(&minus_inv_e, w).to_f64() < 1e-6 {
        return Err(HpError::Unsupported);
    }
    let start = crate::domains::special::lambert::lambert_w(xf, 0).map_err(|_| HpError::Domain)?;
    let mut y = BigF::from_f64(start).ok_or(HpError::Range)?;
    let one = BigF::one();
    let mut prec = 48u64;
    loop {
        tick()?;
        prec = (prec * 2).min(w);
        // Newton: y ← y − (y e^y − x) / (e^y (y + 1)).
        let ey = exp(&y, prec + 16)?;
        let f = y.mul(&ey, prec + 16).sub(x, prec + 16);
        let d = ey.mul(&y.add(&one, prec + 16), prec + 16);
        if d.is_zero() {
            return Err(HpError::Unsupported);
        }
        y = y.sub(&f.div(&d, prec + 16)?, prec + 16);
        if prec >= w {
            break;
        }
    }
    // Two more full-precision steps to be safe.
    for _ in 0..2 {
        let ey = exp(&y, w)?;
        let f = y.mul(&ey, w).sub(x, w);
        let d = ey.mul(&y.add(&one, w), w);
        y = y.sub(&f.div(&d, w)?, w);
    }
    if y.is_zero() {
        return Ok(y);
    }
    Ok(y.round(p))
}

/// `B(a, b) = Γ(a) Γ(b) / Γ(a + b)`.
pub fn beta(a: &BigF, b: &BigF, p: u64) -> HpResult<BigF> {
    use super::special::gamma;
    let w = p + 24;
    let ga = gamma(a, w)?;
    let gb = gamma(b, w)?;
    let gab = gamma(&a.add(b, w), w)?;
    if gab.is_zero() {
        return Err(HpError::Domain);
    }
    ga.mul(&gb, w).div(&gab, p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digits(x: &BigF, sig: usize) -> String {
        let d = super::super::to_decimal(x, sig).unwrap();
        format!("{}{}e{}", if d.neg { "-" } else { "" }, d.digits, d.k)
    }

    fn q(n: i64, d: i64) -> BigF {
        BigF::from_ratio(&BigRational::new(n.into(), d.into()), 300)
    }

    // References: mpmath at 50 digits.
    #[test]
    fn values_to_thirty_digits() {
        let p = 140;
        assert_eq!(
            digits(&erf(&BigF::one(), p).unwrap(), 30),
            "842700792949714869341220635083e-1"
        );
        assert_eq!(
            digits(&digamma(&q(1, 3), p).unwrap(), 30),
            "-313203378002080632299641907429e0"
        );
        assert_eq!(
            digits(&euler_gamma(p).unwrap(), 30),
            "577215664901532860606512090082e-1"
        );
        assert_eq!(
            digits(&besselj(&BigF::zero(), &BigF::one(), p).unwrap(), 30),
            "765197686557966551449717526103e-1"
        );
    }

    #[test]
    fn signs_and_edges() {
        let p = 100;
        assert!(erf(&small(-2), p).unwrap().is_negative());
        assert_eq!(erf(&small(100), p).unwrap(), BigF::one());
        assert!(digamma(&small(-1), p).is_err());
        assert!(ellipk(&BigF::one(), p).is_err());
        let _ = ci(&small(1), p).unwrap();
        let _ = si(&small(1), p).unwrap();
        let _ = ellipe(&q(1, 2), p).unwrap();
        let _ = lambert_w0(&BigF::one(), p).unwrap();
        let _ = polygamma(&small(1), &small(1), p).unwrap();
        let _ = erfc(&small(3), p).unwrap();
        let _ = beta(&small(2), &small(3), p).unwrap();
    }
}
