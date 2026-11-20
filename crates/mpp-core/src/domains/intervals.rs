//! Sets of real numbers described by conditions: the natural domain of an expression,
//! inequalities solved to intervals (`x² − 4 < 0` → `(−2, 2)`), and the range of a
//! function on an interval. These are the building blocks the function-analysis
//! helpers use for "the domain" and "the range".
//!
//! Every answer is exact or an error: an inequality is solved by finding all the points
//! where its sides can change sign (the real zeros, and the ends of the domain), and
//! testing one point between each pair. When the zeros can't all be found (a periodic
//! equation over the whole line, or one only solvable numerically outside polynomials)
//! the answer is an error rather than a guess.

use std::cmp::Ordering;

use crate::domains::sets::{Piece, RealSet};
use crate::domains::solve::{self, poly};
use crate::domains::{util, Ctx};
use crate::error::{MathError, MathResult};
use crate::expr::{Constant, Expr, ExprNode, LimitDir, LogicOp, RelOp};
use crate::functions::Func;

/// Deepest nesting of domain conditions followed.
const MAX_DEPTH: usize = 16;

fn unsupported(what: impl Into<String>) -> MathError {
    MathError::Unsupported(what.into())
}

/// The real numbers where `f` (as a function of `var`) is defined: denominators are
/// non-zero, even roots and real powers have non-negative bases, logarithms positive
/// arguments, `asin`/`acos` arguments in [−1, 1].
pub fn natural_domain(f: &Expr, var: &str) -> MathResult<RealSet> {
    domain_at(&f.simplify(), var, 0)
}

fn domain_at(f: &Expr, var: &str, depth: usize) -> MathResult<RealSet> {
    if depth > MAX_DEPTH {
        return Err(MathError::TooLarge("the conditions of this domain".into()));
    }
    let mut conditions: Vec<(Expr, RelOp)> = Vec::new();
    collect_conditions(f, var, &mut conditions)?;
    let mut set = RealSet::reals();
    for (g, op) in conditions {
        let part = inequality_at(&g, op, var, depth + 1)?;
        set = set.intersection(&part)?;
    }
    Ok(set)
}

/// Conditions `g op 0` that make `e` defined.
fn collect_conditions(e: &Expr, var: &str, out: &mut Vec<(Expr, RelOp)>) -> MathResult<()> {
    if !e.contains_sym(var) {
        return Ok(());
    }
    match e.node() {
        ExprNode::Pow(b, k) => {
            if k.contains_sym(var) {
                if b.contains_sym(var) {
                    out.push((b.clone(), RelOp::Gt));
                }
            } else if let Some(n) = k.as_number() {
                if let Some(q) = n.as_rational() {
                    let negative = n.sign() == Some(Ordering::Less);
                    let even_root = (q.denom() % 2u32) == 0u32.into();
                    if even_root {
                        out.push((b.clone(), if negative { RelOp::Gt } else { RelOp::Ge }));
                    } else if *q.denom() != num_bigint::BigInt::from(1) {
                        // An odd root of a negative number is complex in the core.
                        out.push((b.clone(), if negative { RelOp::Gt } else { RelOp::Ge }));
                    } else if negative {
                        out.push((b.clone(), RelOp::Ne));
                    }
                } else {
                    // A decimal power: real only for a positive base.
                    out.push((b.clone(), RelOp::Gt));
                }
            } else {
                return Err(unsupported(format!(
                    "the domain of a power with the symbolic exponent {k}"
                )));
            }
        }
        ExprNode::Func(f, args) => match (f, args.as_slice()) {
            (Func::Ln, [x]) => out.push((x.clone(), RelOp::Gt)),
            (Func::Log, [x, base]) => {
                out.push((x.clone(), RelOp::Gt));
                if base.contains_sym(var) {
                    return Err(unsupported(
                        "the domain of a logarithm with a variable base",
                    ));
                }
            }
            (Func::Asin | Func::Acos, [x]) => {
                out.push(((x.clone() + Expr::one()).simplify(), RelOp::Ge));
                out.push(((Expr::one() - x.clone()).simplify(), RelOp::Ge));
            }
            (Func::Acosh, [x]) => out.push(((x.clone() - Expr::one()).simplify(), RelOp::Ge)),
            (Func::Atanh, [x]) => {
                out.push(((x.clone() + Expr::one()).simplify(), RelOp::Gt));
                out.push(((Expr::one() - x.clone()).simplify(), RelOp::Gt));
            }
            (
                Func::Sin
                | Func::Cos
                | Func::Atan
                | Func::Sinh
                | Func::Cosh
                | Func::Tanh
                | Func::Asinh
                | Func::Abs
                | Func::Sign
                | Func::Floor
                | Func::Ceil
                | Func::Min
                | Func::Max,
                _,
            ) => {}
            _ => {
                return Err(unsupported(format!(
                    "the domain of {} (it has infinitely many gaps, or none that can be listed)",
                    f.name()
                )))
            }
        },
        ExprNode::Sym(_) | ExprNode::Add(_) | ExprNode::Mul(_) => {}
        _ => {
            return Err(unsupported(format!("the domain of {e}")));
        }
    }
    for c in e.children() {
        collect_conditions(c, var, out)?;
    }
    Ok(())
}

