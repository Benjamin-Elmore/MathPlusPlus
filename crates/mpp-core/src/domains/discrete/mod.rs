//! Discrete mathematics: Catalan numbers, the hook-length formula, Burnside counting
//! (any small permutation group, plus necklaces and bracelets in closed form), graphs
//! as values, small Ramsey checks by search, Hamming distance and linear codes.

pub mod codes;
pub mod combinatorics;
pub mod graph;
pub mod perm;
pub mod ramsey;

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::ToPrimitive;

use crate::domains::fieldkit::{
    domain_err, list_of, matrix_expr, matrix_rows, truth, tuple, usize_arg,
};
use crate::domains::number_theory::is_prime;
use crate::domains::util::{equal, expect_args, expect_args_between, int_arg};
use crate::domains::{Ctx, Domain, Value};
use crate::error::{MathError, MathResult};
use crate::expr::Expr;

/// The discrete mathematics domain.
pub struct Discrete;

const OPS: &[&str] = &[
    "catalan",
    "hooklength",
    "burnside",
    "necklaces",
    "bracelets",
    "graphdegrees",
    "adjacency",
    "graphlaplacian",
    "istree",
    "spanningtrees",
    "ramsey",
    "ramseynumber",
    "hamming",
    "codeparams",
    "hammingcode",
];

/// Largest alphabet for linear codes.
const MAX_Q: u64 = 1 << 20;

impl Domain for Discrete {
    fn name(&self) -> &'static str {
        "discrete mathematics"
    }

    fn operators(&self) -> &'static [&'static str] {
        OPS
    }

    fn apply(&self, op: &str, args: &[Value], ctx: &mut Ctx<'_>) -> Option<MathResult<Value>> {
        Some(apply(op, args, ctx))
    }
}

fn int_matrix(rows: &[Vec<i64>]) -> Expr {
    matrix_expr(
        rows.iter()
            .map(|r| r.iter().map(|&x| Expr::int(x)).collect())
            .collect(),
    )
}

fn graph(op: &str, args: &[Value]) -> MathResult<graph::Graph> {
    expect_args_between(op, args, 1, 2)?;
    let n = match args.get(1) {
        Some(n) => Some(usize_arg(op, n, graph::MAX_VERTICES)?),
        None => None,
    };
    graph::graph_arg(op, &args[0], n)
}

fn small(op: &str, e: &Expr) -> MathResult<usize> {
    usize_arg(op, e, 64)
}

