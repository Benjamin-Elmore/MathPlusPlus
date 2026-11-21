//! Probability distributions and confidence intervals.
//!
//! Normal, binomial, Poisson, Student t and chi-squared: densities (or probability mass
//! functions), cumulative distribution functions and their inverses (quantiles).
//!
//! Discrete distributions with exact parameters give exact answers: `binompdf(10, 1/2, 5)`
//! is `63/256`, and a Poisson probability with a rational mean is a rational multiple of
//! `e^{-λ}`. Continuous distributions are computed in `f64` from the special functions
//! below (log-gamma, the regularised incomplete gamma and beta functions, `erf`), which
//! are accurate to a few units in the last place over the ranges used here. Iterations
//! are capped, and a computation that does not converge is an error rather than an
//! inaccurate number.

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};

use crate::domains::util::{self, arity, items, rat_expr};
use crate::domains::{Ctx, Domain, Value};
use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode};

/// The distributions domain.
pub struct Distributions;

const OPS: &[&str] = &[
    "normalpdf",
    "normalcdf",
    "invnorm",
    "binompdf",
    "binomcdf",
    "poissonpdf",
    "poissoncdf",
    "tpdf",
    "tcdf",
    "invt",
    "chi2pdf",
    "chi2cdf",
    "invchi2",
    "ci",
];

/// Machine precision used as the convergence target.
const EPS: f64 = 1e-16;
/// Smallest number used to avoid division by zero in continued fractions.
const FPMIN: f64 = 1e-300;
/// Most iterations of a series or continued fraction.
const MAX_ITER: usize = 100_000;
/// Largest `n` (or `k`) for which discrete probabilities are summed exactly.
const MAX_EXACT: u64 = 2_000;

fn domain(function: &str, detail: impl Into<String>) -> MathError {
    MathError::Domain {
        function: function.into(),
        detail: detail.into(),
    }
}

fn no_convergence(what: &str) -> MathError {
    MathError::Inconclusive(format!("{what} did not converge"))
}

/// A finite `f64` as an expression, or an error.
fn float(op: &str, x: f64) -> MathResult<Expr> {
    if x.is_finite() {
        Ok(Expr::float(x))
    } else {
        Err(domain(op, "the result is not a finite number"))
    }
}

// ---- special functions ------------------------------------------------------

const LANCZOS_G: f64 = 7.0;
const LANCZOS: [f64; 9] = [
    0.999_999_999_999_809_9,
    676.520_368_121_885_1,
    -1_259.139_216_722_402_8,
    771.323_428_777_653_1,
    -176.615_029_162_140_6,
    12.507_343_278_686_905,
    -0.138_571_095_265_720_12,
    9.984_369_578_019_572e-6,
    1.505_632_735_149_311_6e-7,
];

/// `ln Γ(x)` for real `x` that is not a non-positive integer (Lanczos, g = 7).
pub fn ln_gamma(x: f64) -> f64 {
    if x < 0.5 {
        // Reflection: Γ(x) Γ(1 − x) = π / sin(πx).
        let s = (std::f64::consts::PI * x).sin().abs();
        return (std::f64::consts::PI / s).ln() - ln_gamma(1.0 - x);
    }
    let x = x - 1.0;
    let mut a = LANCZOS[0];
    for (i, c) in LANCZOS.iter().enumerate().skip(1) {
        a += c / (x + i as f64);
    }
    let t = x + LANCZOS_G + 0.5;
    0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
}

/// The regularised lower incomplete gamma function `P(a, x)`, `a > 0`, `x ≥ 0`.
pub fn gamma_p(a: f64, x: f64) -> MathResult<f64> {
    if a <= 0.0 || x < 0.0 || !a.is_finite() || x.is_nan() {
        return Err(domain(
            "the incomplete gamma function",
            "needs a > 0 and x ≥ 0",
        ));
    }
    if x == 0.0 {
        return Ok(0.0);
    }
    if x.is_infinite() {
        return Ok(1.0);
    }
    if x < a + 1.0 {
        gamma_series(a, x)
    } else {
        Ok(1.0 - gamma_cf(a, x)?)
    }
}

/// The regularised upper incomplete gamma function `Q(a, x) = 1 − P(a, x)`.
pub fn gamma_q(a: f64, x: f64) -> MathResult<f64> {
    if a <= 0.0 || x < 0.0 || !a.is_finite() || x.is_nan() {
        return Err(domain(
            "the incomplete gamma function",
            "needs a > 0 and x ≥ 0",
        ));
    }
    if x == 0.0 {
        return Ok(1.0);
    }
    if x.is_infinite() {
        return Ok(0.0);
    }
    if x < a + 1.0 {
        Ok(1.0 - gamma_series(a, x)?)
    } else {
        gamma_cf(a, x)
    }
}

fn gamma_prefactor(a: f64, x: f64) -> f64 {
    (-x + a * x.ln() - ln_gamma(a)).exp()
}

fn gamma_series(a: f64, x: f64) -> MathResult<f64> {
    let mut ap = a;
    let mut del = 1.0 / a;
    let mut sum = del;
    for _ in 0..MAX_ITER {
        ap += 1.0;
        del *= x / ap;
        sum += del;
        if del.abs() < sum.abs() * EPS {
            return Ok((sum * gamma_prefactor(a, x)).clamp(0.0, 1.0));
        }
    }
    Err(no_convergence("the incomplete gamma series"))
}

