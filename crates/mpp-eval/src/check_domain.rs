//! Checks over a domain: `\frac{dE}{dt} \le 0 \text{ for all } t \ge 0
//! \overset{?}{=} \text{true}`, or an identity with a condition (`…, x > 0`).
//!
//! The statement is tried at points spread over the domain (its ends, points at
//! growing distances from a one-sided end, and pseudo-random points); it passes when it
//! holds at every point where it can be decided, and fails at the first point where it
//! does not. Values with units are compared in SI units, and the variable is given the
//! unit that makes the statement consistent (seconds for a time in `x(t)`).

use mpp_core::number::format_f64;
use mpp_core::units::Unit;
use mpp_core::{EmptyEnv, Expr, ExprNode, LogicOp, RelOp};

use crate::check::Verdict;
use crate::units_wire::{factor, fold, unit_of_factor, Folded};

/// Points that must be decided before the check counts as passed.
const WANTED: usize = 12;

/// Distances from a one-sided end (in the variable's unit).
const OFFSETS: &[f64] = &[
    0.0, 0.001, 0.01, 0.1, 0.25, 0.5, 1.0, 1.5, 2.0, 3.0, 5.0, 7.5, 10.0, 20.0, 50.0, 100.0,
];

/// One end of a domain.
#[derive(Clone, Debug)]
struct End {
    at: f64,
    strict: bool,
}

/// The values of one variable a statement is checked for.
#[derive(Clone, Debug)]
struct Domain {
    var: String,
    lo: Option<End>,
    hi: Option<End>,
}

/// What is checked: a proposition with the truth value wanted, or two sides equal.
enum Claim<'a> {
    Truth(&'a Expr, bool),
    Equal(&'a Expr, &'a Expr),
}

/// Checks `lhs = rhs` (or `lhs` against `true`/`false`) for every value of the
/// conditions' variables. `None` when the conditions are not a domain of the
/// statement's free names (the ordinary check applies then).
pub fn check_on_domain(lhs: &Expr, rhs: &Expr, conditions: &[Expr]) -> Option<Verdict> {
    let claim = match (lhs.node(), rhs.node()) {
        (_, ExprNode::Bool(want)) if is_proposition(lhs) => Claim::Truth(lhs, *want),
        (ExprNode::Bool(want), _) if is_proposition(rhs) => Claim::Truth(rhs, *want),
        _ => Claim::Equal(lhs, rhs),
    };
    let mut domains: Vec<Domain> = Vec::new();
    for c in conditions {
        read_domain(c, &mut domains)?;
    }
    let [domain] = domains.as_slice() else {
        return None;
    };
    let free = match &claim {
        Claim::Truth(p, _) => p.free_symbols(),
        Claim::Equal(a, b) => a.free_symbols().union(&b.free_symbols()).cloned().collect(),
    };
    if free.iter().any(|n| **n != *domain.var) {
        return None;
    }
    let unit = choose_unit(&claim, domain)?;
    // The statement at each point, as a proposition with known sides.
    let instances: Vec<(f64, Expr)> = points(domain)
        .into_iter()
        .map(|x| {
            let at = (Expr::float(x) * factor(&unit)).simplify();
            let p = match &claim {
                Claim::Truth(p, _) => p.subs(&domain.var, &at),
                Claim::Equal(a, b) => Expr::relation(
                    RelOp::Eq,
                    a.subs(&domain.var, &at),
                    b.subs(&domain.var, &at),
                ),
            };
            (x, p)
        })
        .collect();
    // Rounding is judged against the size of the values met over the whole domain, so
    // `0` and `−10⁻¹⁹` agree where the sides are of order 10⁻².
    let scale = instances
        .iter()
        .flat_map(|(_, p)| sides_of(p))
        .filter_map(|s| si_value(&s))
        .map(|(v, _)| v.abs())
        .fold(0.0, f64::max);
    let want = match &claim {
        Claim::Truth(_, want) => *want,
        Claim::Equal(..) => true,
    };
    let mut decided = 0;
    for (x, p) in &instances {
        match truth_at(p, scale) {
            Some(h) if h == want => decided += 1,
            Some(_) => {
                let shown = if unit.is_one() {
                    format_f64(*x, 6)
                } else {
                    format!("{} {}", format_f64(*x, 6), unit.text())
                };
                let (l, r) = match (&claim, p.node()) {
                    (Claim::Equal(..), ExprNode::Relation(_, a, b)) => (side_text(a), side_text(b)),
                    _ => ((!want).to_string(), want.to_string()),
                };
                return Some(Verdict::Fails {
                    at: vec![(domain.var.clone(), shown)],
                    lhs: l,
                    rhs: r,
                });
            }
            None => {}
        }
    }
    let piecewise = match &claim {
        Claim::Truth(p, _) => crate::check::piecewise_name(p),
        Claim::Equal(a, b) => {
            crate::check::piecewise_name(a).or_else(|| crate::check::piecewise_name(b))
        }
    };
    Some(if decided < WANTED {
        Verdict::Unknown(format!(
            "the statement could be decided at only {decided} points of its domain"
        ))
    } else if let Some(name) = piecewise {
        // Points tried prove nothing about a function that changes rule between them.
        Verdict::Unknown(crate::check::piecewise_unknown(
            name,
            decided,
            "the statement holds",
        ))
    } else {
        Verdict::Holds { points: decided }
    })
}

