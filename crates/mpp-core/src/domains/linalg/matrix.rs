//! Matrices over exact expressions: arithmetic, determinants, inverses and row reduction.
//!
//! Each algorithm picks its arithmetic from the entries: exact rationals use Gaussian
//! elimination over Q, approximate numbers use `f64` with partial pivoting, and anything
//! symbolic uses fraction-free elimination (row reduction) or cofactor expansion
//! (determinants) on expressions, with exact zero tests.

use std::collections::HashMap;

use num_rational::BigRational;
use num_traits::{One, Zero};

use crate::domains::util::{is_zero, normalize, numeric, rat_expr};
use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode};
use crate::number::Number;

/// Largest number of rows or columns.
pub const MAX_DIM: usize = 64;
/// Largest symbolic matrix for cofactor expansion and fraction-free elimination.
pub const MAX_SYMBOLIC_DIM: usize = 10;
/// Relative tolerance for zero tests with approximate entries.
const FLOAT_TOL: f64 = 1e-10;

/// A matrix of expressions stored row by row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Matrix {
    rows: usize,
    cols: usize,
    data: Vec<Expr>,
}

/// How the entries should be computed with.
enum Kind {
    Rational(Vec<BigRational>),
    Float(Vec<f64>),
    Symbolic,
}

fn shape_error(what: &str) -> MathError {
    MathError::Domain {
        function: "matrix".into(),
        detail: what.into(),
    }
}

fn check_dims(rows: usize, cols: usize) -> MathResult<()> {
    if rows == 0 || cols == 0 {
        return Err(shape_error(
            "a matrix needs at least one row and one column",
        ));
    }
    if rows > MAX_DIM || cols > MAX_DIM {
        return Err(MathError::TooLarge(format!("a {rows}×{cols} matrix")));
    }
    Ok(())
}

impl Matrix {
    /// A matrix from its entries, row by row.
    pub fn new(rows: usize, cols: usize, data: Vec<Expr>) -> MathResult<Matrix> {
        check_dims(rows, cols)?;
        if data.len() != rows * cols {
            return Err(shape_error(
                "the number of entries does not match the shape",
            ));
        }
        Ok(Matrix { rows, cols, data })
    }

    /// A matrix from rows of equal length.
    pub fn from_rows(rows: Vec<Vec<Expr>>) -> MathResult<Matrix> {
        let r = rows.len();
        let c = rows.first().map_or(0, Vec::len);
        if rows.iter().any(|row| row.len() != c) {
            return Err(shape_error("the rows have different lengths"));
        }
        Matrix::new(r, c, rows.into_iter().flatten().collect())
    }

    /// A column vector.
    pub fn column(entries: Vec<Expr>) -> MathResult<Matrix> {
        let n = entries.len();
        Matrix::new(n, 1, entries)
    }

    /// Reads a matrix value; a tuple is read as a column vector.
    pub fn from_expr(e: &Expr) -> Option<Matrix> {
        match e.node() {
            ExprNode::Matrix { rows, cols, data } => Matrix::new(*rows, *cols, data.clone()).ok(),
            ExprNode::Tuple(v) => Matrix::column(v.clone()).ok(),
            _ => None,
        }
    }

    /// The matrix as an expression.
    pub fn to_expr(&self) -> Expr {
        Expr::new(ExprNode::Matrix {
            rows: self.rows,
            cols: self.cols,
            data: self.data.clone(),
        })
    }

    /// The `n × n` identity.
    pub fn identity(n: usize) -> MathResult<Matrix> {
        check_dims(n, n)?;
        let data = (0..n * n)
            .map(|i| {
                if i / n == i % n {
                    Expr::one()
                } else {
                    Expr::zero()
                }
            })
            .collect();
        Ok(Matrix {
            rows: n,
            cols: n,
            data,
        })
    }

    /// The zero matrix.
    pub fn zeros(rows: usize, cols: usize) -> MathResult<Matrix> {
        check_dims(rows, cols)?;
        Ok(Matrix {
            rows,
            cols,
            data: vec![Expr::zero(); rows * cols],
        })
    }

