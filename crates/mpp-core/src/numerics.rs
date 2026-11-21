//! Numerical methods with their error estimates: Riemann sums (left, right, midpoint),
//! the trapezoid and Simpson rules, Newton's method (every iteration shown), Euler's
//! method and the classical Runge–Kutta method (RK4), and Taylor polynomials with the
//! Lagrange remainder bound ("how many terms for an error below ε").
//!
//! Sums are exact when every sample value is a rational number; error bounds use the
//! largest value of the needed derivative on the interval, found by dense sampling.

use std::fmt;

use crate::calc::diff::diff;
use crate::calc::integrate::integrate_definite;
use crate::calc::series::series;
use crate::error::{MathError, MathResult};
use crate::expr::Expr;
use crate::number::format_f64;
use crate::simplify::{add, mul, simplify};
use crate::steps::Steps;

use crate::calc_support::{at, integral_holder, max_abs, value};

/// Most subintervals a quadrature rule may use.
pub const MAX_SUBINTERVALS: usize = 1_000_000;
/// Subintervals up to which the sum is formed exactly.
const MAX_EXACT_TERMS: usize = 200;
/// Most iterations of Newton's method.
pub const MAX_NEWTON: usize = 100;
/// Most steps of Euler's method or RK4.
pub const MAX_ODE_STEPS: usize = 1_000_000;
/// Highest Taylor degree tried when looking for enough terms.
pub const MAX_TAYLOR_DEGREE: u32 = 60;

/// A Riemann-sum or Newton–Cotes rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuadratureRule {
    /// Left endpoints.
    Left,
    /// Right endpoints.
    Right,
    /// Midpoints.
    Midpoint,
    /// The trapezoid rule.
    Trapezoid,
    /// Simpson's rule (needs an even number of subintervals).
    Simpson,
}

impl QuadratureRule {
    /// The rule's name.
    pub fn name(self) -> &'static str {
        match self {
            QuadratureRule::Left => "left Riemann sum",
            QuadratureRule::Right => "right Riemann sum",
            QuadratureRule::Midpoint => "midpoint rule",
            QuadratureRule::Trapezoid => "trapezoid rule",
            QuadratureRule::Simpson => "Simpson's rule",
        }
    }

    /// The derivative order in the error bound and the bound's constant `K` in
    /// `|E| ≤ M_k (b − a)^(k+1) / (K nᵏ)`.
    fn error_form(self) -> (u32, f64) {
        match self {
            QuadratureRule::Left | QuadratureRule::Right => (1, 2.0),
            QuadratureRule::Midpoint => (2, 24.0),
            QuadratureRule::Trapezoid => (2, 12.0),
            QuadratureRule::Simpson => (4, 180.0),
        }
    }
}

impl fmt::Display for QuadratureRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The result of a quadrature rule.
#[derive(Clone, Debug, PartialEq)]
pub struct Quadrature {
    /// The rule used.
    pub rule: QuadratureRule,
    /// The approximation (exact when every sample is rational).
    pub value: Expr,
    /// The sample points `(x, f(x))` with their weights already applied to nothing.
    pub samples: Vec<(f64, f64)>,
    /// The bound `|E| ≤ …` from the rule's error formula, when the derivative is finite.
    pub error_bound: Option<f64>,
    /// The largest `|f⁽ᵏ⁾|` on the interval used in the bound.
    pub derivative_max: Option<f64>,
    /// The exact integral, when the CAS finds it.
    pub exact: Option<Expr>,
    /// `|approximation − exact|`, when the exact value is known.
    pub actual_error: Option<f64>,
}

