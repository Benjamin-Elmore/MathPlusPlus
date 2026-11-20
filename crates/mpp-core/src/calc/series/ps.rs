//! Truncated power series `Σ cᵢ t^(val + i)` with exact (symbolic) coefficients, known for
//! exponents below `prec`, and the arithmetic on them: sums, products, reciprocals, powers,
//! composition with elementary functions, term-wise derivative and integral.

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::One;

use crate::error::{MathError, MathResult};
use crate::expr::Expr;
use crate::functions::{func, Func};
use crate::number::binomial;
use crate::simplify::{add, mul, pow, simplify};

/// A truncated (Laurent) power series in `t`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Ps {
    /// Exponent of the first stored coefficient.
    pub val: i64,
    /// Coefficients of `t^val, t^(val+1), …, t^(prec−1)`.
    pub c: Vec<Expr>,
    /// Exponents from `prec` on are unknown (`O(t^prec)`).
    pub prec: i64,
}

fn unsupported(what: &str) -> MathError {
    MathError::Unsupported(format!("a series expansion of {what}"))
}

/// True for coefficients that are exactly zero (or a constant that is numerically zero).
pub(crate) fn is_zero_coeff(e: &Expr) -> bool {
    e.is_zero()
        || (e.is_constant()
            && e.eval_f64(&crate::eval::EmptyEnv)
                .is_ok_and(|v| v.abs() < 1e-13))
}

impl Ps {
    /// Builds a series from coefficients starting at `val`, keeping exponents below `prec`.
    pub fn new(val: i64, c: Vec<Expr>, prec: i64) -> Ps {
        let mut s = Ps { val, c, prec };
        s.normalize();
        s
    }

    /// `O(t^prec)` only.
    pub fn zero(prec: i64) -> Ps {
        Ps {
            val: prec,
            c: Vec::new(),
            prec,
        }
    }

    /// A constant, known to order `n`.
    pub fn constant(e: Expr, n: i64) -> Ps {
        Ps::new(0, vec![e], n)
    }

    /// Coefficient of `t^e` (zero outside the stored range).
    pub fn get(&self, e: i64) -> Expr {
        if e < self.val || e >= self.prec {
            return Expr::zero();
        }
        usize::try_from(e - self.val)
            .ok()
            .and_then(|i| self.c.get(i).cloned())
            .unwrap_or_else(Expr::zero)
    }

    /// Strips leading zeros, pads or truncates to `prec`.
    fn normalize(&mut self) {
        let len = usize::try_from((self.prec - self.val).max(0)).unwrap_or(0);
        self.c.truncate(len);
        while self.c.len() < len {
            self.c.push(Expr::zero());
        }
        let lead = self.c.iter().take_while(|e| is_zero_coeff(e)).count();
        if lead > 0 {
            self.c.drain(..lead);
            self.val += lead as i64;
        }
        if self.c.is_empty() {
            self.val = self.prec;
        }
    }

    /// True when every known coefficient is zero.
    pub fn is_zero(&self) -> bool {
        self.c.is_empty()
    }

    /// The leading coefficient (after normalisation).
    pub fn lead(&self) -> Option<&Expr> {
        self.c.first()
    }

    /// Sum.
    pub fn add(&self, o: &Ps) -> Ps {
        let prec = self.prec.min(o.prec);
        let val = self.val.min(o.val).min(prec);
        let c = (val..prec)
            .map(|e| simplify(&add(vec![self.get(e), o.get(e)])))
            .collect();
        Ps::new(val, c, prec)
    }

    /// `c · self`.
    pub fn scale(&self, k: &Expr) -> Ps {
        let c = self
            .c
            .iter()
            .map(|e| simplify(&mul(vec![k.clone(), e.clone()])))
            .collect();
        Ps::new(self.val, c, self.prec)
    }

    /// Negation.
    pub fn neg(&self) -> Ps {
        self.scale(&Expr::int(-1))
    }

