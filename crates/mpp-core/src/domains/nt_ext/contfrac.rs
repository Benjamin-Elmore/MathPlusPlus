//! Simple continued fractions `[a₀; a₁, a₂, …]`: exact for rationals (Euclid), exact and
//! periodic for quadratic irrationals `a + b√d` (the (P + √D)/Q recurrence, Lagrange's
//! theorem), numeric otherwise; convergents `p_k/q_k` and the value of a finite
//! expansion.

use std::collections::HashMap;

use num_bigint::BigInt;
use num_integer::Integer;
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};

use crate::domains::util::normalize;
use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode};

/// Most terms produced.
pub const MAX_TERMS: usize = 10_000;

/// The expansion of a quadratic irrational: `prefix` then `period` repeating forever.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Periodic {
    /// Terms before the repeating block.
    pub prefix: Vec<BigInt>,
    /// The repeating block.
    pub period: Vec<BigInt>,
}

/// The expansion of a rational number (finite, last term > 1 unless it is the only one).
pub fn of_rational(q: &BigRational) -> Vec<BigInt> {
    let (mut p, mut d) = (q.numer().clone(), q.denom().clone());
    let mut out = Vec::new();
    while !d.is_zero() && out.len() < MAX_TERMS {
        let (a, r) = p.div_mod_floor(&d);
        out.push(a);
        p = std::mem::replace(&mut d, r);
    }
    out
}

/// `a + b√d` with `d > 1` not a perfect square, read from an expression.
pub fn surd_parts(e: &Expr) -> Option<(BigRational, BigRational, BigInt)> {
    fn sqrt_of(e: &Expr) -> Option<BigInt> {
        if let ExprNode::Pow(base, exp) = e.node() {
            let half = BigRational::new(BigInt::one(), BigInt::from(2));
            if exp.as_number()?.as_rational()? == &half {
                let d = base.as_number()?.as_integer()?;
                if d > BigInt::one() {
                    return Some(d);
                }
            }
        }
        None
    }
    fn term(e: &Expr) -> Option<(BigRational, BigRational, Option<BigInt>)> {
        if let Some(q) = e.as_number().and_then(|n| n.as_rational()) {
            return Some((q.clone(), BigRational::zero(), None));
        }
        if let Some(d) = sqrt_of(e) {
            return Some((BigRational::zero(), BigRational::one(), Some(d)));
        }
        if let ExprNode::Mul(fs) = e.node() {
            if let [c, s] = fs.as_slice() {
                let c = c.as_number()?.as_rational()?.clone();
                return Some((BigRational::zero(), c, Some(sqrt_of(s)?)));
            }
        }
        None
    }
    let e = normalize(e);
    let parts: Vec<Expr> = match e.node() {
        ExprNode::Add(ts) => ts.clone(),
        _ => vec![e.clone()],
    };
    let (mut a, mut b, mut d) = (BigRational::zero(), BigRational::zero(), None::<BigInt>);
    for t in &parts {
        let (ta, tb, td) = term(t)?;
        a += ta;
        if let Some(td) = td {
            match &d {
                Some(d0) if *d0 != td => return None,
                _ => d = Some(td),
            }
            b += tb;
        }
    }
    let d = d?;
    if b.is_zero() {
        return None;
    }
    let s = d.sqrt();
    if &s * &s == d {
        return None;
    }
    Some((a, b, d))
}

/// The periodic expansion of `a + b√d` (`b ≠ 0`, `d` not a square).
pub fn of_surd(a: &BigRational, b: &BigRational, d: &BigInt) -> MathResult<Periodic> {
    // Write x = (P + √D)/Q with integers and Q | D − P².
    let q0 = a.denom().lcm(b.denom());
    let p0 = (a * BigRational::from_integer(q0.clone())).to_integer();
    let bb = (b * BigRational::from_integer(q0.clone())).to_integer();
    let (mut p, mut q) = if bb.is_negative() {
        (-p0, -q0)
    } else {
        (p0, q0)
    };
    let mut big_d = &bb * &bb * d;
    if !(&big_d - &p * &p).is_multiple_of(&q) {
        let qa = q.abs();
        p *= &qa;
        big_d *= &qa * &qa;
        q *= &qa;
    }
    let s = big_d.sqrt();
    let mut seen: HashMap<(BigInt, BigInt), usize> = HashMap::new();
    let mut terms = Vec::new();
    while terms.len() < MAX_TERMS {
        if let Some(&start) = seen.get(&(p.clone(), q.clone())) {
            let period = terms.split_off(start);
            return Ok(Periodic {
                prefix: terms,
                period,
            });
        }
        seen.insert((p.clone(), q.clone()), terms.len());
        let num = if q.is_positive() {
            &p + &s
        } else {
            &p + &s + 1
        };
        let a_k = num.div_floor(&q);
        p = &a_k * &q - &p;
        q = (&big_d - &p * &p) / &q;
        terms.push(a_k);
    }
    Err(MathError::TooLarge(
        "the period of this continued fraction".into(),
    ))
}

/// The first `n` terms of a real number's expansion, computed in floating point
/// (reliable only while the convergents stay below about 10⁸).
pub fn of_float(x: f64, n: usize) -> MathResult<Vec<BigInt>> {
    if !x.is_finite() {
        return Err(MathError::NotANumber(format!("{x}")));
    }
    let mut out = Vec::new();
    let mut y = x;
    for _ in 0..n.min(MAX_TERMS) {
        let a = y.floor();
        out.push(BigInt::from(
            a.to_i64()
                .ok_or_else(|| MathError::TooLarge(format!("{a}")))?,
        ));
        let f = y - a;
        if f.abs() < 1e-12 {
            break;
        }
        y = 1.0 / f;
        if y.abs() > 1e15 {
            break;
        }
    }
    Ok(out)
}

