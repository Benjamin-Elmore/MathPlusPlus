//! Regression (least-squares fits), correlation and quartiles.
//!
//! Linear and polynomial fits on exact (rational) data are solved exactly from the
//! normal equations, so data taken from `y = 2x + 1` gives exactly `2`, `1` and
//! `R² = 1`. Other data, and the exponential, power and logarithmic models, are fitted
//! in `f64` (polynomials by Householder QR, which avoids squaring the condition number).
//!
//! `R²` is always `1 − SS_res/SS_tot` on the original data, also for the exponential
//! and power models that are fitted on logarithms.

use std::cmp::Ordering;
use std::fmt;

use num_rational::BigRational;
use num_traits::{Signed, ToPrimitive, Zero};

use crate::domains::stats::median;
use crate::domains::util::{self, arity, items, rat_expr};
use crate::domains::{Ctx, Domain, Value};
use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode};
use crate::functions::Func;
use crate::simplify::add;

/// The regression domain.
pub struct Regression;

const OPS: &[&str] = &[
    "linreg",
    "polyreg",
    "expreg",
    "powreg",
    "logreg",
    "corr",
    "quartiles",
    "iqr",
];

/// Highest polynomial degree fitted.
pub const MAX_DEGREE: usize = 20;

/// The kind of model a fit uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FitKind {
    /// `y = a + b x` (coefficients `[a, b]`).
    Linear,
    /// `y = c₀ + c₁ x + … + c_d x^d` (coefficients `[c₀, …, c_d]`).
    Polynomial(usize),
    /// `y = a e^{b x}` (coefficients `[a, b]`).
    Exponential,
    /// `y = a x^b` (coefficients `[a, b]`).
    Power,
    /// `y = a + b ln x` (coefficients `[a, b]`).
    Logarithmic,
}

impl fmt::Display for FitKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FitKind::Linear => write!(f, "linear"),
            FitKind::Polynomial(d) => write!(f, "polynomial of degree {d}"),
            FitKind::Exponential => write!(f, "exponential"),
            FitKind::Power => write!(f, "power"),
            FitKind::Logarithmic => write!(f, "logarithmic"),
        }
    }
}

/// A fitted model.
#[derive(Clone, Debug, PartialEq)]
pub struct Fit {
    pub kind: FitKind,
    /// The coefficients, in the order given by [`FitKind`].
    pub coefficients: Vec<Expr>,
    /// The fitted function as an expression in the requested variable.
    pub model: Expr,
    /// `R²` on the original data; `None` when all `y` are equal (then `R²` is undefined).
    pub r_squared: Option<Expr>,
}

fn domain(op: &str, detail: impl Into<String>) -> MathError {
    MathError::Domain {
        function: op.into(),
        detail: detail.into(),
    }
}

fn float(op: &str, x: f64) -> MathResult<Expr> {
    if x.is_finite() {
        Ok(Expr::float(x))
    } else {
        Err(domain(
            op,
            "the fit has no finite coefficients for this data",
        ))
    }
}

/// Data as exact rationals, when every value is one.
fn exact(xs: &[Expr]) -> Option<Vec<BigRational>> {
    xs.iter().map(|x| util::rational(&x.simplify())).collect()
}

fn reals(op: &str, xs: &[Expr]) -> MathResult<Vec<f64>> {
    xs.iter()
        .map(|x| util::real_arg(op, &x.simplify()))
        .collect()
}

fn check_pairs(op: &str, xs: &[Expr], ys: &[Expr], at_least: usize) -> MathResult<()> {
    if xs.len() != ys.len() {
        return Err(domain(
            op,
            format!(
                "the lists have different lengths ({} and {})",
                xs.len(),
                ys.len()
            ),
        ));
    }
    if xs.len() < at_least {
        return Err(domain(
            op,
            format!("needs at least {at_least} data points, got {}", xs.len()),
        ));
    }
    Ok(())
}

fn distinct_count(xs: &[f64]) -> usize {
    let mut v = xs.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    v.dedup();
    v.len()
}

