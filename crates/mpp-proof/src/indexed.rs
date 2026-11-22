//! Sums over indexed families: `\sum_{i=1}^{n} a_i b_i`, `A := \sum a_i^2`.
//!
//! The CAS reads `a_i` as one fixed symbol, so a sum over a family cannot be handed to it
//! as written. The methods, strongest first:
//!
//! 1. **Real squares** (✓): a sum of squares of real numbers is never negative, and it is
//!    0 only when every term is 0. Used only when every name in the squares is known to be
//!    real.
//! 2. **Linearity** (✓ for every n): each sum is expanded term by term and written as a
//!    combination of the basic sums it is made of (`\sum (a_i x + b_i)^2 = x^2 \sum a_i^2 +
//!    2x \sum a_i b_i + \sum b_i^2`). When both sides become the same combination of the
//!    same basic sums, the identity holds for every n and every family. A proof, not a
//!    sample: finite sums are linear.
//! 3. **Small cases and random families** (≈): the sums written out for n = 1, …, 4 and
//!    checked symbolically, then evaluated on random families of random length. This is
//!    evidence for every n, not a proof, so it is never better than ≈.

use std::collections::BTreeSet;

use mpp_core::{Expr, ExprNode};
use mpp_eval::Verdict;
use mpp_syntax::math::{parse_expr, BigOpKind, Node, NodeKind};
use mpp_syntax::{Severity, Span};

use crate::algebra::{fmt, holds, wrap, Knowledge, Rng};
use crate::cas::{self, Def, LineOut};
use crate::model::ClaimOutcome;
use crate::notation::{self, ClaimOp};

/// The symbol that stands for one basic sum in the linearity method.
const ATOM: &str = "\\Upsilon";
/// The lengths written out symbolically.
const SMALL_N: std::ops::RangeInclusive<i64> = 1..=4;
/// Random families tried, how many must agree, and their longest length.
const FAMILY_TRIES: usize = 30;
const FAMILY_WANTED: usize = 10;
const FAMILY_MAX_N: u64 = 8;
/// The most terms a sum is written out with.
const MAX_TERMS: i64 = 64;

/// A claim whose sides (LaTeX, as the checker reads them) use sums over a family.
pub(crate) struct FamilyClaim<'a> {
    pub lhs: &'a str,
    pub op: ClaimOp,
    pub rhs: &'a str,
    /// The names the claim uses, as written.
    pub names: &'a BTreeSet<String>,
}

/// Checks a claim about sums over indexed families.
pub(crate) fn check(claim: &FamilyClaim<'_>, k: &Knowledge) -> ClaimOutcome {
    if let Some(o) = real_squares_nonnegative(claim, k) {
        return o;
    }
    if let Some(o) = real_squares_zero(claim, k) {
        return o;
    }
    if claim.op == ClaimOp::Eq {
        if let Some(o) = linearity(claim, k) {
            return o;
        }
    }
    instances(claim, k)
}

// ---- reading sums -----------------------------------------------------------------

/// A sum over an indexed family, found in some LaTeX.
#[derive(Debug, Clone)]
struct FamilySum {
    /// Where the whole sum sits in the text it was found in.
    span: Span,
    index: String,
    from: String,
    to: String,
    body: String,
}

/// The outermost sums over a family in `text`, in order.
fn find_sums(text: &str, k: &Knowledge) -> Result<Vec<FamilySum>, String> {
    if text.contains(ATOM) || text.contains('Υ') {
        return Err("the text uses the symbol the checker reserves for sums".into());
    }
    let (node, diags) = parse_expr(text, 0);
    if diags.iter().any(|d| d.severity == Severity::Error) {
        return Err(format!("${text}$ could not be read"));
    }
    let mut out = Vec::new();
    let mut err = None;
    collect(&node, text, k, &mut out, &mut err);
    match err {
        Some(e) => Err(e),
        None => {
            out.sort_by_key(|s| s.span.start);
            Ok(out)
        }
    }
}

