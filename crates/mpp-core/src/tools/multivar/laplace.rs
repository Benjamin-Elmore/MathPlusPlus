//! Laplace transforms: the table (powers, exponentials, sines and cosines, hyperbolic
//! functions), linearity, the first shifting theorem (`e^{at}`) and multiplication by `tⁿ`;
//! the inverse by partial fractions; and linear ODEs with constant coefficients solved by
//! transforming them.

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};

use crate::calc::diff;
use crate::calc::integrate::rational::{decompose, to_rational};
use crate::domains::util::{self, rat_expr};
use crate::error::{MathError, MathResult};
use crate::expr::{Constant, Expr, ExprNode};
use crate::functions::{func, Func};
use crate::poly::Poly;
use crate::simplify::{add, mul, pow, simplify};

/// Most derivatives taken for `tⁿ·f(t)`.
const MAX_POWER: i64 = 20;

fn unsupported(f: &Expr) -> MathError {
    MathError::Unsupported(format!("the Laplace transform of {f}"))
}

/// `(a, b)` with `e = a·t + b`, both free of `t`.
fn linear(e: &Expr, t: &str) -> Option<(Expr, Expr)> {
    let c = e.as_poly(t)?;
    match c.as_slice() {
        [b] => Some((Expr::zero(), b.clone())),
        [b, a] => Some((a.clone(), b.clone())),
        _ => None,
    }
}

/// `L{f}(s)` with respect to `t`.
pub fn laplace(f: &Expr, t: &str, s: &str) -> MathResult<Expr> {
    let f = simplify(f);
    if !f.contains_sym(t) {
        return Ok(simplify(&(f / Expr::sym(s))));
    }
    if let ExprNode::Add(terms) = f.node() {
        let parts = terms
            .iter()
            .map(|term| laplace(term, t, s))
            .collect::<MathResult<Vec<_>>>()?;
        return Ok(simplify(&add(parts)));
    }
    let factors: Vec<Expr> = match f.node() {
        ExprNode::Mul(fs) => fs.clone(),
        _ => vec![f.clone()],
    };
    let mut constant = Vec::new();
    let mut power: i64 = 0;
    let mut shift = Expr::zero();
    let mut kernel: Option<(Func, Expr)> = None;
    for g in &factors {
        if !g.contains_sym(t) {
            constant.push(g.clone());
            continue;
        }
        match g.node() {
            ExprNode::Sym(_) => power += 1,
            ExprNode::Pow(b, x) if b.is_sym(t) => {
                let n = x
                    .as_number()
                    .and_then(|n| n.as_i64())
                    .filter(|n| (0..=MAX_POWER).contains(n))
                    .ok_or_else(|| unsupported(&f))?;
                power += n;
            }
            ExprNode::Pow(b, x) if matches!(b.node(), ExprNode::Const(Constant::E)) => {
                let (a, c) = linear(x, t).ok_or_else(|| unsupported(&f))?;
                shift = simplify(&(shift + a));
                constant.push(pow(Expr::e(), c));
            }
            ExprNode::Func(fun @ (Func::Sin | Func::Cos | Func::Sinh | Func::Cosh), args)
                if kernel.is_none() =>
            {
                let arg = args.first().ok_or_else(|| unsupported(&f))?;
                let (b, c) = linear(arg, t).ok_or_else(|| unsupported(&f))?;
                if !util::is_zero(&c) {
                    return Err(unsupported(&f));
                }
                kernel = Some((*fun, b));
            }
            _ => return Err(unsupported(&f)),
        }
    }
    let sv = Expr::sym(s);
    // The transform of the kernel at s.
    let base = match &kernel {
        None => pow(sv.clone(), Expr::int(-1)),
        Some((fun, b)) => {
            let b2 = pow(b.clone(), Expr::int(2));
            let s2 = pow(sv.clone(), Expr::int(2));
            let (top, den) = match fun {
                Func::Sin => (b.clone(), s2 + b2),
                Func::Cos => (sv.clone(), s2 + b2),
                Func::Sinh => (b.clone(), s2 - b2),
                _ => (sv.clone(), s2 - b2),
            };
            mul(vec![top, pow(den, Expr::int(-1))])
        }
    };
    // tⁿ g(t) ↦ (−1)ⁿ dⁿG/dsⁿ.
    let mut g = simplify(&base);
    if power > 0 {
        let n = u32::try_from(power).map_err(|_| unsupported(&f))?;
        g = diff(&g, s, n, None)?;
        if power % 2 == 1 {
            g = -g;
        }
    }
    // e^{at} g(t) ↦ G(s − a).
    if !util::is_zero(&shift) {
        g = g.subs(s, &(sv - shift));
    }
    Ok(simplify(&mul(vec![mul(constant), g])))
}

