//! Exact roots of polynomials: rational roots first, then the quadratic formula,
//! Cardano's formula for cubics (the trigonometric form when all three roots are real),
//! Ferrari's method for quartics, and numeric roots beyond degree four.

use num_bigint::BigInt;
use num_complex::Complex64;
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};

use crate::domains::util::{numeric, rat_expr};
use crate::error::{MathError, MathResult};
use crate::expr::Expr;
use crate::factor::factor_poly;
use crate::functions::Func;
use crate::number::Number;
use crate::poly::Poly;

/// Highest degree the numeric root finder accepts.
pub const MAX_DEGREE: usize = 200;
/// Node count above which an exact radical form is replaced by numeric roots.
const MAX_EXACT_SIZE: usize = 400;

/// One root of a polynomial.
#[derive(Clone, Debug, PartialEq)]
pub struct Root {
    /// The value (exact radicals, or approximate when `exact` is false).
    pub value: Expr,
    /// How many times it is a root.
    pub multiplicity: u32,
    /// False for numeric roots.
    pub exact: bool,
}

fn q(n: i64, d: i64) -> BigRational {
    BigRational::new(BigInt::from(n), BigInt::from(d))
}

/// Square root that writes negative constants as `i·√(−e)` and exact Gaussian
/// rationals `a + bi` in the form `√((|z| + a)/2) ± i √((|z| − a)/2)`.
pub fn sqrt_signed(e: &Expr) -> Expr {
    if let Some(Number::Complex(a, b)) = e.as_number() {
        let (a, b) = (rat_expr(a.clone()), b.clone());
        let modulus = (a.pow(2) + rat_expr(b.clone() * b.clone())).sqrt();
        let re = ((&modulus + &a) / Expr::int(2)).sqrt();
        let im = ((&modulus - &a) / Expr::int(2)).sqrt();
        let sign = if b.is_negative() { -1 } else { 1 };
        return (re + Expr::int(sign) * Expr::i() * im).simplify();
    }
    if let Some(v) = numeric(e) {
        if v < 0.0 {
            return Expr::i() * negate(e).sqrt();
        }
    }
    e.sqrt()
}

/// `−e`, with the sign distributed over a sum.
fn negate(e: &Expr) -> Expr {
    let n = (-e).simplify();
    match n.expand() {
        Ok(x) if x.size() <= n.size() => x,
        _ => n,
    }
}

/// The real cube root of a constant (negative radicands give negative roots).
pub fn real_cbrt(e: &Expr) -> Expr {
    if let Some(v) = numeric(e) {
        if v < 0.0 {
            return -negate(e).root(Expr::int(3));
        }
    }
    e.root(Expr::int(3))
}

/// Roots of the polynomial with coefficients `coeffs` (lowest power first) in one
/// variable. Rational coefficients give exact roots up to degree four (per irreducible
/// factor); symbolic coefficients are solved exactly up to degree two.
pub fn roots(coeffs: &[Expr]) -> MathResult<Vec<Root>> {
    let coeffs: Vec<Expr> = coeffs.iter().map(Expr::simplify).collect();
    let degree = coeffs.len().saturating_sub(1);
    if degree > MAX_DEGREE {
        return Err(MathError::TooLarge(format!(
            "a polynomial of degree {degree}"
        )));
    }
    if coeffs.iter().all(|c| {
        c.as_number().is_some_and(Number::is_exact)
            && c.as_number().and_then(Number::as_rational).is_some()
    }) {
        let rats: Vec<BigRational> = coeffs
            .iter()
            .filter_map(|c| c.as_number().and_then(Number::as_rational).cloned())
            .collect();
        return rational_roots(&Poly::new(rats));
    }
    match degree {
        0 => Ok(Vec::new()),
        1 => Ok(vec![exact(-&coeffs[0] / coeffs[1].clone(), 1)]),
        2 => Ok(quadratic(&coeffs[2], &coeffs[1], &coeffs[0])
            .into_iter()
            .map(|r| exact(r, 1))
            .collect()),
        _ => {
            if coeffs.iter().all(|c| c.is_constant()) {
                numeric_roots_of(&coeffs)
            } else {
                Err(MathError::Unsupported(format!(
                    "exact roots of a degree-{degree} polynomial with symbolic coefficients"
                )))
            }
        }
    }
}

fn exact(value: Expr, multiplicity: u32) -> Root {
    Root {
        value: value.simplify(),
        multiplicity,
        exact: true,
    }
}