fn gamma_cf(a: f64, x: f64) -> MathResult<f64> {
    let mut b = x + 1.0 - a;
    let mut c = 1.0 / FPMIN;
    let mut d = 1.0 / b;
    let mut h = d;
    for i in 1..MAX_ITER {
        let an = -(i as f64) * (i as f64 - a);
        b += 2.0;
        d = an * d + b;
        if d.abs() < FPMIN {
            d = FPMIN;
        }
        c = b + an / c;
        if c.abs() < FPMIN {
            c = FPMIN;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < EPS {
            return Ok((gamma_prefactor(a, x) * h).clamp(0.0, 1.0));
        }
    }
    Err(no_convergence("the incomplete gamma continued fraction"))
}

/// The regularised incomplete beta function `I_x(a, b)`, `a, b > 0`, `0 ≤ x ≤ 1`.
pub fn beta_inc(a: f64, b: f64, x: f64) -> MathResult<f64> {
    if a <= 0.0 || b <= 0.0 || !(0.0..=1.0).contains(&x) || !a.is_finite() || !b.is_finite() {
        return Err(domain(
            "the incomplete beta function",
            "needs a, b > 0 and 0 ≤ x ≤ 1",
        ));
    }
    if x == 0.0 || x == 1.0 {
        return Ok(x);
    }
    let bt = (ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b) + a * x.ln() + b * (1.0 - x).ln()).exp();
    if x < (a + 1.0) / (a + b + 2.0) {
        Ok((bt * beta_cf(a, b, x)? / a).clamp(0.0, 1.0))
    } else {
        Ok((1.0 - bt * beta_cf(b, a, 1.0 - x)? / b).clamp(0.0, 1.0))
    }
}

fn beta_cf(a: f64, b: f64, x: f64) -> MathResult<f64> {
    let qab = a + b;
    let qap = a + 1.0;
    let qam = a - 1.0;
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < FPMIN {
        d = FPMIN;
    }
    d = 1.0 / d;
    let mut h = d;
    for m in 1..MAX_ITER {
        let m = m as f64;
        let m2 = 2.0 * m;
        let aa = m * (b - m) * x / ((qam + m2) * (a + m2));
        d = 1.0 + aa * d;
        if d.abs() < FPMIN {
            d = FPMIN;
        }
        c = 1.0 + aa / c;
        if c.abs() < FPMIN {
            c = FPMIN;
        }
        d = 1.0 / d;
        h *= d * c;
        let aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2));
        d = 1.0 + aa * d;
        if d.abs() < FPMIN {
            d = FPMIN;
        }
        c = 1.0 + aa / c;
        if c.abs() < FPMIN {
            c = FPMIN;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < EPS {
            return Ok(h);
        }
    }
    Err(no_convergence("the incomplete beta continued fraction"))
}

/// The complementary error function.
pub fn erfc(x: f64) -> MathResult<f64> {
    if x.is_nan() {
        return Err(domain("erfc", "the argument is not a number"));
    }
    if x < 0.0 {
        return Ok(2.0 - erfc(-x)?);
    }
    gamma_q(0.5, x * x)
}

/// The error function.
pub fn erf(x: f64) -> MathResult<f64> {
    if x.is_nan() {
        return Err(domain("erf", "the argument is not a number"));
    }
    if x.abs() < 0.5 {
        let p = gamma_p(0.5, x * x)?;
        Ok(if x < 0.0 { -p } else { p })
    } else {
        Ok(1.0 - erfc(x)?)
    }
}

// ---- normal ------------------------------------------------------------------

/// The standard normal density φ(z).
pub fn std_normal_pdf(z: f64) -> f64 {
    (-0.5 * z * z).exp() / (2.0 * std::f64::consts::PI).sqrt()
}

/// The standard normal distribution function Φ(z).
pub fn std_normal_cdf(z: f64) -> MathResult<f64> {
    if z == f64::INFINITY {
        return Ok(1.0);
    }
    if z == f64::NEG_INFINITY {
        return Ok(0.0);
    }
    Ok(0.5 * erfc(-z / std::f64::consts::SQRT_2)?)
}

/// The standard normal quantile Φ⁻¹(p), `0 < p < 1` (Acklam's approximation refined by
/// Halley steps on Φ).
pub fn std_normal_inv(p: f64) -> MathResult<f64> {
    check_probability("invnorm", p)?;
    if p > 0.5 {
        return Ok(-std_normal_inv(1.0 - p)?);
    }
    const A: [f64; 6] = [
        -3.969_683_028_665_376e1,
        2.209_460_984_245_205e2,
        -2.759_285_104_469_687e2,
        1.383_577_518_672_69e2,
        -3.066_479_806_614_716e1,
        2.506_628_277_459_239,
    ];
    const B: [f64; 5] = [
        -5.447_609_879_822_406e1,
        1.615_858_368_580_409e2,
        -1.556_989_798_598_866e2,
        6.680_131_188_771_972e1,
        -1.328_068_155_288_572e1,
    ];
    const C: [f64; 6] = [
        -7.784_894_002_430_293e-3,
        -3.223_964_580_411_365e-1,
        -2.400_758_277_161_838,
        -2.549_732_539_343_734,
        4.374_664_141_464_968,
        2.938_163_982_698_783,
    ];
    const D: [f64; 4] = [
        7.784_695_709_041_462e-3,
        3.224_671_290_700_398e-1,
        2.445_134_137_142_996,
        3.754_408_661_907_416,
    ];
    const P_LOW: f64 = 0.024_25;
    let mut x = if p < P_LOW {
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    };
    for _ in 0..3 {
        let e = std_normal_cdf(x)? - p;
        let u = e * (2.0 * std::f64::consts::PI).sqrt() * (0.5 * x * x).exp();
        let step = u / (1.0 + x * u / 2.0);
        if !step.is_finite() {
            break;
        }
        x -= step;
    }
    Ok(x)
}