// ---- exact linear algebra ----------------------------------------------------------

/// Solves the square system `m · c = rhs` exactly; `None` when it is singular.
fn solve_exact(
    mut m: Vec<Vec<BigRational>>,
    mut rhs: Vec<BigRational>,
) -> Option<Vec<BigRational>> {
    let n = rhs.len();
    for col in 0..n {
        let pivot = (col..n).find(|&r| !m[r][col].is_zero())?;
        m.swap(col, pivot);
        rhs.swap(col, pivot);
        for r in 0..n {
            if r != col && !m[r][col].is_zero() {
                let factor = &m[r][col] / &m[col][col];
                for k in col..n {
                    let delta = &factor * &m[col][k];
                    m[r][k] -= delta;
                }
                let delta = &factor * &rhs[col];
                rhs[r] -= delta;
            }
        }
    }
    Some((0..n).map(|i| &rhs[i] / &m[i][i]).collect())
}

fn exact_poly_fit(xs: &[BigRational], ys: &[BigRational], d: usize) -> Option<Vec<BigRational>> {
    let m = d + 1;
    // Power sums Σ x^k for k = 0 … 2d and Σ y x^k for k = 0 … d.
    let mut sx = vec![BigRational::zero(); 2 * d + 1];
    let mut sxy = vec![BigRational::zero(); m];
    for (x, y) in xs.iter().zip(ys) {
        let mut p = BigRational::from_integer(1.into());
        for k in 0..=2 * d {
            sx[k] += &p;
            if k < m {
                sxy[k] += &p * y;
            }
            p *= x;
        }
    }
    let normal: Vec<Vec<BigRational>> = (0..m)
        .map(|i| (0..m).map(|j| sx[i + j].clone()).collect())
        .collect();
    solve_exact(normal, sxy)
}

// ---- f64 least squares -------------------------------------------------------------------

/// Least squares `min ‖A c − y‖` by Householder QR; `None` when `A` is rank deficient.
fn least_squares(a: &[Vec<f64>], y: &[f64]) -> Option<Vec<f64>> {
    let rows = a.len();
    let cols = a.first()?.len();
    if rows < cols {
        return None;
    }
    let mut r: Vec<Vec<f64>> = a.to_vec();
    let mut b = y.to_vec();
    let scale = a
        .iter()
        .flatten()
        .fold(0.0f64, |m, v| m.max(v.abs()))
        .max(1.0);
    for k in 0..cols {
        let norm = (k..rows).map(|i| r[i][k] * r[i][k]).sum::<f64>().sqrt();
        if norm <= 1e-12 * scale {
            return None;
        }
        let alpha = if r[k][k] > 0.0 { -norm } else { norm };
        let mut v: Vec<f64> = (k..rows).map(|i| r[i][k]).collect();
        v[0] -= alpha;
        let vnorm2 = v.iter().map(|x| x * x).sum::<f64>();
        if vnorm2 == 0.0 {
            continue;
        }
        for j in k..cols {
            let dot: f64 = (k..rows).map(|i| v[i - k] * r[i][j]).sum();
            let f = 2.0 * dot / vnorm2;
            for i in k..rows {
                r[i][j] -= f * v[i - k];
            }
        }
        let dot: f64 = (k..rows).map(|i| v[i - k] * b[i]).sum();
        let f = 2.0 * dot / vnorm2;
        for i in k..rows {
            b[i] -= f * v[i - k];
        }
    }
    let mut c = vec![0.0; cols];
    for i in (0..cols).rev() {
        let s: f64 = (i + 1..cols).map(|j| r[i][j] * c[j]).sum();
        c[i] = (b[i] - s) / r[i][i];
    }
    c.iter().all(|v| v.is_finite()).then_some(c)
}

