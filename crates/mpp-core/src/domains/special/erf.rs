//! The error function and its complement.
//!
//! `erf x` uses the all-positive series `erf x = (2/√π) e^{−x²} Σ 2ⁿ x^{2n+1}/(2n+1)!!`
//! for `|x| < 2` (no cancellation), and `erfc x` the Laplace continued fraction for
//! `x ≥ 2` (modified Lentz). The other side of each comes from `erf + erfc = 1`.

use std::f64::consts::PI;

/// Most terms or continued-fraction steps.
const MAX_ITER: usize = 2_000;

fn erf_series(x: f64) -> f64 {
    let x2 = x * x;
    let mut term = x;
    let mut sum = x;
    for n in 1..MAX_ITER {
        term *= 2.0 * x2 / (2 * n + 1) as f64;
        sum += term;
        if term.abs() <= 1e-17 * sum.abs() {
            break;
        }
    }
    2.0 / PI.sqrt() * (-x2).exp() * sum
}

/// `erfc x` for `x ≥ 2` by the continued fraction
/// `erfc x = e^{−x²}/√π · 1/(x + (1/2)/(x + 1/(x + (3/2)/(x + …))))`.
fn erfc_cf(x: f64) -> f64 {
    const TINY: f64 = 1e-300;
    let mut f = x;
    let mut c = x;
    let mut d = 0.0;
    for n in 1..MAX_ITER {
        let a = n as f64 / 2.0;
        d = x + a * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = x + a / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        let delta = c * d;
        f *= delta;
        if (delta - 1.0).abs() < 1e-16 {
            break;
        }
    }
    (-x * x).exp() / PI.sqrt() / f
}

/// The error function.
pub fn erf(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x.abs() < 2.0 {
        erf_series(x)
    } else if x > 0.0 {
        1.0 - erfc_cf(x)
    } else {
        erfc_cf(-x) - 1.0
    }
}

/// The complementary error function `1 − erf x`, accurate in the far tail.
pub fn erfc(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x >= 2.0 {
        erfc_cf(x)
    } else if x > -2.0 {
        1.0 - erf_series(x)
    } else {
        2.0 - erfc_cf(-x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::fieldkit::close;
    use proptest::prelude::*;

    #[test]
    fn reference_values() {
        // Abramowitz & Stegun Table 7.1 / DLMF §7.2 (values to 16 digits from DLMF 7.2.1).
        assert!(close(erf(0.5), 0.520_499_877_813_046_5, 1e-14));
        assert!(close(erf(1.0), 0.842_700_792_949_714_9, 1e-14));
        assert!(close(erf(2.0), 0.995_322_265_018_952_7, 1e-14));
        assert!(close(erfc(3.0), 2.209_049_699_858_544e-5, 1e-12));
        assert!(close(erfc(5.0), 1.537_459_794_428_035e-12, 1e-11));
        assert_eq!(erf(0.0), 0.0);
    }

    proptest! {
        #[test]
        fn odd_and_complementary(x in -6.0f64..6.0) {
            prop_assert!((erf(-x) + erf(x)).abs() < 1e-15);
            prop_assert!((erf(x) + erfc(x) - 1.0).abs() < 1e-14);
            prop_assert!(erf(x).abs() <= 1.0);
        }

        #[test]
        fn derivative_matches(x in -3.0f64..3.0) {
            // d/dx erf x = (2/√π) e^{−x²}
            let h = 1e-5;
            let num = (erf(x + h) - erf(x - h)) / (2.0 * h);
            prop_assert!((num - 2.0 / PI.sqrt() * (-x * x).exp()).abs() < 1e-8);
        }
    }
}