fn collect(
    node: &Node,
    text: &str,
    k: &Knowledge,
    out: &mut Vec<FamilySum>,
    err: &mut Option<String>,
) {
    if let NodeKind::BigOp(b) = &node.kind {
        let subs = subscripts(&b.body, text);
        let index = match &b.index {
            Some(i) => Some(sub_key(notation::text(text, i.span))),
            None if subs.len() == 1 => subs.iter().next().cloned(),
            None if subs.is_empty() => None,
            None => {
                err.get_or_insert_with(|| {
                    "the sum does not say which index it runs over".to_string()
                });
                return;
            }
        };
        if let Some(index) = index.filter(|i| subs.contains(i)) {
            if b.op != BigOpKind::Sum {
                err.get_or_insert_with(|| "products over a family are not computed".into());
                return;
            }
            if b.over.is_some() {
                err.get_or_insert_with(|| {
                    "a sum over a set of indices (such as i < j) is not computed".into()
                });
                return;
            }
            let (from, to) = match (&b.from, &b.to) {
                (Some(f), Some(t)) => (
                    notation::node_text(f, text).trim().to_string(),
                    notation::node_text(t, text).trim().to_string(),
                ),
                (None, None) => family_range(&b.body, text, &index, k),
                _ => {
                    err.get_or_insert_with(|| "the sum has only one of its bounds".into());
                    return;
                }
            };
            let whole = notation::node_text(node, text);
            out.push(FamilySum {
                span: Span::new(node.span.start, node.span.start + whole.len()),
                index,
                from,
                to,
                body: notation::node_text(&b.body, text).trim().to_string(),
            });
            return;
        }
    }
    mpp_syntax::math::visit::children(node, &mut |c| collect(c, text, k, out, err));
}

/// `i` for `i`, `{i}` and ` i `.
fn sub_key(t: &str) -> String {
    t.trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .trim()
        .to_string()
}

/// The subscripts of the names in `node` that are single letters (the candidate indices).
fn subscripts(node: &Node, text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    notation::walk(node, &mut |n| {
        if let NodeKind::Name(name) = &n.kind {
            if let Some(sub) = &name.sub {
                let s = sub_key(notation::text(text, sub.span));
                if s.chars().count() == 1 && s.chars().all(char::is_alphabetic) {
                    out.insert(s);
                }
            }
        }
    });
    out
}

/// The range of a sum written without bounds (`\sum a_i^2`): the family's own range when
/// it was written out (`a_1, \dots, a_n`), otherwise 1 to n.
fn family_range(body: &Node, text: &str, index: &str, k: &Knowledge) -> (String, String) {
    let mut base = None;
    notation::walk(body, &mut |n| {
        if let NodeKind::Name(name) = &n.kind {
            if let Some(sub) = &name.sub {
                if sub_key(notation::text(text, sub.span)) == index && base.is_none() {
                    base = Some(member_base_text(n, text));
                }
            }
        }
    });
    base.and_then(|b| {
        k.families
            .iter()
            .find(|(fb, _, _)| *fb == b)
            .map(|(_, f, t)| (f.clone(), t.clone()))
    })
    .unwrap_or_else(|| ("1".into(), "n".into()))
}

/// The base of a subscripted name as written (`a` for `a_i`, `\alpha` for `\alpha_{i}`).
fn member_base_text(n: &Node, text: &str) -> String {
    let t = notation::node_text(n, text);
    t.split('_').next().unwrap_or("").trim().to_string()
}

/// The base of the CAS symbol `sym` when it is a member of a family indexed by `index`
/// (`a_i` gives `a`).
fn member_base<'s>(sym: &'s str, index: &str) -> Option<&'s str> {
    sym.strip_suffix(index)
        .and_then(|s| s.strip_suffix('_'))
        .filter(|b| !b.is_empty())
}

/// True when the index appears in `body` outside a subscript (`\sum i a_i`).
fn bare_index(body: &str, index: &str) -> bool {
    let (node, _) = parse_expr(body, 0);
    let mut subs: Vec<Span> = Vec::new();
    notation::walk(&node, &mut |n| {
        if let NodeKind::Name(name) = &n.kind {
            if let Some(s) = &name.sub {
                subs.push(s.span);
            }
        }
    });
    let mut found = false;
    notation::walk(&node, &mut |n| {
        let inside = subs
            .iter()
            .any(|s| s.start <= n.span.start && n.span.end <= s.end);
        if !inside
            && matches!(n.kind, NodeKind::Name(_) | NodeKind::Constant(_))
            && notation::text(body, n.span).trim() == index
        {
            found = true;
        }
    });
    found
}

// ---- 1. real squares ----------------------------------------------------------------

