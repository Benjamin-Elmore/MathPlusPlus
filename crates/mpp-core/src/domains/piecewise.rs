//! Piecewise functions of one variable as values: the pieces as sets of the real line,
//! continuity and differentiability at the joins, derivatives, definite integrals over
//! several pieces and continuous antiderivatives.
//!
//! A piecewise expression's cases are tried in order, so each piece is the set where its
//! condition holds minus the sets of the cases before it; `otherwise` takes what is left.
//! Conditions are solved with [`crate::domains::intervals::solve_inequality`]; when a
//! condition can't be solved (it mentions another name, say) these functions return
//! `Ok(None)` and the caller keeps its usual behaviour.

use std::cmp::Ordering;

use crate::domains::intervals::{set_condition, solve_inequality};
use crate::domains::sets::{Piece, RealSet};
use crate::domains::util;
use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode, LimitDir};
use crate::simplify::add;

/// One piece: a formula and the set where it applies.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub value: Expr,
    pub set: RealSet,
}

/// The pieces of a piecewise expression in `var`, or `None` when its conditions are not
/// all conditions on `var` that can be solved.
pub fn parts(e: &Expr, var: &str) -> MathResult<Option<Vec<Part>>> {
    let ExprNode::Piecewise { cases, otherwise } = e.node() else {
        return Ok(None);
    };
    let mut covered = RealSet::empty();
    let mut out = Vec::new();
    for (value, cond) in cases {
        let Ok(set) = solve_inequality(cond, var) else {
            return Ok(None);
        };
        let own = set.difference(&covered)?;
        covered = covered.union(&set)?;
        if !own.is_empty() {
            out.push(Part {
                value: value.clone(),
                set: own,
            });
        }
    }
    if let Some(o) = otherwise {
        let rest = covered.complement()?;
        if !rest.is_empty() {
            out.push(Part {
                value: o.clone(),
                set: rest,
            });
        }
    }
    Ok(Some(out))
}

/// What happens at a point where two pieces meet.
#[derive(Clone, Debug, PartialEq)]
pub struct Join {
    pub at: Expr,
    /// The limit from the left and from the right.
    pub left: Expr,
    pub right: Expr,
    /// The value at the point, when the function is defined there.
    pub value: Option<Expr>,
    /// `None` when it can't be decided (symbolic values).
    pub continuous: Option<bool>,
    /// The one-sided derivatives.
    pub left_slope: Option<Expr>,
    pub right_slope: Option<Expr>,
    pub differentiable: Option<bool>,
}

impl Join {
    /// A sentence such as "at x = 1 it is continuous and differentiable".
    pub fn describe(&self, var: &str) -> String {
        let at = format!("at {var} = {}", self.at);
        match (self.continuous, self.differentiable) {
            (Some(true), Some(true)) => format!("{at} it is continuous and differentiable"),
            (Some(true), Some(false)) => match (&self.left_slope, &self.right_slope) {
                (Some(l), Some(r)) => format!(
                    "{at} it is continuous but not differentiable (the slope is {l} on the left and {r} on the right)"
                ),
                _ => format!("{at} it is continuous but not differentiable"),
            },
            (Some(false), _) => match &self.value {
                None => format!(
                    "{at} it is not defined (the limits are {} and {})",
                    self.left, self.right
                ),
                Some(_) if !util::equal(&self.left, &self.right) => format!(
                    "{at} it jumps from {} to {}, so it is neither continuous nor differentiable there",
                    self.left, self.right
                ),
                Some(v) => format!(
                    "{at} its value {v} differs from the limit {}, so it is not continuous there",
                    self.left
                ),
            },
            _ => format!("{at} continuity can't be decided"),
        }
    }
}

/// The points where pieces meet, sorted, with continuity and differentiability.
pub fn joins(parts: &[Part], var: &str) -> MathResult<Vec<Join>> {
    let mut points: Vec<Expr> = Vec::new();
    for p in parts {
        for piece in p.set.pieces() {
            for end in [&piece.lo, &piece.hi] {
                if !end.is_infinite() && !points.iter().any(|q| util::equal(q, end)) {
                    points.push(end.clone());
                }
            }
        }
    }
    points.sort_by(|a, b| util::compare(a, b).unwrap_or(Ordering::Equal));
    let mut out = Vec::new();
    for c in points {
        let left = parts.iter().find(|p| touches(&p.set, &c, true));
        let right = parts.iter().find(|p| touches(&p.set, &c, false));
        let (Some(l), Some(r)) = (left, right) else {
            continue; // an end of the domain, not a join
        };
        let at = parts.iter().find(|p| p.set.contains(&c).unwrap_or(false));
        if std::ptr::eq(l, r) && at.is_some_and(|a| std::ptr::eq(a, l)) {
            continue; // inside one piece
        }
        let lim = |p: &Part, dir: LimitDir| -> MathResult<Expr> {
            crate::calc::limit(&p.value, var, &c, dir, None)
        };
        let left_v = lim(l, LimitDir::Left)?;
        let right_v = lim(r, LimitDir::Right)?;
        let value = at.map(|p| p.value.subs(var, &c).simplify());
        let continuous = match &value {
            None => Some(false),
            Some(v) => decide_equal(&left_v, &right_v)
                .and_then(|lr| decide_equal(&left_v, v).map(|lv| lr && lv)),
        };
        let slope = |p: &Part, dir: LimitDir| -> Option<Expr> {
            let d = crate::calc::diff(&p.value, var, 1, None).ok()?;
            crate::calc::limit(&d, var, &c, dir, None).ok()
        };
        let (ls, rs) = (slope(l, LimitDir::Left), slope(r, LimitDir::Right));
        let differentiable = match continuous {
            Some(false) => Some(false),
            Some(true) => match (&ls, &rs) {
                (Some(a), Some(b)) => decide_equal(a, b),
                _ => None,
            },
            None => None,
        };
        out.push(Join {
            at: c,
            left: left_v,
            right: right_v,
            value,
            continuous,
            left_slope: ls,
            right_slope: rs,
            differentiable,
        });
    }
    Ok(out)
}

