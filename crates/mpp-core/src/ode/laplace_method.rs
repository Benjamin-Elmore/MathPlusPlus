//! The Laplace transform method for a parsed constant-coefficient equation with initial
//! values at 0; the transforms themselves are [`crate::transforms::laplace`].

use crate::error::{MathError, MathResult};
use crate::expr::Expr;
use crate::steps::Steps;
use crate::transforms::laplace::{has_step_or_delta, inverse, transform, transformed_solution};

use super::parse::Ode;
use super::{Candidate, Condition};
use crate::calc_support::value;

/// The transform method for `Σ aₖ y⁽ᵏ⁾ = g` (constant `aₖ`) with `y⁽ʲ⁾(0)` given for
/// `j < n` (missing ones become constants).
pub(crate) fn solve_ivp(
    ode: &Ode,
    conditions: &[Condition],
    steps: &mut Steps,
) -> MathResult<Candidate> {
    let (a, g) = ode.linear_coefficients().ok_or_else(|| {
        MathError::Unsupported("the Laplace transform method for a non-linear equation".into())
    })?;
    if a.iter()
        .any(|c| c.contains_sym(&ode.x) || value(c).is_none())
    {
        return Err(MathError::Unsupported(
            "the Laplace transform method for variable coefficients".into(),
        ));
    }
    let t = ode.x.as_str();
    let s = if t == "s" { "p" } else { "s" };
    let n = ode.order;
    let mut init = Vec::new();
    for j in 0..n {
        let c = conditions
            .iter()
            .find(|c| c.order == j && c.at.is_zero())
            .ok_or_else(|| MathError::Domain {
                function: "the Laplace transform method".into(),
                detail: format!(
                    "it needs the initial values y(0), …, y^({})(0); y^({j})(0) is missing",
                    n - 1
                ),
            })?;
        init.push(c.value.clone());
    }
    let big_g = transform(&g, t, s)?;
    let (p_s, y_s) = transformed_solution(&a, &big_g, &init, s);
    let y_t = inverse(&y_s, s, t)?;
    steps.push(
        "Laplace transform",
        ode.original.clone(),
        Expr::equation(Expr::sym(&ode.y), y_t.clone()),
        Some(format!(
            "transform both sides (L{{y⁽ᵏ⁾}} = sᵏY − sᵏ⁻¹y(0) − …): ({p_s})·Y − (initial terms) = {big_g}, so Y(s) = {y_s}; partial fractions and the table (with e^(−as)F(s) ↔ H(t − a)f(t − a)) give y(t) = {y_t}"
        )),
    );
    let mut cand = Candidate::explicit(y_t, "Laplace transform");
    if has_step_or_delta(&cand_expr(&cand)) {
        cand.notes.push("H is the Heaviside step function".into());
    }
    Ok(cand)
}

fn cand_expr(c: &Candidate) -> Expr {
    match &c.form {
        super::Form::Explicit(e) => e.clone(),
        _ => Expr::zero(),
    }
}
