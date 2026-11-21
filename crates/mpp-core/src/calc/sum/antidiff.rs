//! Antidifferences: a `T(k)` with `T(k + 1) − T(k) = t(k)`, so that
//! `Σ_{k=a}^{b} t(k) = T(b + 1) − T(a)` for any bounds, symbolic ones included.

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Zero};

use super::rational::{as_ratfun, partial_fractions, PartialFractions};
use super::special::bernoulli_numbers;
use super::{shift, split_constant};
use crate::expr::{Expr, ExprNode};
use crate::functions::{func, Func};
use crate::poly::{as_poly, Poly};
use crate::simplify::{add, mul, pow, simplify};

/// Highest polynomial degree summed by Faulhaber's formula.
pub(crate) const MAX_POLY_DEGREE: usize = 60;
/// Largest shift tried when looking for `u(k) − u(k + m)`.
pub(crate) const MAX_TELESCOPE_SHIFT: i64 = 8;
/// Largest integer gap between poles grouped into one telescoping block.
const MAX_POLE_GAP: i64 = 64;

/// An antidifference together with the name of the method that found it.
#[derive(Clone, Debug)]
pub(crate) struct Antidiff {
    pub expr: Expr,
    pub rule: &'static str,
}

/// `Σ_{k=0}^{n−1} k^j` as a polynomial in `n` (Faulhaber's formula).
pub(crate) fn faulhaber(j: usize) -> Option<Poly> {
    let b = bernoulli_numbers(j)?;
    let mut coeffs = vec![BigRational::zero(); j + 2];
    let mut binom = BigInt::one(); // C(j+1, i)
    let jp1 = BigRational::from_integer(BigInt::from(j + 1));
    for (i, bi) in b.iter().enumerate().take(j + 1) {
        coeffs[j + 1 - i] = bi * BigRational::from_integer(binom.clone()) / &jp1;
        binom = binom * BigInt::from(j + 1 - i) / BigInt::from(i + 1);
    }
    Some(Poly::new(coeffs))
}

/// Finds an antidifference of `t` in `k`.
pub(crate) fn antidifference(t: &Expr, k: &str) -> Option<Antidiff> {
    antidiff_depth(t, k, 0)
}

fn antidiff_depth(t: &Expr, k: &str, depth: u32) -> Option<Antidiff> {
    if depth > 8 {
        return None;
    }
    let kk = Expr::sym(k);
    if !t.contains_sym(k) {
        return Some(Antidiff {
            expr: mul(vec![t.clone(), kk]),
            rule: "sum of a constant",
        });
    }
    if let Some(a) = polynomial(t, k) {
        return Some(a);
    }
    let (c, rest) = split_constant(t, k);
    if !c.is_one() {
        let inner = antidiff_depth(&rest, k, depth + 1)?;
        return Some(Antidiff {
            expr: mul(vec![c, inner.expr]),
            rule: inner.rule,
        });
    }
    if let Some(a) = poly_geometric(t, k) {
        return Some(a);
    }
    if let Some(a) = rational(t, k) {
        return Some(a);
    }
    if let Some(a) = telescoping(t, k) {
        return Some(a);
    }
    if let ExprNode::Add(ts) = t.node() {
        let mut parts = Vec::with_capacity(ts.len());
        let rule = "linearity of summation";
        for p in ts {
            let a = antidiff_depth(p, k, depth + 1)?;
            parts.push(a.expr);
        }
        return Some(Antidiff {
            expr: add(parts),
            rule,
        });
    }
    None
}

/// Polynomial terms by Faulhaber's formula.
fn polynomial(t: &Expr, k: &str) -> Option<Antidiff> {
    let coeffs = as_poly(t, k)?;
    if coeffs.len() > MAX_POLY_DEGREE + 1 {
        return None;
    }
    let mut parts = Vec::with_capacity(coeffs.len());
    for (j, c) in coeffs.iter().enumerate() {
        if c.is_zero() {
            continue;
        }
        parts.push(mul(vec![c.clone(), faulhaber(j)?.to_expr(k)]));
    }
    Some(Antidiff {
        expr: add(parts),
        rule: "Faulhaber's formula",
    })
}

/// Splits `t` into its polynomial factors and the rest.
fn split_poly_part(t: &Expr, k: &str) -> (Expr, Expr) {
    let factors = match t.node() {
        ExprNode::Mul(fs) => fs.clone(),
        _ => vec![t.clone()],
    };
    let (p, g): (Vec<Expr>, Vec<Expr>) = factors
        .into_iter()
        .partition(|f| f.is_polynomial_in(k) && !is_power_with_index_exponent(f, k));
    (mul(p), mul(g))
}

