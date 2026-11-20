//! The calculus hook the domains use (ODE solving needs antiderivatives).
//!
//! The full calculus (`calc::diff`, `calc::integrate`) lives in its own module; it plugs
//! in through [`Calculus`] on the context. [`BasicCalculus`] is the small built-in
//! fallback: complete differentiation of elementary functions, and antiderivatives of
//! polynomials, powers of linear terms, `e^(ax+b)`, `sin`/`cos` of linear terms and
//! polynomial multiples of those.

use crate::error::{MathError, MathResult};
use crate::expr::{Constant, Expr, ExprNode};
use crate::functions::Func;
use crate::simplify::{add, mul};

/// Differentiation and integration, as the domains need them.
pub trait Calculus: Send + Sync {
    /// `d/dvar e`.
    fn diff(&self, e: &Expr, var: &str) -> MathResult<Expr>;

    /// An antiderivative `∫ e dvar` (no constant), or an error when none is found.
    fn integrate(&self, e: &Expr, var: &str) -> MathResult<Expr>;
}

/// The built-in fallback calculus.
#[derive(Clone, Copy, Debug, Default)]
pub struct BasicCalculus;

/// Most nested calls the tabular integration may make.
const MAX_TABULAR: usize = 32;

impl Calculus for BasicCalculus {
    fn diff(&self, e: &Expr, var: &str) -> MathResult<Expr> {
        diff(e, var).map(|d| d.simplify())
    }

    fn integrate(&self, e: &Expr, var: &str) -> MathResult<Expr> {
        integrate(&e.simplify(), var).map(|r| r.simplify())
    }
}

/// Derivative of `e` with respect to `var` (elementary functions).
pub fn diff(e: &Expr, var: &str) -> MathResult<Expr> {
    if !e.contains_sym(var) {
        return Ok(Expr::zero());
    }
    Ok(match e.node() {
        ExprNode::Sym(_) => Expr::one(),
        ExprNode::Add(ts) => add(ts.iter().map(|t| diff(t, var)).collect::<MathResult<_>>()?),
        ExprNode::Mul(fs) => {
            let mut terms = Vec::with_capacity(fs.len());
            for i in 0..fs.len() {
                let mut prod = fs.clone();
                prod[i] = diff(&fs[i], var)?;
                terms.push(mul(prod));
            }
            add(terms)
        }
        ExprNode::Pow(b, p) => {
            if !p.contains_sym(var) {
                // d(b^p) = p b^(p-1) b'
                mul(vec![p.clone(), b.pow(p - &Expr::one()), diff(b, var)?])
            } else {
                // d(b^p) = b^p (p' ln b + p b'/b)
                let ln_b = Expr::func(Func::Ln, vec![b.clone()])?;
                let inner = diff(p, var)? * ln_b + p * &diff(b, var)? / b.clone();
                e * &inner
            }
        }
        ExprNode::Func(f, args) => {
            let [u] = args.as_slice() else {
                return Err(MathError::Unsupported(format!(
                    "differentiating {} here",
                    f.name()
                )));
            };
            let du = diff(u, var)?;
            let call = |g: Func, a: Expr| Expr::func(g, vec![a]);
            let one = Expr::one();
            let outer = match f {
                Func::Sin => call(Func::Cos, u.clone())?,
                Func::Cos => -call(Func::Sin, u.clone())?,
                Func::Tan => call(Func::Cos, u.clone())?.pow(-2),
                Func::Sec => call(Func::Sec, u.clone())? * call(Func::Tan, u.clone())?,
                Func::Csc => -(call(Func::Csc, u.clone())? * call(Func::Cot, u.clone())?),
                Func::Cot => -(call(Func::Sin, u.clone())?.pow(-2)),
                Func::Asin => (&one - &u.pow(2)).pow(Expr::rational(-1, 2)),
                Func::Acos => -(&one - &u.pow(2)).pow(Expr::rational(-1, 2)),
                Func::Atan => (&one + &u.pow(2)).recip(),
                Func::Sinh => call(Func::Cosh, u.clone())?,
                Func::Cosh => call(Func::Sinh, u.clone())?,
                Func::Tanh => call(Func::Cosh, u.clone())?.pow(-2),
                Func::Asinh => (u.pow(2) + one).pow(Expr::rational(-1, 2)),
                Func::Acosh => (u.pow(2) - one).pow(Expr::rational(-1, 2)),
                Func::Atanh => (&one - &u.pow(2)).recip(),
                Func::Ln => u.recip(),
                Func::Abs => call(Func::Sign, u.clone())?,
                _ => {
                    return Err(MathError::Unsupported(format!(
                        "differentiating {} here",
                        f.name()
                    )))
                }
            };
            outer * du
        }
        _ => return Err(MathError::Unsupported(format!("differentiating {e} here"))),
    })
}

