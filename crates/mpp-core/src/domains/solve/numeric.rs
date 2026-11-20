//! Numeric solving: bracketing with bisection polished by Newton steps for one equation,
//! and Newton's method with a finite-difference Jacobian for small systems.

use crate::error::{MathError, MathResult};
use crate::eval::Env;
use crate::expr::Expr;

/// Samples taken when scanning an interval for sign changes.
const SAMPLES: usize = 4000;
/// Most roots reported from one scan.
const MAX_ROOTS: usize = 1000;
/// Most unknowns a numeric system may have.
pub const MAX_UNKNOWNS: usize = 16;

/// A one-variable environment.
struct One<'a> {
    name: &'a str,
    value: f64,
}

impl Env for One<'_> {
    fn real(&self, name: &str) -> Option<f64> {
        (name == self.name).then_some(self.value)
    }
}

/// Several variables at once.
struct Many<'a> {
    names: &'a [String],
    values: &'a [f64],
}

impl Env for Many<'_> {
    fn real(&self, name: &str) -> Option<f64> {
        self.names
            .iter()
            .position(|n| n == name)
            .map(|i| self.values[i])
    }
}

/// `f(x)` as an `f64`, `None` outside the domain.
pub fn eval_at(f: &Expr, var: &str, x: f64) -> Option<f64> {
    f.eval_f64(&One {
        name: var,
        value: x,
    })
    .ok()
    .filter(|v| v.is_finite())
}

fn tolerance(x: f64) -> f64 {
    1e-12 * x.abs().max(1.0)
}

/// Bisection on a bracket `[a, b]` with `f(a)`, `f(b)` of opposite signs.
pub fn bisect(f: &Expr, var: &str, mut a: f64, mut b: f64) -> Option<f64> {
    let mut fa = eval_at(f, var, a)?;
    let fb = eval_at(f, var, b)?;
    if fa == 0.0 {
        return Some(a);
    }
    if fb == 0.0 {
        return Some(b);
    }
    if fa.signum() == fb.signum() {
        return None;
    }
    for _ in 0..200 {
        let m = 0.5 * (a + b);
        if (b - a).abs() <= tolerance(m) {
            break;
        }
        let fm = eval_at(f, var, m)?;
        if fm == 0.0 {
            return Some(m);
        }
        if fm.signum() == fa.signum() {
            a = m;
            fa = fm;
        } else {
            b = m;
        }
    }
    Some(0.5 * (a + b))
}

/// Newton's method from `x0` with a central-difference derivative.
pub fn newton(f: &Expr, var: &str, x0: f64) -> Option<f64> {
    let mut x = x0;
    for _ in 0..100 {
        let fx = eval_at(f, var, x)?;
        if fx == 0.0 {
            return Some(x);
        }
        let h = 1e-7 * x.abs().max(1.0);
        let d = (eval_at(f, var, x + h)? - eval_at(f, var, x - h)?) / (2.0 * h);
        if d == 0.0 || !d.is_finite() {
            return None;
        }
        let step = fx / d;
        x -= step;
        if !x.is_finite() {
            return None;
        }
        if step.abs() <= tolerance(x) {
            return Some(x);
        }
    }
    None
}

/// Real roots of `f` in `[lo, hi]`: sign changes are bracketed and bisected, and local
/// minima of `|f|` close to zero (double roots) are polished by Newton.
pub fn roots_in(f: &Expr, var: &str, lo: f64, hi: f64) -> MathResult<Vec<f64>> {
    if !(lo.is_finite() && hi.is_finite()) || lo >= hi {
        return Err(MathError::Domain {
            function: "solve".into(),
            detail: format!("[{lo}, {hi}] is not a finite interval"),
        });
    }
    let step = (hi - lo) / SAMPLES as f64;
    let xs: Vec<f64> = (0..=SAMPLES).map(|i| lo + step * i as f64).collect();
    let ys: Vec<Option<f64>> = xs.iter().map(|&x| eval_at(f, var, x)).collect();
    let scale = ys
        .iter()
        .flatten()
        .map(|v| v.abs())
        .fold(0.0, f64::max)
        .max(1.0);
    let mut found: Vec<f64> = Vec::new();
    for i in 0..SAMPLES {
        let (Some(ya), Some(yb)) = (ys[i], ys[i + 1]) else {
            continue;
        };
        if ya == 0.0 {
            found.push(xs[i]);
        } else if ya.signum() != yb.signum() && yb != 0.0 {
            if let Some(r) = bisect(f, var, xs[i], xs[i + 1]) {
                // A pole also changes sign: keep only real zeros.
                if eval_at(f, var, r).is_some_and(|v| v.abs() <= 1e-6 * scale) {
                    found.push(r);
                }
            }
        } else if i > 0 {
            if let Some(yp) = ys[i - 1] {
                let is_min = ya.abs() < yp.abs() && ya.abs() <= yb.abs();
                if is_min && ya.abs() < 1e-3 * scale {
                    if let Some(r) = newton(f, var, xs[i]) {
                        if (lo..=hi).contains(&r)
                            && eval_at(f, var, r).is_some_and(|v| v.abs() <= 1e-9 * scale)
                        {
                            found.push(r);
                        }
                    }
                }
            }
        }
        if found.len() > MAX_ROOTS {
            return Err(MathError::TooLarge("the number of roots".into()));
        }
    }
    if ys[SAMPLES] == Some(0.0) {
        found.push(hi);
    }
    found.sort_by(f64::total_cmp);
    found.dedup_by(|a, b| (*a - *b).abs() <= 1e-9 * a.abs().max(1.0));
    Ok(found)
}

