//! Applied maths: gradient descent, KKT checks, zero-sum games by linear programming,
//! two-player Shapley values, controllability, present value and put–call parity, and
//! Hamilton's equations produced as an ODE system (data the differential-equation
//! solver can integrate).

pub mod games;
pub mod optimize;

use num_rational::BigRational;

use crate::domains::calculus::Calculus;
use crate::domains::fieldkit::{
    domain_err, float, matrix_rows, real_list_of, truth, tuple, usize_arg, var_list,
};
use crate::domains::linalg::Matrix;
use crate::domains::util::{expect_args, expect_args_between, rational, real_arg};
use crate::domains::{Ctx, Domain, Value};
use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode, RelOp};

use optimize::Constraint;

/// The applied maths domain.
pub struct Applied;

const OPS: &[&str] = &[
    "graddescent",
    "kkt",
    "gamevalue",
    "shapley",
    "ctrb",
    "ctrbrank",
    "presentvalue",
    "putfromcall",
    "callfromput",
    "hamilton",
];

impl Domain for Applied {
    fn name(&self) -> &'static str {
        "applied mathematics"
    }

    fn operators(&self) -> &'static [&'static str] {
        OPS
    }

    fn apply(&self, op: &str, args: &[Value], ctx: &mut Ctx<'_>) -> Option<MathResult<Value>> {
        Some(apply(op, args, ctx))
    }
}

/// Hamilton's equations `q̇_i = ∂H/∂p_i`, `ṗ_i = −∂H/∂q_i` as `(state variable,
/// right-hand side)` pairs, q's first then p's. This is the hand-off format for the
/// ODE solver: integrate `d(name)/dt = rhs` for each pair.
pub fn hamilton_system(
    h: &Expr,
    qs: &[String],
    ps: &[String],
    calc: &dyn Calculus,
) -> MathResult<Vec<(String, Expr)>> {
    if qs.len() != ps.len() || qs.is_empty() {
        return Err(domain_err(
            "hamilton",
            "give as many momenta as coordinates (at least one)",
        ));
    }
    let mut out = Vec::with_capacity(2 * qs.len());
    for (q, p) in qs.iter().zip(ps) {
        out.push((q.clone(), calc.diff(h, p)?.simplify()));
    }
    for (q, p) in qs.iter().zip(ps) {
        out.push((p.clone(), (-calc.diff(h, q)?).simplify()));
    }
    Ok(out)
}

/// The controllability matrix `[B, AB, …, A^{n−1}B]`.
pub fn controllability(a: &Matrix, b: &Matrix) -> MathResult<Matrix> {
    let n = a.rows();
    if !a.is_square() || b.rows() != n {
        return Err(domain_err("ctrb", "A must be n×n and B must have n rows"));
    }
    let mut blocks = vec![b.clone()];
    for k in 1..n {
        blocks.push(a.mul(&blocks[k - 1])?.simplified());
    }
    let mut out = blocks[0].clone();
    for blk in &blocks[1..] {
        out = out.augment(blk)?;
    }
    Ok(out)
}

fn matrix(op: &str, e: &Expr) -> MathResult<Matrix> {
    Matrix::from_expr(e).ok_or_else(|| domain_err(op, format!("{e} is not a matrix")))
}

fn constraint(e: &Expr) -> Constraint {
    match e.node() {
        ExprNode::Relation(RelOp::Le | RelOp::Lt, a, b) => Constraint {
            g: (a - b).simplify(),
            equality: false,
        },
        ExprNode::Relation(RelOp::Ge | RelOp::Gt, a, b) => Constraint {
            g: (b - a).simplify(),
            equality: false,
        },
        ExprNode::Relation(RelOp::Eq, a, b) => Constraint {
            g: (a - b).simplify(),
            equality: true,
        },
        _ => Constraint {
            g: e.clone(),
            equality: false,
        },
    }
}

fn exact_rational(op: &str, e: &Expr) -> MathResult<BigRational> {
    if let Some(q) = rational(&e.simplify()) {
        return Ok(q);
    }
    let v = real_arg(op, e)?;
    BigRational::from_float(v).ok_or_else(|| domain_err(op, format!("{e} is not a finite number")))
}

fn ratios(v: Vec<BigRational>) -> Expr {
    tuple(v.into_iter().map(Expr::ratio).collect())
}

fn discount(r: &Expr, t: &Expr) -> Expr {
    Expr::e().pow(-(r * t))
}