/// The sum in `side`, when `side` is one family sum or a name defined as one.
fn whole_sum(side: &str, k: &Knowledge) -> Option<FamilySum> {
    let side = side.trim();
    let text = match k.defs.iter().find(|(d, _)| d.name == side) {
        Some((d, _)) if k.indexed.contains(side) => d.body.trim().to_string(),
        _ => side.to_string(),
    };
    let sums = find_sums(&text, k).ok()?;
    let [s] = sums.as_slice() else {
        return None;
    };
    (s.span.start == 0 && s.span.end >= text.trim_end().len()).then(|| s.clone())
}

/// The base of the body when the body is an even power (`(a_i x + b_i)^2` gives
/// `a_i x + b_i`) whose names are all real.
fn real_square_base(s: &FamilySum, k: &Knowledge) -> Option<String> {
    let (node, _) = parse_expr(&s.body, 0);
    let NodeKind::Pow { base, exp } = &node.ungrouped().kind else {
        return None;
    };
    let even = notation::text(&s.body, exp.span)
        .trim()
        .trim_matches(['{', '}'])
        .parse::<u32>()
        .is_ok_and(|e| e > 0 && e % 2 == 0);
    if !even {
        return None;
    }
    let names = notation::names(base, &s.body);
    let real = names.iter().all(|n| is_real(n, &s.index, k));
    real.then(|| notation::node_text(base, &s.body).trim().to_string())
}

/// True when `name` is known to be real: declared so, or a member of a family declared
/// real (`a_i` when `a_1, \dots, a_n` are real numbers).
fn is_real(name: &str, index: &str, k: &Knowledge) -> bool {
    if k.reals.contains(name) {
        return true;
    }
    let Some((base, sub)) = name.split_once('_') else {
        return false;
    };
    sub_key(sub) == index
        && k.reals
            .iter()
            .any(|r| r.split_once('_').is_some_and(|(b, _)| b == base))
}

/// `0 \le \sum (…)^2` (or `\sum (…)^2 \ge 0`) for real terms.
fn real_squares_nonnegative(claim: &FamilyClaim<'_>, k: &Knowledge) -> Option<ClaimOutcome> {
    let side = match claim.op {
        ClaimOp::Le if claim.lhs.trim() == "0" => claim.rhs,
        ClaimOp::Ge if claim.rhs.trim() == "0" => claim.lhs,
        _ => return None,
    };
    let s = whole_sum(side, k)?;
    real_square_base(&s, k)?;
    Some(ClaimOutcome::ProvedBy(
        "a sum of squares of real numbers is never negative".into(),
    ))
}

/// `t = 0` for a term `t` of a sum of real squares known to be 0 (`A = 0` with
/// `A := \sum a_i^2` gives `a_i = 0`).
fn real_squares_zero(claim: &FamilyClaim<'_>, k: &Knowledge) -> Option<ClaimOutcome> {
    if claim.op != ClaimOp::Eq {
        return None;
    }
    let term = match (claim.lhs.trim(), claim.rhs.trim()) {
        (t, "0") | ("0", t) => t,
        _ => return None,
    };
    let squash = |t: &str| t.replace([' ', '{', '}'], "");
    for e in &k.equations {
        let zero_side = match (e.lhs.trim(), e.rhs.trim()) {
            (s, "0") | ("0", s) => s,
            _ => continue,
        };
        let Some(s) = whole_sum(zero_side, k) else {
            continue;
        };
        let Some(base) = real_square_base(&s, k) else {
            continue;
        };
        if squash(&base) == squash(term) {
            return Some(ClaimOutcome::ProvedBy(format!(
                "a sum of squares of real numbers is 0 only when every term is 0, and ${} = 0$",
                zero_side.trim()
            )));
        }
    }
    None
}

// ---- 2. linearity -------------------------------------------------------------------

/// The definitions a claim needs, with the definitions that sum over a family rewritten
/// by `rewrite`.
fn claim_defs(
    claim: &FamilyClaim<'_>,
    k: &Knowledge,
    rewrite: &mut dyn FnMut(&str) -> Result<String, String>,
) -> Result<Vec<Def>, String> {
    let mut out = Vec::new();
    for d in k.defs_for(claim.names) {
        if k.indexed.contains(&d.name) {
            out.push(Def::new(d.name.clone(), rewrite(&d.body)?));
        } else {
            out.push(d);
        }
    }
    Ok(out)
}

