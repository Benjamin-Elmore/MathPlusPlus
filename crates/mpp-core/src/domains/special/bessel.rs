//! Bessel functions of the first kind `J_n(x)` for integer order, by Miller's
//! backward recurrence `J_{k−1} = (2k/x) J_k − J_{k+1}`, normalised with
//! `J₀ + 2 Σ J_{2k} = 1` (DLMF 10.12.4). The backward direction is stable for every
//! order, so one routine covers small and large `x`.

use crate::error::{MathError, MathResult};

/// Largest `|n|` and `|x|` accepted.
pub const MAX_ORDER: i64 = 10_000;
/// Largest argument accepted.
pub const MAX_ARG: f64 = 1e4;

/// `J_n(x)`.
pub fn bessel_j(n: i64, x: f64) -> MathResult<f64> {
    if !x.is_finite() || x.abs() > MAX_ARG || n.abs() > MAX_ORDER {
        return Err(MathError::TooLarge(format!("J_{n}({x})")));
    }
    // J_{−n} = (−1)ⁿ J_n and J_n(−x) = (−1)ⁿ J_n(x) (DLMF 10.4.1, 10.4.2).
    let sign_n = if n < 0 && n % 2 != 0 { -1.0 } else { 1.0 };
    let n = n.unsigned_abs();
    let sign_x = if x < 0.0 && n % 2 == 1 { -1.0 } else { 1.0 };
    let x = x.abs();
    if x == 0.0 {
        return Ok(if n == 0 { 1.0 } else { 0.0 });
    }
    let big = (n as f64).max(x);
    let start = (big + 30.0 + (50.0 * big).sqrt()) as u64;
    let start = start + start % 2;
    let (mut jp1, mut j) = (0.0f64, 1e-30f64);
    let mut sum = 0.0;
    let mut saved = if start == n { j } else { 0.0 };
    for k in (1..=start).rev() {
        let jm1 = 2.0 * k as f64 / x * j - jp1;
        jp1 = j;
        j = jm1;
        if (k - 1) % 2 == 0 && k > 1 {
            sum += 2.0 * j;
        }
        if k - 1 == n {
            saved = j;
        }
        if j.abs() > 1e250 {
            j *= 1e-250;
            jp1 *= 1e-250;
            sum *= 1e-250;
            saved *= 1e-250;
        }
    }
    sum += j; // J₀ term
    Ok(sign_n * sign_x * saved / sum)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::fieldkit::close;
    use proptest::prelude::*;

    #[test]
    fn reference_values() {
        // Abramowitz & Stegun Table 9.1 and DLMF §10.75 (first zero of J₀).
        assert!(close(
            bessel_j(0, 1.0).unwrap(),
            0.765_197_686_557_966_6,
            1e-14
        ));
        assert!(close(
            bessel_j(1, 1.0).unwrap(),
            0.440_050_585_744_933_5,
            1e-14
        ));
        assert!(close(
            bessel_j(2, 3.0).unwrap(),
            0.486_091_260_585_891_1,
            1e-14
        ));
        assert!(close(
            bessel_j(5, 10.0).unwrap(),
            -0.234_061_528_186_793_6,
            1e-13
        ));
        assert!(close(
            bessel_j(0, 100.0).unwrap(),
            0.019_985_850_304_223_12,
            1e-12
        ));
        assert!(bessel_j(0, 2.404_825_557_695_773).unwrap().abs() < 1e-14);
        assert_eq!(bessel_j(0, 0.0).unwrap(), 1.0);
        assert_eq!(bessel_j(3, 0.0).unwrap(), 0.0);
        assert!(close(
            bessel_j(-1, 1.0).unwrap(),
            -0.440_050_585_744_933_5,
            1e-14
        ));
    }

    proptest! {
        #[test]
        fn three_term_recurrence(n in 1i64..40, x in 0.1f64..60.0) {
            // J_{n−1} + J_{n+1} = (2n/x) J_n (DLMF 10.6.1).
            let (a, b, c) = (
                bessel_j(n - 1, x).unwrap(),
                bessel_j(n, x).unwrap(),
                bessel_j(n + 1, x).unwrap(),
            );
            prop_assert!((a + c - 2.0 * n as f64 / x * b).abs() < 1e-11 * (1.0 + 2.0 * n as f64 / x));
        }

        #[test]
        fn bounded(n in 0i64..50, x in -200.0f64..200.0) {
            prop_assert!(bessel_j(n, x).unwrap().abs() <= 1.0 + 1e-12);
        }
    }
}
