//! `Σ_{k≥a} c (αk + β)^(−s)` for a non-integer real `s > 1`: a Hurwitz ζ value,
//! `c α^(−s) ζ(s, a + β/α)` (plain `ζ(s)` when `a + β/α = 1`). Summing such a series
//! term by term converges far too slowly to give its digits (`Σ 1/n^1.0001 = 10000.58`).

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
    if s.is_integer() || s <= BigRational::one() {
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
