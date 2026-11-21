//! The Laplace transform, the one engine every caller uses: a table of transforms (powers,
//! exponentials, sines and cosines, hyperbolic functions, their products, the step
//! `H(t − a)` and the delta `δ(t − a)`), the inverse transform of rational functions by
//! partial fractions (with the shift `e^{−as} F(s) ↔ H(t − a) f(t − a)`, and `cosh`/`sinh`
//! for a quadratic with real irrational roots), and the transform method for
//! constant-coefficient initial value problems given as a list of coefficients
//! ([`solve_coefficients`]; the differential-equations solver uses the same algebra for
//! a parsed equation, in `ode::laplace_method`).
//!
//! `H(t − a)` is written `H(t - a)` (also `heaviside`, `step`, `u`, `theta`) and the delta
//! `delta(t - a)` (also `dirac`, `δ`).

use std::collections::BTreeMap;

use crate::calc::diff::diff;
use crate::calc::integrate::rational::{decompose, to_rational};
use crate::error::{MathError, MathResult};
use crate::expr::{Constant, Expr, ExprNode};
use crate::functions::{func, Func};
use crate::poly::Poly;
use crate::simplify::{add, mul, pow, simplify};

use crate::calc_support::value;
use crate::ode::tidy;

/// Names accepted for the Heaviside step.
const STEP_NAMES: &[&str] = &["H", "heaviside", "Heaviside", "step", "u", "theta", "θ"];
/// Names accepted for the Dirac delta.
const DELTA_NAMES: &[&str] = &["delta", "dirac", "Dirac", "δ"];

fn neg(e: Expr) -> Expr {
    mul(vec![Expr::int(-1), e])
}

/// `H(t − a)`.
pub fn step(arg: Expr) -> Expr {
    Expr::apply("H", vec![arg])
}

/// `δ(t − a)`.
pub fn delta(arg: Expr) -> Expr {
    Expr::apply("delta", vec![arg])
}

fn step_arg(e: &Expr) -> Option<&Expr> {
    match e.node() {
        ExprNode::Apply(f, args) if args.len() == 1 && STEP_NAMES.contains(&&**f) => Some(&args[0]),
        _ => None,
    }
}

fn delta_arg(e: &Expr) -> Option<&Expr> {
    match e.node() {
        ExprNode::Apply(f, args) if args.len() == 1 && DELTA_NAMES.contains(&&**f) => {
            Some(&args[0])
        }
        _ => None,
    }
}

/// True when `e` contains a step or delta function.
pub fn has_step_or_delta(e: &Expr) -> bool {
    step_arg(e).is_some()
        || delta_arg(e).is_some()
        || e.children().into_iter().any(has_step_or_delta)
}

/// The shift `a` of `t − a` (with `a ≥ 0`), when `arg` has that form.
fn shift_of(arg: &Expr, t: &str) -> Option<Expr> {
    let c = crate::poly::as_poly(arg, t)?;
    if c.len() != 2 || !c[1].is_one() {
        return None;
    }
    let a = simplify(&neg(c[0].clone()));
    (value(&a)? >= 0.0).then_some(a)
}

/// `L{f}(s)` from the table.
pub fn transform(f: &Expr, t: &str, s: &str) -> MathResult<Expr> {
    let e = crate::expand::expand(&simplify(f))?;
    let terms = match e.node() {
        ExprNode::Add(ts) => ts.clone(),
        _ => vec![e.clone()],
    };
    let mut out = Vec::new();
    for term in terms {
        out.push(transform_term(&term, t, s)?);
    }
    Ok(tidy(&add(out)))
}

fn unsupported(term: &Expr) -> MathError {
    MathError::Unsupported(format!(
        "the Laplace transform of {term} (not in the table)"
    ))
}

