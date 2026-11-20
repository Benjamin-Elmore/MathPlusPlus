//! Rational functions of the summation index over Q: reading them from an expression and
//! splitting them into partial fractions over their rational poles.

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};

use crate::expr::{Expr, ExprNode};
use crate::poly::Poly;

/// Largest degree handled.
const MAX_DEGREE: usize = 64;

/// `p/q` with rational coefficients, `q` monic and `gcd(p, q) = 1`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RatFun {
    pub num: Poly,
    pub den: Poly,
}

/// One pole `r` of multiplicity `m` with the coefficients of `1/(k − r)^j`, `j = 1 … m`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Pole {
    pub root: BigRational,
    pub coeffs: Vec<BigRational>,
}

/// A polynomial part plus poles: `poly(k) + Σ Σ c_j/(k − r)^j`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PartialFractions {
    pub poly: Poly,
    pub poles: Vec<Pole>,
}

fn rf(num: Poly, den: Poly) -> Option<RatFun> {
    if den.is_zero() {
        return None;
    }
    if num.degree().unwrap_or(0) > MAX_DEGREE || den.degree().unwrap_or(0) > MAX_DEGREE {
        return None;
    }
    Some(RatFun { num, den })
}

fn constant(c: BigRational) -> Poly {
    Poly::constant(c)
}

/// Reads `e` as a rational function of `var` with rational coefficients.
pub(crate) fn as_ratfun(e: &Expr, var: &str) -> Option<RatFun> {
    let raw = read(e, var)?;
    Some(reduce(raw))
}

fn read(e: &Expr, var: &str) -> Option<RatFun> {
    match e.node() {
        ExprNode::Num(n) => rf(constant(n.as_rational()?.clone()), Poly::constant(one())),
        ExprNode::Sym(s) if &**s == var => rf(Poly::x(), constant(one())),
        ExprNode::Add(ts) => {
            let mut acc = rf(Poly::zero(), constant(one()))?;
            for t in ts {
                let r = read(t, var)?;
                let num = acc.num.mul(&r.den).add(&r.num.mul(&acc.den));
                let den = acc.den.mul(&r.den);
                acc = reduce(rf(num, den)?);
            }
            Some(acc)
        }
        ExprNode::Mul(fs) => {
            let mut acc = rf(constant(one()), constant(one()))?;
            for f in fs {
                let r = read(f, var)?;
                acc = reduce(rf(acc.num.mul(&r.num), acc.den.mul(&r.den))?);
            }
            Some(acc)
        }
        ExprNode::Pow(b, x) => {
            let k = x.as_number()?.as_i64()?;
            if k.unsigned_abs() > MAX_DEGREE as u64 {
                return None;
            }
            let base = read(b, var)?;
            let mut num = constant(one());
            let mut den = constant(one());
            for _ in 0..k.unsigned_abs() {
                num = num.mul(&base.num);
                den = den.mul(&base.den);
            }
            if k < 0 {
                std::mem::swap(&mut num, &mut den);
            }
            rf(num, den)
        }
        _ => None,
    }
}

fn one() -> BigRational {
    BigRational::one()
}

/// Cancels the gcd and makes the denominator monic.
fn reduce(r: RatFun) -> RatFun {
    let g = r.num.gcd(&r.den);
    let (mut num, mut den) = (r.num, r.den);
    if g.degree().is_some_and(|d| d > 0) {
        if let (Ok((n, _)), Ok((d, _))) = (num.div_rem(&g), den.div_rem(&g)) {
            num = n;
            den = d;
        }
    }
    let lc = den.leading();
    if !lc.is_zero() && !lc.is_one() {
        let inv = lc.recip();
        num = num.scale(&inv);
        den = den.scale(&inv);
    }
    RatFun { num, den }
}

/// The Taylor coefficients of `p(r + t)`, lowest first.
fn shift_poly(p: &Poly, r: &BigRational) -> Vec<BigRational> {
    let mut out = Vec::new();
    let mut d = p.clone();
    let mut fact = BigRational::one();
    let mut i = 0i64;
    while !d.is_zero() {
        out.push(d.eval(r) / &fact);
        d = d.derivative();
        i += 1;
        fact *= BigRational::from_integer(BigInt::from(i));
    }
    out
}

/// Partial fractions of `f` when every pole is rational; `None` otherwise.
pub(crate) fn partial_fractions(f: &RatFun) -> Option<PartialFractions> {
    let (poly, rem) = f.num.div_rem(&f.den).ok()?;
    let mut poles = Vec::new();
    let mut accounted = 0usize;
    for (part, mult) in f.den.square_free() {
        let roots = part.rational_roots()?;
        let deg = part.degree().unwrap_or(0);
        if roots.len() != deg {
            return None;
        }
        let m = usize::try_from(mult).ok()?;
        for r in roots {
            accounted += m;
            // h = den / (k − r)^m
            let lin = Poly::new(vec![-r.clone(), one()]);
            let mut h = f.den.clone();
            for _ in 0..m {
                let (q, rr) = h.div_rem(&lin).ok()?;
                if !rr.is_zero() {
                    return None;
                }
                h = q;
            }
            // g(t) = rem(r + t) / h(r + t) as a series to order m.
            let pn = shift_poly(&rem, &r);
            let ph = shift_poly(&h, &r);
            let h0 = ph.first()?.clone();
            if h0.is_zero() {
                return None;
            }
            let mut g: Vec<BigRational> = Vec::with_capacity(m);
            for j in 0..m {
                let mut acc = pn.get(j).cloned().unwrap_or_else(BigRational::zero);
                for i in 1..=j {
                    if let Some(hi) = ph.get(i) {
                        acc -= hi * &g[j - i];
                    }
                }
                g.push(acc / &h0);
            }
            // c_j = g_{m−j}
            let coeffs = (1..=m).map(|j| g[m - j].clone()).collect();
            poles.push(Pole { root: r, coeffs });
        }
    }
    if accounted != f.den.degree().unwrap_or(0) {
        return None;
    }
    Some(PartialFractions { poly, poles })
}

/// True when `r` is a non-positive integer.
pub(crate) fn is_nonpositive_integer(r: &BigRational) -> bool {
    r.is_integer() && !r.is_positive()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k() -> Expr {
        Expr::sym("k")
    }
    fn q(p: i64, d: i64) -> BigRational {
        BigRational::new(p.into(), d.into())
    }

    #[test]
    fn simple_and_double_poles() {
        let e = (k() * (k() + Expr::one())).recip();
        let pf = as_ratfun(&e, "k")
            .as_ref()
            .and_then(partial_fractions)
            .expect("rational poles");
        assert!(pf.poly.is_zero());
        assert_eq!(pf.poles.len(), 2);
        let at_minus_one = pf.poles.iter().find(|p| p.root == q(-1, 1)).expect("pole");
        assert_eq!(at_minus_one.coeffs, vec![q(-1, 1)]);
        let e = (k().pow(2) * (k() + Expr::one())).recip();
        let pf = as_ratfun(&e, "k")
            .as_ref()
            .and_then(partial_fractions)
            .expect("rational poles");
        let at_zero = pf.poles.iter().find(|p| p.root.is_zero()).expect("pole");
        assert_eq!(at_zero.coeffs, vec![q(-1, 1), q(1, 1)]);
    }

    #[test]
    fn irreducible_denominators_are_refused() {
        let e = (k().pow(2) + Expr::one()).recip();
        let r = as_ratfun(&e, "k").expect("rational");
        assert!(partial_fractions(&r).is_none());
        assert!(as_ratfun(&Expr::sym("a").recip(), "k").is_none());
    }
}