// ---- Student t and chi-squared -------------------------------------------------

/// The density of Student's t with `nu` degrees of freedom.
pub fn t_pdf(t: f64, nu: f64) -> MathResult<f64> {
    check_dof("tpdf", nu)?;
    let ln = ln_gamma((nu + 1.0) / 2.0)
        - ln_gamma(nu / 2.0)
        - 0.5 * (nu * std::f64::consts::PI).ln()
        - (nu + 1.0) / 2.0 * (1.0 + t * t / nu).ln();
    Ok(ln.exp())
}

/// The lower tail `P(T ≤ t)` of Student's t.
pub fn t_cdf(t: f64, nu: f64) -> MathResult<f64> {
    check_dof("tcdf", nu)?;
    if t.is_nan() {
        return Err(domain("tcdf", "the argument is not a number"));
    }
    if t.is_infinite() {
        return Ok(if t > 0.0 { 1.0 } else { 0.0 });
    }
    if t == 0.0 {
        return Ok(0.5);
    }
    let tail = 0.5 * beta_inc(nu / 2.0, 0.5, nu / (nu + t * t))?;
    Ok(if t > 0.0 { 1.0 - tail } else { tail })
}

/// The quantile of Student's t: the `t` with `P(T ≤ t) = p`.
pub fn t_inv(p: f64, nu: f64) -> MathResult<f64> {
    check_dof("invt", nu)?;
    check_probability("invt", p)?;
    if p == 0.5 {
        return Ok(0.0);
    }
    if p > 0.5 {
        return Ok(-t_inv(1.0 - p, nu)?);
    }
    // Lower tail, p < 1/2: the root is negative and the cdf is accurate there.
    let x0 = std_normal_inv(p)?.min(-1e-3);
    solve_increasing(
        "invt",
        |x| t_cdf(x, nu),
        |x| t_pdf(x, nu),
        p,
        x0,
        None,
        Some(0.0),
    )
}

/// The density of the chi-squared distribution with `k` degrees of freedom.
pub fn chi2_pdf(x: f64, k: f64) -> MathResult<f64> {
    check_dof("chi2pdf", k)?;
    if x < 0.0 {
        return Ok(0.0);
    }
    if x == 0.0 {
        return match k.partial_cmp(&2.0) {
            Some(std::cmp::Ordering::Less) => Err(domain(
                "chi2pdf",
                "the density is infinite at 0 for fewer than 2 degrees of freedom",
            )),
            Some(std::cmp::Ordering::Equal) => Ok(0.5),
            _ => Ok(0.0),
        };
    }
    let h = k / 2.0;
    Ok(((h - 1.0) * x.ln() - x / 2.0 - h * 2f64.ln() - ln_gamma(h)).exp())
}

/// The lower tail `P(X ≤ x)` of the chi-squared distribution.
pub fn chi2_cdf(x: f64, k: f64) -> MathResult<f64> {
    check_dof("chi2cdf", k)?;
    if x.is_nan() {
        return Err(domain("chi2cdf", "the argument is not a number"));
    }
    if x <= 0.0 {
        return Ok(0.0);
    }
    gamma_p(k / 2.0, x / 2.0)
}

/// The quantile of the chi-squared distribution.
pub fn chi2_inv(p: f64, k: f64) -> MathResult<f64> {
    check_dof("invchi2", k)?;
    check_probability("invchi2", p)?;
    // Wilson–Hilferty starting value.
    let z = std_normal_inv(p)?;
    let c = 2.0 / (9.0 * k);
    let x0 = (k * (1.0 - c + z * c.sqrt()).powi(3)).max(1e-8);
    if p <= 0.5 {
        solve_increasing(
            "invchi2",
            |x| chi2_cdf(x, k),
            |x| chi2_pdf(x, k).or(Ok(0.0)),
            p,
            x0,
            Some(0.0),
            None,
        )
    } else {
        // Solve on the upper tail, which keeps its precision near p = 1.
        solve_increasing(
            "invchi2",
            |x| Ok(-gamma_q(k / 2.0, x / 2.0)?),
            |x| chi2_pdf(x, k).or(Ok(0.0)),
            -(1.0 - p),
            x0,
            Some(0.0),
            None,
        )
    }
}

