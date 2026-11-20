//! Analysis tools: the distributions δ, δ^{(j)} and the Heaviside step (symbolic, with
//! the pairing `⟨δ^{(j)}, ψ⟩ = (−1)^j ψ^{(j)}(0)`), L^p norms of vectors and functions,
//! the discrete Fourier transform (direct and fast), Bernstein polynomials and
//! fixed-point iteration with a convergence check.
//!
//! The derivatives `H′ = δ` and `(δ^{(j)})′ = δ^{(j+1)}` are in
//! [`crate::domains::special::partial`] with the other derivative rules.

pub mod approx;
pub mod fourier;
pub mod quad;

use num_complex::Complex64;

use crate::domains::fieldkit::{
    complex, complex_list_of, domain_err, eval_at, float, list_of, tuple, usize_arg,
};
use crate::domains::util::{
    compare, expect_args, expect_args_between, numeric, rational, real_arg, sign, sym_arg,
};
use crate::domains::{Ctx, Domain, Value};
use crate::error::{MathError, MathResult};
use crate::expr::Expr;
use crate::functions::{func, Func};

/// The analysis domain.
pub struct Analysis;

const OPS: &[&str] = &[
    "heaviside",
    "dirac",
    "diracpair",
    "lpnorm",
    "dft",
    "idft",
    "fft",
    "ifft",
    "bernstein",
    "bernsteinbasis",
    "fixedpoint",
];

/// Samples used for the sup norm of a function.
const SUP_SAMPLES: usize = 4_001;

impl Domain for Analysis {
    fn name(&self) -> &'static str {
        "analysis"
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
        "heaviside" => {
            expect_args(op, args, 1)?;
            let x = &args[0];
            if !x.is_constant() {
                return Ok(Expr::apply(op, args.to_vec()));
            }
            Ok(match sign(x) {
                Some(std::cmp::Ordering::Greater) => Expr::one(),
                Some(std::cmp::Ordering::Less) => Expr::zero(),
                Some(std::cmp::Ordering::Equal) => {
                    ctx.note("H(0) = 1/2 by the half-maximum convention");
                    Expr::rational(1, 2)
                }
                None => return Err(domain_err(op, format!("{x} is not a real number"))),
            })
        }
        "dirac" => {
            expect_args_between(op, args, 1, 2)?;
            let x = &args[0];
            let order = match args.get(1) {
                Some(j) => usize_arg(op, j, 1_000)?,
                None => 0,
            };
            if !x.is_constant() {
                return Ok(if order == 0 {
                    Expr::apply(op, vec![x.clone()])
                } else {
                    Expr::apply(op, vec![x.clone(), Expr::int(order as i64)])
                });
            }
            match sign(x) {
                Some(std::cmp::Ordering::Equal) => Err(MathError::Undefined(
                    "δ at 0 (δ is a distribution: pair it with a test function using diracpair)"
                        .into(),
                )),
                Some(_) => Ok(Expr::zero()),
                None => Err(domain_err(op, format!("{x} is not a real number"))),
            }
        }
        "diracpair" => {
            // diracpair(ψ, x[, j[, a]]) = ⟨δ_a^{(j)}, ψ⟩ = (−1)^j ψ^{(j)}(a)
            expect_args_between(op, args, 2, 4)?;
            let var = sym_arg(op, &args[1])?;
            let order = match args.get(2) {
                Some(j) => usize_arg(op, j, 64)?,
                None => 0,
            };
            let at = args.get(3).cloned().unwrap_or_else(Expr::zero);
            let mut d = args[0].clone();
            for _ in 0..order {
                d = ctx.calculus.diff(&d, &var)?;
            }
            let v = d.subs(&var, &at).simplify();
            Ok(if order % 2 == 1 { (-v).simplify() } else { v })
        }
        "lpnorm" => lp_norm(op, args, ctx),
        "dft" | "idft" | "fft" | "ifft" => {
            expect_args(op, args, 1)?;
            let x = complex_list_of(op, &args[0])?;
            let inverse = op.starts_with('i');
            let out = if op.ends_with("dft") {
                fourier::dft(&x, inverse)?
            } else {
                fourier::fft(&x, inverse)?
            };
            ctx.note("≈ numeric");
            Ok(tuple(out.into_iter().map(clean).map(complex).collect()))
        }
        "bernstein" => {
            expect_args(op, args, 3)?;
            let var = sym_arg(op, &args[1])?;
            let n = usize_arg(op, &args[2], approx::MAX_BERNSTEIN)?;
            approx::bernstein(&args[0], &var, n)
        }
        "bernsteinbasis" => {
            expect_args(op, args, 3)?;
            let k = usize_arg(op, &args[0], approx::MAX_BERNSTEIN)?;
            let n = usize_arg(op, &args[1], approx::MAX_BERNSTEIN)?;
            Ok(approx::basis(k, n, &args[2])?.simplify())
        }
        "fixedpoint" => {
            expect_args_between(op, args, 3, 5)?;
            let var = sym_arg(op, &args[1])?;
            let x0 = real_arg(op, &args[2])?;
            let tol = match args.get(3) {
                Some(t) => real_arg(op, t)?.abs().max(1e-15),
                None => 1e-12,
            };
            let max_iter = match args.get(4) {
                Some(m) => usize_arg(op, m, approx::MAX_ITER)?,
                None => 10_000,
            };
            let r = approx::fixed_point(&args[0], &var, x0, tol, max_iter)?;
            ctx.note(format!(
                "converged in {} steps; |g′(x*)| ≈ {:.3} {}",
                r.iterations,
                r.rate,
                if r.rate < 1.0 {
                    "< 1, so the fixed point attracts nearby starts"
                } else {
                    "≥ 1, so the fixed point is not attracting"
                }
            ));
            Ok(float(r.value))
        }
        _ => Err(MathError::UnknownFunction(op.into())),
    }
}

