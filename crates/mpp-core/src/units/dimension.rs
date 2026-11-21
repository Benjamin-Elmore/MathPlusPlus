//! Physical dimensions: rational exponents of the seven SI base quantities.

use std::fmt;

use num_rational::Rational64;
use num_traits::{CheckedAdd, CheckedMul, Signed, Zero};

use crate::error::{MathError, MathResult};

/// Number of SI base quantities.
pub const BASE_COUNT: usize = 7;
/// Largest numerator or denominator an exponent may have.
const MAX_EXPONENT: i64 = 1_000_000;

/// The SI base symbols, in the order of the exponent vector:
/// length, mass, time, current, temperature, amount, luminous intensity.
pub const BASE_SYMBOLS: [&str; BASE_COUNT] = ["m", "kg", "s", "A", "K", "mol", "cd"];

/// A dimension such as L·T⁻² (acceleration).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Dimension(pub [Rational64; BASE_COUNT]);

fn too_large() -> MathError {
    MathError::TooLarge("this unit exponent".into())
}

fn checked(r: Option<Rational64>) -> MathResult<Rational64> {
    let r = r.ok_or_else(too_large)?;
    if r.numer().abs() > MAX_EXPONENT || *r.denom() > MAX_EXPONENT {
        return Err(too_large());
    }
    Ok(r)
}

impl Dimension {
    /// Dimensionless.
    pub fn none() -> Dimension {
        Dimension::default()
    }

    /// The `i`-th base dimension to the power one.
    pub fn base(i: usize) -> Dimension {
        let mut d = Dimension::none();
        if let Some(e) = d.0.get_mut(i) {
            *e = Rational64::from_integer(1);
        }
        d
    }

    /// From integer exponents `[L, M, T, I, Θ, N, J]`.
    pub fn from_ints(e: [i64; BASE_COUNT]) -> Dimension {
        Dimension(e.map(Rational64::from_integer))
    }

    /// True when every exponent is zero.
    pub fn is_none(&self) -> bool {
        self.0.iter().all(Zero::is_zero)
    }

    /// `self · other` (exponents add).
    pub fn mul(&self, other: &Dimension) -> MathResult<Dimension> {
        let mut out = [Rational64::zero(); BASE_COUNT];
        for (i, o) in out.iter_mut().enumerate() {
            *o = checked(self.0[i].checked_add(&other.0[i]))?;
        }
        Ok(Dimension(out))
    }

    /// `self / other`.
    pub fn div(&self, other: &Dimension) -> MathResult<Dimension> {
        self.mul(&other.pow(Rational64::from_integer(-1))?)
    }

    /// `self ^ k`.
    pub fn pow(&self, k: Rational64) -> MathResult<Dimension> {
        let mut out = [Rational64::zero(); BASE_COUNT];
        for (i, o) in out.iter_mut().enumerate() {
            *o = checked(self.0[i].checked_mul(&k))?;
        }
        Ok(Dimension(out))
    }
}

impl fmt::Display for Dimension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const NAMES: [&str; BASE_COUNT] = ["L", "M", "T", "I", "Θ", "N", "J"];
        if self.is_none() {
            return write!(f, "1");
        }
        let mut first = true;
        for (name, e) in NAMES.iter().zip(self.0.iter()) {
            if e.is_zero() {
                continue;
            }
            if !first {
                write!(f, "·")?;
            }
            first = false;
            if *e == Rational64::from_integer(1) {
                write!(f, "{name}")?;
            } else if e.is_integer() {
                write!(f, "{name}^{}", e.numer())?;
            } else {
                write!(f, "{name}^({e})")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn dim() -> impl Strategy<Value = Dimension> {
        prop::array::uniform7(-6i64..=6).prop_map(Dimension::from_ints)
    }

    proptest! {
        #[test]
        fn multiplication_is_a_group(a in dim(), b in dim(), c in dim()) {
            prop_assert_eq!(a.mul(&b).unwrap(), b.mul(&a).unwrap());
            prop_assert_eq!(a.mul(&b).unwrap().mul(&c).unwrap(), a.mul(&b.mul(&c).unwrap()).unwrap());
            prop_assert_eq!(a.mul(&Dimension::none()).unwrap(), a);
            prop_assert!(a.div(&a).unwrap().is_none());
        }

        #[test]
        fn powers_distribute(a in dim(), b in dim(), n in -4i64..=4, d in 1i64..=4) {
            let k = Rational64::new(n, d);
            prop_assert_eq!(
                a.mul(&b).unwrap().pow(k).unwrap(),
                a.pow(k).unwrap().mul(&b.pow(k).unwrap()).unwrap()
            );
            if n != 0 {
                prop_assert_eq!(a.pow(k).unwrap().pow(k.recip()).unwrap(), a);
            }
        }
    }

    #[test]
    fn display_and_overflow() {
        let accel = Dimension::from_ints([1, 0, -2, 0, 0, 0, 0]);
        assert_eq!(accel.to_string(), "L·T^-2");
        assert_eq!(Dimension::none().to_string(), "1");
        let big = Dimension::from_ints([MAX_EXPONENT, 0, 0, 0, 0, 0, 0]);
        assert!(big.pow(Rational64::from_integer(2)).is_err());
    }
}
