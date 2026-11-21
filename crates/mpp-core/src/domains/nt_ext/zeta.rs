//! The Riemann zeta function, the Hurwitz zeta function and Dirichlet L-functions.
//!
//! Exact values: `ζ(2k) = (−1)^{k+1} B_{2k} (2π)^{2k} / (2 (2k)!)` and
//! `ζ(−n) = (−1)ⁿ B_{n+1}/(n+1)` (DLMF 25.6.2, 25.6.3). Numerically, the Hurwitz zeta
//! `ζ(s, a)` by Euler–Maclaurin summation (DLMF 2.10.1 applied to 25.11.1; N terms plus 15 Bernoulli
//! corrections), the functional equation for `Re s < 0`, and
//! `L(s, χ) = q^{−s} Σ_{a=1}^{q} χ(a) ζ(s, a/q)` for a character of period q, with
//! `L(1, χ) = −(1/q) Σ χ(a) ψ(a/q)` when `Σ χ(a) = 0`.

use std::f64::consts::PI;

use num_bigint::BigInt;
use num_complex::Complex64;
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};

use crate::error::{MathError, MathResult};
use crate::expr::Expr;
use crate::functions::{digamma_complex, gamma_complex};
use crate::number::ratio_to_f64;

/// Largest index for exact Bernoulli numbers.
pub const MAX_BERNOULLI: usize = 300;
/// Largest |s| for numeric evaluation.
pub const MAX_S: f64 = 100.0;
/// Euler–Maclaurin correction terms.
const EM_TERMS: usize = 15;

/// B₀ … B_n (with B₁ = −1/2), by the Akiyama–Tanigawa algorithm.
pub fn bernoulli(n: usize) -> MathResult<Vec<BigRational>> {
    if n > MAX_BERNOULLI {
        return Err(MathError::TooLarge(format!("B_{n}")));
    }
    let mut a: Vec<BigRational> = Vec::with_capacity(n + 1);
    let mut out = Vec::with_capacity(n + 1);
    for m in 0..=n {
        a.push(BigRational::new(BigInt::one(), BigInt::from(m + 1)));
        for j in (1..=m).rev() {
            let diff = &a[j - 1] - &a[j];
            a[j - 1] = diff * BigRational::from_integer(BigInt::from(j));
        }
        out.push(a[0].clone());
    }
    if n >= 1 {
        out[1] = -out[1].clone();
    }
    Ok(out)
}

fn factorial(n: usize) -> BigInt {
    (1..=n).fold(BigInt::one(), |acc, k| acc * BigInt::from(k))
}

/// The exact value of ζ(s) at an integer s, when it has a closed form (s even ≥ 2 or
/// s ≤ 0). `Err` at the pole s = 1, `Ok(None)` for odd s ≥ 3.
pub fn exact(s: i64) -> MathResult<Option<Expr>> {
    if s == 1 {
        return Err(MathError::Undefined(
            "ζ(1) (the harmonic series diverges)".into(),
        ));
    }
    if s >= 2 && s % 2 == 0 {
        let n = s as usize;
        let b = bernoulli(n)?;
        let mut c = b[n].abs() * BigRational::from_integer(BigInt::from(2).pow(n as u32))
            / BigRational::from_integer(2 * factorial(n));
        if c.is_negative() {
            c = -c;
        }
        return Ok(Some(
            (Expr::ratio(c) * Expr::pi().pow(Expr::int(s))).simplify(),
        ));
    }
    if s <= 0 {
        let n = s.unsigned_abs() as usize;
        let b = bernoulli(n + 1)?;
        let mut v = &b[n + 1] / BigRational::from_integer(BigInt::from(n + 1));
        if n % 2 == 1 {
            v = -v;
        }
        return Ok(Some(Expr::ratio(v)));
    }
    Ok(None)
}

fn check_s(s: Complex64) -> MathResult<()> {
    if !s.re.is_finite() || !s.im.is_finite() || s.norm() > MAX_S {
        return Err(MathError::TooLarge(format!("ζ at {s} (|s| ≤ {MAX_S})")));
    }
    Ok(())
}