    /// Number of rows.
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Number of columns.
    pub fn cols(&self) -> usize {
        self.cols
    }

    /// The entries, row by row.
    pub fn entries(&self) -> &[Expr] {
        &self.data
    }

    /// Entry `(r, c)`, counted from zero.
    pub fn get(&self, r: usize, c: usize) -> &Expr {
        &self.data[r * self.cols + c]
    }

    fn row_vecs(&self) -> Vec<Vec<Expr>> {
        self.data.chunks(self.cols).map(<[Expr]>::to_vec).collect()
    }

    /// True for row or column vectors.
    pub fn is_vector(&self) -> bool {
        self.rows == 1 || self.cols == 1
    }

    /// True for square matrices.
    pub fn is_square(&self) -> bool {
        self.rows == self.cols
    }

    fn require_square(&self, what: &str) -> MathResult<()> {
        if self.is_square() {
            Ok(())
        } else {
            Err(shape_error(&format!(
                "{what} needs a square matrix, not {}×{}",
                self.rows, self.cols
            )))
        }
    }

    fn map(&self, f: impl Fn(&Expr) -> Expr) -> Matrix {
        Matrix {
            rows: self.rows,
            cols: self.cols,
            data: self.data.iter().map(f).collect(),
        }
    }

    /// Entry-wise simplification.
    pub fn simplified(&self) -> Matrix {
        self.map(Expr::simplify)
    }

    fn same_shape(&self, other: &Matrix, what: &str) -> MathResult<()> {
        if self.rows == other.rows && self.cols == other.cols {
            Ok(())
        } else {
            Err(shape_error(&format!(
                "{what} needs matrices of the same shape, not {}×{} and {}×{}",
                self.rows, self.cols, other.rows, other.cols
            )))
        }
    }

    /// `self + other`.
    pub fn add(&self, other: &Matrix) -> MathResult<Matrix> {
        self.same_shape(other, "adding")?;
        Ok(Matrix {
            rows: self.rows,
            cols: self.cols,
            data: self
                .data
                .iter()
                .zip(&other.data)
                .map(|(a, b)| a + b)
                .collect(),
        })
    }

    /// `self − other`.
    pub fn sub(&self, other: &Matrix) -> MathResult<Matrix> {
        self.same_shape(other, "subtracting")?;
        Ok(Matrix {
            rows: self.rows,
            cols: self.cols,
            data: self
                .data
                .iter()
                .zip(&other.data)
                .map(|(a, b)| a - b)
                .collect(),
        })
    }

    /// `k · self`.
    pub fn scale(&self, k: &Expr) -> Matrix {
        self.map(|a| k * a)
    }

    /// The matrix product `self · other`.
    pub fn mul(&self, other: &Matrix) -> MathResult<Matrix> {
        if self.cols != other.rows {
            return Err(shape_error(&format!(
                "can't multiply a {}×{} matrix by a {}×{} matrix",
                self.rows, self.cols, other.rows, other.cols
            )));
        }
        let mut data = Vec::with_capacity(self.rows * other.cols);
        for r in 0..self.rows {
            for c in 0..other.cols {
                let terms = (0..self.cols)
                    .map(|k| self.get(r, k) * other.get(k, c))
                    .collect();
                data.push(crate::simplify::add(terms));
            }
        }
        Matrix::new(self.rows, other.cols, data)
    }

    /// The transpose.
    pub fn transpose(&self) -> Matrix {
        let mut data = Vec::with_capacity(self.data.len());
        for c in 0..self.cols {
            for r in 0..self.rows {
                data.push(self.get(r, c).clone());
            }
        }
        Matrix {
            rows: self.cols,
            cols: self.rows,
            data,
        }
    }

    /// The trace.
    pub fn trace(&self) -> MathResult<Expr> {
        self.require_square("the trace")?;
        Ok(crate::simplify::add(
            (0..self.rows).map(|i| self.get(i, i).clone()).collect(),
        ))
    }

