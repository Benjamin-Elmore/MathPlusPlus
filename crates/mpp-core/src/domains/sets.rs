//! Sets and intervals: finite sets, intervals with open or closed ends, and unions of
//! them on the real line, with union, intersection, difference, complement, membership
//! and subset tests.
//!
//! A union of disjoint pieces is the node `union(I₁, I₂, …)`, printed `I₁ ∪ I₂ ∪ …`.

use std::cmp::Ordering;

use crate::domains::util::{self, expect_args, expect_args_between};
use crate::domains::{Ctx, Domain, Value};
use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode};

/// Most pieces a set may have.
pub const MAX_PIECES: usize = 100_000;

/// One piece of a real set: an interval, or a single point when `lo = hi` (closed).
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub lo: Expr,
    pub hi: Expr,
    pub lo_closed: bool,
    pub hi_closed: bool,
}

impl Piece {
    fn point(v: Expr) -> Piece {
        Piece {
            lo: v.clone(),
            hi: v,
            lo_closed: true,
            hi_closed: true,
        }
    }

    fn is_point(&self) -> bool {
        self.lo_closed && self.hi_closed && self.lo == self.hi
    }

    fn to_expr(&self) -> Expr {
        if self.is_point() {
            Expr::new(ExprNode::Set(vec![self.lo.clone()]))
        } else {
            Expr::new(ExprNode::Interval {
                lo: self.lo.clone(),
                hi: self.hi.clone(),
                lo_closed: self.lo_closed,
                hi_closed: self.hi_closed,
            })
        }
    }
}

/// A subset of the real line as sorted, disjoint, non-touching pieces.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct RealSet {
    pieces: Vec<Piece>,
}

fn cmp(a: &Expr, b: &Expr) -> MathResult<Ordering> {
    util::compare(a, b).ok_or_else(|| {
        MathError::Unsupported(format!(
            "comparing {a} and {b} (set bounds must be real numbers)"
        ))
    })
}

/// Orders lower bounds: at the same value a closed bound starts first.
fn cmp_lo(a: &Piece, b: &Piece) -> MathResult<Ordering> {
    Ok(cmp(&a.lo, &b.lo)?.then(b.lo_closed.cmp(&a.lo_closed)))
}

impl RealSet {
    /// The empty set.
    pub fn empty() -> RealSet {
        RealSet::default()
    }

    /// All real numbers.
    pub fn reals() -> RealSet {
        RealSet {
            pieces: vec![Piece {
                lo: Expr::neg_infinity(),
                hi: Expr::infinity(),
                lo_closed: false,
                hi_closed: false,
            }],
        }
    }

    /// The pieces, in order.
    pub fn pieces(&self) -> &[Piece] {
        &self.pieces
    }