fn linearity(claim: &FamilyClaim<'_>, k: &Knowledge) -> Option<ClaimOutcome> {
    let mut atoms: Vec<String> = Vec::new();
    let lhs = atomise(claim.lhs, k, &mut atoms).ok()?;
    let rhs = atomise(claim.rhs, k, &mut atoms).ok()?;
    let defs = claim_defs(claim, k, &mut |body| atomise(body, k, &mut atoms)).ok()?;
    if atoms.is_empty() {
        return None;
    }
    let line = format!("{} \\overset{{?}}{{=}} {}", wrap(&lhs), wrap(&rhs));
    match cas::run_one(&defs, line) {
        LineOut::Verdict(Verdict::Proved) => Some(ClaimOutcome::ProvedBy(
            "finite sums are linear: both sides are the same combination of the same basic sums, so it holds for every n"
                .into(),
        )),
        LineOut::Verdict(Verdict::Holds { points }) => Some(ClaimOutcome::Numeric(format!(
            "both sides are combinations of the same basic sums that agree at {points} random points"
        ))),
        _ => None,
    }
}

/// `text` with every sum over a family replaced by a combination of basic sums, each
/// written as a reserved symbol (one per basic sum, recorded in `atoms`).
fn atomise(text: &str, k: &Knowledge, atoms: &mut Vec<String>) -> Result<String, String> {
    atomise_within(text, k, atoms, &[])
}

/// [`atomise`] inside sums over the indices `outer`, which a sum's bounds must not use
/// (a basic sum whose range depends on an outer index is not a constant).
fn atomise_within(
    text: &str,
    k: &Knowledge,
    atoms: &mut Vec<String>,
    outer: &[String],
) -> Result<String, String> {
    let sums = find_sums(text, k)?;
    let mut out = text.to_string();
    for s in sums.iter().rev() {
        if bare_index(&s.body, &s.index) {
            return Err("the index is used outside a subscript".into());
        }
        let bounds = format!("{} {}", s.from, s.to);
        let (bn, _) = parse_expr(&bounds, 0);
        if notation::names(&bn, &bounds)
            .iter()
            .any(|n| outer.contains(n) || *n == s.index)
        {
            return Err("a sum's range depends on an outer index".into());
        }
        let mut inner = outer.to_vec();
        inner.push(s.index.clone());
        let body = atomise_within(&s.body, k, atoms, &inner)?;
        let replacement = atomise_sum(&body, s, atoms)?;
        out.replace_range(s.span.start..s.span.end, &wrap(&replacement));
    }
    Ok(out)
}

fn atomise_sum(body: &str, s: &FamilySum, atoms: &mut Vec<String>) -> Result<String, String> {
    let value = match cas::run_one(&[], format!("{} = ?", wrap(body))) {
        LineOut::Value(v) => v,
        LineOut::Error(e) => return Err(e),
        LineOut::Verdict(_) => return Err("the sum's terms were not read as a value".into()),
    };
    let expanded = mpp_core::expand(&value).map_err(|e| e.to_string())?;
    let terms: Vec<Expr> = match expanded.node() {
        ExprNode::Add(ts) => ts.clone(),
        _ => vec![expanded.clone()],
    };
    let mut parts = Vec::new();
    for t in terms {
        let factors: Vec<Expr> = match t.node() {
            ExprNode::Mul(fs) => fs.clone(),
            _ => vec![t.clone()],
        };
        let (dep, indep): (Vec<Expr>, Vec<Expr>) = factors.into_iter().partition(|f| {
            f.free_symbols()
                .iter()
                .any(|sym| member_base(sym, &s.index).is_some())
        });
        let coef = wrap(&mpp_core::mul(indep).to_latex());
        if dep.is_empty() {
            parts.push(format!(
                "{coef} \\left({} - {} + 1\\right)",
                wrap(&s.to),
                wrap(&s.from)
            ));
            continue;
        }
        let mono = mpp_core::mul(dep);
        // The same basic sum whatever its index is called.
        let renames: Vec<(String, Expr)> = mono
            .free_symbols()
            .iter()
            .filter_map(|sym| {
                member_base(sym, &s.index).map(|b| (sym.to_string(), Expr::sym(&format!("{b}_#"))))
            })
            .collect();
        let pairs: Vec<(&str, Expr)> = renames
            .iter()
            .map(|(a, b)| (a.as_str(), b.clone()))
            .collect();
        let key = format!("{}|{}|{}", mono.subs_all(&pairs), s.from, s.to);
        let n = match atoms.iter().position(|a| *a == key) {
            Some(n) => n,
            None => {
                atoms.push(key);
                atoms.len() - 1
            }
        };
        parts.push(format!("{coef} {ATOM}_{{{}}}", n + 1));
    }
    Ok(if parts.is_empty() {
        "0".into()
    } else {
        parts.join(" + ")
    })
}

