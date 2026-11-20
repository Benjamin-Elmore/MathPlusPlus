//! The table of standard integrals, each also matched with a linear inner function
//! `a·x + b` (the answer is then divided by `a`).

use crate::expr::{Constant, Expr, ExprNode};
use crate::functions::{func, Func};
use crate::simplify::{add, mul, pow};

use super::engine::{div, linear_coeffs};

/// A table entry that matched: the antiderivative, the rule name and an optional note.
pub(crate) type Hit = (Expr, &'static str, Option<String>);

fn f1(f: Func, u: &Expr) -> Expr {
    func(f, vec![u.clone()])
}

fn ln_abs(u: &Expr) -> Expr {
    f1(Func::Ln, &f1(Func::Abs, u))
}

fn neg(e: Expr) -> Expr {
    mul(vec![Expr::int(-1), e])
}

/// Divides by the inner slope `a` and adds a note when the inner function is not `x`.
fn scaled(result: Expr, a: &Expr, inner: &Expr, var: &str, rule: &'static str) -> Hit {
    if a.is_one() && inner.is_sym(var) {
        return (result, rule, None);
    }
    let u = if var == "u" { "w" } else { "u" };
    let note = if a.is_one() {
        format!("with {u} = {inner}, d{u} = d{var}")
    } else {
        format!("linear inner function {u} = {inner}, d{u} = {a} d{var}: divide by {a}")
    };
    (div(result, a.clone()), rule, Some(note))
}

/// The table entry for `e`, if one matches.
pub(crate) fn lookup(e: &Expr, var: &str) -> Option<Hit> {
    if e.is_sym(var) {
        let r = mul(vec![
            Expr::rational(1, 2),
            pow(Expr::sym(var), Expr::int(2)),
        ]);
        return Some((r, "power rule", Some("∫ xⁿ dx = xⁿ⁺¹/(n + 1)".into())));
    }
    match e.node() {
        ExprNode::Pow(base, exp) => power(base, exp, var),
        ExprNode::Func(f, args) if args.len() == 1 => function(*f, &args[0], var),
        ExprNode::Mul(fs) if fs.len() == 2 => product(&fs[0], &fs[1], var),
        _ => None,
    }
}

fn power(base: &Expr, exp: &Expr, var: &str) -> Option<Hit> {
    // (a x + b)^n
    if !exp.contains_sym(var) {
        if let Some((a, _)) = linear_coeffs(base, var) {
            if exp.as_number().is_some_and(|n| n.is_minus_one()) {
                return Some(scaled(ln_abs(base), &a, base, var, "integral of 1/x"));
            }
            let n1 = add(vec![exp.clone(), Expr::one()]);
            if n1.is_zero() {
                return None;
            }
            let r = div(pow(base.clone(), n1.clone()), n1);
            let mut hit = scaled(r, &a, base, var, "power rule");
            if !exp.is_number() {
                hit.2 = Some(format!(
                    "{}assuming {exp} ≠ −1",
                    hit.2.map(|n| format!("{n}; ")).unwrap_or_default()
                ));
            } else if hit.2.is_none() {
                hit.2 = Some("∫ xⁿ dx = xⁿ⁺¹/(n + 1)".into());
            }
            return Some(hit);
        }
        // Squares and reciprocal squares of trig and hyperbolic functions.
        if let (ExprNode::Func(f, args), Some(k)) =
            (base.node(), exp.as_number().and_then(|n| n.as_i64()))
        {
            let u = args.first()?;
            let (a, _) = linear_coeffs(u, var)?;
            let (r, rule) = match (f, k) {
                (Func::Sec, 2) | (Func::Cos, -2) => (f1(Func::Tan, u), "integral of sec²"),
                (Func::Csc, 2) | (Func::Sin, -2) => (neg(f1(Func::Cot, u)), "integral of csc²"),
                (Func::Cosh, -2) => (f1(Func::Tanh, u), "integral of sech²"),
                (Func::Sinh, -2) => (
                    neg(pow(f1(Func::Tanh, u), Expr::int(-1))),
                    "integral of csch²",
                ),
                (Func::Sin | Func::Cos | Func::Tan | Func::Cot, -1) => {
                    let g = match f {
                        Func::Sin => Func::Csc,
                        Func::Cos => Func::Sec,
                        Func::Tan => Func::Cot,
                        _ => Func::Tan,
                    };
                    return function(g, u, var);
                }
                _ => return None,
            };
            return Some(scaled(r, &a, u, var, rule));
        }
        return None;
    }
    // c^(a x + b)
    if !base.contains_sym(var) {
        let (a, _) = linear_coeffs(exp, var)?;
        let e = pow(base.clone(), exp.clone());
        if matches!(base.node(), ExprNode::Const(Constant::E)) {
            return Some(scaled(e, &a, exp, var, "integral of eˣ"));
        }
        let ln_b = f1(Func::Ln, base);
        if ln_b.is_zero() {
            return None;
        }
        return Some(scaled(div(e, ln_b), &a, exp, var, "integral of aˣ"));
    }
    None
}

fn function(f: Func, u: &Expr, var: &str) -> Option<Hit> {
    let (a, _) = linear_coeffs(u, var)?;
    let (r, rule) = match f {
        Func::Sin => (neg(f1(Func::Cos, u)), "integral of sin"),
        Func::Cos => (f1(Func::Sin, u), "integral of cos"),
        Func::Tan => (neg(ln_abs(&f1(Func::Cos, u))), "integral of tan"),
        Func::Cot => (ln_abs(&f1(Func::Sin, u)), "integral of cot"),
        Func::Sec => (
            ln_abs(&add(vec![f1(Func::Sec, u), f1(Func::Tan, u)])),
            "integral of sec",
        ),
        Func::Csc => (
            neg(ln_abs(&add(vec![f1(Func::Csc, u), f1(Func::Cot, u)]))),
            "integral of csc",
        ),
        Func::Sinh => (f1(Func::Cosh, u), "integral of sinh"),
        Func::Cosh => (f1(Func::Sinh, u), "integral of cosh"),
        Func::Tanh => (f1(Func::Ln, &f1(Func::Cosh, u)), "integral of tanh"),
        Func::Ln => (
            add(vec![mul(vec![u.clone(), f1(Func::Ln, u)]), neg(u.clone())]),
            "integral of ln",
        ),
        Func::Abs => (
            mul(vec![Expr::rational(1, 2), u.clone(), f1(Func::Abs, u)]),
            "integral of |x|",
        ),
        _ => return None,
    };
    Some(scaled(r, &a, u, var, rule))
}

fn product(p: &Expr, q: &Expr, var: &str) -> Option<Hit> {
    let (ExprNode::Func(f, fa), ExprNode::Func(g, ga)) = (p.node(), q.node()) else {
        return None;
    };
    let u = fa.first()?;
    if ga.first() != Some(u) {
        return None;
    }
    let (a, _) = linear_coeffs(u, var)?;
    let (r, rule) = match (f, g) {
        (Func::Sec, Func::Tan) | (Func::Tan, Func::Sec) => {
            (f1(Func::Sec, u), "integral of sec·tan")
        }
        (Func::Csc, Func::Cot) | (Func::Cot, Func::Csc) => {
            (neg(f1(Func::Csc, u)), "integral of csc·cot")
        }
        _ => return None,
    };
    Some(scaled(r, &a, u, var, rule))
}
