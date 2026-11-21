//! Special functions as domain operators: `erf`, `erfc`, Lambert `W` (both real
//! branches), Bessel `J_n`, the beta function, Legendre and Chebyshev polynomials and
//! the complete elliptic integrals `K` and `E` (modulus convention).
//!
//! Each operator returns an exact value where one is known (`erf 0 = 0`,
//! `W(e) = 1`, `B(2, 3) = 1/12`, `K(0) = π/2`, polynomials with exact coefficients),
//! a numeric value for numeric arguments, and stays unevaluated (an `Apply` node)
//! for symbolic arguments. [`partial`] gives the standard derivatives so the calculus
//! can differentiate these applications, and [`call_real`] lets numeric evaluation
//! reach them inside larger expressions. Γ for complex arguments is the built-in
//! `Gamma` (Lanczos), so it has no operator here.

pub mod bessel;
pub mod elliptic;
pub mod erf;
pub mod lambert;
pub mod orthopoly;

use num_complex::Complex64;

use crate::domains::fieldkit::{complex, complex_arg, domain_err, float, usize_arg};
use crate::domains::util::{equal, expect_args, expect_args_between, numeric, rational};
use crate::domains::{Ctx, Domain, Value};
use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode};
use crate::functions::{func, gamma_complex, Func};

use orthopoly::Family;

/// The special-functions domain.
pub struct SpecialFunctions;

const OPS: &[&str] = &[
    "erf",
    "erfc",
    "lambertw",
    "besselj",
    "beta",
    "legendrep",
    "chebyshevt",
    "chebyshevu",
    "ellipk",
    "ellipe",
];

impl Domain for SpecialFunctions {
    fn name(&self) -> &'static str {
        "special functions"
    }

    fn operators(&self) -> &'static [&'static str] {
        OPS
    }

    fn apply(&self, op: &str, args: &[Value], ctx: &mut Ctx<'_>) -> Option<MathResult<Value>> {
        Some(apply(op, args, ctx))
    }
}

fn family(op: &str) -> Option<Family> {
    match op {
        "legendrep" => Some(Family::Legendre),
        "chebyshevt" => Some(Family::ChebyshevT),
        "chebyshevu" => Some(Family::ChebyshevU),
        _ => None,
    }
}

fn arity_of(op: &str, args: &[Value]) -> MathResult<()> {
    match op {
        "lambertw" => expect_args_between(op, args, 1, 2),
        "besselj" | "beta" | "legendrep" | "chebyshevt" | "chebyshevu" => expect_args(op, args, 2),
        _ => expect_args(op, args, 1),
    }
}

fn apply(op: &str, args: &[Value], ctx: &mut Ctx<'_>) -> MathResult<Value> {
    arity_of(op, args)?;
    if let Some(fam) = family(op) {
        return orthogonal(op, fam, args);
    }
    // Symbolic arguments: leave the application for the calculus and the printer.
    if args.iter().any(|a| !a.is_constant()) {
        return Ok(Expr::apply(op, args.to_vec()));
    }
    if let Some(exact) = exact_value(op, args) {
        return Ok(exact);
    }
    if op == "beta" {
        return beta(args);
    }
    let vals = args
        .iter()
        .map(|a| numeric(a).ok_or_else(|| domain_err(op, format!("{a} is not a real number"))))
        .collect::<MathResult<Vec<f64>>>()?;
    ctx.note("≈ numeric");
    call_real(op, &vals)
        .unwrap_or_else(|| Err(MathError::UnknownFunction(op.into())))
        .map(float)
}

