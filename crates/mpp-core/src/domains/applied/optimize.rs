//! Smooth optimisation: gradient descent `x_{k+1} = x_k − η ∇f(x_k)` and a check of the
//! Karush–Kuhn–Tucker conditions at a candidate point for
//! `min f(x)` subject to `g_i(x) ≤ 0` (and `h_j(x) = 0`):
//! stationarity `∇f + Σ λ_i ∇g_i = 0`, primal feasibility, dual feasibility `λ_i ≥ 0`
//! for inequalities and complementary slackness `λ_i g_i(x) = 0`.

use crate::domains::calculus::Calculus;
use crate::domains::fieldkit::eval_at;
use crate::error::{MathError, MathResult};
use crate::expr::Expr;

/// Most gradient-descent steps.
pub const MAX_STEPS: usize = 1_000_000;
/// Tolerance used by the KKT check.
pub const KKT_TOL: f64 = 1e-7;

/// Symbolic gradient of `f` in `vars`.
pub fn gradient(f: &Expr, vars: &[String], calc: &dyn Calculus) -> MathResult<Vec<Expr>> {
    vars.iter().map(|v| calc.diff(f, v)).collect()
}

fn eval_all(es: &[Expr], vars: &[String], x: &[f64]) -> MathResult<Vec<f64>> {
    es.iter().map(|e| eval_at(e, vars, x)).collect()
}

/// The result of gradient descent.
#[derive(Clone, Debug, PartialEq)]
pub struct Descent {
    /// The final point.
    pub point: Vec<f64>,
    /// Steps taken.
    pub steps: usize,
    /// ‖∇f‖ at the final point.
    pub grad_norm: f64,
}

/// Fixed-step gradient descent until ‖∇f‖ ≤ `tol` or `max_steps`.
pub fn gradient_descent(
    f: &Expr,
    vars: &[String],
    x0: &[f64],
    eta: f64,
    tol: f64,
    max_steps: usize,
    calc: &dyn Calculus,
) -> MathResult<Descent> {
    if eta.is_nan() || eta <= 0.0 {
        return Err(MathError::Domain {
            function: "graddescent".into(),
            detail: "the step size η must be positive".into(),
        });
    }
    let grad = gradient(f, vars, calc)?;
    let mut x = x0.to_vec();
    for step in 0..=max_steps.min(MAX_STEPS) {
        let g = eval_all(&grad, vars, &x)?;
        let norm = g.iter().map(|v| v * v).sum::<f64>().sqrt();
        if !norm.is_finite() {
            return Err(MathError::Divergent(format!(
                "gradient descent (step {step}); try a smaller η"
            )));
        }
        if norm <= tol {
            return Ok(Descent {
                point: x,
                steps: step,
                grad_norm: norm,
            });
        }
        for (xi, gi) in x.iter_mut().zip(&g) {
            *xi -= eta * gi;
        }
    }
    Err(MathError::Inconclusive(format!(
        "gradient descent did not converge in {max_steps} steps"
    )))
}

/// A constraint `g(x) ≤ 0` (inequality) or `h(x) = 0` (equality).
#[derive(Clone, Debug, PartialEq)]
pub struct Constraint {
    /// The constraint function in `≤ 0` / `= 0` form.
    pub g: Expr,
    /// True for an equality.
    pub equality: bool,
}

/// What the KKT check found.
#[derive(Clone, Debug, PartialEq)]
pub struct KktReport {
    /// All conditions hold within tolerance.
    pub holds: bool,
    /// The multipliers used (given, or fitted on the active set).
    pub multipliers: Vec<f64>,
    /// ‖∇f + Σ λ ∇g‖.
    pub stationarity: f64,
    /// Largest constraint violation.
    pub infeasibility: f64,
    /// Most negative inequality multiplier (0 if none).
    pub dual_violation: f64,
    /// Largest |λ_i g_i|.
    pub slackness: f64,
}

/// Solves the small symmetric system `M λ = r` (Gaussian elimination, partial pivots,
/// a tiny ridge so rank-deficient active sets still give an answer).
fn solve_small(mut m: Vec<Vec<f64>>, mut r: Vec<f64>) -> Vec<f64> {
    let n = r.len();
    for (i, row) in m.iter_mut().enumerate() {
        row[i] += 1e-14;
    }
    for c in 0..n {
        let p = (c..n)
            .max_by(|&a, &b| m[a][c].abs().total_cmp(&m[b][c].abs()))
            .unwrap_or(c);
        m.swap(c, p);
        r.swap(c, p);
        let d = m[c][c];
        if d.abs() < 1e-300 {
            continue;
        }
        for i in c + 1..n {
            let f = m[i][c] / d;
            for j in c..n {
                m[i][j] -= f * m[c][j];
            }
            r[i] -= f * r[c];
        }
    }
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        let s: f64 = (i + 1..n).map(|j| m[i][j] * x[j]).sum();
        x[i] = if m[i][i].abs() < 1e-300 {
            0.0
        } else {
            (r[i] - s) / m[i][i]
        };
    }
    x
}