fn apply(op: &str, args: &[Value], ctx: &mut Ctx<'_>) -> MathResult<Value> {
    match op {
        "catalan" => {
            expect_args(op, args, 1)?;
            let n = usize_arg(op, &args[0], combinatorics::MAX_CATALAN)?;
            Ok(Expr::bigint(combinatorics::catalan(n)?))
        }
        "hooklength" => {
            expect_args(op, args, 1)?;
            let shape = list_of(op, &args[0])?
                .iter()
                .map(|x| usize_arg(op, x, combinatorics::MAX_CELLS))
                .collect::<MathResult<Vec<_>>>()?;
            Ok(Expr::bigint(combinatorics::hook_length(&shape)?))
        }
        "burnside" => {
            expect_args(op, args, 2)?;
            let group = perm::closure(&perm::generators_arg(op, &args[0])?)?;
            ctx.note(format!("the group has {} elements", group.len()));
            Ok(Expr::bigint(perm::burnside(
                &group,
                &int_arg(op, &args[1])?,
            )?))
        }
        "necklaces" | "bracelets" => {
            expect_args(op, args, 2)?;
            let n = usize_arg(op, &args[0], combinatorics::MAX_BEADS)?;
            let k = int_arg(op, &args[1])?;
            if k.sign() == num_bigint::Sign::Minus {
                return Err(domain_err(op, "the number of colours must be non-negative"));
            }
            Ok(Expr::bigint(if op == "necklaces" {
                combinatorics::necklaces(n, &k)?
            } else {
                combinatorics::bracelets(n, &k)?
            }))
        }
        "graphdegrees" => {
            let g = graph(op, args)?;
            Ok(tuple(
                g.degrees()
                    .into_iter()
                    .map(|d| Expr::int(d as i64))
                    .collect(),
            ))
        }
        "adjacency" => Ok(int_matrix(&graph(op, args)?.adjacency())),
        "graphlaplacian" => Ok(int_matrix(&graph(op, args)?.laplacian())),
        "istree" => Ok(truth(graph(op, args)?.is_tree())),
        "spanningtrees" => Ok(Expr::bigint(graph(op, args)?.spanning_trees())),
        "ramsey" => {
            expect_args(op, args, 3)?;
            let (s, t) = (small(op, &args[0])?, small(op, &args[1])?);
            let n = usize_arg(op, &args[2], ramsey::MAX_N)?;
            match ramsey::counterexample(s, t, n)? {
                None => Ok(truth(true)),
                Some(red) => {
                    let list: Vec<String> = red.iter().map(|(u, v)| format!("{u}{v}")).collect();
                    ctx.note(format!(
                        "colouring with no red K_{s} and no blue K_{t}: red edges {}",
                        if list.is_empty() {
                            "none".into()
                        } else {
                            list.join(", ")
                        }
                    ));
                    Ok(truth(false))
                }
            }
        }
        "ramseynumber" => {
            expect_args(op, args, 2)?;
            let (s, t) = (small(op, &args[0])?, small(op, &args[1])?);
            match ramsey::ramsey_number(s, t)? {
                Some(n) => Ok(Expr::int(n as i64)),
                None => Err(MathError::TooLarge(format!(
                    "R({s}, {t}) by search (only K_n up to n = {} is searched)",
                    ramsey::MAX_N
                ))),
            }
        }
        "hamming" => {
            expect_args(op, args, 2)?;
            if let (Some(a), Some(b)) = (
                crate::domains::util::integer(&args[0]),
                crate::domains::util::integer(&args[1]),
            ) {
                // Two non-negative integers: count differing bits.
                if a.sign() != num_bigint::Sign::Minus && b.sign() != num_bigint::Sign::Minus {
                    return Ok(Expr::int((a ^ b).magnitude().count_ones() as i64));
                }
            }
            let (a, b) = (list_of(op, &args[0])?, list_of(op, &args[1])?);
            if a.len() != b.len() {
                return Err(domain_err(op, "the words have different lengths"));
            }
            let d = a.iter().zip(&b).filter(|(x, y)| !equal(x, y)).count();
            Ok(Expr::int(d as i64))
        }
        "codeparams" => {
            expect_args(op, args, 2)?;
            let q = int_arg(op, &args[1])?
                .to_u64()
                .filter(|&q| q <= MAX_Q)
                .ok_or_else(|| {
                    MathError::TooLarge(format!("an alphabet of size {} (q ≤ {MAX_Q})", args[1]))
                })?;
            if !is_prime(&BigInt::from(q))? {
                return Err(domain_err(op, format!("q = {q} must be a prime")));
            }
            let pm = BigInt::from(q);
            let rows = matrix_rows(op, &args[0])?
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|x| Ok(int_arg(op, x)?.mod_floor(&pm).to_u64().unwrap_or(0)))
                        .collect::<MathResult<Vec<u64>>>()
                })
                .collect::<MathResult<Vec<_>>>()?;
            let (n, k, d) = codes::parameters(&rows, q)?;
            if k > 0 {
                ctx.note(format!(
                    "Singleton bound: d = {d} ≤ n − k + 1 = {}",
                    n - k + 1
                ));
            }
            Ok(tuple(vec![
                Expr::int(n as i64),
                Expr::int(k as i64),
                Expr::int(d as i64),
            ]))
        }
        "hammingcode" => {
            expect_args(op, args, 1)?;
            let m = usize_arg(op, &args[0], codes::MAX_HAMMING_M)?;
            let g = codes::hamming_code(m)?;
            Ok(matrix_expr(
                g.iter()
                    .map(|r| r.iter().map(|&x| Expr::int(x as i64)).collect())
                    .collect(),
            ))
        }
        _ => Err(MathError::UnknownFunction(op.into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(op: &str, args: &[Expr]) -> MathResult<Expr> {
        Discrete.apply(op, args, &mut Ctx::new()).unwrap()
    }

    fn t(v: &[i64]) -> Expr {
        tuple(v.iter().map(|&x| Expr::int(x)).collect())
    }

    fn edges(v: &[(i64, i64)]) -> Expr {
        tuple(v.iter().map(|&(a, b)| t(&[a, b])).collect())
    }

    #[test]
    fn operators() {
        assert_eq!(run("catalan", &[Expr::int(5)]), Ok(Expr::int(42)));
        assert_eq!(run("hooklength", &[t(&[3, 2])]), Ok(Expr::int(5)));
        // Colourings of a square's corners with 3 colours up to rotation: 24 (OEIS A000031… k = 3).
        let gens = tuple(vec![t(&[2, 3, 4, 1])]);
        assert_eq!(run("burnside", &[gens, Expr::int(3)]), Ok(Expr::int(24)));
        assert_eq!(
            run("necklaces", &[Expr::int(4), Expr::int(3)]),
            Ok(Expr::int(24))
        );
        assert_eq!(
            run("bracelets", &[Expr::int(6), Expr::int(2)]),
            Ok(Expr::int(13))
        );
        let path = edges(&[(1, 2), (2, 3), (3, 4)]);
        assert_eq!(
            run("graphdegrees", std::slice::from_ref(&path)),
            Ok(t(&[1, 2, 2, 1]))
        );
        assert_eq!(run("istree", std::slice::from_ref(&path)), Ok(truth(true)));
        assert_eq!(
            run("istree", &[path.clone(), Expr::int(5)]),
            Ok(truth(false))
        );
        let k4 = edges(&[(1, 2), (1, 3), (1, 4), (2, 3), (2, 4), (3, 4)]);
        assert_eq!(
            run("spanningtrees", std::slice::from_ref(&k4)),
            Ok(Expr::int(16))
        );
        let adj = run("adjacency", std::slice::from_ref(&path)).unwrap();
        assert_eq!(run("spanningtrees", &[adj]), Ok(Expr::int(1)));
        let lap = run("graphlaplacian", &[path]).unwrap();
        assert_eq!(lap.to_string().matches("-1").count(), 6);
        assert_eq!(
            run("ramsey", &[Expr::int(3), Expr::int(3), Expr::int(6)]),
            Ok(truth(true))
        );
        assert_eq!(
            run("ramsey", &[Expr::int(3), Expr::int(3), Expr::int(5)]),
            Ok(truth(false))
        );
        assert_eq!(
            run("ramseynumber", &[Expr::int(3), Expr::int(3)]),
            Ok(Expr::int(6))
        );
        assert_eq!(
            run("hamming", &[t(&[1, 0, 1]), t(&[0, 0, 1])]),
            Ok(Expr::int(1))
        );
        assert_eq!(
            run("hamming", &[Expr::int(0b1011), Expr::int(0b0110)]),
            Ok(Expr::int(3))
        );
        let g = run("hammingcode", &[Expr::int(3)]).unwrap();
        assert_eq!(run("codeparams", &[g, Expr::int(2)]), Ok(t(&[7, 4, 3])));
        assert!(run(
            "codeparams",
            &[run("hammingcode", &[Expr::int(3)]).unwrap(), Expr::int(4)]
        )
        .is_err());
    }
}
