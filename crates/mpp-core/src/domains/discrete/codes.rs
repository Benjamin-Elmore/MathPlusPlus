//! Coding theory: Hamming distance, the parameters [n, k, d] of a linear code over 𝔽_q
//! (q prime) from a generator matrix (rank by row reduction mod q, minimum distance by
//! enumerating the q^k codewords, capped), and binary Hamming codes
//! [2^m − 1, 2^m − 1 − m, 3] in systematic form.

use crate::error::{MathError, MathResult};

/// Most codewords enumerated for the minimum distance.
pub const MAX_CODEWORDS: u64 = 1 << 20;
/// Largest m for a Hamming code (n = 2^m − 1 columns).
pub const MAX_HAMMING_M: usize = 6;

/// The number of positions where `a` and `b` differ.
pub fn hamming<T: PartialEq>(a: &[T], b: &[T]) -> MathResult<usize> {
    if a.len() != b.len() {
        return Err(MathError::Domain {
            function: "hamming".into(),
            detail: format!(
                "the words have different lengths ({} and {})",
                a.len(),
                b.len()
            ),
        });
    }
    Ok(a.iter().zip(b).filter(|(x, y)| x != y).count())
}

fn inv_mod(a: u64, q: u64) -> u64 {
    let mut base = a % q;
    let mut e = q - 2;
    let mut acc = 1u64;
    while e > 0 {
        if e & 1 == 1 {
            acc = acc * base % q;
        }
        base = base * base % q;
        e >>= 1;
    }
    acc
}

/// Row-reduces `g` over 𝔽_q and returns the non-zero rows (a basis of the code).
pub fn row_basis(g: &[Vec<u64>], q: u64) -> Vec<Vec<u64>> {
    let mut m: Vec<Vec<u64>> = g
        .iter()
        .map(|r| r.iter().map(|x| x % q).collect())
        .collect();
    let cols = m.first().map_or(0, Vec::len);
    let mut rank = 0;
    for c in 0..cols {
        let Some(p) = (rank..m.len()).find(|&r| m[r][c] != 0) else {
            continue;
        };
        m.swap(rank, p);
        let inv = inv_mod(m[rank][c], q);
        for x in &mut m[rank] {
            *x = *x * inv % q;
        }
        for r in 0..m.len() {
            if r != rank && m[r][c] != 0 {
                let f = m[r][c];
                for j in 0..cols {
                    m[r][j] = (m[r][j] + q * q - f * m[rank][j] % q) % q;
                }
            }
        }
        rank += 1;
    }
    m.truncate(rank);
    m
}

/// `(n, k, d)` of the code spanned by the rows of `g` over 𝔽_q.
pub fn parameters(g: &[Vec<u64>], q: u64) -> MathResult<(usize, usize, usize)> {
    let n = g.first().map_or(0, Vec::len);
    let basis = row_basis(g, q);
    let k = basis.len();
    if k == 0 {
        return Ok((n, 0, 0));
    }
    let count = (q as u128).checked_pow(k as u32).unwrap_or(u128::MAX);
    if count > MAX_CODEWORDS as u128 {
        return Err(MathError::TooLarge(format!(
            "a code with {q}^{k} codewords"
        )));
    }
    let mut best = n;
    let mut coeffs = vec![0u64; k];
    for _ in 1..count as u64 {
        // Next coefficient vector (base-q counter).
        for c in &mut coeffs {
            *c += 1;
            if *c == q {
                *c = 0;
            } else {
                break;
            }
        }
        let weight = (0..n)
            .filter(|&j| {
                coeffs
                    .iter()
                    .zip(&basis)
                    .map(|(c, row)| c * row[j])
                    .sum::<u64>()
                    % q
                    != 0
            })
            .count();
        best = best.min(weight);
    }
    Ok((n, k, best))
}

/// A generator matrix `[I_k | Aᵀ]` of the binary Hamming code with `m` check bits.
pub fn hamming_code(m: usize) -> MathResult<Vec<Vec<u64>>> {
    if !(2..=MAX_HAMMING_M).contains(&m) {
        return Err(MathError::Domain {
            function: "hammingcode".into(),
            detail: format!("m must be 2 … {MAX_HAMMING_M}"),
        });
    }
    let n = (1usize << m) - 1;
    // Columns of the parity-check matrix H = [A | I_m]: non-unit-weight vectors first.
    let a_cols: Vec<usize> = (1..=n).filter(|v| !v.is_power_of_two()).collect();
    let k = a_cols.len();
    let mut g = vec![vec![0u64; n]; k];
    for (i, &col) in a_cols.iter().enumerate() {
        g[i][i] = 1;
        for bit in 0..m {
            g[i][k + bit] = (col >> bit & 1) as u64;
        }
    }
    Ok(g)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn hamming_codes() {
        // [7, 4, 3] and [15, 11, 3] (catalogue: [2^m − 1, 2^m − 1 − m, 3]).
        assert_eq!(parameters(&hamming_code(3).unwrap(), 2).unwrap(), (7, 4, 3));
        assert_eq!(
            parameters(&hamming_code(4).unwrap(), 2).unwrap(),
            (15, 11, 3)
        );
        assert_eq!(hamming(&[1, 0, 1, 1], &[1, 1, 1, 0]).unwrap(), 2);
        assert!(hamming(&[1], &[1, 0]).is_err());
        // The ternary repetition code [3, 1, 3].
        assert_eq!(parameters(&[vec![1, 1, 1]], 3).unwrap(), (3, 1, 3));
    }

    proptest! {
        #[test]
        fn singleton_bound(rows in prop::collection::vec(prop::collection::vec(0u64..3, 6), 1..4)) {
            let (n, k, d) = parameters(&rows, 3).unwrap();
            prop_assert!(k == 0 || d <= n - k + 1);
        }

        #[test]
        fn hamming_metric(a in prop::collection::vec(0u8..2, 8), b in prop::collection::vec(0u8..2, 8), c in prop::collection::vec(0u8..2, 8)) {
            let (ab, bc, ac) = (hamming(&a, &b).unwrap(), hamming(&b, &c).unwrap(), hamming(&a, &c).unwrap());
            prop_assert!(ac <= ab + bc);
            prop_assert_eq!(ab, hamming(&b, &a).unwrap());
        }
    }
}
