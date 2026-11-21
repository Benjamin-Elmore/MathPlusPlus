//! Factoring: integer polynomials in one variable and common factors of sums.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use num_rational::BigRational;
use num_traits::{One, Signed, Zero};

use crate::expr::{Expr, ExprNode};
use crate::functions;
use crate::number::{gcd_rational, Number};
use crate::poly::Poly;
use crate::printer::display_order;
use crate::simplify::{add, mul, pow, simplify, split_coeff, split_pow};

/// Factors `e` as far as this version can: rational polynomials in one variable are split
/// into linear and irreducible factors over Q; other sums lose their common factors.
pub fn factor(e: &Expr) -> Expr {
    factor_node(&simplify(e))
}

fn factor_node(e: &Expr) -> Expr {
    match e.node() {
        ExprNode::Add(_) => factor_sum(e),
        ExprNode::Mul(fs) => mul(fs.iter().map(factor_node).collect()),
        ExprNode::Pow(b, x) => pow(factor_node(b), x.clone()),
        ExprNode::Func(f, args) => functions::func(*f, args.iter().map(factor_node).collect()),
        _ => e.clone(),
    }
}

fn factor_sum(e: &Expr) -> Expr {
    let syms = e.free_symbols();
    if syms.len() == 1 {
        if let Some(var) = syms.iter().next() {
            if let Some(p) = Poly::from_expr(e, var) {
                if p.degree().is_some_and(|d| d >= 1) {
                    return factor_univariate(&p, var);
                }
            }
        }
    }
    if let Some(f) = factor_homogeneous(e) {
        return f;
    }
    let ExprNode::Add(terms) = e.node() else {
        return e.clone();
    };
    match common_factor(terms) {
        Some((outside, inner)) => {
            let inner_factored = if inner.free_symbols().len() == 1 {
                factor_sum_once(&inner)
            } else {
                factor_homogeneous(&simplify(&inner)).unwrap_or(inner)
            };
            mul(vec![outside, inner_factored])
        }
        None => e.clone(),
    }
}

/// Factors a homogeneous polynomial in two names (`x² − y² = (x − y)(x + y)`): factor
/// `p(x, 1)` in one variable and put the powers of `y` back. `None` when `e` is not one.
fn factor_homogeneous(e: &Expr) -> Option<Expr> {
    let syms: Vec<String> = e.free_symbols().iter().map(|s| s.to_string()).collect();
    let [a, b] = syms.as_slice() else {
        return None;
    };
    // Homogeneous of degree d: e(a·t, b·t) = t^d · e(a, b).
    let t = "_homogeneous_t";
    let scaled = e
        .subs(a, &(Expr::sym(a) * Expr::sym(t)))
        .subs(b, &(Expr::sym(b) * Expr::sym(t)));
    let coeffs = crate::expand::expand(&scaled).ok()?.as_poly(t)?;
    let mut nonzero = coeffs
        .iter()
        .enumerate()
        .filter(|(_, c)| !simplify(c).is_zero());
    let (d, _) = nonzero.next()?;
    if nonzero.next().is_some() || d < 2 {
        return None;
    }
    let p = Poly::from_expr(&e.subs(b, &Expr::one()), a)?;
    let (c, factors) = factor_poly(&p);
    if factors.len() < 2 && factors.iter().all(|(_, m)| *m == 1) && p.degree() == Some(d) {
        return None;
    }
    let homogenize = |f: &Poly| -> Expr {
        let k = f.degree().unwrap_or(0);
        add(f
            .coeffs()
            .iter()
            .enumerate()
            .map(|(j, cj)| {
                mul(vec![
                    Expr::ratio(cj.clone()),
                    pow(Expr::sym(a), Expr::int(j as i64)),
                    pow(Expr::sym(b), Expr::int((k - j) as i64)),
                ])
            })
            .collect())
    };
    let mut parts = vec![Expr::ratio(c)];
    let mut used = 0;
    for (f, m) in &factors {
        used += f.degree().unwrap_or(0) * (*m as usize);
        parts.push(pow(homogenize(f), Expr::int(i64::from(*m))));
    }
    if used < d {
        parts.push(pow(Expr::sym(b), Expr::int((d - used) as i64)));
    }
    Some(mul(parts))
}