fn transform_term(term: &Expr, t: &str, s: &str) -> MathResult<Expr> {
    let factors = match term.node() {
        ExprNode::Mul(fs) => fs.clone(),
        _ => vec![term.clone()],
    };
    let ts = Expr::sym(t);
    // Step and delta factors.
    let mut rest = Vec::new();
    let mut shift: Option<Expr> = None;
    let mut is_delta = false;
    for f in factors {
        if let Some(arg) = step_arg(&f) {
            if shift.is_some() {
                return Err(unsupported(term));
            }
            shift = Some(shift_of(arg, t).ok_or_else(|| unsupported(term))?);
        } else if let Some(arg) = delta_arg(&f) {
            if shift.is_some() {
                return Err(unsupported(term));
            }
            shift = Some(shift_of(arg, t).ok_or_else(|| unsupported(term))?);
            is_delta = true;
        } else {
            rest.push(f);
        }
    }
    let body = simplify(&mul(rest));
    if is_delta {
        // L{δ(t − a) h(t)} = h(a) e^{−as}
        let a = shift.unwrap_or_else(Expr::zero);
        return Ok(simplify(&mul(vec![
            body.subs(t, &a),
            pow(Expr::e(), neg(mul(vec![a, Expr::sym(s)]))),
        ])));
    }
    if let Some(a) = shift {
        // L{H(t − a) h(t)} = e^{−as} L{h(t + a)}
        let moved = simplify(&body.subs(t, &add(vec![ts.clone(), a.clone()])));
        let inner = transform(&moved, t, s)?;
        return Ok(simplify(&mul(vec![
            pow(Expr::e(), neg(mul(vec![a, Expr::sym(s)]))),
            inner,
        ])));
    }
    basic(&body, t, s).ok_or_else(|| unsupported(term))
}

/// `c tⁿ e^{at} (cos bt | sin bt | 1)`.
fn basic(term: &Expr, t: &str, s: &str) -> Option<Expr> {
    let factors = match term.node() {
        ExprNode::Mul(fs) => fs.clone(),
        _ => vec![term.clone()],
    };
    let mut coeff = Vec::new();
    let mut n = 0u32;
    let mut a = Expr::zero();
    let mut trig: Option<(Func, Expr)> = None;
    for f in factors {
        if !f.contains_sym(t) {
            coeff.push(f);
            continue;
        }
        match f.node() {
            ExprNode::Sym(_) => n += 1,
            ExprNode::Pow(b, k) if b.is_sym(t) => {
                n += u32::try_from(k.as_number()?.as_i64()?).ok()?;
            }
            ExprNode::Pow(b, k) if matches!(b.node(), ExprNode::Const(Constant::E)) => {
                let c = crate::poly::as_poly(k, t)?;
                if c.len() != 2 {
                    return None;
                }
                coeff.push(pow(Expr::e(), c[0].clone()));
                a = add(vec![a, c[1].clone()]);
            }
            ExprNode::Func(g @ (Func::Sin | Func::Cos | Func::Sinh | Func::Cosh), args)
                if trig.is_none() =>
            {
                let c = crate::poly::as_poly(&args[0], t)?;
                if c.len() != 2 || !c[0].is_zero() {
                    return None;
                }
                trig = Some((*g, c[1].clone()));
            }
            _ => return None,
        }
    }
    let sa = add(vec![Expr::sym(s), neg(a)]);
    let base = match trig {
        None => pow(sa, Expr::int(-1)),
        Some((g, b)) => {
            let (num, sign) = match g {
                Func::Cos => (sa.clone(), 1),
                Func::Sin => (b.clone(), 1),
                Func::Cosh => (sa.clone(), -1),
                _ => (b.clone(), -1),
            };
            let den = add(vec![
                pow(sa, Expr::int(2)),
                mul(vec![Expr::int(sign), pow(b, Expr::int(2))]),
            ]);
            mul(vec![num, pow(den, Expr::int(-1))])
        }
    };
    // L{tⁿ f} = (−1)ⁿ dⁿ/dsⁿ F(s)
    let d = diff(&base, s, n, None).ok()?;
    let sign = if n.is_multiple_of(2) { 1 } else { -1 };
    Some(simplify(&mul(vec![Expr::int(sign), mul(coeff), d])))
}

