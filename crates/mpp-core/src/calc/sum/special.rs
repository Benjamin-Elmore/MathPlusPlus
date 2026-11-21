//! Exact special values the summation engine needs: Bernoulli numbers, ζ at even integers,
//! the digamma function at positive rationals (Gauss's digamma theorem) and logarithms of
//! rationals written over primes.

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};

use crate::expr::Expr;
use crate::functions::{func, Func};
use crate::simplify::{add, mul, pow};

/// Largest Bernoulli index computed (the recurrence is quadratic in the index).
pub(crate) const MAX_BERNOULLI: usize = 300;
/// Largest denominator for which Gauss's digamma theorem is used.
const MAX_DIGAMMA_DENOMINATOR: i64 = 24;
/// Largest integer whose logarithm is split over its prime factors.
const MAX_LN_FACTOR: u64 = 1_000_000;

fn ri(n: i64) -> BigRational {
    BigRational::from_integer(BigInt::from(n))
}

/// The Bernoulli numbers `B₀ … B_n` (with `B₁ = −1/2`); `None` past [`MAX_BERNOULLI`].
pub(crate) fn bernoulli_numbers(n: usize) -> Option<Vec<BigRational>> {
    if n > MAX_BERNOULLI {
        return None;
    }
    let mut b: Vec<BigRational> = Vec::with_capacity(n + 1);
    for m in 0..=n {
        if m == 0 {
            b.push(BigRational::one());
            continue;
        }
        if m > 1 && m % 2 == 1 {
            b.push(BigRational::zero());
            continue;
        }
        // Σ_{i=0}^{m} C(m+1, i) B_i = 0.
        let mut acc = BigRational::zero();
        let mut binom = BigInt::one();
        for (i, bi) in b.iter().enumerate() {
            if !bi.is_zero() {
                acc += bi * BigRational::from_integer(binom.clone());
            }
            binom = binom * BigInt::from(m + 1 - i) / BigInt::from(i + 1);
        }
        b.push(-acc / BigRational::from_integer(BigInt::from(m + 1)));
    }
    Some(b)
}

/// `ζ(m)` for an even `m ≥ 2`: `|B_m| (2π)^m / (2 · m!)`.
pub(crate) fn zeta_even(m: u32) -> Option<Expr> {
    if m < 2 || m % 2 == 1 {
        return None;
    }
    let idx = usize::try_from(m).ok()?;
    let b = bernoulli_numbers(idx)?.pop()?;
    let mut fact = BigInt::one();
    for i in 2..=m {
        fact *= BigInt::from(i);
    }
    let two_pow = BigInt::one() << (idx - 1);
    let coeff = b.abs() * BigRational::from_integer(two_pow) / BigRational::from_integer(fact);
    Some(mul(vec![
        Expr::ratio(coeff),
        pow(Expr::pi(), Expr::int(i64::from(m))),
    ]))
}

/// Prime factorisation of a small positive integer.
fn prime_factors(mut m: u64) -> Vec<(u64, i64)> {
    let mut out = Vec::new();
    let mut p = 2u64;
    while p * p <= m {
        let mut e = 0;
        while m.is_multiple_of(p) {
            m /= p;
            e += 1;
        }
        if e > 0 {
            out.push((p, e));
        }
        p += 1;
    }
    if m > 1 {
        out.push((m, 1));
    }
    out
}

fn ln_of_integer(m: &BigInt) -> Expr {
    match m.to_u64().filter(|v| *v <= MAX_LN_FACTOR && *v >= 1) {
        Some(1) => Expr::zero(),
        Some(v) => add(prime_factors(v)
            .into_iter()
            .map(|(p, e)| {
                mul(vec![
                    Expr::int(e),
                    func(Func::Ln, vec![Expr::int(p as i64)]),
                ])
            })
            .collect()),
        None => func(Func::Ln, vec![Expr::bigint(m.clone())]),
    }
}

