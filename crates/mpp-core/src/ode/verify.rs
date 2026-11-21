//! Checking a solution by putting it back into the equation: symbolically when the
//! residual simplifies to zero, otherwise at many sample points (with sample values for
//! the arbitrary constants). An answer that fails is never returned.

use std::collections::BTreeMap;

use num_complex::Complex64;

use crate::calc::diff::diff;
use crate::error::{MathError, MathResult};
use crate::expr::Expr;
use crate::simplify::{add, mul, pow, simplify};

use super::parse::Ode;

/// Sample values of the independent variable.
const POINTS: [f64; 12] = [
    0.37, 1.13, 0.61, 2.21, 0.83, 1.71, 2.93, 0.29, 1.47, 3.31, 0.53, 1.93,
];
/// Values given to constants and other parameters.
const PARAMS: [f64; 5] = [0.7, 1.3, -0.45, 2.1, 0.9];
/// Points that must give a value.
const MIN_POINTS: usize = 4;
/// Relative tolerance.
const TOL: f64 = 1e-7;

/// How a solution was confirmed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verified {
    /// The residual simplified to zero.
    Symbolic,
    /// The residual vanished at sample points (to about 7 digits).
    Numeric,
}

impl Verified {
    /// A phrase for the steps.
    pub fn describe(self) -> &'static str {
        match self {
            Verified::Symbolic => "substituting it back gives 0 exactly",
            Verified::Numeric => {
                "substituting it back gives 0 at every sample point (numerical check)"
            }
        }
    }
}

fn fail(what: &str) -> MathError {
    MathError::Inconclusive(format!(
        "the solution {what} (it did not satisfy the equation when substituted back)"
    ))
}

/// Checks the explicit solution `y = sol` (constants and parameters may be free).
pub fn explicit(ode: &Ode, sol: &Expr) -> MathResult<Verified> {
    let mut values = vec![sol.clone()];
    for k in 1..=ode.order {
        let d = diff(sol, &ode.x, k, None).map_err(|_| fail(&sol.to_string()))?;
        values.push(d);
    }
    let r = simplify(&ode.substitute(&values));
    if r.is_zero() {
        return Ok(Verified::Symbolic);
    }
    if let Ok(e) = crate::expand::expand(&r) {
        if simplify(&e).is_zero() {
            return Ok(Verified::Symbolic);
        }
    }
    let scale_terms: Vec<Expr> = values.clone();
    if vanishes(&r, &scale_terms, &ode.x, None) {
        Ok(Verified::Numeric)
    } else {
        Err(fail(&format!("{} = {sol}", ode.y)))
    }
}

/// Checks the implicit solution `g(x, y) = 0` of a first-order equation: along any level
/// curve `y′ = −g_x/g_y`, so the residual with that slope must vanish at every `(x, y)`.
pub fn implicit(ode: &Ode, g: &Expr) -> MathResult<Verified> {
    if ode.order != 1 {
        return Err(MathError::Unsupported(
            "checking an implicit solution of a higher-order equation".into(),
        ));
    }
    let gx = diff(g, &ode.x, 1, None)?;
    let gy = diff(g, &ode.y, 1, None)?;
    let slope = simplify(&mul(vec![
        Expr::int(-1),
        gx,
        pow(gy.clone(), Expr::int(-1)),
    ]));
    let r = simplify(&ode.substitute(&[Expr::sym(&ode.y), slope.clone()]));
    if r.is_zero() {
        return Ok(Verified::Symbolic);
    }
    if vanishes(&r, &[slope], &ode.x, Some(&ode.y)) {
        Ok(Verified::Numeric)
    } else {
        Err(fail(&format!("{g} = 0")))
    }
}

/// Checks an explicit solution only on `(lo, hi)` (for solutions with jumps).
pub fn explicit_on(ode: &Ode, sol: &Expr, lo: f64, hi: f64) -> MathResult<Verified> {
    let mut values = vec![sol.clone()];
    for k in 1..=ode.order {
        values.push(diff(sol, &ode.x, k, None).map_err(|_| fail(&sol.to_string()))?);
    }
    let r = simplify(&ode.substitute(&values));
    if r.is_zero() {
        return Ok(Verified::Symbolic);
    }
    let width = if hi.is_finite() { hi - lo } else { 4.0 };
    let points: Vec<f64> = POINTS
        .iter()
        .map(|p| lo + width * (0.05 + 0.9 * (p / 3.5)))
        .collect();
    if vanishes_at(&r, &values, &ode.x, None, &points) {
        Ok(Verified::Numeric)
    } else {
        Err(fail(&format!("{} = {sol}", ode.y)))
    }
}