    /// `[self | other]`.
    pub fn augment(&self, other: &Matrix) -> MathResult<Matrix> {
        if self.rows != other.rows {
            return Err(shape_error("augmenting needs the same number of rows"));
        }
        let cols = self.cols + other.cols;
        let mut data = Vec::with_capacity(self.rows * cols);
        for r in 0..self.rows {
            data.extend_from_slice(&self.data[r * self.cols..(r + 1) * self.cols]);
            data.extend_from_slice(&other.data[r * other.cols..(r + 1) * other.cols]);
        }
        Matrix::new(self.rows, cols, data)
    }

    /// The dot product of two vectors of equal length.
    pub fn dot(&self, other: &Matrix) -> MathResult<Expr> {
        if !self.is_vector() || !other.is_vector() || self.data.len() != other.data.len() {
            return Err(shape_error(
                "the dot product needs two vectors of the same length",
            ));
        }
        Ok(crate::simplify::add(
            self.data
                .iter()
                .zip(&other.data)
                .map(|(a, b)| a * b)
                .collect(),
        ))
    }

    /// The cross product of two 3-vectors (a column vector).
    pub fn cross(&self, other: &Matrix) -> MathResult<Matrix> {
        if !self.is_vector() || !other.is_vector() || self.data.len() != 3 || other.data.len() != 3
        {
            return Err(shape_error(
                "the cross product needs two vectors with 3 entries",
            ));
        }
        let (a, b) = (&self.data, &other.data);
        Matrix::column(vec![
            &a[1] * &b[2] - &a[2] * &b[1],
            &a[2] * &b[0] - &a[0] * &b[2],
            &a[0] * &b[1] - &a[1] * &b[0],
        ])
    }

    fn kind(&self) -> Kind {
        let rats: Option<Vec<BigRational>> = self
            .data
            .iter()
            .map(|e| e.as_number().and_then(Number::as_rational).cloned())
            .collect();
        if let Some(r) = rats {
            return Kind::Rational(r);
        }
        let any_float = self
            .data
            .iter()
            .any(|e| e.as_number().is_some_and(|n| !n.is_exact()));
        if any_float {
            if let Some(f) = self.data.iter().map(numeric).collect::<Option<Vec<f64>>>() {
                return Kind::Float(f);
            }
        }
        Kind::Symbolic
    }

    /// The determinant.
    pub fn det(&self) -> MathResult<Expr> {
        self.require_square("the determinant")?;
        let n = self.rows;
        match self.kind() {
            Kind::Rational(a) => Ok(rat_expr(det_rational(a, n))),
            Kind::Float(a) => Ok(Expr::float(det_float(a, n))),
            Kind::Symbolic => {
                if n > MAX_SYMBOLIC_DIM {
                    return Err(MathError::TooLarge(format!(
                        "the symbolic determinant of a {n}×{n} matrix"
                    )));
                }
                let mut memo = HashMap::new();
                let full = (1u32 << n) - 1;
                Ok(normalize(&self.laplace(0, full, &mut memo)))
            }
        }
    }

    /// Cofactor expansion along row `row` over the columns in `mask`, memoised.
    fn laplace(&self, row: usize, mask: u32, memo: &mut HashMap<u32, Expr>) -> Expr {
        if row == self.rows {
            return Expr::one();
        }
        if let Some(v) = memo.get(&mask) {
            return v.clone();
        }
        let mut terms = Vec::new();
        let mut sign = 1i64;
        for c in 0..self.cols {
            if mask & (1 << c) == 0 {
                continue;
            }
            let a = self.get(row, c);
            if !a.is_zero() {
                let minor = self.laplace(row + 1, mask & !(1 << c), memo);
                if !minor.is_zero() {
                    terms.push(Expr::int(sign) * a.clone() * minor);
                }
            }
            sign = -sign;
        }
        let v = crate::simplify::add(terms);
        memo.insert(mask, v.clone());
        v
    }