fn r_squared_f64(ys: &[f64], predicted: &[f64]) -> Option<f64> {
    let n = ys.len() as f64;
    let mean = ys.iter().sum::<f64>() / n;
    let ss_tot: f64 = ys.iter().map(|y| (y - mean) * (y - mean)).sum();
    if ss_tot == 0.0 {
        return None;
    }
    let ss_res: f64 = ys
        .iter()
        .zip(predicted)
        .map(|(y, p)| (y - p) * (y - p))
        .sum();
    Some(1.0 - ss_res / ss_tot)
}

fn poly_model(coefficients: &[Expr], var: &str) -> Expr {
    let x = Expr::sym(var);
    add(coefficients
        .iter()
        .enumerate()
        .map(|(k, c)| c * &x.pow(Expr::int(k as i64)))
        .collect())
    .simplify()
}

/// Fits a polynomial of degree `d` (linear when `d = 1`).
pub fn poly_fit(xs: &[Expr], ys: &[Expr], d: usize, var: &str) -> MathResult<Fit> {
    let op = if d == 1 { "linreg" } else { "polyreg" };
    if d > MAX_DEGREE {
        return Err(domain(op, format!("the degree is at most {MAX_DEGREE}")));
    }
    check_pairs(op, xs, ys, d + 1)?;
    let kind = if d == 1 {
        FitKind::Linear
    } else {
        FitKind::Polynomial(d)
    };
    let fx = reals(op, xs)?;
    if distinct_count(&fx) < d + 1 {
        return Err(domain(
            op,
            format!(
                "a degree-{d} fit needs at least {} different x values",
                d + 1
            ),
        ));
    }
    if let (Some(ex), Some(ey)) = (exact(xs), exact(ys)) {
        if let Some(c) = exact_poly_fit(&ex, &ey, d) {
            let coefficients: Vec<Expr> = c.iter().cloned().map(rat_expr).collect();
            // Exact R².
            let n = BigRational::from_integer((ey.len() as i64).into());
            let mean = ey.iter().fold(BigRational::zero(), |s, y| s + y) / n;
            let ss_tot = ey
                .iter()
                .fold(BigRational::zero(), |s, y| s + (y - &mean) * (y - &mean));
            let ss_res = ex.iter().zip(&ey).fold(BigRational::zero(), |s, (x, y)| {
                let mut p = BigRational::zero();
                for coeff in c.iter().rev() {
                    p = p * x + coeff;
                }
                s + (y - &p) * (y - &p)
            });
            let r_squared = (!ss_tot.is_zero())
                .then(|| rat_expr(BigRational::from_integer(1.into()) - ss_res / ss_tot));
            return Ok(Fit {
                kind,
                model: poly_model(&coefficients, var),
                coefficients,
                r_squared,
            });
        }
    }
    let fy = reals(op, ys)?;
    let a: Vec<Vec<f64>> = fx
        .iter()
        .map(|x| (0..=d).map(|k| x.powi(k as i32)).collect())
        .collect();
    let c = least_squares(&a, &fy)
        .ok_or_else(|| domain(op, "the data do not determine the coefficients"))?;
    let predicted: Vec<f64> = fx
        .iter()
        .map(|x| c.iter().rev().fold(0.0, |p, k| p * x + k))
        .collect();
    let coefficients = c
        .iter()
        .map(|&v| float(op, v))
        .collect::<MathResult<Vec<_>>>()?;
    let r_squared = r_squared_f64(&fy, &predicted)
        .map(|r| float(op, r))
        .transpose()?;
    Ok(Fit {
        kind,
        model: poly_model(&coefficients, var),
        coefficients,
        r_squared,
    })
}

