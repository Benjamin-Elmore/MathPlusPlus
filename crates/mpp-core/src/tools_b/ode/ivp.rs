//! Numerical solutions of initial value problems used as functions: the system
//! `Y′ = F(t, Y)`, `Y(t₀) = Y₀` is kept in an expression,
//!
//! `ivp((F₀, …, Fₙ₋₁), (Y₀₀, …, Y₀ₙ₋₁), t₀, k, t)`,
//!
//! which is the k-th component of the solution at `t`. The right-hand sides are written
//! in the placeholders `#y0 … #yN` for the components and `#t` for the variable. It
//! is a number when `t` is one: the system is integrated by the adaptive
//! Dormand–Prince 5(4) method with a tight tolerance (relative and absolute 10⁻¹²),
//! and its derivative in `t` is the right-hand side at the solution, so the chain rule
//! and conserved quantities work on it symbolically.

use std::collections::BTreeMap;

use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode};

/// The operator name.
pub const IVP: &str = "ivp";
/// The placeholder for the variable in the right-hand sides.
pub const VAR: &str = "#t";

/// The placeholder for component `k`.
pub fn component(k: usize) -> String {
    format!("#y{k}")
}

/// Relative and absolute tolerance of the integration.
pub const TOLERANCE: f64 = 1e-12;
/// Most accepted steps for one integration.
pub const MAX_STEPS: usize = 2_000_000;

/// A system with its starting values, read from the arguments of `ivp`.
#[derive(Clone, Debug, PartialEq)]
pub struct Ivp {
    pub rhs: Vec<Expr>,
    pub y0: Vec<f64>,
    pub t0: f64,
}

impl Ivp {
    /// Reads `(F…), (Y₀…), t₀` (the first three arguments of `ivp`).
    pub fn read(args: &[Expr]) -> Option<(Ivp, usize, Expr)> {
        let [rhs, y0, t0, k, t] = args else {
            return None;
        };
        let (ExprNode::Tuple(rhs), ExprNode::Tuple(y0)) = (rhs.node(), y0.node()) else {
            return None;
        };
        if rhs.len() != y0.len() || rhs.is_empty() {
            return None;
        }
        let y0: Option<Vec<f64>> = y0.iter().map(number).collect();
        let k = k
            .as_number()?
            .as_i64()
            .and_then(|k| usize::try_from(k).ok())?;
        if k >= rhs.len() {
            return None;
        }
        Some((
            Ivp {
                rhs: rhs.clone(),
                y0: y0?,
                t0: number(t0)?,
            },
            k,
            t.clone(),
        ))
    }

    /// `ivp(F, Y₀, t₀, k, t)` as an expression.
    pub fn expr(&self, k: usize, t: Expr) -> Expr {
        Expr::apply(
            IVP,
            vec![
                Expr::new(ExprNode::Tuple(self.rhs.clone())),
                Expr::new(ExprNode::Tuple(
                    self.y0.iter().map(|v| Expr::float(*v)).collect(),
                )),
                Expr::float(self.t0),
                Expr::int(k as i64),
                t,
            ],
        )
    }

    /// A key that identifies the problem (for caching solutions).
    pub fn key(&self) -> String {
        let rhs: Vec<String> = self.rhs.iter().map(|r| r.to_string()).collect();
        let y0: Vec<String> = self.y0.iter().map(|v| format!("{v:e}")).collect();
        format!("{}|{}|{:e}", rhs.join(";"), y0.join(";"), self.t0)
    }

    /// Checks the right-hand sides use only the placeholders, and evaluates them once.
    pub fn validate(&self) -> MathResult<()> {
        let allowed: Vec<String> = (0..self.rhs.len())
            .map(component)
            .chain(std::iter::once(VAR.to_string()))
            .collect();
        for r in &self.rhs {
            if let Some(s) = r
                .free_symbols()
                .iter()
                .find(|s| !allowed.contains(&s.to_string()))
            {
                return Err(MathError::UnboundVariable(s.to_string()));
            }
        }
        let mut out = vec![0.0; self.rhs.len()];
        self.eval(self.t0, &self.y0, &mut out)
            .ok_or_else(|| MathError::NotANumber("the right-hand side at the start".into()))
    }

    /// `F(t, y)` into `out`; `None` when a value is not finite.
    pub fn eval(&self, t: f64, y: &[f64], out: &mut [f64]) -> Option<()> {
        let mut env: BTreeMap<String, f64> = BTreeMap::new();
        env.insert(VAR.to_string(), t);
        for (k, v) in y.iter().enumerate() {
            env.insert(component(k), *v);
        }
        for (o, r) in out.iter_mut().zip(&self.rhs) {
            let v = r.eval_f64(&env).ok().filter(|v| v.is_finite())?;
            *o = v;
        }
        Some(())
    }

    /// The derivative of component `k` at `t`: `F_k` with each component and the
    /// variable replaced by the solution at `t`.
    pub fn derivative(&self, k: usize, t: &Expr) -> Expr {
        // All at once: a solution put in must keep its own placeholders.
        let names: Vec<String> = (0..self.rhs.len()).map(component).collect();
        let mut pairs: Vec<(&str, Expr)> = names
            .iter()
            .enumerate()
            .map(|(j, n)| (n.as_str(), self.expr(j, t.clone())))
            .collect();
        pairs.push((VAR, t.clone()));
        self.rhs[k].subs_all(&pairs)
    }

    /// Integrates from `(t, y)` to `t_end` (either direction), returning every accepted
    /// step `(t, y)` (the last one at `t_end`).
    pub fn integrate(&self, t: f64, y: &[f64], t_end: f64) -> MathResult<Vec<(f64, Vec<f64>)>> {
        dopri(self, t, y, t_end)
    }
}

