//! Evaluates a real expression tree at a given binary precision.

use std::cmp::Ordering;
use std::collections::HashMap;

use num_bigint::BigInt;
use num_integer::Integer;
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive};

use super::float::{asin, atan, atan2, exp, ln, pi, sin_cos, BigF, HpError, HpResult};
use super::special::{gamma, hurwitz};
use crate::expr::{Constant, Expr, ExprNode};
use crate::functions::Func;
use crate::number::Number;

/// One evaluation at precision `p`.
pub(super) struct Eval<'a> {
    vars: &'a HashMap<String, BigRational>,
    p: u64,
    /// An input was an approximate (floating-point) number.
    pub inexact: bool,
    /// The largest magnitude (in bits) of any term that was added or reduced: digits
    /// below `2^(scale − p)` may be cancellation noise.
    pub scale: i64,
}

fn unsupported<T>() -> HpResult<T> {
    Err(HpError::Unsupported)
}

impl<'a> Eval<'a> {
    pub fn new(vars: &'a HashMap<String, BigRational>, p: u64) -> Self {
        Eval {
            vars,
            p,
            inexact: false,
            scale: i64::MIN,
        }
    }

    fn note(&mut self, x: &BigF) {
        if !x.is_zero() {
            self.scale = self.scale.max(x.mag());
        }
    }

    pub fn ev(&mut self, e: &Expr) -> HpResult<BigF> {
        let p = self.p;
        match e.node() {
            ExprNode::Num(n) => match n {
                Number::Rational(q) => Ok(BigF::from_ratio(q, p)),
                Number::Float(x) => {
                    self.inexact = true;
                    BigF::from_f64(*x).ok_or(HpError::Range)
                }
                _ => unsupported(),
            },
            ExprNode::Sym(s) => match self.vars.get(&**s) {
                Some(q) => Ok(BigF::from_ratio(q, p)),
                None => unsupported(),
            },
            ExprNode::Const(c) => match c {
                Constant::Pi => Ok(pi(p)),
                Constant::E => exp(&BigF::one(), p),
                _ => unsupported(),
            },
            ExprNode::Add(ts) => {
                let mut acc = BigF::zero();
                for t in ts {
                    let v = self.ev(t)?;
                    self.note(&v);
                    acc = acc.add(&v, p);
                }
                Ok(acc)
            }
            ExprNode::Mul(fs) => {
                let mut acc = BigF::one();
                for f in fs {
                    acc = acc.mul(&self.ev(f)?, p);
                }
                Ok(acc)
            }
            ExprNode::Pow(b, x) => self.pow(b, x),
            ExprNode::Func(f, args) => self.func(*f, args),
            ExprNode::Apply(name, args) => match (&**name, args.as_slice()) {
                ("zeta", [s]) => {
                    let s = self.ev(s)?;
                    hurwitz(&s, &BigF::one(), p)
                }
                ("hurwitzzeta", [s, a]) => {
                    let (s, a) = (self.ev(s)?, self.ev(a)?);
                    hurwitz(&s, &a, p)
                }
                _ => unsupported(),
            },
            _ => unsupported(),
        }
    }

    /// Evaluates `e` accurately in absolute terms (as exp and the trigonometric
    /// functions need): a large value is evaluated again with more bits.
    fn ev_abs(&mut self, e: &Expr) -> HpResult<BigF> {
        let v = self.ev(e)?;
        if v.mag() <= 0 {
            return Ok(v);
        }
        let extra = v.mag() as u64 + 8;
        if extra > 1 << 22 {
            return Err(HpError::Range);
        }
        self.with_bits(extra, |s| s.ev(e))
    }

    /// Evaluates `e` with `extra` more bits (relative accuracy for a small result).
    fn with_bits(
        &mut self,
        extra: u64,
        f: impl FnOnce(&mut Self) -> HpResult<BigF>,
    ) -> HpResult<BigF> {
        let p = self.p;
        self.p = p + extra;
        let out = f(self);
        self.p = p;
        out
    }