fn is_power_with_index_exponent(f: &Expr, k: &str) -> bool {
    matches!(f.node(), ExprNode::Pow(_, x) if x.contains_sym(k))
}

/// The ratio `g(k + 1)/g(k)` when it does not depend on `k`.
pub(crate) fn constant_ratio(g: &Expr, k: &str) -> Option<Expr> {
    if !g.contains_sym(k) {
        return None;
    }
    let r = simplify(&mul(vec![shift(g, k, 1), pow(g.clone(), Expr::int(-1))]));
    (!r.contains_sym(k) && !r.is_undefined() && !r.is_zero()).then_some(r)
}

/// `p(k) · g(k)` with `g(k + 1) = r · g(k)`, `r ≠ 1`: the antidifference is `q(k) g(k)` with
/// `r q(k + 1) − q(k) = p(k)`.
pub(crate) fn poly_geometric_parts(t: &Expr, k: &str) -> Option<(Vec<Expr>, Expr, Expr)> {
    let (p, g) = split_poly_part(t, k);
    let r = constant_ratio(&g, k)?;
    if r.is_one() {
        return None;
    }
    let pc = as_poly(&p, k)?;
    if pc.is_empty() || pc.len() > MAX_POLY_DEGREE + 1 {
        return None;
    }
    let d = pc.len() - 1;
    let r_minus_1 = add(vec![r.clone(), Expr::int(-1)]);
    let inv = pow(r_minus_1, Expr::int(-1));
    let mut q = vec![Expr::zero(); d + 1];
    for j in (0..=d).rev() {
        let mut acc = vec![pc[j].clone()];
        let mut binom = BigInt::one();
        // Σ_{i>j} C(i, j) q_i, with C(i, j) built up as i grows.
        for (i, qi) in q.iter().enumerate().skip(j + 1) {
            binom = binom * BigInt::from(i) / BigInt::from(i - j);
            acc.push(mul(vec![
                Expr::int(-1),
                r.clone(),
                Expr::bigint(binom.clone()),
                qi.clone(),
            ]));
        }
        q[j] = simplify(&mul(vec![add(acc), inv.clone()]));
    }
    Some((q, g, r))
}

fn poly_geometric(t: &Expr, k: &str) -> Option<Antidiff> {
    let (q, g, _) = poly_geometric_parts(t, k)?;
    let kk = Expr::sym(k);
    let qk = add(q
        .iter()
        .enumerate()
        .map(|(j, c)| mul(vec![c.clone(), pow(kk.clone(), Expr::int(j as i64))]))
        .collect());
    let rule = if q.len() == 1 {
        "geometric series"
    } else {
        "arithmetico-geometric series"
    };
    Some(Antidiff {
        expr: mul(vec![qk, g]),
        rule,
    })
}

/// `t = u(k) − u(k + m)`: returns `(u, m)`.
pub(crate) fn telescoping_parts(t: &Expr, k: &str) -> Option<(Expr, i64)> {
    let ExprNode::Add(ts) = t.node() else {
        return None;
    };
    if ts.len() != 2 {
        return None;
    }
    for (u, v) in [(&ts[0], &ts[1]), (&ts[1], &ts[0])] {
        for m in 1..=MAX_TELESCOPE_SHIFT {
            if simplify(&add(vec![shift(u, k, m), v.clone()])).is_zero() {
                return Some((u.clone(), m));
            }
        }
    }
    None
}

fn telescoping(t: &Expr, k: &str) -> Option<Antidiff> {
    let (u, m) = telescoping_parts(t, k)?;
    // T(k) = −Σ_{j=0}^{m−1} u(k + j)
    let parts = (0..m).map(|j| mul(vec![Expr::int(-1), shift(&u, k, j)]));
    Some(Antidiff {
        expr: add(parts.collect()),
        rule: "telescoping series",
    })
}

/// Partial fractions with simple poles. Poles an integer apart telescope; a block whose
/// coefficients do not cancel keeps a digamma function (a harmonic number).
fn rational(t: &Expr, k: &str) -> Option<Antidiff> {
    let r = as_ratfun(t, k)?;
    if r.den.degree().unwrap_or(0) == 0 {
        return None;
    }
    let pf = partial_fractions(&r)?;
    let expr = rational_antidiff(&pf, k)?;
    Some(Antidiff {
        expr,
        rule: "partial fractions",
    })
}

/// A block of poles an integer apart: the largest root `ρ` and `(mᵢ, cᵢ)` for each member.
pub(crate) type PoleGroup = (BigRational, Vec<(i64, BigRational)>);