/// `ln e`, with logarithms of positive rationals and their rational powers written as sums
/// of logarithms of primes (`ln(√3/2) = ½ ln 3 − ln 2`).
pub(crate) fn ln_expand(e: &Expr) -> Expr {
    use crate::expr::ExprNode;
    match e.node() {
        ExprNode::Num(n) => match n.as_rational() {
            Some(r) if r.is_positive() => {
                add(vec![ln_of_integer(r.numer()), -ln_of_integer(r.denom())])
            }
            _ => func(Func::Ln, vec![e.clone()]),
        },
        ExprNode::Pow(b, x) => {
            let positive_base = b
                .as_number()
                .and_then(|n| n.as_rational())
                .is_some_and(Signed::is_positive);
            if positive_base && x.is_number() {
                mul(vec![x.clone(), ln_expand(b)])
            } else {
                func(Func::Ln, vec![e.clone()])
            }
        }
        ExprNode::Mul(fs) => {
            let parts: Vec<Expr> = fs.iter().map(ln_expand).collect();
            if parts.iter().any(contains_ln_of_non_atom) {
                func(Func::Ln, vec![e.clone()])
            } else {
                add(parts)
            }
        }
        _ => func(Func::Ln, vec![e.clone()]),
    }
}

/// True when a logarithm in `e` was left unsplit (so the product must stay whole).
fn contains_ln_of_non_atom(e: &Expr) -> bool {
    use crate::expr::ExprNode;
    match e.node() {
        ExprNode::Func(Func::Ln, args) => !args.iter().all(Expr::is_number),
        _ => e.children().iter().any(|c| contains_ln_of_non_atom(c)),
    }
}

/// `ψ(x) + γ` for a positive rational `x`, exactly (Euler's γ is left out so that callers
/// can cancel it: in a convergent sum the γ terms add up to zero).
pub(crate) fn digamma_plus_gamma(x: &BigRational) -> Option<Expr> {
    if !x.is_positive() {
        return None;
    }
    let n = x.floor().to_integer();
    let f = x - BigRational::from_integer(n.clone());
    let steps = n.to_i64()?;
    if steps > 10_000 {
        return None;
    }
    // ψ(f + n) = ψ(f) + Σ_{j=0}^{n−1} 1/(f + j); for f = 0 start from ψ(1) = −γ.
    let (base, start, mut harmonic) = if f.is_zero() {
        (Expr::zero(), BigRational::one(), BigRational::zero())
    } else {
        (gauss_digamma(&f)?, f.clone(), BigRational::zero())
    };
    let count = if f.is_zero() { steps - 1 } else { steps };
    for j in 0..count {
        harmonic += (&start + ri(j)).recip();
    }
    Some(add(vec![base, Expr::ratio(harmonic)]))
}

/// Gauss's digamma theorem for `0 < p/q < 1` (without the `−γ`):
/// `ψ(p/q) + γ = −ln(2q) − (π/2) cot(πp/q) + 2 Σ_{n=1}^{⌊(q−1)/2⌋} cos(2πnp/q) ln sin(πn/q)`.
fn gauss_digamma(r: &BigRational) -> Option<Expr> {
    let p = r.numer().to_i64()?;
    let q = r.denom().to_i64()?;
    if q > MAX_DIGAMMA_DENOMINATOR || p <= 0 || p >= q {
        return None;
    }
    let angle = |num: i64, den: i64| mul(vec![Expr::rational(num, den), Expr::pi()]);
    let mut terms = vec![
        -ln_of_integer(&BigInt::from(2 * q)),
        mul(vec![
            Expr::rational(-1, 2),
            Expr::pi(),
            func(Func::Cot, vec![angle(p, q)]),
        ]),
    ];
    for n in 1..=((q - 1) / 2) {
        let c = func(Func::Cos, vec![angle(2 * n * p, q)]);
        if c.is_zero() {
            continue;
        }
        let s = func(Func::Sin, vec![angle(n, q)]);
        terms.push(mul(vec![Expr::int(2), c, ln_expand(&s)]));
    }
    Some(add(terms))
}