/// Solves `f(x) = target` for an increasing `f` with derivative `df`, starting at `x0`,
/// inside the open bounds `lo`/`hi` when given: bracketing, then Newton steps that fall
/// back to bisection when they leave the bracket.
fn solve_increasing(
    op: &str,
    f: impl Fn(f64) -> MathResult<f64>,
    df: impl Fn(f64) -> MathResult<f64>,
    target: f64,
    x0: f64,
    lo: Option<f64>,
    hi: Option<f64>,
) -> MathResult<f64> {
    // Bracket: a with f(a) ≤ target, b with f(b) ≥ target.
    let mut a = x0;
    let mut b = x0;
    let mut step = x0.abs().max(1.0);
    let mut found = false;
    for _ in 0..2_000 {
        let fa = f(a)?;
        let fb = f(b)?;
        if fa <= target && fb >= target {
            found = true;
            break;
        }
        if fa > target {
            a = match lo {
                Some(l) => l + (a - l) / 2.0,
                None => a - step,
            };
        }
        if fb < target {
            b = match hi {
                Some(h) => h - (h - b) / 2.0,
                None => b + step,
            };
        }
        step *= 2.0;
        if !a.is_finite() || !b.is_finite() {
            break;
        }
    }
    if !found {
        return Err(no_convergence(&format!("the search for the root of {op}")));
    }
    let mut x = x0.clamp(a, b);
    for i in 0..500 {
        let fx = f(x)? - target;
        // At the precision of the cdf itself: nothing better can be found.
        if fx.abs() <= 8.0 * f64::EPSILON * target.abs() {
            return Ok(x);
        }
        if fx < 0.0 {
            a = x;
        } else {
            b = x;
        }
        let d = df(x)?;
        let newton = x - fx / d;
        let next = if i % 4 != 3 && d > 0.0 && newton.is_finite() && newton > a && newton < b {
            newton
        } else {
            a + (b - a) / 2.0
        };
        let tol = 4.0 * f64::EPSILON * next.abs().max(1e-300);
        if (next - x).abs() <= tol || (b - a) <= tol {
            return Ok(next);
        }
        x = next;
    }
    Err(no_convergence(op))
}

fn check_probability(op: &str, p: f64) -> MathResult<()> {
    if p > 0.0 && p < 1.0 {
        Ok(())
    } else {
        Err(domain(
            op,
            format!("the probability must be between 0 and 1, not {p}"),
        ))
    }
}

fn check_dof(op: &str, nu: f64) -> MathResult<()> {
    if nu > 0.0 && nu.is_finite() {
        Ok(())
    } else {
        Err(domain(
            op,
            format!("the degrees of freedom must be positive, not {nu}"),
        ))
    }
}

// ---- binomial and Poisson --------------------------------------------------------

/// A non-negative integer argument.
fn count_arg(op: &str, what: &str, e: &Expr) -> MathResult<u64> {
    util::integer(&e.simplify())
        .filter(|n| !n.is_negative())
        .and_then(|n| n.to_u64())
        .ok_or_else(|| domain(op, format!("{what} must be a whole number ≥ 0, not {e}")))
}

/// An integer argument (possibly negative).
fn int_value(op: &str, what: &str, e: &Expr) -> MathResult<i64> {
    util::integer(&e.simplify())
        .and_then(|n| n.to_i64())
        .ok_or_else(|| domain(op, format!("{what} must be a whole number, not {e}")))
}

/// A probability parameter: exact when it is a rational, else a float, in `[0, 1]`.
enum Prob {
    Exact(BigRational),
    Float(f64),
}

fn prob_arg(op: &str, e: &Expr) -> MathResult<Prob> {
    let e = e.simplify();
    let bad = || {
        domain(
            op,
            format!("the probability must be between 0 and 1, not {e}"),
        )
    };
    if let Some(q) = util::rational(&e) {
        if q.is_negative() || q > BigRational::one() {
            return Err(bad());
        }
        return Ok(Prob::Exact(q));
    }
    let p = util::real_arg(op, &e)?;
    if (0.0..=1.0).contains(&p) {
        Ok(Prob::Float(p))
    } else {
        Err(bad())
    }
}

fn binomial_coeff(n: u64, k: u64) -> BigInt {
    let k = k.min(n - k);
    let mut c = BigInt::one();
    for i in 0..k {
        c = c * BigInt::from(n - i) / BigInt::from(i + 1);
    }
    c
}

fn ratio_pow(q: &BigRational, k: u64) -> BigRational {
    let mut out = BigRational::one();
    for _ in 0..k {
        out *= q;
    }
    out
}

fn ln_choose(n: f64, k: f64) -> f64 {
    ln_gamma(n + 1.0) - ln_gamma(k + 1.0) - ln_gamma(n - k + 1.0)
}

/// `P(X = k)` for `X ~ B(n, p)`: exact for rational `p` and `n ≤ 2000`.
pub fn binom_pmf(n: &Expr, p: &Expr, k: &Expr) -> MathResult<Expr> {
    let op = "binompdf";
    let n = count_arg(op, "the number of trials n", n)?;
    let p = prob_arg(op, p)?;
    let k = int_value(op, "k", k)?;
    if k < 0 || k as u64 > n {
        return Ok(Expr::zero());
    }
    let k = k as u64;
    match p {
        Prob::Exact(q) if n <= MAX_EXACT => {
            let r = BigRational::from_integer(binomial_coeff(n, k))
                * ratio_pow(&q, k)
                * ratio_pow(&(BigRational::one() - &q), n - k);
            Ok(rat_expr(r))
        }
        p => float(op, binom_pmf_f64(n, prob_f64(&p), k)),
    }
}

fn prob_f64(p: &Prob) -> f64 {
    match p {
        Prob::Exact(q) => q.to_f64().unwrap_or(f64::NAN),
        Prob::Float(x) => *x,
    }
}

fn binom_pmf_f64(n: u64, p: f64, k: u64) -> f64 {
    let (nf, kf) = (n as f64, k as f64);
    if p == 0.0 {
        return if k == 0 { 1.0 } else { 0.0 };
    }
    if p == 1.0 {
        return if k == n { 1.0 } else { 0.0 };
    }
    (ln_choose(nf, kf) + kf * p.ln() + (nf - kf) * (1.0 - p).ln()).exp()
}