/// Factors a univariate sum without looking for common factors again.
fn factor_sum_once(e: &Expr) -> Expr {
    let syms = e.free_symbols();
    if let Some(var) = syms.iter().next() {
        if let Some(p) = Poly::from_expr(e, var) {
            if p.degree().is_some_and(|d| d >= 1) {
                return factor_univariate(&p, var);
            }
        }
    }
    e.clone()
}

/// Pulls the rational content and shared powers out of a sum:
/// `6x²y + 9xy² = 3xy · (2x + 3y)`. Returns `None` when nothing is shared.
fn common_factor(terms: &[Expr]) -> Option<(Expr, Expr)> {
    let split: Vec<(Number, Vec<(Expr, Expr)>)> = terms
        .iter()
        .map(|t| {
            let (c, rest) = split_coeff(t);
            let factors = match rest.node() {
                ExprNode::Mul(fs) => fs.iter().map(split_pow).collect(),
                _ if rest.is_one() => Vec::new(),
                _ => vec![split_pow(&rest)],
            };
            (c, factors)
        })
        .collect();
    // Rational content, signed so the first displayed term is positive.
    let mut content = None;
    if split.iter().all(|(c, _)| c.as_rational().is_some()) {
        let mut g: Option<BigRational> = None;
        for (c, _) in &split {
            if let Some(r) = c.as_rational() {
                g = Some(match g {
                    None => r.abs(),
                    Some(acc) => gcd_rational(&acc, r),
                });
            }
        }
        if let Some(mut g) = g.filter(|g| !g.is_zero()) {
            let ordered = display_order(terms);
            if ordered
                .first()
                .is_some_and(|t| split_coeff(t).0.is_negative())
            {
                g = -g;
            }
            content = Some(g);
        }
    }
    // Shared bases with the smallest rational exponent.
    let mut shared: BTreeMap<Expr, BigRational> = BTreeMap::new();
    if let Some((_, first)) = split.first() {
        for (base, exp) in first {
            if let Some(r) = exp.as_number().and_then(Number::as_rational) {
                shared.insert(base.clone(), r.clone());
            }
        }
    }
    for (_, factors) in split.iter().skip(1) {
        shared.retain(|base, min| {
            match factors
                .iter()
                .find(|(b, _)| b == base)
                .and_then(|(_, x)| x.as_number().and_then(Number::as_rational).cloned())
            {
                Some(r) => {
                    if r < *min {
                        *min = r;
                    }
                    true
                }
                None => false,
            }
        });
    }
    shared.retain(|_, r| !r.is_zero());
    let content_trivial = content.as_ref().is_none_or(One::is_one);
    if content_trivial && shared.is_empty() {
        return None;
    }
    let mut outside_factors = Vec::new();
    let mut divide_by = Vec::new();
    if let Some(c) = content.filter(|c| !c.is_one()) {
        divide_by.push(Expr::ratio(c.recip()));
        outside_factors.push(Expr::ratio(c));
    }
    for (base, r) in shared {
        divide_by.push(pow(base.clone(), Expr::ratio(-r.clone())));
        outside_factors.push(pow(base, Expr::ratio(r)));
    }
    let inner = add(terms
        .iter()
        .map(|t| {
            let mut fs = vec![t.clone()];
            fs.extend(divide_by.iter().cloned());
            mul(fs)
        })
        .collect());
    Some((mul(outside_factors), inner))
}

