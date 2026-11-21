//! Numerical solutions for initial value problems without a closed form: the equation is
//! written as a first-order system `Y′ = F(x, Y)` and integrated by the adaptive
//! Dormand–Prince 5(4) method (RK45). When the step size is limited by stability rather
//! than accuracy (a stiff equation) the solver switches to an L-stable Rosenbrock method
//! (ROS2) with a numerical Jacobian. The result is a sampled solution for graphing.

use std::collections::BTreeMap;

use crate::domains::solve::isolate::isolate;
use crate::error::{MathError, MathResult};
use crate::expr::Expr;
use crate::simplify::simplify;
use crate::steps::Steps;

use super::parse::Ode;
use super::Condition;
use crate::tools_b::util::value;

/// Most accepted steps.
pub const MAX_STEPS: usize = 200_000;
/// Most sample points returned.
pub const MAX_SAMPLES: usize = 2_000;
/// Default relative and absolute tolerance.
pub const TOLERANCE: f64 = 1e-8;

/// A sampled numerical solution.
#[derive(Clone, Debug, PartialEq)]
pub struct NumericSolution {
    /// The method(s) used.
    pub method: String,
    /// True when stiffness was detected and the implicit method was used.
    pub stiff: bool,
    /// `(x, y)` samples.
    pub points: Vec<(f64, f64)>,
    /// Accepted steps.
    pub steps: usize,
}

impl NumericSolution {
    /// `y` at the last point.
    pub fn last(&self) -> Option<(f64, f64)> {
        self.points.last().copied()
    }

    /// `y` at `x` by linear interpolation between samples.
    pub fn at(&self, x: f64) -> Option<f64> {
        let i = self
            .points
            .windows(2)
            .position(|w| (w[0].0 <= x && x <= w[1].0) || (w[1].0 <= x && x <= w[0].0))?;
        let (a, b) = (self.points[i], self.points[i + 1]);
        if a.0 == b.0 {
            return Some(a.1);
        }
        Some(a.1 + (b.1 - a.1) * (x - a.0) / (b.0 - a.0))
    }
}

/// The system `Y′ = F(x, Y)` of an n-th order equation, `Y = (y, y′, …, y⁽ⁿ⁻¹⁾)`.
struct System {
    x: String,
    names: Vec<String>,
    highest: Expr,
}

impl System {
    fn new(ode: &Ode) -> MathResult<System> {
        let top = ode.d_name(ode.order);
        let highest = match crate::poly::as_poly(&ode.residual, &top) {
            Some(c) if c.len() == 2 && !c[1].contains_sym(&top) => {
                simplify(&(-(c[0].clone()) / c[1].clone()))
            }
            _ => {
                let iso = isolate(&ode.residual, &Expr::zero(), &top, "k")?.ok_or_else(|| {
                    MathError::Unsupported(format!(
                        "a numerical solution: {} cannot be solved for {top}",
                        ode.original
                    ))
                })?;
                match iso.values.as_slice() {
                    [one] => one.clone(),
                    _ => {
                        return Err(MathError::Unsupported(format!(
                            "a numerical solution: {top} has several branches",
                        )))
                    }
                }
            }
        };
        let names = (0..ode.order).map(|k| ode.d_name(k)).collect();
        Ok(System {
            x: ode.x.clone(),
            names,
            highest,
        })
    }

    fn eval(&self, x: f64, y: &[f64], out: &mut [f64]) -> Option<()> {
        let n = y.len();
        out[..n - 1].copy_from_slice(&y[1..]);
        let mut env: BTreeMap<String, f64> = BTreeMap::new();
        env.insert(self.x.clone(), x);
        for (name, v) in self.names.iter().zip(y) {
            env.insert(name.clone(), *v);
        }
        let top = self.highest.eval_f64(&env).ok().filter(|v| v.is_finite())?;
        out[n - 1] = top;
        Some(())
    }
}

