//! Factorisation in the Gaussian integers ℤ[i], a unique factorisation domain
//! (h_{ℚ(i)} = 1). A rational prime p splits as π π̄ when p ≡ 1 (mod 4), stays prime
//! when p ≡ 3 (mod 4), and 2 = −i (1 + i)² ramifies.

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{One, Signed, Zero};

use crate::domains::number_theory::{factorint, powmod};
use crate::error::{MathError, MathResult};

/// A Gaussian integer `re + im·i`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gaussian {
    /// Real part.
    pub re: BigInt,
    /// Imaginary part.
    pub im: BigInt,
}

impl Gaussian {
    /// `re + im·i`.
    pub fn new(re: impl Into<BigInt>, im: impl Into<BigInt>) -> Gaussian {
        Gaussian {
            re: re.into(),
            im: im.into(),
        }
    }

    /// The norm `re² + im²`.
    pub fn norm(&self) -> BigInt {
        &self.re * &self.re + &self.im * &self.im
    }

    /// The product.
    pub fn mul(&self, o: &Gaussian) -> Gaussian {
        Gaussian {
            re: &self.re * &o.re - &self.im * &o.im,
            im: &self.re * &o.im + &self.im * &o.re,
        }
    }

    /// The complex conjugate.
    pub fn conj(&self) -> Gaussian {
        Gaussian {
            re: self.re.clone(),
            im: -&self.im,
        }
    }

    fn is_zero(&self) -> bool {
        self.re.is_zero() && self.im.is_zero()
    }

    /// The exact quotient, when `o` divides `self`.
    pub fn div_exact(&self, o: &Gaussian) -> Option<Gaussian> {
        let n = o.norm();
        if n.is_zero() {
            return None;
        }
        let t = self.mul(&o.conj());
        if t.re.is_multiple_of(&n) && t.im.is_multiple_of(&n) {
            Some(Gaussian {
                re: t.re / &n,
                im: t.im / n,
            })
        } else {
            None
        }
    }

    /// The remainder of division with the quotient rounded to the nearest Gaussian
    /// integer (its norm is at most half the divisor's).
    fn rem(&self, o: &Gaussian) -> Gaussian {
        let n = o.norm();
        let t = self.mul(&o.conj());
        let two_n: BigInt = &n * 2;
        let round = |x: &BigInt| -> BigInt {
            let num: BigInt = x * 2 + &n;
            num.div_floor(&two_n)
        };
        let q = Gaussian {
            re: round(&t.re),
            im: round(&t.im),
        };
        let qo = q.mul(o);
        Gaussian {
            re: &self.re - qo.re,
            im: &self.im - qo.im,
        }
    }

    /// The associate in the first quadrant (re > 0, im ≥ 0).
    pub fn normalised(&self) -> Gaussian {
        let mut z = self.clone();
        for _ in 0..4 {
            if z.re.is_positive() && !z.im.is_negative() {
                return z;
            }
            z = z.mul(&Gaussian::new(0, 1));
        }
        z
    }
}

fn gcd(a: &Gaussian, b: &Gaussian) -> Gaussian {
    let (mut a, mut b) = (a.clone(), b.clone());
    while !b.is_zero() {
        let r = a.rem(&b);
        a = std::mem::replace(&mut b, r);
    }
    a
}

/// The Gaussian prime above a rational prime p ≡ 1 (mod 4): `gcd(p, x + i)` with
/// `x² ≡ −1 (mod p)`.
fn split_prime(p: &BigInt) -> MathResult<Gaussian> {
    let e = (p - 1u32) / 4u32;
    let mut c = BigInt::from(2);
    while &c < p {
        let x = powmod(&c, &e, p)?;
        if (&x * &x + 1u32).is_multiple_of(p) {
            return Ok(gcd(&Gaussian::new(p.clone(), 0), &Gaussian::new(x, 1)).normalised());
        }
        c += 1u32;
    }
    Err(MathError::Undefined(format!(
        "a square root of −1 modulo {p}"
    )))
}

/// `z = unit · ∏ πᵢ^{eᵢ}` with each πᵢ in the first quadrant, ordered by norm.
pub fn factor(z: &Gaussian) -> MathResult<(Gaussian, Vec<(Gaussian, u32)>)> {
    if z.is_zero() {
        return Err(MathError::Undefined("the factorisation of 0".into()));
    }
    let mut rest = z.clone();
    let mut out: Vec<(Gaussian, u32)> = Vec::new();
    let mut divide_out = |rest: &mut Gaussian, pi: Gaussian| {
        let mut e = 0;
        while let Some(q) = rest.div_exact(&pi) {
            *rest = q;
            e += 1;
        }
        if e > 0 {
            out.push((pi, e));
        }
    };
    for (p, _) in factorint(&z.norm())? {
        let r = p.mod_floor(&BigInt::from(4));
        if p == BigInt::from(2) {
            divide_out(&mut rest, Gaussian::new(1, 1));
        } else if r == BigInt::from(3) {
            divide_out(&mut rest, Gaussian::new(p.clone(), 0));
        } else {
            let pi = split_prime(&p)?;
            let pib = pi.conj().normalised();
            divide_out(&mut rest, pi);
            divide_out(&mut rest, pib);
        }
    }
    if !rest.norm().is_one() {
        return Err(MathError::Undefined(format!(
            "a complete factorisation (left over {} + {}i)",
            rest.re, rest.im
        )));
    }
    Ok((rest, out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn g(a: i64, b: i64) -> Gaussian {
        Gaussian::new(a, b)
    }

    #[test]
    fn reference_factorisations() {
        // 5 = (2 + i)(2 − i) = −i (1 + 2i)(2 + i); 2 = −i (1 + i)²; 3 is prime.
        let (u, fs) = factor(&g(2, 0)).unwrap();
        assert_eq!((u, fs), (g(0, -1), vec![(g(1, 1), 2)]));
        let (_, fs) = factor(&g(3, 0)).unwrap();
        assert_eq!(fs, vec![(g(3, 0), 1)]);
        let (_, fs) = factor(&g(5, 0)).unwrap();
        assert_eq!(fs.len(), 2);
        assert!(fs
            .iter()
            .all(|(p, e)| p.norm() == BigInt::from(5) && *e == 1));
        // 3 + 4i = (2 + i)² · (−i)… check by multiplying back.
        let z = g(3, 4);
        let (u, fs) = factor(&z).unwrap();
        let prod = fs
            .iter()
            .fold(u, |acc, (p, e)| (0..*e).fold(acc, |a, _| a.mul(p)));
        assert_eq!(prod, z);
        assert!(factor(&g(0, 0)).is_err());
    }

    proptest! {
        #[test]
        fn factorisation_multiplies_back(a in -300i64..300, b in -300i64..300) {
            prop_assume!(a != 0 || b != 0);
            let z = g(a, b);
            let (u, fs) = factor(&z).unwrap();
            prop_assert!(u.norm().is_one());
            let prod = fs.iter().fold(u, |acc, (p, e)| (0..*e).fold(acc, |x, _| x.mul(p)));
            prop_assert_eq!(prod, z);
            for (p, _) in fs {
                // A Gaussian prime has prime norm or is a rational prime ≡ 3 (mod 4).
                let n = p.norm();
                let prime_norm = crate::domains::number_theory::is_prime(&n).unwrap();
                prop_assert!(prime_norm || (p.im.is_zero() && p.re.mod_floor(&BigInt::from(4)) == BigInt::from(3)));
            }
        }
    }
}