/// The real `var` for which `cond` holds: a relation (`x^2 - 4 < 0`, `x \ne 1`), a chain
/// (`0 < x \le 1`), or `and` / `or` / `not` of these.
pub fn solve_inequality(cond: &Expr, var: &str) -> MathResult<RealSet> {
    condition_at(cond, var, 0)
}

fn condition_at(cond: &Expr, var: &str, depth: usize) -> MathResult<RealSet> {
    match cond.node() {
        ExprNode::Bool(true) => Ok(RealSet::reals()),
        ExprNode::Bool(false) => Ok(RealSet::empty()),
        ExprNode::Relation(op, a, b) => inequality_at(&(a - b).simplify(), *op, var, depth),
        ExprNode::Logic(LogicOp::And, parts) => {
            let mut set = RealSet::reals();
            for p in parts {
                set = set.intersection(&condition_at(p, var, depth)?)?;
            }
            Ok(set)
        }
        ExprNode::Logic(LogicOp::Or, parts) => {
            let mut set = RealSet::empty();
            for p in parts {
                set = set.union(&condition_at(p, var, depth)?)?;
            }
            Ok(set)
        }
        ExprNode::Logic(LogicOp::Not, parts) => match parts.as_slice() {
            [p] => condition_at(p, var, depth)?.complement(),
            _ => Err(unsupported(format!("the condition {cond}"))),
        },
        ExprNode::Apply(name, args) if &**name == "in" => match args.as_slice() {
            [x, set] if x.is_sym(var) => RealSet::from_expr(set),
            _ => Err(unsupported(format!("the condition {cond}"))),
        },
        _ => Err(unsupported(format!("{cond} as a condition on {var}"))),
    }
}

