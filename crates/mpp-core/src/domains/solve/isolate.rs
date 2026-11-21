//! Solving by isolating the unknown: when it occurs once, peel the expression around it
//! layer by layer (subtract, divide, take roots, apply inverse functions). Periodic
//! inverses introduce an integer parameter (`x = π/6 + 2πk`).

use crate::domains::util::{is_zero, numeric};
use crate::error::MathResult;
use crate::expr::{Constant, Expr, ExprNode};
use crate::functions::Func;
use crate::number::Number;
use crate::simplify::{add, mul};

/// Most branches isolation may open (each ± or periodic inverse doubles them).
const MAX_BRANCHES: usize = 64;

/// What isolation found.
#[derive(Clone, Debug, PartialEq)]
pub struct Isolated {
    /// Candidate values (may contain the integer parameter).
    pub values: Vec<Expr>,
    /// True when a periodic inverse introduced the parameter.
    pub periodic: bool,
}

/// How often the symbol `var` occurs in `e`.
pub fn occurrences(e: &Expr, var: &str) -> usize {
    match e.node() {
        ExprNode::Sym(s) => usize::from(&**s == var),
        _ => e.children().iter().map(|c| occurrences(c, var)).sum(),
    }
}

/// Solves `lhs = rhs` for `var` when `var` occurs exactly once in `lhs` and not in `rhs`.
/// `Ok(None)` means isolation does not apply.
pub fn isolate(lhs: &Expr, rhs: &Expr, var: &str, param: &str) -> MathResult<Option<Isolated>> {
    if occurrences(lhs, var) != 1 || rhs.contains_sym(var) {
        return Ok(None);
    }
    let mut work = vec![(lhs.clone(), rhs.clone())];
    let mut done = Vec::new();
    let mut periodic = false;
    let k = Expr::sym(param);
    while let Some((l, r)) = work.pop() {
        if work.len() + done.len() > MAX_BRANCHES {
            return Ok(None);
        }
        if l.is_sym(var) {
            done.push(r.simplify());
            continue;
        }
        let Some(next) = peel(&l, &r, var, &k, &mut periodic)? else {
            return Ok(None);
        };
        work.extend(next);
    }
    done.reverse();
    Ok(Some(Isolated {
        values: done,
        periodic,
    }))
}

/// A real constant outside `[lo, hi]` (so `asin` and friends have no real value).
fn outside(r: &Expr, lo: f64, hi: f64) -> bool {
    numeric(r).is_some_and(|v| v < lo - 1e-12 || v > hi + 1e-12)
}

fn f1(f: Func, a: Expr) -> MathResult<Expr> {
    Expr::func(f, vec![a])
}

