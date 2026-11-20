//! Complete elliptic integrals of the first and second kind in terms of the
//! **modulus** `k` (the parameter is `m = k²`), by the arithmetic–geometric mean
//! (DLMF 19.8.5 and 19.8.6):
//! `K(k) = π / (2 AGM(1, k′))`, `E(k) = K(k) (1 − Σ 2^{n−1} c_n²)`, `k′ = √(1 − k²)`.

use std::f64::consts::PI;

use crate::error::{MathError, MathResult};

/// Most AGM steps (it converges quadratically, so a handful suffice).
const MAX_STEPS: usize = 64;

fn check(name: &str, k: f64) -> MathResult<()> {
    if !k.is_finite() || k.abs() > 1.0 {
        return Err(MathError::Domain {
            function: name.into(),
            detail: format!("the modulus must satisfy |k| ≤ 1, not {k}"),
        });
    }
    Ok(())
}

/// `(K(k), E(k))` together.
pub fn complete(k: f64) -> MathResult<(f64, f64)> {
    check("K", k)?;
    if k.abs() == 1.0 {
        return Err(MathError::Undefined(
            "K(±1) (it diverges logarithmically)".into(),
        ));
    }
    let mut a = 1.0f64;
    let mut b = (1.0 - k * k).sqrt();
    let mut c = k;
    let mut sum = 0.5 * c * c; // 2^{−1} c₀²
    let mut pow2 = 0.5;
    for _ in 0..MAX_STEPS {
        if (a - b).abs() <= 1e-16 * a {
            break;
        }
        let an = 0.5 * (a + b);
        let bn = (a * b).sqrt();
        // c_{n+1} = c_n² / (4 a_{n+1}) avoids the cancellation in (a_n − b_n)/2.
        c = c * c / (4.0 * an);
        a = an;
        b = bn;
        pow2 *= 2.0;
        sum += pow2 * c * c;
    }
    let kk = PI / (2.0 * a);
    Ok((kk, kk * (1.0 - sum)))
}

/// `K(k)`.
pub fn ellip_k(k: f64) -> MathResult<f64> {
    complete(k).map(|(kk, _)| kk)
}

/// `E(k)`; `E(±1) = 1`.
pub fn ellip_e(k: f64) -> MathResult<f64> {
    check("E", k)?;
    if k.abs() == 1.0 {
        return Ok(1.0);
    }
    complete(k).map(|(_, e)| e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::fieldkit::close;
    use proptest::prelude::*;

    #[test]
    fn reference_values() {
        let k = std::f64::consts::FRAC_1_SQRT_2;
        // Abramowitz & Stegun Table 17.1 (m = 1/2), DLMF §19.20.
        assert!(close(ellip_k(k).unwrap(), 1.854_074_677_301_372, 1e-14));
        let e = ellip_e(k).unwrap();
        assert!(close(e, 1.350_643_881_047_675_5, 1e-14), "{e}");
        assert!(close(ellip_k(0.0).unwrap(), PI / 2.0, 1e-15));
        assert!(close(ellip_e(0.0).unwrap(), PI / 2.0, 1e-15));
        assert_eq!(ellip_e(1.0).unwrap(), 1.0);
        assert!(ellip_k(1.0).is_err());
        assert!(ellip_k(1.5).is_err());
        // Pendulum period at θ₀ = 30°: T = 4√(L/g) K(sin 15°) ≈ 2.041 s (lab 2).
        let t = 4.0 * (1.0f64 / 9.81).sqrt() * ellip_k((15f64).to_radians().sin()).unwrap();
        assert!((t - 2.041).abs() < 0.001, "{t}");
    }

    proptest! {
        #[test]
        fn legendre_relation(k in 0.01f64..0.99) {
            // E K′ + E′ K − K K′ = π/2 (DLMF 19.7.1).
            let kp = (1.0 - k * k).sqrt();
            let (kk, e) = complete(k).unwrap();
            let (kkp, ep) = complete(kp).unwrap();
            prop_assert!((e * kkp + ep * kk - kk * kkp - PI / 2.0).abs() < 1e-12);
        }
    }
}