// ---- 3. small cases and random families -------------------------------------------

/// `text` with every sum over a family written out for families of length `size`. The
/// members it writes (`a_{3}`) are added to `members`, the names in the bounds to
/// `sizes`.
fn expand_at(
    text: &str,
    size: i64,
    k: &Knowledge,
    members: &mut BTreeSet<String>,
    sizes: &mut BTreeSet<String>,
) -> Result<String, String> {
    let mut cur = text.to_string();
    for _depth in 0..4 {
        let sums = find_sums(&cur, k)?;
        if sums.is_empty() {
            return Ok(cur);
        }
        let mut out = cur.clone();
        for s in sums.iter().rev() {
            let lo = bound(&s.from, size, sizes)?;
            let hi = bound(&s.to, size, sizes)?;
            if hi - lo >= MAX_TERMS {
                return Err("the sum has too many terms to write out".into());
            }
            let mut terms = Vec::new();
            for j in lo..=hi {
                terms.push(wrap(&substitute(&s.body, &s.index, j, members)?));
            }
            let replacement = if terms.is_empty() {
                "0".to_string()
            } else {
                terms.join(" + ")
            };
            out.replace_range(s.span.start..s.span.end, &wrap(&replacement));
        }
        cur = out;
    }
    Err("the sums are nested too deeply".into())
}

/// A bound for families of length `size`: every name in it stands for the length.
fn bound(text: &str, size: i64, sizes: &mut BTreeSet<String>) -> Result<i64, String> {
    if let Ok(v) = text.trim().parse::<i64>() {
        return Ok(v);
    }
    let (node, _) = parse_expr(text, 0);
    let names = notation::names(&node, text);
    if names.len() != 1 {
        return Err(format!("the bound ${text}$ is not a length"));
    }
    let defs: Vec<Def> = names
        .iter()
        .map(|n| Def::new(n.clone(), size.to_string()))
        .collect();
    sizes.extend(names);
    let v = cas::run_one(&defs, format!("{} = ?", wrap(text)))
        .real()
        .ok_or_else(|| format!("the bound ${text}$ has no value"))?;
    if v.fract() != 0.0 || v.abs() > 1e6 {
        return Err(format!("the bound ${text}$ is not a whole number"));
    }
    Ok(v as i64)
}

/// `body` with the index set to `j`: `a_i` becomes `a_{j}` and a bare `i` becomes `(j)`.
fn substitute(
    body: &str,
    index: &str,
    j: i64,
    members: &mut BTreeSet<String>,
) -> Result<String, String> {
    let (node, diags) = parse_expr(body, 0);
    if diags.iter().any(|d| d.severity == Severity::Error) {
        return Err(format!("${body}$ could not be read"));
    }
    let mut edits: Vec<(Span, String)> = Vec::new();
    notation::walk(&node, &mut |n| match &n.kind {
        NodeKind::Name(name) => {
            if let Some(sub) = &name.sub {
                if sub_key(notation::text(body, sub.span)) == index {
                    let base = member_base_text(n, body);
                    let member = format!("{base}_{{{j}}}");
                    let whole = notation::node_text(n, body);
                    edits.push((
                        Span::new(n.span.start, n.span.start + whole.len()),
                        member.clone(),
                    ));
                    members.insert(member);
                }
            } else if notation::text(body, n.span).trim() == index {
                edits.push((n.span, format!("({j})")));
            }
        }
        NodeKind::Constant(_) if notation::text(body, n.span).trim() == index => {
            edits.push((n.span, format!("({j})")));
        }
        _ => {}
    });
    // Keep the outermost edit where two overlap (a member's subscript is a name too).
    edits.sort_by_key(|(s, _)| (s.start, std::cmp::Reverse(s.end)));
    let mut kept: Vec<(Span, String)> = Vec::new();
    for e in edits {
        if kept.last().is_some_and(|(s, _)| e.0.start < s.end) {
            continue;
        }
        kept.push(e);
    }
    let mut out = body.to_string();
    for (span, t) in kept.iter().rev() {
        out.replace_range(span.start..span.end, t);
    }
    Ok(out)
}

/// The claim written out for families of length `size`: both sides and the definitions.
struct Instance {
    lhs: String,
    rhs: String,
    defs: Vec<Def>,
    members: BTreeSet<String>,
}