/// Fits `y = a e^{b x}` (needs `y > 0`), `y = a x^b` (needs `x, y > 0`) or
/// `y = a + b ln x` (needs `x > 0`).
pub fn transformed_fit(xs: &[Expr], ys: &[Expr], kind: FitKind, var: &str) -> MathResult<Fit> {
    let op = match kind {
        FitKind::Exponential => "expreg",
        FitKind::Power => "powreg",
        FitKind::Logarithmic => "logreg",
        _ => return poly_fit(xs, ys, if kind == FitKind::Linear { 1 } else { 2 }, var),
    };
    check_pairs(op, xs, ys, 2)?;
    let fx = reals(op, xs)?;
    let fy = reals(op, ys)?;
    let need_x = matches!(kind, FitKind::Power | FitKind::Logarithmic);
    let need_y = matches!(kind, FitKind::Exponential | FitKind::Power);
    if need_x && fx.iter().any(|&x| x <= 0.0) {
        return Err(domain(op, "every x value must be positive for this model"));
    }
    if need_y && fy.iter().any(|&y| y <= 0.0) {
        return Err(domain(op, "every y value must be positive for this model"));
    }
    let tx: Vec<f64> = if need_x {
        fx.iter().map(|x| x.ln()).collect()
    } else {
        fx.clone()
    };
    let ty: Vec<f64> = if need_y {
        fy.iter().map(|y| y.ln()).collect()
    } else {
        fy.clone()
    };
    if distinct_count(&tx) < 2 {
        return Err(domain(op, "the fit needs at least 2 different x values"));
    }
    let a: Vec<Vec<f64>> = tx.iter().map(|&x| vec![1.0, x]).collect();
    let c = least_squares(&a, &ty)
        .ok_or_else(|| domain(op, "the data do not determine the coefficients"))?;
    let (a0, b) = (c[0], c[1]);
    let a_coef = if need_y { a0.exp() } else { a0 };
    let predict = |x: f64| match kind {
        FitKind::Exponential => a_coef * (b * x).exp(),
        FitKind::Power => a_coef * x.powf(b),
        _ => a_coef + b * x.ln(),
    };
    let predicted: Vec<f64> = fx.iter().map(|&x| predict(x)).collect();
    let (ea, eb) = (float(op, a_coef)?, float(op, b)?);
    let x = Expr::sym(var);
    let model = match kind {
        FitKind::Exponential => &ea * &Expr::e().pow(&eb * &x),
        FitKind::Power => &ea * &x.pow(eb.clone()),
        _ => &ea + &(&eb * &Expr::func(Func::Ln, vec![x])?),
    };
    let r_squared = r_squared_f64(&fy, &predicted)
        .map(|r| float(op, r))
        .transpose()?;
    Ok(Fit {
        kind,
        coefficients: vec![ea, eb],
        model,
        r_squared,
    })
}

/// Pearson's correlation coefficient `r`: exact (possibly with a square root) for
/// exact data, a float otherwise. An error when either list has no spread.
pub fn correlation(xs: &[Expr], ys: &[Expr]) -> MathResult<Expr> {
    let op = "corr";
    check_pairs(op, xs, ys, 2)?;
    let no_spread = || domain(op, "a list with all values equal has no correlation");
    if let (Some(ex), Some(ey)) = (exact(xs), exact(ys)) {
        let n = BigRational::from_integer((ex.len() as i64).into());
        let mx = ex.iter().fold(BigRational::zero(), |s, v| s + v) / &n;
        let my = ey.iter().fold(BigRational::zero(), |s, v| s + v) / &n;
        let (mut sxy, mut sxx, mut syy) = (
            BigRational::zero(),
            BigRational::zero(),
            BigRational::zero(),
        );
        for (x, y) in ex.iter().zip(&ey) {
            let (dx, dy) = (x - &mx, y - &my);
            sxy += &dx * &dy;
            sxx += &dx * &dx;
            syy += &dy * &dy;
        }
        if sxx.is_zero() || syy.is_zero() {
            return Err(no_spread());
        }
        return Ok((rat_expr(sxy) / rat_expr(sxx * syy).sqrt()).simplify());
    }
    let (fx, fy) = (reals(op, xs)?, reals(op, ys)?);
    let n = fx.len() as f64;
    let mx = fx.iter().sum::<f64>() / n;
    let my = fy.iter().sum::<f64>() / n;
    let sxy: f64 = fx.iter().zip(&fy).map(|(x, y)| (x - mx) * (y - my)).sum();
    let sxx: f64 = fx.iter().map(|x| (x - mx) * (x - mx)).sum();
    let syy: f64 = fy.iter().map(|y| (y - my) * (y - my)).sum();
    if sxx == 0.0 || syy == 0.0 {
        return Err(no_spread());
    }
    float(op, (sxy / (sxx * syy).sqrt()).clamp(-1.0, 1.0))
}

