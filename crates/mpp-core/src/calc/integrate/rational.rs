//! Rational functions over Q: polynomial division, then partial fractions over the factors
//! of the denominator (distinct and repeated linear factors, irreducible quadratics), each
//! integrated to logarithms, powers and arctangents.

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};

use crate::expr::{Expr, ExprNode};
use crate::factor::factor_poly;
use crate::functions::{func, Func};
use crate::poly::Poly;
use crate::simplify::{add, mul, pow, simplify};
use crate::steps::Steps;

use super::engine::{div, Integrator};
use super::holder;

/// Largest power of a sum expanded while reading a rational function.
const MAX_POWER: i64 = 48;
/// Largest denominator degree handled.
const MAX_DEGREE: usize = 24;

fn q(n: i64) -> BigRational {
    BigRational::from_integer(BigInt::from(n))
}

/// `e = num/den` with rational coefficients, in lowest terms; `None` otherwise.
pub(crate) fn to_rational(e: &Expr, var: &str) -> Option<(Poly, Poly)> {
    let (n, d) = read(e, var)?;
    if d.is_zero() {
        return None;
    }
    let g = n.gcd(&d);
    if g.degree().is_some_and(|k| k >= 1) {
        let (n2, _) = n.div_rem(&g).ok()?;
        let (d2, _) = d.div_rem(&g).ok()?;
        return Some(normalise(n2, d2));
    }
    Some(normalise(n, d))
}

/// Makes the denominator monic.
fn normalise(n: Poly, d: Poly) -> (Poly, Poly) {
    let lead = d.leading();
    if lead.is_zero() || lead.is_one() {
        return (n, d);
    }
    let inv = lead.recip();
    (n.scale(&inv), d.scale(&inv))
}

fn read(e: &Expr, var: &str) -> Option<(Poly, Poly)> {
    let one = || Poly::constant(BigRational::one());
    match e.node() {
        ExprNode::Num(n) => Some((Poly::constant(n.as_rational()?.clone()), one())),
        ExprNode::Sym(s) if &**s == var => Some((Poly::x(), one())),
        ExprNode::Add(terms) => {
            let mut acc = (Poly::zero(), one());
            for t in terms {
                let (n, d) = read(t, var)?;
                acc = (acc.0.mul(&d).add(&n.mul(&acc.1)), acc.1.mul(&d));
                acc = reduce(acc);
            }
            Some(acc)
        }
        ExprNode::Mul(fs) => {
            let mut acc = (one(), one());
            for f in fs {
                let (n, d) = read(f, var)?;
                acc = reduce((acc.0.mul(&n), acc.1.mul(&d)));
            }
            Some(acc)
        }
        ExprNode::Pow(b, x) => {
            let k = x.as_number()?.as_i64()?;
            if k.abs() > MAX_POWER {
                return None;
            }
            let (n, d) = read(b, var)?;
            let mut pn = one();
            let mut pd = one();
            for _ in 0..k.abs() {
                pn = pn.mul(&n);
                pd = pd.mul(&d);
            }
            if k >= 0 {
                Some((pn, pd))
            } else if pn.is_zero() {
                None
            } else {
                Some((pd, pn))
            }
        }
        _ => None,
    }
}

fn reduce((n, d): (Poly, Poly)) -> (Poly, Poly) {
    let g = n.gcd(&d);
    if g.degree().is_some_and(|k| k >= 1) {
        if let (Ok((n2, _)), Ok((d2, _))) = (n.div_rem(&g), d.div_rem(&g)) {
            return (n2, d2);
        }
    }
    (n, d)
}

fn poly_power(p: &Poly, k: u32) -> Poly {
    let mut acc = Poly::constant(BigRational::one());
    for _ in 0..k {
        acc = acc.mul(p);
    }
    acc
}

/// Integral of the polynomial `p`: `Σ cₖ xᵏ⁺¹/(k + 1)`.
fn int_poly(p: &Poly, var: &str) -> Expr {
    let x = Expr::sym(var);
    add(p
        .coeffs()
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.is_zero())
        .map(|(k, c)| {
            let k1 = k as i64 + 1;
            mul(vec![Expr::ratio(c / q(k1)), pow(x.clone(), Expr::int(k1))])
        })
        .collect())
}