/// `L⁻¹{F}(t)` for `F = Σ e^{−aⱼ s} Rⱼ(s)` with rational `Rⱼ` (rational coefficients).
pub fn inverse(f: &Expr, s: &str, t: &str) -> MathResult<Expr> {
    let e = crate::expand::expand(&simplify(f))?;
    let terms = match e.node() {
        ExprNode::Add(ts) => ts.clone(),
        _ => vec![e.clone()],
    };
    // Group by the exponential shift.
    let mut groups: BTreeMap<Expr, Vec<Expr>> = BTreeMap::new();
    for term in terms {
        let factors = match term.node() {
            ExprNode::Mul(fs) => fs.clone(),
            _ => vec![term.clone()],
        };
        let mut shift = Expr::zero();
        let mut rest = Vec::new();
        for f in factors {
            if let ExprNode::Pow(b, k) = f.node() {
                if matches!(b.node(), ExprNode::Const(Constant::E)) && k.contains_sym(s) {
                    let c =
                        crate::poly::as_poly(k, s).ok_or_else(|| inverse_unsupported(f.clone()))?;
                    if c.len() != 2 || !c[0].is_zero() {
                        return Err(inverse_unsupported(f.clone()));
                    }
                    shift = add(vec![shift, neg(c[1].clone())]);
                    continue;
                }
            }
            rest.push(f);
        }
        groups.entry(simplify(&shift)).or_default().push(mul(rest));
    }
    let mut out = Vec::new();
    for (a, parts) in groups {
        let r = simplify(&add(parts));
        let g = inverse_rational(&r, s, t)?;
        if a.is_zero() {
            out.push(g);
        } else {
            if value(&a).is_none_or(|v| v < 0.0) {
                return Err(inverse_unsupported(r));
            }
            let moved = simplify(&g.subs(t, &add(vec![Expr::sym(t), neg(a.clone())])));
            out.push(mul(vec![step(add(vec![Expr::sym(t), neg(a)])), moved]));
        }
    }
    Ok(tidy(&add(out)))
}

fn inverse_unsupported(e: Expr) -> MathError {
    MathError::Unsupported(format!("the inverse Laplace transform of {e}"))
}

fn rat(e: &crate::poly::Poly, k: usize) -> Expr {
    e.coeffs()
        .get(k)
        .map_or_else(Expr::zero, |q| Expr::ratio(q.clone()))
}

/// The inverse transform of a proper rational function by partial fractions.
fn inverse_rational(r: &Expr, s: &str, t: &str) -> MathResult<Expr> {
    if r.is_zero() {
        return Ok(Expr::zero());
    }
    let (num, den) = to_rational(r, s).ok_or_else(|| inverse_unsupported(r.clone()))?;
    let (q, rem) = num.div_rem(&den)?;
    let mut out = Vec::new();
    if !q.is_zero() {
        // A constant quotient is c·δ(t); higher powers of s would be derivatives of δ.
        if q.degree() != Some(0) {
            return Err(inverse_unsupported(r.clone()));
        }
        out.push(mul(vec![rat(&q, 0), delta(Expr::sym(t))]));
    }
    if !rem.is_zero() {
        let pieces = decompose(&rem, &den).ok_or_else(|| inverse_unsupported(r.clone()))?;
        let ts = Expr::sym(t);
        for (factor, k, numer) in pieces {
            out.push(piece(&factor, k, &numer, &ts).ok_or_else(|| inverse_unsupported(r.clone()))?);
        }
    }
    Ok(simplify(&add(out)))
}