/// Roots of a polynomial over Q, factor by factor.
pub fn rational_roots(p: &Poly) -> MathResult<Vec<Root>> {
    let Some(deg) = p.degree() else {
        return Err(MathError::Undefined(
            "the roots of the zero polynomial".into(),
        ));
    };
    if deg > MAX_DEGREE {
        return Err(MathError::TooLarge(format!("a polynomial of degree {deg}")));
    }
    let (_, factors) = factor_poly(p);
    let mut out = Vec::new();
    for (f, m) in factors {
        let c: Vec<Expr> = f.coeffs().iter().cloned().map(rat_expr).collect();
        let found: Vec<Expr> = match f.degree() {
            Some(1) => vec![-&c[0] / c[1].clone()],
            Some(2) => quadratic(&c[2], &c[1], &c[0]),
            Some(3) => cubic(&f)?,
            Some(4) => quartic(&f)?,
            _ => Vec::new(),
        };
        if found.is_empty() || found.iter().any(|r| r.size() > MAX_EXACT_SIZE) {
            out.extend(numeric_roots_of(&c)?.into_iter().map(|mut r| {
                r.multiplicity = m;
                r
            }));
        } else {
            out.extend(found.into_iter().map(|r| exact(r, m)));
        }
    }
    sort_roots(&mut out);
    Ok(out)
}

/// Sorts roots: real ones ascending, then complex ones by real then imaginary part.
pub fn sort_roots(rs: &mut [Root]) {
    let key = |r: &Root| -> (bool, f64, f64) {
        let v = r
            .value
            .eval_complex(&crate::eval::EmptyEnv)
            .unwrap_or(Complex64::new(f64::NAN, f64::NAN));
        (v.im.abs() > 1e-12 * v.norm().max(1.0), v.re, v.im)
    };
    rs.sort_by(|a, b| {
        let (ca, ra, ia) = key(a);
        let (cb, rb, ib) = key(b);
        ca.cmp(&cb).then(ra.total_cmp(&rb)).then(ia.total_cmp(&ib))
    });
}

/// The two roots of `a x² + b x + c` (`a ≠ 0`).
pub fn quadratic(a: &Expr, b: &Expr, c: &Expr) -> Vec<Expr> {
    let disc = (b.pow(2) - Expr::int(4) * a.clone() * c.clone()).simplify();
    let two_a = Expr::int(2) * a.clone();
    if disc.is_zero() {
        return vec![(-b / two_a).simplify()];
    }
    let s = sqrt_signed(&disc);
    vec![
        ((-b - s.clone()) / two_a.clone()).simplify(),
        ((-b + s) / two_a).simplify(),
    ]
}

/// Roots of an irreducible cubic over Q (no rational roots).
fn cubic(f: &Poly) -> MathResult<Vec<Expr>> {
    let lead = f.leading();
    let m = f.scale(&(BigRational::one() / lead));
    let c = m.coeffs();
    let (a, b, cc) = (c[2].clone(), c[1].clone(), c[0].clone());
    // x = t − a/3: t³ + p t + q = 0
    let p = &b - &a * &a / q(3, 1);
    let qq = q(2, 27) * &a * &a * &a - &a * &b / q(3, 1) + &cc;
    let shift = rat_expr(-&a / q(3, 1));
    let ts = depressed_cubic(&p, &qq);
    Ok(ts
        .into_iter()
        .map(|t| (t + shift.clone()).simplify())
        .collect())
}

