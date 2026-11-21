//! The vector operators: divergence, curl and the Laplacian.

use crate::calc::diff;
use crate::error::{MathError, MathResult};
use crate::expr::Expr;
use crate::simplify::{add, simplify};

fn d(e: &Expr, v: &str) -> MathResult<Expr> {
    diff(e, v, 1, None)
}

/// `∇ · F = Σ ∂Fᵢ/∂xᵢ`.
pub fn divergence(field: &[Expr], vars: &[&str]) -> MathResult<Expr> {
    if field.len() != vars.len() {
        return Err(MathError::WrongArity {
            function: "the divergence".into(),
            expected: vars.len().to_string(),
            got: field.len(),
        });
    }
    let parts = field
        .iter()
        .zip(vars)
        .map(|(f, v)| d(f, v))
        .collect::<MathResult<Vec<_>>>()?;
    Ok(simplify(&add(parts)))
}

/// `∇ × F` for a 3-D field; for a 2-D field `(P, Q)` the scalar curl `∂Q/∂x − ∂P/∂y`.
pub fn curl(field: &[Expr], vars: &[&str]) -> MathResult<Vec<Expr>> {
    match (field, vars) {
        ([p, q], [x, y]) => Ok(vec![simplify(&(d(q, x)? - d(p, y)?))]),
        ([p, q, r], [x, y, z]) => Ok(vec![
            simplify(&(d(r, y)? - d(q, z)?)),
            simplify(&(d(p, z)? - d(r, x)?)),
            simplify(&(d(q, x)? - d(p, y)?)),
        ]),
        _ => Err(MathError::WrongArity {
            function: "the curl".into(),
            expected: "2 or 3".into(),
            got: field.len(),
        }),
    }
}

/// `∇²f = Σ ∂²f/∂xᵢ²`.
pub fn laplacian(f: &Expr, vars: &[&str]) -> MathResult<Expr> {
    let parts = vars
        .iter()
        .map(|v| diff(f, v, 2, None))
        .collect::<MathResult<Vec<_>>>()?;
    Ok(simplify(&add(parts)))
}