/// The quartiles `(Q1, Q2, Q3)` by the median-of-halves method (as on the TI-84): `Q2` is
/// the median; `Q1` and `Q3` are the medians of the lower and upper halves, which leave
/// out the middle value when the count is odd. Needs at least two values.
pub fn quartiles(xs: &[Expr]) -> MathResult<(Expr, Expr, Expr)> {
    if xs.len() < 2 {
        return Err(MathError::Undefined(
            "the quartiles of fewer than two values".into(),
        ));
    }
    let mut v: Vec<Expr> = xs.iter().map(Expr::simplify).collect();
    let mut failed = false;
    v.sort_by(|a, b| {
        util::compare(a, b).unwrap_or_else(|| {
            failed = true;
            Ordering::Equal
        })
    });
    if failed {
        return Err(MathError::Unsupported(
            "ordering symbolic data (quartiles need numbers)".into(),
        ));
    }
    let half = v.len() / 2;
    let q1 = median(&v[..half])?;
    let q2 = median(&v)?;
    let q3 = median(&v[v.len() - half..])?;
    Ok((q1, q2, q3))
}

fn list(op: &str, e: &Expr) -> MathResult<Vec<Expr>> {
    items(op, std::slice::from_ref(e))
}

fn two_lists(op: &str, args: &[Expr], extra: usize) -> MathResult<(Vec<Expr>, Vec<Expr>)> {
    if args.len() != 2 + extra {
        return Err(arity(op, &(2 + extra).to_string(), args.len()));
    }
    Ok((list(op, &args[0])?, list(op, &args[1])?))
}

fn note_fit(ctx: &mut Ctx<'_>, fit: &Fit) {
    match &fit.r_squared {
        Some(r) => ctx.note(format!("{} fit, R² = {r}", fit.kind)),
        None => ctx.note(format!(
            "{} fit; R² is undefined because every y value is the same",
            fit.kind
        )),
    }
}

impl Domain for Regression {
    fn name(&self) -> &'static str {
        "regression"
    }

    fn operators(&self) -> &'static [&'static str] {
        OPS
    }

    fn apply(&self, op: &str, args: &[Value], ctx: &mut Ctx<'_>) -> Option<MathResult<Value>> {
        Some(apply(op, args, ctx))
    }
}