/// Exact values: erf 0, W at 0, e and −1/e, J_n(0), K(0), E(0), E(±1).
fn exact_value(op: &str, args: &[Value]) -> Option<Expr> {
    let x = &args[0];
    let is = |v: &Expr| equal(x, v);
    let branch = || {
        args.get(1).map_or(Some(0), |b| {
            rational(b).and_then(|q| q.to_integer().try_into().ok())
        })
    };
    match op {
        "erf" if x.is_zero() => Some(Expr::zero()),
        "erfc" if x.is_zero() => Some(Expr::one()),
        "lambertw" => {
            let k: i64 = branch()?;
            let minus_inv_e = -Expr::e().recip();
            if k == 0 && x.is_zero() {
                Some(Expr::zero())
            } else if k == 0 && is(&Expr::e()) {
                Some(Expr::one())
            } else if (k == 0 || k == -1) && is(&minus_inv_e) {
                Some(Expr::int(-1))
            } else {
                None
            }
        }
        "besselj" if args[1].is_zero() => {
            let n = rational(x)?;
            Some(if n.numer() == &0.into() {
                Expr::one()
            } else {
                Expr::zero()
            })
        }
        "ellipk" | "ellipe" if x.is_zero() => Some(Expr::pi() / Expr::int(2)),
        "ellipe" if is(&Expr::one()) || is(&Expr::int(-1)) => Some(Expr::one()),
        _ => None,
    }
}

/// `B(a, b) = Γ(a) Γ(b) / Γ(a + b)`: exact through the built-in Γ for exact
/// arguments, numeric (complex allowed) otherwise.
fn beta(args: &[Value]) -> MathResult<Value> {
    let (a, b) = (&args[0], &args[1]);
    if rational(a).is_some() && rational(b).is_some() {
        let g = |x: &Expr| func(Func::Gamma, vec![x.clone()]);
        let sum = (a + b).simplify();
        if rational(&sum).is_some_and(|s| {
            s <= num_rational::BigRational::from_integer(0.into()) && s.is_integer()
        }) {
            // Γ(a + b) has a pole, so B is 0 when Γ(a), Γ(b) are finite.
            return Ok(Expr::zero());
        }
        return Ok((g(a) * g(b) / g(&sum)).simplify());
    }
    let (za, zb) = (complex_arg("beta", a)?, complex_arg("beta", b)?);
    Ok(complex(beta_complex(za, zb)))
}

fn beta_complex(a: Complex64, b: Complex64) -> Complex64 {
    gamma_complex(a) * gamma_complex(b) / gamma_complex(a + b)
}

fn orthogonal(op: &str, fam: Family, args: &[Value]) -> MathResult<Value> {
    let n = usize_arg(op, &args[0], orthopoly::MAX_DEGREE)?;
    let x = &args[1];
    if let Some(q) = rational(x) {
        return Ok(Expr::ratio(orthopoly::poly(fam, n).eval(&q)));
    }
    if !x.is_constant() {
        let p = orthopoly::poly(fam, n);
        return Ok(match x.as_sym() {
            Some(v) => p.to_expr(v),
            None => orthopoly::at_expr(&p, x),
        });
    }
    let v = numeric(x).ok_or_else(|| domain_err(op, format!("{x} is not a real number")))?;
    Ok(float(orthopoly::value(fam, n, v)))
}

fn order(op: &str, x: f64) -> MathResult<i64> {
    if x.fract() != 0.0 || x.abs() > 1e9 {
        return Err(domain_err(
            op,
            format!("the order must be an integer, not {x}"),
        ));
    }
    Ok(x as i64)
}

/// True for the names [`call_real`] evaluates.
pub fn has_numeric_value(name: &str) -> bool {
    matches!(
        name,
        crate::expr::EULER_GAMMA
            | "polygamma"
            | "erf"
            | "erfc"
            | "lambertw"
            | "besselj"
            | "beta"
            | "legendrep"
            | "chebyshevt"
            | "chebyshevu"
            | "ellipk"
            | "ellipe"
            | "heaviside"
            | "H"
            | "dirac"
            | "delta"
    )
}