/// Applies `rule` with `n` subintervals to `∫_a^b f d(var)`.
pub fn quadrature_rule(
    f: &Expr,
    var: &str,
    a: &Expr,
    b: &Expr,
    n: usize,
    rule: QuadratureRule,
    mut steps: Option<&mut Steps>,
) -> MathResult<Quadrature> {
    if n == 0 || n > MAX_SUBINTERVALS {
        return Err(MathError::Domain {
            function: rule.name().into(),
            detail: format!("the number of subintervals must be between 1 and {MAX_SUBINTERVALS}"),
        });
    }
    if rule == QuadratureRule::Simpson && n % 2 == 1 {
        return Err(MathError::Domain {
            function: rule.name().into(),
            detail: "Simpson's rule needs an even number of subintervals".into(),
        });
    }
    if let Some(s) = f.free_symbols().iter().find(|s| &***s != var) {
        return Err(MathError::UnboundVariable(s.to_string()));
    }
    let (av, bv) = match (value(a), value(b)) {
        (Some(x), Some(y)) => (x, y),
        _ => {
            return Err(MathError::Unsupported(
                "a quadrature rule with bounds that are not numbers".into(),
            ))
        }
    };
    let dx_exact = simplify(&mul(vec![
        add(vec![b.clone(), mul(vec![Expr::int(-1), a.clone()])]),
        Expr::rational(1, n as i64),
    ]));
    let dx = (bv - av) / n as f64;
    // Nodes (as offsets in units of Δx/2) and weights.
    let mut nodes: Vec<(i64, i64)> = Vec::new();
    match rule {
        QuadratureRule::Left => nodes.extend((0..n as i64).map(|i| (2 * i, 1))),
        QuadratureRule::Right => nodes.extend((1..=n as i64).map(|i| (2 * i, 1))),
        QuadratureRule::Midpoint => nodes.extend((0..n as i64).map(|i| (2 * i + 1, 1))),
        QuadratureRule::Trapezoid => nodes
            .extend((0..=n as i64).map(|i| (2 * i, if i == 0 || i == n as i64 { 1 } else { 2 }))),
        QuadratureRule::Simpson => nodes.extend((0..=n as i64).map(|i| {
            let w = if i == 0 || i == n as i64 {
                1
            } else if i % 2 == 1 {
                4
            } else {
                2
            };
            (2 * i, w)
        })),
    }
    let factor = match rule {
        QuadratureRule::Trapezoid => Expr::rational(1, 2),
        QuadratureRule::Simpson => Expr::rational(1, 3),
        _ => Expr::one(),
    };
    let mut samples = Vec::with_capacity(nodes.len());
    let mut float_sum = 0.0;
    for &(half, w) in &nodes {
        let x = av + dx * half as f64 / 2.0;
        let y = at(f, var, x).ok_or_else(|| {
            MathError::Undefined(format!("the integrand at {var} = {}", format_f64(x, 8)))
        })?;
        samples.push((x, y));
        float_sum += w as f64 * y;
    }
    let exact_sum = if nodes.len() <= MAX_EXACT_TERMS {
        exact_weighted_sum(f, var, a, &dx_exact, &nodes)
    } else {
        None
    };
    let fac = match rule {
        QuadratureRule::Trapezoid => 0.5,
        QuadratureRule::Simpson => 1.0 / 3.0,
        _ => 1.0,
    };
    let approx = fac * dx * float_sum;
    let value_expr = match exact_sum {
        Some(s) => simplify(&mul(vec![factor, dx_exact.clone(), s])),
        None => Expr::float(approx),
    };
    let (k, constant) = rule.error_form();
    let derivative_max = diff(f, var, k, None)
        .ok()
        .and_then(|d| max_abs(&d, var, av, bv));
    let error_bound = derivative_max
        .map(|m| m * (bv - av).abs().powi(k as i32 + 1) / (constant * (n as f64).powi(k as i32)));
    let exact = integrate_definite(f, var, a, b, None).ok();
    let actual_error = exact
        .as_ref()
        .and_then(value)
        .map(|v| (v - value(&value_expr).unwrap_or(approx)).abs());
    let holder = integral_holder(f, var, a, b);
    let formula = match rule {
        QuadratureRule::Left => "Δx·[f(x₀) + f(x₁) + … + f(xₙ₋₁)]",
        QuadratureRule::Right => "Δx·[f(x₁) + f(x₂) + … + f(xₙ)]",
        QuadratureRule::Midpoint => "Δx·[f(m₁) + … + f(mₙ)] with mᵢ the midpoints",
        QuadratureRule::Trapezoid => "(Δx/2)·[f(x₀) + 2f(x₁) + … + 2f(xₙ₋₁) + f(xₙ)]",
        QuadratureRule::Simpson => "(Δx/3)·[f(x₀) + 4f(x₁) + 2f(x₂) + … + 4f(xₙ₋₁) + f(xₙ)]",
    };
    let shown: Vec<String> = samples
        .iter()
        .take(8)
        .map(|(x, y)| format!("f({}) = {}", format_f64(*x, 6), format_f64(*y, 8)))
        .collect();
    let more = if samples.len() > 8 { ", …" } else { "" };
    let mut note = format!(
        "n = {n}, Δx = {dx_exact}; {} = {formula}; {}{more}",
        rule.name(),
        shown.join(", ")
    );
    if let (Some(m), Some(e)) = (derivative_max, error_bound) {
        let (num, den) = match k {
            1 => ("(b − a)²", "2n"),
            2 => ("(b − a)³", if constant == 12.0 { "12n²" } else { "24n²" }),
            _ => ("(b − a)⁵", "180n⁴"),
        };
        note.push_str(&format!(
            "; error bound |E| ≤ M·{num}/({den}) with M = max|f{}| ≈ {} on the interval, so |E| ≤ {}",
            primes(k),
            format_f64(m, 6),
            format_f64(e, 4)
        ));
    }
    if let (Some(ex), Some(err)) = (&exact, actual_error) {
        note.push_str(&format!(
            "; the exact value is {ex}, so the actual error is {}",
            format_f64(err, 4)
        ));
    }
    Steps::record(&mut steps, rule.name(), &holder, &value_expr, Some(note));
    Ok(Quadrature {
        rule,
        value: value_expr,
        samples,
        error_bound,
        derivative_max,
        exact,
        actual_error,
    })
}

