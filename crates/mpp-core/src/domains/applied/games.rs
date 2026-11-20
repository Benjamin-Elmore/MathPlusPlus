//! Finite two-player zero-sum games solved exactly by linear programming: the value
//! `max_x min_y xᵀAy = min_y max_x xᵀAy` and optimal mixed strategies, by the simplex
//! method on rationals with Bland's rule (so it always terminates). Plus the
//! two-player Shapley value.

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};

use crate::error::{MathError, MathResult};

/// Largest payoff matrix side.
pub const MAX_SIDE: usize = 50;

/// The solution of a zero-sum game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameSolution {
    /// The value to the row player.
    pub value: BigRational,
    /// An optimal mixed strategy of the row (maximising) player.
    pub row: Vec<BigRational>,
    /// An optimal mixed strategy of the column (minimising) player.
    pub col: Vec<BigRational>,
}

/// Solves the game with payoff matrix `a` (row player receives `a[i][j]`).
pub fn solve(a: &[Vec<BigRational>]) -> MathResult<GameSolution> {
    let m = a.len();
    let n = a.first().map_or(0, Vec::len);
    if m == 0 || n == 0 || a.iter().any(|r| r.len() != n) {
        return Err(MathError::Domain {
            function: "gamevalue".into(),
            detail: "the payoff matrix must be rectangular and non-empty".into(),
        });
    }
    if m > MAX_SIDE || n > MAX_SIDE {
        return Err(MathError::TooLarge(format!("a {m}×{n} game")));
    }
    // Shift every payoff to be ≥ 1 so the LP value is positive.
    let min = a.iter().flatten().min().cloned().unwrap_or_default();
    let shift = BigRational::one() - min;
    // Tableau: rows 0..m are A'y + s = 1, last row is the objective z − Σ y = 0.
    let cols = n + m;
    let mut t: Vec<Vec<BigRational>> = Vec::with_capacity(m + 1);
    for (i, row) in a.iter().enumerate() {
        let mut r: Vec<BigRational> = row.iter().map(|x| x + &shift).collect();
        r.extend((0..m).map(|k| {
            if k == i {
                BigRational::one()
            } else {
                BigRational::zero()
            }
        }));
        r.push(BigRational::one());
        t.push(r);
    }
    let mut obj: Vec<BigRational> = vec![-BigRational::one(); n];
    obj.extend(vec![BigRational::zero(); m + 1]);
    t.push(obj);
    let mut basis: Vec<usize> = (n..n + m).collect();
    // Bland: the lowest-index column with a negative reduced cost enters.
    while let Some(enter) = (0..cols).find(|&j| t[m][j].is_negative()) {
        let mut leave: Option<usize> = None;
        for i in 0..m {
            if t[i][enter].is_positive() {
                let ratio = &t[i][cols] / &t[i][enter];
                leave = match leave {
                    None => Some(i),
                    Some(l) => {
                        let best = &t[l][cols] / &t[l][enter];
                        if ratio < best || (ratio == best && basis[i] < basis[l]) {
                            Some(i)
                        } else {
                            Some(l)
                        }
                    }
                };
            }
        }
        let Some(r) = leave else {
            return Err(MathError::Undefined(
                "a bounded linear program (unexpected)".into(),
            ));
        };
        let piv = t[r][enter].clone();
        for x in &mut t[r] {
            *x = &*x / &piv;
        }
        let pivot_row = t[r].clone();
        for (i, row) in t.iter_mut().enumerate() {
            if i != r && !row[enter].is_zero() {
                let f = row[enter].clone();
                for (x, p) in row.iter_mut().zip(&pivot_row) {
                    *x -= &f * p;
                }
            }
        }
        basis[r] = enter;
    }
    let z = t[m][cols].clone();
    let mut y = vec![BigRational::zero(); n];
    for (i, &b) in basis.iter().enumerate() {
        if b < n {
            y[b] = t[i][cols].clone();
        }
    }
    let x: Vec<BigRational> = (0..m).map(|i| t[m][n + i].clone() / &z).collect();
    let col: Vec<BigRational> = y.into_iter().map(|v| v / &z).collect();
    Ok(GameSolution {
        value: z.recip() - shift,
        row: x,
        col,
    })
}