/// Roots of `t³ + p t + q`.
fn depressed_cubic(p: &BigRational, qq: &BigRational) -> Vec<Expr> {
    let pe = rat_expr(p.clone());
    let qe = rat_expr(qq.clone());
    let half = Expr::rational(1, 2);
    let s3 = Expr::int(3).sqrt();
    if p.is_zero() {
        let r = real_cbrt(&-qe);
        let re = -(&r * &half);
        let im = &r * &s3 * half;
        return vec![r, &re - &(Expr::i() * im.clone()), re + Expr::i() * im];
    }
    // Discriminant −(4p³ + 27q²).
    let disc = -(q(4, 1) * p * p * p + q(27, 1) * qq * qq);
    if disc.is_positive() {
        // Three real roots (casus irreducibilis): trigonometric form.
        let m = (Expr::int(-1) * pe.clone() / Expr::int(3)).sqrt() * Expr::int(2);
        let arg = rat_expr(q(3, 1) * qq / (q(2, 1) * p)) * (Expr::int(-3) / pe).sqrt();
        let theta = Expr::func(Func::Acos, vec![arg.simplify()])
            .unwrap_or_else(|_| Expr::undefined())
            / Expr::int(3);
        return (0..3)
            .map(|k| {
                let angle = &theta + &(Expr::int(2 * k) * Expr::pi() / Expr::int(3));
                let cos = Expr::func(Func::Cos, vec![angle.simplify()])
                    .unwrap_or_else(|_| Expr::undefined());
                (&m * &cos).simplify()
            })
            .collect();
    }
    // One real root (Cardano) and a complex pair.
    let d = rat_expr(qq * qq / q(4, 1) + p * p * p / q(27, 1));
    let sd = d.sqrt();
    let minus_half_q = rat_expr(-qq / q(2, 1));
    let u = real_cbrt(&(&minus_half_q + &sd).simplify());
    let v = real_cbrt(&(&minus_half_q - &sd).simplify());
    let sum = &u + &v;
    let re = -(&sum * &half);
    let im = s3 * half * (u - v);
    vec![
        sum.simplify(),
        (&re - &(Expr::i() * im.clone())).simplify(),
        (re + Expr::i() * im).simplify(),
    ]
}

/// Roots of an irreducible quartic over Q (Ferrari).
fn quartic(f: &Poly) -> MathResult<Vec<Expr>> {
    let lead = f.leading();
    let m = f.scale(&(BigRational::one() / lead));
    let c = m.coeffs();
    let (a, b, cc, d) = (c[3].clone(), c[2].clone(), c[1].clone(), c[0].clone());
    // x = y − a/4: y⁴ + p y² + q y + r = 0
    let a2 = &a * &a;
    let p = &b - q(3, 8) * &a2;
    let qq = &cc - &a * &b / q(2, 1) + &a2 * &a / q(8, 1);
    let r = &d - &a * &cc / q(4, 1) + &a2 * &b / q(16, 1) - q(3, 256) * &a2 * &a2;
    let shift = rat_expr(-&a / q(4, 1));
    let ys = if qq.is_zero() {
        // Biquadratic: z = y².
        let one = Expr::one();
        let zs = quadratic(&one, &rat_expr(p.clone()), &rat_expr(r.clone()));
        let mut out = Vec::new();
        for z in zs {
            let s = sqrt_signed(&z);
            out.push(-s.clone());
            out.push(s);
        }
        out
    } else {
        // Resolvent cubic 8m³ + 8p m² + (2p² − 8r) m − q² = 0, with a root m > 0.
        let resolvent = Poly::new(vec![
            -(&qq * &qq),
            q(2, 1) * &p * &p - q(8, 1) * &r,
            q(8, 1) * &p,
            q(8, 1),
        ]);
        let mval = positive_resolvent_root(&resolvent)?;
        let two_m = (Expr::int(2) * mval.clone()).simplify();
        let s2m = two_m.sqrt();
        let pe = rat_expr(p);
        let qe = rat_expr(qq);
        let mut out = Vec::new();
        for s in [1i64, -1] {
            let se = Expr::int(s);
            let disc = (-&two_m
                - Expr::int(2) * pe.clone()
                - Expr::int(2) * se.clone() * qe.clone() / s2m.clone())
            .simplify();
            let sq = sqrt_signed(&disc);
            let base = &se * &s2m;
            out.push(((&base - &sq) / Expr::int(2)).simplify());
            out.push(((base + sq) / Expr::int(2)).simplify());
        }
        out
    };
    Ok(ys
        .into_iter()
        .map(|y| (y + shift.clone()).simplify())
        .collect())
}

/// A positive root of the resolvent cubic: rational when there is one, else the largest
/// real root in radicals.
fn positive_resolvent_root(res: &Poly) -> MathResult<Expr> {
    if let Some(rs) = res.rational_roots() {
        if let Some(r) = rs.into_iter().filter(|r| r.is_positive()).max() {
            return Ok(rat_expr(r));
        }
    }
    let lead = res.leading();
    let m = res.scale(&(BigRational::one() / lead));
    let c = m.coeffs();
    let (a, b, cc) = (c[2].clone(), c[1].clone(), c[0].clone());
    let p = &b - &a * &a / q(3, 1);
    let qq = q(2, 27) * &a * &a * &a - &a * &b / q(3, 1) + &cc;
    let shift = rat_expr(-&a / q(3, 1));
    let best = depressed_cubic(&p, &qq)
        .into_iter()
        .map(|t| (t + shift.clone()).simplify())
        .filter_map(|e| numeric(&e).map(|v| (v, e)))
        .filter(|(v, _)| *v > 0.0)
        .max_by(|x, y| x.0.total_cmp(&y.0));
    best.map(|(_, e)| e)
        .ok_or_else(|| MathError::Unsupported("this quartic in radicals".into()))
}