    /// True for the empty set.
    pub fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }

    /// Builds a set from pieces in any order (empty pieces are dropped).
    pub fn from_pieces(pieces: Vec<Piece>) -> MathResult<RealSet> {
        if pieces.len() > MAX_PIECES {
            return Err(MathError::TooLarge(format!(
                "a set of {} pieces",
                pieces.len()
            )));
        }
        let mut live = Vec::new();
        for p in pieces {
            let o = cmp(&p.lo, &p.hi)?;
            let empty =
                o == Ordering::Greater || (o == Ordering::Equal && !(p.lo_closed && p.hi_closed));
            if !empty {
                live.push(p);
            }
        }
        // Sort (an insertion sort keeps the fallible comparison simple).
        let mut sorted: Vec<Piece> = Vec::with_capacity(live.len());
        for p in live {
            let mut at = sorted.len();
            for (i, q) in sorted.iter().enumerate() {
                if cmp_lo(&p, q)? == Ordering::Less {
                    at = i;
                    break;
                }
            }
            sorted.insert(at, p);
        }
        let mut merged: Vec<Piece> = Vec::new();
        for p in sorted {
            if let Some(last) = merged.last_mut() {
                let o = cmp(&p.lo, &last.hi)?;
                let touches = o == Ordering::Less
                    || (o == Ordering::Equal && (last.hi_closed || p.lo_closed));
                if touches {
                    match cmp(&p.hi, &last.hi)? {
                        Ordering::Greater => {
                            last.hi = p.hi;
                            last.hi_closed = p.hi_closed;
                        }
                        Ordering::Equal => last.hi_closed |= p.hi_closed,
                        Ordering::Less => {}
                    }
                    continue;
                }
            }
            merged.push(p);
        }
        Ok(RealSet { pieces: merged })
    }

    /// Reads an interval, a finite set of real constants, or a `union(…)` of them.
    pub fn from_expr(e: &Expr) -> MathResult<RealSet> {
        let mut pieces = Vec::new();
        collect(e, &mut pieces)?;
        RealSet::from_pieces(pieces)
    }

    /// The set as an expression.
    pub fn to_expr(&self) -> Expr {
        let mut points = Vec::new();
        let mut parts = Vec::new();
        for p in &self.pieces {
            if p.is_point() {
                points.push(p.lo.clone());
            } else {
                parts.push(p.to_expr());
            }
        }
        if !points.is_empty() {
            parts.push(Expr::new(ExprNode::Set(points)));
        }
        match parts.len() {
            0 => Expr::new(ExprNode::Set(Vec::new())),
            1 => parts
                .pop()
                .unwrap_or_else(|| Expr::new(ExprNode::Set(Vec::new()))),
            _ => Expr::apply("union", parts),
        }
    }

    /// `self ∪ other`.
    pub fn union(&self, other: &RealSet) -> MathResult<RealSet> {
        let mut all = self.pieces.clone();
        all.extend(other.pieces.iter().cloned());
        RealSet::from_pieces(all)
    }

    /// `self ∩ other`.
    pub fn intersection(&self, other: &RealSet) -> MathResult<RealSet> {
        let mut out = Vec::new();
        for a in &self.pieces {
            for b in &other.pieces {
                let (lo, lo_closed) = match cmp(&a.lo, &b.lo)? {
                    Ordering::Greater => (a.lo.clone(), a.lo_closed),
                    Ordering::Less => (b.lo.clone(), b.lo_closed),
                    Ordering::Equal => (a.lo.clone(), a.lo_closed && b.lo_closed),
                };
                let (hi, hi_closed) = match cmp(&a.hi, &b.hi)? {
                    Ordering::Less => (a.hi.clone(), a.hi_closed),
                    Ordering::Greater => (b.hi.clone(), b.hi_closed),
                    Ordering::Equal => (a.hi.clone(), a.hi_closed && b.hi_closed),
                };
                out.push(Piece {
                    lo,
                    hi,
                    lo_closed,
                    hi_closed,
                });
            }
        }
        RealSet::from_pieces(out)
    }

    /// `ℝ \ self`.
    pub fn complement(&self) -> MathResult<RealSet> {
        let mut out = Vec::new();
        let mut lo = Expr::neg_infinity();
        let mut lo_closed = false;
        for p in &self.pieces {
            out.push(Piece {
                lo: lo.clone(),
                hi: p.lo.clone(),
                lo_closed,
                hi_closed: !p.lo_closed,
            });
            lo = p.hi.clone();
            lo_closed = !p.hi_closed;
        }
        out.push(Piece {
            lo,
            hi: Expr::infinity(),
            lo_closed,
            hi_closed: false,
        });
        // Infinite ends are never included.
        for p in &mut out {
            if p.lo.is_infinite() {
                p.lo_closed = false;
            }
            if p.hi.is_infinite() {
                p.hi_closed = false;
            }
        }
        RealSet::from_pieces(out)
    }

    /// `self \ other`.
    pub fn difference(&self, other: &RealSet) -> MathResult<RealSet> {
        self.intersection(&other.complement()?)
    }

    /// True when the real constant `x` is in the set.
    pub fn contains(&self, x: &Expr) -> MathResult<bool> {
        for p in &self.pieces {
            let lo = cmp(x, &p.lo)?;
            let hi = cmp(x, &p.hi)?;
            let above = lo == Ordering::Greater || (lo == Ordering::Equal && p.lo_closed);
            let below = hi == Ordering::Less || (hi == Ordering::Equal && p.hi_closed);
            if above && below {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// True when `self ⊆ other`.
    pub fn is_subset(&self, other: &RealSet) -> MathResult<bool> {
        Ok(self.difference(other)?.is_empty())
    }
}

fn collect(e: &Expr, out: &mut Vec<Piece>) -> MathResult<()> {
    match e.node() {
        ExprNode::Interval {
            lo,
            hi,
            lo_closed,
            hi_closed,
        } => out.push(Piece {
            lo: lo.simplify(),
            hi: hi.simplify(),
            lo_closed: *lo_closed && !lo.is_infinite(),
            hi_closed: *hi_closed && !hi.is_infinite(),
        }),
        ExprNode::Set(v) => {
            for x in v {
                let x = x.simplify();
                if !(x.is_number() || x.is_constant()) || util::numeric(&x).is_none() {
                    return Err(MathError::Unsupported(format!(
                        "{x} in a set on the real line (only real numbers)"
                    )));
                }
                out.push(Piece::point(x));
            }
        }
        ExprNode::Apply(name, parts) if &**name == "union" => {
            for p in parts {
                collect(p, out)?;
            }
        }
        _ => {
            return Err(MathError::NotANumber(format!("{e} as a set")));
        }
    }
    Ok(())
}

/// Finite sets of arbitrary (also symbolic) elements.
mod finite {
    use super::*;

    pub fn elements(e: &Expr) -> Option<Vec<Expr>> {
        match e.node() {
            ExprNode::Set(v) => Some(dedup(v.iter().map(Expr::simplify).collect())),
            _ => None,
        }
    }

    pub fn dedup(v: Vec<Expr>) -> Vec<Expr> {
        let mut out: Vec<Expr> = Vec::new();
        for x in v {
            if !out.contains(&x) {
                out.push(x);
            }
        }
        out.sort();
        out
    }

    pub fn set(v: Vec<Expr>) -> Expr {
        Expr::new(ExprNode::Set(dedup(v)))
    }
}

/// The sets domain.
pub struct Sets;

const OPS: &[&str] = &[
    "union",
    "intersect",
    "setminus",
    "complement",
    "member",
    "subset",
    "card",
];

impl Domain for Sets {
    fn name(&self) -> &'static str {
        "sets and intervals"
    }

    fn operators(&self) -> &'static [&'static str] {
        OPS
    }

    fn apply(&self, op: &str, args: &[Value], _ctx: &mut Ctx<'_>) -> Option<MathResult<Value>> {
        Some(apply(op, args))
    }
}