/// `L⁻¹{numer / factorᵏ}` for a monic linear or irreducible quadratic factor.
fn piece(factor: &Poly, k: u32, numer: &Poly, t: &Expr) -> Option<Expr> {
    match factor.degree()? {
        1 => {
            // (s − p)^{−k}: t^{k−1} e^{pt}/(k − 1)!
            let p = simplify(&neg(rat(factor, 0)));
            let mut fact = 1i64;
            for j in 1..i64::from(k) {
                fact *= j;
            }
            Some(simplify(&mul(vec![
                rat(numer, 0),
                Expr::rational(1, fact),
                pow(t.clone(), Expr::int(i64::from(k) - 1)),
                pow(Expr::e(), mul(vec![p, t.clone()])),
            ])))
        }
        2 => {
            // s² + b s + c = (s − α)² + ω²
            let b = rat(factor, 1);
            let c = rat(factor, 0);
            let alpha = simplify(&mul(vec![Expr::rational(-1, 2), b.clone()]));
            let w2 = simplify(&add(vec![
                c,
                neg(mul(vec![Expr::rational(1, 4), pow(b, Expr::int(2))])),
            ]));
            let (a_coef, b_coef) = (rat(numer, 1), rat(numer, 0));
            // numer = A (s − α) + (B + A α)
            let shifted = simplify(&add(vec![b_coef, mul(vec![a_coef.clone(), alpha.clone()])]));
            let ex = pow(Expr::e(), mul(vec![alpha, t.clone()]));
            let w2v = value(&w2)?;
            if w2v < 0.0 && k == 1 {
                // (s − α)² − γ² with γ irrational: A cosh γt + (B + Aα)/γ sinh γt
                let gamma = simplify(&pow(neg(w2), Expr::rational(1, 2)));
                let gt = mul(vec![gamma.clone(), t.clone()]);
                let body = add(vec![
                    mul(vec![a_coef, func(Func::Cosh, vec![gt.clone()])]),
                    mul(vec![
                        shifted,
                        pow(gamma, Expr::int(-1)),
                        func(Func::Sinh, vec![gt]),
                    ]),
                ]);
                return Some(simplify(&mul(vec![ex, body])));
            }
            if w2v <= 0.0 {
                return None;
            }
            let w = simplify(&pow(w2, Expr::rational(1, 2)));
            let wt = mul(vec![w.clone(), t.clone()]);
            let (cos, sin) = (
                func(Func::Cos, vec![wt.clone()]),
                func(Func::Sin, vec![wt.clone()]),
            );
            let body = match k {
                1 => add(vec![
                    mul(vec![a_coef, cos]),
                    mul(vec![shifted, pow(w, Expr::int(-1)), sin]),
                ]),
                2 => add(vec![
                    // (s − α)/((s − α)² + ω²)² ↔ t sin ωt / (2ω)
                    mul(vec![
                        a_coef,
                        t.clone(),
                        sin.clone(),
                        pow(mul(vec![Expr::int(2), w.clone()]), Expr::int(-1)),
                    ]),
                    // 1/((s − α)² + ω²)² ↔ (sin ωt − ωt cos ωt)/(2ω³)
                    mul(vec![
                        shifted,
                        add(vec![sin, neg(mul(vec![wt, cos]))]),
                        pow(mul(vec![Expr::int(2), pow(w, Expr::int(3))]), Expr::int(-1)),
                    ]),
                ]),
                _ => return None,
            };
            Some(simplify(&mul(vec![ex, body])))
        }
        _ => None,
    }
}

/// The transformed equation `Σ aₖ (sᵏ Y − Σ_{j<k} s^{k−1−j} y⁽ʲ⁾(0)) = G` solved for `Y`:
/// returns `(P(s), Y(s))` with `P(s) = Σ aₖ sᵏ`.
pub(crate) fn transformed_solution(
    a: &[Expr],
    big_g: &Expr,
    init: &[Expr],
    s: &str,
) -> (Expr, Expr) {
    let ss = Expr::sym(s);
    let mut p_s = Vec::new();
    let mut ic = Vec::new();
    for (k, ak) in a.iter().enumerate() {
        p_s.push(mul(vec![ak.clone(), pow(ss.clone(), Expr::int(k as i64))]));
        for (j, yj) in init.iter().enumerate().take(k) {
            ic.push(mul(vec![
                ak.clone(),
                pow(ss.clone(), Expr::int((k - 1 - j) as i64)),
                yj.clone(),
            ]));
        }
    }
    let p_s = simplify(&add(p_s));
    let y_s = tidy(&mul(vec![
        add(vec![big_g.clone(), add(ic)]),
        pow(p_s.clone(), Expr::int(-1)),
    ]));
    (p_s, y_s)
}

/// Solves `Σ aₖ y⁽ᵏ⁾ = g(t)` (coefficients `a₀, a₁, …` from the lowest order up, all
/// constant) with `y(0), y′(0), …` given, by the Laplace transform. Returns `Y(s)` and
/// `y(t)`; the solution is checked by putting it back into the equation.
pub fn solve_coefficients(
    coeffs: &[Expr],
    forcing: &Expr,
    initial: &[Expr],
    t: &str,
    s: &str,
) -> MathResult<(Expr, Expr)> {
    let order = coeffs.len().saturating_sub(1);
    if order == 0 || initial.len() != order {
        return Err(MathError::WrongArity {
            function: "the initial conditions".into(),
            expected: order.to_string(),
            got: initial.len(),
        });
    }
    if coeffs.iter().any(|a| !a.is_constant()) {
        return Err(MathError::Unsupported(
            "the Laplace method for coefficients that are not constant".into(),
        ));
    }
    let big_g = transform(forcing, t, s)?;
    let (_, big_y) = transformed_solution(coeffs, &big_g, initial, s);
    let y = inverse(&big_y, s, t)?;
    check_coefficients(coeffs, forcing, initial, &y, t)?;
    Ok((big_y, y))
}