/// Groups poles whose roots differ by integers: `(ρ, [(m_i, c_i)])` with `r_i = ρ − m_i`,
/// `m_i ≥ 0`.
pub(crate) fn pole_groups(poles: &[(BigRational, BigRational)]) -> Option<Vec<PoleGroup>> {
    let mut groups: Vec<(BigRational, Vec<(BigRational, BigRational)>)> = Vec::new();
    for (r, c) in poles {
        match groups
            .iter_mut()
            .find(|(rep, _)| (rep.clone() - r).is_integer())
        {
            Some((_, members)) => members.push((r.clone(), c.clone())),
            None => groups.push((r.clone(), vec![(r.clone(), c.clone())])),
        }
    }
    let mut out = Vec::with_capacity(groups.len());
    for (_, members) in groups {
        let rho = members.iter().map(|(r, _)| r.clone()).max()?;
        let mut shifted = Vec::with_capacity(members.len());
        for (r, c) in members {
            let m = num_traits::ToPrimitive::to_i64(&(&rho - &r).to_integer())?;
            if m > MAX_POLE_GAP {
                return None;
            }
            shifted.push((m, c));
        }
        out.push((rho, shifted));
    }
    Some(out)
}

fn rational_antidiff(pf: &PartialFractions, k: &str) -> Option<Expr> {
    let kk = Expr::sym(k);
    let mut parts = Vec::new();
    if !pf.poly.is_zero() {
        parts.push(polynomial(&pf.poly.to_expr(k), k)?.expr);
    }
    let mut simple = Vec::new();
    for pole in &pf.poles {
        if pole.coeffs.len() > 1 && pole.coeffs[1..].iter().any(|c| !c.is_zero()) {
            return None; // repeated poles need polygamma functions
        }
        simple.push((pole.root.clone(), pole.coeffs[0].clone()));
    }
    for (rho, members) in pole_groups(&simple)? {
        // A(j) = 1/(j − ρ); 1/(k − r_i) = A(k + m_i).
        let a_of = |j: Expr| pow(add(vec![j, Expr::ratio(-rho.clone())]), Expr::int(-1));
        let total: BigRational = members.iter().map(|(_, c)| c.clone()).sum();
        if !total.is_zero() {
            // Σ A(k) has antidifference ψ(k − ρ).
            parts.push(mul(vec![
                Expr::ratio(total),
                func(
                    Func::Digamma,
                    vec![add(vec![kk.clone(), Expr::ratio(-rho.clone())])],
                ),
            ]));
        }
        // A(k + m) − A(k) has antidifference Σ_{j=0}^{m−1} A(k + j).
        for (m, c) in members {
            for j in 0..m {
                parts.push(mul(vec![
                    Expr::ratio(c.clone()),
                    a_of(add(vec![kk.clone(), Expr::int(j)])),
                ]));
            }
        }
    }
    Some(add(parts))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k() -> Expr {
        Expr::sym("k")
    }

    fn check(t: &Expr) {
        let a = antidifference(t, "k").expect("an antidifference");
        let diff = simplify(&add(vec![shift(&a.expr, "k", 1), -a.expr.clone()]));
        for kv in [3.0, 4.5, 7.0] {
            let lhs = diff.eval_f64(&[("k", kv)]).unwrap_or(f64::NAN);
            let rhs = t.eval_f64(&[("k", kv)]).unwrap_or(f64::NAN);
            assert!(
                (lhs - rhs).abs() < 1e-9 * rhs.abs().max(1.0),
                "{t}: {lhs} vs {rhs}"
            );
        }
    }

    #[test]
    fn faulhaber_polynomials() {
        let f2 = faulhaber(2).expect("small degree");
        // Σ_{k<n} k² = n³/3 − n²/2 + n/6
        assert_eq!(
            f2.to_expr("n"),
            Expr::sym("n").pow(3) / Expr::int(3) - Expr::sym("n").pow(2) / Expr::int(2)
                + Expr::sym("n") / Expr::int(6)
        );
    }

    #[test]
    fn antidifferences_difference_back() {
        let cases = vec![
            k().pow(3),
            Expr::int(2).pow(k()),
            k() * Expr::rational(1, 3).pow(k()),
            (k() * (k() + Expr::one())).recip(),
            (k() * (k() + Expr::int(2))).recip(),
            (k() + Expr::one()).sqrt() - k().sqrt(),
            k().recip(),
            (Expr::int(2) * k() + Expr::one()).recip()
                - (Expr::int(2) * k() + Expr::int(3)).recip(),
        ];
        for t in cases {
            check(&t);
        }
    }
}
