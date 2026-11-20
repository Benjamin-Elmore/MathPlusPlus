//! Factoring: integer polynomials in one variable and common factors of sums.

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
    let ExprNode::Add(terms) = e.node() else {
        return e.clone();
    };
    match common_factor(terms) {
        Some((outside, inner)) => {
            let inner_factored = if inner.free_symbols().len() == 1 {
                factor_sum_once(&inner)
            } else {
                inner
            };
            mul(vec![outside, inner_factored])
        }
        None => e.clone(),
    }
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
            factors.push((rest, mult));
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
}