/// Puts `y` back into the equation and the initial conditions (numerically at a few
/// points), so a wrong solution is never returned.
fn check_coefficients(
    coeffs: &[Expr],
    forcing: &Expr,
    initial: &[Expr],
    y: &Expr,
    t: &str,
) -> MathResult<()> {
    let mut lhs = Vec::new();
    let mut d = y.clone();
    let mut derivs = vec![y.clone()];
    for a in coeffs {
        lhs.push(mul(vec![a.clone(), d.clone()]));
        d = diff(&d, t, 1, None)?;
        derivs.push(d.clone());
    }
    let residual = simplify(&add(vec![add(lhs), neg(forcing.clone())]));
    let ok_at = |e: &Expr, x: f64| -> bool {
        match crate::domains::solve::numeric::eval_at(e, t, x) {
            Some(v) => v.abs() <= 1e-7 * (1.0 + v.abs()),
            None => false,
        }
    };
    let equation_ok = [0.3, 0.7, 1.3].iter().all(|x| ok_at(&residual, *x));
    let initial_ok = initial.iter().zip(&derivs).all(|(y0, dk)| {
        let at0 = simplify(&dk.subs(t, &Expr::zero()));
        crate::domains::util::equal(&at0, y0)
    });
    if equation_ok && initial_ok {
        Ok(())
    } else {
        Err(MathError::Inconclusive(
            "the Laplace solution (it did not check out when put back into the equation)".into(),
        ))
    }
}

/// One interval between jumps: `(lo, hi, solution there, residual there)`.
pub(crate) type Piece = (f64, f64, Expr, Expr);

/// The pieces of a solution with steps: for each interval between the jump points of the
/// solution and of the equation, both with every `H` replaced by 0 or 1 and every `δ` by 0.
pub(crate) fn pieces(e: &Expr, residual: &Expr, t: &str) -> Vec<Piece> {
    let mut jumps = Vec::new();
    collect_jumps(e, t, &mut jumps);
    collect_jumps(residual, t, &mut jumps);
    jumps.sort_by(f64::total_cmp);
    jumps.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
    let mut bounds = vec![0.0];
    bounds.extend(jumps.iter().copied().filter(|j| *j > 0.0));
    bounds.push(f64::INFINITY);
    let mut out = Vec::new();
    for w in bounds.windows(2) {
        let mid = if w[1].is_finite() {
            0.5 * (w[0] + w[1])
        } else {
            w[0] + 1.0
        };
        out.push((
            w[0],
            w[1],
            replace_steps(e, t, mid),
            replace_steps(residual, t, mid),
        ));
    }
    out
}

fn collect_jumps(e: &Expr, t: &str, out: &mut Vec<f64>) {
    if let Some(arg) = step_arg(e).or_else(|| delta_arg(e)) {
        if let Some(a) = shift_of(arg, t).and_then(|a| value(&a)) {
            out.push(a);
        }
    }
    for c in e.children() {
        collect_jumps(c, t, out);
    }
}