    /// Difference.
    pub fn sub(&self, o: &Ps) -> Ps {
        self.add(&o.neg())
    }

    /// Product.
    pub fn mul(&self, o: &Ps) -> Ps {
        if self.is_zero() || o.is_zero() {
            return Ps::zero((self.prec + o.val).min(o.prec + self.val));
        }
        let val = self.val + o.val;
        let prec = (self.prec + o.val).min(o.prec + self.val);
        let mut c = Vec::new();
        for e in val..prec {
            let mut terms = Vec::new();
            for (i, a) in self.c.iter().enumerate() {
                let ea = self.val + i as i64;
                let eb = e - ea;
                if eb < o.val {
                    break;
                }
                let b = o.get(eb);
                if !b.is_zero() && !a.is_zero() {
                    terms.push(mul(vec![a.clone(), b]));
                }
            }
            c.push(simplify(&add(terms)));
        }
        Ps::new(val, c, prec)
    }

    /// Reciprocal (a Laurent series when the leading exponent is positive).
    pub fn recip(&self) -> MathResult<Ps> {
        let Some(c0) = self.lead().cloned() else {
            return Err(MathError::Unsupported(
                "a series expansion of 1/f where f has no non-zero term to this order".into(),
            ));
        };
        let rel = self.prec - self.val;
        let inv0 = pow(c0, Expr::int(-1));
        let mut b: Vec<Expr> = Vec::with_capacity(usize::try_from(rel).unwrap_or(0));
        for m in 0..rel {
            if m == 0 {
                b.push(simplify(&inv0));
                continue;
            }
            let mut terms = Vec::new();
            for i in 1..=m {
                let ci = self.get(self.val + i);
                if ci.is_zero() {
                    continue;
                }
                let bi = &b[usize::try_from(m - i).unwrap_or(0)];
                terms.push(mul(vec![ci, bi.clone()]));
            }
            b.push(simplify(&mul(vec![
                Expr::int(-1),
                inv0.clone(),
                add(terms),
            ])));
        }
        Ok(Ps::new(-self.val, b, -self.val + rel))
    }

    /// Quotient.
    pub fn div(&self, o: &Ps) -> MathResult<Ps> {
        Ok(self.mul(&o.recip()?))
    }

    /// Integer power.
    pub fn powi(&self, n: i64) -> MathResult<Ps> {
        if n < 0 {
            return self.recip()?.powi(-n);
        }
        let mut result = Ps::constant(Expr::one(), self.prec.max(1) + self.val * (n - 1).max(0));
        let mut base = self.clone();
        let mut e = n;
        let mut first = true;
        while e > 0 {
            if e & 1 == 1 {
                result = if first {
                    base.clone()
                } else {
                    result.mul(&base)
                };
                first = false;
            }
            e >>= 1;
            if e > 0 {
                base = base.mul(&base);
            }
        }
        if first {
            // n = 0
            return Ok(Ps::constant(Expr::one(), self.prec - self.val));
        }
        Ok(result)
    }

    /// Splits into the constant term and the rest (`val ≥ 0` required).
    pub fn split_constant(&self) -> MathResult<(Expr, Ps)> {
        if self.val < 0 {
            return Err(MathError::Unsupported(
                "a series expansion around a pole of the inner function".into(),
            ));
        }
        let a0 = self.get(0);
        let rest = self.sub(&Ps::constant(a0.clone(), self.prec));
        Ok((a0, rest))
    }

    /// `Σ_j f_j h^j` for `h` with positive valuation.
    fn compose(h: &Ps, n: i64, coeff: &dyn Fn(i64) -> Expr) -> Ps {
        let mut acc = Ps::constant(coeff(0), n.min(h.prec.max(1)));
        if h.is_zero() {
            return Ps::constant(coeff(0), h.prec);
        }
        let mut power = Ps::constant(Expr::one(), h.prec);
        let mut j = 1;
        while j * h.val < acc.prec {
            power = power.mul(h);
            let cj = coeff(j);
            if !cj.is_zero() {
                acc = acc.add(&power.scale(&cj));
            }
            j += 1;
        }
        // The first omitted term is O(t^(j·val)).
        acc.prec = acc.prec.min(h.prec.max(h.val * j));
        acc.normalize();
        acc
    }