fn is_proposition(e: &Expr) -> bool {
    matches!(e.node(), ExprNode::Relation(..) | ExprNode::Logic(..))
}

fn side_text(e: &Expr) -> String {
    match si_value(e) {
        Some((x, _)) => format_f64(x, 10),
        None => e.to_string(),
    }
}

/// `t ≥ 0`, `0 ≤ t ≤ 10`, `t \in [0, 1]` (lowered as a membership) into a domain.
fn read_domain(c: &Expr, out: &mut Vec<Domain>) -> Option<()> {
    match c.node() {
        ExprNode::Logic(LogicOp::And, items) => {
            for i in items {
                read_domain(i, out)?;
            }
            Some(())
        }
        ExprNode::Relation(op, a, b) => {
            let (var, op, bound) = match (a.as_sym(), b.as_sym()) {
                (Some(v), _) if plain_number(b).is_some() => (v, *op, b),
                (_, Some(v)) if plain_number(a).is_some() => (v, flip(*op)?, a),
                _ => return None,
            };
            let at = plain_number(bound)?;
            let d = domain_of(out, var);
            let end = |strict| Some(End { at, strict });
            match op {
                RelOp::Ge => d.lo = end(false),
                RelOp::Gt => d.lo = end(true),
                RelOp::Le => d.hi = end(false),
                RelOp::Lt => d.hi = end(true),
                _ => return None,
            }
            Some(())
        }
        _ => None,
    }
}

fn domain_of<'a>(out: &'a mut Vec<Domain>, var: &str) -> &'a mut Domain {
    if let Some(i) = out.iter().position(|d| d.var == var) {
        return &mut out[i];
    }
    out.push(Domain {
        var: var.to_string(),
        lo: None,
        hi: None,
    });
    let last = out.len() - 1;
    &mut out[last]
}

fn flip(op: RelOp) -> Option<RelOp> {
    Some(match op {
        RelOp::Lt => RelOp::Gt,
        RelOp::Le => RelOp::Ge,
        RelOp::Gt => RelOp::Lt,
        RelOp::Ge => RelOp::Le,
        _ => return None,
    })
}

/// A bound written as a plain number (a domain bound with units is not read here).
fn plain_number(e: &Expr) -> Option<f64> {
    if crate::units_wire::contains_units(e) || !e.is_constant() {
        return None;
    }
    e.eval_f64(&EmptyEnv).ok().filter(|x| x.is_finite())
}

/// The points tried, in the variable's unit.
fn points(d: &Domain) -> Vec<f64> {
    let mut out = Vec::new();
    let inside = |x: f64| {
        d.lo.as_ref()
            .is_none_or(|e| x > e.at || (!e.strict && x == e.at))
            && d.hi
                .as_ref()
                .is_none_or(|e| x < e.at || (!e.strict && x == e.at))
    };
    match (&d.lo, &d.hi) {
        (Some(lo), Some(hi)) => {
            let n = 24;
            for k in 0..=n {
                out.push(lo.at + (hi.at - lo.at) * f64::from(k) / f64::from(n));
            }
        }
        (Some(lo), None) => out.extend(OFFSETS.iter().map(|o| lo.at + o)),
        (None, Some(hi)) => out.extend(OFFSETS.iter().map(|o| hi.at - o)),
        (None, None) => out.extend(OFFSETS.iter().flat_map(|o| [*o, -o])),
    }
    // A few points between the fixed ones, the same every run.
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let fixed = out.clone();
    for w in fixed.windows(2) {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let u = (seed >> 11) as f64 / (1u64 << 53) as f64;
        out.push(w[0] + (w[1] - w[0]) * u);
    }
    out.retain(|x| x.is_finite() && inside(*x));
    out
}

