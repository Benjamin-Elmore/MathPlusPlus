//! Polynomials: coefficient extraction, dense polynomials over Q, division and gcd.

use num_bigint::BigInt;
use num_integer::Integer;
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};

use crate::error::{MathError, MathResult};
use crate::expand::expand;
use crate::expr::{Expr, ExprNode};
use crate::number::{self, Number};
use crate::simplify::{add, mul, pow};

/// A dense polynomial in one variable with rational coefficients, lowest power first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Poly {
    coeffs: Vec<BigRational>,
}

impl Poly {
    /// Builds a polynomial from coefficients, lowest power first (trailing zeros dropped).
    pub fn new(mut coeffs: Vec<BigRational>) -> Poly {
        while coeffs.last().is_some_and(Zero::is_zero) {
            coeffs.pop();
        }
        Poly { coeffs }
    }

    /// From integer coefficients, lowest power first.
    pub fn from_ints(coeffs: &[i64]) -> Poly {
        Poly::new(
            coeffs
                .iter()
                .map(|c| BigRational::from_integer(BigInt::from(*c)))
                .collect(),
        )
    }

    /// The zero polynomial.
    pub fn zero() -> Poly {
        Poly { coeffs: Vec::new() }
    }

    /// The constant `c`.
    pub fn constant(c: BigRational) -> Poly {
        Poly::new(vec![c])
    }

    /// `x`.
    pub fn x() -> Poly {
        Poly::from_ints(&[0, 1])
    }

    /// Coefficients, lowest power first.
    pub fn coeffs(&self) -> &[BigRational] {
        &self.coeffs
    }

    /// True for the zero polynomial.
    pub fn is_zero(&self) -> bool {
        self.coeffs.is_empty()
    }

    /// Degree; `None` for zero.
    pub fn degree(&self) -> Option<usize> {
        self.coeffs.len().checked_sub(1)
    }

    /// Leading coefficient (zero for the zero polynomial).
    pub fn leading(&self) -> BigRational {
        self.coeffs
            .last()
            .cloned()
            .unwrap_or_else(BigRational::zero)
    }

    /// Sum.
    pub fn add(&self, other: &Poly) -> Poly {
        let n = self.coeffs.len().max(other.coeffs.len());
        let zero = BigRational::zero();
        Poly::new(
            (0..n)
                .map(|i| self.coeffs.get(i).unwrap_or(&zero) + other.coeffs.get(i).unwrap_or(&zero))
                .collect(),
        )
    }

    /// Difference.
    pub fn sub(&self, other: &Poly) -> Poly {
        self.add(&other.scale(&-BigRational::one()))
    }

    /// Product.
    pub fn mul(&self, other: &Poly) -> Poly {
        if self.is_zero() || other.is_zero() {
            return Poly::zero();
        }
        let mut out = vec![BigRational::zero(); self.coeffs.len() + other.coeffs.len() - 1];
        for (i, a) in self.coeffs.iter().enumerate() {
            for (j, b) in other.coeffs.iter().enumerate() {
                out[i + j] += a * b;
            }
        }
        Poly::new(out)
    }

    /// Multiplies every coefficient by `c`.
    pub fn scale(&self, c: &BigRational) -> Poly {
        Poly::new(self.coeffs.iter().map(|a| a * c).collect())
    }

    /// Quotient and remainder; errors when dividing by zero.
    pub fn div_rem(&self, divisor: &Poly) -> MathResult<(Poly, Poly)> {
        let d = divisor.degree().ok_or(MathError::DivisionByZero)?;
        let lead = divisor.leading();
        let mut rem = self.coeffs.clone();
        let Some(n) = self.degree() else {
            return Ok((Poly::zero(), Poly::zero()));
        };
        if n < d {
            return Ok((Poly::zero(), self.clone()));
        }
        let mut quot = vec![BigRational::zero(); n - d + 1];
        for k in (0..=n - d).rev() {
            let c = &rem[k + d] / &lead;
            if !c.is_zero() {
                for (j, b) in divisor.coeffs.iter().enumerate() {
                    rem[k + j] -= &c * b;
                }
            }
            quot[k] = c;
        }
        rem.truncate(d);
        Ok((Poly::new(quot), Poly::new(rem)))
    }

