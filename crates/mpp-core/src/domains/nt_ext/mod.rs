//! More number theory, beside the modular arithmetic and primes of
//! [`crate::domains::number_theory`] (which already has `powmod` by repeated squaring,
//! `crt` and `totient`): arithmetic functions, Dirichlet convolution and Möbius
//! inversion, prime counting, p-adic valuations, continued fractions, Gaussian
//! integers, finite fields, and the ζ and L functions.

pub mod arith;
pub mod contfrac;
pub mod finite_field;
pub mod gaussian;
pub mod zeta;

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{ToPrimitive, Zero};

use crate::domains::fieldkit::{
    complex, complex_arg, complex_list_of, domain_err, float, list_of, positive_int, tuple,
    usize_arg,
};
use crate::domains::util::{expect_args, expect_args_between, int_arg, numeric, rational};
use crate::domains::{Ctx, Domain, Value};
use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode};
use crate::number::Number;

use finite_field::FiniteField;
use gaussian::Gaussian;

/// The extended number theory domain.
pub struct NumberTheoryExt;

const OPS: &[&str] = &[
    "divcount",
    "divsum",
    "mobius",
    "dirconv",
    "mobiusinv",
    "primepi",
    "primepiapprox",
    "padicval",
    "padicabs",
    "factorialval",
    "contfrac",
    "convergents",
    "contfracvalue",
    "gaussfactor",
    "gfmodulus",
    "gfadd",
    "gfmul",
    "gfinv",
    "gfpow",
    "frobenius",
    "gftrace",
    "zeta",
    "hurwitzzeta",
    "dirichletl",
    "bernoulli",
];

/// Default number of terms for a numeric continued fraction.
const DEFAULT_TERMS: usize = 12;

impl Domain for NumberTheoryExt {
    fn name(&self) -> &'static str {
        "number theory (extended)"
    }

    fn operators(&self) -> &'static [&'static str] {
        OPS
    }

    fn apply(&self, op: &str, args: &[Value], ctx: &mut Ctx<'_>) -> Option<MathResult<Value>> {
        Some(apply(op, args, ctx))
    }
}

fn ints(v: Vec<BigInt>) -> Expr {
    tuple(v.into_iter().map(Expr::bigint).collect())
}

fn int_list(op: &str, e: &Expr) -> MathResult<Vec<BigInt>> {
    list_of(op, e)?.iter().map(|x| int_arg(op, x)).collect()
}

fn rational_arg(op: &str, e: &Expr) -> MathResult<BigRational> {
    rational(&e.simplify()).ok_or_else(|| domain_err(op, format!("{e} is not a rational number")))
}

fn prime_arg(op: &str, e: &Expr) -> MathResult<BigInt> {
    positive_int(op, e)
}

fn gaussian_arg(op: &str, e: &Expr) -> MathResult<Gaussian> {
    let e = e.simplify();
    let n = e
        .as_number()
        .ok_or_else(|| domain_err(op, format!("{e} is not a Gaussian integer")))?;
    let (re, im) = match n {
        Number::Rational(q) => (q.clone(), BigRational::zero()),
        Number::Complex(a, b) => (a.clone(), b.clone()),
        _ => {
            return Err(domain_err(
                op,
                format!("{e} is not an exact Gaussian integer"),
            ))
        }
    };
    if !re.is_integer() || !im.is_integer() {
        return Err(domain_err(op, format!("{e} is not a Gaussian integer")));
    }
    Ok(Gaussian::new(re.to_integer(), im.to_integer()))
}

fn gaussian_expr(z: &Gaussian) -> Expr {
    Expr::num(Number::complex(
        BigRational::from_integer(z.re.clone()),
        BigRational::from_integer(z.im.clone()),
    ))
}

fn field(op: &str, args: &[Value]) -> MathResult<FiniteField> {
    let p = int_arg(op, &args[0])?
        .to_u64()
        .ok_or_else(|| domain_err(op, "the characteristic must be a positive prime"))?;
    let m = usize_arg(op, &args[1], finite_field::MAX_M)?;
    FiniteField::new(p, m)
}

