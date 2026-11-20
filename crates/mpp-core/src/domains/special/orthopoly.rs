//! Classical orthogonal polynomials by their three-term recurrences (DLMF 18.9.1):
//! Legendre `(n+1) P_{n+1} = (2n+1) x P_n − n P_{n−1}`, Chebyshev
//! `T_{n+1} = 2x T_n − T_{n−1}` (and `U` with `U₁ = 2x`). Exact polynomials with
//! rational coefficients, or values at a number.

use num_bigint::BigInt;
use num_rational::BigRational;

use crate::expr::Expr;
use crate::poly::Poly;

/// Largest degree built exactly.
pub const MAX_DEGREE: usize = 500;

/// Which family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// Legendre `P_n`.
    Legendre,
    /// Chebyshev of the first kind `T_n`.
    ChebyshevT,
    /// Chebyshev of the second kind `U_n`.
    ChebyshevU,
}

fn rat(n: i64) -> BigRational {
    BigRational::from_integer(BigInt::from(n))
}

/// The polynomial of degree `n` (callers cap `n` at [`MAX_DEGREE`]).
pub fn poly(family: Family, n: usize) -> Poly {
    let x = Poly::x();
    let mut prev = Poly::constant(rat(1));
    if n == 0 {
        return prev;
    }
    let mut cur = match family {
        Family::ChebyshevU => x.scale(&rat(2)),
        _ => x.clone(),
    };
    for k in 1..n {
        let next = match family {
            Family::Legendre => {
                let kk = k as i64;
                x.mul(&cur)
                    .scale(&rat(2 * kk + 1))
                    .sub(&prev.scale(&rat(kk)))
                    .scale(&BigRational::new(BigInt::from(1), BigInt::from(kk + 1)))
            }
            Family::ChebyshevT | Family::ChebyshevU => x.mul(&cur).scale(&rat(2)).sub(&prev),
        };
        prev = std::mem::replace(&mut cur, next);
    }
    cur
}

/// The value at a real number by the recurrence.
pub fn value(family: Family, n: usize, x: f64) -> f64 {
    let mut prev = 1.0;
    if n == 0 {
        return prev;
    }
    let mut cur = match family {
        Family::ChebyshevU => 2.0 * x,
        _ => x,
    };
    for k in 1..n {
        let kf = k as f64;
        let next = match family {
            Family::Legendre => ((2.0 * kf + 1.0) * x * cur - kf * prev) / (kf + 1.0),
            _ => 2.0 * x * cur - prev,
        };
        prev = cur;
        cur = next;
    }
    cur
}

/// The polynomial evaluated at an expression (Horner form, then simplified).
pub fn at_expr(p: &Poly, x: &Expr) -> Expr {
    let mut acc = Expr::zero();
    for c in p.coeffs().iter().rev() {
        acc = &(&acc * x) + &Expr::ratio(c.clone());
    }
    crate::domains::util::normalize(&acc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn low_degrees() {
        // DLMF Table 18.3.1: P₂ = (3x² − 1)/2, T₃ = 4x³ − 3x, U₂ = 4x² − 1.
        assert_eq!(
            poly(Family::Legendre, 2).to_expr("x").to_string(),
            "3*x^2/2 - 1/2"
        );
        assert_eq!(
            poly(Family::ChebyshevT, 3).to_expr("x").to_string(),
            "4*x^3 - 3*x"
        );
        assert_eq!(
            poly(Family::ChebyshevU, 2).to_expr("x").to_string(),
            "4*x^2 - 1"
        );
        assert_eq!(poly(Family::Legendre, 0).to_expr("x"), Expr::one());
        // P₅(1/2) = 23/256 (P₅ = (63x⁵ − 70x³ + 15x)/8, DLMF Table 18.3.1).
        let p5 = poly(Family::Legendre, 5);
        assert_eq!(
            p5.eval(&BigRational::new(1.into(), 2.into())),
            BigRational::new(23.into(), 256.into())
        );
    }

    proptest! {
        #[test]
        fn chebyshev_cosine_identity(n in 0usize..40, t in 0.0f64..3.0) {
            // T_n(cos θ) = cos(nθ).
            prop_assert!((value(Family::ChebyshevT, n, t.cos()) - (n as f64 * t).cos()).abs() < 1e-10);
            // U_n(cos θ) sin θ = sin((n+1)θ).
            prop_assert!((value(Family::ChebyshevU, n, t.cos()) * t.sin() - ((n + 1) as f64 * t).sin()).abs() < 1e-9);
        }

        #[test]
        fn legendre_bounded_and_normalised(n in 0usize..60, x in -1.0f64..1.0) {
            prop_assert!(value(Family::Legendre, n, x).abs() <= 1.0 + 1e-12);
            prop_assert!((value(Family::Legendre, n, 1.0) - 1.0).abs() < 1e-12);
        }

        #[test]
        fn exact_and_numeric_agree(n in 0usize..20, num in -8i64..8) {
            let x = BigRational::new(num.into(), 8.into());
            let exact = crate::number::ratio_to_f64(&poly(Family::Legendre, n).eval(&x));
            prop_assert!((exact - value(Family::Legendre, n, num as f64 / 8.0)).abs() < 1e-12);
        }
    }
}
