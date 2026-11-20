//! Probability and information: Markov chains (exact `P^k` and stationary
//! distributions), seeded random sampling ([`rng::Rng`], the generator behind
//! `%seed`), Monte Carlo integrals and sums with their standard error, Brownian paths,
//! entropy, mutual information and channel capacity.

pub mod information;
pub mod rng;

use crate::domains::fieldkit::{
    domain_err, float, real_list_of, real_matrix, tuple, usize_arg, NumericFn,
};
use crate::domains::linalg::Matrix;
use crate::domains::util::{
    equal, expect_args, expect_args_between, int_arg, is_zero, real_arg, sym_arg,
};
use crate::domains::{Ctx, Domain, Value};
use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode};

use num_traits::ToPrimitive;
use rng::Rng;

/// Most samples drawn by one operator.
pub const MAX_SAMPLES: usize = 10_000_000;
/// Largest Markov chain.
const MAX_STATES: usize = 64;

/// The probability and information domain.
pub struct Probability;

const OPS: &[&str] = &[
    "markovpower",
    "stationary",
    "runif",
    "rnorm",
    "montecarlo",
    "brownian",
    "entropy",
    "jointentropy",
    "condentropy",
    "mutualinfo",
    "bsccapacity",
    "capacity",
];

impl Domain for Probability {
    fn name(&self) -> &'static str {
        "probability and information"
    }

    fn operators(&self) -> &'static [&'static str] {
        OPS
    }

    fn apply(&self, op: &str, args: &[Value], ctx: &mut Ctx<'_>) -> Option<MathResult<Value>> {
        Some(apply(op, args, ctx))
    }
}

/// Reads a seed: any integer, reduced to 64 bits (negative seeds wrap).
pub fn seed_arg(op: &str, e: &Expr) -> MathResult<u64> {
    let n = int_arg(op, e)?;
    let (sign, digits) = n.to_u64_digits();
    let low = digits.first().copied().unwrap_or(0);
    Ok(if sign == num_bigint::Sign::Minus {
        low.wrapping_neg()
    } else {
        low
    })
}

/// A square matrix whose rows are probability vectors.
fn stochastic(op: &str, e: &Expr) -> MathResult<Matrix> {
    let m = Matrix::from_expr(e).ok_or_else(|| domain_err(op, format!("{e} is not a matrix")))?;
    if !m.is_square() || m.rows() > MAX_STATES {
        return Err(domain_err(
            op,
            "a transition matrix must be square (at most 64 states)",
        ));
    }
    for r in 0..m.rows() {
        let row: Vec<Expr> = (0..m.cols()).map(|c| m.get(r, c).clone()).collect();
        let sum = crate::simplify::add(row.clone());
        if !equal(&sum, &Expr::one()) {
            return Err(domain_err(
                op,
                format!("row {} sums to {sum}, not 1", r + 1),
            ));
        }
        if row
            .iter()
            .any(|x| crate::domains::util::sign(x) == Some(std::cmp::Ordering::Less))
        {
            return Err(domain_err(
                op,
                format!("row {} has a negative probability", r + 1),
            ));
        }
    }
    Ok(m)
}

/// `P^k` by repeated squaring (exact for exact entries).
pub fn matrix_power(p: &Matrix, k: u64) -> MathResult<Matrix> {
    let mut acc = Matrix::identity(p.rows())?;
    let mut base = p.clone();
    let mut k = k;
    while k > 0 {
        if k & 1 == 1 {
            acc = acc.mul(&base)?.simplified();
        }
        k >>= 1;
        if k > 0 {
            base = base.mul(&base)?.simplified();
        }
    }
    Ok(acc)
}

/// The stationary distribution π with πP = π and Σπ = 1, when it is unique.
pub fn stationary(p: &Matrix) -> MathResult<Vec<Expr>> {
    let n = p.rows();
    let a = p.transpose().sub(&Matrix::identity(n)?)?;
    let null = a.nullspace()?;
    match null.as_slice() {
        [v] => {
            let entries = v.entries().to_vec();
            let sum = crate::simplify::add(entries.clone()).simplify();
            if is_zero(&sum) {
                return Err(MathError::Undefined(
                    "a normalisable stationary distribution".into(),
                ));
            }
            Ok(entries.iter().map(|x| (x / &sum).simplify()).collect())
        }
        [] => Err(MathError::Undefined("a stationary distribution".into())),
        _ => Err(MathError::Inconclusive(format!(
            "a unique stationary distribution (the chain has {} closed classes)",
            null.len()
        ))),
    }
}