/// Checks the KKT conditions at `x`; when `lambda` is `None` the multipliers are
/// fitted by least squares on the active constraints.
pub fn kkt(
    f: &Expr,
    cons: &[Constraint],
    vars: &[String],
    x: &[f64],
    lambda: Option<&[f64]>,
    calc: &dyn Calculus,
) -> MathResult<KktReport> {
    let gf = eval_all(&gradient(f, vars, calc)?, vars, x)?;
    let gvals: Vec<f64> = cons
        .iter()
        .map(|c| eval_at(&c.g, vars, x))
        .collect::<MathResult<_>>()?;
    let grads: Vec<Vec<f64>> = cons
        .iter()
        .map(|c| eval_all(&gradient(&c.g, vars, calc)?, vars, x))
        .collect::<MathResult<_>>()?;
    let scale = 1f64.max(gf.iter().fold(0.0, |a: f64, v| a.max(v.abs())));
    let lam: Vec<f64> = match lambda {
        Some(l) => {
            if l.len() != cons.len() {
                return Err(MathError::Domain {
                    function: "kkt".into(),
                    detail: format!("give one multiplier per constraint ({})", cons.len()),
                });
            }
            l.to_vec()
        }
        None => {
            let active: Vec<usize> = (0..cons.len())
                .filter(|&i| cons[i].equality || gvals[i].abs() <= KKT_TOL * scale)
                .collect();
            let mut lam = vec![0.0; cons.len()];
            if !active.is_empty() {
                // Normal equations Gᵀ G λ = −Gᵀ ∇f over the active gradients.
                let k = active.len();
                let mut m = vec![vec![0.0; k]; k];
                let mut r = vec![0.0; k];
                for (a, &i) in active.iter().enumerate() {
                    for (b, &j) in active.iter().enumerate() {
                        m[a][b] = grads[i].iter().zip(&grads[j]).map(|(p, q)| p * q).sum();
                    }
                    r[a] = -grads[i].iter().zip(&gf).map(|(p, q)| p * q).sum::<f64>();
                }
                for (a, v) in solve_small(m, r).into_iter().enumerate() {
                    lam[active[a]] = v;
                }
            }
            lam
        }
    };
    let mut resid = gf.clone();
    for (l, g) in lam.iter().zip(&grads) {
        for (r, gi) in resid.iter_mut().zip(g) {
            *r += l * gi;
        }
    }
    let stationarity = resid.iter().map(|v| v * v).sum::<f64>().sqrt();
    let infeasibility = cons
        .iter()
        .zip(&gvals)
        .map(|(c, &g)| if c.equality { g.abs() } else { g.max(0.0) })
        .fold(0.0, f64::max);
    let dual_violation = cons
        .iter()
        .zip(&lam)
        .filter(|(c, _)| !c.equality)
        .map(|(_, &l)| (-l).max(0.0))
        .fold(0.0, f64::max);
    let slackness = cons
        .iter()
        .zip(lam.iter().zip(&gvals))
        .filter(|(c, _)| !c.equality)
        .map(|(_, (l, g))| (l * g).abs())
        .fold(0.0, f64::max);
    let tol = KKT_TOL * scale;
    Ok(KktReport {
        holds: stationarity <= tol
            && infeasibility <= tol
            && dual_violation <= tol
            && slackness <= tol,
        multipliers: lam,
        stationarity,
        infeasibility,
        dual_violation,
        slackness,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::calculus::BasicCalculus;
    use proptest::prelude::*;

    fn xy() -> (Expr, Expr, Vec<String>) {
        (Expr::sym("x"), Expr::sym("y"), vec!["x".into(), "y".into()])
    }

    #[test]
    fn kkt_examples() {
        // min x² + y² s.t. 1 − x − y ≤ 0: optimum (1/2, 1/2) with λ = 1.
        let (x, y, vars) = xy();
        let f = x.pow(2) + y.pow(2);
        let g = Constraint {
            g: Expr::one() - x.clone() - y.clone(),
            equality: false,
        };
        let r = kkt(
            &f,
            std::slice::from_ref(&g),
            &vars,
            &[0.5, 0.5],
            None,
            &BasicCalculus,
        )
        .unwrap();
        assert!(r.holds, "{r:?}");
        assert!((r.multipliers[0] - 1.0).abs() < 1e-9);
        // (1, 0) is feasible but not optimal.
        let r = kkt(
            &f,
            std::slice::from_ref(&g),
            &vars,
            &[1.0, 0.0],
            None,
            &BasicCalculus,
        )
        .unwrap();
        assert!(!r.holds);
        // A negative multiplier violates dual feasibility.
        let r = kkt(&f, &[g], &vars, &[0.5, 0.5], Some(&[-1.0]), &BasicCalculus).unwrap();
        assert!(!r.holds);
        // Equality constraint x + y = 2 on x² + y²: (1, 1) with λ = −2.
        let h = Constraint {
            g: x.clone() + y.clone() - Expr::int(2),
            equality: true,
        };
        let r = kkt(&f, &[h], &vars, &[1.0, 1.0], None, &BasicCalculus).unwrap();
        assert!(r.holds && (r.multipliers[0] + 2.0).abs() < 1e-9);
    }

    proptest! {
        #[test]
        fn descent_finds_quadratic_minimum(a in -5.0f64..5.0, b in -5.0f64..5.0) {
            // f = (x − a)² + 2(y − b)² has its minimum at (a, b).
            let (x, y, vars) = xy();
            let f = (x - Expr::float(a)).pow(2) + Expr::int(2) * (y - Expr::float(b)).pow(2);
            let d = gradient_descent(&f, &vars, &[0.0, 0.0], 0.1, 1e-10, 10_000, &BasicCalculus).unwrap();
            prop_assert!((d.point[0] - a).abs() < 1e-9 && (d.point[1] - b).abs() < 1e-9);
        }
    }

    #[test]
    fn descent_diverges_with_large_steps() {
        let (x, _, _) = xy();
        let vars = vec!["x".to_string()];
        let r = gradient_descent(
            &x.pow(2),
            &vars,
            &[1.0],
            1.5,
            1e-12,
            100_000,
            &BasicCalculus,
        );
        assert!(r.is_err());
    }
}
