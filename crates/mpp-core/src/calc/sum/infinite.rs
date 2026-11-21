//! Infinite sums `Σ_{k=a}^{∞} t(k)`: the convergence verdict first, then closed forms, then
//! an accelerated numeric value.

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{Signed, ToPrimitive, Zero};

use super::antidiff::{constant_ratio, poly_geometric_parts, telescoping_parts};
use super::convergence::{convergence, term_ratio};
use super::rational::{as_ratfun, partial_fractions};
use super::special::{digamma_plus_gamma, hurwitz_zeta};
use super::{
    alternating_split, holder, int_value, numeric, shift, split_constant, term_at, value_f64,
    MAX_DIRECT_TERMS,
};
use crate::calc::limit::limit;
use crate::error::{MathError, MathResult};
use crate::expr::{BigOpKind, Expr, ExprNode, LimitDir};
use crate::functions::{func, Func};
use crate::note::{maths, tex};
use crate::simplify::{add, mul, pow, simplify};
use crate::steps::Steps;

/// A closed form: the value, the rule's name and an optional note.
struct Closed {
    value: Expr,
    rule: &'static str,
    note: Option<String>,
}

fn closed(value: Expr, rule: &'static str, note: Option<String>) -> Option<Closed> {
    Some(Closed {
        value: simplify(&value),
        rule,
        note,
    })
}

pub(super) fn infinite_sum(
    t: &Expr,
    k: &str,
    a: &Expr,
    log: &mut Option<&mut Steps>,
) -> MathResult<Expr> {
    let shown = holder(BigOpKind::Sum, t, k, a, &Expr::infinity());
    if t.is_zero() {
        Steps::record(log, "zero series", &shown, &Expr::zero(), None);
        return Ok(Expr::zero());
    }
    let verdict = convergence(t, k, a);
    let verdict_note = Some(verdict.to_string());
    if verdict.converges == Some(false) {
        Steps::record(log, verdict.test.clone(), &shown, &shown, verdict_note);
        return Err(MathError::Divergent(format!(
            "the series {} (by the {})",
            maths(&shown),
            verdict.test
        )));
    }
    if let Some(c) = closed_form(t, k, a, 0)? {
        Steps::record(log, verdict.test.clone(), &shown, &shown, verdict_note);
        Steps::record(log, c.rule, &shown, &c.value, c.note);
        return Ok(c.value);
    }
    let start = int_value(a).and_then(|n| n.to_f64());
    match (verdict.converges, start) {
        (Some(true), Some(start)) if t.free_symbols().iter().all(|s| &**s == k) => {
            let (value, method) = numeric::infinite(t, k, start, &verdict.test)?;
            Steps::record(log, verdict.test.clone(), &shown, &shown, verdict_note);
            let out = Expr::float(value);
            Steps::record(
                log,
                "numeric summation",
                &shown,
                &out,
                Some(method.describe().into()),
            );
            Ok(out)
        }
        (Some(true), _) => Err(MathError::NoClosedForm(maths(&shown))),
        _ => Err(MathError::Inconclusive(format!(
            "whether {} converges ({})",
            maths(&shown),
            verdict.detail
        ))),
    }
}

