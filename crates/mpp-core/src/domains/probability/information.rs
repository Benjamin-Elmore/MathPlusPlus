//! Information theory on finite alphabets: Shannon entropy (with 0 log 0 = 0), joint
//! and conditional entropy, mutual information `I(X;Y) = H(X) + H(Y) − H(X,Y)`, the
//! binary symmetric channel's capacity `1 − H₂(p)` and the capacity of any discrete
//! memoryless channel by the Blahut–Arimoto algorithm (Blahut 1972, Arimoto 1972).

use crate::error::{MathError, MathResult};

/// Tolerance on probabilities summing to 1.
const SUM_TOL: f64 = 1e-9;
/// Most Blahut–Arimoto iterations.
const MAX_BA: usize = 100_000;

fn check_distribution(p: &[f64]) -> MathResult<()> {
    if p.iter().any(|&x| !(0.0..=1.0 + SUM_TOL).contains(&x)) {
        return Err(MathError::Domain {
            function: "H".into(),
            detail: "probabilities must lie in [0, 1]".into(),
        });
    }
    let s: f64 = p.iter().sum();
    if (s - 1.0).abs() > SUM_TOL * p.len().max(1) as f64 {
        return Err(MathError::Domain {
            function: "H".into(),
            detail: format!("the probabilities sum to {s}, not 1"),
        });
    }
    Ok(())
}

/// `−Σ p log_b p` (0 log 0 = 0) for a probability vector.
pub fn entropy(p: &[f64], base: f64) -> MathResult<f64> {
    check_distribution(p)?;
    if !(base > 0.0 && base != 1.0) {
        return Err(MathError::Domain {
            function: "H".into(),
            detail: format!("the logarithm base must be positive and not 1, not {base}"),
        });
    }
    let h: f64 = p.iter().filter(|&&x| x > 0.0).map(|&x| -x * x.ln()).sum();
    Ok((h / base.ln()).max(0.0))
}

/// The binary entropy `H₂(p)` in bits.
pub fn binary_entropy(p: f64) -> MathResult<f64> {
    entropy(&[p, 1.0 - p], 2.0)
}

fn flatten(joint: &[Vec<f64>]) -> Vec<f64> {
    joint.iter().flatten().copied().collect()
}

/// `(H(X), H(Y), H(X,Y))` in bits for a joint distribution `joint[x][y]`.
pub fn entropies(joint: &[Vec<f64>]) -> MathResult<(f64, f64, f64)> {
    let all = flatten(joint);
    check_distribution(&all)?;
    let px: Vec<f64> = joint.iter().map(|r| r.iter().sum()).collect();
    let cols = joint.first().map_or(0, Vec::len);
    let py: Vec<f64> = (0..cols)
        .map(|j| joint.iter().map(|r| r[j]).sum())
        .collect();
    Ok((entropy(&px, 2.0)?, entropy(&py, 2.0)?, entropy(&all, 2.0)?))
}

/// `I(X;Y)` in bits.
pub fn mutual_information(joint: &[Vec<f64>]) -> MathResult<f64> {
    let (hx, hy, hxy) = entropies(joint)?;
    Ok((hx + hy - hxy).max(0.0))
}

/// The capacity of a binary symmetric channel with flip probability p, in bits.
pub fn bsc_capacity(p: f64) -> MathResult<f64> {
    Ok(1.0 - binary_entropy(p)?)
}

/// The capacity (bits) and an optimal input distribution of the channel with
/// transition matrix `w[x][y] = P(y | x)`, by Blahut–Arimoto to `tol`.
pub fn channel_capacity(w: &[Vec<f64>], tol: f64) -> MathResult<(f64, Vec<f64>)> {
    for row in w {
        check_distribution(row)?;
    }
    let nx = w.len();
    let ny = w.first().map_or(0, Vec::len);
    if nx == 0 || ny == 0 {
        return Err(MathError::Domain {
            function: "capacity".into(),
            detail: "the channel matrix is empty".into(),
        });
    }
    let mut p = vec![1.0 / nx as f64; nx];
    for _ in 0..MAX_BA {
        // q(y) = Σ_x p(x) w(y|x); D(x) = Σ_y w(y|x) ln(w(y|x)/q(y)).
        let q: Vec<f64> = (0..ny)
            .map(|y| (0..nx).map(|x| p[x] * w[x][y]).sum())
            .collect();
        let d: Vec<f64> = (0..nx)
            .map(|x| {
                (0..ny)
                    .filter(|&y| w[x][y] > 0.0)
                    .map(|y| w[x][y] * (w[x][y] / q[y]).ln())
                    .sum()
            })
            .collect();
        let weights: Vec<f64> = (0..nx).map(|x| p[x] * d[x].exp()).collect();
        let z: f64 = weights.iter().sum();
        // Bounds: ln z ≤ C ≤ max D (nats).
        let lower = z.ln();
        let upper = d.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        p = weights.iter().map(|v| v / z).collect();
        if upper - lower < tol {
            return Ok((lower / std::f64::consts::LN_2, p));
        }
    }
    Err(MathError::Inconclusive(
        "Blahut–Arimoto did not converge".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn reference_values() {
        // Fair coin: 1 bit; uniform on 8: 3 bits; point mass: 0 (catalogue).
        assert!((entropy(&[0.5, 0.5], 2.0).unwrap() - 1.0).abs() < 1e-15);
        assert!((entropy(&[0.125; 8], 2.0).unwrap() - 3.0).abs() < 1e-14);
        assert_eq!(entropy(&[1.0, 0.0], 2.0).unwrap(), 0.0);
        assert!(entropy(&[0.5, 0.6], 2.0).is_err());
        // BSC(0.11): 1 − H₂(0.11) = 0.50008404 (closed form, Cover & Thomas §7.1.4).
        assert!((bsc_capacity(0.11).unwrap() - 0.500_084_04).abs() < 1e-8);
        // Independent X, Y: I = 0; X = Y uniform on 2: I = 1 bit.
        let ind = vec![vec![0.25, 0.25], vec![0.25, 0.25]];
        assert!(mutual_information(&ind).unwrap().abs() < 1e-15);
        let same = vec![vec![0.5, 0.0], vec![0.0, 0.5]];
        assert!((mutual_information(&same).unwrap() - 1.0).abs() < 1e-15);
        // Binary erasure channel with ε = 0.3: C = 1 − ε = 0.7 (Cover & Thomas §7.1.5).
        let bec = vec![vec![0.7, 0.3, 0.0], vec![0.0, 0.3, 0.7]];
        let (c, p) = channel_capacity(&bec, 1e-12).unwrap();
        assert!((c - 0.7).abs() < 1e-9);
        assert!((p[0] - 0.5).abs() < 1e-6);
    }

    proptest! {
        #[test]
        fn blahut_arimoto_matches_bsc(p in 0.0f64..1.0) {
            let w = vec![vec![1.0 - p, p], vec![p, 1.0 - p]];
            let (c, _) = channel_capacity(&w, 1e-12).unwrap();
            prop_assert!((c - bsc_capacity(p).unwrap()).abs() < 1e-8);
        }

        #[test]
        fn entropy_bounds(v in prop::collection::vec(0.0f64..1.0, 1..10)) {
            let s: f64 = v.iter().sum();
            prop_assume!(s > 1e-6);
            let p: Vec<f64> = v.iter().map(|x| x / s).collect();
            let h = entropy(&p, 2.0).unwrap();
            prop_assert!(h >= 0.0 && h <= (p.len() as f64).log2() + 1e-12);
        }
    }
}