fn instance(claim: &FamilyClaim<'_>, k: &Knowledge, size: i64) -> Result<Instance, String> {
    let mut members = BTreeSet::new();
    let mut sizes = BTreeSet::new();
    let lhs = expand_at(claim.lhs, size, k, &mut members, &mut sizes)?;
    let rhs = expand_at(claim.rhs, size, k, &mut members, &mut sizes)?;
    let mut defs = claim_defs(claim, k, &mut |body| {
        expand_at(body, size, k, &mut members, &mut sizes)
    })?;
    let defined: BTreeSet<String> = defs.iter().map(|d| d.name.clone()).collect();
    let mut all: Vec<Def> = sizes
        .iter()
        .filter(|n| !defined.contains(*n))
        .map(|n| Def::new(n.clone(), size.to_string()))
        .collect();
    all.append(&mut defs);
    Ok(Instance {
        lhs,
        rhs,
        defs: all,
        members,
    })
}

fn instances(claim: &FamilyClaim<'_>, k: &Knowledge) -> ClaimOutcome {
    let mut symbolic: Vec<i64> = Vec::new();
    if claim.op == ClaimOp::Eq {
        for size in SMALL_N {
            let inst = match instance(claim, k, size) {
                Ok(i) => i,
                Err(e) => return ClaimOutcome::Unverified(e),
            };
            let line = format!(
                "{} \\overset{{?}}{{=}} {}",
                wrap(&inst.lhs),
                wrap(&inst.rhs)
            );
            match cas::run_one(&inst.defs, line) {
                LineOut::Verdict(Verdict::Proved) => symbolic.push(size),
                LineOut::Verdict(v @ (Verdict::Fails { .. } | Verdict::Mismatch(_))) => {
                    return wrong_or_unverified(
                        claim,
                        k,
                        format!(
                            "for n = {size}: {}",
                            v.describe().trim_start_matches("✗ ").trim()
                        ),
                    );
                }
                _ => {}
            }
        }
    }
    match families(claim, k) {
        Ok(agreed) if agreed >= 3 => {
            let random = format!("on {agreed} random families of length up to {FAMILY_MAX_N}");
            ClaimOutcome::Numeric(if symbolic.len() == SMALL_N.count() {
                format!("written out for n = 1, …, 4 and proved symbolically for those, and checked numerically {random} (evidence for every n, not a proof)")
            } else {
                format!("checked numerically {random} (evidence for every n, not a proof)")
            })
        }
        Ok(_) => ClaimOutcome::Unverified(
            "too few random families met the conditions known so far".into(),
        ),
        Err(Failure::Wrong(at)) => wrong_or_unverified(claim, k, at),
        Err(Failure::Unknown(e)) => ClaimOutcome::Unverified(e),
    }
}

enum Failure {
    Wrong(String),
    Unknown(String),
}

/// A counterexample is ✗ only when nothing the checker cannot use bears on the claim: no
/// earlier equation shares its names, and every name in it was introduced for all values.
fn wrong_or_unverified(claim: &FamilyClaim<'_>, k: &Knowledge, at: String) -> ClaimOutcome {
    let reach = k.closure(claim.names);
    let touched = k.equations.iter().any(|e| !e.names.is_disjoint(&reach));
    let free: Vec<&String> = reach
        .iter()
        .filter(|n| !k.defs.iter().any(|(d, _)| &d.name == *n))
        .filter(|n| !n.contains('_'))
        .collect();
    let universal = free.iter().all(|n| k.universal.contains(*n));
    if !touched && universal {
        ClaimOutcome::Failed(at)
    } else {
        ClaimOutcome::Unverified(format!(
            "the sides differ ({at}), but the claim may rest on facts the checker cannot use"
        ))
    }
}