/// `P(X ≤ k)` for `X ~ B(n, p)`: exact for rational `p` and `n ≤ 2000`.
pub fn binom_cdf(n: &Expr, p: &Expr, k: &Expr) -> MathResult<Expr> {
    let op = "binomcdf";
    let n = count_arg(op, "the number of trials n", n)?;
    let p = prob_arg(op, p)?;
    let k = int_value(op, "k", k)?;
    if k < 0 {
        return Ok(Expr::zero());
    }
    if k as u64 >= n {
        return Ok(Expr::one());
    }
    let k = k as u64;
    match p {
        Prob::Exact(q) if n <= MAX_EXACT => {
            let r = BigRational::one() - &q;
            let mut total = BigRational::zero();
            for j in 0..=k {
                total += BigRational::from_integer(binomial_coeff(n, j))
                    * ratio_pow(&q, j)
                    * ratio_pow(&r, n - j);
            }
            Ok(rat_expr(total))
        }
        p => {
            let p = prob_f64(&p);
            if p == 0.0 {
                return Ok(Expr::one());
            }
            if p == 1.0 {
                return Ok(Expr::zero());
            }
            // P(X ≤ k) = I_{1−p}(n − k, k + 1).
            float(op, beta_inc((n - k) as f64, k as f64 + 1.0, 1.0 - p)?)
        }
    }
}

/// A Poisson mean: exact rational or float, positive.
fn mean_arg(op: &str, e: &Expr) -> MathResult<Prob> {
    let e = e.simplify();
    let bad = || domain(op, format!("the mean λ must be positive, not {e}"));
    if let Some(q) = util::rational(&e) {
        if !q.is_positive() {
            return Err(bad());
        }
        return Ok(Prob::Exact(q));
    }
    let l = util::real_arg(op, &e)?;
    if l > 0.0 {
        Ok(Prob::Float(l))
    } else {
        Err(bad())
    }
}

fn factorial(k: u64) -> BigInt {
    (1..=k).fold(BigInt::one(), |acc, i| acc * BigInt::from(i))
}

/// `e^{-λ}` times an exact rational.
fn times_exp_neg(r: BigRational, lambda: &BigRational) -> Expr {
    (rat_expr(r) * Expr::e().pow(-rat_expr(lambda.clone()))).simplify()
}

/// `P(X = k)` for `X ~ Poisson(λ)`: exact (`λ^k e^{-λ}/k!`) for rational λ.
pub fn poisson_pmf(lambda: &Expr, k: &Expr) -> MathResult<Expr> {
    let op = "poissonpdf";
    let lambda = mean_arg(op, lambda)?;
    let k = int_value(op, "k", k)?;
    if k < 0 {
        return Ok(Expr::zero());
    }
    let k = k as u64;
    match lambda {
        Prob::Exact(l) if k <= MAX_EXACT => {
            let r = ratio_pow(&l, k) / BigRational::from_integer(factorial(k));
            Ok(times_exp_neg(r, &l))
        }
        l => {
            let l = prob_f64(&l);
            let kf = k as f64;
            float(op, (kf * l.ln() - l - ln_gamma(kf + 1.0)).exp())
        }
    }
}

/// `P(X ≤ k)` for `X ~ Poisson(λ)`: exact for rational λ and `k ≤ 2000`.
pub fn poisson_cdf(lambda: &Expr, k: &Expr) -> MathResult<Expr> {
    let op = "poissoncdf";
    let lambda = mean_arg(op, lambda)?;
    let k = int_value(op, "k", k)?;
    if k < 0 {
        return Ok(Expr::zero());
    }
    let k = k as u64;
    match lambda {
        Prob::Exact(l) if k <= MAX_EXACT => {
            let mut term = BigRational::one();
            let mut total = BigRational::one();
            for j in 1..=k {
                term = term * &l / BigRational::from_integer(BigInt::from(j));
                total += &term;
            }
            Ok(times_exp_neg(total, &l))
        }
        l => float(op, gamma_q(k as f64 + 1.0, prob_f64(&l))?),
    }
}

// ---- confidence intervals ----------------------------------------------------------

/// A confidence level: `0.95` or `95` (per cent).
fn level_arg(op: &str, e: &Expr) -> MathResult<f64> {
    let v = util::real_arg(op, &e.simplify())?;
    let level = if (1.0..100.0).contains(&v) {
        v / 100.0
    } else {
        v
    };
    if level > 0.0 && level < 1.0 {
        Ok(level)
    } else {
        Err(domain(
            op,
            format!("the confidence level must be between 0 and 1 (or 0 and 100 %), not {v}"),
        ))
    }
}

fn mean_and_sd(xs: &[f64]) -> (f64, f64) {
    let n = xs.len() as f64;
    let m = xs.iter().sum::<f64>() / n;
    let ss = xs.iter().map(|x| (x - m) * (x - m)).sum::<f64>();
    (m, (ss / (n - 1.0)).sqrt())
}

/// The t-interval for a mean: `x̄ ± t* s/√n` with the sample standard deviation `s`
/// and `n − 1` degrees of freedom. Needs at least two values.
pub fn ci_mean(xs: &[f64], level: f64) -> MathResult<(f64, f64)> {
    if xs.len() < 2 {
        return Err(MathError::Undefined(
            "a confidence interval from fewer than two values".into(),
        ));
    }
    let (m, s) = mean_and_sd(xs);
    let n = xs.len() as f64;
    let t = t_inv(1.0 - (1.0 - level) / 2.0, n - 1.0)?;
    let half = t * s / n.sqrt();
    Ok((m - half, m + half))
}