/// The first `n` terms of a periodic expansion.
pub fn take(per: &Periodic, n: usize) -> Vec<BigInt> {
    per.prefix
        .iter()
        .chain(per.period.iter().cycle())
        .take(n.min(MAX_TERMS))
        .cloned()
        .collect()
}

/// The convergents `p_k / q_k` of `[a₀; a₁, …]`.
pub fn convergents(terms: &[BigInt]) -> Vec<BigRational> {
    let (mut p0, mut p1) = (BigInt::one(), BigInt::zero());
    let (mut q0, mut q1) = (BigInt::zero(), BigInt::one());
    let mut out = Vec::with_capacity(terms.len());
    for a in terms {
        let p = a * &p0 + &p1;
        let q = a * &q0 + &q1;
        p1 = std::mem::replace(&mut p0, p.clone());
        q1 = std::mem::replace(&mut q0, q.clone());
        out.push(BigRational::new(p, q));
    }
    out
}

/// The value of a finite expansion; terms after the first must be positive.
pub fn value(terms: &[BigInt]) -> MathResult<BigRational> {
    if terms.is_empty() {
        return Err(MathError::Domain {
            function: "contfrac".into(),
            detail: "an empty continued fraction".into(),
        });
    }
    if terms.iter().skip(1).any(|a| !a.is_positive()) {
        return Err(MathError::Domain {
            function: "contfrac".into(),
            detail: "the terms after the first must be positive".into(),
        });
    }
    Ok(convergents(terms).pop().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn ints(v: &[i64]) -> Vec<BigInt> {
        v.iter().map(|&x| BigInt::from(x)).collect()
    }

    #[test]
    fn reference_expansions() {
        // 415/93 = [4; 2, 6, 7] (Hardy & Wright §10.1).
        let q = BigRational::new(415.into(), 93.into());
        assert_eq!(of_rational(&q), ints(&[4, 2, 6, 7]));
        assert_eq!(value(&ints(&[4, 2, 6, 7])).unwrap(), q);
        // √2 = [1; 2̅], √7 = [2; 1̅,̅1̅,̅1̅,̅4̅] (OEIS A010121), φ = [1; 1̅] (catalogue).
        let s2 = surd_parts(&Expr::int(2).sqrt()).unwrap();
        assert_eq!(
            of_surd(&s2.0, &s2.1, &s2.2).unwrap(),
            Periodic {
                prefix: ints(&[1]),
                period: ints(&[2])
            }
        );
        let s7 = surd_parts(&Expr::int(7).sqrt()).unwrap();
        assert_eq!(
            of_surd(&s7.0, &s7.1, &s7.2).unwrap().period,
            ints(&[1, 1, 1, 4])
        );
        let phi = (Expr::one() + Expr::int(5).sqrt()) / Expr::int(2);
        let p = surd_parts(&phi).unwrap();
        assert_eq!(
            of_surd(&p.0, &p.1, &p.2).unwrap(),
            Periodic {
                prefix: vec![],
                period: ints(&[1])
            }
        );
        // 1 − √2 = −0.414… = [−1; 1, 1̅,̅2̅]… check through the convergents instead.
        let neg = Expr::one() - Expr::int(2).sqrt();
        let n = surd_parts(&neg).unwrap();
        let per = of_surd(&n.0, &n.1, &n.2).unwrap();
        let c = convergents(&take(&per, 20));
        let approx = crate::number::ratio_to_f64(c.last().unwrap());
        assert!((approx - (1.0 - 2f64.sqrt())).abs() < 1e-14);
        // π = [3; 7, 15, 1, 292, …] (OEIS A001203), convergent 355/113.
        let pi_terms = of_float(std::f64::consts::PI, 5).unwrap();
        assert_eq!(pi_terms, ints(&[3, 7, 15, 1, 292]));
        assert_eq!(
            convergents(&pi_terms)[3],
            BigRational::new(355.into(), 113.into())
        );
        assert!(value(&ints(&[1, 0])).is_err());
    }

    proptest! {
        #[test]
        fn rational_round_trip(p in -100_000i64..100_000, q in 1i64..100_000) {
            let r = BigRational::new(p.into(), q.into());
            prop_assert_eq!(value(&of_rational(&r)).unwrap(), r);
        }

        #[test]
        fn convergent_error_bound(d in 2i64..500) {
            // |α − p_k/q_k| < 1/(q_k q_{k+1}) for irrational α = √d.
            let s = (d as f64).sqrt();
            prop_assume!(s.fract() != 0.0);
            let parts = surd_parts(&Expr::int(d).sqrt()).unwrap();
            let per = of_surd(&parts.0, &parts.1, &parts.2).unwrap();
            let c = convergents(&take(&per, 8));
            for k in 0..c.len() - 1 {
                let qk = crate::number::ratio_to_f64(&BigRational::from_integer(c[k].denom().clone()));
                let qk1 = crate::number::ratio_to_f64(&BigRational::from_integer(c[k + 1].denom().clone()));
                if qk * qk1 > 1e11 {
                    break; // beyond what f64 can resolve
                }
                let err = (s - crate::number::ratio_to_f64(&c[k])).abs();
                prop_assert!(err < 1.0 / (qk * qk1) + 1e-14);
            }
        }
    }
}