/// Numeric roots of a polynomial with constant coefficients (lowest power first).
pub fn numeric_roots_of(coeffs: &[Expr]) -> MathResult<Vec<Root>> {
    let cs: Vec<Complex64> = coeffs
        .iter()
        .map(|c| {
            c.eval_complex(&crate::eval::EmptyEnv)
                .map_err(|_| MathError::NotANumber(format!("the coefficient {c}")))
        })
        .collect::<MathResult<_>>()?;
    let zs = numeric_roots(&cs)?;
    let mut out: Vec<Root> = zs
        .into_iter()
        .map(|z| Root {
            value: complex_expr(z),
            multiplicity: 1,
            exact: false,
        })
        .collect();
    sort_roots(&mut out);
    Ok(out)
}

/// An approximate complex number as an expression (real when the imaginary part is noise).
pub fn complex_expr(z: Complex64) -> Expr {
    if z.im.abs() <= 1e-10 * z.norm().max(1.0) {
        Expr::float(z.re)
    } else {
        Expr::num(Number::complex_float(z.re, z.im))
    }
}

/// Roots of `Σ cₖ zᵏ` by the Aberth–Ehrlich iteration, polished with Newton steps.
pub fn numeric_roots(coeffs: &[Complex64]) -> MathResult<Vec<Complex64>> {
    let mut cs = coeffs.to_vec();
    while cs.last().is_some_and(|c| c.norm() == 0.0) {
        cs.pop();
    }
    let n = cs.len().saturating_sub(1);
    if n == 0 {
        return Ok(Vec::new());
    }
    if n > MAX_DEGREE {
        return Err(MathError::TooLarge(format!("a polynomial of degree {n}")));
    }
    if cs.iter().any(|c| !c.re.is_finite() || !c.im.is_finite()) {
        return Err(MathError::NotANumber("a coefficient".into()));
    }
    // Zero roots first.
    let zeros = cs.iter().take_while(|c| c.norm() == 0.0).count();
    let rest: Vec<Complex64> = cs[zeros..].to_vec();
    let m = rest.len() - 1;
    let lead = rest[m];
    let monic: Vec<Complex64> = rest.iter().map(|c| c / lead).collect();
    let bound = 1.0 + monic[..m].iter().map(|c| c.norm()).fold(0.0, f64::max);
    let eval = |z: Complex64| -> (Complex64, Complex64) {
        let mut p = Complex64::new(0.0, 0.0);
        let mut dp = Complex64::new(0.0, 0.0);
        for c in monic.iter().rev() {
            dp = dp * z + p;
            p = p * z + c;
        }
        (p, dp)
    };
    let mut z: Vec<Complex64> = (0..m)
        .map(|k| {
            let angle = 2.0 * std::f64::consts::PI * (k as f64) / (m as f64) + 0.4;
            Complex64::from_polar(bound * 0.5, angle)
        })
        .collect();
    for _ in 0..1000 {
        let mut moved = 0.0f64;
        for i in 0..m {
            let (p, dp) = eval(z[i]);
            if p.norm() == 0.0 {
                continue;
            }
            let ratio = p / dp;
            let sum: Complex64 = (0..m)
                .filter(|&j| j != i)
                .map(|j| {
                    let d = z[i] - z[j];
                    if d.norm() == 0.0 {
                        Complex64::new(0.0, 0.0)
                    } else {
                        d.inv()
                    }
                })
                .sum();
            let denom = Complex64::new(1.0, 0.0) - ratio * sum;
            let step = if denom.norm() == 0.0 {
                ratio
            } else {
                ratio / denom
            };
            if step.re.is_finite() && step.im.is_finite() {
                z[i] -= step;
                moved = moved.max(step.norm() / z[i].norm().max(1.0));
            }
        }
        if moved < 1e-15 {
            break;
        }
    }
    let mut out = vec![Complex64::new(0.0, 0.0); zeros];
    out.extend(z);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::EmptyEnv;
    use proptest::prelude::*;

    fn check_roots(coeffs: &[i64]) {
        let p = Poly::from_ints(coeffs);
        let rs = rational_roots(&p).unwrap();
        let total: u32 = rs.iter().map(|r| r.multiplicity).sum();
        assert_eq!(total as usize, p.degree().unwrap(), "{coeffs:?}: {rs:?}");
        let cs: Vec<Complex64> = coeffs
            .iter()
            .map(|&c| Complex64::new(c as f64, 0.0))
            .collect();
        let scale: f64 = coeffs
            .iter()
            .map(|c| (*c as f64).abs())
            .sum::<f64>()
            .max(1.0);
        for r in &rs {
            let z = r.value.eval_complex(&EmptyEnv).unwrap();
            let mut v = Complex64::new(0.0, 0.0);
            for c in cs.iter().rev() {
                v = v * z + c;
            }
            let zs = z.norm().max(1.0).powi(coeffs.len() as i32);
            assert!(
                v.norm() < 1e-7 * scale * zs,
                "{coeffs:?}: root {} gives {v}",
                r.value
            );
        }
    }

    #[test]
    fn rational_and_quadratic() {
        let rs = rational_roots(&Poly::from_ints(&[-6, 1, 1])).unwrap();
        assert_eq!(
            rs.iter().map(|r| r.value.clone()).collect::<Vec<_>>(),
            vec![Expr::int(-3), Expr::int(2)]
        );
        let rs = rational_roots(&Poly::from_ints(&[-2, 0, 1])).unwrap();
        assert_eq!(rs[1].value, Expr::int(2).sqrt());
        let rs = rational_roots(&Poly::from_ints(&[1, 0, 1])).unwrap();
        assert_eq!(rs.len(), 2);
        assert!(rs.iter().all(|r| r.exact));
        // (x − 1)² (x + 2)
        let rs = rational_roots(&Poly::from_ints(&[2, -3, 0, 1])).unwrap();
        assert_eq!(rs.len(), 2);
        assert_eq!(rs[1].multiplicity, 2);
    }

    #[test]
    fn cubics_and_quartics() {
        check_roots(&[-2, 0, 0, 1]); // ∛2
        check_roots(&[1, -3, 0, 1]); // three real roots, casus irreducibilis
        check_roots(&[1, 1, 0, 1]); // one real, two complex
        check_roots(&[1, 0, 0, 0, 1]); // x⁴ + 1
        check_roots(&[6, 0, -5, 0, 1]); // (x² − 2)(x² − 3)
        check_roots(&[1, 1, 1, 1, 1]); // cyclotomic
        check_roots(&[-1, 1, 0, 0, 1]); // general quartic, irrational resolvent
        check_roots(&[3, 1, -4, 2, 1]);
        check_roots(&[1, 0, 0, 0, 0, 1, 1]); // degree 6: numeric parts
    }

    #[test]
    fn casus_irreducibilis_is_real() {
        let rs = rational_roots(&Poly::from_ints(&[1, -3, 0, 1])).unwrap();
        assert_eq!(rs.len(), 3);
        for r in &rs {
            assert!(r.exact);
            assert!(
                r.value.eval_f64(&EmptyEnv).is_ok(),
                "{} is not real",
                r.value
            );
        }
    }

    #[test]
    fn symbolic_quadratic() {
        let (a, b, c) = (Expr::sym("a"), Expr::sym("b"), Expr::sym("c"));
        let rs = roots(&[c.clone(), b.clone(), a.clone()]).unwrap();
        assert_eq!(rs.len(), 2);
        let env = [("a", 1.0), ("b", -3.0), ("c", 2.0)];
        let mut vals: Vec<f64> = rs.iter().map(|r| r.value.eval_f64(&env).unwrap()).collect();
        vals.sort_by(f64::total_cmp);
        assert!((vals[0] - 1.0).abs() < 1e-12 && (vals[1] - 2.0).abs() < 1e-12);
        assert!(roots(&[c, b, a.clone(), a]).is_err());
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(48))]
        #[test]
        fn roots_satisfy_the_polynomial(coeffs in prop::collection::vec(-9i64..=9, 2..=5)) {
            prop_assume!(coeffs.last().is_some_and(|c| *c != 0));
            check_roots(&coeffs);
        }
    }
}