/// Two-player Shapley values `(φ₁, φ₂)` with v(∅) = 0:
/// `φ₁ = [v({1}) + v({1,2}) − v({2})]/2` and symmetrically.
pub fn shapley2(
    v1: &BigRational,
    v2: &BigRational,
    v12: &BigRational,
) -> (BigRational, BigRational) {
    let two = BigRational::from_integer(BigInt::from(2));
    ((v1 + v12 - v2) / &two, (v2 + v12 - v1) / two)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn q(n: i64) -> BigRational {
        BigRational::from_integer(BigInt::from(n))
    }

    fn game(rows: &[&[i64]]) -> Vec<Vec<BigRational>> {
        rows.iter()
            .map(|r| r.iter().map(|&x| q(x)).collect())
            .collect()
    }

    #[test]
    fn classic_games() {
        // Matching pennies: value 0, both mix 1/2–1/2 (catalogue).
        let s = solve(&game(&[&[1, -1], &[-1, 1]])).unwrap();
        let half = BigRational::new(1.into(), 2.into());
        assert_eq!(s.value, q(0));
        assert_eq!(s.row, vec![half.clone(), half.clone()]);
        assert_eq!(s.col, vec![half.clone(), half]);
        // Rock–paper–scissors: value 0, uniform play.
        let rps = solve(&game(&[&[0, -1, 1], &[1, 0, -1], &[-1, 1, 0]])).unwrap();
        assert_eq!(rps.value, q(0));
        assert!(rps
            .row
            .iter()
            .all(|p| *p == BigRational::new(1.into(), 3.into())));
        // [[3, 1], [2, 4]]: no saddle point, value 5/2 with x = (1/2, 1/2).
        let g = solve(&game(&[&[3, 1], &[2, 4]])).unwrap();
        assert_eq!(g.value, BigRational::new(5.into(), 2.into()));
        // A saddle point: [[4, 5], [2, 1]] has value 4.
        assert_eq!(solve(&game(&[&[4, 5], &[2, 1]])).unwrap().value, q(4));
        // Shapley: v1 = 1, v2 = 2, v12 = 6 → (5/2, 7/2).
        let (a, b) = shapley2(&q(1), &q(2), &q(6));
        assert_eq!(
            (a, b),
            (
                BigRational::new(5.into(), 2.into()),
                BigRational::new(7.into(), 2.into())
            )
        );
    }

    proptest! {
        #[test]
        fn optimal_strategies_guarantee_the_value(entries in prop::collection::vec(-9i64..10, 9), m in 1usize..4, n in 1usize..4) {
            let a: Vec<Vec<BigRational>> = (0..m).map(|i| (0..n).map(|j| q(entries[i * 3 + j])).collect()).collect();
            let s = solve(&a).unwrap();
            let one = BigRational::one();
            prop_assert_eq!(s.row.iter().sum::<BigRational>(), one.clone());
            prop_assert_eq!(s.col.iter().sum::<BigRational>(), one);
            // Row strategy earns ≥ v against every column; column strategy concedes ≤ v to every row.
            for j in 0..n {
                let pay: BigRational = a.iter().zip(&s.row).map(|(r, x)| x * &r[j]).sum();
                prop_assert!(pay >= s.value);
            }
            for row in &a {
                let pay: BigRational = row.iter().zip(&s.col).map(|(x, y)| x * y).sum();
                prop_assert!(pay <= s.value);
            }
        }

        #[test]
        fn shapley_is_efficient(v1 in -50i64..50, v2 in -50i64..50, v12 in -50i64..50) {
            let (a, b) = shapley2(&q(v1), &q(v2), &q(v12));
            prop_assert_eq!(a + b, q(v12));
        }
    }
}