/// One unknown numerator coefficient: factor index, power and `x`-power (0 or 1).
#[derive(Clone, Copy)]
struct Unknown {
    factor: usize,
    power: u32,
    x_power: usize,
}

/// Solves the square system `a·x = b` over Q.
fn solve(mut a: Vec<Vec<BigRational>>, mut b: Vec<BigRational>) -> Option<Vec<BigRational>> {
    let n = b.len();
    for col in 0..n {
        let pivot = (col..n).find(|&r| !a[r][col].is_zero())?;
        a.swap(col, pivot);
        b.swap(col, pivot);
        let inv = a[col][col].recip();
        for v in a[col].iter_mut().skip(col) {
            *v = &*v * &inv;
        }
        b[col] = &b[col] * &inv;
        for r in 0..n {
            if r != col && !a[r][col].is_zero() {
                let f = a[r][col].clone();
                for c in col..n {
                    let sub = &f * &a[col][c];
                    a[r][c] -= sub;
                }
                let sub = &f * &b[col];
                b[r] -= sub;
            }
        }
    }
    Some(b)
}

/// Partial fractions of `rem/den` (`deg rem < deg den`, `den` monic): the factors and the
/// numerator of each term.
pub(crate) fn decompose(rem: &Poly, den: &Poly) -> Option<Vec<(Poly, u32, Poly)>> {
    let (c, factors) = factor_poly(den);
    if factors
        .iter()
        .any(|(f, _)| f.degree().is_none_or(|d| d > 2))
    {
        return None;
    }
    // den = c·Π fᵢ^mᵢ with fᵢ primitive; make each fᵢ monic.
    let mut c = c;
    let mut monic = Vec::with_capacity(factors.len());
    for (f, m) in &factors {
        let lead = f.leading();
        for _ in 0..*m {
            c *= &lead;
        }
        monic.push((f.monic(), *m));
    }
    if c.is_zero() {
        return None;
    }
    let target = rem.scale(&c.recip());
    let n = den.degree()?;
    let mut unknowns = Vec::with_capacity(n);
    for (i, (f, m)) in monic.iter().enumerate() {
        for k in 1..=*m {
            for j in 0..f.degree()? {
                unknowns.push(Unknown {
                    factor: i,
                    power: k,
                    x_power: j,
                });
            }
        }
    }
    if unknowns.len() != n {
        return None;
    }
    let full = monic
        .iter()
        .fold(Poly::constant(BigRational::one()), |acc, (f, m)| {
            acc.mul(&poly_power(f, *m))
        });
    let mut columns = Vec::with_capacity(n);
    for u in &unknowns {
        let (f, _) = &monic[u.factor];
        let (cof, r) = full.div_rem(&poly_power(f, u.power)).ok()?;
        if !r.is_zero() {
            return None;
        }
        let mut shift = vec![BigRational::zero(); u.x_power];
        shift.push(BigRational::one());
        columns.push(cof.mul(&Poly::new(shift)));
    }
    let coeff = |p: &Poly, d: usize| p.coeffs().get(d).cloned().unwrap_or_else(BigRational::zero);
    let a: Vec<Vec<BigRational>> = (0..n)
        .map(|d| columns.iter().map(|col| coeff(col, d)).collect())
        .collect();
    let b: Vec<BigRational> = (0..n).map(|d| coeff(&target, d)).collect();
    let sol = solve(a, b)?;
    let mut terms: Vec<(Poly, u32, Poly)> = Vec::new();
    for (i, (f, m)) in monic.iter().enumerate() {
        for k in 1..=*m {
            let mut num = vec![BigRational::zero(); 2];
            for (u, s) in unknowns.iter().zip(&sol) {
                if u.factor == i && u.power == k {
                    num[u.x_power] = s.clone();
                }
            }
            let num = Poly::new(num);
            if !num.is_zero() {
                terms.push((f.clone(), k, num));
            }
        }
    }
    Some(terms)
}