/// The Hurwitz zeta function ζ(s, a) for a > 0, s ≠ 1.
pub fn hurwitz(s: Complex64, a: f64) -> MathResult<Complex64> {
    check_s(s)?;
    if a.is_nan() || a <= 0.0 || !a.is_finite() {
        return Err(MathError::Domain {
            function: "ζ".into(),
            detail: format!("the Hurwitz zeta function needs a > 0, not {a}"),
        });
    }
    let one = Complex64::new(1.0, 0.0);
    if (s - one).norm() < 1e-15 {
        return Err(MathError::Undefined("ζ(1, a) (a pole)".into()));
    }
    let b = bernoulli(2 * EM_TERMS)?;
    let n = (20.0 + s.norm()).ceil() as usize;
    let pw = |x: f64, e: Complex64| (e * x.ln()).exp();
    let mut sum = Complex64::new(0.0, 0.0);
    for k in 0..n {
        sum += pw(k as f64 + a, -s);
    }
    let na = n as f64 + a;
    sum += pw(na, one - s) / (s - one);
    sum += pw(na, -s) * 0.5;
    // Σ B_{2j}/(2j)! · s(s+1)…(s+2j−2) · (N+a)^{−s−2j+1}
    let mut rising = s; // s(s+1)…(s+2j−2) for j = 1
    let mut fact = 2.0f64; // (2j)!
    for j in 1..=EM_TERMS {
        let term = rising * ratio_to_f64(&b[2 * j]) / fact * pw(na, -s - (2 * j - 1) as f64);
        sum += term;
        if term.norm() < 1e-17 * sum.norm() {
            break;
        }
        rising *= (s + (2 * j - 1) as f64) * (s + (2 * j) as f64);
        fact *= ((2 * j + 1) * (2 * j + 2)) as f64;
    }
    Ok(sum)
}

/// ζ(s) numerically (functional equation for Re s < 0).
pub fn zeta(s: Complex64) -> MathResult<Complex64> {
    check_s(s)?;
    if s.re < 0.0 {
        let one = Complex64::new(1.0, 0.0);
        let two = Complex64::new(2.0, 0.0);
        let pi = Complex64::new(PI, 0.0);
        let fe = two.powc(s) * pi.powc(s - one) * (s * PI / 2.0).sin() * gamma_complex(one - s);
        return Ok(fe * hurwitz(one - s, 1.0)?);
    }
    hurwitz(s, 1.0)
}