/// Evaluates the claim on random families of random length, skipping families that break
/// a condition known so far. Returns how many agreed.
fn families(claim: &FamilyClaim<'_>, k: &Knowledge) -> Result<usize, Failure> {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut agreed = 0usize;
    let mut last = String::from("no random family could be evaluated");
    for _ in 0..FAMILY_TRIES {
        if agreed >= FAMILY_WANTED {
            break;
        }
        let size = 1 + rng.below(FAMILY_MAX_N) as i64;
        let inst = instance(claim, k, size).map_err(Failure::Unknown)?;
        let defined: BTreeSet<&String> = inst.defs.iter().map(|d| &d.name).collect();
        // Free names: the members written out, and the claim's other names that are not
        // defined, not lengths and not family members as written (`a_i`).
        let mut free: BTreeSet<String> = inst.members.clone();
        for n in k.closure(claim.names) {
            if !defined.contains(&n) && !is_template(&n) {
                free.insert(n);
            }
        }
        let point: Vec<Def> = free
            .iter()
            .map(|n| {
                let u = rng.below(10_000) as f64 / 10_000.0;
                Def::new(n.clone(), format!("{:.4}", -3.0 + 6.0 * u))
            })
            .collect();
        let known: BTreeSet<&String> = free.iter().chain(defined.iter().copied()).collect();
        let conditions: Vec<_> = k
            .conditions
            .iter()
            .filter(|c| {
                !matches!(c.op, ClaimOp::Congruent | ClaimOp::Divides)
                    && !c.names.is_empty()
                    && c.names.iter().all(|n| known.contains(n))
            })
            .collect();
        let mut defs = point.clone();
        defs.extend(inst.defs.iter().cloned());
        let mut lines = vec![
            format!("{} = ?", wrap(&inst.lhs)),
            format!("{} = ?", wrap(&inst.rhs)),
        ];
        for c in &conditions {
            lines.push(format!("{} = ?", wrap(&c.lhs)));
            lines.push(format!("{} = ?", wrap(&c.rhs)));
        }
        let out = cas::run(&defs, &lines);
        let meets = conditions.iter().enumerate().all(|(ci, c)| {
            match (
                out.get(2 + 2 * ci).and_then(LineOut::real),
                out.get(3 + 2 * ci).and_then(LineOut::real),
            ) {
                (Some(a), Some(b)) => holds(c.op, a, b, None),
                _ => true,
            }
        });
        if !meets {
            continue;
        }
        let (Some(l), Some(r)) = (
            out.first().and_then(LineOut::real),
            out.get(1).and_then(LineOut::real),
        ) else {
            if let Some(LineOut::Error(e)) = out.first().or(out.get(1)) {
                last = e.clone();
            }
            continue;
        };
        if holds(claim.op, l, r, None) {
            agreed += 1;
        } else {
            let at: Vec<String> = point
                .iter()
                .map(|d| format!("{} = {}", d.name, d.body))
                .collect();
            return Err(Failure::Wrong(format!(
                "for n = {size} at {}: left side {}, right side {}",
                at.join(", "),
                fmt(l),
                fmt(r)
            )));
        }
    }
    if agreed == 0 {
        return Err(Failure::Unknown(last));
    }
    Ok(agreed)
}