fn closed_form(t: &Expr, k: &str, a: &Expr, depth: u32) -> MathResult<Option<Closed>> {
    if depth > 6 {
        return Ok(None);
    }
    if t.is_zero() {
        return Ok(closed(Expr::zero(), "zero series", None));
    }
    let (c, rest) = split_constant(t, k);
    if !c.is_one() && rest.contains_sym(k) {
        return Ok(closed_form(&rest, k, a, depth + 1)?.map(|inner| Closed {
            value: simplify(&mul(vec![c, inner.value])),
            ..inner
        }));
    }
    if let Some(found) = geometric(t, k, a)? {
        return Ok(Some(found));
    }
    if let Some(found) = exponential_family(t, k, a)? {
        return Ok(Some(found));
    }
    if let Some(found) = rational_sum(t, k, a)? {
        return Ok(Some(found));
    }
    if let Some(found) = alternating_rational(t, k, a)? {
        return Ok(Some(found));
    }
    if let Some(found) = telescoping(t, k, a) {
        return Ok(Some(found));
    }
    if let Some(value) = super::zeta_series::hurwitz_series(t, k, a) {
        return Ok(closed(value, "Hurwitz zeta function", None));
    }
    if let ExprNode::Add(ts) = t.node() {
        let mut values = Vec::with_capacity(ts.len());
        for p in ts {
            match closed_form(p, k, a, depth + 1)? {
                Some(c) => values.push(c.value),
                None => return Ok(None),
            }
        }
        return Ok(closed(add(values), "linearity of summation", None));
    }
    Ok(None)
}

/// `Σ p(k) r^k` for `|r| < 1` (assumed when `r` is symbolic): `−q(a) g(a)`.
fn geometric(t: &Expr, k: &str, a: &Expr) -> MathResult<Option<Closed>> {
    let Some((q, g, r)) = poly_geometric_parts(t, k) else {
        return Ok(None);
    };
    if let Some(v) = value_f64(&r) {
        if v.abs() >= 1.0 {
            return Err(MathError::Divergent(format!(
                "a geometric series with ratio {}",
                maths(&r)
            )));
        }
    }
    let q_at_a = add(q
        .iter()
        .enumerate()
        .map(|(j, c)| mul(vec![c.clone(), pow(a.clone(), Expr::int(j as i64))]))
        .collect());
    let value = mul(vec![Expr::int(-1), q_at_a, g.subs(k, a)]);
    let symbolic = !r.is_constant();
    let note = if symbolic {
        format!(
            "common ratio {}; valid for {}",
            maths(&r),
            tex(
                &format!("\\left|{}\\right| < 1", r.to_latex()),
                &format!("|{}| < 1", r.to_text())
            )
        )
    } else {
        format!("common ratio {}", maths(&r))
    };
    let rule = if q.len() == 1 {
        "geometric series"
    } else {
        "arithmetico-geometric series"
    };
    Ok(closed(value, rule, Some(note)))
}

/// `√w` when `w` is visibly a square.
fn square_root(w: &Expr) -> Expr {
    if let ExprNode::Pow(b, x) = w.node() {
        if x.as_number().and_then(|n| n.as_i64()) == Some(2) {
            return b.clone();
        }
    }
    pow(w.clone(), Expr::rational(1, 2))
}