/// Factors a polynomial over Q: content, square-free parts, rational roots.
pub fn factor_poly(p: &Poly) -> (BigRational, Vec<(Poly, u32)>) {
    let mut factors: Vec<(Poly, u32)> = Vec::new();
    for (part, mult) in p.square_free() {
        let mut rest = primitive(&part);
        if let Some(roots) = rest.rational_roots() {
            for r in roots {
                // (q x − p) for the root p/q
                let lin = Poly::new(vec![
                    BigRational::from_integer(-r.numer().clone()),
                    BigRational::from_integer(r.denom().clone()),
                ]);
                if let Ok((quot, rem)) = rest.div_rem(&lin) {
                    if rem.is_zero() {
                        factors.push((lin, mult));
                        rest = primitive(&quot);
                    }
                }
            }
        }
        if rest.degree().is_some_and(|d| d >= 1) {
            for g in split_over_integers(&rest) {
                factors.push((g, mult));
            }
        }
    }
    factors.sort_by(|(a, ma), (b, mb)| {
        a.degree()
            .cmp(&b.degree())
            .then_with(|| ma.cmp(mb))
            .then_with(|| a.coeffs().cmp(b.coeffs()))
    });
    // Whatever is left is the constant: p = c · Π fᵢ^mᵢ.
    let mut lead_product = BigRational::one();
    for (f, m) in &factors {
        for _ in 0..*m {
            lead_product *= f.leading();
        }
    }
    let c = if lead_product.is_zero() {
        p.leading()
    } else {
        p.leading() / lead_product
    };
    (c, factors)
}

/// Highest degree split into non-linear factors (the root finder works in `f64`).
const MAX_SPLIT_DEGREE: usize = 40;
/// Most root groupings tried for one polynomial.
const MAX_GROUPINGS: usize = 200_000;

/// Splits a primitive integer polynomial with no rational roots into factors of degree
/// two or more over Z (`x⁴ + x² + 1 = (x² − x + 1)(x² + x + 1)`).
///
/// Each true factor is `c · Π (x − r)` over a set of the complex roots closed under
/// conjugation, with `c` dividing the leading coefficient `a`; so `a · Π (x − r)` has
/// integer coefficients. The roots are found numerically and every candidate is checked
/// by exact division, so a factor returned is always a factor. A part with no split
/// found within the budget is left as it is.
fn split_over_integers(p: &Poly) -> Vec<Poly> {
    use num_complex::Complex64;
    use num_traits::ToPrimitive;
    let Some(n) = p.degree() else {
        return vec![p.clone()];
    };
    // A quadratic or cubic with no rational root is irreducible over Q.
    if !(4..=MAX_SPLIT_DEGREE).contains(&n) {
        return vec![p.clone()];
    }
    let coeffs: Option<Vec<f64>> = p.coeffs().iter().map(ToPrimitive::to_f64).collect();
    let Some(coeffs) = coeffs.filter(|c| c.iter().all(|x| x.is_finite() && x.abs() < 1e12)) else {
        return vec![p.clone()];
    };
    let cs: Vec<Complex64> = coeffs.iter().map(|&c| Complex64::new(c, 0.0)).collect();
    let Ok(roots) = crate::domains::solve::poly::numeric_roots(&cs) else {
        return vec![p.clone()];
    };
    // Real roots alone, complex roots with their conjugates.
    let mut units: Vec<Vec<Complex64>> = Vec::new();
    let mut used = vec![false; roots.len()];
    for i in 0..roots.len() {
        if used[i] {
            continue;
        }
        used[i] = true;
        let r = roots[i];
        if r.im.abs() <= 1e-7 * r.norm().max(1.0) {
            units.push(vec![Complex64::new(r.re, 0.0)]);
            continue;
        }
        let partner = (0..roots.len()).filter(|&j| !used[j]).min_by(|&j, &k| {
            let dj = (roots[j] - r.conj()).norm();
            let dk = (roots[k] - r.conj()).norm();
            dj.partial_cmp(&dk).unwrap_or(Ordering::Equal)
        });
        match partner {
            Some(j) => {
                used[j] = true;
                units.push(vec![r, r.conj()]);
            }
            None => return vec![p.clone()],
        }
    }
    let lead = coeffs[n];
    let mut budget = MAX_GROUPINGS;
    for d in 2..=n / 2 {
        let mut chosen = Vec::new();
        if let Some(g) = find_factor(p, &units, 0, d, lead, &mut chosen, &mut budget) {
            if let Ok((quot, rem)) = p.div_rem(&g) {
                if rem.is_zero() {
                    let mut out = split_over_integers(&g);
                    out.extend(split_over_integers(&primitive(&quot)));
                    return out;
                }
            }
        }
        if budget == 0 {
            break;
        }
    }
    vec![p.clone()]
}