/// True for a family member as written in a sum (`a_i`, `b_{j}`): a subscript that is a
/// single letter.
pub(crate) fn is_template(name: &str) -> bool {
    name.split_once('_').is_some_and(|(b, s)| {
        let s = sub_key(s);
        !b.is_empty() && s.chars().count() == 1 && s.chars().all(char::is_alphabetic)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algebra::Equation;

    fn names(list: &[&str]) -> BTreeSet<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn reals() -> Knowledge {
        Knowledge {
            reals: names(&["a_1", "a_n", "b_1", "b_n", "x"]),
            families: vec![
                ("a".into(), "1".into(), "n".into()),
                ("b".into(), "1".into(), "n".into()),
            ],
            ..Knowledge::default()
        }
    }

    fn defined(k: &mut Knowledge, name: &str, body: &str, uses: &[&str]) {
        k.defs.push((Def::new(name, body), names(uses)));
        k.indexed.insert(name.into());
    }

    #[test]
    fn expanding_a_square_is_proved_for_every_n() {
        let mut k = reals();
        defined(&mut k, "A", "\\sum a_i^2", &["a_i"]);
        defined(&mut k, "B", "\\sum b_i^2", &["b_i"]);
        defined(&mut k, "C", "\\sum a_i b_i", &["a_i", "b_i"]);
        let ns = names(&["x", "A", "B", "C"]);
        let c = FamilyClaim {
            lhs: "\\sum_{i=1}^{n} (a_i x + b_i)^2",
            op: ClaimOp::Eq,
            rhs: "A x^2 + 2 C x + B",
            names: &ns,
        };
        assert!(matches!(check(&c, &k), ClaimOutcome::ProvedBy(_)));
        // A wrong expansion is not proved.
        let c = FamilyClaim {
            rhs: "A x^2 + C x + B",
            ..c
        };
        assert!(!matches!(
            check(&c, &k),
            ClaimOutcome::ProvedBy(_) | ClaimOutcome::Proved | ClaimOutcome::Numeric(_)
        ));
    }

    #[test]
    fn lagrange_identity_is_proved_by_linearity() {
        // (Σ a_i²)(Σ b_i²) − (Σ a_i b_i)² = ½ Σ_i Σ_j (a_i b_j − a_j b_i)²: the double sum
        // splits into products of the same basic sums, so it holds for every n (✓).
        let k = reals();
        let ns = names(&["n"]);
        let c = FamilyClaim {
            lhs: "\\left(\\sum_{i=1}^{n} a_i^2\\right) \\left(\\sum_{i=1}^{n} b_i^2\\right) - \\left(\\sum_{i=1}^{n} a_i b_i\\right)^2",
            op: ClaimOp::Eq,
            rhs: "\\frac{1}{2} \\sum_{i=1}^{n} \\sum_{j=1}^{n} (a_i b_j - a_j b_i)^2",
            names: &ns,
        };
        let out = check(&c, &k);
        assert!(matches!(&out, ClaimOutcome::ProvedBy(_)), "{out:?}");
    }

    #[test]
    fn small_cases_and_random_families_give_evidence_only() {
        // The index outside a subscript rules out linearity: n = 1…4 and random families.
        let k = reals();
        let ns = names(&["n"]);
        let c = FamilyClaim {
            lhs: "\\sum_{i=1}^{n} (i a_i + a_i)",
            op: ClaimOp::Eq,
            rhs: "\\sum_{i=1}^{n} (i + 1) a_i",
            names: &ns,
        };
        let out = check(&c, &k);
        assert!(matches!(&out, ClaimOutcome::Numeric(_)), "{out:?}");
        // Cauchy–Schwarz itself: random families agree, which is ≈, never ✓.
        let c = FamilyClaim {
            lhs: "\\left(\\sum_{i=1}^{n} a_i b_i\\right)^2",
            op: ClaimOp::Le,
            rhs: "\\left(\\sum_{i=1}^{n} a_i^2\\right) \\left(\\sum_{i=1}^{n} b_i^2\\right)",
            names: &ns,
        };
        let out = check(&c, &k);
        assert!(matches!(&out, ClaimOutcome::Numeric(_)), "{out:?}");
        // A wrong one is caught at a small n.
        let c = FamilyClaim {
            lhs: "\\sum_{i=1}^{n} (i a_i + a_i)",
            op: ClaimOp::Eq,
            rhs: "\\sum_{i=1}^{n} i a_i",
            names: &ns,
        };
        let out = check(&c, &k);
        assert!(
            !matches!(&out, ClaimOutcome::Numeric(_) | ClaimOutcome::ProvedBy(_)),
            "{out:?}"
        );
    }

    #[test]
    fn real_squares() {
        let mut k = reals();
        let ns = names(&["x"]);
        let c = FamilyClaim {
            lhs: "0",
            op: ClaimOp::Le,
            rhs: "\\sum_{i=1}^{n} (a_i x + b_i)^2",
            names: &ns,
        };
        assert!(matches!(check(&c, &k), ClaimOutcome::ProvedBy(_)));
        // Not known to be real: no rule.
        k.reals.clear();
        assert!(real_squares_nonnegative(&c, &k).is_none());

        let mut k = reals();
        defined(&mut k, "A", "\\sum a_i^2", &["a_i"]);
        k.equations.push(Equation {
            lhs: "A".into(),
            rhs: "0".into(),
            names: names(&["A"]),
            defines: None,
        });
        let ns = names(&["a_i"]);
        let c = FamilyClaim {
            lhs: "a_i",
            op: ClaimOp::Eq,
            rhs: "0",
            names: &ns,
        };
        assert!(matches!(check(&c, &k), ClaimOutcome::ProvedBy(_)));
    }

    #[test]
    fn a_false_family_inequality_is_found() {
        let k = reals();
        let ns = names(&[]);
        let c = FamilyClaim {
            lhs: "\\sum_{i=1}^{n} a_i",
            op: ClaimOp::Ge,
            rhs: "0",
            names: &ns,
        };
        assert!(!matches!(
            check(&c, &k),
            ClaimOutcome::ProvedBy(_) | ClaimOutcome::Proved | ClaimOutcome::Numeric(_)
        ));
    }
}