/// Series of `exp`, `sin`, `cos`, `sinh` and `cosh`, recognised from the term ratio.
fn exponential_family(t: &Expr, k: &str, a: &Expr) -> MathResult<Option<Closed>> {
    let Some(start) = int_value(a).filter(|n| !n.is_negative()) else {
        return Ok(None);
    };
    if constant_ratio(t, k).is_some() {
        return Ok(None);
    }
    let r = term_ratio(t, k);
    let kk = Expr::sym(k);
    let lin = |c: i64, d: i64| add(vec![mul(vec![Expr::int(c), kk.clone()]), Expr::int(d)]);
    // A rational function of k (numerator and denominator of degree ≤ 4 here) that takes
    // the same value at five points is constant.
    let free = |e: Expr| {
        let first = e.subs(k, &Expr::int(7));
        let same = [11, 13, 17, 23]
            .iter()
            .all(|n| e.subs(k, &Expr::int(*n)) == first);
        (same && !first.contains_sym(k) && !first.is_zero() && !first.is_undefined())
            .then_some(first)
    };
    let zero = BigInt::zero();
    let t0 = || term_at(t, k, &zero);
    // Σ_{k=0}^{a−1} t(k), removed when the sum starts later.
    let head = || -> MathResult<Expr> {
        let mut parts = Vec::new();
        let mut n = BigInt::zero();
        while n < start {
            parts.push(term_at(t, k, &n)?);
            n += 1u32;
        }
        Ok(add(parts))
    };
    if start > BigInt::from(MAX_DIRECT_TERMS) {
        return Ok(None);
    }
    // exp: r = x/(k + 1)
    if let Some(x) = free(mul(vec![r.clone(), lin(1, 1)])) {
        let c = t0()?;
        let value = add(vec![
            mul(vec![c, pow(Expr::e(), x.clone())]),
            mul(vec![Expr::int(-1), head()?]),
        ]);
        return Ok(closed(
            value,
            "exponential series",
            Some(format!("Σ xᵏ/k! = eˣ with x = {x}")),
        ));
    }
    let families: [(i64, i64, i64, &str); 4] = [
        (-1, 2, 3, "sine series"),
        (-1, 1, 2, "cosine series"),
        (1, 2, 3, "hyperbolic sine series"),
        (1, 1, 2, "hyperbolic cosine series"),
    ];
    for (sign, d1, d2, rule) in families {
        // r = sign · w / ((2k + d1)(2k + d2))
        let Some(w) = free(mul(vec![
            Expr::int(sign),
            r.clone(),
            lin(2, d1),
            lin(2, d2),
        ])) else {
            continue;
        };
        let y = square_root(&w);
        let c = t0()?;
        let f = match rule {
            "sine series" => Func::Sin,
            "cosine series" => Func::Cos,
            "hyperbolic sine series" => Func::Sinh,
            _ => Func::Cosh,
        };
        // Odd series start at y, even ones at 1.
        let scale = if d1 == 2 {
            mul(vec![c, pow(y.clone(), Expr::int(-1))])
        } else {
            c
        };
        let value = add(vec![
            mul(vec![scale, func(f, vec![y.clone()])]),
            mul(vec![Expr::int(-1), head()?]),
        ]);
        return Ok(closed(
            value,
            rule,
            Some(format!("the Maclaurin series of {}({y})", f.name())),
        ));
    }
    Ok(None)
}