/// Solves the initial value problem numerically from the conditions (all at one point,
/// one for each `y⁽ᵏ⁾`, `k < n`) up to `x_end`.
pub fn solve_numeric(
    ode: &Ode,
    conditions: &[Condition],
    x_end: f64,
    mut steps: Option<&mut Steps>,
) -> MathResult<NumericSolution> {
    if let Some(s) = ode
        .residual
        .free_symbols()
        .iter()
        .find(|s| ***s != *ode.x && !(0..=ode.order).any(|k| *ode.d_name(k) == ***s))
    {
        return Err(MathError::UnboundVariable(s.to_string()));
    }
    let n = ode.order as usize;
    let x0 = conditions
        .first()
        .and_then(|c| value(&c.at))
        .ok_or_else(|| MathError::Domain {
            function: "a numerical solution".into(),
            detail: "it needs initial conditions at one point".into(),
        })?;
    let mut y0 = vec![f64::NAN; n];
    for c in conditions {
        let at = value(&c.at).unwrap_or(f64::NAN);
        if (at - x0).abs() > 1e-12 || c.order as usize >= n {
            return Err(MathError::Domain {
                function: "a numerical solution".into(),
                detail:
                    "every condition must be at the same point and of order below the equation's"
                        .into(),
            });
        }
        y0[c.order as usize] =
            value(&c.value).ok_or_else(|| MathError::NotANumber(c.value.to_string()))?;
    }
    if y0.iter().any(|v| v.is_nan()) {
        return Err(MathError::Domain {
            function: "a numerical solution".into(),
            detail: format!("it needs y(x₀) … y^({})(x₀)", n - 1),
        });
    }
    if !x_end.is_finite() || x_end == x0 {
        return Err(MathError::Domain {
            function: "a numerical solution".into(),
            detail: "the end point must differ from the start".into(),
        });
    }
    let sys = System::new(ode)?;
    let result = integrate(&sys, x0, &y0, x_end, TOLERANCE)?;
    Steps::record(
        &mut steps,
        "numerical solution",
        &ode.original,
        &Expr::float(result.last().map_or(f64::NAN, |p| p.1)),
        Some(format!(
            "no closed form was found, so the equation was solved numerically from {} = {} to {} with {} ({} steps{}); y({}) ≈ {}",
            ode.x,
            crate::number::format_f64(x0, 8),
            crate::number::format_f64(x_end, 8),
            result.method,
            result.steps,
            if result.stiff { ", stiffness detected" } else { "" },
            crate::number::format_f64(x_end, 8),
            crate::number::format_f64(result.last().map_or(f64::NAN, |p| p.1), 10)
        )),
    );
    Ok(result)
}

// Dormand–Prince 5(4) tableau.
const C: [f64; 7] = [0.0, 0.2, 0.3, 0.8, 8.0 / 9.0, 1.0, 1.0];
const A: [[f64; 6]; 7] = [
    [0.0; 6],
    [0.2, 0.0, 0.0, 0.0, 0.0, 0.0],
    [3.0 / 40.0, 9.0 / 40.0, 0.0, 0.0, 0.0, 0.0],
    [44.0 / 45.0, -56.0 / 15.0, 32.0 / 9.0, 0.0, 0.0, 0.0],
    [
        19372.0 / 6561.0,
        -25360.0 / 2187.0,
        64448.0 / 6561.0,
        -212.0 / 729.0,
        0.0,
        0.0,
    ],
    [
        9017.0 / 3168.0,
        -355.0 / 33.0,
        46732.0 / 5247.0,
        49.0 / 176.0,
        -5103.0 / 18656.0,
        0.0,
    ],
    [
        35.0 / 384.0,
        0.0,
        500.0 / 1113.0,
        125.0 / 192.0,
        -2187.0 / 6784.0,
        11.0 / 84.0,
    ],
];
const B5: [f64; 7] = [
    35.0 / 384.0,
    0.0,
    500.0 / 1113.0,
    125.0 / 192.0,
    -2187.0 / 6784.0,
    11.0 / 84.0,
    0.0,
];
const B4: [f64; 7] = [
    5179.0 / 57600.0,
    0.0,
    7571.0 / 16695.0,
    393.0 / 640.0,
    -92097.0 / 339200.0,
    187.0 / 2100.0,
    1.0 / 40.0,
];

fn norm(err: &[f64], y: &[f64], y_new: &[f64], tol: f64) -> f64 {
    let n = err.len() as f64;
    (err.iter()
        .zip(y.iter().zip(y_new))
        .map(|(e, (a, b))| {
            let sc = tol + tol * a.abs().max(b.abs());
            (e / sc).powi(2)
        })
        .sum::<f64>()
        / n)
        .sqrt()
}

