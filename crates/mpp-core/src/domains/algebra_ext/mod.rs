//! Algebra beside the linear algebra domain (which already has `rank`, `trace` and
//! `nullspace`): the kernel under its own name, the matrix exponential, Lie brackets,
//! character tables of small groups, and orbit/stabiliser counts for permutation
//! groups given by generators.

pub mod characters;
pub mod matexp;

use crate::domains::discrete::perm;
use crate::domains::fieldkit::{domain_err, matrix_expr, tuple, usize_arg};
use crate::domains::linalg::Matrix;
use crate::domains::util::{expect_args, expect_args_between};
use crate::domains::{Ctx, Domain, Value};
use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode};

/// The algebra domain.
pub struct Algebra;

const OPS: &[&str] = &[
    "kernel",
    "expm",
    "liebracket",
    "chartable",
    "grouporder",
    "orbit",
    "stabilizer",
];

impl Domain for Algebra {
    fn name(&self) -> &'static str {
        "algebra"
    }

    fn operators(&self) -> &'static [&'static str] {
        OPS
    }

    fn apply(&self, op: &str, args: &[Value], ctx: &mut Ctx<'_>) -> Option<MathResult<Value>> {
        Some(apply(op, args, ctx))
    }
}

fn matrix(op: &str, e: &Expr) -> MathResult<Matrix> {
    match e.node() {
        ExprNode::Matrix { .. } => {
            Matrix::from_expr(e).ok_or_else(|| domain_err(op, format!("{e} is not a matrix")))
        }
        _ => Err(domain_err(op, format!("{e} is not a matrix"))),
    }
}

fn point(op: &str, e: &Expr, n: usize) -> MathResult<usize> {
    let x = usize_arg(op, e, perm::MAX_POINTS)?;
    if x == 0 || x > n {
        return Err(domain_err(op, format!("the point must be 1 … {n}")));
    }
    Ok(x - 1)
}

fn apply(op: &str, args: &[Value], ctx: &mut Ctx<'_>) -> MathResult<Value> {
    match op {
        "kernel" => {
            expect_args(op, args, 1)?;
            let basis = matrix(op, &args[0])?.nullspace()?;
            ctx.note(format!("dim ker = {}", basis.len()));
            Ok(tuple(basis.into_iter().map(|v| v.to_expr()).collect()))
        }
        "expm" => {
            expect_args_between(op, args, 1, 2)?;
            let a = matrix(op, &args[0])?;
            let t = args.get(1).cloned().unwrap_or_else(Expr::one);
            Ok(matexp::expm(&a, &t)?.to_expr())
        }
        "liebracket" => {
            expect_args(op, args, 2)?;
            let (x, y) = (matrix(op, &args[0])?, matrix(op, &args[1])?);
            Ok(x.mul(&y)?.sub(&y.mul(&x)?)?.simplified().to_expr())
        }
        "chartable" => {
            expect_args(op, args, 1)?;
            let name = match args[0].node() {
                ExprNode::Sym(s) => s.to_string(),
                ExprNode::Apply(s, a) if a.len() == 1 => format!("{s}{}", a[0]),
                _ => args[0].to_string(),
            };
            let t = characters::table(&name)?;
            ctx.note(format!(
                "rows: irreducible characters; columns: classes {} of sizes {}",
                t.classes.join(", "),
                t.class_sizes
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            Ok(tuple(vec![
                tuple(t.class_sizes.iter().map(|&s| Expr::int(s as i64)).collect()),
                matrix_expr(t.chars),
            ]))
        }
        "grouporder" => {
            expect_args(op, args, 1)?;
            let g = perm::closure(&perm::generators_arg(op, &args[0])?)?;
            Ok(Expr::int(g.len() as i64))
        }
        "orbit" | "stabilizer" => {
            expect_args(op, args, 2)?;
            let gens = perm::generators_arg(op, &args[0])?;
            let x = point(op, &args[1], gens[0].len())?;
            let g = perm::closure(&gens)?;
            let orbit = perm::orbit(&g, x);
            let stab = perm::stabilizer_order(&g, x);
            ctx.note(format!(
                "orbit–stabiliser: |Orb| · |Stab| = {} · {} = {} = |G|",
                orbit.len(),
                stab,
                g.len()
            ));
            Ok(if op == "orbit" {
                Expr::new(ExprNode::Set(
                    orbit.into_iter().map(|p| Expr::int(p as i64 + 1)).collect(),
                ))
            } else {
                Expr::int(stab as i64)
            })
        }
        _ => Err(MathError::UnknownFunction(op.into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(op: &str, args: &[Expr]) -> MathResult<Expr> {
        Algebra.apply(op, args, &mut Ctx::new()).unwrap()
    }

    fn mat(rows: &[&[i64]]) -> Expr {
        Expr::matrix(
            rows.iter()
                .map(|r| r.iter().map(|&v| Expr::int(v)).collect())
                .collect(),
        )
        .unwrap()
    }

    fn t(v: &[i64]) -> Expr {
        tuple(v.iter().map(|&x| Expr::int(x)).collect())
    }

    #[test]
    fn operators() {
        // ker [[1, 2], [2, 4]] is spanned by (−2, 1); rank–nullity 1 + 1 = 2.
        let k = run("kernel", &[mat(&[&[1, 2], &[2, 4]])]).unwrap();
        assert_eq!(k.to_string().matches(',').count(), 1, "{k}");
        // [X, Y] for the sl₂ basis: [E, F] = H.
        let e = mat(&[&[0, 1], &[0, 0]]);
        let f = mat(&[&[0, 0], &[1, 0]]);
        assert_eq!(
            run("liebracket", &[e.clone(), f]),
            Ok(mat(&[&[1, 0], &[0, -1]]))
        );
        assert_eq!(
            run("liebracket", &[e.clone(), e.clone()]),
            Ok(mat(&[&[0, 0], &[0, 0]]))
        );
        let ex = run("expm", &[e, Expr::int(3)]).unwrap();
        assert_eq!(ex, mat(&[&[1, 3], &[0, 1]]));
        let ct = run("chartable", &[Expr::sym("S3")]).unwrap();
        assert_eq!(
            ct.to_string(),
            tuple(vec![
                t(&[1, 3, 2]),
                mat(&[&[1, 1, 1], &[1, -1, 1], &[2, 0, -1]])
            ])
            .to_string()
        );
        // The rotations of a square: |G| = 4, orbit of 1 is everything, stabiliser trivial.
        let rot = tuple(vec![t(&[2, 3, 4, 1])]);
        assert_eq!(
            run("grouporder", std::slice::from_ref(&rot)),
            Ok(Expr::int(4))
        );
        assert_eq!(
            run("stabilizer", &[rot.clone(), Expr::one()]),
            Ok(Expr::one())
        );
        let orb = run("orbit", &[rot, Expr::one()]).unwrap();
        assert_eq!(
            orb,
            Expr::new(ExprNode::Set((1..=4).map(Expr::int).collect()))
        );
        assert!(run("orbit", &[tuple(vec![t(&[2, 1])]), Expr::int(3)]).is_err());
    }
}