fn primes(k: u32) -> String {
    match k {
        1 => "′".into(),
        2 => "″".into(),
        _ => format!("⁽{k}⁾"),
    }
}

/// `Σ w·f(a + (h/2)·Δx)` exactly, when every sample value is a rational number.
fn exact_weighted_sum(
    f: &Expr,
    var: &str,
    a: &Expr,
    dx: &Expr,
    nodes: &[(i64, i64)],
) -> Option<Expr> {
    let mut terms = Vec::with_capacity(nodes.len());
    for &(half, w) in nodes {
        let x = simplify(&add(vec![
            a.clone(),
            mul(vec![Expr::rational(half, 2), dx.clone()]),
        ]));
        let y = simplify(&f.subs(var, &x));
        let rational = y
            .as_number()
            .is_some_and(|n| n.is_exact() && n.as_rational().is_some());
        if !rational {
            return None;
        }
        terms.push(mul(vec![Expr::int(w), y]));
    }
    Some(simplify(&add(terms)))
}

/// One iteration of Newton's method.
#[derive(Clone, Debug, PartialEq)]
pub struct Iterate {
    /// The iteration number (from 0).
    pub n: usize,
    /// `xₙ`.
    pub x: f64,
    /// `f(xₙ)`.
    pub fx: f64,
    /// `f′(xₙ)`.
    pub dfx: f64,
    /// `xₙ₊₁ = xₙ − f(xₙ)/f′(xₙ)`.
    pub next: f64,
}

/// Newton's method for `f = 0` from `x0` with the exact derivative, stopping when a step is
/// below `tol` (relative to `max(1, |x|)`) and `|f|` is small. Returns the root and every
/// iteration.
pub fn newton(
    f: &Expr,
    var: &str,
    x0: f64,
    tol: f64,
    mut steps: Option<&mut Steps>,
) -> MathResult<(f64, Vec<Iterate>)> {
    if !x0.is_finite() {
        return Err(not_finite_start());
    }
    let df = diff(f, var, 1, None)?;
    Steps::record(
        &mut steps,
        "Newton's method",
        f,
        &df,
        Some(format!(
            "xₙ₊₁ = xₙ − f(xₙ)/f′(xₙ) with f′({var}) = {df}, starting at x₀ = {}",
            format_f64(x0, 10)
        )),
    );
    newton_iterate(
        |x| {
            at(f, var, x)
                .ok_or_else(|| MathError::Undefined(format!("f at {var} = {}", format_f64(x, 10))))
        },
        |x| {
            at(&df, var, x)
                .ok_or_else(|| MathError::Undefined(format!("f′ at {var} = {}", format_f64(x, 10))))
        },
        x0,
        tol,
        |it| {
            Steps::record(
                &mut steps,
                format!("iteration {}", it.n + 1),
                &Expr::float(it.x),
                &Expr::float(it.next),
                Some(format!(
                    "x{} = {} − ({})/({}) = {}",
                    subscript(it.n + 1),
                    format_f64(it.x, 12),
                    format_f64(it.fx, 8),
                    format_f64(it.dfx, 8),
                    format_f64(it.next, 12)
                )),
            );
        },
    )
}

