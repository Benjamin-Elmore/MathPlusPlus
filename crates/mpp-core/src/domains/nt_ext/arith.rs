//! Arithmetic functions: divisor count τ, divisor sums σ_k, the Möbius function μ,
//! Dirichlet convolution and Möbius inversion of sequences, the prime-counting
//! function π(x) (exact by a sieve, plus the x/ln x estimate), p-adic valuations and
//! absolute values, and Legendre's formula for v_p(n!).

use num_bigint::BigInt;
use num_integer::Integer;
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};

use crate::domains::number_theory::{factorint, is_prime};
use crate::error::{MathError, MathResult};
use crate::expr::Expr;

/// Largest x for the exact prime count (an odd-only bit sieve of 6 MB).
pub const MAX_PRIMEPI: u64 = 100_000_000;
/// Longest sequence for Dirichlet convolution and Möbius inversion.
pub const MAX_SEQ: usize = 100_000;

fn positive(name: &str, n: &BigInt) -> MathResult<()> {
    if n.is_positive() {
        Ok(())
    } else {
        Err(MathError::Domain {
            function: name.into(),
            detail: format!("{name}(n) needs a positive integer, not {n}"),
        })
    }
}

/// τ(n): the number of positive divisors, `∏ (a + 1)`.
pub fn divisor_count(n: &BigInt) -> MathResult<BigInt> {
    positive("τ", n)?;
    Ok(factorint(n)?
        .iter()
        .fold(BigInt::one(), |acc, (_, a)| acc * BigInt::from(a + 1)))
}

/// σ_k(n): the sum of the k-th powers of the positive divisors,
/// `∏ (p^{k(a+1)} − 1)/(p^k − 1)` (and τ for k = 0).
pub fn divisor_sigma(n: &BigInt, k: u32) -> MathResult<BigInt> {
    positive("σ", n)?;
    if k == 0 {
        return divisor_count(n);
    }
    let mut acc = BigInt::one();
    for (p, a) in factorint(n)? {
        let pk = p.pow(k);
        acc *= (pk.pow(a + 1) - 1u32) / (pk - 1u32);
    }
    Ok(acc)
}

/// μ(n): 0 when a square divides n, otherwise (−1)^(number of prime factors).
pub fn mobius(n: &BigInt) -> MathResult<i32> {
    positive("μ", n)?;
    let fs = factorint(n)?;
    if fs.iter().any(|(_, a)| *a > 1) {
        return Ok(0);
    }
    Ok(if fs.len() % 2 == 0 { 1 } else { -1 })
}

/// μ(1), …, μ(n) by a linear sieve.
pub fn mobius_table(n: usize) -> Vec<i8> {
    let mut mu = vec![1i8; n + 1];
    let mut composite = vec![false; n + 1];
    let mut primes: Vec<usize> = Vec::new();
    if n >= 1 {
        mu[0] = 0;
    }
    for i in 2..=n {
        if !composite[i] {
            primes.push(i);
            mu[i] = -1;
        }
        for &p in &primes {
            let ip = i * p;
            if ip > n {
                break;
            }
            composite[ip] = true;
            if i % p == 0 {
                mu[ip] = 0;
                break;
            }
            mu[ip] = -mu[i];
        }
    }
    mu
}

fn check_len(name: &str, n: usize) -> MathResult<()> {
    if n > MAX_SEQ {
        Err(MathError::TooLarge(format!("{name} of {n} terms")))
    } else {
        Ok(())
    }
}

/// `(f * g)(n) = Σ_{d | n} f(d) g(n/d)` for `n = 1 … min(len f, len g)`, where
/// `f[0]` is `f(1)`.
pub fn dirichlet_convolution(f: &[Expr], g: &[Expr]) -> MathResult<Vec<Expr>> {
    let n = f.len().min(g.len());
    check_len("a Dirichlet convolution", n)?;
    let mut terms: Vec<Vec<Expr>> = vec![Vec::new(); n];
    for d in 1..=n {
        for m in (d..=n).step_by(d) {
            terms[m - 1].push(&f[d - 1] * &g[m / d - 1]);
        }
    }
    Ok(terms
        .into_iter()
        .map(|t| crate::simplify::add(t).simplify())
        .collect())
}

