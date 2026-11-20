//! The matrix exponential `exp(tA) = Σ (tA)^k / k!`.
//!
//! Exact where a closed form is cheap: nilpotent matrices (the series stops), 2 × 2
//! matrices (`e^{τt}[cosh(st) I + sinh(st)/s (A − τI)]` with τ = tr A / 2 and
//! s² = τ² − det A, and its cos/sin and s = 0 forms), and diagonalisable matrices with
//! exact real eigenvalues (`P e^{tD} P⁻¹`). Otherwise numeric, by scaling and squaring
//! with a Taylor polynomial (Moler & Van Loan 2003, method 3).

use crate::domains::linalg::Matrix;
use crate::domains::util::{is_zero, numeric, sign};
use crate::error::{MathError, MathResult};
use crate::expr::Expr;
use crate::functions::{func, Func};

/// Largest matrix for the exact paths.
const MAX_EXACT: usize = 6;

fn exp(e: &Expr) -> Expr {
    Expr::e().pow(e.clone())
}

fn is_zero_matrix(m: &Matrix) -> bool {
    m.entries().iter().all(is_zero)
}

/// `exp(tA)` for a square matrix `a` and a scalar `t` (a number or a symbol).
pub fn expm(a: &Matrix, t: &Expr) -> MathResult<Matrix> {
    if !a.is_square() {
        return Err(MathError::Domain {
            function: "exp".into(),
            detail: "the matrix exponential needs a square matrix".into(),
        });
    }
    let n = a.rows();
    if n <= MAX_EXACT {
        if let Some(m) = nilpotent(a, t)? {
            return Ok(m);
        }
        if n == 2 {
            if let Some(m) = two_by_two(a, t)? {
                return Ok(m);
            }
        }
        if let Some(m) = diagonalisable(a, t)? {
            return Ok(m);
        }
    }
    let tv = numeric(t).ok_or_else(|| {
        MathError::Unsupported(format!(
            "an exact exponential of this matrix with symbolic {t}"
        ))
    })?;
    numeric_expm(a, tv)
}

fn nilpotent(a: &Matrix, t: &Expr) -> MathResult<Option<Matrix>> {
    let n = a.rows();
    let mut sum = Matrix::identity(n)?;
    let mut term = Matrix::identity(n)?;
    for k in 1..=n {
        term = term.mul(a)?.scale(&(t / &Expr::int(k as i64))).simplified();
        if is_zero_matrix(&term) {
            return Ok(Some(sum.simplified()));
        }
        sum = sum.add(&term)?;
    }
    Ok(None)
}

fn two_by_two(a: &Matrix, t: &Expr) -> MathResult<Option<Matrix>> {
    let tau = (a.trace()? / Expr::int(2)).simplify();
    let det = a.det()?;
    let s2 = (tau.pow(2) - det).simplify();
    if !s2.is_constant() {
        return Ok(None);
    }
    let i2 = Matrix::identity(2)?;
    let shifted = a.sub(&i2.scale(&tau))?;
    let pre = exp(&(&tau * t));
    let (c, sfac) = match sign(&s2) {
        Some(std::cmp::Ordering::Equal) => (Expr::one(), t.clone()),
        Some(std::cmp::Ordering::Greater) => {
            let s = s2.sqrt();
            let st = &s * t;
            (
                func(Func::Cosh, vec![st.clone()]),
                func(Func::Sinh, vec![st]) / s,
            )
        }
        Some(std::cmp::Ordering::Less) => {
            let w = (-s2).sqrt();
            let wt = &w * t;
            (
                func(Func::Cos, vec![wt.clone()]),
                func(Func::Sin, vec![wt]) / w,
            )
        }
        None => return Ok(None),
    };
    let m = i2.scale(&c).add(&shifted.scale(&sfac))?.scale(&pre);
    Ok(Some(m.simplified()))
}

fn diagonalisable(a: &Matrix, t: &Expr) -> MathResult<Option<Matrix>> {
    let Ok(eigs) = a.eigenvectors() else {
        return Ok(None);
    };
    let n = a.rows();
    if eigs
        .iter()
        .any(|e| !e.exact || !e.value.free_symbols().is_empty())
    {
        return Ok(None);
    }
    if eigs.iter().any(|e| numeric(&e.value).is_none()) {
        return Ok(None); // complex eigenvalues: leave to the numeric path
    }
    let mut cols: Vec<Matrix> = Vec::new();
    let mut lambdas: Vec<Expr> = Vec::new();
    for e in &eigs {
        for v in &e.vectors {
            cols.push(v.clone());
            lambdas.push(e.value.clone());
        }
    }
    if cols.len() != n {
        return Ok(None);
    }
    let mut data = vec![Expr::zero(); n * n];
    for (j, v) in cols.iter().enumerate() {
        for i in 0..n {
            data[i * n + j] = v.get(i, 0).clone();
        }
    }
    let p = Matrix::new(n, n, data)?;
    let p_inv = p.inverse()?;
    let mut d = vec![Expr::zero(); n * n];
    for (i, l) in lambdas.iter().enumerate() {
        d[i * n + i] = exp(&(l * t));
    }
    let d = Matrix::new(n, n, d)?;
    Ok(Some(p.mul(&d)?.mul(&p_inv)?.simplified()))
}