/// Rounds away the last-bit noise of transforms (|parts| below 1e-12 of the scale).
fn clean(z: Complex64) -> Complex64 {
    let scale = z.norm().max(1.0);
    let fix = |v: f64| if v.abs() < 1e-12 * scale { 0.0 } else { v };
    Complex64::new(fix(z.re), fix(z.im))
}

/// `lpnorm(v, p)` for a vector (exact for exact entries), or
/// `lpnorm(f, x, a, b, p)` for a function on [a, b] (numeric).
fn lp_norm(op: &str, args: &[Value], ctx: &mut Ctx<'_>) -> MathResult<Value> {
    let p_arg = args
        .last()
        .ok_or_else(|| crate::domains::util::arity(op, "2 or 5", 0))?;
    let infinite = p_arg.is_infinite();
    let p = if infinite {
        f64::INFINITY
    } else {
        let p = real_arg(op, p_arg)?;
        if p < 1.0 {
            return Err(domain_err(op, format!("an L^p norm needs p ≥ 1, not {p}")));
        }
        p
    };
    match args.len() {
        2 => {
            let v = list_of(op, &args[0])?;
            if infinite {
                let mut best = Expr::zero();
                for x in &v {
                    let a = func(Func::Abs, vec![x.clone()]);
                    match compare(&a, &best) {
                        Some(std::cmp::Ordering::Greater) => best = a,
                        Some(_) => {}
                        None => return Ok(Expr::apply(op, args.to_vec())),
                    }
                }
                return Ok(best.simplify());
            }
            let pe = p_arg.clone();
            let sum = crate::simplify::add(
                v.iter()
                    .map(|x| func(Func::Abs, vec![x.clone()]).pow(pe.clone()))
                    .collect(),
            );
            let out = sum.pow(pe.recip()).simplify();
            if rational(p_arg).is_none() {
                ctx.note("≈ numeric");
                if let Some(n) = numeric(&out) {
                    return Ok(float(n));
                }
            }
            Ok(out)
        }
        5 => {
            let var = sym_arg(op, &args[1])?;
            let (a, b) = (real_arg(op, &args[2])?, real_arg(op, &args[3])?);
            if a >= b {
                return Err(domain_err(op, "the interval must have a < b"));
            }
            let names = [var];
            let f = &args[0];
            ctx.note("≈ numeric");
            if infinite {
                ctx.note("sup norm estimated by sampling");
                let mut best = 0.0f64;
                for i in 0..SUP_SAMPLES {
                    let t = a + (b - a) * i as f64 / (SUP_SAMPLES - 1) as f64;
                    best = best.max(eval_at(f, &names, &[t])?.abs());
                }
                return Ok(float(best));
            }
            let mut g = |t: f64| eval_at(f, &names, &[t]).map(|v| v.abs().powf(p));
            let integral = quad::integrate(&mut g, a, b, 1e-11)?;
            Ok(float(integral.powf(1.0 / p)))
        }
        n => Err(crate::domains::util::arity(op, "2 or 5", n)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::fieldkit::close;

    fn run(op: &str, args: &[Expr]) -> MathResult<Expr> {
        Analysis.apply(op, args, &mut Ctx::new()).unwrap()
    }

    fn t(v: &[i64]) -> Expr {
        tuple(v.iter().map(|&x| Expr::int(x)).collect())
    }

    #[test]
    fn distributions() {
        let x = Expr::sym("x");
        assert_eq!(run("heaviside", &[Expr::int(3)]), Ok(Expr::one()));
        assert_eq!(run("heaviside", &[Expr::int(-3)]), Ok(Expr::zero()));
        assert_eq!(run("heaviside", &[Expr::zero()]), Ok(Expr::rational(1, 2)));
        assert_eq!(
            run("heaviside", std::slice::from_ref(&x)),
            Ok(Expr::apply("heaviside", vec![x.clone()]))
        );
        assert_eq!(run("dirac", &[Expr::int(2)]), Ok(Expr::zero()));
        assert!(run("dirac", &[Expr::zero()]).is_err());
        assert_eq!(
            run("dirac", &[x.clone(), Expr::zero()]),
            Ok(Expr::apply("dirac", vec![x.clone()]))
        );
        // ⟨δ, cos⟩ = 1, ⟨δ′, sin⟩ = −cos 0 = −1, ⟨δ″, x³ + x²⟩ = 2 (catalogue: δ′(ψ) = −ψ′(0)).
        let cos = func(Func::Cos, vec![x.clone()]);
        let sin = func(Func::Sin, vec![x.clone()]);
        assert_eq!(run("diracpair", &[cos, x.clone()]), Ok(Expr::one()));
        assert_eq!(
            run("diracpair", &[sin, x.clone(), Expr::one()]),
            Ok(Expr::int(-1))
        );
        let poly = x.pow(3) + x.pow(2);
        assert_eq!(
            run("diracpair", &[poly.clone(), x.clone(), Expr::int(2)]),
            Ok(Expr::int(2))
        );
        assert_eq!(
            run("diracpair", &[poly, x, Expr::zero(), Expr::int(2)]),
            Ok(Expr::int(12))
        );
    }

    #[test]
    fn norms() {
        assert_eq!(
            run("lpnorm", &[t(&[3, -4]), Expr::int(2)]),
            Ok(Expr::int(5))
        );
        assert_eq!(run("lpnorm", &[t(&[3, -4]), Expr::one()]), Ok(Expr::int(7)));
        assert_eq!(
            run("lpnorm", &[t(&[3, -4, 2]), Expr::infinity()]),
            Ok(Expr::int(4))
        );
        // ‖x‖₂ on [0, 1] = 1/√3; ‖sin‖₂ on [0, π] = √(π/2).
        let x = Expr::sym("x");
        let v = run(
            "lpnorm",
            &[
                x.clone(),
                x.clone(),
                Expr::zero(),
                Expr::one(),
                Expr::int(2),
            ],
        )
        .unwrap();
        assert!(close(numeric(&v).unwrap(), 1.0 / 3f64.sqrt(), 1e-10));
        let s = func(Func::Sin, vec![x.clone()]);
        let v = run(
            "lpnorm",
            &[s, x.clone(), Expr::zero(), Expr::pi(), Expr::int(2)],
        )
        .unwrap();
        assert!(close(
            numeric(&v).unwrap(),
            (std::f64::consts::PI / 2.0).sqrt(),
            1e-10
        ));
        let sup = run(
            "lpnorm",
            &[
                x.pow(2) - x.clone(),
                x,
                Expr::zero(),
                Expr::one(),
                Expr::infinity(),
            ],
        )
        .unwrap();
        assert!(close(numeric(&sup).unwrap(), 0.25, 1e-6));
        assert!(run("lpnorm", &[t(&[1]), Expr::rational(1, 2)]).is_err());
    }

    #[test]
    fn transforms_and_approximation() {
        let d = run("dft", &[t(&[1, 2, 3, 4])]).unwrap();
        assert_eq!(
            d.to_string(),
            run("fft", &[t(&[1, 2, 3, 4])]).unwrap().to_string()
        );
        let back = run("ifft", &[d]).unwrap();
        let vals = list_of("ifft", &back).unwrap();
        assert!(close(numeric(&vals[2]).unwrap(), 3.0, 1e-12));
        let x = Expr::sym("x");
        assert_eq!(
            run("bernstein", &[x.clone(), x.clone(), Expr::int(3)]),
            Ok(x.clone())
        );
        let b = run("bernsteinbasis", &[Expr::one(), Expr::int(2), x.clone()]).unwrap();
        assert!(close(
            eval_at(&b, &["x".into()], &[0.5]).unwrap(),
            0.5,
            1e-15
        ));
        let g = func(Func::Cos, vec![x.clone()]);
        let fp = run("fixedpoint", &[g, x, Expr::one()]).unwrap();
        assert!(close(numeric(&fp).unwrap(), 0.739_085_133_215_160_7, 1e-11));
    }
}