/// Möbius inversion: from `F(n) = Σ_{d | n} f(d)` recover `f(n) = Σ_{d | n} μ(d) F(n/d)`.
pub fn mobius_inversion(big_f: &[Expr]) -> MathResult<Vec<Expr>> {
    let n = big_f.len();
    check_len("a Möbius inversion", n)?;
    let mu = mobius_table(n);
    let mu_seq: Vec<Expr> = (1..=n).map(|i| Expr::int(i64::from(mu[i]))).collect();
    dirichlet_convolution(&mu_seq, big_f)
}

/// π(x) for `x ≤` [`MAX_PRIMEPI`], by an odd-only sieve of Eratosthenes.
pub fn prime_pi(x: u64) -> MathResult<u64> {
    if x > MAX_PRIMEPI {
        return Err(MathError::TooLarge(format!(
            "an exact prime count up to {x} (the limit is {MAX_PRIMEPI}; use the estimate)"
        )));
    }
    if x < 2 {
        return Ok(0);
    }
    // Bit i stands for the odd number 2i + 1; set means composite.
    let half = (x as usize).div_ceil(2);
    let mut bits = vec![0u64; half / 64 + 1];
    let mut i = 1usize;
    while (2 * i + 1) * (2 * i + 1) <= x as usize {
        if bits[i / 64] >> (i % 64) & 1 == 0 {
            let p = 2 * i + 1;
            let mut j = (p * p) / 2;
            while j < half {
                bits[j / 64] |= 1 << (j % 64);
                j += p;
            }
        }
        i += 1;
    }
    // Count unset bits among 1 … half−1 (index 0 is the number 1), plus the prime 2.
    let mut count = 1u64;
    for idx in 1..half {
        if bits[idx / 64] >> (idx % 64) & 1 == 0 {
            count += 1;
        }
    }
    Ok(count)
}

/// The estimate π(x) ≈ x / ln x (prime number theorem).
pub fn prime_pi_estimate(x: f64) -> MathResult<f64> {
    if x.is_nan() || x <= 1.0 || !x.is_finite() {
        return Err(MathError::Domain {
            function: "π".into(),
            detail: format!("the estimate x/ln x needs x > 1, not {x}"),
        });
    }
    Ok(x / x.ln())
}

fn prime_arg(p: &BigInt) -> MathResult<()> {
    if p.is_positive() && is_prime(p)? {
        Ok(())
    } else {
        Err(MathError::Domain {
            function: "v_p".into(),
            detail: format!("{p} is not a prime"),
        })
    }
}

fn int_valuation(p: &BigInt, n: &BigInt) -> i64 {
    let mut n = n.abs();
    let mut v = 0;
    while (&n % p).is_zero() {
        n /= p;
        v += 1;
    }
    v
}

/// v_p(x) for a non-zero rational x; `None` for x = 0 (v_p(0) = ∞).
pub fn valuation(p: &BigInt, x: &BigRational) -> MathResult<Option<i64>> {
    prime_arg(p)?;
    if x.is_zero() {
        return Ok(None);
    }
    Ok(Some(
        int_valuation(p, x.numer()) - int_valuation(p, x.denom()),
    ))
}

/// |x|_p = p^(−v_p(x)), and |0|_p = 0.
pub fn padic_abs(p: &BigInt, x: &BigRational) -> MathResult<BigRational> {
    match valuation(p, x)? {
        None => Ok(BigRational::zero()),
        Some(v) => {
            let pv = BigRational::from_integer(p.pow(v.unsigned_abs() as u32));
            Ok(if v >= 0 { pv.recip() } else { pv })
        }
    }
}