/// Solves `fᵢ(vars) = 0` by Newton's method from `start`; `None` when it does not converge.
pub fn newton_system(fs: &[Expr], vars: &[String], start: &[f64]) -> Option<Vec<f64>> {
    let n = vars.len();
    if n == 0 || n > MAX_UNKNOWNS || fs.len() != n || start.len() != n {
        return None;
    }
    let eval = |x: &[f64]| -> Option<Vec<f64>> {
        let env = Many {
            names: vars,
            values: x,
        };
        fs.iter()
            .map(|f| f.eval_f64(&env).ok().filter(|v| v.is_finite()))
            .collect()
    };
    let mut x = start.to_vec();
    for _ in 0..100 {
        let fx = eval(&x)?;
        let norm = fx.iter().map(|v| v.abs()).fold(0.0, f64::max);
        if norm < 1e-14 {
            return Some(x);
        }
        // Jacobian by central differences.
        let mut jac = vec![vec![0.0; n]; n];
        for j in 0..n {
            let h = 1e-7 * x[j].abs().max(1.0);
            let mut xp = x.clone();
            let mut xm = x.clone();
            xp[j] += h;
            xm[j] -= h;
            let (fp, fm) = (eval(&xp)?, eval(&xm)?);
            for i in 0..n {
                jac[i][j] = (fp[i] - fm[i]) / (2.0 * h);
            }
        }
        let rhs: Vec<f64> = fx.iter().map(|v| -v).collect();
        let dx = solve_dense(jac, rhs)?;
        let mut biggest = 0.0f64;
        for j in 0..n {
            x[j] += dx[j];
            biggest = biggest.max(dx[j].abs() / x[j].abs().max(1.0));
        }
        if x.iter().any(|v| !v.is_finite()) {
            return None;
        }
        if biggest < 1e-13 {
            let fx = eval(&x)?;
            let norm = fx.iter().map(|v| v.abs()).fold(0.0, f64::max);
            return (norm < 1e-8).then_some(x);
        }
    }
    let fx = eval(&x)?;
    let norm = fx.iter().map(|v| v.abs()).fold(0.0, f64::max);
    (norm < 1e-8).then_some(x)
}

/// Solves a dense linear system by Gaussian elimination with partial pivoting.
pub fn solve_dense(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for col in 0..n {
        let pivot = (col..n).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))?;
        if a[pivot][col].abs() < 1e-300 {
            return None;
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        for row in col + 1..n {
            let factor = a[row][col] / a[col][col];
            if factor != 0.0 {
                for k in col..n {
                    a[row][k] -= factor * a[col][k];
                }
                b[row] -= factor * b[col];
            }
        }
    }
    let mut x = vec![0.0; n];
    for row in (0..n).rev() {
        let s: f64 = (row + 1..n).map(|k| a[row][k] * x[k]).sum();
        x[row] = (b[row] - s) / a[row][row];
    }
    x.iter().all(|v| v.is_finite()).then_some(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_find_every_root() {
        let x = Expr::sym("x");
        let f = Expr::call("cos", vec![x.clone()]).unwrap() - x.clone();
        let rs = roots_in(&f, "x", -10.0, 10.0).unwrap();
        assert_eq!(rs.len(), 1);
        assert!((rs[0] - 0.739_085_133_215_160_6).abs() < 1e-10);
        // A double root and a pole.
        let f = (x.clone() - Expr::one()).pow(2);
        let rs = roots_in(&f, "x", -3.0, 3.3).unwrap();
        assert_eq!(rs.len(), 1);
        assert!((rs[0] - 1.0).abs() < 1e-6);
        let f = x.recip();
        assert!(roots_in(&f, "x", -1.3, 1.0).unwrap().is_empty());
    }

    #[test]
    fn newton_for_systems() {
        let (x, y) = (Expr::sym("x"), Expr::sym("y"));
        let fs = vec![
            x.pow(2) + y.pow(2) - Expr::int(4),
            Expr::e().pow(x.clone()) + y.clone() - Expr::one(),
        ];
        let vars = vec!["x".to_string(), "y".to_string()];
        let s = newton_system(&fs, &vars, &[1.0, -1.0]).unwrap();
        let env = [("x", s[0]), ("y", s[1])];
        for f in &fs {
            assert!(f.eval_f64(&env).unwrap().abs() < 1e-9);
        }
    }
}