/// True when `set` contains an interval that ends at `c` (from the left when `left`).
fn touches(set: &RealSet, c: &Expr, left: bool) -> bool {
    set.pieces().iter().any(|p| {
        if left {
            util::equal(&p.hi, c) && util::compare(&p.lo, c) == Some(Ordering::Less)
        } else {
            util::equal(&p.lo, c) && util::compare(&p.hi, c) == Some(Ordering::Greater)
        }
    })
}

fn decide_equal(a: &Expr, b: &Expr) -> Option<bool> {
    if a.is_infinite() || b.is_infinite() {
        return Some(a == b);
    }
    let d = (a - b).simplify();
    if util::is_zero(&d) {
        return Some(true);
    }
    d.is_constant().then_some(false)
}

/// Builds a piecewise expression from parts.
pub fn from_parts(var: &str, parts: Vec<Part>) -> Expr {
    let mut parts: Vec<Part> = parts.into_iter().filter(|p| !p.set.is_empty()).collect();
    if parts.len() == 1 && parts[0].set == RealSet::reals() {
        return parts.remove(0).value;
    }
    Expr::new(ExprNode::Piecewise {
        cases: parts
            .into_iter()
            .map(|p| (p.value, set_condition(var, &p.set)))
            .collect(),
        otherwise: None,
    })
}

/// The derivative piece by piece; joins where the function is not differentiable are
/// left out of every piece. Returns the derivative and the joins.
pub fn derivative(e: &Expr, var: &str) -> MathResult<Option<(Expr, Vec<Join>)>> {
    let Some(ps) = parts(e, var)? else {
        return Ok(None);
    };
    let js = joins(&ps, var)?;
    let mut out = Vec::with_capacity(ps.len());
    for p in &ps {
        let mut set = p.set.clone();
        for j in &js {
            if j.differentiable != Some(true) {
                let single = RealSet::from_pieces(vec![Piece {
                    lo: j.at.clone(),
                    hi: j.at.clone(),
                    lo_closed: true,
                    hi_closed: true,
                }])?;
                set = set.difference(&single)?;
            }
        }
        // A closed end of the domain that is not a join has only a one-sided derivative.
        out.push(Part {
            value: crate::calc::diff(&p.value, var, 1, None)?.simplify(),
            set,
        });
    }
    Ok(Some((from_parts(var, out), js)))
}

/// `∫_lo^hi e dvar` piece by piece.
pub fn integrate_definite(e: &Expr, var: &str, lo: &Expr, hi: &Expr) -> MathResult<Option<Expr>> {
    let Some(ps) = parts(e, var)? else {
        return Ok(None);
    };
    let (a, b, sign) = match util::compare(lo, hi) {
        Some(Ordering::Greater) => (hi, lo, -1),
        Some(_) => (lo, hi, 1),
        None => return Ok(None),
    };
    let range = RealSet::from_pieces(vec![Piece {
        lo: a.clone(),
        hi: b.clone(),
        lo_closed: true,
        hi_closed: true,
    }])?;
    let mut covered = RealSet::empty();
    let mut terms = Vec::new();
    for p in &ps {
        let inside = p.set.intersection(&range)?;
        covered = covered.union(&inside)?;
        for piece in inside.pieces() {
            if util::equal(&piece.lo, &piece.hi) {
                continue;
            }
            terms.push(crate::calc::integrate_definite(
                &p.value, var, &piece.lo, &piece.hi, None,
            )?);
        }
    }
    // Missing single points don't change an integral; a missing interval does.
    let gaps = range.difference(&covered)?;
    if gaps.pieces().iter().any(|g| !util::equal(&g.lo, &g.hi)) {
        return Err(MathError::Undefined(format!(
            "the integrand on part of [{a}, {b}] (it is not defined on {})",
            gaps.to_expr()
        )));
    }
    Ok(Some((Expr::int(sign) * add(terms)).simplify()))
}

