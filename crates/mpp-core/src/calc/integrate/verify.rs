//! The self-check: an antiderivative is differentiated back and compared with the integrand,
//! symbolically when the difference simplifies to zero, otherwise at many sample points.

use std::collections::BTreeMap;

use crate::calc::diff::diff;
use crate::error::{MathError, MathResult};
use crate::expr::Expr;
use crate::simplify::{add, mul, simplify};

/// Sample values of the integration variable (spread over several scales and both signs).
const POINTS: [f64; 24] = [
    0.37, 1.13, -0.61, 2.21, 0.83, -1.71, 0.12, 3.07, -2.93, 0.55, 1.62, -0.29, 4.41, -4.13, 0.91,
    6.27, -7.39, 0.047, 9.83, -0.88, 1.37, 12.6, -15.2, 0.71,
];
/// Values given to other symbols (parameters), chosen per symbol and point.
const PARAMS: [f64; 7] = [1.3, 0.7, 2.2, 1.7, 0.9, 2.6, 1.15];
/// Points that must agree.
const MIN_AGREE: usize = 3;
/// Relative tolerance of the comparison.
const TOL: f64 = 1e-6;

/// `Ok` when `d big_f / d var` equals `f`.
pub(crate) fn check_antiderivative(f: &Expr, big_f: &Expr, var: &str) -> MathResult<()> {
    let fail = || MathError::Inconclusive(format!("the antiderivative {big_f} of {f}"));
    if big_f.is_undefined() {
        return Err(fail());
    }
    let d = diff(big_f, var, 1, None).map_err(|_| fail())?;
    let difference = simplify(&add(vec![d.clone(), mul(vec![Expr::int(-1), f.clone()])]));
    if difference.is_zero() {
        return Ok(());
    }
    if agrees_numerically(f, &d, var) {
        Ok(())
    } else {
        Err(fail())
    }
}

/// True when `a` and `b` agree at enough sample points and disagree at none.
pub(crate) fn agrees_numerically(a: &Expr, b: &Expr, var: &str) -> bool {
    let mut names: Vec<String> = a
        .free_symbols()
        .into_iter()
        .chain(b.free_symbols())
        .map(|s| s.to_string())
        .filter(|s| s != var)
        .collect();
    names.sort();
    names.dedup();
    let mut agree = 0;
    for (i, x) in POINTS.iter().enumerate() {
        let mut env: BTreeMap<String, f64> = BTreeMap::new();
        env.insert(var.to_string(), *x);
        for (j, n) in names.iter().enumerate() {
            env.insert(n.clone(), PARAMS[(i + 3 * j) % PARAMS.len()]);
        }
        let (Ok(va), Ok(vb)) = (a.eval_f64(&env), b.eval_f64(&env)) else {
            continue;
        };
        if !va.is_finite() || !vb.is_finite() || va.abs() > 1e12 {
            continue;
        }
        let scale = 1.0_f64.max(va.abs()).max(vb.abs());
        if (va - vb).abs() <= TOL * scale {
            agree += 1;
        } else {
            return false;
        }
    }
    agree >= MIN_AGREE
}
