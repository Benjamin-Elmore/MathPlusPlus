//! Approximation and iteration: Bernstein polynomials
//! `B_n[f](x) = Σ_k f(k/n) C(n,k) x^k (1−x)^{n−k}` and fixed-point iteration
//! `x_{k+1} = g(x_k)` with a convergence check.

use num_bigint::BigInt;

use crate::domains::fieldkit::eval_at;
use crate::domains::util::normalize;
use crate::error::{MathError, MathResult};
use crate::expr::Expr;
use crate::number::binomial;

/// Highest Bernstein degree.
pub const MAX_BERNSTEIN: usize = 200;
/// Most fixed-point iterations.
pub const MAX_ITER: usize = 1_000_000;

/// The k-th Bernstein basis polynomial of degree n in `x`.
pub fn basis(k: usize, n: usize, x: &Expr) -> MathResult<Expr> {
    if k > n {
        return Err(MathError::Domain {
            function: "bernstein".into(),
            detail: format!("the index k = {k} exceeds the degree n = {n}"),
        });
    }
    let c = binomial(&BigInt::from(n).into(), &BigInt::from(k))?;
    Ok(Expr::ratio(c)
        * x.pow(Expr::int(k as i64))
        * (&Expr::one() - x).pow(Expr::int((n - k) as i64)))
}

/// `B_n[f]` as an expanded polynomial in `var`.
pub fn bernstein(f: &Expr, var: &str, n: usize) -> MathResult<Expr> {
    if n == 0 || n > MAX_BERNSTEIN {
        return Err(MathError::Domain {
            function: "bernstein".into(),
            detail: format!("the degree must be 1 … {MAX_BERNSTEIN}"),
        });
    }
    let x = Expr::sym(var);
    let mut terms = Vec::with_capacity(n + 1);
    for k in 0..=n {
        let fk = f.subs(var, &Expr::rational(k as i64, n as i64)).simplify();
        terms.push(fk * basis(k, n, &x)?);
    }
    Ok(normalize(&crate::simplify::add(terms)))
}

/// The result of a fixed-point iteration.
#[derive(Clone, Debug, PartialEq)]
pub struct FixedPoint {
    /// The fixed point found.
    pub value: f64,
    /// Iterations used.
    pub iterations: usize,
    /// `|g′(x*)|` estimated by a central difference (< 1 means a local contraction).
    pub rate: f64,
}

/// Iterates `x ↦ g(x)` from `x0` until successive values agree to `tol` (relative to
/// max(1, |x|)), for at most `max_iter` steps.
pub fn fixed_point(
    g: &Expr,
    var: &str,
    x0: f64,
    tol: f64,
    max_iter: usize,
) -> MathResult<FixedPoint> {
    let names = [var.to_string()];
    let step = |x: f64| eval_at(g, &names, &[x]);
    let mut x = x0;
    for i in 1..=max_iter.min(MAX_ITER) {
        let next = step(x)?;
        if !next.is_finite() {
            return Err(MathError::Divergent(format!(
                "the iteration (step {i} gave {next})"
            )));
        }
        if (next - x).abs() <= tol * next.abs().max(1.0) {
            let h = 1e-6 * next.abs().max(1.0);
            let rate = ((step(next + h)? - step(next - h)?) / (2.0 * h)).abs();
            return Ok(FixedPoint {
                value: next,
                iterations: i,
                rate,
            });
        }
        x = next;
    }
    Err(MathError::Inconclusive(format!(
        "the iteration did not settle within {max_iter} steps (last value {x})"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn bernstein_examples() {
        // B_n reproduces linear functions and B_n[x²] = x² + x(1 − x)/n (Lorentz 1953).
        let x = Expr::sym("x");
        assert_eq!(bernstein(&x, "x", 5).unwrap(), x);
        let b = bernstein(&x.pow(2), "x", 4).unwrap();
        let want = normalize(&(x.pow(2) + &x * &(&Expr::one() - &x) / Expr::int(4)));
        assert_eq!(b, want);
        assert!(bernstein(&x, "x", 0).is_err());
    }

    #[test]
    fn fixed_points() {
        // cos x = x at the Dottie number 0.7390851332151607 (OEIS A003957).
        let g = Expr::func(crate::functions::Func::Cos, vec![Expr::sym("x")]).unwrap();
        let r = fixed_point(&g, "x", 1.0, 1e-14, 1000).unwrap();
        assert!((r.value - 0.739_085_133_215_160_7).abs() < 1e-13);
        assert!(r.rate < 1.0);
        // x ↦ 2x diverges.
        let d = Expr::int(2) * Expr::sym("x");
        assert!(fixed_point(&d, "x", 1.0, 1e-12, 5000).is_err());
    }

    proptest! {
        #[test]
        fn babylonian_square_root(a in 0.5f64..1000.0) {
            // x ↦ (x + a/x)/2 converges to √a.
            let x = Expr::sym("x");
            let g = (&x + &(Expr::float(a) / x.clone())) / Expr::int(2);
            let r = fixed_point(&g, "x", a.max(1.0), 1e-14, 200).unwrap();
            prop_assert!((r.value - a.sqrt()).abs() < 1e-10 * a.sqrt());
        }

        #[test]
        fn bernstein_partition_of_unity(n in 1usize..12, t in 0.0f64..1.0) {
            let x = Expr::sym("x");
            let sum: f64 = (0..=n)
                .map(|k| eval_at(&basis(k, n, &x).unwrap(), &["x".to_string()], &[t]).unwrap())
                .sum();
            prop_assert!((sum - 1.0).abs() < 1e-12);
        }
    }
}
