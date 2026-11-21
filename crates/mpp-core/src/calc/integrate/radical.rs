//! Quadratics with symbolic coefficients and square roots of quadratics: the forms
//! `1/(a² ± x²)`, `1/√(a² − x²)`, `√(a² − x²)`, `√(x² ± a²)` and `(x² + a²)^(−3/2)`, after
//! completing the square. These are the trigonometric substitutions `x = a sin θ`,
//! `x = a tan θ` and `x = a sec θ`, written out as their standard results.

use std::cmp::Ordering;

use crate::eval::EmptyEnv;
use crate::expr::{Expr, ExprNode};
use crate::functions::{func, Func};
use crate::number::Number;
use crate::poly::as_poly;
use crate::simplify::{add, mul, pow, simplify};
use crate::steps::Steps;

use super::engine::{div, Integrator};
use super::holder;

/// The sign of a quantity free of the integration variable, when it is clear
/// (squares and square roots count as positive, as in the textbook forms `a² ± x²`).
pub(crate) fn sign_of(e: &Expr) -> Option<Ordering> {
    if e.is_constant() {
        let v = e.eval_f64(&EmptyEnv).ok()?;
        return if v > 0.0 {
            Some(Ordering::Greater)
        } else if v < 0.0 {
            Some(Ordering::Less)
        } else {
            Some(Ordering::Equal)
        };
    }
    match e.node() {
        ExprNode::Pow(b, x) => {
            let r = x.as_number().and_then(Number::as_rational)?;
            let even = r.is_integer() && (r.to_integer() % 2u8) == 0.into();
            let root = !r.is_integer() && r.denom() % 2u8 == 0.into();
            if even || root {
                Some(Ordering::Greater)
            } else {
                let s = sign_of(b)?;
                if s == Ordering::Greater || r.is_integer() {
                    // odd integer powers keep the sign
                    Some(s)
                } else {
                    None
                }
            }
        }
        ExprNode::Mul(fs) => fs.iter().try_fold(Ordering::Greater, |acc, f| {
            let s = sign_of(f)?;
            Some(match (acc, s) {
                (_, Ordering::Equal) | (Ordering::Equal, _) => Ordering::Equal,
                (a, b) if a == b => Ordering::Greater,
                _ => Ordering::Less,
            })
        }),
        ExprNode::Add(ts) => {
            let signs: Option<Vec<Ordering>> = ts.iter().map(sign_of).collect();
            let signs = signs?;
            if signs.iter().all(|s| *s == Ordering::Greater) {
                Some(Ordering::Greater)
            } else if signs.iter().all(|s| *s == Ordering::Less) {
                Some(Ordering::Less)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// `(k, base, s)` with `e = k · base^s`, `k` free of `var`.
fn split_power(e: &Expr, var: &str) -> Option<(Expr, Expr, Expr)> {
    let factors: Vec<Expr> = match e.node() {
        ExprNode::Mul(fs) => fs.clone(),
        _ => vec![e.clone()],
    };
    let (consts, rest): (Vec<Expr>, Vec<Expr>) =
        factors.into_iter().partition(|f| !f.contains_sym(var));
    let [single] = <[Expr; 1]>::try_from(rest).ok()?;
    let ExprNode::Pow(b, s) = single.node() else {
        return None;
    };
    Some((mul(consts), b.clone(), s.clone()))
}

fn neg(e: Expr) -> Expr {
    mul(vec![Expr::int(-1), e])
}

/// Integrates `k/(p x² + r)` and `k·Q^s` (`s = ±1/2, −3/2`, `Q` quadratic).
pub(crate) fn quadratic_forms(eng: &mut Integrator, e: &Expr, steps: &mut Steps) -> Option<Expr> {
    let var = eng.var.clone();
    let (k, base, s) = split_power(e, &var)?;
    let c = as_poly(&base, &var)?;
    if c.len() != 3 {
        return None;
    }
    let (r, qq, p) = (simplify(&c[0]), simplify(&c[1]), simplify(&c[2]));
    let sv = s.as_number()?.as_rational()?.clone();
    let (num, den) = (sv.numer().clone(), sv.denom().clone());
    let x = Expr::sym(&var);
    let (result, rule, note) = if num == (-1).into() && den == 1.into() {
        if !qq.is_zero() {
            return None;
        }
        let (sp, sr) = (sign_of(&p)?, sign_of(&r)?);
        let (ap, ar) = (
            if sp == Ordering::Less {
                neg(p.clone())
            } else {
                p.clone()
            },
            if sr == Ordering::Less {
                neg(r.clone())
            } else {
                r.clone()
            },
        );
        let (a, b) = (square_root(&ap), square_root(&ar));
        let ln_ratio =
            |top: Expr, bottom: Expr| func(Func::Ln, vec![func(Func::Abs, vec![div(top, bottom)])]);
        let ax = mul(vec![a.clone(), x.clone()]);
        match (sp, sr) {
            (Ordering::Greater, Ordering::Greater) => (
                div(
                    func(Func::Atan, vec![div(ax, b.clone())]),
                    mul(vec![a, b.clone()]),
                ),
                "integral of 1/(a² + x²)",
                format!(
                    "∫ dx/(x² + a²) = (1/a)·arctan(x/a), here a = {}",
                    simplify(&div(b, square_root(&ap)))
                ),
            ),
            (Ordering::Greater, Ordering::Less) => (
                div(
                    ln_ratio(
                        add(vec![ax.clone(), neg(b.clone())]),
                        add(vec![ax, b.clone()]),
                    ),
                    mul(vec![Expr::int(2), a, b.clone()]),
                ),
                "integral of 1/(x² − a²)",
                format!(
                    "∫ dx/(x² − a²) = (1/(2a))·ln|(x − a)/(x + a)|, here a = {}",
                    simplify(&div(b, square_root(&ap)))
                ),
            ),
            (Ordering::Less, Ordering::Greater) => (
                div(
                    ln_ratio(
                        add(vec![ax.clone(), b.clone()]),
                        add(vec![ax, neg(b.clone())]),
                    ),
                    mul(vec![Expr::int(2), a, b.clone()]),
                ),
                "integral of 1/(a² − x²)",
                format!(
                    "∫ dx/(a² − x²) = (1/(2a))·ln|(x + a)/(x − a)|, here a = {}",
                    simplify(&div(b, square_root(&ap)))
                ),
            ),
            _ => return None,
        }
    } else if den == 2.into() && (num == 1.into() || num == (-1).into() || num == (-3).into()) {
        let sp = sign_of(&p)?;
        if sp == Ordering::Equal {
            return None;
        }
        // Q = p((x + q/(2p))² + m)
        let shift = simplify(&div(qq.clone(), mul(vec![Expr::int(2), p.clone()])));
        let t = simplify(&add(vec![x.clone(), shift]));
        let m = simplify(&add(vec![
            div(r.clone(), p.clone()),
            neg(div(
                pow(qq.clone(), Expr::int(2)),
                mul(vec![Expr::int(4), pow(p.clone(), Expr::int(2))]),
            )),
        ]));
        let sm = sign_of(&m)?;
        if sm == Ordering::Equal || (sp == Ordering::Less && sm == Ordering::Greater) {
            return None;
        }
        let sqrt_q = base.sqrt();
        let completed = if qq.is_zero() {
            String::new()
        } else {
            format!("complete the square: {base} = {p}·(({t})² + {m}); ")
        };
        if num == (-3).into() {
            let res = div(t.clone(), mul(vec![p.clone(), m.clone(), sqrt_q]));
            let sub = if sp == Ordering::Less {
                "sin"
            } else if sm == Ordering::Greater {
                "tan"
            } else {
                "sec"
            };
            (
                res,
                "trigonometric substitution",
                format!(
                    "{completed}substitute {t} = a·{sub} θ with a = √{}",
                    simplify(&abs_expr(&m))
                ),
            )
        } else if sp == Ordering::Less {
            // M = −m > 0: x = a sin θ
            let big_m = simplify(&neg(m.clone()));
            let a = big_m.sqrt();
            let asin = func(Func::Asin, vec![div(t.clone(), a.clone())]);
            let ap = neg(p.clone()).sqrt();
            let res = if num == (-1).into() {
                div(asin, ap)
            } else {
                add(vec![
                    mul(vec![Expr::rational(1, 2), t.clone(), sqrt_q]),
                    mul(vec![Expr::rational(1, 2), ap, big_m, asin]),
                ])
            };
            (
                res,
                "trigonometric substitution",
                format!(
                    "{completed}substitute {t} = a·sin θ with a = {a}, d{var} = a·cos θ dθ, √(a² − ({t})²) = a·cos θ"
                ),
            )
        } else {
            let sp_root = p.sqrt();
            let inner = add(vec![t.clone(), div(sqrt_q.clone(), sp_root.clone())]);
            let log = if sm == Ordering::Greater {
                func(Func::Ln, vec![inner])
            } else {
                func(Func::Ln, vec![func(Func::Abs, vec![inner])])
            };
            let res = if num == (-1).into() {
                div(log, sp_root)
            } else {
                add(vec![
                    mul(vec![Expr::rational(1, 2), t.clone(), sqrt_q]),
                    mul(vec![Expr::rational(1, 2), sp_root, m.clone(), log]),
                ])
            };
            let a = simplify(&abs_expr(&m)).sqrt();
            let sub = if sm == Ordering::Greater {
                format!(
                    "{t} = a·tan θ with a = {a}, d{var} = a·sec² θ dθ, √(({t})² + a²) = a·sec θ"
                )
            } else {
                format!("{t} = a·sec θ with a = {a}, d{var} = a·sec θ·tan θ dθ, √(({t})² − a²) = a·tan θ")
            };
            (
                res,
                "trigonometric substitution",
                format!("{completed}substitute {sub}"),
            )
        }
    } else {
        return None;
    };
    let result = simplify(&mul(vec![k, result]));
    steps.push(rule, holder(e, &var), result.clone(), Some(note));
    Some(result)
}

fn abs_expr(m: &Expr) -> Expr {
    if sign_of(m) == Some(Ordering::Less) {
        neg(m.clone())
    } else {
        m.clone()
    }
}

/// `√e`, written as `b` when `e = b²` (both signs of `b` give the same results in the
/// `1/(p x² + r)` formulas, which only change sign together with `b`).
fn square_root(e: &Expr) -> Expr {
    if let ExprNode::Pow(b, x) = e.node() {
        if x.as_number().and_then(|n| n.as_i64()) == Some(2) {
            return b.clone();
        }
    }
    e.sqrt()
}