/// One coordinate range `(x, a, b)` of a Monte Carlo box.
fn ranges(op: &str, e: &Expr) -> MathResult<Vec<(String, f64, f64)>> {
    let items = match e.node() {
        ExprNode::Tuple(v) if v.iter().all(|x| matches!(x.node(), ExprNode::Tuple(_))) => v.clone(),
        _ => vec![e.clone()],
    };
    items
        .iter()
        .map(|r| match r.node() {
            ExprNode::Tuple(t) if t.len() == 3 => Ok((
                sym_arg(op, &t[0])?,
                real_arg(op, &t[1])?,
                real_arg(op, &t[2])?,
            )),
            _ => Err(domain_err(op, format!("{r} is not a range (x, a, b)"))),
        })
        .collect()
}

/// Monte Carlo estimate of `∫_box f` with its standard error: points are drawn
/// coordinate by coordinate, in the order the ranges are listed.
pub fn monte_carlo(
    f: &Expr,
    box_: &[(String, f64, f64)],
    n: usize,
    seed: u64,
) -> MathResult<(f64, f64)> {
    if !(2..=MAX_SAMPLES).contains(&n) {
        return Err(MathError::TooLarge(format!(
            "{n} samples (2 … {MAX_SAMPLES})"
        )));
    }
    let names: Vec<String> = box_.iter().map(|r| r.0.clone()).collect();
    let f = NumericFn::new(f, &names);
    let volume: f64 = box_.iter().map(|r| r.2 - r.1).product();
    let mut rng = Rng::seeded(seed);
    let mut point = vec![0.0; box_.len()];
    // Welford's running mean and variance.
    let (mut mean, mut m2) = (0.0f64, 0.0f64);
    for i in 1..=n {
        for (slot, r) in point.iter_mut().zip(box_) {
            *slot = rng.uniform_in(r.1, r.2);
        }
        let v = f.eval(&point)?;
        let delta = v - mean;
        mean += delta / i as f64;
        m2 += delta * (v - mean);
    }
    let var = m2 / (n - 1) as f64;
    Ok((volume * mean, volume * (var / n as f64).sqrt()))
}

/// A standard Brownian path `W(t_k)`, `t_k = kT/n`, `k = 0 … n`.
pub fn brownian(n: usize, t: f64, seed: u64) -> MathResult<Vec<f64>> {
    if n == 0 || n > MAX_SAMPLES || t.is_nan() || t <= 0.0 || !t.is_finite() {
        return Err(MathError::Domain {
            function: "brownian".into(),
            detail: format!("needs 1 … {MAX_SAMPLES} steps and a positive time"),
        });
    }
    let mut rng = Rng::seeded(seed);
    let sd = (t / n as f64).sqrt();
    let mut w = Vec::with_capacity(n + 1);
    w.push(0.0);
    let mut cur = 0.0;
    for _ in 0..n {
        cur += sd * rng.normal();
        w.push(cur);
    }
    Ok(w)
}

fn joint(op: &str, e: &Expr) -> MathResult<Vec<Vec<f64>>> {
    real_matrix(op, e, 1_000)
}

