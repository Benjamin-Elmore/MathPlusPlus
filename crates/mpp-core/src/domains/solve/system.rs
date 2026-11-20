//! Systems of equations: linear systems exactly (row reduction), small nonlinear systems
//! by elimination (solve one equation for one unknown, substitute, repeat) and, when that
//! fails, Newton's method from a few starting points.

use crate::domains::linalg::Matrix;
use crate::domains::solve::{numeric, solve, Solutions};
use crate::domains::util::{is_zero, to_zero_form};
use crate::domains::Ctx;
use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode};

/// Most solution branches elimination may follow.
const MAX_BRANCHES: usize = 64;
/// Most equations or unknowns in one system.
pub const MAX_SYSTEM: usize = 64;

/// The solutions of a system.
#[derive(Clone, Debug, PartialEq)]
pub struct SystemSolution {
    /// Each solution assigns values to the unknowns it determines.
    pub assignments: Vec<Vec<(String, Expr)>>,
    /// Unknowns left free (the solution set is a family in these).
    pub free: Vec<String>,
    /// False when found numerically.
    pub exact: bool,
}

impl SystemSolution {
    /// One solution as a tuple of equations `(x = 1, y = 2)`, several as a set of them.
    pub fn to_expr(&self) -> Expr {
        let tuple = |a: &Vec<(String, Expr)>| {
            Expr::new(ExprNode::Tuple(
                a.iter()
                    .map(|(v, e)| Expr::equation(Expr::sym(v), e.clone()))
                    .collect(),
            ))
        };
        match self.assignments.as_slice() {
            [one] => tuple(one),
            many => Expr::new(ExprNode::Set(many.iter().map(tuple).collect())),
        }
    }
}

/// Solves the equations `eqs` for `vars`.
pub fn solve_system(
    eqs: &[Expr],
    vars: &[String],
    ctx: &mut Ctx<'_>,
) -> MathResult<SystemSolution> {
    if eqs.len() > MAX_SYSTEM || vars.len() > MAX_SYSTEM {
        return Err(MathError::TooLarge("this system".into()));
    }
    if vars.is_empty() {
        return Err(MathError::Domain {
            function: "solve".into(),
            detail: "there are no unknowns to solve for".into(),
        });
    }
    let fs: Vec<Expr> = eqs.iter().map(to_zero_form).collect();
    if let Some((a, b)) = linear_form(&fs, vars) {
        ctx.step(
            "row reduction",
            &Expr::new(ExprNode::Tuple(eqs.to_vec())),
            &a.augment(&b)?.to_expr(),
            None,
        );
        return solve_linear(&a, &b, vars);
    }
    if let Some(assignments) = eliminate(&fs, vars, 0) {
        let assignments: Vec<_> = assignments.into_iter().filter(|a| verify(&fs, a)).collect();
        let free = vars
            .iter()
            .filter(|v| assignments.iter().all(|a| a.iter().all(|(n, _)| n != *v)))
            .cloned()
            .collect();
        ctx.step(
            "elimination",
            &Expr::new(ExprNode::Tuple(eqs.to_vec())),
            &SystemSolution {
                assignments: assignments.clone(),
                free: Vec::new(),
                exact: true,
            }
            .to_expr(),
            Some("solve one equation for one unknown and substitute".into()),
        );
        return Ok(SystemSolution {
            assignments: assignments.into_iter().map(|a| order(a, vars)).collect(),
            free,
            exact: true,
        });
    }
    if fs.len() != vars.len() {
        return Err(MathError::Unsupported(
            "numeric solving of a system with a different number of equations and unknowns".into(),
        ));
    }
    let mut found: Vec<Vec<f64>> = Vec::new();
    for start in starts(vars.len()) {
        if let Some(x) = numeric::newton_system(&fs, vars, &start) {
            let new = !found.iter().any(|y| {
                y.iter()
                    .zip(&x)
                    .all(|(a, b)| (a - b).abs() <= 1e-7 * a.abs().max(1.0))
            });
            if new {
                found.push(x);
            }
        }
    }
    if found.is_empty() {
        return Err(MathError::Unsupported(
            "solving this system (no exact method applies and Newton's method did not converge)"
                .into(),
        ));
    }
    ctx.note("solved numerically by Newton's method from several starting points");
    Ok(SystemSolution {
        assignments: found
            .into_iter()
            .map(|x| {
                vars.iter()
                    .cloned()
                    .zip(x.into_iter().map(Expr::float))
                    .collect()
            })
            .collect(),
        free: Vec::new(),
        exact: false,
    })
}