fn not_finite_start() -> MathError {
    MathError::Domain {
        function: "Newton's method".into(),
        detail: "the starting point must be a finite number".into(),
    }
}

/// The Newton iteration itself, the one every Newton answer in Math++ comes from:
/// `xₙ₊₁ = xₙ − f(xₙ)/f′(xₙ)` from `x0`, with `f` and `f′` given as functions (the exact
/// derivative for [`newton`], a central difference where the solver polishes roots). Stops
/// when a step is below `tol` (relative to `max(1, |x|)`; a non-positive `tol` means
/// `10⁻¹²`) and `|f|` is small; `each` sees every iteration.
pub fn newton_iterate(
    f: impl Fn(f64) -> MathResult<f64>,
    df: impl Fn(f64) -> MathResult<f64>,
    x0: f64,
    tol: f64,
    mut each: impl FnMut(&Iterate),
) -> MathResult<(f64, Vec<Iterate>)> {
    if !x0.is_finite() {
        return Err(not_finite_start());
    }
    let tol = if tol.is_finite() && tol > 0.0 {
        tol
    } else {
        1e-12
    };
    let mut x = x0;
    let mut table = Vec::new();
    for n in 0..MAX_NEWTON {
        let fx = f(x)?;
        let dfx = df(x)?;
        if dfx == 0.0 {
            if fx == 0.0 {
                // Standing on a root where the tangent is flat (a double root).
                return Ok((x, table));
            }
            return Err(MathError::Domain {
                function: "Newton's method".into(),
                detail: format!(
                    "f′({}) = 0, so the tangent line is flat and the method cannot continue; try another starting point",
                    format_f64(x, 10)
                ),
            });
        }
        let next = x - fx / dfx;
        if !next.is_finite() {
            return Err(MathError::Domain {
                function: "Newton's method".into(),
                detail: "the iterates blew up; try another starting point".into(),
            });
        }
        let it = Iterate {
            n,
            x,
            fx,
            dfx,
            next,
        };
        each(&it);
        table.push(it);
        let small_step = (next - x).abs() <= tol * x.abs().max(1.0);
        x = next;
        if small_step {
            let fx = f(x).unwrap_or(f64::INFINITY);
            let scale = table.iter().map(|it| it.fx.abs()).fold(1.0, f64::max);
            if fx.abs() <= 1e-8 * scale {
                return Ok((x, table));
            }
        }
    }
    Err(MathError::Domain {
        function: "Newton's method".into(),
        detail: format!(
            "the iterates did not settle after {MAX_NEWTON} steps (last {}); try another starting point",
            format_f64(x, 10)
        ),
    })
}

fn subscript(n: usize) -> String {
    const DIGITS: [char; 10] = ['₀', '₁', '₂', '₃', '₄', '₅', '₆', '₇', '₈', '₉'];
    n.to_string()
        .chars()
        .filter_map(|c| c.to_digit(10).map(|d| DIGITS[d as usize]))
        .collect()
}

/// A table of `(x, y)` values from a step-by-step ODE method.
#[derive(Clone, Debug, PartialEq)]
pub struct OdeTable {
    /// The method's name.
    pub method: &'static str,
    /// The rows `(xₙ, yₙ)`, starting at the initial condition.
    pub rows: Vec<(f64, f64)>,
}

impl OdeTable {
    /// The last `y`.
    pub fn last(&self) -> Option<f64> {
        self.rows.last().map(|r| r.1)
    }

    /// The table as a matrix with columns `n`, `xₙ`, `yₙ`.
    pub fn to_matrix(&self) -> Option<Expr> {
        Expr::matrix(
            self.rows
                .iter()
                .enumerate()
                .map(|(i, (x, y))| vec![Expr::int(i as i64), Expr::float(*x), Expr::float(*y)])
                .collect(),
        )
    }
}