    /// The minor with row `r` and column `c` removed.
    pub fn minor(&self, r: usize, c: usize) -> MathResult<Matrix> {
        let data = (0..self.rows)
            .filter(|&i| i != r)
            .flat_map(|i| (0..self.cols).filter(move |&j| j != c).map(move |j| (i, j)))
            .map(|(i, j)| self.get(i, j).clone())
            .collect();
        Matrix::new(self.rows - 1, self.cols - 1, data)
    }

    /// The adjugate (transposed cofactor matrix).
    pub fn adjugate(&self) -> MathResult<Matrix> {
        self.require_square("the adjugate")?;
        let n = self.rows;
        if n == 1 {
            return Matrix::identity(1);
        }
        let mut data = Vec::with_capacity(n * n);
        for r in 0..n {
            for c in 0..n {
                // adj[r][c] = (−1)^(r+c) det(minor(c, r))
                let m = self.minor(c, r)?.det()?;
                let sign = if (r + c) % 2 == 0 { 1 } else { -1 };
                data.push((Expr::int(sign) * m).simplify());
            }
        }
        Matrix::new(n, n, data)
    }

    /// The inverse; an error for singular matrices.
    pub fn inverse(&self) -> MathResult<Matrix> {
        self.require_square("the inverse")?;
        let n = self.rows;
        let singular = || MathError::Undefined("the inverse of a singular matrix".into());
        match self.kind() {
            Kind::Rational(_) | Kind::Float(_) => {
                let aug = self.augment(&Matrix::identity(n)?)?;
                let (r, pivots) = aug.rref()?;
                if pivots.len() < n || pivots[n - 1] != n - 1 {
                    return Err(singular());
                }
                let data = (0..n)
                    .flat_map(|i| (n..2 * n).map(move |j| (i, j)))
                    .map(|(i, j)| r.get(i, j).clone())
                    .collect();
                Matrix::new(n, n, data)
            }
            Kind::Symbolic => {
                let d = self.det()?;
                if is_zero(&d) {
                    return Err(singular());
                }
                let adj = self.adjugate()?;
                Ok(adj.map(|a| crate::factor::factor(&(a / &d)).simplify()))
            }
        }
    }

    /// The reduced row echelon form and the pivot columns.
    pub fn rref(&self) -> MathResult<(Matrix, Vec<usize>)> {
        match self.kind() {
            Kind::Rational(a) => {
                let (r, p) = rref_rational(a, self.rows, self.cols);
                Ok((
                    Matrix {
                        rows: self.rows,
                        cols: self.cols,
                        data: r.into_iter().map(rat_expr).collect(),
                    },
                    p,
                ))
            }
            Kind::Float(a) => {
                let (r, p) = rref_float(a, self.rows, self.cols);
                Ok((
                    Matrix {
                        rows: self.rows,
                        cols: self.cols,
                        data: r.into_iter().map(Expr::float).collect(),
                    },
                    p,
                ))
            }
            Kind::Symbolic => self.rref_symbolic(),
        }
    }

    /// Zero test for symbolic elimination: exact, with a tolerance for approximate numbers.
    fn entry_is_zero(e: &Expr) -> bool {
        match e.as_number() {
            Some(n) if !n.is_exact() => n.to_complex64().norm() <= FLOAT_TOL,
            Some(n) => n.is_zero(),
            None => is_zero(e),
        }
    }

