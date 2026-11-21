//! Γ and the Hurwitz ζ function at high precision (Stirling's series and the
//! Euler–Maclaurin formula, both with Bernoulli numbers).

use std::cell::RefCell;
use std::cmp::Ordering;

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, ToPrimitive, Zero};

use super::float::{exp, ln, pi, sin_cos, BigF, HpError, HpResult};

/// Largest `n` for which `Γ(n) = (n−1)!` is computed exactly.
const EXACT_FACTORIAL: u64 = 3000;
/// Largest Bernoulli index used.
const MAX_BERNOULLI: usize = 300;

thread_local! {
    static BERNOULLI: RefCell<Vec<BigRational>> = const { RefCell::new(Vec::new()) };
}

/// `B_n` (with `B₁ = −1/2`), or `None` past [`MAX_BERNOULLI`]. The table grows only as
/// far as asked, since the recurrence is quadratic and the large entries are costly.
fn bernoulli(n: usize) -> Option<BigRational> {
    if n > MAX_BERNOULLI {
        return None;
    }
    BERNOULLI.with(|b| {
        let mut b = b.borrow_mut();
        while b.len() <= n {
            let m = b.len();
            let next = if m == 0 {
                BigRational::one()
            } else if m > 1 && m % 2 == 1 {
                BigRational::zero()
            } else {
                // Σ_{i<m} C(m+1, i) B_i + (m+1) B_m = 0.
                let mut acc = BigRational::zero();
                let mut binom = BigInt::one();
                for (i, bi) in b.iter().enumerate() {
                    if !bi.is_zero() {
                        acc += bi * BigRational::from_integer(binom.clone());
                    }
                    binom = binom * BigInt::from(m + 1 - i) / BigInt::from(i + 1);
                }
                -acc / BigRational::from_integer(BigInt::from(m + 1))
            };
            b.push(next);
        }
        b.get(n).cloned()
    })
}

fn ratio(q: &BigRational, p: u64) -> BigF {
    BigF::from_ratio(q, p)
}

/// The integer value of `x`, when it is one.
fn as_integer(x: &BigF) -> Option<BigInt> {
    (x.exponent() >= 0).then(|| x.round_int())
}

/// `ln Γ(x)` for `x > 0`.
pub fn ln_gamma(x: &BigF, p: u64) -> HpResult<BigF> {
    if x.sign() != Ordering::Greater {
        return Err(HpError::Domain);
    }
    let w = p + 32 + x.mag().max(0) as u64;
    // Shift up until Stirling's series reaches the precision: Γ(x) = Γ(x+N) / ∏ (x+j).
    let x0 = (w / 2 + 16) as i64;
    let mut y = x.clone().round(w);
    let mut prod = BigF::one();
    let mut shifted = false;
    if y.mag() < 62 {
        let start = y.to_f64();
        if start < x0 as f64 {
            let n = (x0 as f64 - start).ceil() as i64;
            for _ in 0..n {
                prod = prod.mul(&y, w);
                y = y.add(&BigF::one(), w);
            }
            shifted = true;
        }
    }
    // (y − ½) ln y − y + ½ ln 2π + Σ B_2m / (2m (2m−1) y^(2m−1)).
    let ly = ln(&y, w)?;
    let half = BigF::from_parts(BigInt::one(), -1);
    let mut s = y.sub(&half, w).mul(&ly, w).sub(&y, w);
    let two_pi = pi(w).ldexp(1);
    s = s.add(&ln(&two_pi, w)?.ldexp(-1), w);
    let inv = y.recip(w)?;
    let inv2 = inv.mul(&inv, w);
    let mut pw = inv.clone();
    let limit = s.mag().max(0) - w as i64 - 4;
    let mut converged = false;
    for m in 1..150usize {
        let b = bernoulli(2 * m).ok_or(HpError::Unsupported)?;
        let c = b / BigRational::from_integer(BigInt::from((2 * m) * (2 * m - 1)));
        let term = ratio(&c, w).mul(&pw, w);
        s = s.add(&term, w);
        if term.is_zero() || term.mag() < limit {
            converged = true;
            break;
        }
        pw = pw.mul(&inv2, w);
    }
    if !converged {
        return Err(HpError::Unsupported);
    }
    if shifted {
        s = s.sub(&ln(&prod, w)?, w);
    }
    Ok(s.round(p))
}

/// `Γ(x)`.
pub fn gamma(x: &BigF, p: u64) -> HpResult<BigF> {
    if let Some(n) = as_integer(x) {
        if n <= BigInt::zero() {
            return Err(HpError::Domain);
        }
        if let Some(k) = n.to_u64().filter(|k| *k <= EXACT_FACTORIAL) {
            let mut f = BigInt::one();
            for j in 2..k {
                f *= j;
            }
            return Ok(BigF::from_int(f).round(p));
        }
    }
    let w = p + 24;
    let half = BigF::from_parts(BigInt::one(), -1);
    if x.cmp_value(&half) == Ordering::Less {
        // Reflection: Γ(x) = π / (sin(πx) Γ(1−x)).
        let wx = w + x.mag().max(0) as u64;
        let (s, _) = sin_cos(&pi(wx).mul(x, wx), w)?;
        if s.is_zero() {
            return Err(HpError::Domain);
        }
        let g = gamma(&BigF::one().sub(x, wx), w)?;
        return pi(w).div(&s.mul(&g, w), p);
    }
    let lg = ln_gamma(x, w + x.mag().max(0) as u64 + 16)?;
    exp(&lg, w).map(|v| v.round(p))
}