/// Legendre's formula `v_p(n!) = Σ_{k ≥ 1} ⌊n / p^k⌋`.
pub fn factorial_valuation(p: &BigInt, n: &BigInt) -> MathResult<BigInt> {
    prime_arg(p)?;
    if n.is_negative() {
        return Err(MathError::Domain {
            function: "v_p".into(),
            detail: "n! needs n ≥ 0".into(),
        });
    }
    let mut total = BigInt::zero();
    let mut q = n.div_floor(p);
    while q.is_positive() {
        total += &q;
        q = q.div_floor(p);
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn b(n: i64) -> BigInt {
        BigInt::from(n)
    }

    #[test]
    fn reference_values() {
        // OEIS A000005 (τ), A000203 (σ), A008683 (μ).
        assert_eq!(divisor_count(&b(360)).unwrap(), b(24));
        assert_eq!(divisor_sigma(&b(360), 1).unwrap(), b(1170));
        assert_eq!(divisor_sigma(&b(12), 2).unwrap(), b(210));
        assert_eq!(divisor_sigma(&b(28), 1).unwrap(), b(56)); // perfect number
        assert_eq!(mobius(&b(1)).unwrap(), 1);
        assert_eq!(mobius(&b(30)).unwrap(), -1);
        assert_eq!(mobius(&b(12)).unwrap(), 0);
        assert_eq!(mobius_table(10)[1..], [1, -1, -1, 0, -1, 1, -1, 0, 0, 1]);
        // OEIS A006880: π(10ⁿ) = 4, 25, 168, 1229, 9592, 78498, 664579.
        for (x, pi) in [
            (10, 4),
            (100, 25),
            (1000, 168),
            (10_000, 1229),
            (1_000_000, 78_498),
        ] {
            assert_eq!(prime_pi(x).unwrap(), pi);
        }
        assert_eq!(prime_pi(2).unwrap(), 1);
        assert_eq!(prime_pi(1).unwrap(), 0);
        assert!(prime_pi(MAX_PRIMEPI + 1).is_err());
        assert!((prime_pi_estimate(1e6).unwrap() - 72_382.4).abs() < 0.1);
        // |8|₂ = 1/8 (catalogue), v₃(18/5) = 2, v₅(18/5) = −1.
        let q = |n, d| BigRational::new(b(n), b(d));
        assert_eq!(padic_abs(&b(2), &q(8, 1)).unwrap(), q(1, 8));
        assert_eq!(valuation(&b(3), &q(18, 5)).unwrap(), Some(2));
        assert_eq!(valuation(&b(5), &q(18, 5)).unwrap(), Some(-1));
        assert_eq!(valuation(&b(5), &q(0, 1)).unwrap(), None);
        assert!(valuation(&b(4), &q(8, 1)).is_err());
        // v₂(10!) = 8, v₅(100!) = 24.
        assert_eq!(factorial_valuation(&b(2), &b(10)).unwrap(), b(8));
        assert_eq!(factorial_valuation(&b(5), &b(100)).unwrap(), b(24));
    }

    #[test]
    fn convolution_identities() {
        // 1 * 1 = τ, and inverting τ gives back the constant 1.
        let ones: Vec<Expr> = vec![Expr::one(); 12];
        let tau = dirichlet_convolution(&ones, &ones).unwrap();
        let expected: Vec<Expr> = (1..=12)
            .map(|n| Expr::bigint(divisor_count(&b(n)).unwrap()))
            .collect();
        assert_eq!(tau, expected);
        assert_eq!(mobius_inversion(&tau).unwrap(), ones);
    }

    proptest! {
        #[test]
        fn multiplicative(m in 1i64..2000, n in 1i64..2000) {
            prop_assume!(b(m).gcd(&b(n)).is_one());
            let mn = b(m * n);
            prop_assert_eq!(divisor_count(&mn).unwrap(), divisor_count(&b(m)).unwrap() * divisor_count(&b(n)).unwrap());
            prop_assert_eq!(divisor_sigma(&mn, 1).unwrap(), divisor_sigma(&b(m), 1).unwrap() * divisor_sigma(&b(n), 1).unwrap());
            prop_assert_eq!(mobius(&mn).unwrap(), mobius(&b(m)).unwrap() * mobius(&b(n)).unwrap());
        }

        #[test]
        fn sigma_matches_divisor_sum(n in 1i64..3000) {
            let s: i64 = (1..=n).filter(|d| n % d == 0).sum();
            prop_assert_eq!(divisor_sigma(&b(n), 1).unwrap(), b(s));
            prop_assert_eq!(mobius_table(n as usize)[n as usize] as i32, mobius(&b(n)).unwrap());
        }

        #[test]
        fn padic_ultrametric(x in -500i64..500, y in -500i64..500) {
            let p = b(3);
            let (qx, qy) = (BigRational::from_integer(b(x)), BigRational::from_integer(b(y)));
            let lhs = padic_abs(&p, &(&qx + &qy)).unwrap();
            let rhs = padic_abs(&p, &qx).unwrap().max(padic_abs(&p, &qy).unwrap());
            prop_assert!(lhs <= rhs);
        }
    }
}
