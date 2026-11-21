//! `Σ_{k≥a} c (αk + β)^(−s)` for a rational `s > 1`: a Hurwitz ζ value,
//! `c α^(−s) ζ(s, a + β/α)` (plain `ζ(s)` when `a + β/α = 1`). Summing such a series
//! term by term converges far too slowly to give its digits (`Σ 1/n^1.0001 = 10000.58`).
//! An even whole `s` has a closed form in powers of π found before this; an odd one
//! (`Σ 1/n³ = ζ(3)`) is written with ζ, the shift taken out when it is a whole or
//! half-whole number (`Σ_{n≥2} 1/n³ = ζ(3) − 1`).

use num_rational::BigRational;
use num_traits::{One, Signed};

use super::{int_value, split_constant};
use crate::expr::{Expr, ExprNode};
use crate::number::Number;
use crate::poly::as_poly;
use crate::simplify::simplify;

/// `e` as `base^(−s)` with an exact rational `s` (powers of powers multiplied out, which
/// is safe for the positive bases allowed here).
fn power_of(e: &Expr) -> Option<(Expr, BigRational)> {
    let ExprNode::Pow(b, x) = e.node() else {
        return None;
    };
    let x = x.as_number().and_then(Number::as_rational)?.clone();
    if let ExprNode::Pow(inner, y) = b.node() {
        let y = y.as_number().and_then(Number::as_rational)?;
        return Some((inner.clone(), -(x * y)));
    }
    Some((b.clone(), -x))
}

fn rational(e: &Expr) -> Option<BigRational> {
    simplify(e)
        .as_number()
        .and_then(Number::as_rational)
        .cloned()
}

/// The closed form, when the term has this shape.
pub(super) fn hurwitz_series(t: &Expr, k: &str, a: &Expr) -> Option<Expr> {
    let start = BigRational::from_integer(int_value(a)?);
    let (c, rest) = split_constant(t, k);
    let (base, s) = power_of(&rest)?;
    if s <= BigRational::one() {
        return None;
    }
    let coeffs = as_poly(&base, k)?;
    if coeffs.len() != 2 {
        return None;
    }
    let (beta, alpha) = (rational(&coeffs[0])?, rational(&coeffs[1])?);
    if !alpha.is_positive() || !(&alpha * &start + &beta).is_positive() {
        return None;
    }
    let shift = &start + &beta / &alpha;
    let s_expr = Expr::ratio(s.clone());
    let zeta = if shift.is_one() {
        Expr::apply("zeta", vec![s_expr.clone()])
    } else if let Some(z) = whole_power_shifted(&s, &shift) {
        z
    } else {
        Expr::apply("hurwitzzeta", vec![s_expr.clone(), Expr::ratio(shift)])
    };
    let scale = if alpha.is_one() {
        Expr::one()
    } else {
        Expr::ratio(alpha).pow(-s_expr)
    };
    let value = simplify(&(c * scale * zeta));
    (!value.is_zero()).then_some(value)
}

/// `ζ(m, x)` for a whole `m` and a whole or half-whole `x > 0`, in terms of `ζ(m)`:
/// `ζ(m, n) = ζ(m) − Σ_{i<n} i^(−m)` and `ζ(m, n + 1/2) = (2^m − 1) ζ(m) − 2^m Σ_{j<n}
/// (2j + 1)^(−m)`.
fn whole_power_shifted(s: &BigRational, x: &BigRational) -> Option<Expr> {
    use num_bigint::BigInt;
    use num_traits::{ToPrimitive, Zero};
    if !s.is_integer() || !x.is_positive() {
        return None;
    }
    let m = s.to_integer().to_u32()?;
    let zeta = Expr::apply("zeta", vec![Expr::ratio(s.clone())]);
    let two = BigInt::from(2);
    let (scale, count, odd) = if x.is_integer() {
        (BigInt::one(), x.to_integer() - 1, false)
    } else if x.denom() == &two {
        (
            two.pow(m) - 1,
            (x - BigRational::new(BigInt::one(), two.clone())).to_integer(),
            true,
        )
    } else {
        return None;
    };
    let count = count.to_i64().filter(|c| *c <= 10_000)?;
    let mut acc = BigRational::zero();
    for i in 1..=count {
        let d = if odd {
            BigInt::from(2 * i - 1)
        } else {
            BigInt::from(i)
        };
        acc += BigRational::new(BigInt::one(), d.pow(m));
    }
    if odd {
        acc *= BigRational::from_integer(two.pow(m));
    }
    Some(simplify(
        &(Expr::ratio(BigRational::from_integer(scale)) * zeta - Expr::ratio(acc)),
    ))
}
