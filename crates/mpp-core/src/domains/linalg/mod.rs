//! Linear algebra over exact expressions: vectors, matrices, determinants, inverses,
//! row reduction, eigenvalues and eigenvectors, and `A x = b`.

pub mod eigen;
pub mod matrix;

pub use eigen::Eigen;
pub use matrix::Matrix;

use crate::domains::util::{expect_args, expect_args_between, int_arg};
use crate::domains::{Ctx, Domain, Value};
use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode};

/// The linear algebra domain.
pub struct LinearAlgebra;

const OPS: &[&str] = &[
    "det",
    "inverse",
    "inv",
    "transpose",
    "trace",
    "rank",
    "rref",
    "nullspace",
    "dot",
    "cross",
    "matmul",
    "matadd",
    "matsub",
    "scale",
    "identity",
    "charpoly",
    "eigenvals",
    "eigen",
    "eigenvects",
    "linsolve",
    "norm",
    "adjugate",
];

fn matrix_arg(op: &str, e: &Expr) -> MathResult<Matrix> {
    Matrix::from_expr(e).ok_or_else(|| MathError::Domain {
        function: op.into(),
        detail: format!("{e} is not a matrix or vector"),
    })
}

fn tuple(items: Vec<Expr>) -> Expr {
    Expr::new(ExprNode::Tuple(items))
}

impl Domain for LinearAlgebra {
    fn name(&self) -> &'static str {
        "linear algebra"
    }

    fn operators(&self) -> &'static [&'static str] {
        OPS
    }

    fn apply(&self, op: &str, args: &[Value], ctx: &mut Ctx<'_>) -> Option<MathResult<Value>> {
        Some(apply(op, args, ctx))
    }
}

fn unary(op: &str, args: &[Value]) -> MathResult<Matrix> {
    expect_args(op, args, 1)?;
    matrix_arg(op, &args[0])
}

fn binary(op: &str, args: &[Value]) -> MathResult<(Matrix, Matrix)> {
    expect_args(op, args, 2)?;
    Ok((matrix_arg(op, &args[0])?, matrix_arg(op, &args[1])?))
}

fn apply(op: &str, args: &[Value], ctx: &mut Ctx<'_>) -> MathResult<Value> {
    match op {
        "det" => {
            let m = unary(op, args)?;
            let d = m.det()?;
            ctx.step("determinant", &args[0], &d, None);
            Ok(d)
        }
        "inverse" | "inv" => Ok(unary(op, args)?.inverse()?.to_expr()),
        "transpose" => Ok(unary(op, args)?.transpose().to_expr()),
        "trace" => unary(op, args)?.trace(),
        "rank" => Ok(Expr::int(unary(op, args)?.rank()? as i64)),
        "rref" => Ok(unary(op, args)?.rref()?.0.to_expr()),
        "adjugate" => Ok(unary(op, args)?.adjugate()?.to_expr()),
        "nullspace" => Ok(tuple(
            unary(op, args)?
                .nullspace()?
                .iter()
                .map(Matrix::to_expr)
                .collect(),
        )),
        "norm" => Ok(unary(op, args)?.norm()),
        "dot" => {
            let (a, b) = binary(op, args)?;
            Ok(a.dot(&b)?)
        }
        "cross" => {
            let (a, b) = binary(op, args)?;
            Ok(a.cross(&b)?.to_expr())
        }
        "matmul" => {
            let (a, b) = binary(op, args)?;
            Ok(a.mul(&b)?.to_expr())
        }
        "matadd" => {
            let (a, b) = binary(op, args)?;
            Ok(a.add(&b)?.to_expr())
        }
        "matsub" => {
            let (a, b) = binary(op, args)?;
            Ok(a.sub(&b)?.to_expr())
        }
        "scale" => {
            expect_args(op, args, 2)?;
            Ok(matrix_arg(op, &args[1])?.scale(&args[0]).to_expr())
        }
        "identity" => {
            expect_args(op, args, 1)?;
            let n = usize::try_from(int_arg(op, &args[0])?).map_err(|_| MathError::Domain {
                function: op.into(),
                detail: "the size must be a positive integer".into(),
            })?;
            Ok(Matrix::identity(n)?.to_expr())
        }
        "charpoly" => {
            expect_args_between(op, args, 1, 2)?;
            let var = match args.get(1) {
                Some(v) => crate::domains::util::sym_arg(op, v)?,
                None => "λ".to_string(),
            };
            matrix_arg(op, &args[0])?.charpoly(&var)
        }
        "eigenvals" | "eigen" => {
            let roots = unary(op, args)?.eigenvalues()?;
            if roots.iter().any(|r| !r.exact) {
                ctx.note("some eigenvalues are numeric");
            }
            Ok(tuple(
                roots
                    .into_iter()
                    .flat_map(|r| std::iter::repeat_n(r.value, r.multiplicity as usize))
                    .collect(),
            ))
        }
        "eigenvects" => {
            let es = unary(op, args)?.eigenvectors()?;
            if es.iter().any(|e| !e.exact) {
                ctx.note("some eigenvectors are numeric");
            }
            Ok(tuple(
                es.into_iter()
                    .map(|e| {
                        tuple(vec![
                            e.value,
                            Expr::int(i64::from(e.multiplicity)),
                            tuple(e.vectors.iter().map(Matrix::to_expr).collect()),
                        ])
                    })
                    .collect(),
            ))
        }
        "linsolve" => {
            let (a, b) = binary(op, args)?;
            linsolve(&a, &b, ctx)
        }
        _ => Err(MathError::UnknownFunction(op.into())),
    }
}