fn rhs_at(rhs: &Expr, x: &str, y: &str, xv: f64, yv: f64) -> MathResult<f64> {
    let env: std::collections::BTreeMap<String, f64> = [(x.to_string(), xv), (y.to_string(), yv)]
        .into_iter()
        .collect();
    rhs.eval_f64(&env)
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| {
            MathError::Undefined(format!(
                "the slope at ({}, {})",
                format_f64(xv, 8),
                format_f64(yv, 8)
            ))
        })
}

fn check_ode_args(rhs: &Expr, x: &str, y: &str, h: f64, n: usize) -> MathResult<()> {
    if let Some(s) = rhs.free_symbols().iter().find(|s| &***s != x && &***s != y) {
        return Err(MathError::UnboundVariable(s.to_string()));
    }
    if !(h.is_finite() && h != 0.0) {
        return Err(MathError::Domain {
            function: "a step method".into(),
            detail: "the step size must be a non-zero number".into(),
        });
    }
    if n == 0 || n > MAX_ODE_STEPS {
        return Err(MathError::Domain {
            function: "a step method".into(),
            detail: format!("the number of steps must be between 1 and {MAX_ODE_STEPS}"),
        });
    }
    Ok(())
}

/// Euler's method for `y′ = rhs(x, y)`, `y(x0) = y0`, with `n` steps of size `h`.
#[allow(clippy::too_many_arguments)]
pub fn euler(
    rhs: &Expr,
    x: &str,
    y: &str,
    x0: f64,
    y0: f64,
    h: f64,
    n: usize,
    mut steps: Option<&mut Steps>,
) -> MathResult<OdeTable> {
    check_ode_args(rhs, x, y, h, n)?;
    let mut rows = vec![(x0, y0)];
    let (mut xv, mut yv) = (x0, y0);
    for i in 0..n {
        crate::budget::check()?;
        let slope = rhs_at(rhs, x, y, xv, yv)?;
        let next = yv + h * slope;
        if i < 20 {
            Steps::record(
                &mut steps,
                "Euler step",
                &Expr::float(yv),
                &Expr::float(next),
                Some(format!(
                    "y{} = y{} + h·f(x{}, y{}) = {} + {}·{} = {}",
                    subscript(i + 1),
                    subscript(i),
                    subscript(i),
                    subscript(i),
                    format_f64(yv, 10),
                    format_f64(h, 6),
                    format_f64(slope, 10),
                    format_f64(next, 10)
                )),
            );
        }
        xv = x0 + h * (i + 1) as f64;
        yv = next;
        if !yv.is_finite() {
            return Err(MathError::Domain {
                function: "Euler's method".into(),
                detail: "the solution blew up; try a smaller step".into(),
            });
        }
        rows.push((xv, yv));
    }
    Ok(OdeTable {
        method: "Euler's method",
        rows,
    })
}

/// The classical fourth-order Runge–Kutta method for `y′ = rhs(x, y)`, `y(x0) = y0`.
#[allow(clippy::too_many_arguments)]
pub fn rk4(
    rhs: &Expr,
    x: &str,
    y: &str,
    x0: f64,
    y0: f64,
    h: f64,
    n: usize,
    mut steps: Option<&mut Steps>,
) -> MathResult<OdeTable> {
    check_ode_args(rhs, x, y, h, n)?;
    let each = |i: usize, yv: f64, [k1, k2, k3, k4]: [f64; 4], next: f64| {
        if i < 20 {
            Steps::record(
                &mut steps,
                "RK4 step",
                &Expr::float(yv),
                &Expr::float(next),
                Some(format!(
                    "k₁ = {}, k₂ = {}, k₃ = {}, k₄ = {}; y{} = y{} + (h/6)(k₁ + 2k₂ + 2k₃ + k₄) = {}",
                    format_f64(k1, 8),
                    format_f64(k2, 8),
                    format_f64(k3, 8),
                    format_f64(k4, 8),
                    subscript(i + 1),
                    subscript(i),
                    format_f64(next, 10)
                )),
            );
        }
    };
    let rows = rk4_march(
        |xv, yv| rhs_at(rhs, x, y, xv, yv),
        x0,
        y0,
        h,
        n,
        each,
        |_| MathError::Domain {
            function: "RK4".into(),
            detail: "the solution blew up; try a smaller step".into(),
        },
    )?;
    Ok(OdeTable {
        method: "classical Runge–Kutta (RK4)",
        rows,
    })
}