    fn rref_symbolic(&self) -> MathResult<(Matrix, Vec<usize>)> {
        if self.rows > MAX_SYMBOLIC_DIM || self.cols > MAX_SYMBOLIC_DIM + 1 {
            return Err(MathError::TooLarge(format!(
                "symbolic row reduction of a {}×{} matrix",
                self.rows, self.cols
            )));
        }
        let mut m = self.row_vecs();
        let mut pivots = Vec::new();
        let mut r = 0;
        for c in 0..self.cols {
            if r == self.rows {
                break;
            }
            let Some(p) = (r..self.rows).find(|&i| !Matrix::entry_is_zero(&m[i][c])) else {
                continue;
            };
            m.swap(r, p);
            let pv = m[r][c].clone();
            for i in 0..self.rows {
                if i == r || Matrix::entry_is_zero(&m[i][c]) {
                    continue;
                }
                let f = m[i][c].clone();
                for k in 0..self.cols {
                    m[i][k] = if k == c {
                        Expr::zero()
                    } else {
                        normalize(&(&pv * &m[i][k] - &f * &m[r][k]))
                    };
                }
            }
            pivots.push(c);
            r += 1;
        }
        for (i, &c) in pivots.iter().enumerate() {
            let pv = m[i][c].clone();
            for (k, e) in m[i].iter_mut().enumerate() {
                *e = if k == c {
                    Expr::one()
                } else if Matrix::entry_is_zero(e) {
                    Expr::zero()
                } else {
                    tidy(&(&*e / &pv))
                };
            }
        }
        for row in m.iter_mut().skip(pivots.len()) {
            row.iter_mut().for_each(|e| *e = Expr::zero());
        }
        Ok((
            Matrix {
                rows: self.rows,
                cols: self.cols,
                data: m.into_iter().flatten().collect(),
            },
            pivots,
        ))
    }

    /// The rank.
    pub fn rank(&self) -> MathResult<usize> {
        Ok(self.rref()?.1.len())
    }

    /// A basis of the null space, as column vectors.
    pub fn nullspace(&self) -> MathResult<Vec<Matrix>> {
        let (r, pivots) = self.rref()?;
        let mut basis = Vec::new();
        for free in (0..self.cols).filter(|c| !pivots.contains(c)) {
            let mut v = vec![Expr::zero(); self.cols];
            v[free] = Expr::one();
            for (row, &pc) in pivots.iter().enumerate() {
                v[pc] = (-r.get(row, free)).simplify();
            }
            basis.push(Matrix::column(v)?);
        }
        Ok(basis)
    }

    /// Euclidean norm of a vector, Frobenius norm of a matrix.
    pub fn norm(&self) -> Expr {
        let squares = self
            .data
            .iter()
            .map(|a| {
                if a.as_number().is_some_and(Number::is_real) || a.is_constant() {
                    a.pow(2)
                } else {
                    Expr::func(crate::functions::Func::Abs, vec![a.clone()])
                        .unwrap_or_else(|_| a.clone())
                        .pow(2)
                }
            })
            .collect();
        crate::simplify::add(squares).sqrt()
    }
}

/// Simplifies a quotient, cancelling common polynomial factors where possible.
fn tidy(e: &Expr) -> Expr {
    let s = e.simplify();
    let f = crate::factor::factor(&s);
    if f.size() < s.size() {
        f
    } else {
        s
    }
}

fn det_rational(mut a: Vec<BigRational>, n: usize) -> BigRational {
    let mut det = BigRational::one();
    for c in 0..n {
        let Some(p) = (c..n).find(|&r| !a[r * n + c].is_zero()) else {
            return BigRational::zero();
        };
        if p != c {
            for k in 0..n {
                a.swap(p * n + k, c * n + k);
            }
            det = -det;
        }
        let pv = a[c * n + c].clone();
        det *= &pv;
        for r in c + 1..n {
            if a[r * n + c].is_zero() {
                continue;
            }
            let f = &a[r * n + c] / &pv;
            for k in c..n {
                let v = &a[c * n + k] * &f;
                a[r * n + k] -= v;
            }
        }
    }
    det
}

fn det_float(mut a: Vec<f64>, n: usize) -> f64 {
    let mut det = 1.0;
    for c in 0..n {
        let p = (c..n)
            .max_by(|&i, &j| a[i * n + c].abs().total_cmp(&a[j * n + c].abs()))
            .unwrap_or(c);
        if a[p * n + c] == 0.0 {
            return 0.0;
        }
        if p != c {
            for k in 0..n {
                a.swap(p * n + k, c * n + k);
            }
            det = -det;
        }
        let pv = a[c * n + c];
        det *= pv;
        for r in c + 1..n {
            let f = a[r * n + c] / pv;
            for k in c..n {
                a[r * n + k] -= f * a[c * n + k];
            }
        }
    }
    det
}

