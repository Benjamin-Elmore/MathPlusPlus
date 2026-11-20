//! Trigonometric identities, for proving checks such as `sin²x + cos²x = 1` and for
//! tidying answers.
//!
//! [`identity_zero`] decides whether an expression is identically zero by bringing it to a
//! normal form: `tan`, `sec`, `csc` and `cot` written with `sin` and `cos`; multiple angles
//! (`sin 2x`, `cos 3x`) and angle sums (`sin(a + b)`) expanded; everything over one
//! denominator; then every `cos²u` replaced by `1 − sin²u` (and `cosh²u` by `1 + sinh²u`)
//! and the numerator expanded. Polynomials in `sin u` and `cos u` with `cos u` to at most
//! the first power are unique modulo `sin²u + cos²u = 1`, so a numerator that expands to 0
//! is a proof (wherever the denominators are not zero).
//!
//! [`tidy`] applies only the Pythagorean reduction and keeps the result when it is shorter
//! (`r sin²θ + r cos²θ` becomes `r`).

use crate::expr::{Expr, ExprNode};
use crate::functions::{func, Func};
use crate::simplify::{add, mul};

/// Largest multiple of an angle expanded (`sin 6x`).
const MAX_MULTIPLE: i64 = 6;
/// Largest power reduced by the Pythagorean identity.
const MAX_POWER: i64 = 24;
/// Expressions larger than this are left alone (expansion grows fast).
const MAX_SIZE: usize = 4_000;

fn sin(u: Expr) -> Expr {
    func(Func::Sin, vec![u])
}

fn cos(u: Expr) -> Expr {
    func(Func::Cos, vec![u])
}

/// True when `e` mentions a trigonometric or hyperbolic function.
pub fn has_trig(e: &Expr) -> bool {
    match e.node() {
        ExprNode::Func(
            Func::Sin
            | Func::Cos
            | Func::Tan
            | Func::Sec
            | Func::Csc
            | Func::Cot
            | Func::Sinh
            | Func::Cosh
            | Func::Tanh,
            _,
        ) => true,
        _ => e.children().into_iter().any(has_trig),
    }
}

/// `tan`, `sec`, `csc`, `cot` and `tanh` in terms of `sin`, `cos`, `sinh`, `cosh`, with
/// multiple angles and angle sums expanded.
fn rewrite(e: &Expr) -> Expr {
    let e = Expr::new(e.map_children(&mut rewrite));
    let ExprNode::Func(f, args) = e.node() else {
        return e;
    };
    let [u] = args.as_slice() else {
        return e;
    };
    match f {
        Func::Tan => sin(u.clone()) / cos(u.clone()),
        Func::Sec => cos(u.clone()).recip(),
        Func::Csc => sin(u.clone()).recip(),
        Func::Cot => cos(u.clone()) / sin(u.clone()),
        Func::Tanh => func(Func::Sinh, vec![u.clone()]) / func(Func::Cosh, vec![u.clone()]),
        Func::Sin | Func::Cos => angle(*f == Func::Sin, u).unwrap_or(e),
        _ => e,
    }
}

/// `sin(a + b)`, `cos(a + b)`, `sin(n u)` and `cos(n u)` expanded; `None` when the angle is
/// neither a sum nor a small multiple.
fn angle(is_sin: bool, u: &Expr) -> Option<Expr> {
    match u.node() {
        ExprNode::Add(terms) if terms.len() >= 2 => {
            let a = terms[0].clone();
            let b = add(terms[1..].to_vec());
            let (sa, ca) = (expand_sin(&a), expand_cos(&a));
            let (sb, cb) = (expand_sin(&b), expand_cos(&b));
            Some(if is_sin {
                sa * cb + ca * sb
            } else {
                ca * cb - sa * sb
            })
        }
        ExprNode::Mul(fs) => {
            let n = fs.first()?.as_number()?.as_i64()?;
            if !(2..=MAX_MULTIPLE).contains(&n) {
                return None;
            }
            let rest = mul(fs[1..].to_vec());
            let smaller = mul(vec![Expr::int(n - 1), rest.clone()]);
            let (s1, c1) = (sin(rest.clone()), cos(rest));
            let (sn, cn) = (expand_sin(&smaller), expand_cos(&smaller));
            Some(if is_sin {
                s1 * cn + c1 * sn
            } else {
                c1 * cn - s1 * sn
            })
        }
        _ => None,
    }
}

fn expand_sin(u: &Expr) -> Expr {
    angle(true, u).unwrap_or_else(|| sin(u.clone()))
}

fn expand_cos(u: &Expr) -> Expr {
    angle(false, u).unwrap_or_else(|| cos(u.clone()))
}

