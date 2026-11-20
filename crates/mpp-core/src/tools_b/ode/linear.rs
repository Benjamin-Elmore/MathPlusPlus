//! Linear equations `aₙ y⁽ⁿ⁾ + … + a₁ y′ + a₀ y = g`: constant coefficients of any order
//! (real, repeated and complex roots of the characteristic polynomial), Cauchy–Euler
//! equations, reduction of order from one known solution, and particular solutions by
//! undetermined coefficients or variation of parameters.

use std::collections::BTreeMap;

use num_bigint::BigInt;
use num_rational::BigRational;

use crate::calc::diff::diff;
use crate::calc::integrate::integrate;
use crate::domains::linalg::Matrix;
use crate::domains::solve::poly::roots;
use crate::error::MathResult;
use crate::expr::{Expr, ExprNode};
use crate::functions::{func, Func};
use crate::number::Number;
use crate::poly::Poly;
use crate::simplify::{add, mul, pow, simplify};
use crate::steps::Steps;

use super::parse::Ode;
use super::{linsys, tidy, Candidate};
use crate::tools_b::util::{pythagorean_tidy, value};

fn neg(e: Expr) -> Expr {
    mul(vec![Expr::int(-1), e])
}

/// The `k`-th derivative (`k = 0` gives `e` itself).
fn nth(e: &Expr, x: &str, k: u32) -> MathResult<Expr> {
    if k == 0 {
        Ok(e.clone())
    } else {
        diff(e, x, k, None)
    }
}

fn is_zero(e: &Expr) -> bool {
    let s = simplify(e);
    s.is_zero() || crate::expand::expand(&s).is_ok_and(|x| simplify(&x).is_zero())
}

/// A characteristic root split into real and imaginary parts, with its multiplicity.
#[derive(Clone, Debug)]
struct CharRoot {
    re: Expr,
    im: Expr,
    multiplicity: u32,
    exact: bool,
}

/// `(α, β)` with `r = α + βi`.
fn re_im(r: &Expr) -> Option<(Expr, Expr)> {
    let iota = "\u{3b9}\u{1}";
    fn lift(e: &Expr, iota: &str) -> Expr {
        if let ExprNode::Num(n) = e.node() {
            if !n.im().is_zero() {
                return add(vec![
                    Expr::num(n.re()),
                    mul(vec![Expr::num(n.im()), Expr::sym(iota)]),
                ]);
            }
            return e.clone();
        }
        crate::simplify::canonical(e.map_children(&mut |c| lift(c, iota)))
    }
    let lifted = crate::expand::expand(&lift(r, iota)).ok()?;
    let c = crate::poly::as_poly(&lifted, iota)?;
    let (mut re, mut im) = (Vec::new(), Vec::new());
    for (k, ck) in c.into_iter().enumerate() {
        let sign = if (k / 2) % 2 == 0 { 1 } else { -1 };
        let term = mul(vec![Expr::int(sign), ck]);
        if k % 2 == 0 {
            re.push(term);
        } else {
            im.push(term);
        }
    }
    let (re, im) = (simplify(&add(re)), simplify(&add(im)));
    // Cross-check numerically.
    let z = r.eval_complex(&crate::eval::EmptyEnv).ok()?;
    let (a, b) = (value(&re)?, value(&im)?);
    ((a - z.re).abs() <= 1e-9 * (1.0 + z.norm()) && (b - z.im).abs() <= 1e-9 * (1.0 + z.norm()))
        .then_some((re, im))
}

fn char_roots(coeffs: &[Expr]) -> Option<Vec<CharRoot>> {
    let rs = roots(coeffs).ok()?;
    let mut out = Vec::new();
    for r in rs {
        let (re, im) = re_im(&r.value)?;
        out.push(CharRoot {
            re,
            im,
            multiplicity: r.multiplicity,
            exact: r.exact,
        });
    }
    Some(out)
}