fn rref_rational(
    mut a: Vec<BigRational>,
    rows: usize,
    cols: usize,
) -> (Vec<BigRational>, Vec<usize>) {
    let mut pivots = Vec::new();
    let mut r = 0;
    for c in 0..cols {
        if r == rows {
            break;
        }
        let Some(p) = (r..rows).find(|&i| !a[i * cols + c].is_zero()) else {
            continue;
        };
        for k in 0..cols {
            a.swap(p * cols + k, r * cols + k);
        }
        let pv = a[r * cols + c].clone();
        for k in 0..cols {
            a[r * cols + k] = &a[r * cols + k] / &pv;
        }
        for i in 0..rows {
            if i == r || a[i * cols + c].is_zero() {
                continue;
            }
            let f = a[i * cols + c].clone();
            for k in 0..cols {
                let v = &a[r * cols + k] * &f;
                a[i * cols + k] -= v;
            }
        }
        pivots.push(c);
        r += 1;
    }
    (a, pivots)
}

fn rref_float(mut a: Vec<f64>, rows: usize, cols: usize) -> (Vec<f64>, Vec<usize>) {
    let scale = a.iter().map(|v| v.abs()).fold(0.0, f64::max).max(1e-300);
    let tol = FLOAT_TOL * scale;
    let mut pivots = Vec::new();
    let mut r = 0;
    for c in 0..cols {
        if r == rows {
            break;
        }
        let p = (r..rows)
            .max_by(|&i, &j| a[i * cols + c].abs().total_cmp(&a[j * cols + c].abs()))
            .unwrap_or(r);
        if a[p * cols + c].abs() <= tol {
            for i in r..rows {
                a[i * cols + c] = 0.0;
            }
            continue;
        }
        for k in 0..cols {
            a.swap(p * cols + k, r * cols + k);
        }
        let pv = a[r * cols + c];
        for k in 0..cols {
            a[r * cols + k] /= pv;
        }
        for i in 0..rows {
            if i == r {
                continue;
            }
            let f = a[i * cols + c];
            if f != 0.0 {
                for k in 0..cols {
                    a[i * cols + k] -= f * a[r * cols + k];
                }
            }
        }
        pivots.push(c);
        r += 1;
    }
    for v in &mut a {
        if v.abs() <= tol {
            *v = 0.0;
        }
    }
    (a, pivots)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn m(rows: &[&[i64]]) -> Matrix {
        Matrix::from_rows(
            rows.iter()
                .map(|r| r.iter().map(|&v| Expr::int(v)).collect())
                .collect(),
        )
        .unwrap()
    }

    #[test]
    fn arithmetic() {
        let a = m(&[&[1, 2], &[3, 4]]);
        let b = m(&[&[0, 1], &[1, 0]]);
        assert_eq!(a.mul(&b).unwrap(), m(&[&[2, 1], &[4, 3]]));
        assert_eq!(a.add(&b).unwrap(), m(&[&[1, 3], &[4, 4]]));
        assert_eq!(a.transpose(), m(&[&[1, 3], &[2, 4]]));
        assert_eq!(a.trace().unwrap(), Expr::int(5));
        assert!(a.mul(&m(&[&[1, 2, 3]])).is_err());
        let u = m(&[&[1], &[0], &[0]]);
        let v = m(&[&[0], &[1], &[0]]);
        assert_eq!(u.cross(&v).unwrap(), m(&[&[0], &[0], &[1]]));
        assert_eq!(u.dot(&v).unwrap(), Expr::zero());
        assert_eq!(m(&[&[3], &[4]]).norm(), Expr::int(5));
    }

    #[test]
    fn determinants() {
        assert_eq!(m(&[&[1, 2], &[3, 4]]).det().unwrap(), Expr::int(-2));
        assert_eq!(
            m(&[&[2, 0, 1], &[1, 3, 2], &[1, 1, 2]]).det().unwrap(),
            Expr::int(6)
        );
        let (a, b, c, d) = (
            Expr::sym("a"),
            Expr::sym("b"),
            Expr::sym("c"),
            Expr::sym("d"),
        );
        let s = Matrix::from_rows(vec![vec![a.clone(), b.clone()], vec![c.clone(), d.clone()]])
            .unwrap();
        assert_eq!(s.det().unwrap(), a * d - b * c);
        let f = Matrix::from_rows(vec![
            vec![Expr::float(1.5), Expr::int(2)],
            vec![Expr::int(3), Expr::int(4)],
        ])
        .unwrap();
        assert_eq!(f.det().unwrap(), Expr::float(0.0));
    }

    #[test]
    fn inverses() {
        let a = m(&[&[2, 1], &[1, 1]]);
        assert_eq!(a.inverse().unwrap(), m(&[&[1, -1], &[-1, 2]]));
        assert!(m(&[&[1, 2], &[2, 4]]).inverse().is_err());
        let (a, b, c, d) = (
            Expr::sym("a"),
            Expr::sym("b"),
            Expr::sym("c"),
            Expr::sym("d"),
        );
        let s = Matrix::from_rows(vec![vec![a, b], vec![c, d]]).unwrap();
        let prod = s.mul(&s.inverse().unwrap()).unwrap();
        for (i, e) in prod.entries().iter().enumerate() {
            let want = if i % 3 == 0 { 1.0 } else { 0.0 };
            let env = [("a", 2.0), ("b", 3.0), ("c", 5.0), ("d", 7.0)];
            assert!((e.eval_f64(&env).unwrap() - want).abs() < 1e-12);
        }
    }

    #[test]
    fn row_reduction() {
        let a = m(&[&[1, 2, 3], &[2, 4, 6], &[1, 0, 1]]);
        let (r, p) = a.rref().unwrap();
        assert_eq!(p, vec![0, 1]);
        assert_eq!(r, m(&[&[1, 0, 1], &[0, 1, 1], &[0, 0, 0]]));
        assert_eq!(a.rank().unwrap(), 2);
        let ns = a.nullspace().unwrap();
        assert_eq!(ns, vec![m(&[&[-1], &[-1], &[1]])]);
        let x = Expr::sym("x");
        let s = Matrix::from_rows(vec![
            vec![x.clone(), Expr::one()],
            vec![Expr::one(), x.clone()],
        ])
        .unwrap();
        assert_eq!(s.rank().unwrap(), 2);
    }

    fn small_matrix(n: usize) -> impl Strategy<Value = Matrix> {
        prop::collection::vec(-5i64..=5, n * n)
            .prop_map(move |v| Matrix::new(n, n, v.into_iter().map(Expr::int).collect()).unwrap())
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]
        #[test]
        fn det_is_multiplicative(a in small_matrix(3), b in small_matrix(3)) {
            let lhs = a.mul(&b).unwrap().det().unwrap();
            let rhs = (a.det().unwrap() * b.det().unwrap()).simplify();
            prop_assert_eq!(lhs, rhs);
        }

        #[test]
        fn inverse_gives_identity(a in small_matrix(3)) {
            prop_assume!(!a.det().unwrap().is_zero());
            let p = a.mul(&a.inverse().unwrap()).unwrap();
            prop_assert_eq!(p, Matrix::identity(3).unwrap());
        }

        #[test]
        fn symbolic_and_rational_det_agree(a in small_matrix(4)) {
            let exact = a.det().unwrap();
            let mut memo = HashMap::new();
            let lap = normalize(&a.laplace(0, 0b1111, &mut memo));
            prop_assert_eq!(exact, lap);
        }
    }
}