/// The z-interval for a mean when the population standard deviation σ is known:
/// `x̄ ± z* σ/√n`.
pub fn ci_mean_known_sigma(xs: &[f64], sigma: f64, level: f64) -> MathResult<(f64, f64)> {
    if xs.is_empty() {
        return Err(MathError::Undefined(
            "a confidence interval from no data".into(),
        ));
    }
    if sigma <= 0.0 || !sigma.is_finite() {
        return Err(domain("ci", "the standard deviation σ must be positive"));
    }
    let n = xs.len() as f64;
    let m = xs.iter().sum::<f64>() / n;
    let z = std_normal_inv(1.0 - (1.0 - level) / 2.0)?;
    let half = z * sigma / n.sqrt();
    Ok((m - half, m + half))
}

/// The Wald interval for a proportion: `p̂ ± z* √(p̂(1 − p̂)/n)` with `p̂ = successes/n`.
pub fn ci_proportion(successes: u64, n: u64, level: f64) -> MathResult<(f64, f64)> {
    if n == 0 || successes > n {
        return Err(domain(
            "ci",
            "a proportion needs 0 ≤ successes ≤ n and n > 0",
        ));
    }
    let p = successes as f64 / n as f64;
    let z = std_normal_inv(1.0 - (1.0 - level) / 2.0)?;
    let half = z * (p * (1.0 - p) / n as f64).sqrt();
    Ok((p - half, p + half))
}

fn closed_interval(lo: f64, hi: f64) -> MathResult<Expr> {
    Ok(Expr::new(ExprNode::Interval {
        lo: float("ci", lo)?,
        hi: float("ci", hi)?,
        lo_closed: true,
        hi_closed: true,
    }))
}

fn percent(level: f64) -> String {
    let p = level * 100.0;
    if (p - p.round()).abs() < 1e-9 {
        format!("{}%", p.round())
    } else {
        format!("{p}%")
    }
}

// ---- the domain ----------------------------------------------------------------------

/// A real argument that may be ±∞.
fn real_or_inf(op: &str, e: &Expr) -> MathResult<f64> {
    let e = e.simplify();
    if e.is_infinite() {
        return Ok(match util::compare(&e, &Expr::zero()) {
            Some(std::cmp::Ordering::Less) => f64::NEG_INFINITY,
            _ => f64::INFINITY,
        });
    }
    util::real_arg(op, &e)
}

/// `μ` and `σ` from optional arguments (defaults 0 and 1).
fn mu_sigma(op: &str, rest: &[Expr]) -> MathResult<(f64, f64)> {
    let mu = match rest.first() {
        Some(m) => util::real_arg(op, &m.simplify())?,
        None => 0.0,
    };
    let sigma = match rest.get(1) {
        Some(s) => util::real_arg(op, &s.simplify())?,
        None => 1.0,
    };
    if sigma <= 0.0 {
        return Err(domain(
            op,
            format!("the standard deviation σ must be positive, not {sigma}"),
        ));
    }
    Ok((mu, sigma))
}

fn dof_arg(op: &str, e: &Expr) -> MathResult<f64> {
    let v = util::real_arg(op, &e.simplify())?;
    check_dof(op, v)?;
    Ok(v)
}

fn prob_value(op: &str, e: &Expr) -> MathResult<f64> {
    let p = util::real_arg(op, &e.simplify())?;
    check_probability(op, p)?;
    Ok(p)
}

fn need(op: &str, args: &[Expr], lo: usize, hi: usize) -> MathResult<()> {
    if (lo..=hi).contains(&args.len()) {
        Ok(())
    } else if lo == hi {
        Err(arity(op, &lo.to_string(), args.len()))
    } else {
        Err(arity(op, &format!("{lo} to {hi}"), args.len()))
    }
}

/// Real data from a list argument.
pub fn real_data(op: &str, arg: &Expr) -> MathResult<Vec<f64>> {
    items(op, std::slice::from_ref(arg))?
        .iter()
        .map(|x| util::real_arg(op, &x.simplify()))
        .collect()
}