/// `Σ_{i=1}^{n} 1/i^m` exactly.
pub(crate) fn generalized_harmonic(n: &BigInt, m: u32) -> Option<BigRational> {
    let count = n.to_i64()?;
    if count > 100_000 {
        return None;
    }
    let mut acc = BigRational::zero();
    for i in 1..=count {
        acc += BigRational::new(BigInt::one(), BigInt::from(i).pow(m));
    }
    Some(acc)
}

/// Hurwitz `ζ(m, x) = Σ_{k≥0} 1/(k + x)^m` for even `m` and positive `x` that is an integer
/// or a half-integer.
pub(crate) fn hurwitz_zeta(m: u32, x: &BigRational) -> Option<Expr> {
    if !x.is_positive() {
        return None;
    }
    let z = zeta_even(m)?;
    let two = BigInt::from(2);
    if x.is_integer() {
        // ζ(m, n) = ζ(m) − Σ_{i=1}^{n−1} i^{−m}
        let h = generalized_harmonic(&(x.to_integer() - 1), m)?;
        return Some(add(vec![z, Expr::ratio(-h)]));
    }
    if x.denom() != &two {
        return None;
    }
    // ζ(m, 1/2) = (2^m − 1) ζ(m); ζ(m, n + 1/2) = ζ(m, 1/2) − Σ_{j<n} (j + 1/2)^{−m}.
    let n = (x - BigRational::new(BigInt::one(), two)).to_integer();
    let count = n.to_i64()?;
    if count > 100_000 {
        return None;
    }
    let mut acc = BigRational::zero();
    for j in 0..count {
        let base = ri(j) + BigRational::new(BigInt::one(), BigInt::from(2));
        acc += base.pow(i32::try_from(m).ok()?).recip();
    }
    let factor = (BigInt::one() << usize::try_from(m).ok()?) - 1;
    Some(add(vec![
        mul(vec![Expr::bigint(factor), z]),
        Expr::ratio(-acc),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bernoulli_values() {
        let b = bernoulli_numbers(12).unwrap_or_default();
        assert_eq!(b[1], BigRational::new((-1).into(), 2.into()));
        assert_eq!(b[2], BigRational::new(1.into(), 6.into()));
        assert_eq!(b[4], BigRational::new((-1).into(), 30.into()));
        assert_eq!(b[12], BigRational::new((-691).into(), 2730.into()));
        assert!(b[5].is_zero());
        assert!(bernoulli_numbers(MAX_BERNOULLI + 1).is_none());
    }

    #[test]
    fn zeta_values() {
        let z2 = zeta_even(2).unwrap_or_else(Expr::undefined);
        assert_eq!(z2, Expr::pi().pow(2) / Expr::int(6));
        let z4 = zeta_even(4).unwrap_or_else(Expr::undefined);
        assert_eq!(z4, Expr::pi().pow(4) / Expr::int(90));
        let z6 = zeta_even(6).unwrap_or_else(Expr::undefined);
        assert_eq!(z6, Expr::pi().pow(6) / Expr::int(945));
        assert!(zeta_even(3).is_none());
    }

    #[test]
    fn digamma_values() {
        let half = BigRational::new(1.into(), 2.into());
        let v = digamma_plus_gamma(&half).unwrap_or_else(Expr::undefined);
        assert_eq!(v, Expr::int(-2) * func(Func::Ln, vec![Expr::int(2)]));
        let three = BigRational::from_integer(3.into());
        assert_eq!(
            digamma_plus_gamma(&three).unwrap_or_else(Expr::undefined),
            Expr::rational(3, 2)
        );
        let third = BigRational::new(1.into(), 3.into());
        let v = digamma_plus_gamma(&third)
            .and_then(|e| e.eval_f64(&crate::eval::EmptyEnv).ok())
            .unwrap_or(f64::NAN);
        let gamma = std::f64::consts::EULER_GAMMA;
        assert!((v - gamma - (-3.132_033_780_020_806)).abs() < 1e-12, "{v}");
    }
}