fn rat(q: &BigRational) -> Expr {
    rat_expr(q.clone())
}

fn coeff(p: &Poly, k: usize) -> BigRational {
    p.coeffs().get(k).cloned().unwrap_or_else(BigRational::zero)
}

fn factorial(n: u32) -> BigInt {
    (1..=n).fold(BigInt::one(), |acc, k| acc * BigInt::from(k))
}

/// `L⁻¹{F}(t)` for a proper rational function of `s`, by partial fractions.
pub fn inverse_laplace(big_f: &Expr, s: &str, t: &str) -> MathResult<Expr> {
    let e = simplify(big_f);
    let fail = || MathError::Unsupported(format!("the inverse Laplace transform of {e}"));
    let (num, den) = to_rational(&e, s).ok_or_else(fail)?;
    if num.is_zero() {
        return Ok(Expr::zero());
    }
    match (num.degree(), den.degree()) {
        (Some(a), Some(b)) if a < b => {}
        _ => {
            return Err(MathError::Unsupported(format!(
                "the inverse Laplace transform of {e} (it is not a proper fraction, so it would need impulses)"
            )))
        }
    }
    let terms = decompose(&num, &den).ok_or_else(fail)?;
    let tv = Expr::sym(t);
    let mut out = Vec::new();
    for (f, k, numer) in terms {
        match f.degree() {
            Some(1) => {
                // A/(s − a)^k ↦ A t^{k−1} e^{at}/(k − 1)!
                let a = -coeff(&f, 0);
                let big_a = coeff(&numer, 0);
                let fact = rat_expr(BigRational::from_integer(factorial(k - 1)));
                out.push(mul(vec![
                    rat(&big_a),
                    pow(tv.clone(), Expr::int(i64::from(k) - 1)),
                    pow(Expr::e(), mul(vec![rat(&a), tv.clone()])),
                    pow(fact, Expr::int(-1)),
                ]));
            }
            Some(2) => out.push(quadratic_term(&f, k, &numer, &tv).ok_or_else(fail)?),
            _ => return Err(fail()),
        }
    }
    Ok(simplify(&add(out)))
}

/// `(Bs + C)/((s − α)² ± β²)^k` back to `t`.
fn quadratic_term(f: &Poly, k: u32, numer: &Poly, t: &Expr) -> Option<Expr> {
    let p = coeff(f, 1);
    let q = coeff(f, 0);
    let two = BigRational::from_integer(BigInt::from(2));
    let alpha = -(&p / &two);
    let beta2 = &q - &(&p * &p) / BigRational::from_integer(BigInt::from(4));
    let big_b = coeff(numer, 1);
    let big_c = coeff(numer, 0);
    let big_d = &big_c + &(&big_b * &alpha);
    let shift = pow(Expr::e(), mul(vec![rat(&alpha), t.clone()]));
    let root = |x: &BigRational| simplify(&pow(rat(x), Expr::rational(1, 2)));
    let body = if beta2.is_positive() {
        let beta = root(&beta2);
        let bt = mul(vec![beta.clone(), t.clone()]);
        let (sin, cos) = (
            func(Func::Sin, vec![bt.clone()]),
            func(Func::Cos, vec![bt.clone()]),
        );
        match k {
            1 => add(vec![
                mul(vec![rat(&big_b), cos]),
                mul(vec![rat(&big_d), pow(beta, Expr::int(-1)), sin]),
            ]),
            2 => {
                // (s−α)/((s−α)²+β²)² ↦ t sin βt/(2β); 1/((s−α)²+β²)² ↦ (sin βt − βt cos βt)/(2β³).
                let first = mul(vec![
                    rat(&big_b),
                    t.clone(),
                    sin.clone(),
                    pow(mul(vec![Expr::int(2), beta.clone()]), Expr::int(-1)),
                ]);
                let second = mul(vec![
                    rat(&big_d),
                    add(vec![sin, mul(vec![Expr::int(-1), bt, cos])]),
                    pow(
                        mul(vec![Expr::int(2), pow(beta, Expr::int(3))]),
                        Expr::int(-1),
                    ),
                ]);
                add(vec![first, second])
            }
            _ => return None,
        }
    } else if beta2.is_negative() && k == 1 {
        let gamma = root(&-beta2);
        let gt = mul(vec![gamma.clone(), t.clone()]);
        add(vec![
            mul(vec![rat(&big_b), func(Func::Cosh, vec![gt.clone()])]),
            mul(vec![
                rat(&big_d),
                pow(gamma, Expr::int(-1)),
                func(Func::Sinh, vec![gt]),
            ]),
        ])
    } else {
        return None;
    };
    Some(mul(vec![shift, body]))
}

