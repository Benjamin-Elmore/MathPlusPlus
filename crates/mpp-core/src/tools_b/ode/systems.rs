//! Linear systems `x′ = A x + f(t)` with a constant matrix `A`: the matrix exponential
//! `e^{At} = L⁻¹{(sI − A)⁻¹}`, the general solution `x = e^{At} c + ∫₀ᵗ e^{A(t−τ)} f(τ) dτ`
//! (by the same transform), the eigenvalues, and for 2×2 systems the type of the
//! equilibrium with sample trajectories for a phase portrait.

use crate::calc::diff::diff;
use crate::domains::linalg::Matrix;
use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode};
use crate::simplify::{add, mul, simplify};
use crate::steps::Steps;

use super::laplace::{inverse, transform};
use super::parse::{parse, placeholder};
use super::{linsys, tidy};
use crate::tools_b::util::value;

/// Most unknowns in a system.
pub const MAX_SYSTEM: usize = 6;

/// The kind of equilibrium at the origin of a 2×2 system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Equilibrium {
    StableNode,
    UnstableNode,
    Saddle,
    StableSpiral,
    UnstableSpiral,
    Centre,
    /// Repeated or zero eigenvalues (degenerate cases).
    Degenerate,
}

impl Equilibrium {
    /// A short description.
    pub fn name(self) -> &'static str {
        match self {
            Equilibrium::StableNode => "stable node",
            Equilibrium::UnstableNode => "unstable node",
            Equilibrium::Saddle => "saddle point",
            Equilibrium::StableSpiral => "stable spiral",
            Equilibrium::UnstableSpiral => "unstable spiral",
            Equilibrium::Centre => "centre",
            Equilibrium::Degenerate => "degenerate equilibrium",
        }
    }
}

/// Data for drawing a phase portrait (handed to the figure helpers).
#[derive(Clone, Debug, PartialEq)]
pub struct PhasePortrait {
    /// The type of the equilibrium.
    pub kind: Equilibrium,
    /// The eigenvalues `(re, im)`.
    pub eigenvalues: Vec<(f64, f64)>,
    /// Sample trajectories, each a list of `(x₁, x₂)` points.
    pub trajectories: Vec<Vec<(f64, f64)>>,
}

/// The solution of a linear system.
#[derive(Clone, Debug, PartialEq)]
pub struct SystemSolution {
    /// The unknown functions, in order.
    pub unknowns: Vec<String>,
    /// The independent variable.
    pub t: String,
    /// The coefficient matrix `A`.
    pub matrix: Expr,
    /// `e^{At}`.
    pub exponential: Expr,
    /// Each unknown's solution.
    pub solution: Vec<Expr>,
    /// The arbitrary constants (the values at `t = 0`) left in it.
    pub constants: Vec<String>,
    /// The eigenvalues of `A`.
    pub eigenvalues: Vec<Expr>,
    /// For 2×2 systems.
    pub portrait: Option<PhasePortrait>,
}

/// Solves the system `equations` (each `xᵢ′ = …`, linear with constant coefficients) with
/// optional initial values `x(0)` given in the order of the unknowns.
pub fn solve_system(
    equations: &[Expr],
    initial: Option<&[Expr]>,
    steps: Option<&mut Steps>,
) -> MathResult<SystemSolution> {
    let n = equations.len();
    if n == 0 || n > MAX_SYSTEM {
        return Err(MathError::Unsupported(format!(
            "a system of {n} equations (1 to {MAX_SYSTEM} are handled)"
        )));
    }
    // Find the unknowns and the variable from each equation's derivative.
    let mut unknowns = Vec::new();
    let mut t = None;
    for eq in equations {
        let names = derivative_names(eq);
        let (f, v) = names
            .into_iter()
            .find(|(f, _)| !unknowns.contains(f))
            .ok_or_else(|| MathError::Domain {
                function: "a system".into(),
                detail: format!("{eq} has no derivative of a new unknown"),
            })?;
        if t.as_ref().is_some_and(|tv: &String| *tv != v) {
            return Err(MathError::Domain {
                function: "a system".into(),
                detail: "the equations use different variables".into(),
            });
        }
        t = Some(v);
        unknowns.push(f);
    }
    let t = t.unwrap_or_else(|| "t".into());
    // Each equation: x_i′ = Σ a_ij x_j + f_i(t).
    let mut a = vec![vec![Expr::zero(); n]; n];
    let mut forcing = vec![Expr::zero(); n];
    for (i, eq) in equations.iter().enumerate() {
        let ode = parse(eq, Some(&unknowns[i]), Some(&t))?;
        let mut r = ode.residual.clone();
        // Other unknowns appear as plain names or as calls; make them placeholders too.
        for (j, u) in unknowns.iter().enumerate() {
            if j != i {
                let o = parse(&Expr::equation(r.clone(), Expr::zero()), Some(u), Some(&t));
                if let Ok(o) = o {
                    r = o.residual;
                } else {
                    r = r.subs_expr(&Expr::apply(u, vec![Expr::sym(&t)]), &Expr::sym(u));
                }
            }
        }
        let mut names: Vec<String> = vec![placeholder(&unknowns[i], 1)];
        names.extend(unknowns.iter().cloned());
        let (row, rhs) = linsys::linear_form(&r, &names).ok_or_else(|| {
            MathError::Unsupported(format!(
                "the system equation {eq} (it must be linear in the unknowns)"
            ))
        })?;
        let lead = simplify(&row[0]);
        if lead.is_zero() || lead.contains_sym(&t) {
            return Err(MathError::Unsupported(format!("the system equation {eq}")));
        }
        for j in 0..n {
            let aij = simplify(&mul(vec![Expr::int(-1), row[j + 1].clone(), lead.recip()]));
            if aij.contains_sym(&t) || value(&aij).is_none() {
                return Err(MathError::Unsupported(
                    "a system with variable or symbolic coefficients".into(),
                ));
            }
            a[i][j] = aij;
        }
        forcing[i] = simplify(&mul(vec![rhs, lead.recip()]));
    }
    solve_matrix(&a, &forcing, &unknowns, &t, initial, steps)
}