/// The basis `xʲ e^{αx}`, `xʲ e^{αx} cos βx`, `xʲ e^{αx} sin βx` from the roots; with
/// `log = true` (Cauchy–Euler) `x` becomes `ln x` in the powers and `e^{αx}` becomes `x^α`.
fn basis_from(roots: &[CharRoot], x: &str, euler: bool) -> Vec<Expr> {
    let xs = Expr::sym(x);
    let lnx = func(Func::Ln, vec![xs.clone()]);
    let t = if euler { lnx.clone() } else { xs.clone() };
    let grow = |a: &Expr, x: &Expr| {
        if euler {
            pow(x.clone(), a.clone())
        } else {
            pow(Expr::e(), mul(vec![a.clone(), x.clone()]))
        }
    };
    let mut out = Vec::new();
    for r in roots {
        let bv = value(&r.im).unwrap_or(0.0);
        if bv < -1e-12 {
            continue;
        }
        for j in 0..r.multiplicity {
            let pj = pow(t.clone(), Expr::int(i64::from(j)));
            let g = grow(&r.re, &xs);
            if bv.abs() <= 1e-12 {
                out.push(simplify(&mul(vec![pj.clone(), g])));
            } else {
                let arg = simplify(&mul(vec![r.im.clone(), t.clone()]));
                out.push(simplify(&mul(vec![
                    pj.clone(),
                    g.clone(),
                    func(Func::Cos, vec![arg.clone()]),
                ])));
                out.push(simplify(&mul(vec![pj, g, func(Func::Sin, vec![arg])])));
            }
        }
    }
    out
}