    /// `exp(self)`.
    pub fn exp(&self) -> MathResult<Ps> {
        let (a0, h) = self.split_constant()?;
        let n = self.prec;
        let series = Ps::compose(&h, n, &|j| Expr::ratio(inv_factorial(j)));
        Ok(series.scale(&pow(Expr::e(), a0)))
    }

    /// `sin`, `cos`, `sinh` or `cosh` of `self`.
    pub fn trig(&self, f: Func) -> MathResult<Ps> {
        let (a0, h) = self.split_constant()?;
        let n = self.prec;
        let hyper = matches!(f, Func::Sinh | Func::Cosh);
        let sign = |j: i64| -> BigRational {
            if hyper || j % 4 < 2 {
                BigRational::one()
            } else {
                -BigRational::one()
            }
        };
        let even = Ps::compose(&h, n, &|j| {
            if j % 2 == 0 {
                Expr::ratio(sign(j) * inv_factorial(j))
            } else {
                Expr::zero()
            }
        });
        let odd = Ps::compose(&h, n, &|j| {
            if j % 2 == 1 {
                Expr::ratio(sign(j - 1) * inv_factorial(j))
            } else {
                Expr::zero()
            }
        });
        let (s0, c0) = match f {
            Func::Sin | Func::Cos => (func(Func::Sin, vec![a0.clone()]), func(Func::Cos, vec![a0])),
            _ => (
                func(Func::Sinh, vec![a0.clone()]),
                func(Func::Cosh, vec![a0]),
            ),
        };
        Ok(match f {
            // sin(a + h) = sin a cos h + cos a sin h
            Func::Sin | Func::Sinh => even.scale(&s0).add(&odd.scale(&c0)),
            // cos(a + h) = cos a cos h − sin a sin h; cosh(a + h) = cosh a cosh h + sinh a sinh h
            Func::Cos => even.scale(&c0).sub(&odd.scale(&s0)),
            _ => even.scale(&c0).add(&odd.scale(&s0)),
        })
    }

    /// `ln(self)` (the constant term must be non-zero).
    pub fn ln(&self) -> MathResult<Ps> {
        if self.val != 0 {
            return Err(unsupported("ln at a zero or pole of its argument"));
        }
        let (a0, h) = self.split_constant()?;
        let u = h.scale(&pow(a0.clone(), Expr::int(-1)));
        let series = Ps::compose(&u, self.prec, &|j| {
            if j == 0 {
                Expr::zero()
            } else {
                let s = if j % 2 == 1 { 1 } else { -1 };
                Expr::rational(s, j)
            }
        });
        Ok(series.add(&Ps::constant(func(Func::Ln, vec![a0]), series.prec)))
    }

    /// `self^α` for a constant exponent.
    pub fn pow_const(&self, alpha: &Expr) -> MathResult<Ps> {
        if let Some(n) = alpha.as_number().and_then(|n| n.as_i64()) {
            if n.unsigned_abs() <= 64 {
                return self.powi(n);
            }
        }
        let Some(c0) = self.lead().cloned() else {
            return Err(unsupported("a power of a function with no non-zero term"));
        };
        // self = c0 t^v (1 + u)
        let v = self.val;
        let shifted_exp = simplify(&mul(vec![Expr::int(v), alpha.clone()]));
        let Some(new_val) = shifted_exp.as_number().and_then(|n| n.as_i64()) else {
            return Err(unsupported(
                "a fractional power at a zero (a Puiseux series)",
            ));
        };
        let norm = Ps::new(
            0,
            self.c
                .iter()
                .map(|e| simplify(&mul(vec![e.clone(), pow(c0.clone(), Expr::int(-1))])))
                .collect(),
            self.prec - v,
        );
        let u = norm.sub(&Ps::constant(Expr::one(), norm.prec));
        let alpha_q = alpha.as_number().and_then(|n| n.as_rational()).cloned();
        let series = Ps::compose(&u, norm.prec, &|j| match &alpha_q {
            Some(a) => {
                binomial(a, &BigInt::from(j)).map_or_else(|_| Expr::undefined(), Expr::ratio)
            }
            None => symbolic_binomial(alpha, j),
        });
        let scaled = series.scale(&pow(c0, alpha.clone()));
        Ok(Ps::new(
            scaled.val + new_val,
            scaled.c,
            scaled.prec + new_val,
        ))
    }