/// The classical fourth-order Runge–Kutta method for `y′ = slope(x, y)` from `(x0, y0)`,
/// `n` steps of size `h`: every RK4 answer in Math++ comes from here. Returns every point,
/// the initial one included. `each(i, yᵢ, [k₁, k₂, k₃, k₄], yᵢ₊₁)` sees each step (for the
/// working); `blew_up(x)` is the error when the solution stops being finite at `x`.
pub fn rk4_march(
    slope: impl Fn(f64, f64) -> MathResult<f64>,
    x0: f64,
    y0: f64,
    h: f64,
    n: usize,
    mut each: impl FnMut(usize, f64, [f64; 4], f64),
    blew_up: impl Fn(f64) -> MathError,
) -> MathResult<Vec<(f64, f64)>> {
    let mut rows = Vec::with_capacity(n + 1);
    rows.push((x0, y0));
    let (mut xv, mut yv) = (x0, y0);
    for i in 0..n {
        crate::budget::check()?;
        let k1 = slope(xv, yv)?;
        let k2 = slope(xv + h / 2.0, yv + h * k1 / 2.0)?;
        let k3 = slope(xv + h / 2.0, yv + h * k2 / 2.0)?;
        let k4 = slope(xv + h, yv + h * k3)?;
        let next = yv + h / 6.0 * (k1 + 2.0 * k2 + 2.0 * k3 + k4);
        each(i, yv, [k1, k2, k3, k4], next);
        xv = x0 + h * (i + 1) as f64;
        yv = next;
        if !yv.is_finite() {
            return Err(blew_up(xv));
        }
        rows.push((xv, yv));
    }
    Ok(rows)
}

/// A Taylor polynomial with its Lagrange error bound.
#[derive(Clone, Debug, PartialEq)]
pub struct TaylorBound {
    /// The degree `n`.
    pub degree: u32,
    /// `Tₙ(x)` about `a`.
    pub polynomial: Expr,
    /// `f⁽ⁿ⁺¹⁾`.
    pub derivative: Expr,
    /// `M = max |f⁽ⁿ⁺¹⁾|` between `a` and the point.
    pub m: f64,
    /// `M |x − a|ⁿ⁺¹ / (n + 1)!`.
    pub bound: f64,
    /// `|f(x) − Tₙ(x)|`, the true error.
    pub actual: Option<f64>,
}

/// The degree-`n` Taylor polynomial of `f` about `a` and the Lagrange bound on its error at
/// `x`: `|Rₙ(x)| ≤ M |x − a|ⁿ⁺¹/(n + 1)!` with `M = max |f⁽ⁿ⁺¹⁾|` between `a` and `x`.
pub fn taylor_bound(
    f: &Expr,
    var: &str,
    a: &Expr,
    x: &Expr,
    n: u32,
    mut steps: Option<&mut Steps>,
) -> MathResult<TaylorBound> {
    let tb = taylor_bound_quiet(f, var, a, x, n)?;
    Steps::record(
        &mut steps,
        "Lagrange error bound",
        &tb.polynomial,
        &Expr::float(tb.bound),
        Some(format!(
            "T{}({var}) = {} about {var} = {a}; f⁽{}⁾ = {}; M = max|f⁽{}⁾| ≈ {} between {a} and {x}; |R{}({x})| ≤ M·|{x} − {a}|^{}/{}! = {}{}",
            subscript(n as usize),
            tb.polynomial,
            n + 1,
            tb.derivative,
            n + 1,
            format_f64(tb.m, 8),
            subscript(n as usize),
            n + 1,
            n + 1,
            format_f64(tb.bound, 6),
            tb.actual
                .map(|v| format!(" (the actual error is {})", format_f64(v, 6)))
                .unwrap_or_default()
        )),
    );
    Ok(tb)
}