/// Numeric values of the special functions (and the distributions of the analysis
/// domain) by name, for evaluating them inside larger expressions. `None` when the
/// name is not one of them.
pub fn call_real(name: &str, args: &[f64]) -> Option<MathResult<f64>> {
    let one = |f: fn(f64) -> MathResult<f64>| -> MathResult<f64> {
        match args {
            [x] => f(*x),
            _ => Err(crate::domains::util::arity(name, "1", args.len())),
        }
    };
    let two = |f: &dyn Fn(f64, f64) -> MathResult<f64>| -> MathResult<f64> {
        match args {
            [a, b] => f(*a, *b),
            _ => Err(crate::domains::util::arity(name, "2", args.len())),
        }
    };
    Some(match name {
        crate::expr::EULER_GAMMA if args.is_empty() => Ok(std::f64::consts::EULER_GAMMA),
        "polygamma" => two(&|n, x| polygamma_f64(order(name, n)?, x)),
        "erf" => one(|x| Ok(erf::erf(x))),
        "erfc" => one(|x| Ok(erf::erfc(x))),
        "lambertw" => match args {
            [x] => lambert::lambert_w(*x, 0),
            [x, k] => order(name, *k).and_then(|k| lambert::lambert_w(*x, k)),
            _ => Err(crate::domains::util::arity(name, "1 to 2", args.len())),
        },
        "besselj" => two(&|n, x| bessel::bessel_j(order(name, n)?, x)),
        "beta" => two(&|a, b| {
            let v = beta_complex(Complex64::new(a, 0.0), Complex64::new(b, 0.0)).re;
            if v.is_finite() {
                Ok(v)
            } else {
                Err(MathError::Undefined(format!("B({a}, {b})")))
            }
        }),
        "legendrep" | "chebyshevt" | "chebyshevu" => two(&|n, x| {
            let n = order(name, n)?;
            if n < 0 || n as usize > orthopoly::MAX_DEGREE {
                return Err(domain_err(
                    name,
                    format!("the degree must be 0 … {}", orthopoly::MAX_DEGREE),
                ));
            }
            Ok(orthopoly::value(
                family(name).unwrap_or(Family::Legendre),
                n as usize,
                x,
            ))
        }),
        "ellipk" => one(elliptic::ellip_k),
        "ellipe" => one(elliptic::ellip_e),
        // At the jump the conventions differ (0, 1/2 or 1), so H(0) has no value here.
        "heaviside" | "H" => one(|x| {
            if x > 0.0 {
                Ok(1.0)
            } else if x < 0.0 {
                Ok(0.0)
            } else {
                Err(MathError::Undefined(
                    "H(0): the value at the jump depends on the convention".into(),
                ))
            }
        }),
        "dirac" | "delta" => match args {
            [x] | [x, _] if *x != 0.0 => Ok(0.0),
            _ => Err(MathError::Undefined(
                "δ at 0 (it is a distribution, not a function)".into(),
            )),
        },
        _ => return None,
    })
}

/// The polygamma function ψ^{(n)}(x) = dⁿ⁺¹/dxⁿ⁺¹ ln Γ(x) for n ≥ 1 (n = 0 is the
/// digamma function): the recurrence ψ^{(n)}(x) = ψ^{(n)}(x + 1) + (−1)ⁿ⁺¹ n!/xⁿ⁺¹ moves
/// x above 20, then the asymptotic series (DLMF 5.15.8) finishes it.
pub fn polygamma_f64(n: i64, x: f64) -> MathResult<f64> {
    if n == 0 {
        return crate::functions::digamma_f64(x);
    }
    if !(1..=50).contains(&n) {
        return Err(domain_err(
            "polygamma",
            "the order must be a whole number from 0 to 50",
        ));
    }
    if x <= 0.0 && x.fract() == 0.0 {
        return Err(MathError::Undefined(format!("ψ^({n}) at the pole {x}")));
    }
    if !x.is_finite() {
        return Err(domain_err("polygamma", "the argument must be finite"));
    }
    let nf = n as f64;
    let sign = if n % 2 == 1 { 1.0 } else { -1.0 }; // (−1)^{n+1}
    let fact: f64 = (1..=n).map(|k| k as f64).product();
    let mut x = x;
    let mut acc = 0.0;
    while x < 20.0 {
        acc += sign * fact / x.powf(nf + 1.0);
        x += 1.0;
    }
    // (−1)^{n+1} [ (n−1)!/xⁿ + n!/(2xⁿ⁺¹) + Σₖ B₂ₖ (2k+n−1)!/((2k)! x^{2k+n}) ]
    const B2K: [f64; 8] = [
        1.0 / 6.0,
        -1.0 / 30.0,
        1.0 / 42.0,
        -1.0 / 30.0,
        5.0 / 66.0,
        -691.0 / 2730.0,
        7.0 / 6.0,
        -3617.0 / 510.0,
    ];
    let fact_m1: f64 = (1..n).map(|k| k as f64).product();
    let mut series = fact_m1 / x.powf(nf) + fact / (2.0 * x.powf(nf + 1.0));
    for (i, b) in B2K.iter().enumerate() {
        let k = (i + 1) as i64;
        // (2k+n−1)! / (2k)!
        let ratio: f64 = ((2 * k + 1)..=(2 * k + n - 1)).map(|j| j as f64).product();
        series += b * ratio / x.powf((2 * k) as f64 + nf);
    }
    Ok(acc + sign * series)
}