fn integrate(
    sys: &System,
    x0: f64,
    y0: &[f64],
    x_end: f64,
    tol: f64,
) -> MathResult<NumericSolution> {
    let n = y0.len();
    let dir = (x_end - x0).signum();
    let span = (x_end - x0).abs();
    let mut x = x0;
    let mut y = y0.to_vec();
    let mut h = dir * (span * 1e-3).min(0.1);
    let mut k: Vec<Vec<f64>> = vec![vec![0.0; n]; 7];
    let mut points = vec![(x, y[0])];
    let mut accepted = 0;
    let mut stiff_hits: u32 = 0;
    let mut non_stiff: u32 = 0;
    let mut stiff = false;
    let blow_up = || MathError::Domain {
        function: "a numerical solution".into(),
        detail: "the solution is not finite here (it may blow up)".into(),
    };
    // Failing at the starting point is not a blow-up: the equation has no value there.
    sys.eval(x, &y, &mut k[0])
        .ok_or_else(|| super::ivp::not_at_start(x0))?;
    while (x_end - x) * dir > 1e-14 * span.max(1.0) {
        if accepted > MAX_STEPS {
            return Err(MathError::TooLarge(
                "the number of steps for this solution".into(),
            ));
        }
        if (x + h - x_end) * dir > 0.0 {
            h = x_end - x;
        }
        if stiff {
            let (y_new, err) = ros2_step(sys, x, &y, h).ok_or_else(blow_up)?;
            let e = norm(&err, &y, &y_new, tol * 10.0);
            if e <= 1.0 {
                x += h;
                y = y_new;
                accepted += 1;
                points.push((x, y[0]));
            }
            let fac = (0.9 / e.max(1e-10).sqrt()).clamp(0.2, 4.0);
            h *= fac;
            if h.abs() < 1e-14 * span.max(1.0) {
                return Err(blow_up());
            }
            continue;
        }
        let mut tmp = vec![0.0; n];
        let mut g6 = vec![0.0; n];
        for s in 1..7 {
            for i in 0..n {
                tmp[i] = y[i] + h * (0..s).map(|j| A[s][j] * k[j][i]).sum::<f64>();
            }
            if s == 5 {
                g6.copy_from_slice(&tmp);
            }
            let (_, tail) = k.split_at_mut(s);
            sys.eval(x + C[s] * h, &tmp, &mut tail[0])
                .ok_or_else(blow_up)?;
        }
        let y5: Vec<f64> = (0..n)
            .map(|i| y[i] + h * (0..7).map(|j| B5[j] * k[j][i]).sum::<f64>())
            .collect();
        let err: Vec<f64> = (0..n)
            .map(|i| h * (0..7).map(|j| (B5[j] - B4[j]) * k[j][i]).sum::<f64>())
            .collect();
        let e = norm(&err, &y, &y5, tol);
        // Stiffness test (Hairer): h·ρ with ρ ≈ ‖k₇ − k₆‖/‖y₇ − y₆‖.
        let num: f64 = (0..n)
            .map(|i| (k[6][i] - k[5][i]).powi(2))
            .sum::<f64>()
            .sqrt();
        let den: f64 = (0..n).map(|i| (y5[i] - g6[i]).powi(2)).sum::<f64>().sqrt();
        if e <= 1.0 {
            // Stability rather than accuracy limits the step: hρ near the stability
            // boundary, with many steps still to go.
            let remaining = ((x_end - x) / h).abs();
            if den > 0.0 && (h * num / den).abs() > 1.0 && remaining > 2_000.0 {
                non_stiff = 0;
                stiff_hits += 1;
                if stiff_hits >= 15 {
                    stiff = true;
                }
            } else {
                non_stiff += 1;
                if non_stiff >= 6 {
                    stiff_hits = 0;
                }
            }
            x += h;
            y = y5;
            let last = k[6].clone();
            k[0] = last;
            accepted += 1;
            points.push((x, y[0]));
        }
        let fac = (0.9 * e.max(1e-10).powf(-0.2)).clamp(0.2, 5.0);
        h *= fac;
        if h.abs() < 1e-14 * span.max(1.0) {
            return Err(blow_up());
        }
    }
    let points = thin(points);
    Ok(NumericSolution {
        method: if stiff {
            "adaptive Dormand–Prince RK45, then the Rosenbrock method ROS2 (stiff)".into()
        } else {
            "adaptive Dormand–Prince RK45".into()
        },
        stiff,
        points,
        steps: accepted,
    })
}

fn thin(points: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    if points.len() <= MAX_SAMPLES {
        return points;
    }
    let stride = points.len().div_ceil(MAX_SAMPLES);
    let last = points.last().copied();
    let mut out: Vec<(f64, f64)> = points.into_iter().step_by(stride).collect();
    if out.last().copied() != last {
        out.extend(last);
    }
    out
}