fn roots_text(roots: &[CharRoot]) -> String {
    roots
        .iter()
        .map(|r| {
            let v = if value(&r.im).is_some_and(|b| b.abs() > 1e-12) {
                format!(
                    "{} ± {}i",
                    r.re,
                    simplify(&func(Func::Abs, vec![r.im.clone()]))
                )
            } else {
                r.re.to_string()
            };
            if r.multiplicity > 1 {
                format!("{v} (×{})", r.multiplicity)
            } else {
                v
            }
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The candidates for a linear equation.
pub(crate) fn candidates(
    ode: &Ode,
    consts: &[String],
    known: Option<&Expr>,
    steps: &mut Steps,
) -> Vec<MathResult<Candidate>> {
    let Some((a, g)) = ode.linear_coefficients() else {
        return Vec::new();
    };
    let x = ode.x.as_str();
    let n = ode.order as usize;
    if a[n].is_zero() {
        return Vec::new();
    }
    let constant = a.iter().all(|c| !c.contains_sym(x) && value(c).is_some());
    let mut out = Vec::new();
    if constant {
        if let Some(c) = constant_coefficients(ode, &a, &g, consts, steps) {
            out.push(c);
        }
    } else if let Some(c) = cauchy_euler(ode, &a, &g, consts, steps) {
        out.push(c);
    } else if n == 2 {
        if let Some(c) = reduction_of_order(ode, &a, &g, consts, known, steps) {
            out.push(c);
        }
    }
    out
}

fn general(basis: &[Expr], consts: &[String], particular: Option<&Expr>) -> Expr {
    let mut terms: Vec<Expr> = basis
        .iter()
        .zip(consts)
        .map(|(b, c)| mul(vec![Expr::sym(c), b.clone()]))
        .collect();
    if let Some(p) = particular {
        terms.push(p.clone());
    }
    simplify(&add(terms))
}

fn constant_coefficients(
    ode: &Ode,
    a: &[Expr],
    g: &Expr,
    consts: &[String],
    steps: &mut Steps,
) -> Option<MathResult<Candidate>> {
    let x = ode.x.as_str();
    let roots = char_roots(a)?;
    let basis = basis_from(&roots, x, false);
    if basis.len() != ode.order as usize {
        return None;
    }
    let r = "r";
    let charpoly = simplify(&add(a
        .iter()
        .enumerate()
        .map(|(k, c)| mul(vec![c.clone(), pow(Expr::sym(r), Expr::int(k as i64))]))
        .collect()));
    let exact_roots = roots.iter().all(|r| r.exact);
    steps.push(
        "constant coefficients",
        ode.original.clone(),
        Expr::equation(charpoly.clone(), Expr::zero()),
        Some(format!(
            "try {y} = e^({r}{x}): the characteristic equation {charpoly} = 0 has roots {}{}; the solutions of the homogeneous equation are {}",
            roots_text(&roots),
            if exact_roots { "" } else { " (found numerically)" },
            basis.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(", "),
            y = ode.y,
        )),
    );
    let (particular, method) = if g.is_zero() {
        (None, "constant coefficients")
    } else if let Some(p) = undetermined_coefficients(ode, a, g, &roots, steps) {
        (Some(p), "constant coefficients + undetermined coefficients")
    } else {
        let p = variation_of_parameters(ode, &basis, &a[a.len() - 1], g, steps)?;
        (Some(p), "constant coefficients + variation of parameters")
    };
    let mut cand = Candidate::explicit(general(&basis, consts, particular.as_ref()), method);
    if !exact_roots {
        cand.notes
            .push("the characteristic roots were found numerically".into());
    }
    Some(Ok(cand))
}

/// One forcing term `c xᵈ e^{αx} cos βx` (or `sin`, or neither).
#[derive(Clone, Debug)]
struct Forcing {
    alpha: Expr,
    beta: Expr,
    degree: u32,
}

/// Reads `g` as a sum of `poly · e^{αx} · cos/sin(βx)` terms; `None` otherwise.
fn forcing_terms(g: &Expr, x: &str) -> Option<Vec<Forcing>> {
    let e = crate::expand::expand(g).ok()?;
    let terms = match e.node() {
        ExprNode::Add(ts) => ts.clone(),
        _ => vec![e.clone()],
    };
    let mut out: Vec<Forcing> = Vec::new();
    for t in terms {
        let factors = match t.node() {
            ExprNode::Mul(fs) => fs.clone(),
            _ => vec![t.clone()],
        };
        let mut alpha = Expr::zero();
        let mut beta = Expr::zero();
        let mut degree = 0u32;
        for f in factors {
            if !f.contains_sym(x) {
                continue;
            }
            match f.node() {
                ExprNode::Sym(_) => degree += 1,
                ExprNode::Pow(b, k) if b.is_sym(x) => {
                    degree += u32::try_from(k.as_number()?.as_i64()?).ok()?;
                }
                ExprNode::Pow(b, k)
                    if matches!(b.node(), ExprNode::Const(crate::expr::Constant::E)) =>
                {
                    let c = crate::poly::as_poly(k, x)?;
                    if c.len() != 2 {
                        return None;
                    }
                    alpha = add(vec![alpha, c[1].clone()]);
                }
                ExprNode::Func(Func::Sin | Func::Cos, args) => {
                    let c = crate::poly::as_poly(&args[0], x)?;
                    if c.len() != 2 || !c[0].is_zero() || !beta.is_zero() {
                        return None;
                    }
                    beta = simplify(&func(Func::Abs, vec![c[1].clone()]));
                }
                _ => return None,
            }
        }
        let alpha = simplify(&alpha);
        match out.iter_mut().find(|f| f.alpha == alpha && f.beta == beta) {
            Some(f) => f.degree = f.degree.max(degree),
            None => out.push(Forcing {
                alpha,
                beta,
                degree,
            }),
        }
    }
    Some(out)
}

/// Undetermined coefficients: a trial of the same shape as `g` (times `xˢ` when it
/// resonates with a root of multiplicity `s`), its coefficients matched term by term.
fn undetermined_coefficients(
    ode: &Ode,
    a: &[Expr],
    g: &Expr,
    roots: &[CharRoot],
    steps: &mut Steps,
) -> Option<Expr> {
    let x = ode.x.as_str();
    let xs = Expr::sym(x);
    let forcing = forcing_terms(g, x)?;
    let mut unknowns: Vec<String> = Vec::new();
    let mut trial_terms = Vec::new();
    for f in &forcing {
        let (fa, fb) = (value(&f.alpha)?, value(&f.beta)?);
        let s = roots
            .iter()
            .find(|r| {
                value(&r.re).is_some_and(|v| (v - fa).abs() < 1e-9)
                    && value(&r.im).is_some_and(|v| (v.abs() - fb).abs() < 1e-9)
            })
            .map_or(0, |r| r.multiplicity);
        let ex = pow(Expr::e(), mul(vec![f.alpha.clone(), xs.clone()]));
        let shift = pow(xs.clone(), Expr::int(i64::from(s)));
        let trig: Vec<Option<Expr>> = if fb.abs() < 1e-12 {
            vec![None]
        } else {
            let arg = mul(vec![f.beta.clone(), xs.clone()]);
            vec![
                Some(func(Func::Cos, vec![arg.clone()])),
                Some(func(Func::Sin, vec![arg])),
            ]
        };
        for t in trig {
            for j in 0..=f.degree {
                let name = format!("\u{1}A{}", unknowns.len());
                unknowns.push(name.clone());
                let mut fs = vec![
                    Expr::sym(&name),
                    shift.clone(),
                    pow(xs.clone(), Expr::int(i64::from(j))),
                    ex.clone(),
                ];
                if let Some(t) = &t {
                    fs.push(t.clone());
                }
                trial_terms.push(mul(fs));
            }
        }
    }
    if unknowns.len() > linsys::MAX_UNKNOWNS {
        return None;
    }
    let trial = simplify(&add(trial_terms));
    let mut lhs = Vec::new();
    for (k, ak) in a.iter().enumerate() {
        if ak.is_zero() {
            continue;
        }
        lhs.push(mul(vec![ak.clone(), nth(&trial, x, k as u32).ok()?]));
    }
    let residual = crate::expand::expand(&add(vec![add(lhs), neg(g.clone())])).ok()?;
    // Group the terms by their shape (everything except numbers and unknowns).
    let terms = match residual.node() {
        ExprNode::Add(ts) => ts.clone(),
        _ => vec![residual.clone()],
    };
    let mut groups: BTreeMap<Expr, Vec<Expr>> = BTreeMap::new();
    for t in terms {
        let factors = match t.node() {
            ExprNode::Mul(fs) => fs.clone(),
            _ => vec![t.clone()],
        };
        let (coef, shape): (Vec<Expr>, Vec<Expr>) = factors
            .into_iter()
            .partition(|f| f.is_constant() || unknowns.iter().any(|u| f.is_sym(u)));
        groups
            .entry(simplify(&mul(shape)))
            .or_default()
            .push(mul(coef));
    }
    let mut rows = Vec::new();
    let mut rhs = Vec::new();
    for (_, parts) in groups {
        let (row, b) = linsys::linear_form(&add(parts), &unknowns)?;
        rows.push(row);
        rhs.push(b);
    }
    let sol = linsys::solve(rows, rhs)?;
    let pairs: Vec<(&str, Expr)> = unknowns
        .iter()
        .map(String::as_str)
        .zip(sol.iter().cloned())
        .collect();
    let particular = tidy(&trial.subs_all(&pairs));
    steps.push(
        "undetermined coefficients",
        trial.clone(),
        Expr::equation(Expr::sym(&format!("{}_p", ode.y)), particular.clone()),
        Some(format!(
            "the forcing {g} suggests the trial {}; matching coefficients gives {}_p = {particular}",
            trial,
            ode.y
        )),
    );
    Some(particular)
}

/// Variation of parameters with the basis `φ₁ … φₙ`: `uⱼ′ = (g/aₙ)·Wⱼ/W`.
fn variation_of_parameters(
    ode: &Ode,
    basis: &[Expr],
    lead: &Expr,
    g: &Expr,
    steps: &mut Steps,
) -> Option<Expr> {
    let x = ode.x.as_str();
    let n = basis.len();
    let mut data = Vec::with_capacity(n * n);
    for i in 0..n {
        for b in basis {
            data.push(nth(b, x, i as u32).ok()?);
        }
    }
    let w_matrix = Matrix::new(n, n, data.clone()).ok()?;
    let w = pythagorean_tidy(&simplify(&w_matrix.det().ok()?));
    if is_zero(&w) {
        return None;
    }
    let g_std = simplify(&mul(vec![g.clone(), pow(lead.clone(), Expr::int(-1))]));
    let mut parts = Vec::new();
    let mut shown = Vec::new();
    for j in 0..n {
        let mut dj = data.clone();
        for i in 0..n {
            dj[i * n + j] = if i == n - 1 {
                Expr::one()
            } else {
                Expr::zero()
            };
        }
        let wj = pythagorean_tidy(&simplify(&Matrix::new(n, n, dj).ok()?.det().ok()?));
        let du = tidy(&mul(vec![g_std.clone(), wj, pow(w.clone(), Expr::int(-1))]));
        let u = integrate(&du, x, None).ok()?;
        shown.push(format!("u{}′ = {du}", j + 1));
        parts.push(mul(vec![u, basis[j].clone()]));
    }
    let particular = tidy(&add(parts));
    steps.push(
        "variation of parameters",
        Expr::sym(&format!("{}_p", ode.y)),
        particular.clone(),
        Some(format!(
            "Wronskian W = {w}; {}; {}_p = Σ uⱼ φⱼ = {particular}",
            shown.join(", "),
            ode.y
        )),
    );
    Some(particular)
}

/// `Σ cₖ xᵏ y⁽ᵏ⁾ = g`: with `x = eᵗ` the indicial polynomial `Σ cₖ r(r−1)…(r−k+1)`.
fn cauchy_euler(
    ode: &Ode,
    a: &[Expr],
    g: &Expr,
    consts: &[String],
    steps: &mut Steps,
) -> Option<MathResult<Candidate>> {
    let x = ode.x.as_str();
    let xs = Expr::sym(x);
    let mut c = Vec::with_capacity(a.len());
    for (k, ak) in a.iter().enumerate() {
        let ck = simplify(&mul(vec![
            ak.clone(),
            pow(xs.clone(), Expr::int(-(k as i64))),
        ]));
        if ck.contains_sym(x) {
            return None;
        }
        value(&ck)?;
        c.push(ck);
    }
    // Indicial polynomial coefficients.
    let mut total = Poly::zero();
    for (k, ck) in c.iter().enumerate() {
        let q = ck.as_number().and_then(Number::as_rational)?.clone();
        let mut falling = Poly::constant(BigRational::from_integer(BigInt::from(1)));
        for j in 0..k {
            let factor = Poly::new(vec![
                BigRational::from_integer(BigInt::from(-(j as i64))),
                BigRational::from_integer(BigInt::from(1)),
            ]);
            falling = falling.mul(&factor);
        }
        total = total.add(&falling.scale(&q));
    }
    let coeffs: Vec<Expr> = total
        .coeffs()
        .iter()
        .map(|q| Expr::ratio(q.clone()))
        .collect();
    let roots = char_roots(&coeffs)?;
    let basis = basis_from(&roots, x, true);
    if basis.len() != ode.order as usize {
        return None;
    }
    steps.push(
        "Cauchy–Euler",
        ode.original.clone(),
        Expr::equation(total.to_expr("r"), Expr::zero()),
        Some(format!(
            "try {y} = {x}^r: the indicial equation {} = 0 has roots {}; the solutions of the homogeneous equation are {}",
            total.to_expr("r"),
            roots_text(&roots),
            basis.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(", "),
            y = ode.y
        )),
    );
    let (particular, method) = if g.is_zero() {
        (None, "Cauchy–Euler")
    } else {
        let p = variation_of_parameters(ode, &basis, &a[a.len() - 1], g, steps)?;
        (Some(p), "Cauchy–Euler + variation of parameters")
    };
    let mut cand = Candidate::explicit(general(&basis, consts, particular.as_ref()), method);
    cand.notes.push(format!("for {x} > 0"));
    Some(Ok(cand))
}

/// Second order with one known solution `y₁`: `y₂ = y₁ ∫ e^{−∫P}/y₁² dx`.
fn reduction_of_order(
    ode: &Ode,
    a: &[Expr],
    g: &Expr,
    consts: &[String],
    known: Option<&Expr>,
    steps: &mut Steps,
) -> Option<MathResult<Candidate>> {
    let x = ode.x.as_str();
    let homogeneous = |y1: &Expr| -> bool {
        let mut terms = Vec::new();
        for (k, ak) in a.iter().enumerate() {
            match nth(y1, x, k as u32) {
                Ok(d) => terms.push(mul(vec![ak.clone(), d])),
                Err(_) => return false,
            }
        }
        let r = simplify(&add(terms));
        is_zero(&r) || super::verify::vanishes(&r, std::slice::from_ref(y1), x, None)
    };
    let y1 = match known {
        Some(k) if homogeneous(k) => k.clone(),
        Some(_) => return None,
        None => guess_homogeneous(x, &homogeneous)?,
    };
    let p = simplify(&mul(vec![a[1].clone(), pow(a[2].clone(), Expr::int(-1))]));
    let int_p = integrate(&p, x, None).ok()?;
    let w = super::first_order::exp_of(&neg(int_p));
    let inner = integrate(
        &tidy(&mul(vec![w, pow(y1.clone(), Expr::int(-2))])),
        x,
        None,
    )
    .ok()?;
    let y2 = tidy(&mul(vec![y1.clone(), inner]));
    let basis = vec![y1.clone(), y2.clone()];
    steps.push(
        "reduction of order",
        ode.original.clone(),
        Expr::equation(Expr::sym(&format!("{}₂", ode.y)), y2.clone()),
        Some(format!(
            "{y}₁ = {y1} solves the homogeneous equation; with P = {p}, {y}₂ = {y}₁ ∫ e^(−∫P d{x})/{y}₁² d{x} = {y2}",
            y = ode.y
        )),
    );
    let (particular, method) = if g.is_zero() {
        (None, "reduction of order")
    } else {
        let part = variation_of_parameters(ode, &basis, &a[2], g, steps)?;
        (Some(part), "reduction of order + variation of parameters")
    };
    Some(Ok(Candidate::explicit(
        general(&basis, consts, particular.as_ref()),
        method,
    )))
}

/// Simple solutions of the homogeneous equation: powers of `x` and exponentials.
fn guess_homogeneous(x: &str, works: &dyn Fn(&Expr) -> bool) -> Option<Expr> {
    let xs = Expr::sym(x);
    let mut tries: Vec<Expr> = Vec::new();
    for m in [1i64, 2, -1, 3, -2, 4, -3] {
        tries.push(pow(xs.clone(), Expr::int(m)));
    }
    for m in [1i64, -1, 2, -2, 3, -3] {
        tries.push(pow(Expr::e(), mul(vec![Expr::int(m), xs.clone()])));
    }
    tries.push(pow(Expr::e(), pow(xs.clone(), Expr::int(2))));
    tries.push(pow(Expr::e(), neg(pow(xs.clone(), Expr::int(2)))));
    tries.push(func(Func::Sin, vec![xs.clone()]));
    tries.push(func(Func::Cos, vec![xs.clone()]));
    tries.push(add(vec![xs.clone(), Expr::one()]));
    tries.into_iter().find(|t| works(t))
}
