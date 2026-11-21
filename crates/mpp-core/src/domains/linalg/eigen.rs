//! Characteristic polynomials, eigenvalues (exact through polynomial roots) and
//! eigenvectors (null spaces of `A − λI`).

use crate::domains::linalg::Matrix;
use crate::domains::solve::poly::{self, Root};
use crate::domains::util::numeric;
use crate::error::{MathError, MathResult};
use crate::expr::Expr;

/// Largest matrix whose eigenvalues are computed.
pub const MAX_EIGEN_DIM: usize = 10;
/// The internal name of the eigenvalue variable.
const LAMBDA: &str = "λ";

/// An eigenvalue with its multiplicity and eigenvectors.
#[derive(Clone, Debug, PartialEq)]
pub struct Eigen {
    /// The eigenvalue.
    pub value: Expr,
    /// Algebraic multiplicity.
    pub multiplicity: u32,
    /// A basis of the eigenspace (column vectors).
    pub vectors: Vec<Matrix>,
    /// False when the value or vectors are numeric.
    pub exact: bool,
}

impl Matrix {
    /// The characteristic polynomial `det(var·I − A)`.
    pub fn charpoly(&self, var: &str) -> MathResult<Expr> {
        if !self.is_square() {
            return Err(MathError::Domain {
                function: "charpoly".into(),
                detail: "the characteristic polynomial needs a square matrix".into(),
            });
        }
        let n = self.rows();
        if n > MAX_EIGEN_DIM {
            return Err(MathError::TooLarge(format!(
                "the eigenvalues of a {n}×{n} matrix"
            )));
        }
        let shifted = Matrix::identity(n)?.scale(&Expr::sym(var)).sub(self)?;
        let d = shifted.det()?;
        let coeffs = d
            .as_poly(var)
            .ok_or_else(|| MathError::NotPolynomial { var: var.into() })?;
        Ok(crate::simplify::add(
            coeffs
                .iter()
                .enumerate()
                .map(|(k, c)| c * &Expr::sym(var).pow(Expr::int(k as i64)))
                .collect(),
        ))
    }

    /// Eigenvalues with algebraic multiplicities.
    pub fn eigenvalues(&self) -> MathResult<Vec<Root>> {
        let p = self.charpoly(LAMBDA)?;
        let coeffs = p
            .as_poly(LAMBDA)
            .ok_or_else(|| MathError::NotPolynomial { var: LAMBDA.into() })?;
        poly::roots(&coeffs)
    }

    /// Eigenvalues with their eigenvectors.
    pub fn eigenvectors(&self) -> MathResult<Vec<Eigen>> {
        let n = self.rows();
        let mut out = Vec::new();
        for root in self.eigenvalues()? {
            let shifted = self
                .sub(&Matrix::identity(n)?.scale(&root.value))?
                .simplified();
            let mut vectors = shifted.nullspace()?;
            let mut exact = root.exact;
            if vectors.is_empty() {
                // The exact zero tests could not see the rank drop: work numerically.
                vectors = numeric_nullspace(&shifted)?;
                exact = false;
            }
            out.push(Eigen {
                value: root.value,
                multiplicity: root.multiplicity,
                vectors,
                exact,
            });
        }
        Ok(out)
    }
}

/// The null space with every entry evaluated to `f64` first.
fn numeric_nullspace(m: &Matrix) -> MathResult<Vec<Matrix>> {
    let entries: Option<Vec<Expr>> = m
        .entries()
        .iter()
        .map(|e| numeric(e).map(Expr::float))
        .collect();
    let entries = entries.ok_or_else(|| {
        MathError::Unsupported("eigenvectors for complex eigenvalues of this matrix".into())
    })?;
    Matrix::new(m.rows(), m.cols(), entries)?.nullspace_loose()
}

impl Matrix {
    /// A null space for a numeric matrix that is singular only up to rounding: the
    /// smallest pivot is dropped when the exact rank is full.
    fn nullspace_loose(&self) -> MathResult<Vec<Matrix>> {
        let ns = self.nullspace()?;
        if !ns.is_empty() {
            return Ok(ns);
        }
        // Drop the last row (the one rounding kept alive) and try again.
        if self.rows() < 2 {
            return Ok(Vec::new());
        }
        let data = self.entries()[..(self.rows() - 1) * self.cols()].to_vec();
        Matrix::new(self.rows() - 1, self.cols(), data)?.nullspace()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::EmptyEnv;

    fn m(rows: &[&[i64]]) -> Matrix {
        Matrix::from_rows(
            rows.iter()
                .map(|r| r.iter().map(|&v| Expr::int(v)).collect())
                .collect(),
        )
        .unwrap()
    }

    /// Checks A v = λ v numerically for every eigenpair.
    fn check_pairs(a: &Matrix) {
        for e in a.eigenvectors().unwrap() {
            assert!(!e.vectors.is_empty(), "no eigenvector for {}", e.value);
            for v in &e.vectors {
                let lhs = a.mul(v).unwrap();
                let rhs = v.scale(&e.value);
                for (x, y) in lhs.entries().iter().zip(rhs.entries()) {
                    let d = (x - y).eval_complex(&EmptyEnv).unwrap();
                    assert!(d.norm() < 1e-8, "A v ≠ λ v for λ = {}", e.value);
                }
            }
        }
    }

    #[test]
    fn charpoly_and_eigenvalues() {
        let a = m(&[&[2, 1], &[1, 2]]);
        let l = Expr::sym("t");
        assert_eq!(
            a.charpoly("t").unwrap(),
            l.pow(2) - Expr::int(4) * l + Expr::int(3)
        );
        let ev: Vec<Expr> = a
            .eigenvalues()
            .unwrap()
            .into_iter()
            .map(|r| r.value)
            .collect();
        assert_eq!(ev, vec![Expr::int(1), Expr::int(3)]);
        check_pairs(&a);
    }

    #[test]
    fn repeated_and_complex() {
        let a = m(&[&[1, 1], &[0, 1]]);
        let es = a.eigenvectors().unwrap();
        assert_eq!(es.len(), 1);
        assert_eq!(es[0].multiplicity, 2);
        assert_eq!(es[0].vectors.len(), 1);
        let r = m(&[&[0, -1], &[1, 0]]);
        let ev = r.eigenvalues().unwrap();
        assert_eq!(ev.len(), 2);
        check_pairs(&r);
    }

    #[test]
    fn irrational_eigenvalues() {
        check_pairs(&m(&[&[1, 1], &[1, 0]]));
        check_pairs(&m(&[&[2, 1, 0], &[1, 3, 1], &[0, 1, 4]]));
        check_pairs(&m(&[
            &[4, 1, 0, 0],
            &[1, 3, 1, 0],
            &[0, 1, 2, 1],
            &[0, 0, 1, 1],
        ]));
    }
}