    fn pow(&mut self, b: &Expr, x: &Expr) -> HpResult<BigF> {
        let p = self.p;
        if matches!(b.node(), ExprNode::Const(Constant::E)) {
            let xv = self.ev_abs(x)?;
            return exp(&xv, p);
        }
        if let Some(q) = x.as_number().and_then(Number::as_rational) {
            let bv = self.ev(b)?;
            if q.is_integer() {
                return bv.powi(q.numer(), p);
            }
            if q.denom() == &BigInt::from(2) {
                let r = bv.sqrt(p + 8)?;
                return r.powi(q.numer(), p);
            }
            if bv.is_negative() {
                if q.denom().is_odd() {
                    let mag = self.real_power(&bv.abs(), &BigF::from_ratio(q, p + 64))?;
                    return Ok(if q.numer().is_odd() { mag.neg() } else { mag });
                }
                return Err(HpError::Domain);
            }
            return self.real_power(&bv, &BigF::from_ratio(q, p + 64));
        }
        let bv = self.ev(b)?;
        let xv = self.ev_abs(x)?;
        self.real_power(&bv, &xv)
    }

    /// `b^x` for `b ≥ 0`.
    fn real_power(&mut self, b: &BigF, x: &BigF) -> HpResult<BigF> {
        let p = self.p;
        match b.sign() {
            Ordering::Less => Err(HpError::Domain),
            Ordering::Equal => {
                if x.sign() == Ordering::Greater {
                    Ok(BigF::zero())
                } else {
                    Err(HpError::Domain)
                }
            }
            Ordering::Greater => {
                let lnmag = 64 - b.mag().unsigned_abs().leading_zeros() as i64 + 1;
                let w = p + 16 + (x.mag() + lnmag).max(0) as u64;
                exp(&x.mul(&ln(b, w)?, w), p)
            }
        }
    }

