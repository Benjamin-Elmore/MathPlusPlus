//! First-order equations: separable, linear (integrating factor), Bernoulli, homogeneous
//! `y′ = F(y/x)`, exact (and made exact by an integrating factor in `x` or in `y`), Riccati
//! with a known particular solution, and Clairaut.
//!
//! Each method returns a candidate; the caller substitutes it back before accepting it.

use std::collections::BTreeMap;

use crate::calc::diff::diff;
use crate::calc::integrate::integrate;
use crate::domains::solve::{isolate, solve, Solutions};
use crate::domains::Ctx;
use crate::error::MathResult;
use crate::expr::{Expr, ExprNode};
use crate::functions::{func, Func};
use crate::simplify::{add, mul, pow, simplify};
use crate::steps::Steps;

use super::parse::Ode;
use super::Candidate;

fn neg(e: Expr) -> Expr {
    mul(vec![Expr::int(-1), e])
}

fn div(a: Expr, b: Expr) -> Expr {
    simplify(&mul(vec![a, pow(b, Expr::int(-1))]))
}

fn int(e: &Expr, var: &str) -> Option<Expr> {
    integrate(&simplify(e), var, None).ok()
}

/// True when `e` is zero, symbolically or at sample points.
fn is_zero(e: &Expr) -> bool {
    let s = simplify(e);
    if s.is_zero() {
        return true;
    }
    if let Ok(x) = crate::expand::expand(&s) {
        if simplify(&x).is_zero() {
            return true;
        }
    }
    let names: Vec<String> = s.free_symbols().iter().map(|n| n.to_string()).collect();
    let mut good = 0;
    for i in 0..8 {
        let env: BTreeMap<String, f64> = names
            .iter()
            .enumerate()
            .map(|(j, n)| (n.clone(), 0.4 + 0.37 * i as f64 + 0.23 * j as f64))
            .collect();
        match s.eval_f64(&env) {
            Ok(v) if v.is_finite() => {
                if v.abs() > 1e-9 {
                    return false;
                }
                good += 1;
            }
            _ => {}
        }
    }
    good >= 4
}

/// `e^u`, writing `e^{k ln|v|}` as `v^k` (a constant multiple of an integrating factor is
/// one too, so the sign of `v` does not matter).
pub(crate) fn exp_of(u: &Expr) -> Expr {
    let u = simplify(u);
    let u = crate::expand::expand(&u).map(|e| simplify(&e)).unwrap_or(u);
    if let ExprNode::Add(terms) = u.node() {
        return simplify(&mul(terms.iter().map(exp_one).collect()));
    }
    exp_one(&u)
}

fn exp_one(u: &Expr) -> Expr {
    let u = simplify(u);
    let (k, rest) = crate::simplify::split_coeff(&u);
    if let ExprNode::Func(Func::Ln, args) = rest.node() {
        if let [a] = args.as_slice() {
            let v = match a.node() {
                ExprNode::Func(Func::Abs, inner) if inner.len() == 1 => inner[0].clone(),
                _ => a.clone(),
            };
            return simplify(&pow(v, Expr::num(k)));
        }
    }
    simplify(&pow(Expr::e(), u))
}

/// The equation as `N(x, y)·y′ + M(x, y) = 0`, when it is linear in `y′`.
fn m_n(ode: &Ode) -> Option<(Expr, Expr)> {
    let p = ode.d_name(1);
    let c = crate::poly::as_poly(&ode.residual, &p)?;
    if c.len() != 2 {
        return None;
    }
    let (m, n) = (simplify(&c[0]), simplify(&c[1]));
    if n.is_zero() || m.contains_sym(&p) || n.contains_sym(&p) {
        return None;
    }
    Some((m, n))
}

/// Tries every first-order method in turn; returns every candidate found, in order.
pub(crate) fn candidates(
    ode: &Ode,
    c: &str,
    particular: Option<&Expr>,
) -> Vec<MathResult<Candidate>> {
    let mut out = Vec::new();
    // Each method keeps its own working, attached to its candidate.
    let mut keep = |r: Option<MathResult<Candidate>>, local: Steps| {
        if let Some(r) = r {
            out.push(r.map(|mut cand| {
                cand.steps = local;
                cand
            }));
        }
    };
    if let Some((m, n)) = m_n(ode) {
        let f = div(neg(m.clone()), n.clone());
        type Method = fn(&Ode, &Expr, &str, &mut Steps) -> Option<MathResult<Candidate>>;
        let methods: [Method; 4] = [separable, linear, bernoulli, homogeneous];
        for method in methods {
            let mut local = Steps::new();
            let r = method(ode, &f, c, &mut local);
            keep(r, local);
        }
        let mut local = Steps::new();
        let r = exact(ode, &m, &n, c, &mut local);
        keep(r, local);
        let mut local = Steps::new();
        let r = riccati(ode, &f, c, particular, &mut local);
        keep(r, local);
    }
    let mut local = Steps::new();
    let r = clairaut(ode, c, &mut local);
    keep(r, local);
    out
}