/// One ROS2 step (γ = 1 + 1/√2) with a numerical Jacobian; returns the new value and an
/// error estimate (difference from the embedded first-order solution).
fn ros2_step(sys: &System, x: f64, y: &[f64], h: f64) -> Option<(Vec<f64>, Vec<f64>)> {
    let n = y.len();
    let gamma = 1.0 + 1.0 / 2f64.sqrt();
    let mut f0 = vec![0.0; n];
    sys.eval(x, y, &mut f0)?;
    // Jacobian by forward differences, and ∂F/∂x.
    let mut jac = vec![vec![0.0; n]; n];
    let mut fy = vec![0.0; n];
    for j in 0..n {
        let d = 1e-7 * y[j].abs().max(1.0);
        let mut yp = y.to_vec();
        yp[j] += d;
        sys.eval(x, &yp, &mut fy)?;
        for i in 0..n {
            jac[i][j] = (fy[i] - f0[i]) / d;
        }
    }
    let dx = 1e-7 * x.abs().max(1.0);
    let mut fx = vec![0.0; n];
    sys.eval(x + dx, y, &mut fx)?;
    let ft: Vec<f64> = (0..n).map(|i| (fx[i] - f0[i]) / dx).collect();
    // M = I − γ h J
    let m: Vec<Vec<f64>> = (0..n)
        .map(|i| {
            (0..n)
                .map(|j| if i == j { 1.0 } else { 0.0 } - gamma * h * jac[i][j])
                .collect()
        })
        .collect();
    let rhs1: Vec<f64> = (0..n).map(|i| f0[i] + gamma * h * ft[i]).collect();
    let k1 = crate::domains::solve::numeric::solve_dense(m.clone(), rhs1)?;
    let y1: Vec<f64> = (0..n).map(|i| y[i] + h * k1[i]).collect();
    let mut f1 = vec![0.0; n];
    sys.eval(x + h, &y1, &mut f1)?;
    let rhs2: Vec<f64> = (0..n)
        .map(|i| f1[i] - 2.0 * k1[i] - gamma * h * ft[i])
        .collect();
    let k2 = crate::domains::solve::numeric::solve_dense(m, rhs2)?;
    let y_new: Vec<f64> = (0..n)
        .map(|i| y[i] + 1.5 * h * k1[i] + 0.5 * h * k2[i])
        .collect();
    let err: Vec<f64> = (0..n).map(|i| 0.5 * h * (k1[i] + k2[i])).collect();
    if y_new.iter().all(|v| v.is_finite()) {
        Some((y_new, err))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools_b::ode::parse::parse;

    fn ode(src: Expr) -> Ode {
        parse(&src, Some("y"), Some("x")).unwrap()
    }

    #[test]
    fn rk45_matches_known_solutions() {
        // y′ = y, y(0) = 1: y(1) = e.
        let o = ode(Expr::equation(Expr::sym("y′"), Expr::sym("y")));
        let s = solve_numeric(
            &o,
            &[Condition::value(Expr::zero(), Expr::one())],
            1.0,
            None,
        )
        .unwrap();
        assert!((s.last().unwrap().1 - std::f64::consts::E).abs() < 1e-6);
        assert!(!s.stiff);
        // y″ = −y, y(0) = 0, y′(0) = 1: sin.
        let o = ode(Expr::equation(Expr::sym("y″"), -Expr::sym("y")));
        let conds = [
            Condition::value(Expr::zero(), Expr::zero()),
            Condition::derivative(1, Expr::zero(), Expr::one()),
        ];
        let s = solve_numeric(&o, &conds, 3.0, None).unwrap();
        assert!((s.last().unwrap().1 - 3f64.sin()).abs() < 1e-6);
        assert!((s.at(1.5).unwrap() - 1.5f64.sin()).abs() < 1e-3);
    }

    #[test]
    fn stiff_equations_switch_methods() {
        // y′ = −1000(y − cos x), y(0) = 0: stiff; y ≈ cos x after the transient.
        let o = ode(Expr::equation(
            Expr::sym("y′"),
            Expr::int(-1000)
                * (Expr::sym("y")
                    - crate::functions::func(crate::functions::Func::Cos, vec![Expr::sym("x")])),
        ));
        let s = solve_numeric(
            &o,
            &[Condition::value(Expr::zero(), Expr::zero())],
            10.0,
            None,
        )
        .unwrap();
        assert!(s.stiff, "{}", s.method);
        let (x, y) = s.last().unwrap();
        // y ≈ cos x + sin x/1000 for large x.
        assert!((y - (x.cos() + x.sin() / 1000.0)).abs() < 1e-3, "{y}");
    }
}
