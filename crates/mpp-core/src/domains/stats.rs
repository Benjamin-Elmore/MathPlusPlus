//! `binom` and basic statistics: mean, median, mode, and the sample or population
//! variance and standard deviation. Exact data gives exact results (`√` stays exact).

use std::cmp::Ordering;

use crate::domains::util::{self, expect_args, items};
use crate::domains::{Ctx, Domain, Value};
use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode};
use crate::functions::Func;
use crate::simplify::add;

/// The statistics domain.
pub struct Statistics;

const OPS: &[&str] = &[
    "binom", "mean", "median", "mode", "var", "pvar", "std", "pstd",
];

fn data(op: &str, args: &[Value]) -> MathResult<Vec<Expr>> {
    let v: Vec<Expr> = items(op, args)?.iter().map(Expr::simplify).collect();
    if v.is_empty() {
        return Err(MathError::Undefined(format!("the {op} of no data")));
    }
    Ok(v)
}

/// The arithmetic mean.
pub fn mean(xs: &[Expr]) -> Expr {
    (add(xs.to_vec()) / Expr::int(xs.len() as i64)).simplify()
}

/// Sorts real data; an error if some values can't be compared.
fn sorted(xs: &[Expr]) -> MathResult<Vec<Expr>> {
    let mut v = xs.to_vec();
    let mut failed = false;
    v.sort_by(|a, b| {
        util::compare(a, b).unwrap_or_else(|| {
            failed = true;
            Ordering::Equal
        })
    });
    if failed {
        return Err(MathError::Unsupported(
            "ordering symbolic data (the median needs numbers)".into(),
        ));
    }
    Ok(v)
}

/// The median (mean of the middle two for an even count).
pub fn median(xs: &[Expr]) -> MathResult<Expr> {
    let v = sorted(xs)?;
    let n = v.len();
    if n == 0 {
        return Err(MathError::Undefined("the median of no data".into()));
    }
    Ok(if n % 2 == 1 {
        v[n / 2].clone()
    } else {
        ((&v[n / 2 - 1] + &v[n / 2]) / Expr::int(2)).simplify()
    })
}

/// The most frequent values, ascending.
pub fn mode(xs: &[Expr]) -> MathResult<Vec<Expr>> {
    let mut counts: Vec<(Expr, usize)> = Vec::new();
    for x in xs {
        match counts.iter_mut().find(|(v, _)| util::equal(v, x)) {
            Some((_, c)) => *c += 1,
            None => counts.push((x.clone(), 1)),
        }
    }
    let best = counts.iter().map(|(_, c)| *c).max().unwrap_or(0);
    let modes: Vec<Expr> = counts
        .into_iter()
        .filter(|(_, c)| *c == best)
        .map(|(v, _)| v)
        .collect();
    sorted(&modes).or(Ok(modes))
}

/// The variance: sample (divide by n − 1) or population (divide by n).
pub fn variance(xs: &[Expr], sample: bool) -> MathResult<Expr> {
    let n = xs.len();
    if sample && n < 2 {
        return Err(MathError::Undefined(
            "the sample variance of fewer than two values".into(),
        ));
    }
    let m = mean(xs);
    let ss = add(xs.iter().map(|x| (x - &m).pow(2)).collect());
    let d = if sample { n - 1 } else { n };
    let v = (ss / Expr::int(d as i64)).simplify();
    Ok(v.expand().map(|e| e.simplify()).unwrap_or(v))
}

impl Domain for Statistics {
    fn name(&self) -> &'static str {
        "statistics"
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
        "binom" => {
            expect_args(op, args, 2)?;
            Expr::func(Func::Binomial, args.to_vec())
        }
        "mean" => Ok(mean(&data(op, args)?)),
        "median" => median(&data(op, args)?),
        "mode" => {
            let m = mode(&data(op, args)?)?;
            Ok(match m.as_slice() {
                [one] => one.clone(),
                _ => Expr::new(ExprNode::Set(m)),
            })
        }
        "var" | "pvar" | "std" | "pstd" => {
            let sample = matches!(op, "var" | "std");
            ctx.note(if sample {
                "sample (divides by n − 1)"
            } else {
                "population (divides by n)"
            });
            let v = variance(&data(op, args)?, sample)?;
            Ok(if op.ends_with("std") { v.sqrt() } else { v })
        }
        _ => Err(MathError::UnknownFunction(op.into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ints(v: &[i64]) -> Vec<Expr> {
        v.iter().map(|&k| Expr::int(k)).collect()
    }

    #[test]
    fn central_values() {
        let xs = ints(&[2, 4, 4, 4, 5, 5, 7, 9]);
        assert_eq!(mean(&xs), Expr::int(5));
        assert_eq!(median(&xs).unwrap(), Expr::rational(9, 2));
        assert_eq!(mode(&xs).unwrap(), ints(&[4]));
        assert_eq!(median(&ints(&[3, 1, 2])).unwrap(), Expr::int(2));
        assert_eq!(mean(&ints(&[1, 2])), Expr::rational(3, 2));
    }

    #[test]
    fn spread() {
        let xs = ints(&[2, 4, 4, 4, 5, 5, 7, 9]);
        assert_eq!(variance(&xs, false).unwrap(), Expr::int(4));
        assert_eq!(variance(&xs, true).unwrap(), Expr::rational(32, 7));
        let mut ctx = Ctx::new();
        let t = Expr::new(ExprNode::Tuple(xs));
        let sd = apply("pstd", std::slice::from_ref(&t), &mut ctx).unwrap();
        assert_eq!(sd, Expr::int(2));
        assert!(ctx.notes.iter().any(|n| n.contains("population")));
        let sd = apply("std", &[t], &mut ctx).unwrap();
        assert_eq!(sd, Expr::rational(32, 7).sqrt());
        assert!(variance(&ints(&[1]), true).is_err());
    }

    #[test]
    fn symbolic_mean_and_binomial() {
        let (a, b) = (Expr::sym("a"), Expr::sym("b"));
        assert_eq!(
            mean(&[a.clone(), b.clone()]),
            (a.clone() + b.clone()) / Expr::int(2)
        );
        assert!(median(&[a, b]).is_err());
        let mut ctx = Ctx::new();
        assert_eq!(
            apply("binom", &ints(&[5, 2]), &mut ctx).unwrap(),
            Expr::int(10)
        );
        assert!(apply("mean", &[], &mut ctx).is_err());
    }
}