/// Splits `f(x, y)` into `g(x)·h(y)` when it factors that way.
fn split_xy(f: &Expr, x: &str, y: &str) -> Option<(Expr, Expr)> {
    let try_split = |e: &Expr| -> Option<(Expr, Expr)> {
        let factors = match e.node() {
            ExprNode::Mul(fs) => fs.clone(),
            _ => vec![e.clone()],
        };
        if factors
            .iter()
            .any(|f| f.contains_sym(x) && f.contains_sym(y))
        {
            return None;
        }
        let (hs, gs): (Vec<Expr>, Vec<Expr>) = factors.into_iter().partition(|f| f.contains_sym(y));
        Some((simplify(&mul(gs)), simplify(&mul(hs))))
    };
    if let Some(s) = try_split(f) {
        return Some(s);
    }
    let factored = crate::factor::factor(f);
    if let Some(s) = try_split(&factored) {
        return Some(s);
    }
    // e^{a(x) + b(y)} = e^{a(x)} e^{b(y)}
    if let ExprNode::Pow(b, e) = f.node() {
        if matches!(b.node(), ExprNode::Const(crate::expr::Constant::E)) {
            if let ExprNode::Add(terms) = e.node() {
                if terms
                    .iter()
                    .all(|t| !(t.contains_sym(x) && t.contains_sym(y)))
                {
                    let (ys, xs): (Vec<Expr>, Vec<Expr>) =
                        terms.iter().cloned().partition(|t| t.contains_sym(y));
                    return Some((
                        simplify(&pow(Expr::e(), add(xs))),
                        simplify(&pow(Expr::e(), add(ys))),
                    ));
                }
            }
        }
    }
    None
}

