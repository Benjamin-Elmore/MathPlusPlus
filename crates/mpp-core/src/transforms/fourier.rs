//! The Fourier transform `𝓕{f}(ω) = ∫_{−∞}^{∞} f(x) e^{−iωx} dx` (angular frequency, no
//! factor in front) and its inverse `𝓕⁻¹{F}(x) = (1/2π) ∫ F(ω) e^{iωx} dω`, by a table:
//!
//! | f(x) | 𝓕{f}(ω) |
//! |---|---|
//! | c | 2πc δ(ω) |
//! | δ(x − b) | e^{−iωb} |
//! | e^{−a x²} | √(π/a) e^{−ω²/(4a)} |
//! | e^{−a\|x\|} | 2a/(a² + ω²) |
//! | 1/(x² + a²) | (π/a) e^{−a\|ω\|} |
//! | H(x) e^{−a x} | 1/(a + iω) |
//!
//! with `a > 0`, sums and constant factors. The inverse uses duality,
//! `𝓕⁻¹{F}(x) = 𝓕{F}(−x)/(2π)`. Anything else is reported as not in the table rather
//! than guessed.

use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode};
use crate::functions::{func, Func};
use crate::simplify::{add, mul};

/// `e^u`.
fn exp(u: Expr) -> Expr {
    Expr::e().pow(u)
}

fn unsupported(f: &Expr) -> MathError {
    MathError::NoClosedForm(format!(
        "the Fourier transform of {f} (it is not in the table: Gaussians, e^(−a|x|), 1/(x² + a²), H(x)e^(−ax), δ and constants)"
    ))
}

/// A positive constant, or `None`.
fn positive(a: &Expr, x: &str) -> Option<Expr> {
    let a = a.simplify();
    if a.contains_sym(x) || !a.is_constant() {
        return None;
    }
    let v = a.eval_f64(&crate::eval::EmptyEnv).ok()?;
    (v.is_finite() && v > 0.0).then_some(a)
}

/// `𝓕{f}(ω)` with `f` in `x`.
pub fn transform(f: &Expr, x: &str, w: &str) -> MathResult<Expr> {
    let f = f.simplify();
    let omega = Expr::sym(w);
    if !f.contains_sym(x) {
        return Ok((Expr::int(2) * Expr::pi() * f * Expr::apply("delta", vec![omega])).simplify());
    }
    match f.node() {
        ExprNode::Add(terms) => {
            let parts = terms
                .iter()
                .map(|t| transform(t, x, w))
                .collect::<MathResult<Vec<_>>>()?;
            return Ok(add(parts).simplify());
        }
        ExprNode::Mul(fs) => {
            let (consts, rest): (Vec<Expr>, Vec<Expr>) =
                fs.iter().cloned().partition(|g| !g.contains_sym(x));
            if !consts.is_empty() {
                let inner = transform(&mul(rest), x, w)?;
                return Ok((mul(consts) * inner).simplify());
            }
        }
        _ => {}
    }
    entry(&f, x, &omega)
        .map(|e| e.simplify())
        .ok_or_else(|| unsupported(&f))
}

/// `𝓕⁻¹{F}(x)` with `F` in `ω`.
pub fn inverse(big_f: &Expr, w: &str, x: &str) -> MathResult<Expr> {
    let u = "u\u{2032}";
    let g = big_f.subs(w, &Expr::sym(u));
    let forward = transform(&g, u, x).map_err(|_| {
        MathError::NoClosedForm(format!(
            "the inverse Fourier transform of {big_f} (it is not in the table)"
        ))
    })?;
    let flipped = forward.subs(x, &(-Expr::sym(x)));
    Ok((flipped / (Expr::int(2) * Expr::pi())).simplify())
}

/// One table entry, or `None`.
fn entry(f: &Expr, x: &str, omega: &Expr) -> Option<Expr> {
    let xs = Expr::sym(x);
    let i = Expr::i();
    // δ(x − b) → e^{−iωb}
    if let ExprNode::Apply(name, args) = f.node() {
        if &**name == "delta" && args.len() == 1 {
            let b = (xs.clone() - args[0].clone()).simplify();
            if !b.contains_sym(x)
                && (args[0].clone() - xs.clone() + b.clone())
                    .simplify()
                    .is_zero()
            {
                return Some(exp(-(i * omega.clone() * b)));
            }
        }
    }
    // e^{u}
    if let ExprNode::Pow(base, u) = f.node() {
        if matches!(base.node(), ExprNode::Const(crate::expr::Constant::E)) {
            // e^{−a x²}
            let q = (u.clone() / xs.clone().pow(2)).simplify();
            if let Some(a) = positive(&(-q), x) {
                let w2 = omega.clone().pow(2);
                return Some((Expr::pi() / a.clone()).sqrt() * exp(-(w2 / (Expr::int(4) * a))));
            }
            // e^{−a|x|}
            let absx = func(Func::Abs, vec![xs.clone()]);
            let q = (u.clone() / absx).simplify();
            if let Some(a) = positive(&(-q), x) {
                return Some(Expr::int(2) * a.clone() / (a.pow(2) + omega.clone().pow(2)));
            }
        }
        // 1/(x² + a²)
        if u.as_number()
            .is_some_and(|n| *n == crate::number::Number::from(-1))
        {
            if let Some(c) = base.as_poly(x) {
                if let [c0, c1, c2] = c.as_slice() {
                    if c1.is_zero() && c2.is_one() {
                        if let Some(a2) = positive(c0, x) {
                            let a = a2.sqrt();
                            let absw = func(Func::Abs, vec![omega.clone()]);
                            return Some(Expr::pi() / a.clone() * exp(-(a * absw)));
                        }
                    }
                }
            }
        }
    }
    // H(x) e^{−a x}
    if let ExprNode::Mul(fs) = f.node() {
        if let [p, q] = fs.as_slice() {
            for (h, e) in [(p, q), (q, p)] {
                let is_step = matches!(h.node(), ExprNode::Apply(n, a) if &**n == "H" && a.len() == 1 && a[0].is_sym(x));
                if !is_step {
                    continue;
                }
                if let ExprNode::Pow(base, u) = e.node() {
                    if matches!(base.node(), ExprNode::Const(crate::expr::Constant::E)) {
                        let q = (u.clone() / xs.clone()).simplify();
                        if let Some(a) = positive(&(-q), x) {
                            return Some((a + Expr::i() * omega.clone()).recip());
                        }
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x() -> Expr {
        Expr::sym("x")
    }

    fn s(e: &Expr) -> String {
        e.to_string()
    }

    #[test]
    fn table_entries() {
        let gauss = exp(-(x().pow(2)));
        assert_eq!(
            s(&transform(&gauss, "x", "ω").unwrap()),
            "sqrt(π)*e^(-ω^2/4)"
        );
        let decay = exp(-(func(Func::Abs, vec![x()])));
        assert_eq!(s(&transform(&decay, "x", "ω").unwrap()), "2/(ω^2 + 1)");
        let lorentz = (x().pow(2) + Expr::one()).recip();
        assert_eq!(s(&transform(&lorentz, "x", "ω").unwrap()), "π*e^(-|ω|)");
        assert!(transform(&func(Func::Sin, vec![x()]), "x", "ω").is_err());
    }

    #[test]
    fn inverse_by_duality() {
        let w = Expr::sym("ω");
        let big = Expr::int(2) / (w.pow(2) + Expr::one());
        assert_eq!(s(&inverse(&big, "ω", "x").unwrap()), "e^(-|x|)");
    }
}