/// A continuous antiderivative: each piece's antiderivative shifted so the pieces meet,
/// starting from the leftmost piece. `None` when the pieces don't form one interval.
pub fn antiderivative(e: &Expr, var: &str) -> MathResult<Option<Expr>> {
    let Some(ps) = parts(e, var)? else {
        return Ok(None);
    };
    // One part per interval, left to right.
    let mut intervals: Vec<(Piece, Expr)> = Vec::new();
    for p in &ps {
        for piece in p.set.pieces() {
            if !util::equal(&piece.lo, &piece.hi) {
                intervals.push((piece.clone(), p.value.clone()));
            }
        }
    }
    intervals.sort_by(|a, b| util::compare(&a.0.lo, &b.0.lo).unwrap_or(Ordering::Equal));
    for w in intervals.windows(2) {
        if !util::equal(&w[0].0.hi, &w[1].0.lo) {
            return Ok(None);
        }
    }
    let mut out = Vec::with_capacity(intervals.len());
    let mut previous: Option<(Expr, Expr)> = None; // (antiderivative, join point)
    for (piece, f) in intervals {
        let mut big_f = crate::calc::integrate(&f, var, None)?;
        if let Some((prev, at)) = &previous {
            let want = crate::calc::limit(prev, var, at, LimitDir::Left, None)?;
            let have = crate::calc::limit(&big_f, var, at, LimitDir::Right, None)?;
            big_f = (big_f + (want - have)).simplify();
        }
        previous = Some((big_f.clone(), piece.hi.clone()));
        out.push(Part {
            value: big_f,
            set: RealSet::from_pieces(vec![piece])?,
        });
    }
    Ok(Some(from_parts(var, out)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::RelOp;
    use crate::functions::Func;

    fn x() -> Expr {
        Expr::sym("x")
    }

    /// x² for x < 1, 2x − 1 for x ≥ 1 (continuous and differentiable at 1).
    fn smooth() -> Expr {
        Expr::new(ExprNode::Piecewise {
            cases: vec![
                (x().pow(2), Expr::relation(RelOp::Lt, x(), Expr::one())),
                (
                    Expr::int(2) * x() - Expr::one(),
                    Expr::relation(RelOp::Ge, x(), Expr::one()),
                ),
            ],
            otherwise: None,
        })
    }

    #[test]
    fn joins_are_reported() {
        let ps = parts(&smooth(), "x").unwrap().unwrap();
        let js = joins(&ps, "x").unwrap();
        assert_eq!(js.len(), 1);
        assert_eq!(js[0].continuous, Some(true));
        assert_eq!(js[0].differentiable, Some(true));
        // |x| written as cases: a corner at 0.
        let abs = Expr::new(ExprNode::Piecewise {
            cases: vec![(-x(), Expr::relation(RelOp::Lt, x(), Expr::zero()))],
            otherwise: Some(x()),
        });
        let js = joins(&parts(&abs, "x").unwrap().unwrap(), "x").unwrap();
        assert_eq!(js[0].differentiable, Some(false));
        assert!(js[0].describe("x").contains("not differentiable"));
        // A jump.
        let step = Expr::new(ExprNode::Piecewise {
            cases: vec![(Expr::zero(), Expr::relation(RelOp::Lt, x(), Expr::zero()))],
            otherwise: Some(Expr::one()),
        });
        let js = joins(&parts(&step, "x").unwrap().unwrap(), "x").unwrap();
        assert_eq!(js[0].continuous, Some(false));
        assert!(js[0].describe("x").contains("jumps from 0 to 1"));
    }

    #[test]
    fn calculus_across_pieces() {
        let (d, _) = derivative(&smooth(), "x").unwrap().unwrap();
        assert_eq!(d.to_string(), "{ 2*x if x < 1; 2 if x ≥ 1 }");
        let i = integrate_definite(&smooth(), "x", &Expr::zero(), &Expr::int(2))
            .unwrap()
            .unwrap();
        assert_eq!(i, Expr::rational(7, 3));
        let f = antiderivative(&smooth(), "x").unwrap().unwrap();
        // Continuous at 1: x³/3 meets x² − x + 1/3.
        let ps = parts(&f, "x").unwrap().unwrap();
        let js = joins(&ps, "x").unwrap();
        assert_eq!(js[0].continuous, Some(true));
        // |x| as cases: the derivative leaves out 0.
        let abs = Expr::new(ExprNode::Piecewise {
            cases: vec![(-x(), Expr::relation(RelOp::Lt, x(), Expr::zero()))],
            otherwise: Some(x()),
        });
        let (d, _) = derivative(&abs, "x").unwrap().unwrap();
        assert_eq!(d.to_string(), "{ -1 if x < 0; 1 if x > 0 }");
        let _ = Func::Abs;
    }
}