/// Complex evaluation hook: the beta function takes complex arguments, the others are
/// evaluated on the real line.
pub fn call_complex(name: &str, args: &[Complex64]) -> Option<MathResult<Complex64>> {
    if name == "beta" {
        return Some(match args {
            [a, b] => Ok(beta_complex(*a, *b)),
            _ => Err(crate::domains::util::arity(name, "2", args.len())),
        });
    }
    if args.iter().all(|z| z.im == 0.0) {
        let re: Vec<f64> = args.iter().map(|z| z.re).collect();
        return call_real(name, &re).map(|r| r.map(|v| Complex64::new(v, 0.0)));
    }
    if OPS.contains(&name) {
        return Some(Err(MathError::Unsupported(format!(
            "{name} of a complex argument"
        ))));
    }
    None
}

/// The partial derivative of the application `name(args)` with respect to its
/// argument number `index` (outer derivative only: the caller applies the chain rule).
/// `None` when no standard formula is registered.
///
/// Sources: DLMF 7.10.1 (erf), 4.13.4 (W), 10.6.1 (J_n), 5.12 + 5.15 (B), 18.9.17
/// (Legendre), 18.9.21 (Chebyshev), 19.4.1 (K, E); `H′ = δ` and `(δ^{(j)})′ = δ^{(j+1)}`
/// as distributions.
pub fn partial(name: &str, args: &[Expr], index: usize) -> Option<Expr> {
    let u = args.first()?.clone();
    let two = Expr::int(2);
    let one = Expr::one();
    let app = |n: &str, a: Vec<Expr>| Expr::apply(n, a);
    let d = match (name, index) {
        ("erf", 0) => &two / &Expr::pi().sqrt() * Expr::e().pow(-u.pow(2)),
        ("erfc", 0) => -(&two / &Expr::pi().sqrt() * Expr::e().pow(-u.pow(2))),
        ("lambertw", 0) => {
            let w = app(name, args.to_vec());
            &w / &(&u * &(&one + &w))
        }
        ("besselj", 1) => {
            let x = args.get(1)?.clone();
            (bessel_j_of(&u - &one, x.clone()) - bessel_j_of(&u + &one, x)) / two
        }
        ("polygamma", 1) => {
            let x = args.get(1)?.clone();
            app("polygamma", vec![(&u + &one).simplify(), x])
        }
        ("legendrep", 1) => {
            let x = args.get(1)?.clone();
            &u * &(&x * &app(name, vec![u.clone(), x.clone()])
                - app(name, vec![&u - &one, x.clone()]))
                / (x.pow(2) - one)
        }
        ("chebyshevt", 1) => {
            let x = args.get(1)?.clone();
            &u * &app("chebyshevu", vec![&u - &one, x])
        }
        ("chebyshevu", 1) => {
            let x = args.get(1)?.clone();
            ((&u + &one) * app("chebyshevt", vec![&u + &one, x.clone()])
                - &x * &app(name, vec![u.clone(), x.clone()]))
                / (x.pow(2) - one)
        }
        ("ellipk", 0) => {
            let (kk, ee) = (
                app("ellipk", vec![u.clone()]),
                app("ellipe", vec![u.clone()]),
            );
            &ee / &(&u * &(&one - &u.pow(2))) - &kk / &u
        }
        ("ellipe", 0) => (app("ellipe", vec![u.clone()]) - app("ellipk", vec![u.clone()])) / u,
        ("beta", 0 | 1) => {
            let (a, b) = (args.first()?.clone(), args.get(1)?.clone());
            let this = if index == 0 { a.clone() } else { b.clone() };
            app("beta", vec![a.clone(), b.clone()])
                * (func(Func::Digamma, vec![this]) - func(Func::Digamma, vec![&a + &b]))
        }
        ("heaviside" | "H", 0) => app("delta", vec![u]),
        ("dirac" | "delta", 0) => {
            let j = match args.get(1) {
                Some(j) => j + &one,
                None => one,
            };
            app("delta", vec![u, j.simplify()])
        }
        _ => return None,
    };
    Some(d.simplify())
}