/// Deterministic starting points for Newton's method.
fn starts(n: usize) -> Vec<Vec<f64>> {
    let seeds = [1.0, -1.0, 0.5, 2.0, -2.0, 0.1, 3.0, -0.5];
    let mut out = Vec::new();
    for (i, _) in seeds.iter().enumerate() {
        out.push(
            (0..n)
                .map(|j| seeds[(i + j * 3) % seeds.len()] * (1.0 + 0.1 * j as f64))
                .collect(),
        );
    }
    out
}

/// Puts an assignment in the order of `vars`.
fn order(mut a: Vec<(String, Expr)>, vars: &[String]) -> Vec<(String, Expr)> {
    a.sort_by_key(|(v, _)| vars.iter().position(|x| x == v).unwrap_or(usize::MAX));
    a
}

/// True when the assignment satisfies every equation (or can't be refuted).
fn verify(fs: &[Expr], a: &[(String, Expr)]) -> bool {
    let pairs: Vec<(&str, Expr)> = a.iter().map(|(v, e)| (v.as_str(), e.clone())).collect();
    fs.iter().all(|f| {
        let at = f.subs_all(&pairs).simplify();
        if is_zero(&at) || !at.is_constant() {
            return true;
        }
        at.eval_complex(&crate::eval::EmptyEnv)
            .is_ok_and(|v| v.norm() < 1e-9)
    })
}

/// `A x = b` when every equation is linear in the unknowns.
fn linear_form(fs: &[Expr], vars: &[String]) -> Option<(Matrix, Matrix)> {
    let mut rows = Vec::new();
    let mut rhs = Vec::new();
    for f in fs {
        let mut row = Vec::new();
        let mut rest = f.clone();
        for v in vars {
            let c = rest.as_poly(v)?;
            let coeff = match c.as_slice() {
                [] => Expr::zero(),
                [_] => Expr::zero(),
                [_, a] => a.clone(),
                _ => return None,
            };
            if vars.iter().any(|w| coeff.contains_sym(w)) {
                return None;
            }
            rest = c.first().cloned().unwrap_or_else(Expr::zero);
            row.push(coeff);
        }
        if vars.iter().any(|w| rest.contains_sym(w)) {
            return None;
        }
        rows.push(row);
        rhs.push(vec![(-rest).simplify()]);
    }
    Some((Matrix::from_rows(rows).ok()?, Matrix::from_rows(rhs).ok()?))
}

fn solve_linear(a: &Matrix, b: &Matrix, vars: &[String]) -> MathResult<SystemSolution> {
    let aug = a.augment(b)?;
    let (r, pivots) = aug.rref()?;
    let n = a.cols();
    if pivots.contains(&n) {
        return Ok(SystemSolution {
            assignments: Vec::new(),
            free: Vec::new(),
            exact: true,
        });
    }
    let free: Vec<String> = (0..n)
        .filter(|c| !pivots.contains(c))
        .map(|c| vars[c].clone())
        .collect();
    let mut assignment = Vec::new();
    for (row, &pc) in pivots.iter().enumerate() {
        let mut value = r.get(row, n).clone();
        for (c, var) in vars.iter().enumerate().take(n).skip(pc + 1) {
            if !pivots.contains(&c) {
                value = value - r.get(row, c).clone() * Expr::sym(var);
            }
        }
        assignment.push((vars[pc].clone(), value.simplify()));
    }
    Ok(SystemSolution {
        assignments: vec![assignment],
        free,
        exact: true,
    })
}

/// How easy it is to solve `f` for `v` (lower is easier); `None` when `v` is absent.
fn difficulty(f: &Expr, v: &str, vars: &[String]) -> Option<u32> {
    if !f.contains_sym(v) {
        return None;
    }
    let others = vars.iter().filter(|w| *w != v && f.contains_sym(w)).count() as u32;
    let score = match f.as_poly(v) {
        Some(c) if c.len() == 2 => {
            if vars.iter().any(|w| c[1].contains_sym(w)) {
                1
            } else {
                0
            }
        }
        Some(c) => 1 + c.len() as u32,
        None => 8,
    };
    Some(score * 4 + others)
}