/// Tries every grouping of `units[from..]` with total degree `d` (the groups already in
/// `chosen` included) and returns the first that gives an exact factor of `p`.
fn find_factor(
    p: &Poly,
    units: &[Vec<num_complex::Complex64>],
    from: usize,
    d: usize,
    lead: f64,
    chosen: &mut Vec<usize>,
    budget: &mut usize,
) -> Option<Poly> {
    let have: usize = chosen.iter().map(|&i| units[i].len()).sum();
    if have == d {
        *budget = budget.saturating_sub(1);
        return candidate(p, units, chosen, lead);
    }
    for i in from..units.len() {
        if *budget == 0 {
            return None;
        }
        if have + units[i].len() > d {
            continue;
        }
        chosen.push(i);
        let found = find_factor(p, units, i + 1, d, lead, chosen, budget);
        chosen.pop();
        if found.is_some() {
            return found;
        }
    }
    None
}

/// The primitive integer polynomial `a · Π (x − r)` over the chosen roots, when its
/// coefficients are integers and it divides `p` exactly.
fn candidate(
    p: &Poly,
    units: &[Vec<num_complex::Complex64>],
    chosen: &[usize],
    lead: f64,
) -> Option<Poly> {
    use num_complex::Complex64;
    let mut m = vec![Complex64::new(1.0, 0.0)];
    for r in chosen.iter().flat_map(|&i| units[i].iter()) {
        // m · (x − r), coefficients from the constant up.
        let mut next = vec![Complex64::new(0.0, 0.0); m.len() + 1];
        for (k, c) in m.iter().enumerate() {
            next[k + 1] += c;
            next[k] -= c * r;
        }
        m = next;
    }
    let mut ints = Vec::with_capacity(m.len());
    for c in &m {
        let v = c.re * lead;
        let rounded = v.round();
        if (v - rounded).abs() > 1e-6 * rounded.abs().max(1.0) || c.im.abs() * lead.abs() > 1e-6 {
            return None;
        }
        ints.push(BigRational::from_integer(num_bigint::BigInt::from(
            rounded as i64,
        )));
    }
    let g = primitive(&Poly::new(ints));
    match p.div_rem(&g) {
        Ok((_, rem)) if rem.is_zero() && g.degree().is_some_and(|k| k >= 1) => Some(g),
        _ => None,
    }
}

fn primitive(p: &Poly) -> Poly {
    let (_, prim) = p.content_primitive();
    Poly::new(prim.into_iter().map(BigRational::from_integer).collect())
}

fn factor_univariate(p: &Poly, var: &str) -> Expr {
    let (c, factors) = factor_poly(p);
    let mut parts = vec![Expr::ratio(c)];
    for (f, m) in factors {
        parts.push(pow(f.to_expr(var), Expr::int(i64::from(m))));
    }
    mul(parts)
}

