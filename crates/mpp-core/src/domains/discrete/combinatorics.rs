//! Counting: Catalan numbers, the hook-length formula for standard Young tableaux,
//! and necklaces/bracelets (Burnside's lemma for the cyclic and dihedral groups in
//! closed form).

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::One;

use crate::domains::number_theory::totient;
use crate::error::{MathError, MathResult};
use crate::number::{binomial, factorial};

/// Largest index for Catalan numbers.
pub const MAX_CATALAN: usize = 20_000;
/// Most cells in a Young diagram.
pub const MAX_CELLS: usize = 5_000;
/// Most beads in a necklace.
pub const MAX_BEADS: usize = 100_000;

/// `C_n = C(2n, n)/(n + 1)`.
pub fn catalan(n: usize) -> MathResult<BigInt> {
    if n > MAX_CATALAN {
        return Err(MathError::TooLarge(format!("C_{n}")));
    }
    let c = binomial(
        &BigRational::from_integer(BigInt::from(2 * n)),
        &BigInt::from(n),
    )?;
    Ok(c.to_integer() / BigInt::from(n + 1))
}

/// The number of standard Young tableaux of shape λ, `n! / ∏ h(c)`.
pub fn hook_length(shape: &[usize]) -> MathResult<BigInt> {
    if shape.windows(2).any(|w| w[0] < w[1]) || shape.contains(&0) {
        return Err(MathError::Domain {
            function: "hooklength".into(),
            detail: "a partition lists positive parts in non-increasing order".into(),
        });
    }
    let n: usize = shape.iter().sum();
    if n > MAX_CELLS {
        return Err(MathError::TooLarge(format!("a diagram of {n} cells")));
    }
    let mut hooks = BigInt::one();
    for (i, &row) in shape.iter().enumerate() {
        for j in 0..row {
            let arm = row - j - 1;
            let leg = shape[i + 1..].iter().take_while(|&&r| r > j).count();
            hooks *= BigInt::from(arm + leg + 1);
        }
    }
    Ok(factorial(&BigInt::from(n))? / hooks)
}

fn divisors(n: usize) -> Vec<usize> {
    (1..=n).filter(|&d| n.is_multiple_of(d)).collect()
}

/// Necklaces of n beads in k colours up to rotation, `(1/n) Σ_{d|n} φ(d) k^{n/d}`.
pub fn necklaces(n: usize, k: &BigInt) -> MathResult<BigInt> {
    if n == 0 || n > MAX_BEADS {
        return Err(MathError::Domain {
            function: "necklaces".into(),
            detail: format!("the number of beads must be 1 … {MAX_BEADS}"),
        });
    }
    let mut total = BigInt::from(0);
    for d in divisors(n) {
        total += totient(&BigInt::from(d))? * num_traits::pow(k.clone(), n / d);
    }
    Ok(total / BigInt::from(n))
}

/// Bracelets (necklaces up to rotation and reflection).
pub fn bracelets(n: usize, k: &BigInt) -> MathResult<BigInt> {
    let neck = BigRational::from_integer(necklaces(n, k)?);
    let half = BigRational::new(BigInt::one(), BigInt::from(2));
    let refl = if n % 2 == 1 {
        BigRational::from_integer(num_traits::pow(k.clone(), n.div_ceil(2))) * &half
    } else {
        BigRational::from_integer((k + 1u32) * num_traits::pow(k.clone(), n / 2))
            / BigRational::from_integer(BigInt::from(4))
    };
    let b = neck * half + refl;
    if !b.is_integer() {
        return Err(MathError::Undefined("a whole number of bracelets".into()));
    }
    Ok(b.to_integer())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::discrete::perm;
    use proptest::prelude::*;

    #[test]
    fn reference_values() {
        // OEIS A000108: 1, 1, 2, 5, 14, 42, 132; C₃₀ = 3814986502092304.
        let c: Vec<BigInt> = (0..7).map(|n| catalan(n).unwrap()).collect();
        assert_eq!(c, [1, 1, 2, 5, 14, 42, 132].map(BigInt::from));
        assert_eq!(catalan(30).unwrap(), BigInt::from(3_814_986_502_092_304u64));
        // f^(3,2) = 5, f^(2,2) = 2 = C₂, f^(4,3,1) = 70 (Stanley EC2 §7.21).
        assert_eq!(hook_length(&[3, 2]).unwrap(), BigInt::from(5));
        assert_eq!(hook_length(&[2, 2]).unwrap(), BigInt::from(2));
        assert_eq!(hook_length(&[4, 3, 1]).unwrap(), BigInt::from(70));
        assert!(hook_length(&[1, 2]).is_err());
        // OEIS A000031 (necklaces, k = 2): 2, 3, 4, 6, 8, 14; A000029 (bracelets): 2, 3, 4, 6, 8, 13.
        let two = BigInt::from(2);
        let neck: Vec<BigInt> = (1..=6).map(|n| necklaces(n, &two).unwrap()).collect();
        assert_eq!(neck, [2, 3, 4, 6, 8, 14].map(BigInt::from));
        let brac: Vec<BigInt> = (1..=6).map(|n| bracelets(n, &two).unwrap()).collect();
        assert_eq!(brac, [2, 3, 4, 6, 8, 13].map(BigInt::from));
    }

    proptest! {
        #[test]
        fn catalan_recurrence(n in 0usize..40) {
            // C_{n+1} = Σ C_i C_{n−i}.
            let s: BigInt = (0..=n).map(|i| catalan(i).unwrap() * catalan(n - i).unwrap()).sum();
            prop_assert_eq!(catalan(n + 1).unwrap(), s);
            // Staircase-free check: f^(n,n) = C_n.
            if n > 0 { prop_assert_eq!(hook_length(&[n, n]).unwrap(), catalan(n).unwrap()); }
        }

        #[test]
        fn closed_forms_match_burnside(n in 1usize..9, k in 1u32..5) {
            let k = BigInt::from(k);
            let c = perm::closure(&[perm::rotation(n)]).unwrap();
            prop_assert_eq!(necklaces(n, &k).unwrap(), perm::burnside(&c, &k).unwrap());
            if n >= 3 {
                let d = perm::closure(&[perm::rotation(n), perm::reflection(n)]).unwrap();
                prop_assert_eq!(bracelets(n, &k).unwrap(), perm::burnside(&d, &k).unwrap());
            }
        }

        #[test]
        fn hook_sum_of_squares(n in 1usize..9) {
            // Σ_λ (f^λ)² = n! over all partitions of n.
            fn parts(n: usize, max: usize, cur: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
                if n == 0 { out.push(cur.clone()); return; }
                for p in (1..=n.min(max)).rev() {
                    cur.push(p);
                    parts(n - p, p, cur, out);
                    cur.pop();
                }
            }
            let mut all = Vec::new();
            parts(n, n, &mut Vec::new(), &mut all);
            let s: BigInt = all.iter().map(|l| { let f = hook_length(l).unwrap(); &f * &f }).sum();
            prop_assert_eq!(s, factorial(&BigInt::from(n)).unwrap());
        }
    }
}