fn taylor_bound_quiet(f: &Expr, var: &str, a: &Expr, x: &Expr, n: u32) -> MathResult<TaylorBound> {
    let (av, xv) = match (value(a), value(x)) {
        (Some(p), Some(q)) => (p, q),
        _ => {
            return Err(MathError::Unsupported(
                "a Taylor bound at a point that is not a number".into(),
            ))
        }
    };
    let s = series(f, var, a, n + 1, None)?;
    let polynomial = s.terms.clone();
    let derivative = diff(f, var, n + 1, None)?;
    let m = max_abs(&derivative, var, av, xv).ok_or_else(|| {
        MathError::Undefined(format!(
            "the bound: f⁽{}⁾ is not finite between {a} and {x}",
            n + 1
        ))
    })?;
    let mut fact = 1.0;
    for k in 1..=(n + 1) {
        fact *= k as f64;
    }
    let bound = m * (xv - av).abs().powi(n as i32 + 1) / fact;
    let actual = match (at(f, var, xv), at(&polynomial, var, xv)) {
        (Some(fx), Some(tx)) => Some((fx - tx).abs()),
        _ => None,
    };
    Ok(TaylorBound {
        degree: n,
        polynomial,
        derivative,
        m,
        bound,
        actual,
    })
}

/// The smallest degree `n` whose Lagrange bound at `x` is below `eps`.
pub fn taylor_terms_needed(
    f: &Expr,
    var: &str,
    a: &Expr,
    x: &Expr,
    eps: f64,
    mut steps: Option<&mut Steps>,
) -> MathResult<TaylorBound> {
    if !(eps.is_finite() && eps > 0.0) {
        return Err(MathError::Domain {
            function: "Taylor terms".into(),
            detail: "the tolerance ε must be a positive number".into(),
        });
    }
    let mut tried = Vec::new();
    for n in 0..=MAX_TAYLOR_DEGREE {
        let tb = taylor_bound_quiet(f, var, a, x, n)?;
        tried.push(format!("n = {n}: bound {}", format_f64(tb.bound, 4)));
        if tb.bound < eps {
            Steps::record(
                &mut steps,
                "terms needed",
                &tb.polynomial,
                &Expr::int(i64::from(n)),
                Some(format!(
                    "the Lagrange bound M·|x − a|ⁿ⁺¹/(n + 1)! falls below ε = {} first at degree n = {n}: {}",
                    format_f64(eps, 4),
                    tried.join("; ")
                )),
            );
            return Ok(tb);
        }
    }
    Err(MathError::Domain {
        function: "Taylor terms".into(),
        detail: format!(
            "the Lagrange bound did not fall below {} by degree {MAX_TAYLOR_DEGREE}",
            format_f64(eps, 4)
        ),
    })
}