fn element(op: &str, f: &FiniteField, e: &Expr) -> MathResult<Vec<u64>> {
    let coeffs = match e.node() {
        ExprNode::Tuple(_) => int_list(op, e)?,
        _ => vec![int_arg(op, e)?],
    };
    f.element(&coeffs)
}

fn element_expr(f: &FiniteField, a: &[u64]) -> Expr {
    let c = f.coefficients(a);
    if f.m() == 1 {
        return Expr::int(c[0] as i64);
    }
    tuple(c.into_iter().map(|x| Expr::int(x as i64)).collect())
}

fn apply(op: &str, args: &[Value], ctx: &mut Ctx<'_>) -> MathResult<Value> {
    match op {
        "divcount" => {
            expect_args(op, args, 1)?;
            Ok(Expr::bigint(arith::divisor_count(&int_arg(op, &args[0])?)?))
        }
        "divsum" => {
            expect_args_between(op, args, 1, 2)?;
            let k = match args.get(1) {
                Some(k) => usize_arg(op, k, 64)? as u32,
                None => 1,
            };
            Ok(Expr::bigint(arith::divisor_sigma(
                &int_arg(op, &args[0])?,
                k,
            )?))
        }
        "mobius" => {
            expect_args(op, args, 1)?;
            Ok(Expr::int(i64::from(arith::mobius(&int_arg(
                op, &args[0],
            )?)?)))
        }
        "dirconv" => {
            expect_args(op, args, 2)?;
            let (f, g) = (list_of(op, &args[0])?, list_of(op, &args[1])?);
            Ok(tuple(arith::dirichlet_convolution(&f, &g)?))
        }
        "mobiusinv" => {
            expect_args(op, args, 1)?;
            Ok(tuple(arith::mobius_inversion(&list_of(op, &args[0])?)?))
        }
        "primepi" => {
            expect_args(op, args, 1)?;
            let x = numeric(&args[0])
                .ok_or_else(|| domain_err(op, format!("{} is not a real number", args[0])))?;
            if x < 0.0 {
                return Ok(Expr::zero());
            }
            if x > arith::MAX_PRIMEPI as f64 {
                return Err(MathError::TooLarge(format!(
                    "an exact prime count up to {x} (the limit is {}; use primepiapprox)",
                    arith::MAX_PRIMEPI
                )));
            }
            Ok(Expr::int(arith::prime_pi(x.floor() as u64)? as i64))
        }
        "primepiapprox" => {
            expect_args(op, args, 1)?;
            let x = numeric(&args[0])
                .ok_or_else(|| domain_err(op, format!("{} is not a real number", args[0])))?;
            ctx.note("prime number theorem estimate x / ln x");
            Ok(float(arith::prime_pi_estimate(x)?))
        }
        "padicval" => {
            expect_args(op, args, 2)?;
            let p = prime_arg(op, &args[0])?;
            Ok(match arith::valuation(&p, &rational_arg(op, &args[1])?)? {
                Some(v) => Expr::int(v),
                None => Expr::infinity(),
            })
        }
        "padicabs" => {
            expect_args(op, args, 2)?;
            let p = prime_arg(op, &args[0])?;
            Ok(Expr::ratio(arith::padic_abs(
                &p,
                &rational_arg(op, &args[1])?,
            )?))
        }
        "factorialval" => {
            expect_args(op, args, 2)?;
            let p = prime_arg(op, &args[0])?;
            Ok(Expr::bigint(arith::factorial_valuation(
                &p,
                &int_arg(op, &args[1])?,
            )?))
        }
        "contfrac" => contfrac_op(op, args, ctx),
        "convergents" => {
            expect_args_between(op, args, 1, 2)?;
            let terms = match args[0].node() {
                ExprNode::Tuple(_) => int_list(op, &args[0])?,
                _ => terms_of(op, &args[0], args.get(1), ctx)?,
            };
            Ok(tuple(
                contfrac::convergents(&terms)
                    .into_iter()
                    .map(Expr::ratio)
                    .collect(),
            ))
        }
        "contfracvalue" => {
            expect_args(op, args, 1)?;
            Ok(Expr::ratio(contfrac::value(&int_list(op, &args[0])?)?))
        }
        "gaussfactor" => {
            expect_args(op, args, 1)?;
            let (unit, fs) = gaussian::factor(&gaussian_arg(op, &args[0])?)?;
            let factors = fs
                .iter()
                .map(|(p, e)| tuple(vec![gaussian_expr(p), Expr::int(i64::from(*e))]))
                .collect();
            Ok(tuple(vec![gaussian_expr(&unit), tuple(factors)]))
        }
        "gfmodulus" => {
            expect_args(op, args, 2)?;
            let f = field(op, args)?;
            let x = Expr::sym("x");
            let terms: Vec<Expr> = f
                .modulus()
                .iter()
                .enumerate()
                .filter(|(_, &c)| c != 0)
                .map(|(i, &c)| Expr::int(c as i64) * x.pow(Expr::int(i as i64)))
                .collect();
            Ok(crate::simplify::add(terms).simplify())
        }
        "gfadd" | "gfmul" => {
            expect_args(op, args, 4)?;
            let f = field(op, args)?;
            let (a, b) = (element(op, &f, &args[2])?, element(op, &f, &args[3])?);
            let r = if op == "gfadd" {
                f.add(&a, &b)
            } else {
                f.mul(&a, &b)
            };
            Ok(element_expr(&f, &r))
        }
        "gfinv" | "frobenius" | "gftrace" => {
            expect_args(op, args, 3)?;
            let f = field(op, args)?;
            let a = element(op, &f, &args[2])?;
            Ok(match op {
                "gfinv" => element_expr(&f, &f.inv(&a)?),
                "frobenius" => element_expr(&f, &f.frobenius(&a)),
                _ => Expr::int(f.trace(&a) as i64),
            })
        }
        "gfpow" => {
            expect_args(op, args, 4)?;
            let f = field(op, args)?;
            let a = element(op, &f, &args[2])?;
            Ok(element_expr(&f, &f.pow(&a, &int_arg(op, &args[3])?)?))
        }
        "zeta" => {
            expect_args(op, args, 1)?;
            let s = &args[0];
            if !s.is_constant() {
                return Ok(Expr::apply(op, args.to_vec()));
            }
            if let Some(q) = rational(s) {
                if q.is_integer() {
                    if let Some(k) = q.to_integer().to_i64() {
                        if let Some(v) = zeta::exact(k)? {
                            return Ok(v);
                        }
                    }
                }
            }
            // An exact real s keeps ζ(s): its digits come from the high-precision
            // evaluator when asked for, not from a 16-digit float.
            if !s.has_float() && rational(s).is_some() {
                zeta::zeta(complex_arg(op, s)?)?;
                return Ok(Expr::apply(op, args.to_vec()));
            }
            ctx.note("≈ numeric");
            Ok(complex(zeta::zeta(complex_arg(op, s)?)?))
        }
        "hurwitzzeta" => {
            expect_args(op, args, 2)?;
            let a = numeric(&args[1])
                .ok_or_else(|| domain_err(op, "a must be a positive real number"))?;
            ctx.note("≈ numeric");
            Ok(complex(zeta::hurwitz(complex_arg(op, &args[0])?, a)?))
        }
        "dirichletl" => {
            expect_args(op, args, 2)?;
            let chi = complex_list_of(op, &args[1])?;
            ctx.note("≈ numeric");
            Ok(complex(zeta::dirichlet_l(
                complex_arg(op, &args[0])?,
                &chi,
            )?))
        }
        "bernoulli" => {
            expect_args(op, args, 1)?;
            let n = usize_arg(op, &args[0], zeta::MAX_BERNOULLI)?;
            Ok(Expr::ratio(zeta::bernoulli(n)?.swap_remove(n)))
        }
        _ => Err(MathError::UnknownFunction(op.into())),
    }
}