fn apply(op: &str, args: &[Value], ctx: &mut Ctx<'_>) -> MathResult<Value> {
    match op {
        "linreg" | "expreg" | "powreg" | "logreg" => {
            let (xs, ys) = two_lists(op, args, 0)?;
            let fit = match op {
                "linreg" => poly_fit(&xs, &ys, 1, "x")?,
                "expreg" => transformed_fit(&xs, &ys, FitKind::Exponential, "x")?,
                "powreg" => transformed_fit(&xs, &ys, FitKind::Power, "x")?,
                _ => transformed_fit(&xs, &ys, FitKind::Logarithmic, "x")?,
            };
            note_fit(ctx, &fit);
            Ok(fit.model)
        }
        "polyreg" => {
            let (xs, ys) = two_lists(op, args, 1)?;
            let d = util::integer(&args[2].simplify())
                .filter(|d| !d.is_negative())
                .and_then(|d| d.to_usize())
                .ok_or_else(|| {
                    domain(
                        op,
                        format!("the degree must be a whole number ≥ 0, not {}", args[2]),
                    )
                })?;
            let fit = poly_fit(&xs, &ys, d, "x")?;
            note_fit(ctx, &fit);
            Ok(fit.model)
        }
        "corr" => {
            let (xs, ys) = two_lists(op, args, 0)?;
            ctx.note("Pearson correlation coefficient r");
            correlation(&xs, &ys)
        }
        "quartiles" | "iqr" => {
            let xs = items(op, args)?;
            let (q1, q2, q3) = quartiles(&xs)?;
            ctx.note("quartiles by the median-of-halves method (the median is left out of the halves when n is odd)");
            Ok(if op == "iqr" {
                (q3 - q1).simplify()
            } else {
                Expr::new(ExprNode::Tuple(vec![q1, q2, q3]))
            })
        }
        _ => Err(MathError::UnknownFunction(op.into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn ints(v: &[i64]) -> Vec<Expr> {
        v.iter().map(|&k| Expr::int(k)).collect()
    }

    fn floats(v: &[f64]) -> Vec<Expr> {
        v.iter().map(|&k| Expr::float(k)).collect()
    }

    fn tuple(v: Vec<Expr>) -> Expr {
        Expr::new(ExprNode::Tuple(v))
    }

    fn num(e: &Expr) -> f64 {
        util::numeric(e).unwrap()
    }

    #[test]
    fn exact_linear_fit() {
        let xs = ints(&[0, 1, 2, 3, 4]);
        let ys = ints(&[1, 3, 5, 7, 9]);
        let fit = poly_fit(&xs, &ys, 1, "x").unwrap();
        assert_eq!(fit.coefficients, ints(&[1, 2]));
        assert_eq!(fit.r_squared, Some(Expr::one()));
        assert_eq!(
            fit.model,
            (Expr::int(2) * Expr::sym("x") + Expr::one()).simplify()
        );
        // A textbook fit: (1, 2), (2, 4), (3, 5), (4, 4), (5, 5) → y = 0.6x + 2.2, R² = 0.6.
        let fit = poly_fit(&ints(&[1, 2, 3, 4, 5]), &ints(&[2, 4, 5, 4, 5]), 1, "x").unwrap();
        assert_eq!(
            fit.coefficients,
            vec![Expr::rational(11, 5), Expr::rational(3, 5)]
        );
        assert_eq!(fit.r_squared, Some(Expr::rational(3, 5)));
    }

    #[test]
    fn exact_quadratic_fit() {
        let xs = ints(&[-2, -1, 0, 1, 2, 3]);
        let ys: Vec<Expr> = [-2i64, -1, 0, 1, 2, 3]
            .iter()
            .map(|&x| Expr::rational(3 * x * x - x + 2, 2))
            .collect();
        let fit = poly_fit(&xs, &ys, 2, "x").unwrap();
        assert_eq!(
            fit.coefficients,
            vec![Expr::one(), Expr::rational(-1, 2), Expr::rational(3, 2)]
        );
        assert_eq!(fit.r_squared, Some(Expr::one()));
        assert!(poly_fit(&ints(&[1, 1, 1]), &ints(&[1, 2, 3]), 1, "x").is_err());
        assert!(poly_fit(&ints(&[1, 2]), &ints(&[1]), 1, "x").is_err());
    }

    #[test]
    fn float_and_transformed_fits() {
        let xs: Vec<f64> = (0..8).map(|k| k as f64 * 0.5).collect();
        let exp: Vec<f64> = xs.iter().map(|x| 3.0 * (0.7 * x).exp()).collect();
        let fit = transformed_fit(&floats(&xs), &floats(&exp), FitKind::Exponential, "x").unwrap();
        assert!((num(&fit.coefficients[0]) - 3.0).abs() < 1e-9);
        assert!((num(&fit.coefficients[1]) - 0.7).abs() < 1e-9);
        assert!((num(fit.r_squared.as_ref().unwrap()) - 1.0).abs() < 1e-12);
        let px: Vec<f64> = (1..9).map(|k| k as f64).collect();
        let pw: Vec<f64> = px.iter().map(|x| 2.5 * x.powf(1.5)).collect();
        let fit = transformed_fit(&floats(&px), &floats(&pw), FitKind::Power, "x").unwrap();
        assert!((num(&fit.coefficients[0]) - 2.5).abs() < 1e-9);
        assert!((num(&fit.coefficients[1]) - 1.5).abs() < 1e-9);
        let lg: Vec<f64> = px.iter().map(|x| 1.0 - 2.0 * x.ln()).collect();
        let fit = transformed_fit(&floats(&px), &floats(&lg), FitKind::Logarithmic, "x").unwrap();
        assert!((num(&fit.coefficients[0]) - 1.0).abs() < 1e-9);
        assert!((num(&fit.coefficients[1]) + 2.0).abs() < 1e-9);
        assert!(
            transformed_fit(&ints(&[1, 2]), &ints(&[-1, 2]), FitKind::Exponential, "x").is_err()
        );
        let cubic: Vec<f64> = xs.iter().map(|x| 0.5 * x * x * x - x + 0.25).collect();
        let fit = poly_fit(&floats(&xs), &floats(&cubic), 3, "x").unwrap();
        assert!((num(&fit.coefficients[3]) - 0.5).abs() < 1e-9);
        assert!((num(&fit.coefficients[0]) - 0.25).abs() < 1e-9);
    }

    #[test]
    fn correlation_and_quartiles() {
        let r = correlation(&ints(&[1, 2, 3]), &ints(&[2, 4, 7])).unwrap();
        assert!((num(&r) - 0.993_399_267_798_783).abs() < 1e-12);
        assert_eq!(
            correlation(&ints(&[1, 2, 3]), &ints(&[6, 4, 2])).unwrap(),
            Expr::int(-1)
        );
        assert!(correlation(&ints(&[1, 2]), &ints(&[3, 3])).is_err());
        let (q1, q2, q3) = quartiles(&ints(&[6, 7, 15, 36, 39, 40, 41, 42, 43, 47, 49])).unwrap();
        assert_eq!((q1, q2, q3), (Expr::int(15), Expr::int(40), Expr::int(43)));
        let (q1, _, q3) = quartiles(&ints(&[1, 2, 3, 4])).unwrap();
        assert_eq!((q1, q3), (Expr::rational(3, 2), Expr::rational(7, 2)));
        let mut ctx = Ctx::new();
        let iqr = apply("iqr", &[tuple(ints(&[1, 2, 3, 4]))], &mut ctx).unwrap();
        assert_eq!(iqr, Expr::int(2));
        assert!(quartiles(&ints(&[1])).is_err());
    }

    #[test]
    fn domain_operators() {
        let mut ctx = Ctx::new();
        let m = apply(
            "linreg",
            &[tuple(ints(&[0, 1, 2])), tuple(ints(&[1, 3, 5]))],
            &mut ctx,
        )
        .unwrap();
        assert_eq!(m, (Expr::int(2) * Expr::sym("x") + Expr::one()).simplify());
        assert!(ctx.notes.iter().any(|n| n.contains("R² = 1")));
        assert!(apply(
            "polyreg",
            &[tuple(ints(&[0, 1])), tuple(ints(&[1, 3]))],
            &mut ctx
        )
        .is_err());
        let flat = apply(
            "linreg",
            &[tuple(ints(&[0, 1])), tuple(ints(&[2, 2]))],
            &mut ctx,
        )
        .unwrap();
        assert_eq!(flat, Expr::int(2));
        assert!(ctx.notes.iter().any(|n| n.contains("undefined")));
    }

    proptest! {
        #[test]
        fn linear_fit_recovers_exact_lines(a in -50i64..50, b in -50i64..50, n in 2usize..12) {
            let xs: Vec<Expr> = (0..n as i64).map(Expr::int).collect();
            let ys: Vec<Expr> = (0..n as i64).map(|x| Expr::int(a * x + b)).collect();
            let fit = poly_fit(&xs, &ys, 1, "x").unwrap();
            prop_assert_eq!(fit.coefficients, vec![Expr::int(b), Expr::int(a)]);
        }
    }
}