/// A short table of the iterates for display: rows `n, xₙ, f(xₙ), xₙ₊₁`.
pub fn newton_matrix(table: &[Iterate]) -> Option<Expr> {
    Expr::matrix(
        table
            .iter()
            .map(|it| {
                vec![
                    Expr::int(it.n as i64),
                    Expr::float(it.x),
                    Expr::float(it.fx),
                    Expr::float(it.next),
                ]
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functions::{func, Func};

    fn x() -> Expr {
        Expr::sym("x")
    }
    fn n(k: i64) -> Expr {
        Expr::int(k)
    }

    #[test]
    fn riemann_sums_are_exact_for_polynomials() {
        let f = x().pow(2);
        let q = |r| quadrature_rule(&f, "x", &n(0), &n(1), 4, r, None).unwrap();
        assert_eq!(q(QuadratureRule::Left).value, Expr::rational(7, 32));
        assert_eq!(q(QuadratureRule::Right).value, Expr::rational(15, 32));
        assert_eq!(q(QuadratureRule::Midpoint).value, Expr::rational(21, 64));
        assert_eq!(q(QuadratureRule::Trapezoid).value, Expr::rational(11, 32));
        // Simpson is exact for cubics.
        let s = quadrature_rule(
            &x().pow(3),
            "x",
            &n(0),
            &n(2),
            2,
            QuadratureRule::Simpson,
            None,
        )
        .unwrap();
        assert_eq!(s.value, n(4));
        assert_eq!(s.actual_error, Some(0.0));
        assert!(quadrature_rule(&f, "x", &n(0), &n(1), 3, QuadratureRule::Simpson, None).is_err());
    }

    #[test]
    fn error_bounds_hold() {
        let f = func(Func::Sin, vec![x()]);
        for rule in [
            QuadratureRule::Left,
            QuadratureRule::Right,
            QuadratureRule::Midpoint,
            QuadratureRule::Trapezoid,
            QuadratureRule::Simpson,
        ] {
            let q = quadrature_rule(&f, "x", &n(0), &Expr::pi(), 10, rule, None).unwrap();
            let bound = q.error_bound.unwrap();
            let actual = q.actual_error.unwrap();
            assert!(actual <= bound * (1.0 + 1e-9), "{rule}: {actual} > {bound}");
        }
        // Trapezoid on sin over [0, π], n = 10: bound π³/1200.
        let q = quadrature_rule(
            &f,
            "x",
            &n(0),
            &Expr::pi(),
            10,
            QuadratureRule::Trapezoid,
            None,
        )
        .unwrap();
        let expected = std::f64::consts::PI.powi(3) / 1200.0;
        assert!((q.error_bound.unwrap() - expected).abs() < 1e-9);
        assert!((value(&q.value).unwrap() - 1.983_523_537_509_455_8).abs() < 1e-12);
    }

    #[test]
    fn newton_iterations() {
        // √2 from x₀ = 1: 1, 1.5, 1.41666…, 1.414215…
        let f = x().pow(2) - n(2);
        let mut steps = Steps::new();
        let (root, table) = newton(&f, "x", 1.0, 1e-14, Some(&mut steps)).unwrap();
        assert!((root - 2f64.sqrt()).abs() < 1e-14);
        assert_eq!(table[0].next, 1.5);
        assert!((table[1].next - 17.0 / 12.0).abs() < 1e-15);
        assert!(table.len() <= 7);
        assert!(steps.iter().any(|s| s.rule == "iteration 1"));
        // A flat tangent stops the method with an explanation.
        assert!(newton(&f, "x", 0.0, 1e-12, None).is_err());
    }

    #[test]
    fn euler_and_rk4_against_the_exact_solution() {
        // y′ = y, y(0) = 1: y(1) = e.
        let y = Expr::sym("y");
        let e1 = euler(&y, "x", "y", 0.0, 1.0, 0.1, 10, None).unwrap();
        assert!((e1.last().unwrap() - 1.1f64.powi(10)).abs() < 1e-12);
        let r = rk4(&y, "x", "y", 0.0, 1.0, 0.1, 10, None).unwrap();
        assert!((r.last().unwrap() - std::f64::consts::E).abs() < 3e-6);
        // RK4 error shrinks like h⁴: halving h cuts it about 16 times.
        let r2 = rk4(&y, "x", "y", 0.0, 1.0, 0.05, 20, None).unwrap();
        let (e_h, e_h2) = (
            (r.last().unwrap() - std::f64::consts::E).abs(),
            (r2.last().unwrap() - std::f64::consts::E).abs(),
        );
        assert!(e_h / e_h2 > 14.0 && e_h / e_h2 < 18.0, "{}", e_h / e_h2);
        assert_eq!(r.rows.len(), 11);
    }

    #[test]
    fn taylor_bounds_and_terms_needed() {
        // eˣ about 0, degree 3, at x = 1: bound e/24.
        let ex = Expr::e().pow(x());
        let tb = taylor_bound(&ex, "x", &n(0), &n(1), 3, None).unwrap();
        assert!((tb.bound - std::f64::consts::E / 24.0).abs() < 1e-9);
        assert!(tb.actual.unwrap() <= tb.bound);
        // sin x at x = 0.5 to within 1e-6: degree 7 (bound 0.5⁸/8! ≈ 9.7e-8; degree 6 gives 1.6e-6... ).
        let s = func(Func::Sin, vec![x()]);
        let need = taylor_terms_needed(&s, "x", &n(0), &Expr::rational(1, 2), 1e-6, None).unwrap();
        assert!(need.bound < 1e-6);
        let prev =
            taylor_bound(&s, "x", &n(0), &Expr::rational(1, 2), need.degree - 1, None).unwrap();
        assert!(prev.bound >= 1e-6);
    }
}