fn term_expr(f: &Poly, k: u32, num: &Poly, var: &str) -> Expr {
    mul(vec![
        num.to_expr(var),
        pow(f.to_expr(var), Expr::int(-i64::from(k))),
    ])
}

fn ln_abs(e: Expr) -> Expr {
    func(Func::Ln, vec![func(Func::Abs, vec![e])])
}

/// `∫ num/f^k` for a monic linear or quadratic `f`.
fn int_term(f: &Poly, k: u32, num: &Poly, var: &str) -> Option<Expr> {
    let fx = f.to_expr(var);
    let ki = i64::from(k);
    let coef = |p: &Poly, d: usize| p.coeffs().get(d).cloned().unwrap_or_else(BigRational::zero);
    match f.degree()? {
        1 => {
            let a = Expr::ratio(coef(num, 0));
            Some(if k == 1 {
                mul(vec![a, ln_abs(fx)])
            } else {
                mul(vec![
                    a,
                    Expr::ratio(BigRational::one() / q(1 - ki)),
                    pow(fx, Expr::int(1 - ki)),
                ])
            })
        }
        2 => {
            // f = x² + p x + r;  A x + B = (A/2)(2x + p) + (B − A p/2)
            let p = coef(f, 1);
            let r = coef(f, 0);
            let a = coef(num, 1);
            let b = coef(num, 0);
            let half_a = &a / q(2);
            let c = &b - &a * &p / q(2);
            let d = &r - &p * &p / q(4);
            let mut parts = Vec::new();
            if !a.is_zero() {
                parts.push(if k == 1 {
                    let inner = if d.is_positive() {
                        func(Func::Ln, vec![fx.clone()])
                    } else {
                        ln_abs(fx.clone())
                    };
                    mul(vec![Expr::ratio(half_a), inner])
                } else {
                    mul(vec![
                        Expr::ratio(half_a / q(1 - ki)),
                        pow(fx.clone(), Expr::int(1 - ki)),
                    ])
                });
            }
            if !c.is_zero() {
                let t = add(vec![Expr::sym(var), Expr::ratio(&p / q(2))]);
                parts.push(mul(vec![Expr::ratio(c), quad_power(&t, &d, &fx, k)?]));
            }
            Some(add(parts))
        }
        _ => None,
    }
}

/// `J_k = ∫ dt/(t² + D)^k` with `t² + D = fx`.
fn quad_power(t: &Expr, d: &BigRational, fx: &Expr, k: u32) -> Option<Expr> {
    if d.is_zero() {
        return None;
    }
    let dd = Expr::ratio(d.clone());
    if k == 1 {
        if d.is_positive() {
            let s = dd.sqrt();
            return Some(div(func(Func::Atan, vec![div(t.clone(), s.clone())]), s));
        }
        // t² − s²: (1/(2s)) ln|(t − s)/(t + s)|
        let s = Expr::ratio(-d.clone()).sqrt();
        let ratio = div(
            add(vec![t.clone(), mul(vec![Expr::int(-1), s.clone()])]),
            add(vec![t.clone(), s.clone()]),
        );
        return Some(div(ln_abs(ratio), mul(vec![Expr::int(2), s])));
    }
    if !d.is_positive() {
        return None;
    }
    // J_k = t/(2D(k−1) f^(k−1)) + (2k−3)/(2D(k−1)) J_(k−1)
    let km1 = q(i64::from(k) - 1);
    let first = mul(vec![
        t.clone(),
        Expr::ratio((q(2) * d * &km1).recip()),
        pow(fx.clone(), Expr::int(1 - i64::from(k))),
    ]);
    let factor = Expr::ratio(q(2 * i64::from(k) - 3) / (q(2) * d * &km1));
    Some(add(vec![
        first,
        mul(vec![factor, quad_power(t, d, fx, k - 1)?]),
    ]))
}

