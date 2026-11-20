//! Exact Gaussian elimination over expressions, for the small linear systems the solver
//! meets: undetermined coefficients and the constants fixed by initial conditions.

use crate::eval::EmptyEnv;
use crate::expr::Expr;
use crate::simplify::{add, mul, pow, simplify};

/// Most unknowns accepted.
pub const MAX_UNKNOWNS: usize = 40;

fn is_zero(e: &Expr) -> bool {
    if e.is_zero() {
        return true;
    }
    if e.is_constant() {
        if let Ok(z) = e.eval_complex(&EmptyEnv) {
            return z.norm() < 1e-13;
        }
    }
    let expanded = crate::expand::expand(e).map(|x| simplify(&x));
    matches!(expanded, Ok(x) if x.is_zero())
}

/// Solves `A u = b` (rows of `A` and `b`), returning one solution or `None` when the
/// system is inconsistent or underdetermined. Free unknowns are not allowed.
pub fn solve(mut a: Vec<Vec<Expr>>, mut b: Vec<Expr>) -> Option<Vec<Expr>> {
    let rows = a.len();
    let cols = a.first().map_or(0, Vec::len);
    if cols == 0 || cols > MAX_UNKNOWNS || b.len() != rows {
        return None;
    }
    let mut pivot_cols = Vec::new();
    let mut r = 0;
    for c in 0..cols {
        let Some(p) = (r..rows).find(|&i| !is_zero(&a[i][c])) else {
            continue;
        };
        a.swap(r, p);
        b.swap(r, p);
        let inv = pow(a[r][c].clone(), Expr::int(-1));
        for v in a[r][c..].iter_mut() {
            *v = simplify(&mul(vec![v.clone(), inv.clone()]));
        }
        b[r] = simplify(&mul(vec![b[r].clone(), inv]));
        for i in 0..rows {
            if i == r || is_zero(&a[i][c]) {
                continue;
            }
            let f = a[i][c].clone();
            for j in c..cols {
                a[i][j] = simplify(&add(vec![
                    a[i][j].clone(),
                    mul(vec![Expr::int(-1), f.clone(), a[r][j].clone()]),
                ]));
            }
            b[i] = simplify(&add(vec![
                b[i].clone(),
                mul(vec![Expr::int(-1), f, b[r].clone()]),
            ]));
        }
        pivot_cols.push(c);
        r += 1;
        if r == rows {
            break;
        }
    }
    if pivot_cols.len() != cols {
        return None;
    }
    if b.iter().skip(r).any(|v| !is_zero(v)) {
        return None;
    }
    let mut out = vec![Expr::zero(); cols];
    for (row, &c) in pivot_cols.iter().enumerate() {
        out[c] = b[row].clone();
    }
    Some(out)
}

/// The coefficients of `e` as a linear form in `unknowns`: `(rows of A, b)` with
/// `Σ Aⱼ uⱼ = b`. `None` when `e` is not linear in them.
pub fn linear_form(e: &Expr, unknowns: &[String]) -> Option<(Vec<Expr>, Expr)> {
    let mut rest = crate::expand::expand(e).unwrap_or_else(|_| e.clone());
    let mut row = Vec::with_capacity(unknowns.len());
    for u in unknowns {
        let c = crate::poly::as_poly(&rest, u)?;
        match c.len() {
            0 | 1 => row.push(Expr::zero()),
            2 => {
                row.push(simplify(&c[1]));
                rest = c[0].clone();
            }
            _ => return None,
        }
    }
    if row
        .iter()
        .chain(std::iter::once(&rest))
        .any(|c| unknowns.iter().any(|u| c.contains_sym(u)))
    {
        return None;
    }
    Some((row, simplify(&mul(vec![Expr::int(-1), rest]))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_systems() {
        let n = Expr::int;
        // x + y = 3, x − y = 1
        let s = solve(vec![vec![n(1), n(1)], vec![n(1), n(-1)]], vec![n(3), n(1)]).unwrap();
        assert_eq!(s, vec![n(2), n(1)]);
        assert!(solve(vec![vec![n(1), n(1)], vec![n(2), n(2)]], vec![n(1), n(3)]).is_none());
        let (row, rhs) = linear_form(
            &(n(2) * Expr::sym("a") - Expr::sym("b") + n(5)),
            &["a".into(), "b".into()],
        )
        .unwrap();
        assert_eq!(row, vec![n(2), n(-1)]);
        assert_eq!(rhs, n(-5));
    }
}