fn apply(op: &str, args: &[Value]) -> MathResult<Value> {
    let truth = |b: bool| Expr::new(ExprNode::Bool(b));
    match op {
        "union" | "intersect" => {
            expect_args_between(op, args, 1, MAX_PIECES)?;
            // Finite sets of any elements first.
            let finite: Option<Vec<Vec<Expr>>> = args.iter().map(finite::elements).collect();
            if let Some(sets) = finite {
                let all_real = sets.iter().flatten().all(|x| util::numeric(x).is_some());
                if !all_real || op == "union" {
                    let out = if op == "union" {
                        sets.concat()
                    } else {
                        let mut it = sets.into_iter();
                        let first = it.next().unwrap_or_default();
                        it.fold(first, |acc, s| {
                            acc.into_iter().filter(|x| s.contains(x)).collect()
                        })
                    };
                    return Ok(finite::set(out));
                }
            }
            let mut acc = RealSet::from_expr(&args[0])?;
            for a in &args[1..] {
                let b = RealSet::from_expr(a)?;
                acc = if op == "union" {
                    acc.union(&b)?
                } else {
                    acc.intersection(&b)?
                };
            }
            Ok(acc.to_expr())
        }
        "setminus" => {
            expect_args(op, args, 2)?;
            if let (Some(a), Some(b)) = (finite::elements(&args[0]), finite::elements(&args[1])) {
                return Ok(finite::set(
                    a.into_iter().filter(|x| !b.contains(x)).collect(),
                ));
            }
            Ok(RealSet::from_expr(&args[0])?
                .difference(&RealSet::from_expr(&args[1])?)?
                .to_expr())
        }
        "complement" => {
            expect_args(op, args, 1)?;
            Ok(RealSet::from_expr(&args[0])?.complement()?.to_expr())
        }
        "member" => {
            expect_args(op, args, 2)?;
            let x = args[0].simplify();
            if let Some(v) = finite::elements(&args[1]) {
                if v.contains(&x) {
                    return Ok(truth(true));
                }
                if v.iter().all(|e| util::numeric(e).is_some()) && util::numeric(&x).is_some() {
                    return Ok(truth(v.iter().any(|e| util::equal(e, &x))));
                }
                return Err(MathError::Unsupported(format!(
                    "deciding whether {x} is in this set"
                )));
            }
            Ok(truth(RealSet::from_expr(&args[1])?.contains(&x)?))
        }
        "subset" => {
            expect_args(op, args, 2)?;
            if let (Some(a), Some(b)) = (finite::elements(&args[0]), finite::elements(&args[1])) {
                if a.iter().all(|x| b.contains(x)) {
                    return Ok(truth(true));
                }
            }
            let a = RealSet::from_expr(&args[0])?;
            let b = RealSet::from_expr(&args[1])?;
            Ok(truth(a.is_subset(&b)?))
        }
        "card" => {
            expect_args(op, args, 1)?;
            match finite::elements(&args[0]) {
                Some(v) => Ok(Expr::int(v.len() as i64)),
                None => {
                    let s = RealSet::from_expr(&args[0])?;
                    if s.pieces().iter().all(Piece::is_point) {
                        Ok(Expr::int(s.pieces().len() as i64))
                    } else if s.is_empty() {
                        Ok(Expr::zero())
                    } else {
                        Ok(Expr::infinity())
                    }
                }
            }
        }
        _ => Err(MathError::UnknownFunction(op.into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iv(lo: Expr, hi: Expr, lc: bool, hc: bool) -> Expr {
        Expr::new(ExprNode::Interval {
            lo,
            hi,
            lo_closed: lc,
            hi_closed: hc,
        })
    }
    fn n(k: i64) -> Expr {
        Expr::int(k)
    }
    fn set(v: Vec<Expr>) -> Expr {
        Expr::new(ExprNode::Set(v))
    }
    fn run(op: &str, args: &[Expr]) -> Expr {
        apply(op, args).unwrap()
    }

    #[test]
    fn unions_merge_touching_pieces() {
        let a = iv(n(0), n(1), true, false);
        let b = iv(n(1), n(2), true, true);
        assert_eq!(run("union", &[a.clone(), b]), iv(n(0), n(2), true, true));
        let c = iv(n(3), n(4), false, false);
        assert_eq!(
            run("union", &[a.clone(), c.clone()]).to_string(),
            Expr::apply("union", vec![a, c]).to_string()
        );
        // (0, 1) ∪ (1, 2) keeps the gap at 1.
        let r = run(
            "union",
            &[iv(n(0), n(1), false, false), iv(n(1), n(2), false, false)],
        );
        assert!(matches!(r.node(), ExprNode::Apply(..)));
    }

    #[test]
    fn intersections_and_differences() {
        let a = iv(n(0), n(5), true, true);
        let b = iv(n(3), Expr::infinity(), false, false);
        assert_eq!(
            run("intersect", &[a.clone(), b.clone()]),
            iv(n(3), n(5), false, true)
        );
        assert_eq!(run("setminus", &[a.clone(), b]), iv(n(0), n(3), true, true));
        let pts = set(vec![n(1), n(7)]);
        assert_eq!(run("intersect", &[a.clone(), pts]), set(vec![n(1)]));
        let c = run("complement", &[a]);
        assert_eq!(
            c,
            Expr::apply(
                "union",
                vec![
                    iv(Expr::neg_infinity(), n(0), false, false),
                    iv(n(5), Expr::infinity(), false, false)
                ]
            )
        );
    }

    #[test]
    fn membership_and_subsets() {
        let truth = |b| Expr::new(ExprNode::Bool(b));
        let a = iv(n(0), Expr::pi(), true, false);
        assert_eq!(run("member", &[n(3), a.clone()]), truth(true));
        assert_eq!(run("member", &[Expr::pi(), a.clone()]), truth(false));
        assert_eq!(
            run("subset", &[iv(n(1), n(2), true, true), a.clone()]),
            truth(true)
        );
        assert_eq!(
            run("subset", &[a, iv(n(1), n(2), true, true)]),
            truth(false)
        );
        let s = set(vec![Expr::sym("a"), Expr::sym("b")]);
        assert_eq!(run("member", &[Expr::sym("a"), s.clone()]), truth(true));
        assert_eq!(run("card", std::slice::from_ref(&s)), n(2));
        let t = set(vec![Expr::sym("b"), Expr::sym("c")]);
        assert_eq!(
            run("intersect", &[s.clone(), t.clone()]),
            set(vec![Expr::sym("b")])
        );
        assert_eq!(
            run("union", &[s, t]).to_string(),
            set(vec![Expr::sym("a"), Expr::sym("b"), Expr::sym("c")]).to_string()
        );
    }

    #[test]
    fn symbolic_bounds_are_an_error() {
        let a = iv(Expr::sym("a"), n(1), true, true);
        assert!(apply("intersect", &[a, iv(n(0), n(2), true, true)]).is_err());
    }
}