fn number(e: &Expr) -> Option<f64> {
    e.eval_f64(&crate::eval::EmptyEnv)
        .ok()
        .filter(|v| v.is_finite())
}

// Dormand–Prince 5(4).
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

/// The error for an equation that cannot be evaluated at its initial condition.
pub(crate) fn not_at_start(t0: f64) -> MathError {
    MathError::Domain {
        function: "a numerical solution".into(),
        detail: format!(
            "the equation has no finite value at the initial condition (at {})",
            crate::number::format_f64(t0, 6)
        ),
    }
}

fn dopri(sys: &Ivp, t0: f64, y0: &[f64], t_end: f64) -> MathResult<Vec<(f64, Vec<f64>)>> {
    let n = y0.len();
    let mut out = vec![(t0, y0.to_vec())];
    if t_end == t0 {
        return Ok(out);
    }
    let blow_up = || MathError::Domain {
        function: "a numerical solution".into(),
        detail: "the solution is not finite here (it may blow up)".into(),
    };
    let dir = (t_end - t0).signum();
    let span = (t_end - t0).abs();
    let mut t = t0;
    let mut y = y0.to_vec();
    let mut k: Vec<Vec<f64>> = vec![vec![0.0; n]; 7];
    // Failing at the starting point is not a blow-up: the equation has no value there.
    sys.eval(t, &y, &mut k[0]).ok_or_else(|| not_at_start(t0))?;
    // A first step from the size of the derivative (Hairer's estimate, simplified).
    let d0 = y.iter().map(|v| v * v).sum::<f64>().sqrt();
    let d1 = k[0].iter().map(|v| v * v).sum::<f64>().sqrt();
    let mut h = if d0 > 1e-5 && d1 > 1e-5 {
        0.01 * d0 / d1
    } else {
        1e-6
    };
    h = dir * h.min(span);
    let mut accepted = 0usize;
    let mut tmp = vec![0.0; n];
    while (t_end - t) * dir > 1e-15 * span.max(t.abs()).max(1.0) {
        crate::budget::check()?;
        if accepted > MAX_STEPS {
            return Err(MathError::TooLarge(
                "the number of steps for this solution".into(),
            ));
        }
        let last = (t + h - t_end) * dir >= 0.0;
        if last {
            h = t_end - t;
        }
        for s in 1..7 {
            for i in 0..n {
                tmp[i] = y[i] + h * (0..s).map(|j| A[s][j] * k[j][i]).sum::<f64>();
            }
            let (_, tail) = k.split_at_mut(s);
            sys.eval(t + C[s] * h, &tmp, &mut tail[0])
                .ok_or_else(blow_up)?;
        }
        let y5: Vec<f64> = (0..n)
            .map(|i| y[i] + h * (0..7).map(|j| B5[j] * k[j][i]).sum::<f64>())
            .collect();
        let err = ((0..n)
            .map(|i| {
                let e = h * (0..7).map(|j| (B5[j] - B4[j]) * k[j][i]).sum::<f64>();
                let sc = TOLERANCE + TOLERANCE * y[i].abs().max(y5[i].abs());
                (e / sc).powi(2)
            })
            .sum::<f64>()
            / n as f64)
            .sqrt();
        if !err.is_finite() {
            return Err(blow_up());
        }
        if err <= 1.0 {
            t = if last { t_end } else { t + h };
            y = y5;
            // First same as last: the seventh stage is the next first stage.
            let k6 = k[6].clone();
            k[0] = k6;
            accepted += 1;
            out.push((t, y.clone()));
            if last {
                break;
            }
        }
        let factor = (0.9 * err.max(1e-10).powf(-0.2)).clamp(0.2, 5.0);
        h *= factor;
        if h.abs() < 1e-14 * span.max(t.abs()).max(1.0) {
            return Err(blow_up());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn y(k: usize) -> Expr {
        Expr::sym(&component(k))
    }

    #[test]
    fn harmonic_oscillator_to_high_accuracy() {
        // y0′ = y1, y1′ = −y0: y0 = cos t.
        let sys = Ivp {
            rhs: vec![y(1), -y(0)],
            y0: vec![1.0, 0.0],
            t0: 0.0,
        };
        sys.validate().unwrap();
        let path = sys.integrate(0.0, &[1.0, 0.0], 20.0).unwrap();
        let (t, v) = path.last().unwrap();
        assert_eq!(*t, 20.0);
        assert!((v[0] - 20f64.cos()).abs() < 1e-9, "{}", v[0]);
        // Backwards too.
        let back = sys.integrate(0.0, &[1.0, 0.0], -3.0).unwrap();
        let (_, v) = back.last().unwrap();
        assert!((v[0] - 3f64.cos()).abs() < 1e-10);
    }

    #[test]
    fn reading_and_derivatives() {
        let sys = Ivp {
            rhs: vec![y(1), -y(0)],
            y0: vec![1.0, 0.0],
            t0: 0.0,
        };
        let e = sys.expr(0, Expr::sym("t"));
        let ExprNode::Apply(_, args) = e.node() else {
            panic!("not an application");
        };
        let (back, k, t) = Ivp::read(args).unwrap();
        assert_eq!((back, k), (sys.clone(), 0));
        assert!(t.is_sym("t"));
        // d/dt y0 = y1 at t.
        assert_eq!(
            sys.derivative(0, &Expr::sym("t")),
            sys.expr(1, Expr::sym("t"))
        );
        // A name other than the placeholders is refused.
        let bad = Ivp {
            rhs: vec![Expr::sym("a")],
            y0: vec![1.0],
            t0: 0.0,
        };
        assert!(bad.validate().is_err());
    }
}