/// Solves `A x = b`: the unique solution as a vector; a family `x₀ + t₁ v₁ + …` with
/// parameters `t₁, t₂, …`; or the empty set.
pub fn linsolve(a: &Matrix, b: &Matrix, ctx: &mut Ctx<'_>) -> MathResult<Value> {
    if b.cols() != 1 || b.rows() != a.rows() {
        return Err(MathError::Domain {
            function: "linsolve".into(),
            detail: "b must be a column with one entry per row of A".into(),
        });
    }
    let (r, pivots) = a.augment(b)?.rref()?;
    let n = a.cols();
    if pivots.contains(&n) {
        ctx.note("the system is inconsistent");
        return Ok(Expr::new(ExprNode::Set(Vec::new())));
    }
    let free: Vec<usize> = (0..n).filter(|c| !pivots.contains(c)).collect();
    let mut x = vec![Expr::zero(); n];
    for (row, &pc) in pivots.iter().enumerate() {
        let mut v = r.get(row, n).clone();
        for (k, &fc) in free.iter().enumerate() {
            let t = Expr::sym(&format!("t_{}", k + 1));
            v = v - r.get(row, fc).clone() * t;
        }
        x[pc] = v.simplify();
    }
    for (k, &fc) in free.iter().enumerate() {
        x[fc] = Expr::sym(&format!("t_{}", k + 1));
    }
    if !free.is_empty() {
        ctx.note(format!(
            "infinitely many solutions; t_1 … t_{} are free parameters",
            free.len()
        ));
    }
    Ok(Matrix::column(x)?.to_expr())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mat(rows: &[&[i64]]) -> Expr {
        Expr::matrix(
            rows.iter()
                .map(|r| r.iter().map(|&v| Expr::int(v)).collect())
                .collect(),
        )
        .unwrap()
    }

    #[test]
    fn operators_through_the_domain() {
        let la = LinearAlgebra;
        let mut ctx = Ctx::new();
        let a = mat(&[&[2, 1], &[1, 2]]);
        let run = |op: &str, args: &[Expr], ctx: &mut Ctx<'_>| la.apply(op, args, ctx).unwrap();
        assert_eq!(
            run("det", std::slice::from_ref(&a), &mut ctx),
            Ok(Expr::int(3))
        );
        assert_eq!(
            run("eigenvals", std::slice::from_ref(&a), &mut ctx)
                .unwrap()
                .to_string(),
            "(1, 3)"
        );
        assert_eq!(
            run("linsolve", &[a.clone(), mat(&[&[3], &[3]])], &mut ctx),
            Ok(mat(&[&[1], &[1]]))
        );
        let sing = mat(&[&[1, 1], &[1, 1]]);
        let fam = run("linsolve", &[sing.clone(), mat(&[&[2], &[2]])], &mut ctx).unwrap();
        assert_eq!(fam.to_string(), "[[-t_1 + 2], [t_1]]");
        let none = run("linsolve", &[sing, mat(&[&[1], &[2]])], &mut ctx).unwrap();
        assert_eq!(none, Expr::new(ExprNode::Set(Vec::new())));
        assert!(run("det", &[Expr::int(3)], &mut ctx).is_err());
        let v = Expr::new(ExprNode::Tuple(vec![
            Expr::int(1),
            Expr::int(2),
            Expr::int(2),
        ]));
        assert_eq!(run("norm", &[v], &mut ctx), Ok(Expr::int(3)));
    }
}
