//! Expansion: distribute products over sums and multiply out integer powers of sums.

use num_traits::ToPrimitive;

use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode};
use crate::functions;
use crate::simplify::{add, canonical, mul, pow, simplify};

/// Most terms an expansion may produce.
pub const MAX_TERMS: usize = 20_000;

fn terms_of(e: &Expr) -> Vec<Expr> {
    match e.node() {
        ExprNode::Add(v) => v.clone(),
        _ => vec![e.clone()],
    }
}

fn too_large() -> MathError {
    MathError::TooLarge("the expanded form".into())
}

/// Multiplies two expanded expressions term by term.
fn distribute(a: &Expr, b: &Expr) -> MathResult<Expr> {
    let (ta, tb) = (terms_of(a), terms_of(b));
    if ta.len().saturating_mul(tb.len()) > MAX_TERMS {
        return Err(too_large());
    }
    let mut out = Vec::with_capacity(ta.len() * tb.len());
    for x in &ta {
        for y in &tb {
            out.push(mul(vec![x.clone(), y.clone()]));
        }
    }
    Ok(add(out))
}

/// Expands products and integer powers of sums everywhere in `e`.
pub fn expand(e: &Expr) -> MathResult<Expr> {
    expand_node(&simplify(e))
}

fn expand_node(e: &Expr) -> MathResult<Expr> {
    crate::budget::check()?;
    match e.node() {
        ExprNode::Add(v) => Ok(add(v.iter().map(expand_node).collect::<MathResult<_>>()?)),
        ExprNode::Mul(v) => {
            let mut acc = Expr::one();
            for f in v {
                acc = distribute(&acc, &expand_node(f)?)?;
            }
            Ok(acc)
        }
        ExprNode::Pow(b, x) => {
            let base = expand_node(b)?;
            let exp = expand_node(x)?;
            let k = exp.as_number().and_then(|n| n.as_i64());
            match (k, base.node()) {
                (Some(k), ExprNode::Add(terms)) if k != 0 => {
                    let n = k.unsigned_abs();
                    if estimate_terms(terms.len(), n) > MAX_TERMS {
                        return Err(too_large());
                    }
                    let mut acc = Expr::one();
                    let mut sq = base.clone();
                    let mut m = n;
                    // Binary powering keeps the number of multiplications small.
                    while m > 0 {
                        if m & 1 == 1 {
                            acc = distribute(&acc, &sq)?;
                        }
                        m >>= 1;
                        if m > 0 {
                            sq = distribute(&sq, &sq)?;
                        }
                    }
                    Ok(if k < 0 { pow(acc, Expr::int(-1)) } else { acc })
                }
                _ => Ok(pow(base, exp)),
            }
        }
        ExprNode::Func(f, args) => Ok(functions::func(
            *f,
            args.iter().map(expand_node).collect::<MathResult<_>>()?,
        )),
        _ => {
            let mut err = None;
            let node = e.map_children(&mut |c| match expand_node(c) {
                Ok(v) => v,
                Err(x) => {
                    err.get_or_insert(x);
                    c.clone()
                }
            });
            match err {
                Some(x) => Err(x),
                None => Ok(canonical(node)),
            }
        }
    }
}

/// Number of terms of `(t_1 + … + t_m)^n` before collecting: C(n + m − 1, m − 1).
fn estimate_terms(m: usize, n: u64) -> usize {
    let m = m as u64;
    let mut acc: f64 = 1.0;
    for j in 1..m {
        acc *= (n + j) as f64 / j as f64;
        if acc > MAX_TERMS as f64 {
            return MAX_TERMS + 1;
        }
    }
    acc.to_usize().unwrap_or(MAX_TERMS + 1)
}

impl Expr {
    /// Expanded form (see [`expand`]).
    pub fn expand(&self) -> MathResult<Expr> {
        expand(self)
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
    fn binomials() {
        let e = (x() + n(1)).pow(2).expand().unwrap();
        assert_eq!(e, x().pow(2) + n(2) * x() + n(1));
        assert_eq!(e.to_string(), "x^2 + 2*x + 1");
        let e = (x() - y()).pow(3).expand().unwrap();
        assert_eq!(e.to_string(), "x^3 - 3*x^2*y + 3*x*y^2 - y^3");
    }

    #[test]
    fn products_of_sums() {
        let e = ((x() + n(1)) * (x() - n(1))).expand().unwrap();
        assert_eq!(e, x().pow(2) - n(1));
        let e = ((x() + y() + n(1)) * (x() - y())).expand().unwrap();
        assert_eq!(e.to_string(), "x^2 - y^2 + x - y");
    }

    #[test]
    fn negative_powers_expand_the_denominator() {
        let e = (x() + n(1)).pow(-2).expand().unwrap();
        assert_eq!(e, (x().pow(2) + n(2) * x() + n(1)).pow(-1));
    }

    #[test]
    fn inside_functions() {
        let e = Expr::call("sin", vec![(x() + n(1)).pow(2)]).unwrap();
        assert_eq!(
            e.expand().unwrap(),
            Expr::call("sin", vec![x().pow(2) + n(2) * x() + n(1)]).unwrap()
        );
    }

    #[test]
    fn refuses_huge_expansions() {
        let e = (x() + y() + n(1)).pow(1000);
        assert!(matches!(e.expand(), Err(MathError::TooLarge(_))));
    }
}