fn apply(op: &str, args: &[Value], ctx: &mut Ctx<'_>) -> MathResult<Value> {
    match op {
        "markovpower" => {
            expect_args(op, args, 2)?;
            let p = stochastic(op, &args[0])?;
            let k = int_arg(op, &args[1])?
                .to_u64()
                .filter(|&k| k <= 1 << 40)
                .ok_or_else(|| domain_err(op, "the number of steps must be 0 … 2⁴⁰"))?;
            Ok(matrix_power(&p, k)?.to_expr())
        }
        "stationary" => {
            expect_args(op, args, 1)?;
            Ok(tuple(stationary(&stochastic(op, &args[0])?)?))
        }
        "runif" | "rnorm" => {
            expect_args_between(op, args, 2, 4)?;
            let seed = seed_arg(op, &args[0])?;
            let n = usize_arg(op, &args[1], MAX_SAMPLES)?;
            let (a, b) = match (args.get(2), args.get(3)) {
                (Some(a), Some(b)) => (real_arg(op, a)?, real_arg(op, b)?),
                (None, None) => (0.0, 1.0),
                _ => return Err(crate::domains::util::arity(op, "2 or 4", args.len())),
            };
            if op == "rnorm" && b <= 0.0 {
                return Err(domain_err(op, "the standard deviation must be positive"));
            }
            let mut rng = Rng::seeded(seed);
            let vals: Vec<Expr> = (0..n)
                .map(|_| {
                    float(if op == "runif" {
                        rng.uniform_in(a, b)
                    } else {
                        a + b * rng.normal()
                    })
                })
                .collect();
            ctx.note(format!("seeded with {seed} (xoshiro256**)"));
            Ok(tuple(vals))
        }
        "montecarlo" => {
            expect_args(op, args, 4)?;
            let box_ = ranges(op, &args[1])?;
            let n = usize_arg(op, &args[2], MAX_SAMPLES)?;
            let seed = seed_arg(op, &args[3])?;
            let (est, se) = monte_carlo(&args[0], &box_, n, seed)?;
            ctx.note(format!(
                "Monte Carlo with {n} samples, seed {seed}: (estimate, standard error)"
            ));
            Ok(tuple(vec![float(est), float(se)]))
        }
        "brownian" => {
            expect_args(op, args, 3)?;
            let seed = seed_arg(op, &args[0])?;
            let n = usize_arg(op, &args[1], MAX_SAMPLES)?;
            let t = real_arg(op, &args[2])?;
            Ok(tuple(
                brownian(n, t, seed)?.into_iter().map(float).collect(),
            ))
        }
        "entropy" => {
            expect_args_between(op, args, 1, 2)?;
            let p = real_list_of(op, &args[0])?;
            let base = match args.get(1) {
                Some(b) => real_arg(op, b)?,
                None => 2.0,
            };
            Ok(float(information::entropy(&p, base)?))
        }
        "jointentropy" | "condentropy" | "mutualinfo" => {
            expect_args(op, args, 1)?;
            let j = joint(op, &args[0])?;
            let (hx, _hy, hxy) = information::entropies(&j)?;
            Ok(float(match op {
                "jointentropy" => hxy,
                // H(Y | X) = H(X, Y) − H(X), rows indexed by x.
                "condentropy" => (hxy - hx).max(0.0),
                _ => information::mutual_information(&j)?,
            }))
        }
        "bsccapacity" => {
            expect_args(op, args, 1)?;
            let p = &args[0];
            if p.is_zero() || p.is_one() {
                return Ok(Expr::one());
            }
            if equal(p, &Expr::rational(1, 2)) {
                return Ok(Expr::zero());
            }
            Ok(float(information::bsc_capacity(real_arg(op, p)?)?))
        }
        "capacity" => {
            expect_args(op, args, 1)?;
            let w = real_matrix(op, &args[0], 256)?;
            let (c, p) = information::channel_capacity(&w, 1e-12)?;
            ctx.note(format!(
                "optimal input distribution ({})",
                p.iter()
                    .map(|x| format!("{x:.6}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            Ok(float(c))
        }
        _ => Err(MathError::UnknownFunction(op.into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::fieldkit::close;
    use crate::domains::fieldkit::list_of;
    use crate::domains::util::numeric;

    fn run(op: &str, args: &[Expr]) -> MathResult<Expr> {
        Probability.apply(op, args, &mut Ctx::new()).unwrap()
    }

    fn m(rows: Vec<Vec<Expr>>) -> Expr {
        Expr::matrix(rows).unwrap()
    }

    #[test]
    fn markov_chains() {
        // Two-state chain with P = [[1/2, 1/2], [1/4, 3/4]]: π = (1/3, 2/3).
        let h = Expr::rational(1, 2);
        let p = m(vec![
            vec![h.clone(), h.clone()],
            vec![Expr::rational(1, 4), Expr::rational(3, 4)],
        ]);
        let pi = run("stationary", std::slice::from_ref(&p)).unwrap();
        assert_eq!(pi, tuple(vec![Expr::rational(1, 3), Expr::rational(2, 3)]));
        let p2 = run("markovpower", &[p.clone(), Expr::int(2)]).unwrap();
        assert_eq!(
            p2.to_string(),
            m(vec![
                vec![Expr::rational(3, 8), Expr::rational(5, 8)],
                vec![Expr::rational(5, 16), Expr::rational(11, 16)]
            ])
            .to_string()
        );
        // Rows of P^k converge to π.
        let p40 = matrix_power(&Matrix::from_expr(&p).unwrap(), 40).unwrap();
        assert!(close(numeric(p40.get(0, 0)).unwrap(), 1.0 / 3.0, 1e-12));
        let bad = m(vec![
            vec![h.clone(), h.clone()],
            vec![h.clone(), Expr::one()],
        ]);
        assert!(run("stationary", &[bad]).is_err());
        let reducible = m(vec![
            vec![Expr::one(), Expr::zero()],
            vec![Expr::zero(), Expr::one()],
        ]);
        assert!(run("stationary", &[reducible]).is_err());
    }

    #[test]
    fn monte_carlo_pi_lab_11() {
        // Lab 11: 4·1[x² + y² ≤ 1] over the unit square, seed 42; within 3σ of π.
        let (x, y) = (Expr::sym("x"), Expr::sym("y"));
        let inside = Expr::relation(crate::expr::RelOp::Le, x.pow(2) + y.pow(2), Expr::one());
        let f = Expr::int(4) * inside;
        let unit = |v: &Expr| tuple(vec![v.clone(), Expr::zero(), Expr::one()]);
        let box_ = tuple(vec![unit(&x), unit(&y)]);
        let n = 100_000;
        let r = run(
            "montecarlo",
            &[f.clone(), box_.clone(), Expr::int(n), Expr::int(42)],
        )
        .unwrap();
        let again = run("montecarlo", &[f, box_, Expr::int(n), Expr::int(42)]).unwrap();
        assert_eq!(r, again, "same seed, same estimate");
        let v = list_of("t", &r).unwrap();
        let (est, se) = (numeric(&v[0]).unwrap(), numeric(&v[1]).unwrap());
        let sigma = 4.0
            * (std::f64::consts::FRAC_PI_4 * (1.0 - std::f64::consts::FRAC_PI_4) / n as f64).sqrt();
        assert!((est - std::f64::consts::PI).abs() < 3.0 * sigma, "{est}");
        assert!(close(se, sigma, 0.02));
    }

    #[test]
    fn sampling_and_paths() {
        let u = run("runif", &[Expr::int(1), Expr::int(3)]).unwrap();
        assert_eq!(u, run("runif", &[Expr::int(1), Expr::int(3)]).unwrap());
        assert_eq!(list_of("t", &u).unwrap().len(), 3);
        // Quadratic variation of a Brownian path on [0, T] is close to T (catalogue).
        let w = brownian(100_000, 2.0, 9).unwrap();
        let qv: f64 = w.windows(2).map(|p| (p[1] - p[0]).powi(2)).sum();
        assert!((qv - 2.0).abs() < 0.05);
        assert_eq!(w[0], 0.0);
        assert!(run(
            "rnorm",
            &[Expr::int(1), Expr::int(3), Expr::zero(), Expr::int(-1)]
        )
        .is_err());
    }

    #[test]
    fn information_operators() {
        let h = Expr::rational(1, 2);
        assert_eq!(
            run("entropy", &[tuple(vec![h.clone(), h.clone()])]).unwrap(),
            float(1.0)
        );
        assert_eq!(
            run("bsccapacity", std::slice::from_ref(&h)),
            Ok(Expr::zero())
        );
        assert_eq!(run("bsccapacity", &[Expr::zero()]), Ok(Expr::one()));
        let q = Expr::rational(1, 4);
        let ind = m(vec![vec![q.clone(), q.clone()], vec![q.clone(), q.clone()]]);
        assert_eq!(
            run("jointentropy", std::slice::from_ref(&ind)).unwrap(),
            float(2.0)
        );
        assert_eq!(
            run("condentropy", std::slice::from_ref(&ind)).unwrap(),
            float(1.0)
        );
        assert!(numeric(&run("mutualinfo", &[ind]).unwrap()).unwrap().abs() < 1e-15);
        let bsc = m(vec![
            vec![Expr::float(0.9), Expr::float(0.1)],
            vec![Expr::float(0.1), Expr::float(0.9)],
        ]);
        let c = numeric(&run("capacity", &[bsc]).unwrap()).unwrap();
        assert!(close(c, information::bsc_capacity(0.1).unwrap(), 1e-9));
        assert_eq!(seed_arg("s", &Expr::int(-1)).unwrap(), u64::MAX);
    }
}