/// `J_n(x)` with a negative whole order written with a positive one,
/// `J_{−n} = (−1)ⁿ J_n` (DLMF 10.4.1), so `J_0′ = −J_1`.
fn bessel_j_of(n: Expr, x: Expr) -> Expr {
    let n = n.simplify();
    if let Some(k) = n.as_number().and_then(|v| v.as_i64()).filter(|k| *k < 0) {
        let j = Expr::apply("besselj", vec![Expr::int(-k), x]);
        return if k % 2 == 0 { j } else { -j };
    }
    Expr::apply("besselj", vec![n, x])
}

/// True when `e` is an application of one of this domain's functions.
pub fn is_special(e: &Expr) -> bool {
    matches!(e.node(), ExprNode::Apply(n, _) if OPS.contains(&n.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::fieldkit::{close, eval_at};

    fn run(op: &str, args: &[Expr]) -> MathResult<Expr> {
        SpecialFunctions.apply(op, args, &mut Ctx::new()).unwrap()
    }

    fn num(e: &Expr) -> f64 {
        eval_at(e, &[], &[]).unwrap()
    }

    #[test]
    fn polygamma_values() {
        let pi2 = std::f64::consts::PI.powi(2);
        // ψ′(1) = π²/6, ψ′(1/2) = π²/2, ψ″(1) = −2ζ(3), ψ‴(1) = π⁴/15 (DLMF 5.15.2–5.15.3).
        assert!(close(polygamma_f64(1, 1.0).unwrap(), pi2 / 6.0, 1e-12));
        assert!(close(polygamma_f64(1, 0.5).unwrap(), pi2 / 2.0, 1e-12));
        assert!(close(
            polygamma_f64(2, 1.0).unwrap(),
            -2.404_113_806_319_188_5,
            1e-12
        ));
        assert!(close(
            polygamma_f64(3, 1.0).unwrap(),
            pi2 * pi2 / 15.0,
            1e-12
        ));
        // Across the switch to the asymptotic series: ψ′(x) − ψ′(x + 1) = 1/x².
        let (a, b) = (
            polygamma_f64(1, 19.5).unwrap(),
            polygamma_f64(1, 20.5).unwrap(),
        );
        assert!(close(a - b, 1.0 / 19.5f64.powi(2), 1e-12));
        assert!(polygamma_f64(1, -2.0).is_err());
        assert!(polygamma_f64(-1, 1.0).is_err());
        assert!(close(
            eval_at(&Expr::euler_gamma(), &[], &[]).unwrap(),
            std::f64::consts::EULER_GAMMA,
            1e-15
        ));
    }

    #[test]
    fn exact_values() {
        assert_eq!(run("erf", &[Expr::zero()]), Ok(Expr::zero()));
        assert_eq!(run("lambertw", &[Expr::e()]), Ok(Expr::one()));
        assert_eq!(
            run("lambertw", &[-Expr::e().recip(), Expr::int(-1)]),
            Ok(Expr::int(-1))
        );
        // B(2, 3) = 1/12 and B(1/2, 1/2) = π (DLMF 5.12.1).
        assert_eq!(
            run("beta", &[Expr::int(2), Expr::int(3)]),
            Ok(Expr::rational(1, 12))
        );
        assert_eq!(
            run("beta", &[Expr::rational(1, 2), Expr::rational(1, 2)]),
            Ok(Expr::pi())
        );
        assert_eq!(
            run("ellipk", &[Expr::zero()]).unwrap(),
            Expr::pi() / Expr::int(2)
        );
        assert_eq!(
            run("besselj", &[Expr::zero(), Expr::zero()]),
            Ok(Expr::one())
        );
        let p2 = run("legendrep", &[Expr::int(2), Expr::sym("x")]).unwrap();
        assert_eq!(p2.to_string(), "3*x^2/2 - 1/2");
        assert_eq!(
            run("chebyshevt", &[Expr::int(4), Expr::one()]),
            Ok(Expr::one())
        );
    }

    #[test]
    fn numeric_and_symbolic() {
        let v = run("erf", &[Expr::rational(1, 2)]).unwrap();
        assert!(close(num(&v), 0.520_499_877_813_046_5, 1e-14));
        let sym = run("erf", &[Expr::sym("x")]).unwrap();
        assert_eq!(sym, Expr::apply("erf", vec![Expr::sym("x")]));
        // Inside a larger expression numeric evaluation reaches the function.
        let e = Expr::int(2) * Expr::apply("besselj", vec![Expr::zero(), Expr::one()]);
        assert!(close(num(&e), 2.0 * 0.765_197_686_557_966_6, 1e-14));
        assert!(run("besselj", &[Expr::rational(1, 2), Expr::one()]).is_err());
        assert!(run("legendrep", &[Expr::int(-1), Expr::one()]).is_err());
    }

    #[test]
    fn complex_gamma_reference() {
        // Γ(1 + i) = 0.4980156681183560 − 0.1549498283018106 i (DLMF §5.4, computed).
        let z = gamma_complex(Complex64::new(1.0, 1.0));
        assert!(close(z.re, 0.498_015_668_118_356, 1e-13));
        assert!(close(z.im, -0.154_949_828_301_810_6, 1e-13));
        let b = beta_complex(Complex64::new(0.5, 0.0), Complex64::new(0.5, 0.0));
        assert!(close(b.re, std::f64::consts::PI, 1e-13));
    }

    #[test]
    fn derivatives_match_finite_differences() {
        let x = Expr::sym("x");
        let names = vec!["x".to_string()];
        let cases: Vec<(Expr, f64)> = vec![
            (Expr::apply("erf", vec![x.clone()]), 0.7),
            (Expr::apply("erfc", vec![x.clone()]), 0.3),
            (Expr::apply("lambertw", vec![x.clone()]), 1.3),
            (
                Expr::apply("lambertw", vec![x.clone(), Expr::int(-1)]),
                -0.2,
            ),
            (Expr::apply("besselj", vec![Expr::int(2), x.clone()]), 2.5),
            (Expr::apply("legendrep", vec![Expr::int(3), x.clone()]), 0.4),
            (
                Expr::apply("chebyshevt", vec![Expr::int(5), x.clone()]),
                0.3,
            ),
            (
                Expr::apply("chebyshevu", vec![Expr::int(4), x.clone()]),
                -0.6,
            ),
            (Expr::apply("ellipk", vec![x.clone()]), 0.5),
            (Expr::apply("ellipe", vec![x.clone()]), 0.5),
            (Expr::apply("beta", vec![x.clone(), Expr::int(3)]), 1.5),
        ];
        for (f, at) in cases {
            let ExprNode::Apply(name, args) = f.node() else {
                unreachable!()
            };
            let idx = args.iter().position(|a| a == &x).unwrap();
            let d = partial(name, args, idx).unwrap();
            let h = 1e-6;
            let fd = (eval_at(&f, &names, &[at + h]).unwrap()
                - eval_at(&f, &names, &[at - h]).unwrap())
                / (2.0 * h);
            let dv = eval_at(&d, &names, &[at]).unwrap();
            assert!(close(dv, fd, 1e-6), "{name}: {dv} vs {fd}");
        }
        assert_eq!(
            partial("heaviside", std::slice::from_ref(&x), 0),
            Some(Expr::apply("delta", vec![x.clone()]))
        );
        assert_eq!(
            partial("H", std::slice::from_ref(&x), 0),
            Some(Expr::apply("delta", vec![x.clone()]))
        );
        assert_eq!(
            partial("dirac", std::slice::from_ref(&x), 0),
            Some(Expr::apply("delta", vec![x, Expr::one()]))
        );
    }
}