impl Domain for Distributions {
    fn name(&self) -> &'static str {
        "probability distributions"
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
        "normalpdf" => {
            need(op, args, 1, 3)?;
            let x = util::real_arg(op, &args[0].simplify())?;
            let (mu, sigma) = mu_sigma(op, &args[1..])?;
            float(op, std_normal_pdf((x - mu) / sigma) / sigma)
        }
        "normalcdf" => {
            need(op, args, 1, 4)?;
            if args.len() == 4 {
                let a = real_or_inf(op, &args[0])?;
                let b = real_or_inf(op, &args[1])?;
                let (mu, sigma) = mu_sigma(op, &args[2..])?;
                if a > b {
                    return Err(domain(op, "the lower bound is above the upper bound"));
                }
                let (za, zb) = ((a - mu) / sigma, (b - mu) / sigma);
                // Use the tail that keeps precision: both bounds above the mean → upper tails.
                let p = if za > 0.0 {
                    std_normal_cdf(-za)? - std_normal_cdf(-zb)?
                } else {
                    std_normal_cdf(zb)? - std_normal_cdf(za)?
                };
                float(op, p.max(0.0))
            } else {
                let x = real_or_inf(op, &args[0])?;
                let (mu, sigma) = mu_sigma(op, &args[1..])?;
                float(op, std_normal_cdf((x - mu) / sigma)?)
            }
        }
        "invnorm" => {
            need(op, args, 1, 3)?;
            let p = prob_value(op, &args[0])?;
            let (mu, sigma) = mu_sigma(op, &args[1..])?;
            float(op, mu + sigma * std_normal_inv(p)?)
        }
        "binompdf" => {
            need(op, args, 3, 3)?;
            binom_pmf(&args[0], &args[1], &args[2])
        }
        "binomcdf" => {
            need(op, args, 3, 3)?;
            binom_cdf(&args[0], &args[1], &args[2])
        }
        "poissonpdf" => {
            need(op, args, 2, 2)?;
            poisson_pmf(&args[0], &args[1])
        }
        "poissoncdf" => {
            need(op, args, 2, 2)?;
            poisson_cdf(&args[0], &args[1])
        }
        "tpdf" => {
            need(op, args, 2, 2)?;
            let x = util::real_arg(op, &args[0].simplify())?;
            float(op, t_pdf(x, dof_arg(op, &args[1])?)?)
        }
        "tcdf" => {
            need(op, args, 2, 2)?;
            let x = real_or_inf(op, &args[0])?;
            float(op, t_cdf(x, dof_arg(op, &args[1])?)?)
        }
        "invt" => {
            need(op, args, 2, 2)?;
            let p = prob_value(op, &args[0])?;
            float(op, t_inv(p, dof_arg(op, &args[1])?)?)
        }
        "chi2pdf" => {
            need(op, args, 2, 2)?;
            let x = util::real_arg(op, &args[0].simplify())?;
            float(op, chi2_pdf(x, dof_arg(op, &args[1])?)?)
        }
        "chi2cdf" => {
            need(op, args, 2, 2)?;
            let x = real_or_inf(op, &args[0])?;
            float(op, chi2_cdf(x, dof_arg(op, &args[1])?)?)
        }
        "invchi2" => {
            need(op, args, 2, 2)?;
            let p = prob_value(op, &args[0])?;
            float(op, chi2_inv(p, dof_arg(op, &args[1])?)?)
        }
        "ci" => {
            need(op, args, 1, 2)?;
            let xs = real_data(op, &args[0])?;
            let level = match args.get(1) {
                Some(l) => level_arg(op, l)?,
                None => 0.95,
            };
            let (lo, hi) = ci_mean(&xs, level)?;
            ctx.note(format!(
                "{} t-interval for the mean (sample standard deviation, n − 1 = {} degrees of freedom)",
                percent(level),
                xs.len() - 1
            ));
            closed_interval(lo, hi)
        }
        _ => Err(MathError::UnknownFunction(op.into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1.0)
    }

    fn num(e: &Expr) -> f64 {
        util::numeric(e).unwrap()
    }

    fn call(op: &str, args: &[Expr]) -> MathResult<Expr> {
        apply(op, args, &mut Ctx::new())
    }

    fn f(x: f64) -> Expr {
        Expr::float(x)
    }

    #[test]
    fn special_functions() {
        assert!(close(ln_gamma(5.0), 24f64.ln(), 1e-14));
        assert!(close(
            ln_gamma(0.5),
            std::f64::consts::PI.sqrt().ln(),
            1e-14
        ));
        assert!(close(erf(1.0).unwrap(), 0.842_700_792_949_714_9, 1e-15));
        assert!(close(erfc(3.0).unwrap(), 2.209_049_699_858_544e-5, 1e-13));
        assert!(close(erf(-0.3).unwrap(), -0.328_626_759_459_127_4, 1e-14));
        assert!(close(beta_inc(2.0, 3.0, 0.4).unwrap(), 0.5248, 1e-13));
        assert!(gamma_p(-1.0, 1.0).is_err());
        assert!(beta_inc(1.0, 1.0, 1.5).is_err());
    }

    #[test]
    fn normal() {
        assert!(close(
            num(&call("normalcdf", &[f(1.96)]).unwrap()),
            0.975_002_104_851_780,
            1e-12
        ));
        assert!(close(
            num(&call("invnorm", &[f(0.975)]).unwrap()),
            1.959_963_984_540_054,
            1e-12
        ));
        assert!(close(
            num(&call("normalpdf", &[Expr::zero()]).unwrap()),
            0.398_942_280_401_432_7,
            1e-14
        ));
        // P(−1 < X < 1) for N(0, 1).
        let p = call(
            "normalcdf",
            &[Expr::int(-1), Expr::int(1), Expr::zero(), Expr::one()],
        )
        .unwrap();
        assert!(close(num(&p), 0.682_689_492_137_085_9, 1e-12));
        // IQ: P(X > 130) for N(100, 15).
        let p = call(
            "normalcdf",
            &[
                Expr::int(130),
                Expr::infinity(),
                Expr::int(100),
                Expr::int(15),
            ],
        )
        .unwrap();
        assert!(close(num(&p), 0.022_750_131_948_179_2, 1e-11));
        assert!(close(
            num(&call("invnorm", &[f(0.9), Expr::int(100), Expr::int(15)]).unwrap()),
            119.223_273_483_169_6,
            1e-12
        ));
        assert!(call("invnorm", &[Expr::int(1)]).is_err());
        assert!(call("normalpdf", &[Expr::zero(), Expr::zero(), Expr::int(-1)]).is_err());
        assert!(close(
            std_normal_inv(1e-10).unwrap(),
            -6.361_340_902_404_056,
            1e-12
        ));
    }

