//! Finite fields 𝔽_{p^m} = 𝔽_p[x]/(f) for the first monic irreducible f of degree m
//! (coefficients compared from the constant term up), found with Rabin's test.
//! Elements are coefficient vectors `c₀ + c₁x + … + c_{m−1}x^{m−1}`. Arithmetic,
//! inverses, powers, the Frobenius map x ↦ x^p and the trace to 𝔽_p.

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{Signed, ToPrimitive, Zero};

use crate::domains::number_theory::{factorint, is_prime};
use crate::error::{MathError, MathResult};

/// Largest characteristic (so products fit in `u128` comfortably and below `u64`).
pub const MAX_P: u64 = 1 << 31;
/// Largest extension degree.
pub const MAX_M: usize = 32;
/// Most candidate polynomials tried when looking for an irreducible one.
const MAX_SEARCH: u64 = 1_000_000;

type Poly = Vec<u64>;

fn trim(mut a: Poly) -> Poly {
    while a.last() == Some(&0) {
        a.pop();
    }
    a
}

/// A finite field of order `p^m`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FiniteField {
    p: u64,
    m: usize,
    /// Monic irreducible modulus, coefficients from the constant term (length m + 1).
    modulus: Poly,
}

impl FiniteField {
    /// 𝔽_{p^m}; errors unless p is a prime below [`MAX_P`] and 1 ≤ m ≤ [`MAX_M`].
    pub fn new(p: u64, m: usize) -> MathResult<FiniteField> {
        if p >= MAX_P || m > MAX_M {
            return Err(MathError::TooLarge(format!("the field of order {p}^{m}")));
        }
        if m == 0 || !is_prime(&BigInt::from(p))? {
            return Err(MathError::Domain {
                function: "𝔽".into(),
                detail: format!(
                    "a finite field of order p^m needs a prime p and m ≥ 1 (got p = {p}, m = {m})"
                ),
            });
        }
        let mut f = FiniteField {
            p,
            m,
            modulus: Vec::new(),
        };
        f.modulus = f.find_irreducible()?;
        Ok(f)
    }

    /// The characteristic p.
    pub fn p(&self) -> u64 {
        self.p
    }

    /// The degree m over 𝔽_p.
    pub fn m(&self) -> usize {
        self.m
    }

    /// The modulus polynomial's coefficients (constant term first, monic).
    pub fn modulus(&self) -> &[u64] {
        &self.modulus
    }

    fn mulc(&self, a: u64, b: u64) -> u64 {
        ((a as u128 * b as u128) % self.p as u128) as u64
    }

    fn inv_c(&self, a: u64) -> u64 {
        // a^(p−2) mod p
        let mut base = a % self.p;
        let mut e = self.p - 2;
        let mut acc = 1u64;
        while e > 0 {
            if e & 1 == 1 {
                acc = self.mulc(acc, base);
            }
            base = self.mulc(base, base);
            e >>= 1;
        }
        acc
    }

    fn add_p(&self, a: &[u64], b: &[u64]) -> Poly {
        let n = a.len().max(b.len());
        trim(
            (0..n)
                .map(|i| (a.get(i).copied().unwrap_or(0) + b.get(i).copied().unwrap_or(0)) % self.p)
                .collect(),
        )
    }

    fn neg_p(&self, a: &[u64]) -> Poly {
        trim(a.iter().map(|&c| (self.p - c) % self.p).collect())
    }

    fn mul_p(&self, a: &[u64], b: &[u64]) -> Poly {
        if a.is_empty() || b.is_empty() {
            return Vec::new();
        }
        let mut out = vec![0u64; a.len() + b.len() - 1];
        for (i, &x) in a.iter().enumerate() {
            if x == 0 {
                continue;
            }
            for (j, &y) in b.iter().enumerate() {
                out[i + j] = (out[i + j] + self.mulc(x, y)) % self.p;
            }
        }
        trim(out)
    }

    /// Remainder and quotient of `a` by a non-zero `b`.
    fn divrem(&self, a: &[u64], b: &[u64]) -> (Poly, Poly) {
        let mut r = trim(a.to_vec());
        let db = b.len() - 1;
        let lead_inv = self.inv_c(b[db]);
        if r.len() < b.len() {
            return (Vec::new(), r);
        }
        let mut q = vec![0u64; r.len() - db];
        while r.len() >= b.len() {
            let shift = r.len() - b.len();
            let c = self.mulc(r[r.len() - 1], lead_inv);
            q[shift] = c;
            for (i, &bc) in b.iter().enumerate() {
                let sub = self.mulc(c, bc);
                r[shift + i] = (r[shift + i] + self.p - sub) % self.p;
            }
            r = trim(r);
        }
        (trim(q), r)
    }