    /// Monic greatest common divisor (zero when both are zero).
    pub fn gcd(&self, other: &Poly) -> Poly {
        let (mut a, mut b) = (self.clone(), other.clone());
        while !b.is_zero() {
            let r = match a.div_rem(&b) {
                Ok((_, r)) => r,
                Err(_) => break,
            };
            a = b;
            b = r;
        }
        a.monic()
    }

    /// Divides by the leading coefficient.
    pub fn monic(&self) -> Poly {
        if self.is_zero() {
            return self.clone();
        }
        let lead = self.leading();
        self.scale(&lead.recip())
    }

    /// Derivative.
    pub fn derivative(&self) -> Poly {
        Poly::new(
            self.coeffs
                .iter()
                .enumerate()
                .skip(1)
                .map(|(i, c)| c * BigRational::from_integer(BigInt::from(i)))
                .collect(),
        )
    }

    /// Value at `x` (Horner).
    pub fn eval(&self, x: &BigRational) -> BigRational {
        self.coeffs
            .iter()
            .rev()
            .fold(BigRational::zero(), |acc, c| acc * x + c)
    }

    /// Splits off the rational content: `self = content · primitive`, where `primitive` has
    /// coprime integer coefficients and a positive leading coefficient.
    pub fn content_primitive(&self) -> (BigRational, Vec<BigInt>) {
        if self.is_zero() {
            return (BigRational::zero(), Vec::new());
        }
        let den = self
            .coeffs
            .iter()
            .fold(BigInt::one(), |acc, c| acc.lcm(c.denom()));
        let ints: Vec<BigInt> = self
            .coeffs
            .iter()
            .map(|c| (c * BigRational::from_integer(den.clone())).to_integer())
            .collect();
        let mut g = ints.iter().fold(BigInt::zero(), |acc, c| acc.gcd(c));
        if ints.last().is_some_and(Signed::is_negative) {
            g = -g;
        }
        let prim = ints.iter().map(|c| c / &g).collect();
        (BigRational::new(g, den), prim)
    }

    /// Square-free decomposition (Yun): `self = c · Π fᵢ^i` with monic, square-free `fᵢ`.
    pub fn square_free(&self) -> Vec<(Poly, u32)> {
        let mut out = Vec::new();
        if self.degree().is_none_or(|d| d == 0) {
            return out;
        }
        let f = self.monic();
        let df = f.derivative();
        let mut c = f.gcd(&df);
        let mut w = match f.div_rem(&c) {
            Ok((q, _)) => q,
            Err(_) => return vec![(f, 1)],
        };
        let mut i = 1u32;
        while c.degree().is_some_and(|d| d > 0) {
            let y = w.gcd(&c);
            let z = w
                .div_rem(&y)
                .map(|(q, _)| q)
                .unwrap_or_else(|_| Poly::zero());
            if z.degree().is_some_and(|d| d > 0) {
                out.push((z, i));
            }
            i += 1;
            c = c
                .div_rem(&y)
                .map(|(q, _)| q)
                .unwrap_or_else(|_| Poly::zero());
            w = y;
        }
        if w.degree().is_some_and(|d| d > 0) {
            out.push((w, i));
        }
        out
    }

    /// Rational roots (each once), by the rational root theorem; `None` if the
    /// coefficients are too large to search.
    pub fn rational_roots(&self) -> Option<Vec<BigRational>> {
        let (_, prim) = self.content_primitive();
        if prim.len() < 2 {
            return Some(Vec::new());
        }
        let mut roots = Vec::new();
        // Strip factors of x first.
        let zeros = prim.iter().take_while(|c| c.is_zero()).count();
        if zeros > 0 {
            roots.push(BigRational::zero());
        }
        let rest = &prim[zeros..];
        if rest.len() < 2 {
            return Some(roots);
        }
        let a0 = &rest[0];
        let an = &rest[rest.len() - 1];
        let ps = number::divisors(a0)?;
        let qs = number::divisors(an)?;
        if ps.len().saturating_mul(qs.len()) > 200_000 {
            return None;
        }
        let poly = Poly::new(
            rest.iter()
                .map(|c| BigRational::from_integer(c.clone()))
                .collect(),
        );
        let mut seen = std::collections::BTreeSet::new();
        for p in &ps {
            for q in &qs {
                for sign in [1, -1] {
                    let r = BigRational::new(p * sign, q.clone());
                    if seen.insert(r.clone()) && poly.eval(&r).is_zero() {
                        roots.push(r);
                    }
                }
            }
        }
        roots.sort();
        Some(roots)
    }