fn replace_steps(e: &Expr, t: &str, at: f64) -> Expr {
    if let Some(arg) = step_arg(e) {
        let v = shift_of(arg, t).and_then(|a| value(&a)).unwrap_or(0.0);
        return Expr::int(if at > v { 1 } else { 0 });
    }
    if delta_arg(e).is_some() {
        return Expr::zero();
    }
    crate::simplify::canonical(e.map_children(&mut |c| replace_steps(c, t, at)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::p;

    fn s(e: &Expr) -> String {
        e.to_string()
    }

    fn ps(v: &[&str]) -> Vec<Expr> {
        v.iter().map(|x| p(x)).collect()
    }

    fn t() -> Expr {
        Expr::sym("t")
    }

    #[test]
    fn table_and_inverse() {
        let s = Expr::sym("s");
        assert_eq!(transform(&Expr::one(), "t", "s").unwrap(), s.recip());
        let e = transform(&pow(Expr::e(), mul(vec![Expr::int(2), t()])), "t", "s").unwrap();
        assert_eq!(e, (s.clone() - Expr::int(2)).recip());
        let sin = transform(
            &func(Func::Sin, vec![mul(vec![Expr::int(3), t()])]),
            "t",
            "s",
        )
        .unwrap();
        assert_eq!(sin, Expr::int(3) / (s.pow(2) + Expr::int(9)));
        let t2 = transform(&t().pow(2), "t", "s").unwrap();
        assert_eq!(t2, Expr::int(2) / s.pow(3));
        // Round trips.
        for f in [
            t() * pow(Expr::e(), neg(t())),
            func(Func::Cos, vec![mul(vec![Expr::int(2), t()])]),
            pow(Expr::e(), t()) * func(Func::Sin, vec![t()]),
            t() * func(Func::Sin, vec![t()]),
        ] {
            let big = transform(&f, "t", "s").unwrap();
            let back = inverse(&big, "s", "t").unwrap();
            let d = simplify(&add(vec![back.clone(), neg(f.clone())]));
            assert!(
                crate::ode::verify::vanishes(&d, std::slice::from_ref(&f), "t", None),
                "{f} → {big} → {back}"
            );
        }
        // Steps and deltas.
        let h = step(t() - Expr::int(2));
        assert_eq!(transform(&h, "t", "s").unwrap().to_string(), "e^(-2*s)/s");
        let back = inverse(
            &(pow(Expr::e(), neg(mul(vec![Expr::int(2), s.clone()]))) / s.clone()),
            "s",
            "t",
        )
        .unwrap();
        assert_eq!(back, h);
        assert_eq!(
            transform(&delta(t() - Expr::int(1)), "t", "s")
                .unwrap()
                .to_string(),
            "e^(-s)"
        );
    }

    #[test]
    fn laplace_table_shift_and_inverse() {
        assert_eq!(
            s(&transform(&p("t^2*exp(3*t)"), "t", "s").unwrap()),
            "2/(s - 3)^3"
        );
        assert_eq!(
            s(&transform(&p("sin(2*t)"), "t", "s").unwrap()),
            "2/(s^2 + 4)"
        );
        assert_eq!(s(&transform(&p("5"), "t", "s").unwrap()), "5/s");
        let shifted = transform(&p("exp(-t)*cos(2*t)"), "t", "s").unwrap();
        assert_eq!(
            s(&inverse(&shifted, "s", "t").unwrap()),
            s(&p("exp(-t)*cos(2*t)"))
        );
        assert_eq!(s(&inverse(&p("1/(s^2 + 1)"), "s", "t").unwrap()), "sin(t)");
        assert_eq!(
            s(&inverse(&p("1/(s*(s + 1))"), "s", "t").unwrap()),
            s(&p("1 - exp(-t)"))
        );
        assert_eq!(
            s(&inverse(&p("1/(s - 2)^2"), "s", "t").unwrap()),
            s(&p("t*exp(2*t)"))
        );
        assert!(inverse(&p("s^2/(s + 1)"), "s", "t").is_err());
        assert!(transform(&p("ln(t)"), "t", "s").is_err());
    }

    #[test]
    fn laplace_solves_odes() {
        // y'' + y = 0, y(0) = 1, y'(0) = 0.
        let (_, y) =
            solve_coefficients(&ps(&["1", "0", "1"]), &p("0"), &ps(&["1", "0"]), "t", "s").unwrap();
        assert_eq!(s(&y), "cos(t)");
        // y'' − 3y' + 2y = 0, y(0) = 0, y'(0) = 1.
        let (_, y) =
            solve_coefficients(&ps(&["2", "-3", "1"]), &p("0"), &ps(&["0", "1"]), "t", "s")
                .unwrap();
        assert_eq!(s(&y), s(&p("exp(2*t) - exp(t)")));
        // y' + y = 1, y(0) = 0.
        let (_, y) = solve_coefficients(&ps(&["1", "1"]), &p("1"), &ps(&["0"]), "t", "s").unwrap();
        assert_eq!(s(&y), s(&p("1 - exp(-t)")));
        assert!(solve_coefficients(&ps(&["1", "1"]), &p("0"), &[], "t", "s").is_err());
    }

    #[test]
    fn real_irrational_roots_invert_to_hyperbolic_functions() {
        let back = inverse(&p("1/(s^2 - 2)"), "s", "t").unwrap();
        let want = p("sinh(sqrt(2)*t)/sqrt(2)");
        let d = simplify(&add(vec![back.clone(), neg(want.clone())]));
        assert!(
            crate::ode::verify::vanishes(&d, std::slice::from_ref(&want), "t", None),
            "{back}"
        );
    }
}
