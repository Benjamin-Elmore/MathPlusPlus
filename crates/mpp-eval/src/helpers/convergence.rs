//! Series as operators: the named convergence `tests`, the radius and the interval of
//! convergence of a power series, answered by [`mpp_core::convergence`].

use mpp_core::convergence;
use mpp_core::{Expr, ExprNode, MathError, MathResult};

use super::operator_args::*;
use crate::holders::Ctx;

pub(super) fn index_of(p: &Parts, term: &Expr) -> String {
    if let Some(k) = p.names.iter().find(|n| term.contains_sym(n)) {
        return k.clone();
    }
    ["k", "n", "i", "j", "m"]
        .iter()
        .find(|k| term.contains_sym(k))
        .map(|k| k.to_string())
        .unwrap_or_else(|| "k".into())
}

pub(super) fn tests(call: &Call, ctx: &Ctx<'_>) -> MathResult<Expr> {
    let p = call.parts();
    let term = p
        .functions
        .first()
        .ok_or_else(|| missing("convergence tests", "the general term aₖ"))?;
    let k = index_of(&p, term);
    let (verdict, outcomes) = logged(ctx, |s| convergence::series_tests(term, &k, s))?;
    for o in &outcomes {
        ctx.note(o.to_string());
    }
    match verdict {
        Some(v) => Ok(Expr::new(ExprNode::Bool(v))),
        None => Err(MathError::Undetermined(format!(
            "whether Σ {term} converges: no test decided"
        ))),
    }
}

pub(super) fn power_series(call: &Call, ctx: &Ctx<'_>, radius: bool) -> MathResult<Expr> {
    let p = call.parts();
    let term = p
        .functions
        .first()
        .ok_or_else(|| missing("interval of convergence", "the general term"))?;
    let k = index_of(&p, term);
    let x = p
        .names
        .iter()
        .find(|n| **n != k)
        .cloned()
        .or_else(|| {
            term.free_symbols()
                .iter()
                .map(|s| s.to_string())
                .find(|s| *s != k)
        })
        .ok_or_else(|| missing("interval of convergence", "the variable x"))?;
    let r = logged(ctx, |s| convergence::power_series_interval(term, &k, &x, s))?;
    for (end, report) in &r.endpoints {
        ctx.note(format!("at {x} = {end}: {report}"));
    }
    Ok(if radius { r.radius } else { r.interval })
}