    #[test]
    fn student_t_and_chi_squared() {
        assert!(close(
            num(&call("tcdf", &[f(2.228), Expr::int(10)]).unwrap()),
            0.975,
            1e-4
        ));
        assert!(close(
            num(&call("invt", &[f(0.975), Expr::int(10)]).unwrap()),
            2.228_138_851_986_274,
            1e-10
        ));
        assert!(close(
            num(&call("tpdf", &[Expr::zero(), Expr::one()]).unwrap()),
            1.0 / std::f64::consts::PI,
            1e-14
        ));
        assert!(close(
            num(&call("chi2cdf", &[f(3.841), Expr::one()]).unwrap()),
            0.95,
            1e-4
        ));
        assert!(close(
            num(&call("invchi2", &[f(0.95), Expr::int(1)]).unwrap()),
            3.841_458_820_694_124,
            1e-10
        ));
        assert!(close(
            num(&call("invchi2", &[f(0.05), Expr::int(10)]).unwrap()),
            3.940_299_136_119_247,
            1e-10
        ));
        assert!(close(
            num(&call("chi2pdf", &[Expr::int(2), Expr::int(2)]).unwrap()),
            0.5 * (-1f64).exp(),
            1e-14
        ));
        assert!(call("tcdf", &[Expr::one(), Expr::zero()]).is_err());
        assert!(call("chi2pdf", &[Expr::zero(), Expr::one()]).is_err());
    }

    #[test]
    fn discrete_distributions() {
        let half = Expr::rational(1, 2);
        assert_eq!(
            call("binompdf", &[Expr::int(10), half.clone(), Expr::int(5)]).unwrap(),
            Expr::rational(63, 256)
        );
        assert_eq!(
            call("binomcdf", &[Expr::int(3), half.clone(), Expr::int(1)]).unwrap(),
            Expr::rational(1, 2)
        );
        assert_eq!(
            call("binompdf", &[Expr::int(3), half.clone(), Expr::int(7)]).unwrap(),
            Expr::zero()
        );
        let float_cdf = call("binomcdf", &[Expr::int(20), f(0.3), Expr::int(5)]).unwrap();
        assert!(close(num(&float_cdf), 0.416_370_829_447_480_9, 1e-12));
        assert!(call("binompdf", &[Expr::int(3), Expr::int(2), Expr::one()]).is_err());
        assert!(call("binompdf", &[half.clone(), half.clone(), Expr::one()]).is_err());
        let p = call("poissoncdf", &[Expr::int(3), Expr::int(2)]).unwrap();
        assert!(close(num(&p), 0.423_190_081_126_844_6, 1e-12));
        let pmf = call("poissonpdf", &[Expr::int(2), Expr::int(3)]).unwrap();
        assert_eq!(
            pmf,
            (Expr::rational(4, 3) * Expr::e().pow(Expr::int(-2))).simplify()
        );
        let big = call("poissonpdf", &[f(2.5), Expr::int(4)]).unwrap();
        assert!(close(num(&big), 0.133_601_885_781_085_3, 1e-12));
        assert!(call("poissonpdf", &[Expr::zero(), Expr::one()]).is_err());
    }

    #[test]
    fn confidence_intervals() {
        let data = Expr::new(ExprNode::Tuple(
            [12, 15, 11, 14, 13].iter().map(|&k| Expr::int(k)).collect(),
        ));
        let mut ctx = Ctx::new();
        let ci = apply("ci", &[data, f(0.95)], &mut ctx).unwrap();
        let ExprNode::Interval { lo, hi, .. } = ci.node() else {
            panic!("not an interval: {ci}")
        };
        // 13 ± 2.776445 · √2.5/√5
        let half = 2.776_445_105_197_799 * (2.5f64).sqrt() / 5f64.sqrt();
        assert!(close(num(lo), 13.0 - half, 1e-10));
        assert!(close(num(hi), 13.0 + half, 1e-10));
        assert!(ctx.notes[0].contains("95% t-interval"));
        let (a, b) = ci_proportion(40, 100, 0.95).unwrap();
        assert!(close(
            a,
            0.4 - 1.959_963_984_540_054 * 0.048_989_794_855_663_56,
            1e-12
        ));
        assert!(b > a);
        let (a, b) = ci_mean_known_sigma(&[10.0, 12.0], 2.0, 0.95).unwrap();
        assert!(close(
            b - a,
            2.0 * 1.959_963_984_540_054 * 2.0 / 2f64.sqrt(),
            1e-12
        ));
        assert!(ci_mean(&[1.0], 0.95).is_err());
        assert!(apply("ci", &[Expr::int(1), Expr::int(200)], &mut Ctx::new()).is_err());
    }

    proptest! {
        #[test]
        fn normal_round_trip(p in 1e-12f64..(1.0 - 1e-12)) {
            let x = std_normal_inv(p).unwrap();
            let back = std_normal_cdf(x).unwrap();
            prop_assert!((back - p).abs() <= 1e-13 * p.min(1.0 - p).max(1e-300) + 1e-15);
        }

        #[test]
        fn t_round_trip(p in 1e-9f64..(1.0 - 1e-9), nu in 0.5f64..200.0) {
            let x = t_inv(p, nu).unwrap();
            let back = t_cdf(x, nu).unwrap();
            prop_assert!((back - p).abs() <= 1e-10 * p.min(1.0 - p) + 1e-15, "{} {} {}", p, nu, back);
        }

        #[test]
        fn chi2_round_trip(p in 1e-9f64..(1.0 - 1e-9), k in 0.5f64..200.0) {
            let x = chi2_inv(p, k).unwrap();
            let back = chi2_cdf(x, k).unwrap();
            prop_assert!((back - p).abs() <= 1e-10 * p.min(1.0 - p) + 1e-15, "{} {} {}", p, k, back);
        }
    }
}
