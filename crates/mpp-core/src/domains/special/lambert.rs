//! The Lambert W function on the real line: the principal branch `W₀` (x ≥ −1/e,
//! W ≥ −1) and the lower branch `W₋₁` (−1/e ≤ x < 0, W ≤ −1). Halley's iteration from
//! a branch-point series or an asymptotic starting value (Corless et al., 1996).

use std::f64::consts::E;

use crate::error::{MathError, MathResult};

/// Most Halley steps.
const MAX_ITER: usize = 100;

fn domain(detail: String) -> MathError {
    MathError::Domain {
        function: "W".into(),
        detail,
    }
}

/// Halley's iteration for `w e^w = x`.
fn halley(x: f64, mut w: f64) -> f64 {
    for _ in 0..MAX_ITER {
        let ew = w.exp();
        let f = w * ew - x;
        let wp1 = w + 1.0;
        if wp1 == 0.0 {
            break;
        }
        let denom = ew * wp1 - (w + 2.0) * f / (2.0 * wp1);
        if denom == 0.0 || !denom.is_finite() {
            break;
        }
        let step = f / denom;
        w -= step;
        if step.abs() <= 1e-15 * (1.0 + w.abs()) {
            break;
        }
    }
    w
}

/// `W_k(x)` for `k = 0` or `k = −1`.
pub fn lambert_w(x: f64, branch: i64) -> MathResult<f64> {
    let branch_pt = -1.0 / E;
    if !x.is_finite() {
        return Err(domain(format!("{x} is not a finite number")));
    }
    if x < branch_pt - 1e-15 {
        return Err(domain(format!(
            "W(x) is real only for x ≥ −1/e, and {x} is below it"
        )));
    }
    // Series in p = √(2(e x + 1)) about the branch point: W = −1 ± p − p²/3 ± 11p³/72.
    let p = (2.0 * (E * x + 1.0)).max(0.0).sqrt();
    match branch {
        0 => {
            if x == 0.0 {
                return Ok(0.0);
            }
            let w0 = if p < 0.5 {
                -1.0 + p - p * p / 3.0 + 11.0 / 72.0 * p * p * p
            } else if x < 3.0 {
                // ln(1 + x) is a fair start on the middle range.
                (1.0 + x).ln() * 0.8
            } else {
                let l1 = x.ln();
                let l2 = l1.ln();
                l1 - l2 + l2 / l1
            };
            Ok(halley(x, w0))
        }
        -1 => {
            if x >= 0.0 {
                return Err(domain(format!(
                    "the branch W₋₁(x) is real only for −1/e ≤ x < 0, not x = {x}"
                )));
            }
            let w0 = if p < 0.5 {
                -1.0 - p - p * p / 3.0 - 11.0 / 72.0 * p * p * p
            } else {
                let l1 = (-x).ln();
                let l2 = (-l1).ln();
                l1 - l2 + l2 / l1
            };
            Ok(halley(x, w0))
        }
        _ => Err(domain(format!(
            "only the real branches k = 0 and k = −1 are available, not k = {branch}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::fieldkit::close;
    use proptest::prelude::*;

    #[test]
    fn reference_values() {
        // The omega constant W₀(1), OEIS A030178.
        assert!(close(
            lambert_w(1.0, 0).unwrap(),
            0.567_143_290_409_783_8,
            1e-15
        ));
        assert!(close(lambert_w(E, 0).unwrap(), 1.0, 1e-15));
        assert!(close(lambert_w(-1.0 / E, 0).unwrap(), -1.0, 1e-7));
        assert!(close(lambert_w(-1.0 / E, -1).unwrap(), -1.0, 1e-7));
        // W₋₁(−0.1) = −3.577152063957297 (Corless et al. 1996; DLMF §4.13).
        assert!(close(
            lambert_w(-0.1, -1).unwrap(),
            -3.577_152_063_957_297,
            1e-14
        ));
        // W₀(10) = 1.7455280027406994 (DLMF §4.13).
        assert!(close(
            lambert_w(10.0, 0).unwrap(),
            1.745_528_002_740_699,
            1e-14
        ));
        assert!(lambert_w(-1.0, 0).is_err());
        assert!(lambert_w(1.0, -1).is_err());
        assert!(lambert_w(1.0, 2).is_err());
    }

    proptest! {
        #[test]
        fn principal_inverts(x in -0.36f64..1e6) {
            let w = lambert_w(x, 0).unwrap();
            prop_assert!(w >= -1.0);
            prop_assert!((w * w.exp() - x).abs() <= 1e-12 * x.abs().max(1.0));
        }

        #[test]
        fn lower_inverts(x in -0.3678f64..-1e-12) {
            let w = lambert_w(x, -1).unwrap();
            prop_assert!(w <= -1.0);
            prop_assert!((w * w.exp() - x).abs() <= 1e-12 * x.abs().max(1e-300) + 1e-15);
        }
    }
}