    /// Converts a polynomial expression in `var` with rational coefficients.
    pub fn from_expr(e: &Expr, var: &str) -> Option<Poly> {
        let coeffs = as_poly(e, var)?;
        let mut out = Vec::with_capacity(coeffs.len());
        for c in coeffs {
            out.push(c.as_number()?.as_rational()?.clone());
        }
        Some(Poly::new(out))
    }

    /// The expression `Σ cₖ var^k`.
    pub fn to_expr(&self, var: &str) -> Expr {
        let x = Expr::sym(var);
        add(self
            .coeffs
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.is_zero())
            .map(|(k, c)| {
                mul(vec![
                    Expr::ratio(c.clone()),
                    pow(x.clone(), Expr::int(k as i64)),
                ])
            })
            .collect())
    }
}

/// Coefficients of `e` as a polynomial in `var`, lowest power first; `None` when `e` is
/// not a polynomial in `var`. The zero polynomial gives an empty list.
pub fn as_poly(e: &Expr, var: &str) -> Option<Vec<Expr>> {
    if !is_polynomial_in(e, var) {
        return None;
    }
    let expanded = expand(e).ok()?;
    let terms = match expanded.node() {
        ExprNode::Add(v) => v.clone(),
        _ => vec![expanded.clone()],
    };
    let mut buckets: Vec<Vec<Expr>> = Vec::new();
    for t in terms {
        let factors = match t.node() {
            ExprNode::Mul(fs) => fs.clone(),
            _ => vec![t.clone()],
        };
        let mut degree = 0usize;
        let mut rest = Vec::new();
        for f in factors {
            if f.is_sym(var) {
                degree += 1;
                continue;
            }
            if let ExprNode::Pow(b, x) = f.node() {
                if b.is_sym(var) {
                    let k = x.as_number()?.as_i64()?;
                    degree += usize::try_from(k).ok()?;
                    continue;
                }
            }
            if f.contains_sym(var) {
                return None;
            }
            rest.push(f);
        }
        if buckets.len() <= degree {
            buckets.resize(degree + 1, Vec::new());
        }
        buckets[degree].push(mul(rest));
    }
    let mut coeffs: Vec<Expr> = buckets.into_iter().map(add).collect();
    while coeffs.last().is_some_and(Expr::is_zero) {
        coeffs.pop();
    }
    Some(coeffs)
}

/// True when `e` is a polynomial in `var` (other symbols count as constants).
pub fn is_polynomial_in(e: &Expr, var: &str) -> bool {
    match e.node() {
        ExprNode::Sym(_) => true,
        ExprNode::Add(v) | ExprNode::Mul(v) => v.iter().all(|t| is_polynomial_in(t, var)),
        ExprNode::Pow(b, x) => {
            if !b.contains_sym(var) {
                return !x.contains_sym(var);
            }
            let ok_exp = x
                .as_number()
                .and_then(Number::as_i64)
                .is_some_and(|k| k >= 0);
            ok_exp && is_polynomial_in(b, var)
        }
        ExprNode::Relation(..)
        | ExprNode::Matrix { .. }
        | ExprNode::Set(_)
        | ExprNode::Tuple(_)
        | ExprNode::Interval { .. }
        | ExprNode::Logic(..)
        | ExprNode::Bool(_) => false,
        _ => !e.contains_sym(var),
    }
}

/// Degree of `e` in `var`, if it is a polynomial (zero has no degree).
pub fn degree(e: &Expr, var: &str) -> Option<usize> {
    as_poly(e, var)?.len().checked_sub(1)
}

fn rational_poly(e: &Expr, var: &str) -> MathResult<Poly> {
    Poly::from_expr(e, var).ok_or_else(|| MathError::NotPolynomial { var: var.into() })
}

/// Quotient and remainder of polynomial division over Q.
pub fn poly_div(a: &Expr, b: &Expr, var: &str) -> MathResult<(Expr, Expr)> {
    let (q, r) = rational_poly(a, var)?.div_rem(&rational_poly(b, var)?)?;
    Ok((q.to_expr(var), r.to_expr(var)))
}