/// Numeric `exp(tA)`: scale so ‖tA‖ ≤ 1/2, Taylor to degree 18, square back.
fn numeric_expm(a: &Matrix, t: f64) -> MathResult<Matrix> {
    let n = a.rows();
    let vals: Vec<f64> = a
        .entries()
        .iter()
        .map(|x| numeric(x).map(|v| v * t))
        .collect::<Option<Vec<f64>>>()
        .ok_or_else(|| {
            MathError::Unsupported(
                "a numeric exponential of a matrix with complex or symbolic entries".into(),
            )
        })?;
    let norm = (0..n)
        .map(|i| (0..n).map(|j| vals[i * n + j].abs()).sum::<f64>())
        .fold(0.0, f64::max);
    let mut squarings = 0u32;
    let mut scale = 1.0;
    while norm * scale > 0.5 && squarings < 1_000 {
        scale *= 0.5;
        squarings += 1;
    }
    let x: Vec<f64> = vals.iter().map(|v| v * scale).collect();
    let mul = |p: &[f64], q: &[f64]| -> Vec<f64> {
        let mut out = vec![0.0; n * n];
        for i in 0..n {
            for k in 0..n {
                let pik = p[i * n + k];
                for j in 0..n {
                    out[i * n + j] += pik * q[k * n + j];
                }
            }
        }
        out
    };
    let mut result: Vec<f64> = (0..n * n)
        .map(|i| if i / n == i % n { 1.0 } else { 0.0 })
        .collect();
    let mut term = result.clone();
    for k in 1..=18 {
        term = mul(&term, &x).into_iter().map(|v| v / k as f64).collect();
        for (r, v) in result.iter_mut().zip(&term) {
            *r += v;
        }
    }
    for _ in 0..squarings {
        result = mul(&result, &result);
    }
    if result.iter().any(|v| !v.is_finite()) {
        return Err(MathError::TooLarge(
            "this matrix exponential (it overflows)".into(),
        ));
    }
    Matrix::new(n, n, result.into_iter().map(Expr::float).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::fieldkit::close;
    use proptest::prelude::*;

    fn m(rows: &[&[i64]]) -> Matrix {
        Matrix::from_rows(
            rows.iter()
                .map(|r| r.iter().map(|&v| Expr::int(v)).collect())
                .collect(),
        )
        .unwrap()
    }

    fn num(m: &Matrix) -> Vec<f64> {
        m.entries().iter().map(|x| numeric(x).unwrap()).collect()
    }

    #[test]
    fn exact_cases() {
        let t = Expr::sym("t");
        // Nilpotent: exp(t [[0,1],[0,0]]) = [[1, t], [0, 1]].
        let e = expm(&m(&[&[0, 1], &[0, 0]]), &t).unwrap();
        assert_eq!(
            e.to_expr().to_string(),
            Matrix::from_rows(vec![
                vec![Expr::one(), t.clone()],
                vec![Expr::zero(), Expr::one()]
            ])
            .unwrap()
            .to_expr()
            .to_string()
        );
        // Rotation generator: exp(t [[0,−1],[1,0]]) = [[cos t, −sin t], [sin t, cos t]].
        let r = expm(&m(&[&[0, -1], &[1, 0]]), &t).unwrap();
        assert_eq!(r.get(0, 0), &func(Func::Cos, vec![t.clone()]));
        assert_eq!(r.get(1, 0), &func(Func::Sin, vec![t.clone()]));
        // Diagonalisable 3 × 3 with eigenvalues 1, 2, 3 at t = 1, against the numeric path.
        let a = m(&[&[1, 1, 0], &[0, 2, 1], &[0, 0, 3]]);
        let exact = num(&expm(&a, &Expr::one()).unwrap());
        let approx = num(&numeric_expm(&a, 1.0).unwrap());
        for (x, y) in exact.iter().zip(&approx) {
            assert!(close(*x, *y, 1e-12));
        }
        assert!(expm(&m(&[&[1, 2, 3]]), &t).is_err());
    }

    proptest! {
        #[test]
        fn one_parameter_group(a in -3i64..3, b in -3i64..3, c in -3i64..3, d in -3i64..3, s in -1.0f64..1.0, u in -1.0f64..1.0) {
            // exp(sX) exp(uX) = exp((s+u)X) (catalogue, Lie theory).
            let x = m(&[&[a, b], &[c, d]]);
            let es = numeric_expm(&x, s).unwrap();
            let eu = numeric_expm(&x, u).unwrap();
            let lhs = num(&es.mul(&eu).unwrap());
            let rhs = num(&numeric_expm(&x, s + u).unwrap());
            for (p, q) in lhs.iter().zip(&rhs) {
                prop_assert!(close(*p, *q, 1e-10));
            }
            // The exact 2 × 2 formula agrees with the numeric path.
            let exact = num(&expm(&x, &Expr::float(s)).unwrap());
            let approx = num(&es);
            for (p, q) in exact.iter().zip(&approx) {
                prop_assert!(close(*p, *q, 1e-10));
            }
        }
    }
}