/// `(a, b)` with `u = a·var + b`, when `u` is linear in `var` with `a ≠ 0`.
fn linear(u: &Expr, var: &str) -> Option<(Expr, Expr)> {
    let c = u.as_poly(var)?;
    match c.as_slice() {
        [b, a] if !a.contains_sym(var) && !a.is_zero() => Some((a.clone(), b.clone())),
        _ => None,
    }
}

fn not_found(e: &Expr) -> MathError {
    MathError::Unsupported(format!(
        "finding an antiderivative of {e} with the basic rules"
    ))
}

/// An antiderivative of `e` in `var` by the basic rules.
pub fn integrate(e: &Expr, var: &str) -> MathResult<Expr> {
    integrate_depth(e, var, 0)
}

fn integrate_depth(e: &Expr, var: &str, depth: usize) -> MathResult<Expr> {
    if depth > MAX_TABULAR {
        return Err(not_found(e));
    }
    let x = Expr::sym(var);
    if !e.contains_sym(var) {
        return Ok(e * &x);
    }
    match e.node() {
        ExprNode::Add(ts) => {
            return Ok(add(ts
                .iter()
                .map(|t| integrate_depth(t, var, depth + 1))
                .collect::<MathResult<_>>()?))
        }
        ExprNode::Mul(fs) => {
            let (consts, vars): (Vec<Expr>, Vec<Expr>) =
                fs.iter().cloned().partition(|f| !f.contains_sym(var));
            if !consts.is_empty() {
                let inner = integrate_depth(&mul(vars), var, depth + 1)?;
                return Ok(mul(consts) * inner);
            }
            return integrate_product(e, &vars, var, depth);
        }
        _ => {}
    }
    if let Some(r) = integrate_simple(e, var)? {
        return Ok(r);
    }
    if e.is_polynomial_in(var) {
        let coeffs = e.as_poly(var).ok_or_else(|| not_found(e))?;
        let terms = coeffs
            .iter()
            .enumerate()
            .map(|(k, c)| {
                let k1 = Expr::int(k as i64 + 1);
                c * &x.pow(k1.clone()) / k1
            })
            .collect();
        return Ok(add(terms));
    }
    Err(not_found(e))
}

/// `∫ f(a x + b) dx` for the basic functions `f`.
fn integrate_simple(e: &Expr, var: &str) -> MathResult<Option<Expr>> {
    let x = Expr::sym(var);
    Ok(match e.node() {
        ExprNode::Sym(_) => Some(x.pow(2) / Expr::int(2)),
        ExprNode::Pow(b, p) if !p.contains_sym(var) => match linear(b, var) {
            Some((a, _)) => {
                if p.as_number().is_some_and(|n| n.is_minus_one()) {
                    Some(Expr::func(Func::Ln, vec![Expr::func(Func::Abs, vec![b.clone()])?])? / a)
                } else {
                    let p1 = p + &Expr::one();
                    Some(b.pow(p1.clone()) / (p1 * a))
                }
            }
            None => None,
        },
        ExprNode::Pow(b, p) if !b.contains_sym(var) => match linear(p, var) {
            Some((a, _)) => {
                if matches!(b.node(), ExprNode::Const(Constant::E)) {
                    Some(e / &a)
                } else {
                    Some(e / &(a * Expr::func(Func::Ln, vec![b.clone()])?))
                }
            }
            None => None,
        },
        ExprNode::Func(f, args) => match (f, args.as_slice()) {
            (Func::Sin, [u]) => linear(u, var)
                .map(|(a, _)| Expr::func(Func::Cos, vec![u.clone()]).map(|c| -c / a))
                .transpose()?,
            (Func::Cos, [u]) => linear(u, var)
                .map(|(a, _)| Expr::func(Func::Sin, vec![u.clone()]).map(|s| s / a))
                .transpose()?,
            (Func::Sinh, [u]) => linear(u, var)
                .map(|(a, _)| Expr::func(Func::Cosh, vec![u.clone()]).map(|c| c / a))
                .transpose()?,
            (Func::Cosh, [u]) => linear(u, var)
                .map(|(a, _)| Expr::func(Func::Sinh, vec![u.clone()]).map(|s| s / a))
                .transpose()?,
            _ => None,
        },
        _ => None,
    })
}

