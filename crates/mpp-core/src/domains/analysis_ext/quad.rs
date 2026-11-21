//! Adaptive Gauss–Kronrod (7–15 point) quadrature on a finite interval, used by the
//! L^p norms and the Monte Carlo checks. Nodes and weights from QUADPACK (qk15).

use crate::error::{MathError, MathResult};

/// Most subintervals kept.
const MAX_INTERVALS: usize = 2_000;

const XGK: [f64; 8] = [
    0.991_455_371_120_812_6,
    0.949_107_912_342_758_5,
    0.864_864_423_359_769_1,
    0.741_531_185_599_394_4,
    0.586_087_235_467_691_1,
    0.405_845_151_377_397_2,
    0.207_784_955_007_898_5,
    0.0,
];
const WGK: [f64; 8] = [
    0.022_935_322_010_529_22,
    0.063_092_092_629_978_55,
    0.104_790_010_322_250_2,
    0.140_653_259_715_525_9,
    0.169_004_726_639_267_9,
    0.190_350_578_064_785_4,
    0.204_432_940_075_298_9,
    0.209_482_141_084_727_8,
];
const WG: [f64; 4] = [
    0.129_484_966_168_869_7,
    0.279_705_391_489_276_7,
    0.381_830_050_505_118_9,
    0.417_959_183_673_469_4,
];

/// One 15-point Kronrod estimate with its error (difference from the 7-point Gauss).
fn kronrod(f: &mut dyn FnMut(f64) -> MathResult<f64>, a: f64, b: f64) -> MathResult<(f64, f64)> {
    let c = 0.5 * (a + b);
    let h = 0.5 * (b - a);
    let fc = f(c)?;
    let mut k = fc * WGK[7];
    let mut g = fc * WG[3];
    for j in 0..7 {
        let dx = h * XGK[j];
        let (f1, f2) = (f(c - dx)?, f(c + dx)?);
        k += WGK[j] * (f1 + f2);
        if j % 2 == 1 {
            g += WG[j / 2] * (f1 + f2);
        }
    }
    Ok((k * h, ((k - g) * h).abs()))
}

/// `∫_a^b f` to an absolute or relative tolerance `tol`.
pub fn integrate(
    f: &mut dyn FnMut(f64) -> MathResult<f64>,
    a: f64,
    b: f64,
    tol: f64,
) -> MathResult<f64> {
    if !a.is_finite() || !b.is_finite() {
        return Err(MathError::Unsupported(
            "numeric integration over an infinite interval".into(),
        ));
    }
    if a == b {
        return Ok(0.0);
    }
    let (first, err) = kronrod(f, a, b)?;
    let mut parts = vec![(a, b, first, err)];
    loop {
        crate::budget::check()?;
        let total: f64 = parts.iter().map(|p| p.2).sum();
        let error: f64 = parts.iter().map(|p| p.3).sum();
        if !total.is_finite() {
            return Err(MathError::NotANumber("the integrand".into()));
        }
        if error <= tol * total.abs().max(1.0) {
            return Ok(total);
        }
        if parts.len() >= MAX_INTERVALS {
            return Err(MathError::Inconclusive(format!(
                "the integral did not converge (estimate {total}, error {error})"
            )));
        }
        // Split the interval with the largest error.
        let (i, _) =
            parts.iter().enumerate().fold(
                (0, -1.0),
                |best, (i, p)| if p.3 > best.1 { (i, p.3) } else { best },
            );
        let (lo, hi, _, _) = parts.swap_remove(i);
        let mid = 0.5 * (lo + hi);
        let (l, le) = kronrod(f, lo, mid)?;
        let (r, re) = kronrod(f, mid, hi)?;
        parts.push((lo, mid, l, le));
        parts.push((mid, hi, r, re));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_integrals() {
        let v = integrate(&mut |x| Ok(x.sin()), 0.0, std::f64::consts::PI, 1e-12).unwrap();
        assert!((v - 2.0).abs() < 1e-12);
        let g = integrate(&mut |x| Ok((-x * x).exp()), -10.0, 10.0, 1e-12).unwrap();
        assert!((g - std::f64::consts::PI.sqrt()).abs() < 1e-11);
        let s = integrate(&mut |x| Ok(x.abs().sqrt()), -1.0, 1.0, 1e-10).unwrap();
        assert!((s - 4.0 / 3.0).abs() < 1e-9);
    }
}
