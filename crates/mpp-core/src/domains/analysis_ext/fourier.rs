//! The discrete Fourier transform `X[k] = Σ_j x[j] e^{−2πijk/N}` (catalogue
//! convention, no normalisation forward, 1/N on the inverse): direct O(N²) summation,
//! and a fast transform (iterative radix-2 for powers of two, Bluestein's chirp-z
//! algorithm for other lengths).

use std::f64::consts::PI;

use num_complex::Complex64;

use crate::error::{MathError, MathResult};

/// Longest input for the direct transform.
pub const MAX_DIRECT: usize = 8_192;
/// Longest input for the fast transform.
pub const MAX_FFT: usize = 1 << 20;

/// The direct DFT (or its inverse when `inverse`).
pub fn dft(x: &[Complex64], inverse: bool) -> MathResult<Vec<Complex64>> {
    let n = x.len();
    if n > MAX_DIRECT {
        return Err(MathError::TooLarge(format!(
            "a direct DFT of {n} points (use the FFT)"
        )));
    }
    let sign = if inverse { 1.0 } else { -1.0 };
    let mut out = Vec::with_capacity(n);
    for k in 0..n {
        let mut acc = Complex64::new(0.0, 0.0);
        for (j, xj) in x.iter().enumerate() {
            // Reduce jk mod n first so the angle stays small and accurate.
            let jk = (j * k) % n;
            acc += xj * Complex64::from_polar(1.0, sign * 2.0 * PI * jk as f64 / n as f64);
        }
        out.push(if inverse { acc / n as f64 } else { acc });
    }
    Ok(out)
}

fn radix2(a: &mut [Complex64], inverse: bool) {
    let n = a.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            a.swap(i, j);
        }
    }
    let sign = if inverse { 1.0 } else { -1.0 };
    let mut len = 2;
    while len <= n {
        let w = Complex64::from_polar(1.0, sign * 2.0 * PI / len as f64);
        for start in (0..n).step_by(len) {
            let mut wk = Complex64::new(1.0, 0.0);
            for k in 0..len / 2 {
                let u = a[start + k];
                let v = a[start + k + len / 2] * wk;
                a[start + k] = u + v;
                a[start + k + len / 2] = u - v;
                wk *= w;
            }
        }
        len <<= 1;
    }
}

/// The fast transform (or its inverse), same convention as [`dft`].
pub fn fft(x: &[Complex64], inverse: bool) -> MathResult<Vec<Complex64>> {
    let n = x.len();
    if n > MAX_FFT {
        return Err(MathError::TooLarge(format!("an FFT of {n} points")));
    }
    if n <= 1 {
        return Ok(x.to_vec());
    }
    let mut out = if n.is_power_of_two() {
        let mut a = x.to_vec();
        radix2(&mut a, inverse);
        a
    } else {
        bluestein(x, inverse)
    };
    if inverse {
        for v in &mut out {
            *v /= n as f64;
        }
    }
    Ok(out)
}

/// Bluestein: `jk = (j² + k² − (k−j)²)/2` turns the DFT into a convolution.
fn bluestein(x: &[Complex64], inverse: bool) -> Vec<Complex64> {
    let n = x.len();
    let m = (2 * n - 1).next_power_of_two();
    let sign = if inverse { 1.0 } else { -1.0 };
    let chirp: Vec<Complex64> = (0..n)
        .map(|k| {
            let k2 = (k as u128 * k as u128 % (2 * n as u128)) as f64;
            Complex64::from_polar(1.0, sign * PI * k2 / n as f64)
        })
        .collect();
    let mut a = vec![Complex64::new(0.0, 0.0); m];
    for k in 0..n {
        a[k] = x[k] * chirp[k];
    }
    let mut b = vec![Complex64::new(0.0, 0.0); m];
    b[0] = chirp[0].conj();
    for k in 1..n {
        b[k] = chirp[k].conj();
        b[m - k] = chirp[k].conj();
    }
    radix2(&mut a, false);
    radix2(&mut b, false);
    for (ai, bi) in a.iter_mut().zip(&b) {
        *ai *= bi;
    }
    radix2(&mut a, true);
    (0..n).map(|k| a[k] / m as f64 * chirp[k]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn c(x: f64) -> Complex64 {
        Complex64::new(x, 0.0)
    }

    #[test]
    fn small_transforms() {
        // DFT of (1, 2, 3, 4) = (10, −2+2i, −2, −2−2i); X[0]/N is the mean (catalogue).
        let x = [c(1.0), c(2.0), c(3.0), c(4.0)];
        let d = dft(&x, false).unwrap();
        let want = [
            Complex64::new(10.0, 0.0),
            Complex64::new(-2.0, 2.0),
            c(-2.0),
            Complex64::new(-2.0, -2.0),
        ];
        for (a, b) in d.iter().zip(want.iter()) {
            assert!((a - b).norm() < 1e-12);
        }
        let f = fft(&x, false).unwrap();
        for (a, b) in f.iter().zip(want.iter()) {
            assert!((a - b).norm() < 1e-12);
        }
    }

    proptest! {
        #[test]
        fn fft_matches_dft_and_inverts(v in prop::collection::vec(-10.0f64..10.0, 1..70)) {
            let x: Vec<Complex64> = v.iter().map(|&t| c(t)).collect();
            let d = dft(&x, false).unwrap();
            let f = fft(&x, false).unwrap();
            for (a, b) in d.iter().zip(&f) {
                prop_assert!((a - b).norm() < 1e-9);
            }
            let back = fft(&f, true).unwrap();
            for (a, b) in back.iter().zip(&x) {
                prop_assert!((a - b).norm() < 1e-9);
            }
            // Parseval: Σ|x|² = (1/N) Σ|X|².
            let e1: f64 = x.iter().map(|z| z.norm_sqr()).sum();
            let e2: f64 = f.iter().map(|z| z.norm_sqr()).sum::<f64>() / x.len() as f64;
            prop_assert!((e1 - e2).abs() < 1e-8 * e1.max(1.0));
        }
    }
}