/// How a quadratic factor is handled: split the numerator around the derivative of the
/// quadratic (a logarithm) and complete the square (an arctangent).
fn quadratic_note(f: &Poly, num: &Poly, var: &str) -> String {
    let coef = |p: &Poly, d: usize| p.coeffs().get(d).cloned().unwrap_or_else(BigRational::zero);
    let (p, r) = (coef(f, 1), coef(f, 0));
    let (a, b) = (coef(num, 1), coef(num, 0));
    let t = add(vec![Expr::sym(var), Expr::ratio(&p / q(2))]);
    let d = Expr::ratio(&r - &p * &p / q(4));
    let split = add(vec![
        mul(vec![Expr::ratio(&a / q(2)), f.derivative().to_expr(var)]),
        Expr::ratio(&b - &a * &p / q(2)),
    ]);
    format!(
        "{} = {split}, where {} is the derivative of {} (a logarithm); complete the square {} = ({t})² + {d} (an arctangent)",
        num.to_expr(var),
        f.derivative().to_expr(var),
        f.to_expr(var),
        f.to_expr(var),
    )
}

/// Integrates a rational function with rational coefficients.
pub(crate) fn partial_fractions(eng: &mut Integrator, e: &Expr, steps: &mut Steps) -> Option<Expr> {
    let var = eng.var.clone();
    let (num, den) = to_rational(e, &var)?;
    let deg = den.degree()?;
    if deg == 0 {
        // A polynomial; sums go term by term instead, for clearer steps.
        if matches!(e.node(), ExprNode::Add(_)) {
            return None;
        }
        let r = int_poly(&num.scale(&den.leading().recip()), &var);
        steps.push(
            "power rule",
            holder(e, &var),
            r.clone(),
            Some("integrate the polynomial term by term".into()),
        );
        return Some(r);
    }
    if deg > MAX_DEGREE {
        return None;
    }
    let (quot, rem) = num.div_rem(&den).ok()?;
    let proper = div(rem.to_expr(&var), den.to_expr(&var));
    if !quot.is_zero() {
        steps.push(
            "polynomial long division",
            holder(e, &var),
            Expr::new(ExprNode::Add(vec![
                holder(&quot.to_expr(&var), &var),
                holder(&proper, &var),
            ])),
            Some(format!("{e} = {} + {proper}", quot.to_expr(&var))),
        );
    }
    let mut pieces = vec![int_poly(&quot, &var)];
    if !rem.is_zero() {
        let terms = decompose(&rem, &den)?;
        let shown: Vec<Expr> = terms
            .iter()
            .map(|(f, k, n)| term_expr(f, *k, n, &var))
            .collect();
        let decomposition = Expr::new(ExprNode::Add(shown.clone()));
        let decomposition = if shown.len() == 1 {
            shown[0].clone()
        } else {
            decomposition
        };
        if shown.len() > 1 || simplify(&decomposition) != simplify(&proper) {
            steps.push(
                "partial fractions",
                holder(&proper, &var),
                holder(&decomposition, &var),
                Some(format!("decomposition: {proper} = {decomposition}")),
            );
        }
        let mut ints = Vec::with_capacity(terms.len());
        for ((f, k, n), shown) in terms.iter().zip(&shown) {
            let r = int_term(f, *k, n, &var)?;
            let (rule, note) = match (f.degree(), k) {
                (Some(1), 1) => ("integral of 1/x", None),
                (Some(1), _) => ("power rule", None),
                (_, 1) => (
                    "integral of (Ax + B)/(x² + px + q)",
                    Some(quadratic_note(f, n, &var)),
                ),
                _ => (
                    "reduction formula for 1/(t² + D)ᵏ",
                    Some(format!(
                        "{}; ∫ dt/(t² + D)ᵏ = t/(2D(k − 1)(t² + D)ᵏ⁻¹) + (2k − 3)/(2D(k − 1))·∫ dt/(t² + D)ᵏ⁻¹",
                        quadratic_note(f, n, &var)
                    )),
                ),
            };
            steps.push(rule, holder(shown, &var), r.clone(), note);
            ints.push(r);
        }
        pieces.push(add(ints));
    }
    Some(simplify(&add(pieces)))
}