/// `cos^n u = cos^(n mod 2) u · (1 − sin²u)^(n div 2)`, and `cosh²u = 1 + sinh²u`.
fn pythagorean(e: &Expr) -> Expr {
    let e = Expr::new(e.map_children(&mut pythagorean));
    let ExprNode::Pow(base, k) = e.node() else {
        return e;
    };
    let Some(n) = k.as_number().and_then(|n| n.as_i64()) else {
        return e;
    };
    if !(2..=MAX_POWER).contains(&n) {
        return e;
    }
    let ExprNode::Func(f @ (Func::Cos | Func::Cosh), args) = base.node() else {
        return e;
    };
    let [u] = args.as_slice() else {
        return e;
    };
    let square = match f {
        Func::Cos => Expr::one() - sin(u.clone()).pow(2),
        _ => Expr::one() + func(Func::Sinh, vec![u.clone()]).pow(2),
    };
    let odd = if n % 2 == 1 {
        base.clone()
    } else {
        Expr::one()
    };
    odd * square.pow(Expr::int(n / 2))
}

/// Numerator and denominator, moving every negative integer power down.
fn fraction(e: &Expr) -> (Expr, Expr) {
    match e.node() {
        ExprNode::Add(ts) => {
            let mut num = Expr::zero();
            let mut den = Expr::one();
            for t in ts {
                let (n, d) = fraction(t);
                if d == den {
                    num = num + n;
                } else {
                    num = num * d.clone() + n * den.clone();
                    den = den * d;
                }
            }
            (num, den)
        }
        ExprNode::Mul(fs) => {
            let (mut nums, mut dens) = (Vec::new(), Vec::new());
            for g in fs {
                let (n, d) = fraction(g);
                nums.push(n);
                dens.push(d);
            }
            (mul(nums), mul(dens))
        }
        ExprNode::Pow(b, p) => match p.as_number().and_then(|n| n.as_i64()) {
            Some(k) if k < 0 => {
                let (n, d) = fraction(b);
                (d.pow(Expr::int(-k)), n.pow(Expr::int(-k)))
            }
            Some(k) if k > 0 => {
                let (n, d) = fraction(b);
                (n.pow(Expr::int(k)), d.pow(Expr::int(k)))
            }
            _ => (e.clone(), Expr::one()),
        },
        _ => (e.clone(), Expr::one()),
    }
}

/// True when `e` is identically zero by the trigonometric normal form (see the module
/// documentation); `false` means "not proved", not "false".
pub fn identity_zero(e: &Expr) -> bool {
    if e.size() > MAX_SIZE {
        return false;
    }
    let rewritten = rewrite(e).simplify();
    let (num, _) = fraction(&rewritten);
    let reduced = pythagorean(&num.simplify());
    match reduced.expand() {
        Ok(x) => {
            let x = pythagorean(&x).expand().map(|y| y.simplify());
            matches!(x, Ok(z) if z.is_zero())
        }
        Err(_) => false,
    }
}

/// `e` with `cos²u` written as `1 − sin²u` (and `cosh²u` as `1 + sinh²u`), expanded, when
/// that is shorter (`r sin²θ + r cos²θ` → `r`); otherwise `e` itself.
pub fn tidy(e: &Expr) -> Expr {
    if !has_trig(e) || e.size() > MAX_SIZE {
        return e.clone();
    }
    let replaced = pythagorean(e);
    if replaced == *e {
        return e.clone();
    }
    let Ok(reduced) = replaced.expand() else {
        return e.clone();
    };
    let reduced = reduced.simplify();
    if reduced.size() < e.size() {
        reduced
    } else {
        e.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x() -> Expr {
        Expr::sym("x")
    }

    #[test]
    fn identities() {
        let (s, c) = (sin(x()), cos(x()));
        assert!(identity_zero(
            &(s.clone().pow(2) + c.clone().pow(2) - Expr::one())
        ));
        let two_x = Expr::int(2) * x();
        assert!(identity_zero(
            &(sin(two_x.clone()) - Expr::int(2) * s.clone() * c.clone())
        ));
        assert!(identity_zero(
            &(cos(two_x.clone()) - (Expr::one() - Expr::int(2) * s.clone().pow(2)))
        ));
        let tan = func(Func::Tan, vec![x()]);
        let sec = func(Func::Sec, vec![x()]);
        assert!(identity_zero(&(tan.pow(2) + Expr::one() - sec.pow(2))));
        let (sh, ch) = (func(Func::Sinh, vec![x()]), func(Func::Cosh, vec![x()]));
        assert!(identity_zero(&(ch.pow(2) - sh.pow(2) - Expr::one())));
        // Not an identity.
        assert!(!identity_zero(&(s.pow(2) - c.pow(2))));
        let y = Expr::sym("y");
        assert!(identity_zero(
            &(sin(x() + y.clone()) - (sin(x()) * cos(y.clone()) + cos(x()) * sin(y)))
        ));
    }

    #[test]
    fn tidy_keeps_the_shorter_form() {
        let r = Expr::sym("r");
        let t = Expr::sym("θ");
        let e = r.clone() * sin(t.clone()).pow(2) + r.clone() * cos(t.clone()).pow(2);
        assert_eq!(tidy(&e), r);
        let c2 = cos(t).pow(2);
        assert_eq!(tidy(&c2), c2);
    }
}
