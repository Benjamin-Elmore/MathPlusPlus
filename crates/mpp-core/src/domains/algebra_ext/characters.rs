//! Character tables of small finite groups: cyclic groups C_n (n ≤ 24), the Klein
//! four-group V₄, S₃, D₄, Q₈, A₄ and S₄. Rows are the irreducible characters, columns
//! the conjugacy classes (sizes given separately). Tables follow James & Liebeck,
//! *Representations and Characters of Groups*, chapters 18–19.

use crate::error::{MathError, MathResult};
use crate::expr::Expr;

/// Largest cyclic group tabulated.
pub const MAX_CYCLIC: usize = 24;

/// A character table.
#[derive(Clone, Debug, PartialEq)]
pub struct CharacterTable {
    /// Group order.
    pub order: usize,
    /// Conjugacy class sizes, in column order.
    pub class_sizes: Vec<usize>,
    /// Short class labels, in column order.
    pub classes: Vec<&'static str>,
    /// `chars[i][j]` = χ_i on class j.
    pub chars: Vec<Vec<Expr>>,
}

fn ints(rows: &[&[i64]]) -> Vec<Vec<Expr>> {
    rows.iter()
        .map(|r| r.iter().map(|&x| Expr::int(x)).collect())
        .collect()
}

/// `e^{2πi k/n}` as an exact expression (simplified to ±1, ±i where possible).
fn root_of_unity(k: usize, n: usize) -> Expr {
    let k = k % n;
    if k == 0 {
        return Expr::one();
    }
    if 2 * k == n {
        return Expr::int(-1);
    }
    if 4 * k == n {
        return Expr::i();
    }
    if 4 * k == 3 * n {
        return -Expr::i();
    }
    Expr::e()
        .pow(Expr::int(2) * Expr::pi() * Expr::i() * Expr::rational(k as i64, n as i64))
        .simplify()
}

/// The table for `name`: "C<n>", "V4" (or "K4"), "S3", "D4", "Q8", "A4", "S4".
pub fn table(name: &str) -> MathResult<CharacterTable> {
    let key: String = name
        .chars()
        .filter(|c| !matches!(c, '_' | '{' | '}' | ' '))
        .collect::<String>()
        .to_uppercase();
    let t = match key.as_str() {
        "V4" | "K4" | "C2XC2" => CharacterTable {
            order: 4,
            class_sizes: vec![1, 1, 1, 1],
            classes: vec!["e", "a", "b", "ab"],
            chars: ints(&[
                &[1, 1, 1, 1],
                &[1, 1, -1, -1],
                &[1, -1, 1, -1],
                &[1, -1, -1, 1],
            ]),
        },
        "S3" | "D3" => CharacterTable {
            order: 6,
            class_sizes: vec![1, 3, 2],
            classes: vec!["e", "(12)", "(123)"],
            chars: ints(&[&[1, 1, 1], &[1, -1, 1], &[2, 0, -1]]),
        },
        "D4" | "D8" => CharacterTable {
            order: 8,
            class_sizes: vec![1, 1, 2, 2, 2],
            classes: vec!["e", "r²", "r", "s", "sr"],
            chars: ints(&[
                &[1, 1, 1, 1, 1],
                &[1, 1, 1, -1, -1],
                &[1, 1, -1, 1, -1],
                &[1, 1, -1, -1, 1],
                &[2, -2, 0, 0, 0],
            ]),
        },
        "Q8" => CharacterTable {
            order: 8,
            class_sizes: vec![1, 1, 2, 2, 2],
            classes: vec!["1", "−1", "±i", "±j", "±k"],
            chars: ints(&[
                &[1, 1, 1, 1, 1],
                &[1, 1, 1, -1, -1],
                &[1, 1, -1, 1, -1],
                &[1, 1, -1, -1, 1],
                &[2, -2, 0, 0, 0],
            ]),
        },
        "A4" => {
            let w = root_of_unity(1, 3);
            let w2 = root_of_unity(2, 3);
            let one = Expr::one();
            CharacterTable {
                order: 12,
                class_sizes: vec![1, 3, 4, 4],
                classes: vec!["e", "(12)(34)", "(123)", "(132)"],
                chars: vec![
                    vec![one.clone(), one.clone(), one.clone(), one.clone()],
                    vec![one.clone(), one.clone(), w.clone(), w2.clone()],
                    vec![one.clone(), one, w2, w],
                    vec![Expr::int(3), Expr::int(-1), Expr::zero(), Expr::zero()],
                ],
            }
        }
        "S4" => CharacterTable {
            order: 24,
            class_sizes: vec![1, 6, 3, 8, 6],
            classes: vec!["e", "(12)", "(12)(34)", "(123)", "(1234)"],
            chars: ints(&[
                &[1, 1, 1, 1, 1],
                &[1, -1, 1, 1, -1],
                &[2, 0, 2, -1, 0],
                &[3, 1, -1, 0, -1],
                &[3, -1, -1, 0, 1],
            ]),
        },
        k if k.starts_with('C') && k[1..].parse::<usize>().is_ok() => {
            let n: usize = k[1..].parse().unwrap_or(0);
            if n == 0 || n > MAX_CYCLIC {
                return Err(MathError::TooLarge(format!(
                    "the cyclic group C_{n} (n ≤ {MAX_CYCLIC})"
                )));
            }
            CharacterTable {
                order: n,
                class_sizes: vec![1; n],
                classes: vec!["g^k"; n],
                chars: (0..n)
                    .map(|j| (0..n).map(|k| root_of_unity(j * k, n)).collect())
                    .collect(),
            }
        }
        _ => {
            return Err(MathError::Domain {
                function: "chartable".into(),
                detail: format!(
                    "no character table for {name}; try C<n>, V4, S3, D4, Q8, A4 or S4"
                ),
            })
        }
    };
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::EmptyEnv;

    #[test]
    fn orthogonality_and_dimensions() {
        let names: Vec<String> = ["V4", "S3", "D4", "Q8", "A4", "S4", "C5", "C6", "C12"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        for name in &names {
            let t = table(name).unwrap();
            let g = t.order as f64;
            assert_eq!(t.class_sizes.iter().sum::<usize>(), t.order, "{name}");
            // Σ d_i² = |G| (catalogue) and row orthogonality Σ_C |C| χ_i χ̄_j = |G| δ_ij.
            let vals: Vec<Vec<num_complex::Complex64>> = t
                .chars
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|x| x.eval_complex(&EmptyEnv).unwrap())
                        .collect()
                })
                .collect();
            let dims: f64 = vals.iter().map(|r| r[0].re * r[0].re).sum();
            assert!((dims - g).abs() < 1e-9, "{name}");
            for i in 0..vals.len() {
                for j in 0..vals.len() {
                    let s: num_complex::Complex64 = (0..t.class_sizes.len())
                        .map(|c| t.class_sizes[c] as f64 * vals[i][c] * vals[j][c].conj())
                        .sum();
                    let want = if i == j { g } else { 0.0 };
                    assert!(
                        (s.re - want).abs() < 1e-9 && s.im.abs() < 1e-9,
                        "{name} rows {i}, {j}"
                    );
                }
            }
        }
        assert!(table("M24").is_err());
        assert!(table("C100").is_err());
        assert_eq!(table("S_3").unwrap().order, 6);
    }
}