/// The unit the variable is measured in: none, seconds, or a unit met in the claim;
/// the first one with which the claim can be evaluated at a point of the domain.
fn choose_unit(claim: &Claim<'_>, d: &Domain) -> Option<Unit> {
    let mut candidates = vec![Unit::one()];
    if let Some(s) = Unit::named("s") {
        candidates.push(s);
    }
    let mut met = Vec::new();
    match claim {
        Claim::Truth(p, _) => units_in(p, &mut met),
        Claim::Equal(a, b) => {
            units_in(a, &mut met);
            units_in(b, &mut met);
        }
    }
    for u in met {
        if !candidates.iter().any(|c| c.dimension() == u.dimension()) {
            candidates.push(u);
        }
    }
    let probe = points(d).into_iter().find(|x| *x != 0.0)?;
    candidates.into_iter().find(|u| {
        let at = (Expr::float(probe) * factor(u)).simplify();
        let sides: Vec<Expr> = match claim {
            Claim::Truth(p, _) => sides_of(&p.subs(&d.var, &at)),
            Claim::Equal(a, b) => vec![a.subs(&d.var, &at), b.subs(&d.var, &at)],
        };
        !sides.is_empty() && sides.iter().all(|s| si_value(s).is_some())
    })
}

fn sides_of(p: &Expr) -> Vec<Expr> {
    match p.node() {
        ExprNode::Relation(_, a, b) => vec![a.clone(), b.clone()],
        ExprNode::Logic(_, items) => items.iter().flat_map(sides_of).collect(),
        _ => Vec::new(),
    }
}

fn units_in(e: &Expr, out: &mut Vec<Unit>) {
    if let Some(u) = unit_of_factor(e) {
        if !out.contains(&u) {
            out.push(u);
        }
        return;
    }
    for c in e.children() {
        units_in(c, out);
    }
}

/// A side as a number in SI units, with its unit (`None` for a plain number);
/// `None` when it is not a known finite value.
fn si_value(e: &Expr) -> Option<(f64, Option<Unit>)> {
    let e = e.simplify();
    if !crate::units_wire::contains_units(&e) {
        return e
            .eval_f64(&EmptyEnv)
            .ok()
            .filter(|x| x.is_finite())
            .map(|x| (x, None));
    }
    let Folded::Known {
        q, certain: true, ..
    } = fold(&e).ok()?
    else {
        return None;
    };
    if q.unit.offset().is_some() {
        return None;
    }
    let x = (q.value.clone() * q.unit.scale())
        .simplify()
        .eval_f64(&EmptyEnv)
        .ok()
        .filter(|x| x.is_finite())?;
    Some((x, (!q.unit.is_dimensionless()).then(|| q.unit.si_base())))
}

/// The truth of a proposition whose sides are known values, with rounding judged
/// against `scale` (the size of the values met); `None` when it cannot be
/// decided (a side is not a number, the sides are of different kinds, or a strict
/// inequality's sides are too close to tell).
fn truth_at(p: &Expr, scale: f64) -> Option<bool> {
    match p.node() {
        ExprNode::Bool(b) => Some(*b),
        ExprNode::Relation(op, a, b) => {
            let (x, ux) = si_value(a)?;
            let (y, uy) = si_value(b)?;
            // A plain 0 compares with a value of any kind.
            if let (Some(ux), Some(uy)) = (&ux, &uy) {
                if ux.dimension() != uy.dimension() {
                    return None;
                }
            } else if (ux.is_some() && y != 0.0) || (uy.is_some() && x != 0.0) {
                return None;
            }
            let close = (x - y).abs() <= 1e-9 * x.abs().max(y.abs()) + 1e-12 * scale;
            Some(match op {
                RelOp::Eq | RelOp::Approx => close,
                RelOp::Ne => !close,
                RelOp::Lt | RelOp::Gt if close => return None,
                RelOp::Lt => x < y,
                RelOp::Gt => x > y,
                RelOp::Le => x < y || close,
                RelOp::Ge => x > y || close,
            })
        }
        ExprNode::Logic(op, items) => {
            let vals: Vec<Option<bool>> = items.iter().map(|i| truth_at(i, scale)).collect();
            match op {
                LogicOp::And if vals.contains(&Some(false)) => Some(false),
                LogicOp::And => vals.iter().all(Option::is_some).then_some(true),
                LogicOp::Or if vals.contains(&Some(true)) => Some(true),
                LogicOp::Or => vals.iter().all(Option::is_some).then_some(false),
                LogicOp::Not => vals.first().copied().flatten().map(|v| !v),
            }
        }
        _ => None,
    }
}