/// Rational terms by partial fractions: simple poles through the digamma function at
/// rational points (Gauss's theorem), double and higher even-order poles through ζ(2m).
fn rational_sum(t: &Expr, k: &str, a: &Expr) -> MathResult<Option<Closed>> {
    let Some(r) = as_ratfun(t, k) else {
        return Ok(None);
    };
    if r.den.degree().unwrap_or(0) == 0 {
        return Ok(None);
    }
    let Some(pf) = partial_fractions(&r) else {
        return Ok(None);
    };
    let Some(start) = int_value(a) else {
        return Ok(None);
    };
    if !pf.poly.is_zero() {
        return Err(MathError::Divergent(format!(
            "the series of {} (its terms do not tend to 0)",
            maths(t)
        )));
    }
    let simple_total: BigRational = pf.poles.iter().map(|p| p.coeffs[0].clone()).sum();
    if !simple_total.is_zero() {
        return Err(MathError::Divergent(format!(
            "the series of {} (its terms behave like {})",
            maths(t),
            maths(&mul(vec![
                Expr::ratio(simple_total.clone()),
                pow(Expr::sym(k), Expr::int(-1))
            ]))
        )));
    }
    // Start beyond every pole; the first few terms are added exactly.
    let start_q = BigRational::from_integer(start.clone());
    let mut eff = start.clone();
    for p in &pf.poles {
        if p.root.is_integer() && p.root >= start_q {
            return Err(MathError::Undefined(format!(
                "the term at {k} = {}",
                p.root
            )));
        }
        let above = p.root.floor().to_integer() + 1;
        if above > eff {
            eff = above;
        }
    }
    if &eff - &start > BigInt::from(MAX_DIRECT_TERMS) {
        return Ok(None);
    }
    let mut parts = Vec::new();
    let mut n = start.clone();
    while n < eff {
        parts.push(term_at(t, k, &n)?);
        n += 1u32;
    }
    let eff_q = BigRational::from_integer(eff);
    let mut used_zeta = false;
    for p in &pf.poles {
        let x = &eff_q - &p.root;
        for (j, c) in p.coeffs.iter().enumerate() {
            if c.is_zero() {
                continue;
            }
            let value = if j == 0 {
                // Σ_{k≥eff} 1/(k − r) contributes −c ψ(eff − r) (γ cancels overall).
                let Some(psi) = digamma_plus_gamma(&x) else {
                    return Ok(None);
                };
                mul(vec![Expr::ratio(-c.clone()), psi])
            } else {
                let Some(z) = u32::try_from(j + 1).ok().and_then(|m| hurwitz_zeta(m, &x)) else {
                    return Ok(None);
                };
                used_zeta = true;
                mul(vec![Expr::ratio(c.clone()), z])
            };
            parts.push(value);
        }
    }
    let rule = if used_zeta {
        "p-series (ζ(2m) by Bernoulli numbers)"
    } else {
        "partial fractions"
    };
    let note = Some(format!(
        "{} pole{} at {}",
        pf.poles.len(),
        if pf.poles.len() == 1 { "" } else { "s" },
        pf.poles
            .iter()
            .map(|p| format!("{k} = {}", p.root))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    Ok(closed(add(parts), rule, note))
}

/// `Σ (−1)^k f(k)` with rational `f`: pairs consecutive terms into a rational term.
fn alternating_rational(t: &Expr, k: &str, a: &Expr) -> MathResult<Option<Closed>> {
    let Some((sign, f)) = alternating_split(t, k) else {
        return Ok(None);
    };
    let Some(start) = int_value(a) else {
        return Ok(None);
    };
    if as_ratfun(&f, k).is_none() {
        return Ok(None);
    }
    let s0 = sign.subs(k, a);
    if !(s0.is_one() || s0.as_number().is_some_and(|n| n.is_minus_one())) {
        return Ok(None);
    }
    // g(j) = s(a) · (f(a + 2j) − f(a + 2j + 1))
    let j = Expr::sym(k);
    let at = |off: i64| {
        f.subs(
            k,
            &add(vec![
                Expr::bigint(start.clone()),
                Expr::int(off),
                mul(vec![Expr::int(2), j.clone()]),
            ]),
        )
    };
    let g = simplify(&mul(vec![
        s0,
        add(vec![at(0), mul(vec![Expr::int(-1), at(1)])]),
    ]));
    let Some(inner) = rational_sum(&g, k, &Expr::zero())? else {
        return Ok(None);
    };
    Ok(closed(
        inner.value,
        "alternating series (pairing terms)",
        Some(format!("consecutive terms paired into {g}")),
    ))
}

/// `u(k) − u(k + m)`: `Σ_{j<m} u(a + j) − m · lim u`.
fn telescoping(t: &Expr, k: &str, a: &Expr) -> Option<Closed> {
    let (u, m) = telescoping_parts(t, k)?;
    let l = limit(&u, k, &Expr::infinity(), LimitDir::Both, None).ok()?;
    if l.is_infinite() || l.is_undefined() {
        return None;
    }
    let mut parts: Vec<Expr> = (0..m)
        .map(|j| shift(&u, k, j).subs(k, a))
        .collect::<Vec<_>>();
    parts.push(mul(vec![Expr::int(-m), l.clone()]));
    closed(
        add(parts),
        "telescoping series",
        Some(format!(
            "{} with {} → {}",
            tex(
                &format!("a_k = u(k) - u(k + {m})"),
                &format!("aₖ = u(k) − u(k + {m})")
            ),
            maths(&Expr::equation(
                Expr::apply("u", vec![Expr::sym(k)]),
                u.clone()
            )),
            maths(&l)
        )),
    )
}