/// Checks a truncated series: its residual must be tiny close to the centre (it is
/// `O((x − a)^N)` for a correct series).
pub fn series(ode: &Ode, terms: &Expr, centre: &Expr) -> MathResult<Verified> {
    let c = centre
        .eval_f64(&crate::eval::EmptyEnv)
        .map_err(|_| fail(&terms.to_string()))?;
    let mut values = vec![terms.clone()];
    for k in 1..=ode.order {
        values.push(diff(terms, &ode.x, k, None).map_err(|_| fail(&terms.to_string()))?);
    }
    let r = simplify(&ode.substitute(&values));
    let points: Vec<f64> = [0.01, 0.015, 0.02, 0.03, 0.04]
        .iter()
        .map(|h| c + h)
        .collect();
    let tol_scale: Vec<Expr> = values
        .iter()
        .map(|v| mul(vec![Expr::int(10), v.clone()]))
        .collect();
    if r.is_zero() || vanishes_at(&r, &tol_scale, &ode.x, None, &points) {
        Ok(Verified::Numeric)
    } else {
        Err(fail(&format!("series {terms}")))
    }
}

/// True when `r` is zero (relative to the size of `scale`) at the sample points.
pub(crate) fn vanishes(r: &Expr, scale: &[Expr], x: &str, y: Option<&str>) -> bool {
    vanishes_at(r, scale, x, y, &POINTS)
}

fn vanishes_at(r: &Expr, scale: &[Expr], x: &str, y: Option<&str>, points: &[f64]) -> bool {
    let mut names: Vec<String> = r
        .free_symbols()
        .iter()
        .map(|s| s.to_string())
        .filter(|s| s != x && Some(s.as_str()) != y)
        .collect();
    names.sort();
    names.dedup();
    let mut good = 0;
    for (i, xv) in points.iter().enumerate() {
        let mut env: BTreeMap<String, f64> = BTreeMap::new();
        env.insert(x.to_string(), *xv);
        if let Some(y) = y {
            env.insert(
                y.to_string(),
                PARAMS[(i + 2) % PARAMS.len()] + 0.1 * i as f64,
            );
        }
        for (j, n) in names.iter().enumerate() {
            env.insert(n.clone(), PARAMS[(i + 3 * j) % PARAMS.len()]);
        }
        let complex: std::collections::HashMap<String, Complex64> = env
            .iter()
            .map(|(k, v)| (k.clone(), Complex64::new(*v, 0.0)))
            .collect();
        let Ok(v) = r.eval_complex(&complex) else {
            continue;
        };
        if !(v.re.is_finite() && v.im.is_finite()) {
            continue;
        }
        let mut size = 1.0_f64;
        for s in scale {
            if let Ok(w) = s.eval_complex(&complex) {
                if w.norm().is_finite() {
                    size = size.max(w.norm());
                }
            }
        }
        if size > 1e12 {
            continue;
        }
        if v.norm() > TOL * size {
            return false;
        }
        good += 1;
    }
    good >= MIN_POINTS
}

/// Checks a condition `y⁽ᵏ⁾(x₀) = v` for an explicit solution.
pub fn condition(ode: &Ode, sol: &Expr, k: u32, x0: &Expr, v: &Expr) -> bool {
    let Ok(d) = diff(sol, &ode.x, k, None) else {
        return false;
    };
    let at = simplify(&add(vec![
        d.subs(&ode.x, x0),
        mul(vec![Expr::int(-1), v.clone()]),
    ]));
    if at.is_zero() {
        return true;
    }
    match at.eval_complex(&crate::eval::EmptyEnv) {
        Ok(z) => {
            let scale = v
                .eval_complex(&crate::eval::EmptyEnv)
                .map(|w| w.norm())
                .unwrap_or(1.0)
                .max(1.0);
            z.norm() <= 1e-9 * scale
        }
        Err(_) => false,
    }
}