fn apply(op: &str, args: &[Value], ctx: &mut Ctx<'_>) -> MathResult<Value> {
    match op {
        "graddescent" => {
            // graddescent(f, vars, x0, η[, steps])
            expect_args_between(op, args, 4, 5)?;
            let vars = var_list(op, &args[1])?;
            let x0 = real_list_of(op, &args[2])?;
            if x0.len() != vars.len() {
                return Err(domain_err(op, "give one starting value per variable"));
            }
            let eta = real_arg(op, &args[3])?;
            let steps = match args.get(4) {
                Some(s) => usize_arg(op, s, optimize::MAX_STEPS)?,
                None => 100_000,
            };
            let d =
                optimize::gradient_descent(&args[0], &vars, &x0, eta, 1e-10, steps, ctx.calculus)?;
            ctx.note(format!(
                "{} steps, ‖∇f‖ = {:.2e} at the end",
                d.steps, d.grad_norm
            ));
            Ok(tuple(d.point.into_iter().map(float).collect()))
        }
        "kkt" => {
            // kkt(f, constraints, vars, point[, multipliers])
            expect_args_between(op, args, 4, 5)?;
            let cons: Vec<Constraint> = match args[1].node() {
                ExprNode::Tuple(v) => v.iter().map(constraint).collect(),
                _ => vec![constraint(&args[1])],
            };
            let vars = var_list(op, &args[2])?;
            let x = real_list_of(op, &args[3])?;
            if x.len() != vars.len() {
                return Err(domain_err(op, "give one coordinate per variable"));
            }
            let lam = match args.get(4) {
                Some(l) => Some(real_list_of(op, l)?),
                None => None,
            };
            let r = optimize::kkt(&args[0], &cons, &vars, &x, lam.as_deref(), ctx.calculus)?;
            ctx.note(format!(
                "stationarity {:.1e}, feasibility {:.1e}, dual {:.1e}, slackness {:.1e}",
                r.stationarity, r.infeasibility, r.dual_violation, r.slackness
            ));
            Ok(tuple(vec![
                truth(r.holds),
                tuple(r.multipliers.into_iter().map(float).collect()),
            ]))
        }
        "gamevalue" => {
            expect_args(op, args, 1)?;
            let a = matrix_rows(op, &args[0])?
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|x| exact_rational(op, x))
                        .collect::<MathResult<Vec<_>>>()
                })
                .collect::<MathResult<Vec<_>>>()?;
            let s = games::solve(&a)?;
            ctx.note("(value, row player's strategy, column player's strategy)");
            Ok(tuple(vec![
                Expr::ratio(s.value),
                ratios(s.row),
                ratios(s.col),
            ]))
        }
        "shapley" => {
            expect_args(op, args, 3)?;
            let (v1, v2, v12) = (&args[0], &args[1], &args[2]);
            let two = Expr::int(2);
            Ok(tuple(vec![
                ((v1 + v12 - v2.clone()) / two.clone()).simplify(),
                ((v2 + v12 - v1.clone()) / two).simplify(),
            ]))
        }
        "ctrb" | "ctrbrank" => {
            expect_args(op, args, 2)?;
            let c = controllability(&matrix(op, &args[0])?, &matrix(op, &args[1])?)?;
            if op == "ctrb" {
                return Ok(c.to_expr());
            }
            let rank = c.rank()?;
            let n = c.rows();
            ctx.note(if rank == n {
                format!("rank {rank} = n, so the system is controllable")
            } else {
                format!("rank {rank} < n = {n}, so the system is not controllable")
            });
            Ok(Expr::int(rank as i64))
        }
        "presentvalue" => {
            // PV = FV e^{−rT}
            expect_args(op, args, 3)?;
            Ok((&args[0] * &discount(&args[1], &args[2])).simplify())
        }
        "putfromcall" | "callfromput" => {
            // C − P = S − K e^{−rT}; arguments (price, S, K, r, T).
            expect_args(op, args, 5)?;
            let (price, s, k) = (&args[0], &args[1], &args[2]);
            let pv_k = k * &discount(&args[3], &args[4]);
            let out = if op == "putfromcall" {
                price - s + pv_k
            } else {
                price + s - pv_k
            };
            Ok(out.simplify())
        }
        "hamilton" => {
            expect_args(op, args, 3)?;
            let qs = var_list(op, &args[1])?;
            let ps = var_list(op, &args[2])?;
            let sys = hamilton_system(&args[0], &qs, &ps, ctx.calculus)?;
            ctx.note("pairs (x, dx/dt): q̇ = ∂H/∂p, ṗ = −∂H/∂q");
            Ok(tuple(
                sys.into_iter()
                    .map(|(v, rhs)| tuple(vec![Expr::sym(&v), rhs]))
                    .collect(),
            ))
        }
        _ => Err(MathError::UnknownFunction(op.into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::calculus::BasicCalculus;
    use crate::domains::util::numeric;

    fn run(op: &str, args: &[Expr]) -> MathResult<Expr> {
        Applied.apply(op, args, &mut Ctx::new()).unwrap()
    }

    fn mat(rows: &[&[i64]]) -> Expr {
        Expr::matrix(
            rows.iter()
                .map(|r| r.iter().map(|&v| Expr::int(v)).collect())
                .collect(),
        )
        .unwrap()
    }

    #[test]
    fn games_and_values() {
        let v = run("gamevalue", &[mat(&[&[1, -1], &[-1, 1]])]).unwrap();
        assert_eq!(v.to_string(), "(0, (1/2, 1/2), (1/2, 1/2))");
        let s = run("shapley", &[Expr::int(1), Expr::int(2), Expr::int(6)]).unwrap();
        assert_eq!(s, tuple(vec![Expr::rational(5, 2), Expr::rational(7, 2)]));
        // PV of 100 at r = 5 % over 2 years: 100 e^{−1/10} ≈ 90.4837.
        let pv = run(
            "presentvalue",
            &[Expr::int(100), Expr::rational(1, 20), Expr::int(2)],
        )
        .unwrap();
        assert!((numeric(&pv).unwrap() - 90.483_741_803_596).abs() < 1e-9);
        // Put–call parity round trip, and the textbook case C = 10, S = 100, K = 100, r = 0.05, T = 1:
        // P = 10 − 100 + 100 e^{−0.05} ≈ 5.1229 (Hull, Options, Futures and Other Derivatives §11.4).
        let args = [
            Expr::int(10),
            Expr::int(100),
            Expr::int(100),
            Expr::rational(1, 20),
            Expr::one(),
        ];
        let p = run("putfromcall", &args).unwrap();
        assert!((numeric(&p).unwrap() - 5.122_942_450_071_4).abs() < 1e-9);
        let c = run(
            "callfromput",
            &[
                p,
                Expr::int(100),
                Expr::int(100),
                Expr::rational(1, 20),
                Expr::one(),
            ],
        )
        .unwrap();
        assert_eq!(c, Expr::int(10));
    }

    #[test]
    fn control_and_mechanics() {
        // Double integrator A = [[0,1],[0,0]], B = (0, 1)ᵀ is controllable; B = (1, 0)ᵀ is not.
        let a = mat(&[&[0, 1], &[0, 0]]);
        assert_eq!(
            run("ctrbrank", &[a.clone(), mat(&[&[0], &[1]])]),
            Ok(Expr::int(2))
        );
        assert_eq!(
            run("ctrbrank", &[a.clone(), mat(&[&[1], &[0]])]),
            Ok(Expr::int(1))
        );
        assert_eq!(
            run("ctrb", &[a, mat(&[&[0], &[1]])]),
            Ok(mat(&[&[0, 1], &[1, 0]]))
        );
        // Harmonic oscillator H = p²/(2m) + k q²/2: q̇ = p/m, ṗ = −k q.
        let (q, p, m, k) = (
            Expr::sym("q"),
            Expr::sym("p"),
            Expr::sym("m"),
            Expr::sym("k"),
        );
        let h = p.pow(2) / (Expr::int(2) * m.clone()) + k.clone() * q.pow(2) / Expr::int(2);
        let sys = hamilton_system(&h, &["q".into()], &["p".into()], &BasicCalculus).unwrap();
        assert_eq!(sys[0], ("q".to_string(), (p.clone() / m).simplify()));
        assert_eq!(sys[1], ("p".to_string(), (-(k * q)).simplify()));
        let shown = run("hamilton", &[h, Expr::sym("q"), Expr::sym("p")]).unwrap();
        assert!(shown.to_string().starts_with("((q, "));
    }

    #[test]
    fn optimisation_operators() {
        let (x, y) = (Expr::sym("x"), Expr::sym("y"));
        let vars = tuple(vec![x.clone(), y.clone()]);
        let f = (x.clone() - Expr::int(3)).pow(2) + (y.clone() + Expr::one()).pow(2);
        let start = tuple(vec![Expr::zero(), Expr::zero()]);
        let r = run(
            "graddescent",
            &[f, vars.clone(), start, Expr::rational(1, 4)],
        )
        .unwrap();
        let ExprNode::Tuple(pt) = r.node() else {
            panic!("{r}")
        };
        assert!((numeric(&pt[0]).unwrap() - 3.0).abs() < 1e-9);
        let obj = x.pow(2) + y.pow(2);
        let con = Expr::relation(RelOp::Ge, x.clone() + y.clone(), Expr::one());
        let pt = tuple(vec![Expr::rational(1, 2), Expr::rational(1, 2)]);
        let r = run("kkt", &[obj, con, vars, pt]).unwrap();
        assert!(r.to_string().starts_with("(true, "), "{r}");
    }
}
