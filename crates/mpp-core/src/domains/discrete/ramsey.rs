//! Small Ramsey checks by exhaustive search: does every red/blue colouring of the
//! edges of K_n contain a red K_s or a blue K_t? Edge colourings are bit masks, so the
//! search is capped at n ≤ 7 (2²¹ colourings).

use crate::error::{MathError, MathResult};

/// Largest n searched.
pub const MAX_N: usize = 7;

fn edge_index(n: usize) -> Vec<Vec<usize>> {
    let mut idx = vec![vec![0; n]; n];
    let mut k = 0;
    for i in 0..n {
        for j in i + 1..n {
            idx[i][j] = k;
            idx[j][i] = k;
            k += 1;
        }
    }
    idx
}

/// Edge masks of every complete subgraph on `size` vertices.
fn clique_masks(n: usize, size: usize, idx: &[Vec<usize>]) -> Vec<u32> {
    let mut out = Vec::new();
    for subset in 0u32..(1 << n) {
        if subset.count_ones() as usize != size {
            continue;
        }
        let verts: Vec<usize> = (0..n).filter(|v| subset >> v & 1 == 1).collect();
        let mut mask = 0u32;
        for (a, &u) in verts.iter().enumerate() {
            for &v in &verts[a + 1..] {
                mask |= 1 << idx[u][v];
            }
        }
        out.push(mask);
    }
    out
}

/// `None` when every colouring of K_n has a red K_s or a blue K_t (so R(s, t) ≤ n);
/// otherwise a colouring without either, as the list of red edges (1-based).
pub fn counterexample(s: usize, t: usize, n: usize) -> MathResult<Option<Vec<(usize, usize)>>> {
    if n > MAX_N {
        return Err(MathError::TooLarge(format!(
            "a Ramsey search on K_{n} (the limit is K_{MAX_N})"
        )));
    }
    if s < 2 || t < 2 {
        return Err(MathError::Domain {
            function: "ramsey".into(),
            detail: "the clique sizes must be at least 2".into(),
        });
    }
    if s > n || t > n {
        // A one-colour K_n avoids the larger clique.
        return Ok(Some(if s > n {
            (0..n)
                .flat_map(|i| (i + 1..n).map(move |j| (i + 1, j + 1)))
                .collect()
        } else {
            Vec::new()
        }));
    }
    let idx = edge_index(n);
    let edges = n * (n - 1) / 2;
    let red = clique_masks(n, s, &idx);
    let blue = clique_masks(n, t, &idx);
    let all = (1u32 << edges) - 1; // edges ≤ 21
    for colouring in 0..=all {
        let has_red = red.iter().any(|&m| colouring & m == m);
        let has_blue = !has_red && blue.iter().any(|&m| !colouring & m == m);
        if !has_red && !has_blue {
            let red_edges = (0..n)
                .flat_map(|i| (i + 1..n).map(move |j| (i, j)))
                .filter(|&(i, j)| colouring >> idx[i][j] & 1 == 1)
                .map(|(i, j)| (i + 1, j + 1))
                .collect();
            return Ok(Some(red_edges));
        }
    }
    Ok(None)
}

/// The smallest n ≤ [`MAX_N`] with every colouring of K_n forced, if there is one.
pub fn ramsey_number(s: usize, t: usize) -> MathResult<Option<usize>> {
    for n in 1..=MAX_N {
        if n >= s.max(t) && counterexample(s, t, n)?.is_none() {
            return Ok(Some(n));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_values() {
        // R(3, 3) = 6 (catalogue): K₅ has a 2-colouring (the pentagon/pentagram) with no
        // monochromatic triangle, K₆ does not.
        let c5 = counterexample(3, 3, 5).unwrap().unwrap();
        assert_eq!(c5.len(), 5);
        assert!(counterexample(3, 3, 6).unwrap().is_none());
        assert_eq!(ramsey_number(3, 3).unwrap(), Some(6));
        // R(2, t) = t.
        assert_eq!(ramsey_number(2, 5).unwrap(), Some(5));
        assert!(counterexample(3, 3, 8).is_err());
    }

    #[test]
    fn symmetric_in_the_colours() {
        for n in 2..=5 {
            for (s, t) in [(2, 3), (3, 4), (2, 4), (3, 3)] {
                assert_eq!(
                    counterexample(s, t, n).unwrap().is_some(),
                    counterexample(t, s, n).unwrap().is_some()
                );
            }
        }
    }
}