/// Elimination; `None` when some step can't be done exactly.
fn eliminate(fs: &[Expr], vars: &[String], depth: usize) -> Option<Vec<Vec<(String, Expr)>>> {
    let mut live = Vec::new();
    for f in fs {
        let f = f.simplify();
        if is_zero(&f) {
            continue;
        }
        if !vars.iter().any(|v| f.contains_sym(v)) {
            if f.is_constant() {
                return Some(Vec::new()); // a contradiction such as 1 = 0
            }
            continue;
        }
        live.push(f);
    }
    if live.is_empty() || depth > MAX_SYSTEM {
        return Some(vec![Vec::new()]);
    }
    let (i, v) = live
        .iter()
        .enumerate()
        .flat_map(|(i, f)| {
            vars.iter()
                .filter_map(move |v| difficulty(f, v, vars).map(|d| (d, i, v)))
        })
        .min_by_key(|(d, _, _)| *d)
        .map(|(_, i, v)| (i, v.clone()))?;
    let mut ctx = Ctx::new();
    let sols = solve(&live[i], &v, None, &mut ctx).ok()?;
    let values = match sols {
        Solutions::Finite {
            values,
            exact: true,
        } => values,
        _ => return None,
    };
    let rest_vars: Vec<String> = vars.iter().filter(|w| **w != v).cloned().collect();
    let mut out = Vec::new();
    for value in values {
        let rest: Vec<Expr> = live
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, f)| f.subs(&v, &value).simplify())
            .collect();
        for mut sub in eliminate(&rest, &rest_vars, depth + 1)? {
            let pairs: Vec<(&str, Expr)> =
                sub.iter().map(|(n, e)| (n.as_str(), e.clone())).collect();
            let back = value.subs_all(&pairs).simplify();
            sub.push((v.clone(), back));
            out.push(sub);
            if out.len() > MAX_BRANCHES {
                return None;
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::EmptyEnv;

    fn s(n: &str) -> Expr {
        Expr::sym(n)
    }
    fn vars(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn linear_unique() {
        let mut ctx = Ctx::new();
        let eqs = vec![
            Expr::equation(s("x") + s("y"), Expr::int(3)),
            Expr::equation(s("x") - s("y"), Expr::int(1)),
        ];
        let sol = solve_system(&eqs, &vars(&["x", "y"]), &mut ctx).unwrap();
        assert_eq!(sol.to_expr().to_string(), "(x = 2, y = 1)");
    }

    #[test]
    fn linear_family_and_inconsistent() {
        let mut ctx = Ctx::new();
        let eqs = vec![
            Expr::equation(s("x") + Expr::int(2) * s("y"), Expr::int(3)),
            Expr::equation(Expr::int(2) * s("x") + Expr::int(4) * s("y"), Expr::int(6)),
        ];
        let sol = solve_system(&eqs, &vars(&["x", "y"]), &mut ctx).unwrap();
        assert_eq!(sol.free, vec!["y".to_string()]);
        assert_eq!(
            sol.assignments[0][0].1,
            Expr::int(3) - Expr::int(2) * s("y")
        );
        let eqs = vec![
            Expr::equation(s("x") + s("y"), Expr::int(1)),
            Expr::equation(s("x") + s("y"), Expr::int(2)),
        ];
        let sol = solve_system(&eqs, &vars(&["x", "y"]), &mut ctx).unwrap();
        assert!(sol.assignments.is_empty());
    }

    #[test]
    fn nonlinear_by_elimination() {
        let mut ctx = Ctx::new();
        let eqs = vec![
            Expr::equation(s("x") + s("y"), Expr::int(3)),
            Expr::equation(s("x") * s("y"), Expr::int(2)),
        ];
        let sol = solve_system(&eqs, &vars(&["x", "y"]), &mut ctx).unwrap();
        assert!(sol.exact);
        assert_eq!(sol.assignments.len(), 2);
        let eqs = vec![
            Expr::equation(s("x").pow(2) + s("y").pow(2), Expr::int(25)),
            Expr::equation(s("y"), s("x") + Expr::one()),
        ];
        let sol = solve_system(&eqs, &vars(&["x", "y"]), &mut ctx).unwrap();
        let mut xs: Vec<f64> = sol
            .assignments
            .iter()
            .map(|a| a[0].1.eval_f64(&EmptyEnv).unwrap())
            .collect();
        xs.sort_by(f64::total_cmp);
        assert_eq!(xs, vec![-4.0, 3.0]);
    }

    #[test]
    fn nonlinear_numeric() {
        let mut ctx = Ctx::new();
        let eqs = vec![
            Expr::equation(
                Expr::call("sin", vec![s("x")]).unwrap() + s("y"),
                Expr::one(),
            ),
            Expr::equation(
                s("x") - Expr::call("cos", vec![s("y")]).unwrap(),
                Expr::zero(),
            ),
        ];
        let sol = solve_system(&eqs, &vars(&["x", "y"]), &mut ctx).unwrap();
        assert!(!sol.exact);
        let a = &sol.assignments[0];
        let env = [
            ("x", a[0].1.eval_f64(&EmptyEnv).unwrap()),
            ("y", a[1].1.eval_f64(&EmptyEnv).unwrap()),
        ];
        for e in &eqs {
            assert!(to_zero_form(e).eval_f64(&env).unwrap().abs() < 1e-9);
        }
    }
}