    /// Term-wise derivative in `t`.
    pub fn derivative(&self) -> Ps {
        let c = (self.val..self.prec)
            .map(|e| simplify(&mul(vec![Expr::int(e), self.get(e)])))
            .collect::<Vec<_>>();
        Ps::new(self.val - 1, c, self.prec - 1)
    }

    /// Term-wise antiderivative in `t` with constant term `c0`.
    pub fn integral(&self, c0: Expr) -> MathResult<Ps> {
        if self.val < 0 {
            return Err(unsupported("an integral with a negative power"));
        }
        let mut c = vec![c0];
        for e in 0..self.prec {
            c.push(simplify(&mul(vec![Expr::rational(1, e + 1), self.get(e)])));
        }
        Ok(Ps::new(0, c, self.prec + 1))
    }

    /// The expression `Σ cᵢ base^(val + i)`.
    pub fn to_expr(&self, base: &Expr) -> Expr {
        add(self
            .c
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.is_zero())
            .map(|(i, c)| {
                mul(vec![
                    c.clone(),
                    pow(base.clone(), Expr::int(self.val + i as i64)),
                ])
            })
            .collect())
    }
}

fn inv_factorial(j: i64) -> BigRational {
    let mut f = BigInt::one();
    for i in 2..=j {
        f *= BigInt::from(i);
    }
    BigRational::new(BigInt::one(), f)
}

/// `C(α, j)` for a symbolic `α`.
fn symbolic_binomial(alpha: &Expr, j: i64) -> Expr {
    let mut factors = vec![Expr::ratio(inv_factorial(j))];
    for i in 0..j {
        factors.push(add(vec![alpha.clone(), Expr::int(-i)]));
    }
    simplify(&mul(factors))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(n: i64) -> Ps {
        Ps::new(1, vec![Expr::one()], n)
    }

    #[test]
    fn arithmetic() {
        let one_plus_t = Ps::constant(Expr::one(), 6).add(&t(6));
        let inv = one_plus_t.recip().unwrap_or_else(|_| Ps::zero(0));
        // 1/(1 + t) = 1 − t + t² − …
        assert_eq!(inv.get(3), Expr::int(-1));
        assert_eq!(inv.prec, 6);
        let sq = one_plus_t.powi(2).unwrap_or_else(|_| Ps::zero(0));
        assert_eq!(sq.get(1), Expr::int(2));
        let laurent = t(6).recip().unwrap_or_else(|_| Ps::zero(0));
        assert_eq!(laurent.val, -1);
        assert!(Ps::zero(4).recip().is_err());
    }

    #[test]
    fn elementary_functions() {
        let e = t(6).exp().unwrap_or_else(|_| Ps::zero(0));
        assert_eq!(e.get(3), Expr::rational(1, 6));
        let s = t(8).trig(Func::Sin).unwrap_or_else(|_| Ps::zero(0));
        assert_eq!(s.get(5), Expr::rational(1, 120));
        let c = t(8).trig(Func::Cos).unwrap_or_else(|_| Ps::zero(0));
        assert_eq!(c.get(4), Expr::rational(1, 24));
        let l = Ps::constant(Expr::one(), 6)
            .add(&t(6))
            .ln()
            .unwrap_or_else(|_| Ps::zero(0));
        assert_eq!(l.get(4), Expr::rational(-1, 4));
    }
}