    fn func(&mut self, f: Func, args: &[Expr]) -> HpResult<BigF> {
        let p = self.p;
        let w = p + 16;
        let one = BigF::one();
        let arg = |s: &mut Self, i: usize| -> HpResult<BigF> {
            args.get(i).map_or_else(unsupported, |a| s.ev(a))
        };
        match f {
            Func::Sin | Func::Cos | Func::Tan | Func::Sec | Func::Csc | Func::Cot => {
                let x = args.first().map_or_else(unsupported, |a| self.ev_abs(a))?;
                self.note(&x);
                let (s, c) = sin_cos(&x, w)?;
                match f {
                    Func::Sin => Ok(s.round(p)),
                    Func::Cos => Ok(c.round(p)),
                    Func::Tan => s.div(&c, p),
                    Func::Sec => c.recip(p),
                    Func::Csc => s.recip(p),
                    _ => c.div(&s, p),
                }
            }
            Func::Asin => asin(&arg(self, 0)?, p),
            Func::Acos => {
                let a = asin(&arg(self, 0)?, w)?;
                self.note(&a);
                Ok(pi(w).ldexp(-1).sub(&a, p))
            }
            Func::Atan => atan(&arg(self, 0)?, p),
            Func::Atan2 => {
                let (y, x) = (arg(self, 0)?, arg(self, 1)?);
                atan2(&y, &x, p)
            }
            Func::Sinh | Func::Cosh | Func::Tanh => {
                let x = args.first().map_or_else(unsupported, |a| self.ev_abs(a))?;
                if f == Func::Tanh && x.mag() > (p as i64 + 8).ilog2() as i64 + 2 {
                    // e^(−2|x|) is below the last bit.
                    return Ok(if x.is_negative() { one.neg() } else { one });
                }
                let extra = (-x.mag()).max(0) as u64;
                let wx = w + extra;
                let ex = exp(&x, wx)?;
                let emx = ex.recip(wx)?;
                self.note(&ex);
                match f {
                    Func::Sinh => Ok(ex.sub(&emx, p).ldexp(-1)),
                    Func::Cosh => Ok(ex.add(&emx, p).ldexp(-1)),
                    _ => ex.sub(&emx, wx).div(&ex.add(&emx, wx), p),
                }
            }
            Func::Asinh => {
                let x = arg(self, 0)?;
                let wx = w + (-x.mag()).max(0) as u64;
                let a = x.abs();
                let r = a.mul(&a, wx).add(&one, wx).sqrt(wx)?;
                let v = ln(&a.add(&r, wx), p)?;
                Ok(if x.is_negative() { v.neg() } else { v })
            }
            Func::Acosh => {
                let x = arg(self, 0)?;
                if x.cmp_value(&one) == Ordering::Less {
                    return Err(HpError::Domain);
                }
                let r = x.mul(&x, w).sub(&one, w).sqrt(w)?;
                ln(&x.add(&r, w), p)
            }
            Func::Atanh => {
                let x = arg(self, 0)?;
                if x.cmp_abs(&one) != Ordering::Less {
                    return Err(HpError::Domain);
                }
                let wx = w + (-x.mag()).max(0) as u64;
                let q = one.add(&x, wx).div(&one.sub(&x, wx), wx)?;
                Ok(ln(&q, p)?.ldexp(-1))
            }
            Func::Ln => {
                let x = arg(self, 0)?;
                ln(&x, p)
            }
            Func::Log => {
                let (x, b) = (arg(self, 0)?, arg(self, 1)?);
                let lb = ln(&b, w)?;
                if lb.is_zero() {
                    return Err(HpError::Domain);
                }
                ln(&x, w)?.div(&lb, p)
            }
            Func::Abs => Ok(arg(self, 0)?.abs()),
            Func::Sign => {
                let x = arg(self, 0)?;
                Ok(BigF::from_i64(match x.sign() {
                    Ordering::Less => -1,
                    Ordering::Equal => 0,
                    Ordering::Greater => 1,
                }))
            }
            Func::Floor | Func::Ceil => {
                let x = arg(self, 0)?;
                let fl = x.floor_int();
                // Too close to an integer to be sure which side it is on.
                let frac = x.sub(&BigF::from_int(fl.clone()), w);
                let near = |d: &BigF| !d.is_zero() && d.mag() < x.mag().max(1) - (p as i64) / 2;
                if near(&frac) || near(&BigF::one().sub(&frac, w)) {
                    return unsupported();
                }
                let v = if f == Func::Ceil && !frac.is_zero() {
                    fl + 1
                } else {
                    fl
                };
                Ok(BigF::from_int(v))
            }
            Func::Factorial => {
                let x = arg(self, 0)?;
                gamma(&x.add(&one, w + x.mag().max(0) as u64), p)
            }
            Func::Gamma => gamma(&arg(self, 0)?, p),
            Func::Min | Func::Max => {
                let mut best: Option<BigF> = None;
                for a in args {
                    let v = self.ev(a)?;
                    let better = best.as_ref().is_none_or(|b| {
                        let o = v.cmp_value(b);
                        if f == Func::Min {
                            o == Ordering::Less
                        } else {
                            o == Ordering::Greater
                        }
                    });
                    if better {
                        best = Some(v);
                    }
                }
                best.map_or_else(unsupported, Ok)
            }
            Func::Binomial => {
                let exact = |e: &Expr| e.as_number().and_then(Number::as_integer);
                match (args.first().and_then(exact), args.get(1).and_then(exact)) {
                    (Some(n), Some(k))
                        if n.is_positive() && k.to_u64().is_some_and(|k| k <= 100_000) =>
                    {
                        let k = k.to_u64().unwrap_or(0);
                        if BigInt::from(k) > n {
                            return Ok(BigF::zero());
                        }
                        let mut acc = BigInt::one();
                        for j in 0..k {
                            acc = acc * (&n - j) / (j + 1);
                        }
                        Ok(BigF::from_int(acc).round(p))
                    }
                    _ => unsupported(),
                }
            }
            _ => unsupported(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_elementary_trees() {
        let vars = HashMap::new();
        let mut ev = Eval::new(&vars, 200);
        let e = Expr::int(2).sqrt() * Expr::pi();
        let v = ev.ev(&e).unwrap().to_f64();
        assert!((v - 2f64.sqrt() * std::f64::consts::PI).abs() < 1e-15);
        assert!(!ev.inexact);
    }
}