    fn rem_mod(&self, a: &[u64], f: &[u64]) -> Poly {
        self.divrem(a, f).1
    }

    fn mulmod(&self, a: &[u64], b: &[u64], f: &[u64]) -> Poly {
        self.rem_mod(&self.mul_p(a, b), f)
    }

    fn powmod_u64(&self, a: &[u64], mut e: u64, f: &[u64]) -> Poly {
        let mut base = self.rem_mod(a, f);
        let mut acc = vec![1u64];
        while e > 0 {
            if e & 1 == 1 {
                acc = self.mulmod(&acc, &base, f);
            }
            base = self.mulmod(&base, &base, f);
            e >>= 1;
        }
        acc
    }

    fn gcd_p(&self, a: &[u64], b: &[u64]) -> Poly {
        let (mut a, mut b) = (trim(a.to_vec()), trim(b.to_vec()));
        while !b.is_empty() {
            let r = self.rem_mod(&a, &b);
            a = std::mem::replace(&mut b, r);
        }
        a
    }

    /// Rabin's irreducibility test for a monic f of degree m.
    fn is_irreducible(&self, f: &[u64]) -> MathResult<bool> {
        let m = f.len() - 1;
        let x = vec![0u64, 1];
        // x^(p^k) mod f for k = 0 … m.
        let mut powers = vec![self.rem_mod(&x, f)];
        for k in 1..=m {
            let prev = &powers[k - 1];
            powers.push(self.powmod_u64(prev, self.p, f));
        }
        if trim(self.add_p(&powers[m], &self.neg_p(&x))).is_empty() {
            for (r, _) in factorint(&BigInt::from(m))? {
                let r = r.to_usize().unwrap_or(m);
                let h = self.add_p(&powers[m / r], &self.neg_p(&x));
                if self.gcd_p(&h, f).len() != 1 {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        Ok(false)
    }

    fn find_irreducible(&self) -> MathResult<Poly> {
        if self.m == 1 {
            return Ok(vec![0, 1]);
        }
        let mut coeffs = vec![0u64; self.m];
        for _ in 0..MAX_SEARCH {
            // Next candidate: increment the lower coefficients as a base-p counter.
            let mut i = 0;
            loop {
                if i == self.m {
                    return Err(MathError::Undefined("an irreducible polynomial".into()));
                }
                coeffs[i] += 1;
                if coeffs[i] == self.p {
                    coeffs[i] = 0;
                    i += 1;
                } else {
                    break;
                }
            }
            if coeffs[0] == 0 {
                continue; // divisible by x
            }
            let mut f = coeffs.clone();
            f.push(1);
            if self.is_irreducible(&f)? {
                return Ok(f);
            }
        }
        Err(MathError::TooLarge(format!(
            "the search for an irreducible polynomial of degree {} over 𝔽_{}",
            self.m, self.p
        )))
    }

    /// Reads an element from integer coefficients (reduced mod p, at most m of them).
    pub fn element(&self, coeffs: &[BigInt]) -> MathResult<Poly> {
        if coeffs.len() > self.m {
            return Err(MathError::Domain {
                function: "𝔽".into(),
                detail: format!(
                    "an element of 𝔽_{{{}^{}}} has at most {} coefficients",
                    self.p, self.m, self.m
                ),
            });
        }
        let p = BigInt::from(self.p);
        Ok(trim(
            coeffs
                .iter()
                .map(|c| c.mod_floor(&p).to_u64().unwrap_or(0))
                .collect(),
        ))
    }

    /// The coefficients padded to length m.
    pub fn coefficients(&self, a: &[u64]) -> Vec<u64> {
        let mut v = a.to_vec();
        v.resize(self.m, 0);
        v
    }

    /// `a + b`.
    pub fn add(&self, a: &[u64], b: &[u64]) -> Poly {
        self.add_p(a, b)
    }

    /// `a − b`.
    pub fn sub(&self, a: &[u64], b: &[u64]) -> Poly {
        self.add_p(a, &self.neg_p(b))
    }

    /// `a · b`.
    pub fn mul(&self, a: &[u64], b: &[u64]) -> Poly {
        self.mulmod(a, b, &self.modulus)
    }

    /// `a⁻¹` by the extended Euclidean algorithm.
    pub fn inv(&self, a: &[u64]) -> MathResult<Poly> {
        let a = trim(a.to_vec());
        if a.is_empty() {
            return Err(MathError::DivisionByZero);
        }
        let (mut r0, mut r1) = (self.modulus.clone(), a);
        let (mut s0, mut s1): (Poly, Poly) = (Vec::new(), vec![1]);
        while !r1.is_empty() {
            let (q, r) = self.divrem(&r0, &r1);
            r0 = std::mem::replace(&mut r1, r);
            let s2 = self.add_p(&s0, &self.neg_p(&self.mul_p(&q, &s1)));
            s0 = std::mem::replace(&mut s1, s2);
        }
        // r0 is a non-zero constant since the modulus is irreducible.
        let c = self.inv_c(r0[0]);
        Ok(self.rem_mod(&self.mul_p(&s0, &[c]), &self.modulus))
    }

    /// `a^e` (a negative e uses the inverse).
    pub fn pow(&self, a: &[u64], e: &BigInt) -> MathResult<Poly> {
        let base = if e.is_negative() {
            self.inv(a)?
        } else {
            trim(a.to_vec())
        };
        let mut e = e.abs();
        let mut base = base;
        let mut acc = vec![1u64];
        let two = BigInt::from(2);
        while !e.is_zero() {
            if e.is_odd() {
                acc = self.mul(&acc, &base);
            }
            base = self.mul(&base, &base);
            e = e.div_floor(&two);
        }
        Ok(acc)
    }

    /// The Frobenius map `a ↦ a^p`.
    pub fn frobenius(&self, a: &[u64]) -> Poly {
        self.powmod_u64(a, self.p, &self.modulus)
    }

    /// The trace to 𝔽_p, `Σ_{j<m} a^{p^j}` (always a constant).
    pub fn trace(&self, a: &[u64]) -> u64 {
        let mut acc: Poly = Vec::new();
        let mut cur = trim(a.to_vec());
        for _ in 0..self.m {
            acc = self.add_p(&acc, &cur);
            cur = self.frobenius(&cur);
        }
        acc.first().copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn small_fields() {
        // 𝔽₄ = 𝔽₂[x]/(x² + x + 1), 𝔽₈ = 𝔽₂[x]/(x³ + x + 1) (Lidl & Niederreiter, Table C).
        assert_eq!(FiniteField::new(2, 2).unwrap().modulus(), &[1, 1, 1]);
        assert_eq!(FiniteField::new(2, 3).unwrap().modulus(), &[1, 1, 0, 1]);
        assert!(FiniteField::new(4, 1).is_err());
        assert!(FiniteField::new(2, 0).is_err());
        let f = FiniteField::new(2, 2).unwrap();
        let a = vec![0, 1]; // x
        assert_eq!(f.mul(&a, &a), vec![1, 1]); // x² = x + 1
        assert_eq!(f.trace(&a), 1);
        assert_eq!(f.trace(&[1]), 0); // Tr(1) = m · 1 = 0 in 𝔽₂
        let f9 = FiniteField::new(3, 2).unwrap();
        let x = vec![0, 1];
        assert_eq!(f9.pow(&x, &BigInt::from(8)).unwrap(), vec![1]); // a^(q−1) = 1
    }

    proptest! {
        #[test]
        fn field_axioms(p in prop::sample::select(vec![2u64, 3, 5, 7, 13]), m in 1usize..5, seed in any::<u64>()) {
            let f = FiniteField::new(p, m).unwrap();
            let gen = |s: u64| -> Vec<u64> { (0..m).map(|i| (s >> (8 * i)) % p).collect() };
            let a = trim(gen(seed));
            let b = trim(gen(seed.rotate_left(17)));
            let q = BigInt::from(p).pow(m as u32);
            // Frobenius to the m-th power is the identity; a^q = a.
            let mut fr = a.clone();
            for _ in 0..m { fr = f.frobenius(&fr); }
            prop_assert_eq!(&fr, &a);
            prop_assert_eq!(f.pow(&a, &q).unwrap(), a.clone());
            // Frobenius is additive and the trace lies in 𝔽_p and is additive.
            prop_assert_eq!(f.frobenius(&f.add(&a, &b)), f.add(&f.frobenius(&a), &f.frobenius(&b)));
            prop_assert_eq!(f.trace(&f.add(&a, &b)), (f.trace(&a) + f.trace(&b)) % p);
            if !a.is_empty() {
                prop_assert_eq!(f.mul(&a, &f.inv(&a).unwrap()), vec![1]);
            }
        }
    }
}