/// Monic greatest common divisor over Q.
pub fn poly_gcd(a: &Expr, b: &Expr, var: &str) -> MathResult<Expr> {
    Ok(rational_poly(a, var)?
        .gcd(&rational_poly(b, var)?)
        .to_expr(var))
}

impl Expr {
    /// See [`as_poly`].
    pub fn as_poly(&self, var: &str) -> Option<Vec<Expr>> {
        as_poly(self, var)
    }

    /// See [`is_polynomial_in`].
    pub fn is_polynomial_in(&self, var: &str) -> bool {
        is_polynomial_in(self, var)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x() -> Expr {
        Expr::sym("x")
    }
    fn n(k: i64) -> Expr {
        Expr::int(k)
    }
    fn q(p: i64, d: i64) -> BigRational {
        BigRational::new(p.into(), d.into())
    }

    #[test]
    fn coefficients() {
        let a = Expr::sym("a");
        let e = (x() + a.clone()).pow(2);
        let c = e.as_poly("x").unwrap();
        assert_eq!(c, vec![a.pow(2), n(2) * a.clone(), n(1)]);
        assert_eq!(n(0).as_poly("x").unwrap(), Vec::<Expr>::new());
        assert!(x().sqrt().as_poly("x").is_none());
        assert!(Expr::call("sin", vec![x()]).unwrap().as_poly("x").is_none());
        assert!(Expr::call("sin", vec![a]).unwrap().is_polynomial_in("x"));
        assert!(!x().recip().is_polynomial_in("x"));
        assert_eq!(degree(&(x().pow(3) + x()), "x"), Some(3));
    }

    #[test]
    fn division_and_gcd() {
        let a = Poly::from_ints(&[-1, 0, 1]); // x^2 - 1
        let b = Poly::from_ints(&[1, 1]); // x + 1
        let (q, r) = a.div_rem(&b).unwrap();
        assert_eq!(q, Poly::from_ints(&[-1, 1]));
        assert!(r.is_zero());
        let (q, r) = Poly::from_ints(&[1, 0, 1]).div_rem(&b).unwrap();
        assert_eq!(q, Poly::from_ints(&[-1, 1]));
        assert_eq!(r, Poly::from_ints(&[2]));
        assert!(a.div_rem(&Poly::zero()).is_err());
        let g = a.gcd(&Poly::from_ints(&[1, 2, 1]));
        assert_eq!(g, b);
        let (q, r) = poly_div(&(x().pow(3) - n(1)), &(x() - n(1)), "x").unwrap();
        assert_eq!(q, x().pow(2) + x() + n(1));
        assert_eq!(r, n(0));
        let g = poly_gcd(&(x().pow(2) - n(1)), &(n(2) * x() - n(2)), "x").unwrap();
        assert_eq!(g, x() - n(1));
        assert!(poly_div(&x().sqrt(), &x(), "x").is_err());
    }

    #[test]
    fn square_free_and_roots() {
        // (x - 1)^2 (x + 2)
        let p = Poly::from_ints(&[2, -3, 0, 1]);
        let sf = p.square_free();
        assert_eq!(
            sf,
            vec![
                (Poly::from_ints(&[2, 1]), 1),
                (Poly::from_ints(&[-1, 1]), 2)
            ]
        );
        assert_eq!(p.rational_roots().unwrap(), vec![q(-2, 1), q(1, 1)]);
        let p = Poly::from_ints(&[-1, 0, 4]); // 4x^2 - 1
        assert_eq!(p.rational_roots().unwrap(), vec![q(-1, 2), q(1, 2)]);
        let (c, prim) = Poly::new(vec![q(1, 2), q(-3, 2)]).content_primitive();
        assert_eq!(c, q(-1, 2));
        assert_eq!(prim, vec![BigInt::from(-1), BigInt::from(3)]);
    }

    #[test]
    fn conversions() {
        let e = n(3) * x().pow(2) - x() + Expr::rational(1, 2);
        let p = Poly::from_expr(&e, "x").unwrap();
        assert_eq!(p.coeffs(), &[q(1, 2), q(-1, 1), q(3, 1)]);
        assert_eq!(p.to_expr("x"), e);
        assert_eq!(p.derivative().to_expr("x"), n(6) * x() - n(1));
        assert_eq!(p.eval(&q(1, 1)), q(5, 2));
    }
}