/// `y′ = g(x) h(y)`: `∫ dy/h(y) = ∫ g(x) dx + C`.
fn separable(ode: &Ode, f: &Expr, c: &str, steps: &mut Steps) -> Option<MathResult<Candidate>> {
    let (x, y) = (ode.x.as_str(), ode.y.as_str());
    let (g, h) = split_xy(f, x, y)?;
    let big_g = int(&g, x)?;
    let cs = Expr::sym(c);
    let ys = Expr::sym(y);
    if !h.contains_sym(y) {
        let sol = simplify(&add(vec![mul(vec![h.clone(), big_g.clone()]), cs]));
        steps.push(
            "separable (direct integration)",
            ode.original.clone(),
            Expr::equation(ys, sol.clone()),
            Some(format!(
                "{y}′ depends on {x} only, so {y} = ∫ ({f}) d{x} + {c}"
            )),
        );
        return Some(Ok(Candidate::explicit(
            sol,
            "separable (direct integration)",
        )));
    }
    // h(y) = k·y: ln|y| = k G + C, so y = C e^{k G}.
    if let Some(cf) = crate::poly::as_poly(&h, y) {
        if cf.len() == 2 && cf[0].is_zero() && !cf[1].contains_sym(y) {
            let sol = simplify(&mul(vec![
                cs.clone(),
                exp_of(&mul(vec![cf[1].clone(), big_g.clone()])),
            ]));
            steps.push(
                "separable",
                ode.original.clone(),
                Expr::equation(ys, sol.clone()),
                Some(format!(
                    "separate: d{y}/{y} = ({}) d{x}; integrate: ln|{y}| = {} + C, so {y} = C·e^({})",
                    simplify(&mul(vec![cf[1].clone(), g.clone()])),
                    simplify(&mul(vec![cf[1].clone(), big_g.clone()])),
                    simplify(&mul(vec![cf[1].clone(), big_g.clone()]))
                )),
            );
            return Some(Ok(Candidate::explicit(sol, "separable")));
        }
    }
    let big_h = int(&pow(h.clone(), Expr::int(-1)), y)?;
    let rhs = simplify(&add(vec![big_g.clone(), cs]));
    let note =
        format!("separate: d{y}/({h}) = ({g}) d{x}; integrate both sides: {big_h} = {big_g} + {c}");
    let cand = explicit_or_implicit(&big_h, &rhs, y, "separable");
    steps.push(
        "separable",
        ode.original.clone(),
        cand.form.relation(y),
        Some(note),
    );
    let zeros = equilibria(&h, y);
    let mut cand = cand;
    if !zeros.is_empty() {
        cand.notes.push(format!(
            "dividing by h({y}) = {h} loses the constant solutions {}",
            zeros
                .iter()
                .map(|z| format!("{y} = {z}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Some(Ok(cand))
}

/// Constant solutions `h(y) = 0`.
fn equilibria(h: &Expr, y: &str) -> Vec<Expr> {
    let mut ctx = Ctx::new();
    match solve(h, y, None, &mut ctx) {
        Ok(Solutions::Finite {
            values,
            exact: true,
        }) => values,
        _ => Vec::new(),
    }
}

/// `lhs(y) = rhs(x)` solved for `y` when there is one branch, otherwise kept implicit.
fn explicit_or_implicit(lhs: &Expr, rhs: &Expr, y: &str, method: &'static str) -> Candidate {
    if let Ok(Some(iso)) = isolate::isolate(lhs, rhs, y, "k") {
        if let [one] = iso.values.as_slice() {
            if !iso.periodic {
                return Candidate::explicit(simplify(one), method);
            }
        }
    }
    Candidate::implicit(simplify(&add(vec![lhs.clone(), neg(rhs.clone())])), method)
}

/// `y′ = a(x) y + b(x)`: with `P = −a`, `μ = e^{∫P}` and `y = (∫ μ b dx + C)/μ`.
fn linear(ode: &Ode, f: &Expr, c: &str, steps: &mut Steps) -> Option<MathResult<Candidate>> {
    let (x, y) = (ode.x.as_str(), ode.y.as_str());
    let cf = crate::poly::as_poly(f, y)?;
    if cf.len() != 2 || cf.iter().any(|e| e.contains_sym(y)) {
        return None;
    }
    let (b, a) = (simplify(&cf[0]), simplify(&cf[1]));
    let sol = linear_solution(&neg(a.clone()), &b, x, c)?;
    steps.push(
        "linear (integrating factor)",
        ode.original.clone(),
        Expr::equation(Expr::sym(y), sol.0.clone()),
        Some(format!(
            "standard form {y}′ + P{y} = Q with P = {}, Q = {b}; integrating factor μ = e^(∫P d{x}) = {}; {y} = (∫ μQ d{x} + {c})/μ",
            simplify(&neg(a)),
            sol.1
        )),
    );
    Some(Ok(Candidate::explicit(
        sol.0,
        "linear (integrating factor)",
    )))
}

/// `y′ + P y = Q`: the solution and the integrating factor.
pub(crate) fn linear_solution(p: &Expr, q: &Expr, x: &str, c: &str) -> Option<(Expr, Expr)> {
    let int_p = int(p, x)?;
    let mu = exp_of(&int_p);
    let int_mq = if q.is_zero() {
        Expr::zero()
    } else {
        int(&simplify(&mul(vec![mu.clone(), q.clone()])), x)?
    };
    let sol = div(add(vec![int_mq, Expr::sym(c)]), mu.clone());
    let sol = crate::expand::expand(&sol)
        .map(|e| simplify(&e))
        .ok()
        .filter(|e| e.size() <= sol.size())
        .unwrap_or(sol);
    Some((sol, mu))
}

/// The terms of `f` as `(power of y, coefficient)`.
fn y_powers(f: &Expr, y: &str) -> Option<Vec<(Expr, Expr)>> {
    let e = crate::expand::expand(f).ok()?;
    let terms = match e.node() {
        ExprNode::Add(ts) => ts.clone(),
        _ => vec![e.clone()],
    };
    let mut out: Vec<(Expr, Expr)> = Vec::new();
    for t in terms {
        let factors = match t.node() {
            ExprNode::Mul(fs) => fs.clone(),
            _ => vec![t.clone()],
        };
        let mut power = Expr::zero();
        let mut coeff = Vec::new();
        for f in factors {
            if f.is_sym(y) {
                power = add(vec![power, Expr::one()]);
            } else if let ExprNode::Pow(b, k) = f.node() {
                if b.is_sym(y) && !k.contains_sym(y) {
                    power = add(vec![power, k.clone()]);
                } else if f.contains_sym(y) {
                    return None;
                } else {
                    coeff.push(f.clone());
                }
            } else if f.contains_sym(y) {
                return None;
            } else {
                coeff.push(f.clone());
            }
        }
        let power = simplify(&power);
        let coeff = simplify(&mul(coeff));
        match out.iter_mut().find(|(p, _)| *p == power) {
            Some((_, c)) => *c = simplify(&add(vec![c.clone(), coeff])),
            None => out.push((power, coeff)),
        }
    }
    Some(out)
}

/// `y′ = a(x) y + b(x) yⁿ` (`n ≠ 0, 1`): `v = y^(1−n)` makes it linear.
fn bernoulli(ode: &Ode, f: &Expr, c: &str, steps: &mut Steps) -> Option<MathResult<Candidate>> {
    let (x, y) = (ode.x.as_str(), ode.y.as_str());
    let terms = y_powers(f, y)?;
    let (mut a, mut b, mut n) = (Expr::zero(), None, None);
    for (p, coeff) in terms {
        if p.is_one() {
            a = coeff;
        } else if p.is_zero() {
            return None;
        } else {
            if n.is_some() {
                return None;
            }
            n = Some(p);
            b = Some(coeff);
        }
    }
    let (n, b) = (n?, b?);
    let one_minus_n = simplify(&add(vec![Expr::one(), neg(n.clone())]));
    // v′ = (1 − n)(a v + b)
    let p = simplify(&neg(mul(vec![one_minus_n.clone(), a.clone()])));
    let q = simplify(&mul(vec![one_minus_n.clone(), b.clone()]));
    let (v, _) = linear_solution(&p, &q, x, c)?;
    let sol = simplify(&pow(v.clone(), pow(one_minus_n.clone(), Expr::int(-1))));
    steps.push(
        "Bernoulli",
        ode.original.clone(),
        Expr::equation(Expr::sym(y), sol.clone()),
        Some(format!(
            "{y}′ = ({a}){y} + ({b}){y}^{n}; with v = {y}^({one_minus_n}) the equation becomes linear: v′ + ({p})v = {q}, so v = {v}"
        )),
    );
    let mut cand = Candidate::explicit(sol, "Bernoulli");
    if crate::calc_support::value(&n).is_some_and(|k| k > 0.0) {
        cand.notes.push(format!("{y} = 0 is also a solution"));
    }
    Some(Ok(cand))
}

/// `y′ = F(y/x)`: with `y = v x`, `x v′ = F(v) − v` is separable.
fn homogeneous(ode: &Ode, f: &Expr, c: &str, steps: &mut Steps) -> Option<MathResult<Candidate>> {
    let (x, y) = (ode.x.as_str(), ode.y.as_str());
    if !f.contains_sym(x) || !f.contains_sym(y) {
        return None;
    }
    // Degree-zero homogeneity: f(t x, t y) = f(x, y) at sample points.
    for (xv, yv, t) in [(1.3, 0.7, 2.1), (0.6, 1.9, 0.45), (2.2, 1.1, 3.3)] {
        let env1: BTreeMap<String, f64> = [(x.to_string(), xv), (y.to_string(), yv)]
            .into_iter()
            .collect();
        let env2: BTreeMap<String, f64> = [(x.to_string(), t * xv), (y.to_string(), t * yv)]
            .into_iter()
            .collect();
        match (f.eval_f64(&env1), f.eval_f64(&env2)) {
            (Ok(a), Ok(b)) if (a - b).abs() <= 1e-9 * (1.0 + a.abs()) => {}
            _ => return None,
        }
    }
    let v = "v";
    let fv = simplify(&f.subs_all(&[(x, Expr::one()), (y, Expr::sym(v))]));
    let h = simplify(&add(vec![fv.clone(), neg(Expr::sym(v))]));
    let ratio = div(Expr::sym(y), Expr::sym(x));
    if is_zero(&h) {
        let sol = simplify(&mul(vec![Expr::sym(c), Expr::sym(x)]));
        return Some(Ok(Candidate::explicit(sol, "homogeneous")));
    }
    let big_h = int(&pow(h.clone(), Expr::int(-1)), v)?;
    let lhs = simplify(&big_h.subs(v, &ratio));
    let rhs = simplify(&add(vec![
        func(Func::Ln, vec![func(Func::Abs, vec![Expr::sym(x)])]),
        Expr::sym(c),
    ]));
    let cand = explicit_or_implicit(&lhs, &rhs, y, "homogeneous");
    steps.push(
        "homogeneous",
        ode.original.clone(),
        cand.form.relation(y),
        Some(format!(
            "{y}′ = F({y}/{x}) with F(v) = {fv}; put {y} = v{x}: {x}v′ = F(v) − v = {h}, which separates: ∫ dv/({h}) = ln|{x}| + {c}, i.e. {big_h} = ln|{x}| + {c} with v = {y}/{x}"
        )),
    );
    Some(Ok(cand))
}

/// `M dx + N dy = 0` with `M_y = N_x` (or made so by `μ(x)` or `μ(y)`): `Φ(x, y) = C`.
fn exact(
    ode: &Ode,
    m: &Expr,
    n: &Expr,
    c: &str,
    steps: &mut Steps,
) -> Option<MathResult<Candidate>> {
    let (x, y) = (ode.x.as_str(), ode.y.as_str());
    let my = diff(m, y, 1, None).ok()?;
    let nx = diff(n, x, 1, None).ok()?;
    let gap = simplify(&add(vec![my.clone(), neg(nx.clone())]));
    let (m2, n2, factor_note, method) = if is_zero(&gap) {
        (m.clone(), n.clone(), String::new(), "exact")
    } else {
        let by_x = div(gap.clone(), n.clone());
        let by_y = div(neg(gap.clone()), m.clone());
        let mu = if !by_x.contains_sym(y) && !by_x.contains_sym(&ode.d_name(1)) {
            Some((exp_of(&int(&by_x, x)?), x))
        } else if !by_y.contains_sym(x) {
            Some((exp_of(&int(&by_y, y)?), y))
        } else {
            None
        }?;
        (
            simplify(&mul(vec![mu.0.clone(), m.clone()])),
            simplify(&mul(vec![mu.0.clone(), n.clone()])),
            format!(
                "M_{y} − N_{x} = {gap} ≠ 0; the integrating factor μ({}) = {} makes it exact; ",
                mu.1, mu.0
            ),
            "exact (integrating factor)",
        )
    };
    let phi_x = int(&m2, x)?;
    let rest = simplify(&add(vec![n2.clone(), neg(diff(&phi_x, y, 1, None).ok()?)]));
    if rest.contains_sym(x) && !is_zero(&diff(&rest, x, 1, None).ok()?) {
        return None;
    }
    let rest = if rest.contains_sym(x) {
        simplify(&rest.subs(x, &Expr::one()))
    } else {
        rest
    };
    let phi = simplify(&add(vec![phi_x, int(&rest, y)?]));
    let cand = explicit_or_implicit(&phi, &Expr::sym(c), y, method);
    steps.push(
        method,
        ode.original.clone(),
        cand.form.relation(y),
        Some(format!(
            "{factor_note}M = {m2}, N = {n2} with M_{y} = N_{x}; the potential Φ with Φ_{x} = M, Φ_{y} = N is Φ = {phi}, so Φ = {c}"
        )),
    );
    Some(Ok(cand))
}

/// `y′ = q₀ + q₁ y + q₂ y²` with a particular solution `y₁`: `y = y₁ + 1/v` with `v` linear.
fn riccati(
    ode: &Ode,
    f: &Expr,
    c: &str,
    particular: Option<&Expr>,
    steps: &mut Steps,
) -> Option<MathResult<Candidate>> {
    let (x, y) = (ode.x.as_str(), ode.y.as_str());
    let q = crate::poly::as_poly(&crate::expand::expand(f).ok()?, y)?;
    if q.len() != 3 || q.iter().any(|e| e.contains_sym(y)) || simplify(&q[2]).is_zero() {
        return None;
    }
    let (q0, q1, q2) = (simplify(&q[0]), simplify(&q[1]), simplify(&q[2]));
    let check = |y1: &Expr| -> bool {
        let Ok(d) = diff(y1, x, 1, None) else {
            return false;
        };
        is_zero(&add(vec![d, neg(f.subs(y, y1))]))
    };
    let y1 = match particular {
        Some(p) if check(p) => p.clone(),
        Some(_) => return None,
        None => guess_particular(f, x, y, &check)?,
    };
    // v′ = −(q₁ + 2 q₂ y₁) v − q₂
    let p = simplify(&add(vec![
        q1.clone(),
        mul(vec![Expr::int(2), q2.clone(), y1.clone()]),
    ]));
    let (v, _) = linear_solution(&p, &neg(q2.clone()), x, c)?;
    let sol = simplify(&add(vec![y1.clone(), pow(v.clone(), Expr::int(-1))]));
    steps.push(
        "Riccati",
        ode.original.clone(),
        Expr::equation(Expr::sym(y), sol.clone()),
        Some(format!(
            "{y}′ = ({q0}) + ({q1}){y} + ({q2}){y}²; a particular solution is {y}₁ = {y1}; with {y} = {y}₁ + 1/v the equation becomes v′ + ({p})v = {}, so v = {v}",
            simplify(&neg(q2.clone()))
        )),
    );
    Some(Ok(Candidate::explicit(sol, "Riccati")))
}

/// Simple particular solutions: constants and `a xᵐ`.
fn guess_particular(f: &Expr, x: &str, y: &str, check: &dyn Fn(&Expr) -> bool) -> Option<Expr> {
    let a = "\u{1d44e}";
    for m in [0i64, 1, -1, 2, -2, 3] {
        let trial = mul(vec![Expr::sym(a), pow(Expr::sym(x), Expr::int(m))]);
        let d = diff(&trial, x, 1, None).ok()?;
        let r = simplify(&add(vec![d, neg(f.subs(y, &trial))]));
        let at = simplify(&r.subs(x, &Expr::int(2)));
        let mut ctx = Ctx::new();
        if let Ok(Solutions::Finite {
            values,
            exact: true,
        }) = solve(&at, a, None, &mut ctx)
        {
            for v in values {
                let cand = simplify(&trial.subs(a, &v));
                if !cand.is_zero() && v.eval_f64(&crate::eval::EmptyEnv).is_ok() && check(&cand) {
                    return Some(cand);
                }
            }
        }
    }
    None
}

/// Clairaut: `y = x y′ + g(y′)` has the lines `y = C x + g(C)` (and a singular envelope).
fn clairaut(ode: &Ode, c: &str, steps: &mut Steps) -> Option<MathResult<Candidate>> {
    let (x, y) = (ode.x.as_str(), ode.y.as_str());
    let p = ode.d_name(1);
    let r = &ode.residual;
    let cy = crate::poly::as_poly(r, y)?;
    if cy.len() != 2
        || cy[1].contains_sym(x)
        || cy[1].contains_sym(&p)
        || simplify(&cy[1]).is_zero()
    {
        return None;
    }
    // r = k·(y − x p − g(p))
    let k = simplify(&cy[1]);
    let rest = div(neg(cy[0].clone()), k.clone());
    let g = simplify(&add(vec![
        rest.clone(),
        neg(mul(vec![Expr::sym(x), Expr::sym(&p)])),
    ]));
    let g = crate::expand::expand(&g).map(|e| simplify(&e)).unwrap_or(g);
    if g.contains_sym(x) || !g.contains_sym(&p) {
        return None;
    }
    // Linear in p would be an ordinary linear equation; Clairaut needs g non-linear.
    if crate::poly::as_poly(&g, &p).is_some_and(|cf| cf.len() <= 2) {
        return None;
    }
    let cs = Expr::sym(c);
    let sol = simplify(&add(vec![
        mul(vec![cs.clone(), Expr::sym(x)]),
        g.subs(&p, &cs),
    ]));
    let mut cand = Candidate::explicit(sol.clone(), "Clairaut");
    // Singular solution: x + g′(p) = 0.
    if let Ok(gp) = diff(&g, &p, 1, None) {
        if let Ok(Some(iso)) = isolate::isolate(&gp, &neg(Expr::sym(x)), &p, "k") {
            if let [pv] = iso.values.as_slice() {
                let singular = simplify(&add(vec![
                    mul(vec![Expr::sym(x), pv.clone()]),
                    g.subs(&p, pv),
                ]));
                cand.notes.push(format!(
                    "singular solution (the envelope of the lines): {y} = {singular}"
                ));
            }
        }
    }
    steps.push(
        "Clairaut",
        ode.original.clone(),
        Expr::equation(Expr::sym(y), sol),
        Some(format!(
            "{y} = {x}{y}′ + g({y}′) with g(p) = {}; the general solution is the family of lines {y} = {c}{x} + g({c})",
            g.subs(&p, &Expr::sym("p"))
        )),
    );
    Some(Ok(cand))
}
