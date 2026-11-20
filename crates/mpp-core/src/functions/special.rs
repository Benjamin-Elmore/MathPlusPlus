//! Numerical values of the error function and the sine and cosine integrals, accurate to
//! about 15 significant digits: power series near zero, continued fractions further out.

use std::f64::consts::{FRAC_PI_2, PI};

use num_complex::Complex64;

/// Relative accuracy aimed for.
const EPS: f64 = 1e-16;
/// Most terms of a series or continued fraction.
const MAX_TERMS: usize = 500;
/// Where the sine and cosine integrals switch from the series to the continued fraction.
const CISI_SWITCH: f64 = 2.0;
/// Where the error function switches from the series to the continued fraction.
const ERF_SWITCH: f64 = 2.5;

/// `erf x`.
pub(crate) fn erf(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x < 0.0 {
        return -erf(-x);
    }
    if x < ERF_SWITCH {
        // erf x = (2/√π) e^(−x²) Σ 2ⁿ x^(2n+1) / (1·3·…·(2n+1)): every term positive.
        let x2 = x * x;
        let mut term = x;
        let mut sum = x;
        for n in 1..MAX_TERMS {
            term *= 2.0 * x2 / (2 * n + 1) as f64;
            sum += term;
            if term <= EPS * sum {
                break;
            }
        }
        2.0 / PI.sqrt() * (-x2).exp() * sum
    } else {
        1.0 - erfc_large(x)
    }
}

/// `erfc x = 1 − erf x` for `x ≥ 2.5`, by the continued fraction
/// `e^(−x²)/√π · 1/(x + (1/2)/(x + 1/(x + (3/2)/(x + …))))`, evaluated from the tail.
fn erfc_large(x: f64) -> f64 {
    let mut f = x;
    for k in (1..=120).rev() {
        f = x + (k as f64 / 2.0) / f;
    }
    (-x * x).exp() / (PI.sqrt() * f)
}

/// `Si x`.
pub(crate) fn si(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x < 0.0 {
        return -si(-x);
    }
    if x == 0.0 {
        return 0.0;
    }
    if x.is_infinite() {
        return FRAC_PI_2;
    }
    cisi(x).1
}

/// `Ci x` for `x > 0`.
pub(crate) fn ci(x: f64) -> f64 {
    if x.is_infinite() {
        return 0.0;
    }
    cisi(x).0
}

/// `(Ci x, Si x)` for `x > 0`.
fn cisi(x: f64) -> (f64, f64) {
    if x < CISI_SWITCH {
        // Si x = Σ (−1)ᵏ x^(2k+1)/((2k+1)(2k+1)!), Ci x = γ + ln x + Σ (−1)ᵏ x^(2k)/(2k (2k)!).
        let mut si = 0.0;
        let mut cin = 0.0;
        let mut fact = 1.0;
        for k in 1..MAX_TERMS {
            fact *= x / k as f64;
            let term = fact / k as f64;
            // Signs run +, −, −, +, +, −, −, … (k odd: sine terms, k even: cosine terms).
            let sign = if k % 4 == 0 || k % 4 == 1 { 1.0 } else { -1.0 };
            if k % 2 == 1 {
                si += sign * term;
            } else {
                cin += sign * term;
            }
            if k > 2 && term < EPS * (si.abs() + cin.abs()) {
                break;
            }
        }
        (std::f64::consts::EULER_GAMMA + x.ln() + cin, si)
    } else {
        // Continued fraction for E₁(i x), evaluated by the modified Lentz method.
        let tiny = 1e-300;
        let mut b = Complex64::new(1.0, x);
        let mut c = Complex64::new(1.0 / tiny, 0.0);
        let mut d = Complex64::new(1.0, 0.0) / b;
        let mut h = d;
        for i in 2..MAX_TERMS {
            let a = -(((i - 1) * (i - 1)) as f64);
            b += Complex64::new(2.0, 0.0);
            d = Complex64::new(1.0, 0.0) / (d * a + b);
            c = b + Complex64::new(a, 0.0) / c;
            let del = c * d;
            h *= del;
            if (del.re - 1.0).abs() + del.im.abs() < EPS {
                break;
            }
        }
        h *= Complex64::new(x.cos(), -x.sin());
        (-h.re, FRAC_PI_2 + h.im)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * (1.0 + b.abs())
    }

    #[test]
    fn error_function_values() {
        assert_eq!(erf(0.0), 0.0);
        assert!(close(erf(0.5), 0.520_499_877_813_046_5, 1e-15));
        assert!(close(erf(1.0), 0.842_700_792_949_714_9, 1e-15));
        assert!(close(erf(2.0), 0.995_322_265_018_952_7, 1e-15));
        assert!(close(erf(3.0), 0.999_977_909_503_001_4, 1e-15));
        assert!(close(erf(-1.0), -0.842_700_792_949_714_9, 1e-15));
        assert!(close(1.0 - erf(5.0), 1.537_459_794_428_034_8e-12, 1e-3));
        assert_eq!(erf(40.0), 1.0);
    }

    #[test]
    fn sine_and_cosine_integrals() {
        assert!(close(si(1.0), 0.946_083_070_367_183, 1e-14));
        assert!(close(si(2.0), 1.605_412_976_802_695, 1e-14));
        assert!(close(si(5.0), 1.549_931_244_944_674, 1e-14));
        assert!(close(si(-3.0), -1.848_652_527_999_468, 1e-14));
        assert!(close(si(100.0), 1.562_225_466_889_056, 1e-13));
        assert!(close(ci(1.0), 0.337_403_922_900_968_1, 1e-14));
        assert!(close(ci(0.1), -1.727_868_386_657_297, 1e-14));
        assert!(close(ci(5.0), -0.190_029_749_656_644_3, 1e-13));
        assert!(close(ci(30.0), -0.033_032_417_282_071_14, 1e-12));
    }
}