/// `L(s, χ)` for a function χ of period `q = chi.len()` given by `χ(1), …, χ(q)`.
pub fn dirichlet_l(s: Complex64, chi: &[Complex64]) -> MathResult<Complex64> {
    let q = chi.len();
    if q == 0 || q > 10_000 {
        return Err(MathError::Domain {
            function: "L".into(),
            detail: "give the character's values χ(1), …, χ(q) over one period (q ≤ 10000)".into(),
        });
    }
    let qf = q as f64;
    let total: Complex64 = chi.iter().sum();
    let one = Complex64::new(1.0, 0.0);
    if (s - one).norm() < 1e-15 {
        if total.norm() > 1e-12 {
            return Err(MathError::Undefined(
                "L(1, χ) for a character with non-zero sum (a pole)".into(),
            ));
        }
        let mut acc = Complex64::new(0.0, 0.0);
        for (i, c) in chi.iter().enumerate() {
            if !c.is_zero() {
                acc += c * digamma_complex(Complex64::new((i + 1) as f64 / qf, 0.0));
            }
        }
        return Ok(-acc / qf);
    }
    let mut acc = Complex64::new(0.0, 0.0);
    for (i, c) in chi.iter().enumerate() {
        if !c.is_zero() {
            acc += c * hurwitz(s, (i + 1) as f64 / qf)?;
        }
    }
    Ok((-s * qf.ln()).exp() * acc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::fieldkit::close;
    use proptest::prelude::*;

    fn re(x: f64) -> Complex64 {
        Complex64::new(x, 0.0)
    }

    #[test]
    fn bernoulli_numbers() {
        // OEIS A027641 / A027642: B₂ = 1/6, B₄ = −1/30, B₁₂ = −691/2730.
        let b = bernoulli(12).unwrap();
        let q = |n: i64, d: i64| BigRational::new(n.into(), d.into());
        assert_eq!(b[1], q(-1, 2));
        assert_eq!(b[2], q(1, 6));
        assert_eq!(b[4], q(-1, 30));
        assert_eq!(b[12], q(-691, 2730));
        assert!(b[11].is_zero());
    }

    #[test]
    fn exact_values() {
        // Catalogue: ζ(0) = −1/2, ζ(2) = π²/6, ζ(4) = π⁴/90; ζ(−1) = −1/12.
        assert_eq!(exact(0).unwrap(), Some(Expr::rational(-1, 2)));
        assert_eq!(exact(-1).unwrap(), Some(Expr::rational(-1, 12)));
        assert_eq!(exact(-2).unwrap(), Some(Expr::zero()));
        assert_eq!(
            exact(2).unwrap(),
            Some((Expr::pi().pow(2) / Expr::int(6)).simplify())
        );
        assert_eq!(
            exact(4).unwrap(),
            Some((Expr::pi().pow(4) / Expr::int(90)).simplify())
        );
        assert_eq!(exact(3).unwrap(), None);
        assert!(exact(1).is_err());
    }

    #[test]
    fn numeric_values() {
        // ζ(3) (Apéry, OEIS A002117), ζ(1/2) (A059750), ζ(2) = π²/6, ζ(−1/2) (OEIS A211113).
        assert!(close(
            zeta(re(3.0)).unwrap().re,
            1.202_056_903_159_594_2,
            1e-14
        ));
        assert!(close(
            zeta(re(0.5)).unwrap().re,
            -1.460_354_508_809_586_8,
            1e-13
        ));
        assert!(close(zeta(re(2.0)).unwrap().re, PI * PI / 6.0, 1e-14));
        assert!(close(
            zeta(re(-0.5)).unwrap().re,
            -0.207_886_224_977_354_6,
            1e-12
        ));
        assert!(close(zeta(re(-3.0)).unwrap().re, 1.0 / 120.0, 1e-12));
        // First non-trivial zero, ρ = 1/2 + 14.134725141734693 i (OEIS A058303).
        assert!(
            zeta(Complex64::new(0.5, 14.134_725_141_734_693))
                .unwrap()
                .norm()
                < 1e-10
        );
        assert!(zeta(re(1.0)).is_err());
        // L(1, χ₋₄) = π/4 (Leibniz) and L(2, χ₋₄) = Catalan's G (OEIS A006752).
        let chi4 = [re(1.0), re(0.0), re(-1.0), re(0.0)];
        assert!(close(
            dirichlet_l(re(1.0), &chi4).unwrap().re,
            PI / 4.0,
            1e-14
        ));
        assert!(close(
            dirichlet_l(re(2.0), &chi4).unwrap().re,
            0.915_965_594_177_219,
            1e-14
        ));
        // The principal character mod 1 gives ζ back.
        assert!(close(
            dirichlet_l(re(3.0), &[re(1.0)]).unwrap().re,
            1.202_056_903_159_594_2,
            1e-14
        ));
        assert!(dirichlet_l(re(1.0), &[re(1.0)]).is_err());
    }

    proptest! {
        #[test]
        fn euler_product_agrees(s in 2.0f64..8.0) {
            // ζ(s) = ∏_p (1 − p^{−s})^{−1}; the product over p < 2000 is within 2000^{1−s}.
            let mut prod = 1.0f64;
            for p in (2u64..2000).filter(|&n| (2..n).take_while(|d| d * d <= n).all(|d| n % d != 0)) {
                prod *= 1.0 / (1.0 - (p as f64).powf(-s));
            }
            let z = zeta(re(s)).unwrap().re;
            prop_assert!((z - prod).abs() <= 2000f64.powf(1.0 - s) + 1e-12);
        }

        #[test]
        fn hurwitz_shift(s in 1.5f64..6.0, a in 0.1f64..5.0) {
            // ζ(s, a) = a^{−s} + ζ(s, a + 1).
            let lhs = hurwitz(re(s), a).unwrap().re;
            let rhs = a.powf(-s) + hurwitz(re(s), a + 1.0).unwrap().re;
            prop_assert!(close(lhs, rhs, 1e-12));
        }
    }
}
