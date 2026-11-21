//! The Laplace transform as the operators `laplace`, `invlaplace` (also `ilaplace`,
//! `inverselaplace`), answered by [`mpp_core::transforms::laplace`]. The script forms
//! (`\mathcal L\{f\}`) are read by the dispatcher.

use mpp_core::transforms::laplace as lt;
use mpp_core::{Expr, MathResult};

use super::operator_args::*;
use crate::holders::Ctx;

/// `\operatorname{laplace}(f)` from `t` to `s`, and the inverse.
pub(crate) fn laplace(call: &Call, ctx: &Ctx<'_>, inverse: bool) -> MathResult<Expr> {
    let p = call.parts();
    let f = p
        .functions
        .first()
        .or(p.numbers.first())
        .ok_or_else(|| missing("laplace", "a function"))?;
    let v = if inverse {
        lt::inverse(f, "s", "t")?
    } else {
        lt::transform(f, "t", "s")?
    };
    ctx.log(
        if inverse {
            "inverse Laplace transform"
        } else {
            "Laplace transform"
        },
        f,
        &v,
        Some(if inverse {
            "partial fractions and the table, with e^(−as)F(s) ↔ H(t − a)f(t − a)".into()
        } else {
            "the table: L{tⁿe^(at)} = n!/(s − a)^(n+1), L{sin bt} = b/(s² + b²), L{H(t − a)f(t − a)} = e^(−as)F(s), L{δ(t − a)} = e^(−as)".into()
        }),
    );
    Ok(v)
}