impl Expr {
    /// Factored form (see [`factor`]).
    pub fn factor(&self) -> Expr {
        factor(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x() -> Expr {
        Expr::sym("x")
    }
    fn y() -> Expr {
        Expr::sym("y")
    }
    fn n(k: i64) -> Expr {
        Expr::int(k)
    }

    #[test]
    fn difference_of_squares() {
        let f = (x().pow(2) - n(1)).factor();
        assert_eq!(f, (x() - n(1)) * (x() + n(1)));
        assert_eq!(f.to_string(), "(x - 1)*(x + 1)");
    }

    #[test]
    fn content_and_repeated_roots() {
        let e = n(2) * x().pow(3) - n(4) * x().pow(2) + n(2) * x();
        assert_eq!(e.factor().to_string(), "2*x*(x - 1)^2");
        let e = (x().pow(2) - n(2) * x() + n(1)).factor();
        assert_eq!(e, (x() - n(1)).pow(2));
    }

    #[test]
    fn rational_roots_and_irreducible_parts() {
        let e = n(6) * x().pow(2) - x() - n(1); // (2x - 1)(3x + 1)
        assert_eq!(e.factor().to_string(), "(2*x - 1)*(3*x + 1)");
        let e = x().pow(3) - n(1);
        assert_eq!(e.factor().to_string(), "(x - 1)*(x^2 + x + 1)");
        let e = x().pow(2) + n(1);
        assert_eq!(e.factor(), e);
        let e = Expr::rational(1, 2) * x().pow(2) - Expr::rational(1, 2);
        assert_eq!(e.factor().to_string(), "(x - 1)*(x + 1)/2");
    }

    #[test]
    fn leading_sign() {
        let e = n(1) - x();
        assert_eq!(e.factor().to_string(), "-(x - 1)");
    }

    #[test]
    fn multivariate_common_factors() {
        let e = n(6) * x().pow(2) * y() + n(9) * x() * y().pow(2);
        assert_eq!(e.factor().to_string(), "3*x*y*(2*x + 3*y)");
        let a = Expr::sym("a");
        let e = a.clone() * x().pow(2) - a.clone();
        assert_eq!(e.factor().to_string(), "a*(x - 1)*(x + 1)");
    }

    #[test]
    fn factor_is_stable_under_simplify() {
        let f = (n(2) * x() + n(2)).factor();
        assert_eq!(f.to_string(), "2*(x + 1)");
        assert_eq!(f.simplify(), f);
    }

    #[test]
    fn factors_inside_quotients() {
        let e = (x().pow(2) - n(1)) / (x() + n(1));
        // Factoring the numerator lets the common factor cancel.
        assert_eq!(e.factor(), x() - n(1));
    }

    #[test]
    fn splits_over_the_integers_beyond_rational_roots() {
        let e = x().pow(6) - n(1);
        assert_eq!(
            e.factor().to_string(),
            "(x - 1)*(x + 1)*(x^2 + x + 1)*(x^2 - x + 1)"
        );
        let e = x().pow(5) + x() + n(1);
        assert_eq!(e.factor().to_string(), "(x^2 + x + 1)*(x^3 - x^2 + 1)");
        let e = x().pow(4) + n(4);
        assert_eq!(e.factor().to_string(), "(x^2 - 2*x + 2)*(x^2 + 2*x + 2)");
        // Irreducible quartic stays whole.
        let e = x().pow(4) + n(1);
        assert_eq!(e.factor(), e);
    }

    #[test]
    fn homogeneous_two_variable_polynomials() {
        let e = x().pow(2) * y() - y().pow(3);
        assert_eq!(e.factor().to_string(), "y*(x + y)*(x - y)");
        let e = x().pow(2) - y().pow(2);
        assert_eq!(e.factor().to_string(), "(x + y)*(x - y)");
    }

    use proptest::prelude::*;

    fn small_poly() -> impl Strategy<Value = Vec<i64>> {
        (1usize..=3).prop_flat_map(|d| {
            (
                prop::collection::vec(-4i64..=4, d),
                prop::sample::select(vec![1i64, 2, 3]),
            )
                .prop_map(|(mut c, lead)| {
                    c.push(lead);
                    c
                })
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]
        #[test]
        fn factors_multiply_back_and_have_no_rational_roots(
            a in small_poly(), b in small_poly(), c in small_poly()
        ) {
            let p = Poly::from_ints(&a).mul(&Poly::from_ints(&b)).mul(&Poly::from_ints(&c));
            prop_assume!(!p.is_zero());
            let (k, factors) = factor_poly(&p);
            let mut back = Poly::constant(k);
            for (f, m) in &factors {
                for _ in 0..*m {
                    back = back.mul(f);
                }
                // A factor of degree 2 or 3 with no rational root is irreducible.
                if f.degree().is_some_and(|d| d >= 2) {
                    prop_assert!(f.rational_roots().is_none_or(|r| r.is_empty()));
                }
            }
            prop_assert_eq!(back, p.clone());
            // Each input factor of degree ≥ 1 is split at least as finely.
            let parts: usize = factors.iter().map(|(_, m)| *m as usize).sum();
            let inputs = [&a, &b, &c].iter().filter(|v| v.len() >= 2).count();
            prop_assert!(parts >= inputs);
        }
    }
}