fn derivative_names(e: &Expr) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let ExprNode::Derivative { expr, vars } = e.node() {
        let name = match expr.node() {
            ExprNode::Sym(s) => Some(s.to_string()),
            ExprNode::Apply(f, args) if args.len() == 1 => Some(f.to_string()),
            _ => None,
        };
        if let (Some(f), [(v, _)]) = (name, vars.as_slice()) {
            out.push((f, v.to_string()));
        }
    }
    for c in e.children() {
        out.extend(derivative_names(c));
    }
    out
}

/// `x′ = A x + f` for a given matrix.
pub fn solve_matrix(
    a: &[Vec<Expr>],
    forcing: &[Expr],
    unknowns: &[String],
    t: &str,
    initial: Option<&[Expr]>,
    steps: Option<&mut Steps>,
) -> MathResult<SystemSolution> {
    let n = a.len();
    let s = if t == "s" { "p" } else { "s" };
    let ss = Expr::sym(s);
    let am = Matrix::new(n, n, a.iter().flatten().cloned().collect())?;
    let si = Matrix::identity(n)?.scale(&ss).sub(&am)?;
    let resolvent = si.inverse()?;
    let mut exp_data = Vec::with_capacity(n * n);
    for i in 0..n {
        for j in 0..n {
            exp_data.push(inverse(resolvent.get(i, j), s, t)?);
        }
    }
    let exponential = Matrix::new(n, n, exp_data.clone())?;
    // Constants: the initial values, or c₁ … cₙ.
    let constants: Vec<String> = (1..=n).map(|k| format!("c_{k}")).collect();
    let c: Vec<Expr> = match initial {
        Some(v) if v.len() == n => v.to_vec(),
        Some(_) => {
            return Err(MathError::Domain {
                function: "a system".into(),
                detail: format!("it needs {n} initial values"),
            })
        }
        None => constants.iter().map(|k| Expr::sym(k)).collect(),
    };
    // Particular part: L⁻¹{(sI − A)⁻¹ F(s)}.
    let mut big_f = Vec::with_capacity(n);
    for f in forcing {
        big_f.push(if f.is_zero() {
            Expr::zero()
        } else {
            transform(f, t, s)?
        });
    }
    let mut solution = Vec::with_capacity(n);
    for i in 0..n {
        let mut terms = Vec::new();
        let mut forced = Vec::new();
        for j in 0..n {
            terms.push(mul(vec![exp_data[i * n + j].clone(), c[j].clone()]));
            if !big_f[j].is_zero() {
                forced.push(mul(vec![resolvent.get(i, j).clone(), big_f[j].clone()]));
            }
        }
        if !forced.is_empty() {
            terms.push(inverse(&simplify(&add(forced)), s, t)?);
        }
        solution.push(tidy(&add(terms)));
    }
    // Check: x′ − A x − f = 0 for every component.
    for i in 0..n {
        let d = diff(&solution[i], t, 1, None)?;
        let mut r = vec![d, mul(vec![Expr::int(-1), forcing[i].clone()])];
        for j in 0..n {
            r.push(mul(vec![
                Expr::int(-1),
                a[i][j].clone(),
                solution[j].clone(),
            ]));
        }
        let r = simplify(&add(r));
        if !r.is_zero() && !super::verify::vanishes(&r, &solution, t, None) {
            return Err(MathError::Inconclusive(format!(
                "the solution of the system (component {} failed its check)",
                unknowns[i]
            )));
        }
    }
    let eigenvalues: Vec<Expr> = am
        .eigenvalues()
        .map(|rs| rs.into_iter().map(|r| r.value).collect())
        .unwrap_or_default();
    let portrait = if n == 2 {
        portrait(a, &eigenvalues)
    } else {
        None
    };
    if let Some(log) = steps {
        log.push(
            "matrix exponential",
            am.to_expr(),
            exponential.to_expr(),
            Some(format!(
                "e^(At) = L⁻¹{{(sI − A)⁻¹}}; the eigenvalues of A are {}; x(t) = e^(At)x(0){}",
                eigenvalues
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
                if forcing.iter().all(Expr::is_zero) {
                    String::new()
                } else {
                    " + ∫₀ᵗ e^(A(t−τ)) f(τ) dτ".into()
                }
            )),
        );
        if let Some(p) = &portrait {
            log.push(
                "phase portrait",
                am.to_expr(),
                am.to_expr(),
                Some(format!("the origin is a {}", p.kind.name())),
            );
        }
    }
    Ok(SystemSolution {
        unknowns: unknowns.to_vec(),
        t: t.to_string(),
        matrix: am.to_expr(),
        exponential: exponential.to_expr(),
        solution,
        constants: if initial.is_some() {
            Vec::new()
        } else {
            constants
        },
        eigenvalues,
        portrait,
    })
}