/// `{var : f op 0}`.
fn inequality_at(f: &Expr, op: RelOp, var: &str, depth: usize) -> MathResult<RealSet> {
    if depth > MAX_DEPTH {
        return Err(MathError::TooLarge(
            "the conditions of this inequality".into(),
        ));
    }
    if !f.contains_sym(var) {
        if !f.is_constant() {
            return Err(unsupported(format!(
                "an inequality whose truth depends on {} (give it a value or an assumption)",
                f.free_symbols()
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        let sign = util::sign(f).ok_or_else(|| unsupported(format!("the sign of {f}")))?;
        return Ok(if holds(op, sign) {
            RealSet::reals()
        } else {
            RealSet::empty()
        });
    }
    let others: Vec<String> = f
        .free_symbols()
        .iter()
        .filter(|s| ***s != *var)
        .map(|s| s.to_string())
        .collect();
    if !others.is_empty() {
        return Err(unsupported(format!(
            "an inequality with the parameters {} (give them values or assumptions)",
            others.join(", ")
        )));
    }
    let domain = domain_at(f, var, depth + 1)?;
    let zeros = real_zeros(f, var, &domain)?;
    let mut pieces: Vec<Piece> = Vec::new();
    for part in domain.pieces() {
        // The zeros inside this part split it into open intervals of constant sign.
        let mut cuts: Vec<Expr> = zeros
            .iter()
            .filter(|z| strictly_inside(z, part))
            .cloned()
            .collect();
        cuts.sort_by(|a, b| util::compare(a, b).unwrap_or(Ordering::Equal));
        let mut ends = vec![part.lo.clone()];
        ends.extend(cuts.iter().cloned());
        ends.push(part.hi.clone());
        for w in ends.windows(2) {
            let (lo, hi) = (&w[0], &w[1]);
            if util::compare(lo, hi) != Some(Ordering::Less) {
                continue;
            }
            let s = sign_between(f, var, lo, hi)?;
            if holds(op, s) {
                pieces.push(Piece {
                    lo: lo.clone(),
                    hi: hi.clone(),
                    lo_closed: false,
                    hi_closed: false,
                });
            }
        }
        // The cuts and closed ends of the part are points to test on their own.
        let mut points = cuts;
        if part.lo_closed && !part.lo.is_infinite() {
            points.push(part.lo.clone());
        }
        if part.hi_closed && !part.hi.is_infinite() {
            points.push(part.hi.clone());
        }
        for p in points {
            let s = sign_at(f, var, &p, &zeros)?;
            if holds(op, s) {
                pieces.push(Piece {
                    lo: p.clone(),
                    hi: p,
                    lo_closed: true,
                    hi_closed: true,
                });
            }
        }
    }
    RealSet::from_pieces(pieces)
}

fn holds(op: RelOp, s: Ordering) -> bool {
    match op {
        RelOp::Eq | RelOp::Approx => s == Ordering::Equal,
        RelOp::Ne => s != Ordering::Equal,
        RelOp::Lt => s == Ordering::Less,
        RelOp::Le => s != Ordering::Greater,
        RelOp::Gt => s == Ordering::Greater,
        RelOp::Ge => s != Ordering::Less,
    }
}

fn strictly_inside(z: &Expr, p: &Piece) -> bool {
    util::compare(z, &p.lo) == Some(Ordering::Greater)
        && util::compare(z, &p.hi) == Some(Ordering::Less)
}

/// Every real zero of `f` in `domain`. Zeros of a rational function come from its
/// numerator's polynomial roots (all of them); otherwise the exact solver must find a
/// finite list, or this is an error.
pub fn real_zeros(f: &Expr, var: &str, domain: &RealSet) -> MathResult<Vec<Expr>> {
    let num = numerator(f, var);
    let candidates: Vec<Expr> =
        match num.as_poly(var) {
            Some(c) if c.len() >= 2 && c.iter().all(Expr::is_constant) => {
                poly::roots(&c)?.into_iter().map(|r| r.value).collect()
            }
            Some(c) if c.len() <= 1 => Vec::new(),
            _ => {
                let mut ctx = Ctx::new();
                match solve::solve_exact(&num, var, &mut ctx)? {
                    Some(solve::Solutions::Finite {
                        values,
                        exact: true,
                    }) => values,
                    Some(solve::Solutions::All) => {
                        return Err(unsupported(format!(
                            "an inequality where {f} is zero everywhere"
                        )))
                    }
                    Some(solve::Solutions::Periodic { .. }) => return Err(unsupported(
                        "an inequality with infinitely many boundary points over the whole line",
                    )),
                    _ => {
                        return Err(unsupported(format!(
                            "finding every point where {f} changes sign"
                        )))
                    }
                }
            }
        };
    let mut out: Vec<Expr> = Vec::new();
    for z in candidates {
        let z = z.simplify();
        if util::numeric(&z).is_none() {
            continue; // complex
        }
        if !domain.contains(&z)? {
            continue;
        }
        let at = f.subs(var, &z).simplify();
        let zero =
            util::is_zero(&at) || util::numeric(&at).is_some_and(|v| v.abs() <= 1e-9 * scale(&z));
        if zero && !out.iter().any(|o| util::equal(o, &z)) {
            out.push(z);
        }
    }
    Ok(out)
}

/// The part of `f` whose zeros are `f`'s zeros: a product without its factors with
/// negative powers (`−x/√(4 − x²)` gives `−x`), or the numerator of a sum.
fn numerator(f: &Expr, var: &str) -> Expr {
    match f.node() {
        ExprNode::Mul(factors) => {
            let kept: Vec<Expr> = factors
                .iter()
                .filter(|g| match g.node() {
                    ExprNode::Pow(_, k) => {
                        !(k.as_number()
                            .is_some_and(|n| n.sign() == Some(Ordering::Less)))
                    }
                    _ => true,
                })
                .cloned()
                .collect();
            crate::simplify::mul(kept)
        }
        ExprNode::Pow(_, k)
            if k.as_number()
                .is_some_and(|n| n.sign() == Some(Ordering::Less)) =>
        {
            Expr::one()
        }
        _ => solve::numer_denom(f, var).0,
    }
}

fn scale(z: &Expr) -> f64 {
    util::numeric(z).map_or(1.0, |v| v.abs().max(1.0))
}

/// The sign of `f` on the open interval `(lo, hi)`, where it has no zeros.
fn sign_between(f: &Expr, var: &str, lo: &Expr, hi: &Expr) -> MathResult<Ordering> {
    let sample = match (lo.is_infinite(), hi.is_infinite()) {
        (true, true) => Expr::zero(),
        (true, false) => (hi - &Expr::one()).simplify(),
        (false, true) => (lo + &Expr::one()).simplify(),
        (false, false) => ((lo + hi) / Expr::int(2)).simplify(),
    };
    sign_of_value(f, var, &sample)
}

/// The sign of `f` at the point `p` (zero when `p` is one of its zeros).
fn sign_at(f: &Expr, var: &str, p: &Expr, zeros: &[Expr]) -> MathResult<Ordering> {
    if zeros.iter().any(|z| util::equal(z, p)) {
        return Ok(Ordering::Equal);
    }
    sign_of_value(f, var, p)
}

fn sign_of_value(f: &Expr, var: &str, at: &Expr) -> MathResult<Ordering> {
    let v = f.subs(var, at).simplify();
    if let Some(n) = v.as_number().filter(|n| n.is_exact() && n.is_real()) {
        return n
            .sign()
            .ok_or_else(|| unsupported(format!("the sign of {v}")));
    }
    let x = util::numeric(&v).ok_or_else(|| unsupported(format!("the sign of {v}")))?;
    if x.abs() < 1e-12 {
        return Err(MathError::Inconclusive(format!("the sign of {v}")));
    }
    Ok(if x > 0.0 {
        Ordering::Greater
    } else {
        Ordering::Less
    })
}

/// The condition `var ∈ set` written out: `x < 1`, `0 ≤ x < 2`, `x = 3`, joined by `or`.
pub fn set_condition(var: &str, set: &RealSet) -> Expr {
    let x = Expr::sym(var);
    let parts: Vec<Expr> = set
        .pieces()
        .iter()
        .map(|p| {
            if p.lo_closed && p.hi_closed && p.lo == p.hi {
                return Expr::relation(RelOp::Eq, x.clone(), p.lo.clone());
            }
            let lower = (!p.lo.is_infinite()).then(|| {
                let op = if p.lo_closed { RelOp::Le } else { RelOp::Lt };
                Expr::relation(op, p.lo.clone(), x.clone())
            });
            let upper = (!p.hi.is_infinite()).then(|| {
                let op = if p.hi_closed { RelOp::Le } else { RelOp::Lt };
                Expr::relation(op, x.clone(), p.hi.clone())
            });
            match (lower, upper) {
                (None, None) => Expr::new(ExprNode::Bool(true)),
                (Some(l), None) => flip(&l),
                (None, Some(u)) => u,
                (Some(l), Some(u)) => Expr::new(ExprNode::Logic(LogicOp::And, vec![l, u])),
            }
        })
        .collect();
    match parts.len() {
        0 => Expr::new(ExprNode::Bool(false)),
        1 => parts
            .into_iter()
            .next()
            .unwrap_or_else(|| Expr::new(ExprNode::Bool(false))),
        _ => Expr::new(ExprNode::Logic(LogicOp::Or, parts)),
    }
}

/// `a < x` as `x > a`.
fn flip(rel: &Expr) -> Expr {
    match rel.node() {
        ExprNode::Relation(op, a, b) => {
            let op = match op {
                RelOp::Lt => RelOp::Gt,
                RelOp::Le => RelOp::Ge,
                other => *other,
            };
            Expr::relation(op, b.clone(), a.clone())
        }
        _ => rel.clone(),
    }
}

// ---- range -------------------------------------------------------------------------

/// The range of `f` on `on` (its natural domain when `None`), for an `f` that is
/// differentiable inside every piece of its domain: the image of each piece runs from
/// the least to the greatest of the values at critical points, closed ends and the
/// limits at open or infinite ends.
pub fn range_of(f: &Expr, var: &str, on: Option<&RealSet>) -> MathResult<RealSet> {
    let f = f.simplify();
    let mut domain = natural_domain(&f, var)?;
    if let Some(s) = on {
        domain = domain.intersection(s)?;
    }
    if !f.contains_sym(var) {
        return Ok(if domain.is_empty() {
            RealSet::empty()
        } else {
            RealSet::from_pieces(vec![point(f)])?
        });
    }
    let df = crate::calc::diff(&f, var, 1, None)?.simplify();
    let d_domain = natural_domain(&df, var)?;
    let mut out = RealSet::empty();
    for part in domain.pieces() {
        if part.lo == part.hi {
            out = out.union(&RealSet::from_pieces(vec![point(
                f.subs(var, &part.lo).simplify(),
            )])?)?;
            continue;
        }
        let interior = RealSet::from_pieces(vec![Piece {
            lo: part.lo.clone(),
            hi: part.hi.clone(),
            lo_closed: false,
            hi_closed: false,
        }])?;
        if !interior.is_subset(&d_domain)? {
            return Err(unsupported(
                "the range of a function with corners or cusps inside its domain",
            ));
        }
        // (value, attained)
        let mut candidates: Vec<(Expr, bool)> = Vec::new();
        if !util::is_zero(&df) {
            for z in real_zeros(&df, var, &interior)? {
                candidates.push((f.subs(var, &z).simplify(), true));
            }
        }
        candidates.push(end_value(
            &f,
            var,
            &part.lo,
            part.lo_closed,
            LimitDir::Right,
        )?);
        candidates.push(end_value(
            &f,
            var,
            &part.hi,
            part.hi_closed,
            LimitDir::Left,
        )?);
        let (lo, lo_closed) = extreme(&candidates, Ordering::Less)?;
        let (hi, hi_closed) = extreme(&candidates, Ordering::Greater)?;
        out = out.union(&RealSet::from_pieces(vec![Piece {
            lo_closed: lo_closed && !lo.is_infinite(),
            hi_closed: hi_closed && !hi.is_infinite(),
            lo,
            hi,
        }])?)?;
    }
    Ok(out)
}

fn point(v: Expr) -> Piece {
    Piece {
        lo: v.clone(),
        hi: v,
        lo_closed: true,
        hi_closed: true,
    }
}

/// The value at a closed end, or the one-sided limit at an open or infinite end.
fn end_value(
    f: &Expr,
    var: &str,
    at: &Expr,
    closed: bool,
    dir: LimitDir,
) -> MathResult<(Expr, bool)> {
    if closed && !at.is_infinite() {
        return Ok((f.subs(var, at).simplify(), true));
    }
    let dir = if at.is_infinite() {
        LimitDir::Both
    } else {
        dir
    };
    let v = crate::calc::limit(f, var, at, dir, None)?;
    if matches!(v.node(), ExprNode::Const(Constant::Undefined)) {
        return Err(unsupported("the range (a limit at an end does not exist)"));
    }
    Ok((v, false))
}

/// The least (`Less`) or greatest (`Greater`) candidate, and whether it is attained.
fn extreme(candidates: &[(Expr, bool)], want: Ordering) -> MathResult<(Expr, bool)> {
    let mut best: Option<(Expr, bool)> = None;
    for (v, attained) in candidates {
        best = Some(match best {
            None => (v.clone(), *attained),
            Some((b, a)) => match util::compare(v, &b) {
                Some(o) if o == want => (v.clone(), *attained),
                Some(Ordering::Equal) => (b, a || *attained),
                Some(_) => (b, a),
                None => return Err(unsupported(format!("comparing {v} and {b}"))),
            },
        });
    }
    best.ok_or_else(|| unsupported("the range of a function on an empty set"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x() -> Expr {
        Expr::sym("x")
    }

    fn rel(op: RelOp, a: Expr, b: Expr) -> Expr {
        Expr::relation(op, a, b)
    }

    fn show(s: &RealSet) -> String {
        s.to_expr().to_string()
    }

    #[test]
    fn inequalities_solve_to_intervals() {
        let s = |c: Expr| show(&solve_inequality(&c, "x").unwrap());
        assert_eq!(
            s(rel(RelOp::Lt, x().pow(2) - Expr::int(4), Expr::zero())),
            "(-2, 2)"
        );
        assert_eq!(
            s(rel(RelOp::Ge, x().pow(2), Expr::int(4))),
            "(-∞, -2] ∪ [2, ∞)"
        );
        assert_eq!(
            s(rel(RelOp::Gt, x().pow(2) + Expr::one(), Expr::zero())),
            "(-∞, ∞)"
        );
        assert_eq!(
            s(rel(RelOp::Lt, x().pow(2) + Expr::one(), Expr::zero())),
            "∅"
        );
        // (x − 1)/(x + 2) ≤ 0 on (−2, 1].
        let q = (x() - Expr::one()) / (x() + Expr::int(2));
        assert_eq!(s(rel(RelOp::Le, q, Expr::zero())), "(-2, 1]");
        // √x < 2 on [0, 4).
        assert_eq!(s(rel(RelOp::Lt, x().sqrt(), Expr::int(2))), "[0, 4)");
        // ln x > 0 on (1, ∞).
        let ln = Expr::func(Func::Ln, vec![x()]).unwrap();
        assert_eq!(s(rel(RelOp::Gt, ln, Expr::zero())), "(1, ∞)");
        // A double root doesn't change the sign: (x − 1)² > 0 everywhere but 1.
        let sq = (x() - Expr::one()).pow(2);
        assert_eq!(s(rel(RelOp::Gt, sq, Expr::zero())), "(-∞, 1) ∪ (1, ∞)");
        assert!(solve_inequality(
            &rel(
                RelOp::Gt,
                Expr::func(Func::Sin, vec![x()]).unwrap(),
                Expr::zero()
            ),
            "x"
        )
        .is_err());
    }

    #[test]
    fn natural_domains() {
        let d = |e: Expr| show(&natural_domain(&e, "x").unwrap());
        assert_eq!(d(x().recip()), "(-∞, 0) ∪ (0, ∞)");
        assert_eq!(d((Expr::int(4) - x().pow(2)).sqrt()), "[-2, 2]");
        assert_eq!(
            d(Expr::func(Func::Ln, vec![x() - Expr::int(3)]).unwrap()),
            "(3, ∞)"
        );
        assert_eq!(d(x().pow(3) + x()), "(-∞, ∞)");
        assert!(natural_domain(&Expr::func(Func::Tan, vec![x()]).unwrap(), "x").is_err());
    }

    #[test]
    fn ranges() {
        let r = |e: Expr| show(&range_of(&e, "x", None).unwrap());
        assert_eq!(r(x().pow(2)), "[0, ∞)");
        assert_eq!(r(x().pow(2) - Expr::int(2) * x() + Expr::int(3)), "[2, ∞)");
        assert_eq!(r((Expr::int(4) - x().pow(2)).sqrt()), "[0, 2]");
        assert_eq!(r(x().recip()), "(-∞, 0) ∪ (0, ∞)");
        assert_eq!(r(Expr::func(Func::Atan, vec![x()]).unwrap()), "(-π/2, π/2)");
        let on = RealSet::from_expr(&Expr::new(ExprNode::Interval {
            lo: Expr::zero(),
            hi: Expr::int(3),
            lo_closed: true,
            hi_closed: true,
        }))
        .unwrap();
        assert_eq!(
            show(&range_of(&x().pow(2), "x", Some(&on)).unwrap()),
            "[0, 9]"
        );
    }

    #[test]
    fn conditions_read_back() {
        let set = solve_inequality(&rel(RelOp::Lt, x().pow(2), Expr::int(4)), "x").unwrap();
        assert_eq!(set_condition("x", &set).to_string(), "(-2 < x) ∧ (x < 2)");
    }
}