/// Solves `Σ aₖ y⁽ᵏ⁾ = g(t)` (coefficients `a₀, a₁, …` from the lowest order up) with
/// `y(0), y′(0), …` given, by the Laplace transform. Returns `Y(s)` and `y(t)`; the
/// solution is checked by putting it back into the equation.
pub fn laplace_ode(
    coeffs: &[Expr],
    forcing: &Expr,
    initial: &[Expr],
    t: &str,
    s: &str,
) -> MathResult<(Expr, Expr)> {
    let order = coeffs.len().saturating_sub(1);
    if order == 0 || initial.len() != order {
        return Err(MathError::WrongArity {
            function: "the initial conditions".into(),
            expected: order.to_string(),
            got: initial.len(),
        });
    }
    if coeffs.iter().any(|a| !a.is_constant()) {
        return Err(MathError::Unsupported(
            "the Laplace method for coefficients that are not constant".into(),
        ));
    }
    let sv = Expr::sym(s);
    let big_g = laplace(forcing, t, s)?;
    // L{y⁽ᵏ⁾} = sᵏY − Σ_{j<k} s^{k−1−j} y⁽ʲ⁾(0)
    let mut char_poly = Vec::new();
    let mut from_initial = Vec::new();
    for (k, a) in coeffs.iter().enumerate() {
        char_poly.push(mul(vec![a.clone(), pow(sv.clone(), Expr::int(k as i64))]));
        for (j, y0) in initial.iter().enumerate().take(k) {
            from_initial.push(mul(vec![
                a.clone(),
                pow(sv.clone(), Expr::int((k - 1 - j) as i64)),
                y0.clone(),
            ]));
        }
    }
    let big_y = simplify(&mul(vec![
        add(vec![big_g, add(from_initial)]),
        pow(add(char_poly), Expr::int(-1)),
    ]));
    let y = inverse_laplace(&big_y, s, t)?;
    check_ode(coeffs, forcing, initial, &y, t)?;
    Ok((big_y, y))
}

/// Puts `y` back into the equation and the initial conditions (numerically at a few
/// points), so a wrong solution is never returned.
fn check_ode(
    coeffs: &[Expr],
    forcing: &Expr,
    initial: &[Expr],
    y: &Expr,
    t: &str,
) -> MathResult<()> {
    let mut lhs = Vec::new();
    let mut d = y.clone();
    let mut derivs = vec![y.clone()];
    for a in coeffs {
        lhs.push(mul(vec![a.clone(), d.clone()]));
        d = diff(&d, t, 1, None)?;
        derivs.push(d.clone());
    }
    let residual = simplify(&(add(lhs) - forcing.clone()));
    let ok_at = |e: &Expr, x: f64| -> bool {
        match crate::domains::solve::numeric::eval_at(e, t, x) {
            Some(v) => v.abs() <= 1e-7 * (1.0 + v.abs()),
            None => false,
        }
    };
    let equation_ok = [0.3, 0.7, 1.3].iter().all(|x| ok_at(&residual, *x));
    let initial_ok = initial.iter().zip(&derivs).all(|(y0, dk)| {
        let at0 = simplify(&dk.subs(t, &Expr::zero()));
        util::equal(&at0, y0)
    });
    if equation_ok && initial_ok {
        Ok(())
    } else {
        Err(MathError::Inconclusive(
            "the Laplace solution (it did not check out when put back into the equation)".into(),
        ))
    }
}