/// The equilibrium type and a few trajectories (by RK4) of a 2×2 homogeneous system.
fn portrait(a: &[Vec<Expr>], eigen: &[Expr]) -> Option<PhasePortrait> {
    let m: Vec<f64> = a.iter().flatten().map(value).collect::<Option<_>>()?;
    let vals: Vec<(f64, f64)> = eigen
        .iter()
        .filter_map(|e| {
            e.eval_complex(&crate::eval::EmptyEnv)
                .ok()
                .map(|z| (z.re, z.im))
        })
        .collect();
    let (tr, det) = (m[0] + m[3], m[0] * m[3] - m[1] * m[2]);
    let disc = tr * tr - 4.0 * det;
    let eps = 1e-12;
    let kind = if det < -eps {
        Equilibrium::Saddle
    } else if det.abs() <= eps || disc.abs() <= eps {
        Equilibrium::Degenerate
    } else if disc > 0.0 {
        if tr < 0.0 {
            Equilibrium::StableNode
        } else {
            Equilibrium::UnstableNode
        }
    } else if tr.abs() <= eps {
        Equilibrium::Centre
    } else if tr < 0.0 {
        Equilibrium::StableSpiral
    } else {
        Equilibrium::UnstableSpiral
    };
    let mut trajectories = Vec::new();
    for k in 0..8 {
        let ang = std::f64::consts::PI * k as f64 / 4.0;
        let (mut x, mut y) = (ang.cos(), ang.sin());
        let mut path = vec![(x, y)];
        let h = 0.02;
        let f = |x: f64, y: f64| (m[0] * x + m[1] * y, m[2] * x + m[3] * y);
        for _ in 0..200 {
            let (k1x, k1y) = f(x, y);
            let (k2x, k2y) = f(x + h * k1x / 2.0, y + h * k1y / 2.0);
            let (k3x, k3y) = f(x + h * k2x / 2.0, y + h * k2y / 2.0);
            let (k4x, k4y) = f(x + h * k3x, y + h * k3y);
            x += h / 6.0 * (k1x + 2.0 * k2x + 2.0 * k3x + k4x);
            y += h / 6.0 * (k1y + 2.0 * k2y + 2.0 * k3y + k4y);
            if !(x.is_finite() && y.is_finite()) || x.abs() > 1e3 || y.abs() > 1e3 {
                break;
            }
            path.push((x, y));
        }
        trajectories.push(path);
    }
    Some(PhasePortrait {
        kind,
        eigenvalues: vals,
        trajectories,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::Name;

    fn d(f: &str) -> Expr {
        Expr::new(ExprNode::Derivative {
            expr: Expr::sym(f),
            vars: vec![(Name::from("t"), 1)],
        })
    }

    #[test]
    fn two_by_two_systems() {
        // x′ = y, y′ = −x: rotation, a centre.
        let eqs = vec![
            Expr::equation(d("x"), Expr::sym("y")),
            Expr::equation(d("y"), -Expr::sym("x")),
        ];
        let s = solve_system(&eqs, Some(&[Expr::one(), Expr::zero()]), None).unwrap();
        assert_eq!(s.solution[0].to_string(), "cos(t)");
        assert_eq!(s.solution[1].to_string(), "-sin(t)");
        assert_eq!(s.portrait.as_ref().unwrap().kind, Equilibrium::Centre);
        // x′ = x + y, y′ = 4x + y: eigenvalues 3 and −1, a saddle.
        let eqs = vec![
            Expr::equation(d("x"), Expr::sym("x") + Expr::sym("y")),
            Expr::equation(d("y"), Expr::int(4) * Expr::sym("x") + Expr::sym("y")),
        ];
        let s = solve_system(&eqs, None, None).unwrap();
        assert_eq!(s.portrait.as_ref().unwrap().kind, Equilibrium::Saddle);
        assert_eq!(s.constants.len(), 2);
        // Forced: x′ = −x + 1, y′ = x − y with x(0) = y(0) = 0.
        let eqs = vec![
            Expr::equation(d("x"), -Expr::sym("x") + Expr::one()),
            Expr::equation(d("y"), Expr::sym("x") - Expr::sym("y")),
        ];
        let s = solve_system(&eqs, Some(&[Expr::zero(), Expr::zero()]), None).unwrap();
        let x1 = s.solution[0].eval_f64(&[("t", 1.0)]).unwrap();
        assert!((x1 - (1.0 - (-1.0f64).exp())).abs() < 1e-12);
    }
}