/// `(a+k)^(−s)`.
fn neg_power(base: &BigF, s: &BigF, s_int: Option<&BigInt>, p: u64) -> HpResult<BigF> {
    match s_int {
        Some(n) => base.powi(&-n, p),
        None => {
            let w = p + 16 + s.mag().max(0) as u64 + 8;
            exp(&s.neg().mul(&ln(base, w)?, w), p)
        }
    }
}

/// The Hurwitz ζ function `ζ(s, a) = Σ_{k≥0} (a+k)^(−s)` for real `s ≠ 1` and `a > 0`.
pub fn hurwitz(s: &BigF, a: &BigF, p: u64) -> HpResult<BigF> {
    if a.sign() != Ordering::Greater {
        return Err(HpError::Domain);
    }
    let one = BigF::one();
    if s.cmp_value(&one) == Ordering::Equal {
        return Err(HpError::Domain);
    }
    let sf = s.to_f64();
    if !(-40.0..=1e6).contains(&sf) {
        return Err(HpError::Unsupported);
    }
    let w = p + 40;
    let s_int = as_integer(s);
    let n = (w as f64 / 4.0 + sf.abs() + 8.0).ceil() as i64;
    let mut sum = BigF::zero();
    let mut base = a.clone().round(w);
    for _ in 0..n {
        sum = sum.add(&neg_power(&base, s, s_int.as_ref(), w)?, w);
        base = base.add(&one, w);
    }
    // Tail: (a+N)^(1−s)/(s−1) + (a+N)^(−s)/2 + Σ B_2j/(2j)! (s)_(2j−1) (a+N)^(−s−2j+1).
    let t0 = neg_power(&base, s, s_int.as_ref(), w)?;
    let sm1 = s.sub(&one, w);
    sum = sum.add(&t0.mul(&base, w).div(&sm1, w)?, w);
    sum = sum.add(&t0.ldexp(-1), w);
    let inv = base.recip(w)?;
    let inv2 = inv.mul(&inv, w);
    // rising = s (s+1) … (s+2j−2), pw = (a+N)^(−s−2j+1).
    let mut rising = s.clone();
    let mut fact = BigInt::from(2);
    let mut pw = t0.mul(&inv, w);
    let limit = sum.mag() - w as i64 - 4;
    for j in 1..150usize {
        let b = bernoulli(2 * j).ok_or(HpError::Unsupported)?;
        let c = ratio(&(b / BigRational::from_integer(fact.clone())), w);
        let term = c.mul(&rising, w).mul(&pw, w);
        sum = sum.add(&term, w);
        if term.is_zero() || term.mag() < limit {
            return Ok(sum.round(p));
        }
        let k = 2 * j as i64;
        rising = rising
            .mul(&s.add(&BigF::from_i64(k - 1), w), w)
            .mul(&s.add(&BigF::from_i64(k), w), w);
        fact *= BigInt::from((k + 1) * (k + 2));
        pw = pw.mul(&inv2, w);
    }
    Err(HpError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(x: &BigF, want: f64) -> bool {
        (x.to_f64() - want).abs() <= 1e-14 * want.abs().max(1.0)
    }

    #[test]
    fn gamma_and_zeta() {
        let half = BigF::from_parts(BigInt::one(), -1);
        assert!(close(
            &gamma(&half, 200).unwrap(),
            std::f64::consts::PI.sqrt()
        ));
        assert!(close(&gamma(&BigF::from_i64(5), 200).unwrap(), 24.0));
        let x = BigF::from_ratio(&BigRational::new(7.into(), 3.into()), 200);
        assert!(close(&gamma(&x, 200).unwrap(), 1.190_639_348_759));
        let mx = BigF::from_ratio(&BigRational::new((-3).into(), 2.into()), 200);
        assert!(close(&gamma(&mx, 200).unwrap(), 2.363_271_801_207_355));
        let two = BigF::from_i64(2);
        let z2 = hurwitz(&two, &BigF::one(), 200).unwrap();
        assert!(close(&z2, std::f64::consts::PI.powi(2) / 6.0));
        let s = BigF::from_ratio(&BigRational::new(11.into(), 10.into()), 200);
        assert!(close(
            &hurwitz(&s, &BigF::one(), 200).unwrap(),
            10.584_448_464_950_81
        ));
        assert_eq!(
            bernoulli(12),
            Some(BigRational::new((-691).into(), 2730.into()))
        );
    }
}