/// One layer: `Some(branches)` (possibly empty when there is no real solution) or `None`.
fn peel(
    l: &Expr,
    r: &Expr,
    var: &str,
    k: &Expr,
    periodic: &mut bool,
) -> MathResult<Option<Vec<(Expr, Expr)>>> {
    let two_pi_k = Expr::int(2) * Expr::pi() * k.clone();
    Ok(Some(match l.node() {
        ExprNode::Add(ts) => {
            let (with, without): (Vec<Expr>, Vec<Expr>) =
                ts.iter().cloned().partition(|t| t.contains_sym(var));
            let [inner] = with.as_slice() else {
                return Ok(None);
            };
            vec![(inner.clone(), (r - &add(without)).simplify())]
        }
        ExprNode::Mul(fs) => {
            let (with, without): (Vec<Expr>, Vec<Expr>) =
                fs.iter().cloned().partition(|t| t.contains_sym(var));
            let [inner] = with.as_slice() else {
                return Ok(None);
            };
            let c = mul(without);
            if is_zero(&c) {
                return Ok(None);
            }
            vec![(inner.clone(), (r / &c).simplify())]
        }
        ExprNode::Pow(b, p) if b.contains_sym(var) => {
            let Some(pn) = p.as_number().and_then(Number::as_rational).cloned() else {
                // b^p = r with a symbolic exponent: b = r^(1/p).
                return Ok(Some(vec![(b.clone(), r.pow(p.recip()).simplify())]));
            };
            if pn.numer().sign() == num_bigint::Sign::NoSign {
                return Ok(None);
            }
            let inv = Expr::ratio(pn.recip());
            let even_num = pn.numer() % 2u32 == num_bigint::BigInt::from(0);
            if even_num {
                // x^(2m/n) = r: x = ± r^(n/2m), real only when r ≥ 0.
                if numeric(r).is_some_and(|v| v < 0.0) {
                    return Ok(Some(Vec::new()));
                }
                let root = r.pow(inv).simplify();
                if root.is_zero() {
                    vec![(b.clone(), root)]
                } else {
                    vec![(b.clone(), (-&root).simplify()), (b.clone(), root)]
                }
            } else if pn.denom() % 2u32 == num_bigint::BigInt::from(0)
                && numeric(r).is_some_and(|v| v < 0.0)
            {
                // An even root is never negative.
                Vec::new()
            } else {
                let value = match numeric(r) {
                    Some(v) if v < 0.0 => -(-r).pow(inv),
                    _ => r.pow(inv),
                };
                vec![(b.clone(), value.simplify())]
            }
        }
        ExprNode::Pow(b, p) => {
            // b^p = r with the unknown in the exponent: p = ln r / ln b.
            if numeric(r).is_some_and(|v| v <= 0.0) && numeric(b).is_some_and(|v| v > 0.0) {
                return Ok(Some(Vec::new()));
            }
            let ln_r = f1(Func::Ln, r.clone())?;
            let value = if matches!(b.node(), ExprNode::Const(Constant::E)) {
                ln_r
            } else {
                ln_r / f1(Func::Ln, b.clone())?
            };
            vec![(p.clone(), value.simplify())]
        }
        ExprNode::Func(f, args) => {
            let u = match args.as_slice() {
                [u] => u.clone(),
                [u, base] if *f == Func::Log && !base.contains_sym(var) => {
                    return Ok(Some(vec![(u.clone(), base.pow(r.clone()).simplify())]));
                }
                _ => return Ok(None),
            };
            match f {
                Func::Sin | Func::Cos if outside(r, -1.0, 1.0) => Vec::new(),
                Func::Sin => {
                    *periodic = true;
                    let a = f1(Func::Asin, r.clone())?;
                    vec![
                        (u.clone(), (&a + &two_pi_k).simplify()),
                        (u, (Expr::pi() - a + two_pi_k).simplify()),
                    ]
                }
                Func::Cos => {
                    *periodic = true;
                    let a = f1(Func::Acos, r.clone())?;
                    if is_zero(&a) || is_zero(&(&a - &Expr::pi())) {
                        vec![(u, (a + two_pi_k).simplify())]
                    } else {
                        vec![
                            (u.clone(), (-&a + two_pi_k.clone()).simplify()),
                            (u, (a + two_pi_k).simplify()),
                        ]
                    }
                }
                Func::Tan => {
                    *periodic = true;
                    let a = f1(Func::Atan, r.clone())?;
                    vec![(u, (a + Expr::pi() * k.clone()).simplify())]
                }
                Func::Cot => {
                    *periodic = true;
                    let a = f1(Func::Atan, r.recip())?;
                    vec![(u, (a + Expr::pi() * k.clone()).simplify())]
                }
                Func::Sec | Func::Csc => {
                    let g = if *f == Func::Sec {
                        Func::Cos
                    } else {
                        Func::Sin
                    };
                    vec![(f1(g, u)?, r.recip().simplify())]
                }
                Func::Asin | Func::Acos | Func::Atan => {
                    let (g, lo, hi) = match f {
                        Func::Asin => (
                            Func::Sin,
                            -std::f64::consts::FRAC_PI_2,
                            std::f64::consts::FRAC_PI_2,
                        ),
                        Func::Acos => (Func::Cos, 0.0, std::f64::consts::PI),
                        _ => (
                            Func::Tan,
                            -std::f64::consts::FRAC_PI_2,
                            std::f64::consts::FRAC_PI_2,
                        ),
                    };
                    if outside(r, lo, hi) {
                        Vec::new()
                    } else {
                        vec![(u, f1(g, r.clone())?)]
                    }
                }
                Func::Sinh => vec![(u, f1(Func::Asinh, r.clone())?)],
                Func::Tanh if outside(r, -1.0, 1.0) => Vec::new(),
                Func::Tanh => vec![(u, f1(Func::Atanh, r.clone())?)],
                Func::Cosh if numeric(r).is_some_and(|v| v < 1.0) => Vec::new(),
                Func::Cosh => {
                    let a = f1(Func::Acosh, r.clone())?;
                    if a.is_zero() {
                        vec![(u, a)]
                    } else {
                        vec![(u.clone(), -&a), (u, a)]
                    }
                }
                Func::Asinh => vec![(u, f1(Func::Sinh, r.clone())?)],
                Func::Acosh => vec![(u, f1(Func::Cosh, r.clone())?)],
                Func::Atanh => vec![(u, f1(Func::Tanh, r.clone())?)],
                Func::Ln => vec![(u, Expr::e().pow(r.clone()))],
                Func::Abs if numeric(r).is_some_and(|v| v < 0.0) => Vec::new(),
                Func::Abs => {
                    if r.is_zero() {
                        vec![(u, r.clone())]
                    } else {
                        vec![(u.clone(), (-r).simplify()), (u, r.clone())]
                    }
                }
                _ => return Ok(None),
            }
        }
        _ => return Ok(None),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x() -> Expr {
        Expr::sym("x")
    }

    #[test]
    fn peels_linear_layers() {
        let lhs = Expr::int(3) * x() + Expr::int(2);
        let r = isolate(&lhs, &Expr::int(11), "x", "k").unwrap().unwrap();
        assert_eq!(r.values, vec![Expr::int(3)]);
        assert!(!r.periodic);
    }

    #[test]
    fn even_powers_give_two_branches() {
        let r = isolate(&x().pow(2), &Expr::int(9), "x", "k")
            .unwrap()
            .unwrap();
        assert_eq!(r.values, vec![Expr::int(-3), Expr::int(3)]);
        let r = isolate(&x().pow(2), &Expr::int(-9), "x", "k")
            .unwrap()
            .unwrap();
        assert!(r.values.is_empty());
        let r = isolate(&x().pow(3), &Expr::int(-8), "x", "k")
            .unwrap()
            .unwrap();
        assert_eq!(r.values, vec![Expr::int(-2)]);
    }

    #[test]
    fn periodic_inverses() {
        let sin = Expr::call("sin", vec![x()]).unwrap();
        let r = isolate(&sin, &Expr::rational(1, 2), "x", "k")
            .unwrap()
            .unwrap();
        assert!(r.periodic);
        assert_eq!(r.values.len(), 2);
        let k0: Vec<f64> = r
            .values
            .iter()
            .map(|v| v.eval_f64(&[("k", 0.0)]).unwrap())
            .collect();
        assert!((k0[0] - std::f64::consts::PI / 6.0).abs() < 1e-12);
        assert!((k0[1] - 5.0 * std::f64::consts::PI / 6.0).abs() < 1e-12);
        let r = isolate(&sin, &Expr::int(2), "x", "k").unwrap().unwrap();
        assert!(r.values.is_empty());
    }

    #[test]
    fn exponentials_and_logs() {
        let e = Expr::e().pow(Expr::int(2) * x());
        let r = isolate(&e, &Expr::int(5), "x", "k").unwrap().unwrap();
        assert!(
            (r.values[0].eval_f64(&crate::eval::EmptyEnv).unwrap() - 5f64.ln() / 2.0).abs() < 1e-12
        );
        let ln = Expr::call("ln", vec![x() - Expr::one()]).unwrap();
        let r = isolate(&ln, &Expr::int(0), "x", "k").unwrap().unwrap();
        assert_eq!(r.values, vec![Expr::int(2)]);
    }

    #[test]
    fn needs_a_single_occurrence() {
        let lhs = x().pow(2) + x();
        assert_eq!(isolate(&lhs, &Expr::one(), "x", "k").unwrap(), None);
    }
}