/// `∫ c·f(a x + b) dx` for a constant `c` and a basic function `f`.
fn integrate_scaled(e: &Expr, var: &str) -> MathResult<Option<Expr>> {
    let ExprNode::Mul(fs) = e.node() else {
        return integrate_simple(e, var);
    };
    let (consts, rest): (Vec<Expr>, Vec<Expr>) =
        fs.iter().cloned().partition(|f| !f.contains_sym(var));
    Ok(integrate_simple(&mul(rest), var)?.map(|r| mul(consts) * r))
}

/// `∫ p(x)·f(x) dx` with `p` a polynomial and `f` a basic function, by the tabular method;
/// otherwise a product of powers of the same base.
fn integrate_product(e: &Expr, factors: &[Expr], var: &str, depth: usize) -> MathResult<Expr> {
    let (polys, others): (Vec<Expr>, Vec<Expr>) = factors
        .iter()
        .cloned()
        .partition(|f| f.is_polynomial_in(var));
    if others.is_empty() {
        let expanded = mul(polys).expand()?;
        if &expanded == e {
            return Err(not_found(e));
        }
        return integrate_depth(&expanded, var, depth + 1);
    }
    if others.len() != 1 || polys.is_empty() {
        return Err(not_found(e));
    }
    let p = mul(polys);
    let f = &others[0];
    // Σ (-1)^k p^(k) F_{k+1}
    let mut terms = Vec::new();
    let mut dp = p;
    let mut anti = f.clone();
    let mut sign = Expr::one();
    for _ in 0..MAX_TABULAR {
        if dp.is_zero() {
            return Ok(add(terms));
        }
        anti = integrate_scaled(&anti, var)?
            .map(|a| a.simplify())
            .ok_or_else(|| not_found(e))?;
        terms.push(mul(vec![sign.clone(), dp.clone(), anti.clone()]));
        dp = diff(&dp, var)?.simplify();
        sign = -sign;
    }
    Err(not_found(e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x() -> Expr {
        Expr::sym("x")
    }

    /// Checks d/dx ∫ e = e numerically at a few points.
    fn check_antiderivative(e: &Expr) {
        let c = BasicCalculus;
        let f = c.integrate(e, "x").unwrap();
        let df = c.diff(&f, "x").unwrap();
        for t in [0.3, 0.7, 1.9] {
            let env = [("x", t)];
            let a = df.eval_f64(&env).unwrap();
            let b = e.eval_f64(&env).unwrap();
            assert!((a - b).abs() < 1e-9, "∫{e} = {f}: {a} vs {b}");
        }
    }

    #[test]
    fn derivatives() {
        let c = BasicCalculus;
        let d = c.diff(&(x().pow(3) + Expr::int(2) * x()), "x").unwrap();
        assert_eq!(d, Expr::int(3) * x().pow(2) + Expr::int(2));
        let d = c
            .diff(&Expr::call("sin", vec![x().pow(2)]).unwrap(), "x")
            .unwrap();
        let env = [("x", 0.5)];
        assert!((d.eval_f64(&env).unwrap() - 2.0 * 0.5 * (0.25f64).cos()).abs() < 1e-12);
    }

    #[test]
    fn antiderivatives() {
        let exp = |u: Expr| Expr::e().pow(u);
        let sin = |u: Expr| Expr::call("sin", vec![u]).unwrap();
        let cos = |u: Expr| Expr::call("cos", vec![u]).unwrap();
        for e in [
            x().pow(4) - Expr::int(3) * x() + Expr::int(7),
            exp(Expr::int(2) * x()),
            x() * exp(x()),
            x().pow(2) * sin(Expr::int(3) * x()),
            cos(x() + Expr::one()),
            (Expr::int(2) * x() + Expr::one()).recip(),
            (x() + Expr::one()).pow(Expr::rational(1, 2)),
            Expr::int(2).pow(x()),
        ] {
            check_antiderivative(&e);
        }
        assert!(BasicCalculus.integrate(&exp(x().pow(2)), "x").is_err());
    }
}