/// Terms of `x`'s expansion: exact for rationals, periodic for quadratic surds, numeric
/// otherwise (n terms).
fn terms_of(op: &str, x: &Expr, n: Option<&Expr>, ctx: &mut Ctx<'_>) -> MathResult<Vec<BigInt>> {
    let n = match n {
        Some(n) => Some(usize_arg(op, n, contfrac::MAX_TERMS)?),
        None => None,
    };
    if let Some(q) = rational(&x.simplify()) {
        let t = contfrac::of_rational(&q);
        return Ok(match n {
            Some(n) => t.into_iter().take(n).collect(),
            None => t,
        });
    }
    if let Some((a, b, d)) = contfrac::surd_parts(x) {
        let per = contfrac::of_surd(&a, &b, &d)?;
        let len = per.prefix.len() + per.period.len();
        return Ok(contfrac::take(&per, n.unwrap_or(len)));
    }
    let v = numeric(x).ok_or_else(|| domain_err(op, format!("{x} is not a real number")))?;
    ctx.note("terms computed in floating point");
    contfrac::of_float(v, n.unwrap_or(DEFAULT_TERMS))
}

fn contfrac_op(op: &str, args: &[Value], ctx: &mut Ctx<'_>) -> MathResult<Value> {
    expect_args_between(op, args, 1, 2)?;
    let x = &args[0];
    if args.len() == 1 {
        if let Some((a, b, d)) = contfrac::surd_parts(x) {
            // [a₀; a₁, …, (repeating block)]
            let per = contfrac::of_surd(&a, &b, &d)?;
            ctx.note("the last group repeats forever");
            let mut items: Vec<Expr> = per.prefix.into_iter().map(Expr::bigint).collect();
            items.push(ints(per.period));
            return Ok(tuple(items));
        }
    }
    Ok(ints(terms_of(op, x, args.get(1), ctx)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::fieldkit::close;

    fn run(op: &str, args: &[Expr]) -> MathResult<Expr> {
        NumberTheoryExt.apply(op, args, &mut Ctx::new()).unwrap()
    }

    fn t(v: &[i64]) -> Expr {
        tuple(v.iter().map(|&x| Expr::int(x)).collect())
    }

    #[test]
    fn operators() {
        assert_eq!(run("divcount", &[Expr::int(360)]), Ok(Expr::int(24)));
        assert_eq!(run("divsum", &[Expr::int(6)]), Ok(Expr::int(12)));
        assert_eq!(
            run("divsum", &[Expr::int(6), Expr::int(0)]),
            Ok(Expr::int(4))
        );
        assert_eq!(run("mobius", &[Expr::int(30)]), Ok(Expr::int(-1)));
        assert_eq!(run("primepi", &[Expr::int(100)]), Ok(Expr::int(25)));
        assert_eq!(run("primepi", &[Expr::rational(101, 10)]), Ok(Expr::int(4)));
        assert_eq!(
            run("padicabs", &[Expr::int(2), Expr::int(8)]),
            Ok(Expr::rational(1, 8))
        );
        assert_eq!(
            run("padicval", &[Expr::int(2), Expr::zero()]),
            Ok(Expr::infinity())
        );
        assert_eq!(
            run("contfrac", &[Expr::rational(415, 93)]),
            Ok(t(&[4, 2, 6, 7]))
        );
        assert_eq!(
            run("contfrac", &[Expr::int(2).sqrt()]).unwrap().to_string(),
            "(1, (2))"
        );
        let golden = (Expr::one() + Expr::int(5).sqrt()) / Expr::int(2);
        assert_eq!(
            run("contfrac", &[golden.clone(), Expr::int(5)]),
            Ok(t(&[1, 1, 1, 1, 1]))
        );
        let conv = run("convergents", &[golden, Expr::int(6)]).unwrap();
        assert_eq!(conv.to_string(), "(1, 2, 3/2, 5/3, 8/5, 13/8)");
        assert_eq!(
            run("contfracvalue", &[t(&[3, 7, 16])]),
            Ok(Expr::rational(355, 113))
        );
        let pi_cf = run("contfrac", &[Expr::pi(), Expr::int(4)]).unwrap();
        assert_eq!(pi_cf, t(&[3, 7, 15, 1]));
        assert_eq!(
            run("dirconv", &[t(&[1, 1, 1, 1]), t(&[1, 1, 1, 1])]),
            Ok(t(&[1, 2, 2, 3]))
        );
        assert_eq!(run("mobiusinv", &[t(&[1, 2, 2, 3])]), Ok(t(&[1, 1, 1, 1])));
        assert_eq!(
            run("bernoulli", &[Expr::int(4)]),
            Ok(Expr::rational(-1, 30))
        );
        assert_eq!(
            run("factorialval", &[Expr::int(5), Expr::int(100)]),
            Ok(Expr::int(24))
        );
    }

    #[test]
    fn gaussian_and_fields() {
        let five = run("gaussfactor", &[Expr::int(5)]).unwrap();
        assert_eq!(five.to_string().matches('i').count(), 3, "{five}");
        assert!(run("gaussfactor", &[Expr::rational(1, 2)]).is_err());
        assert_eq!(
            run("gfmodulus", &[Expr::int(2), Expr::int(3)])
                .unwrap()
                .to_string(),
            "x^3 + x + 1"
        );
        let x = t(&[0, 1, 0]);
        assert_eq!(
            run(
                "gfmul",
                &[Expr::int(2), Expr::int(3), x.clone(), t(&[0, 0, 1])]
            ),
            Ok(t(&[1, 1, 0]))
        );
        assert_eq!(
            run(
                "gfpow",
                &[Expr::int(2), Expr::int(3), x.clone(), Expr::int(7)]
            ),
            Ok(t(&[1, 0, 0]))
        );
        assert_eq!(
            run("frobenius", &[Expr::int(2), Expr::int(3), x.clone()]),
            Ok(t(&[0, 0, 1]))
        );
        assert_eq!(
            run("gftrace", &[Expr::int(2), Expr::int(3), x.clone()]),
            Ok(Expr::zero())
        );
        let inv = run("gfinv", &[Expr::int(2), Expr::int(3), x.clone()]).unwrap();
        assert_eq!(
            run("gfmul", &[Expr::int(2), Expr::int(3), x, inv]),
            Ok(t(&[1, 0, 0]))
        );
        assert_eq!(
            run(
                "gfadd",
                &[Expr::int(7), Expr::int(1), Expr::int(5), Expr::int(4)]
            ),
            Ok(Expr::int(2))
        );
        assert!(run(
            "gfadd",
            &[Expr::int(6), Expr::int(1), Expr::int(5), Expr::int(4)]
        )
        .is_err());
    }

    #[test]
    fn zeta_operators() {
        assert_eq!(
            run("zeta", &[Expr::int(2)]),
            Ok((Expr::pi().pow(2) / Expr::int(6)).simplify())
        );
        assert_eq!(run("zeta", &[Expr::zero()]), Ok(Expr::rational(-1, 2)));
        let z3 = run("zeta", &[Expr::int(3)]).unwrap();
        assert!(close(numeric(&z3).unwrap(), 1.202_056_903_159_594_2, 1e-14));
        assert!(run("zeta", &[Expr::one()]).is_err());
        assert_eq!(
            run("zeta", &[Expr::sym("s")]),
            Ok(Expr::apply("zeta", vec![Expr::sym("s")]))
        );
        let l = run("dirichletl", &[Expr::one(), t(&[1, 0, -1, 0])]).unwrap();
        assert!(close(
            numeric(&l).unwrap(),
            std::f64::consts::PI / 4.0,
            1e-14
        ));
    }
}
