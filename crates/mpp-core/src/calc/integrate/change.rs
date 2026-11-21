//! Changes of variable that u-substitution cannot find, because the new variable is not a
//! sub-expression with its derivative beside it:
//! - `u = e^{ax}` for functions of exponentials (`1/(1 + e^x)`);
//! - `u = x^k` for `x^{k−1} h(x^k)` (`x/√(1 − x⁴)`);
//! - `x = e^u` for functions of `ln x` (`cos(ln x)`);
//! - `x = 1/u` for a power of `x` times a root of a quadratic (`1/(x²√(x² − 9))`);
//! - `t = tan(x/2)` (Weierstrass) for rational functions of `sin x` and `cos x`.
//!
//! Every result is differentiated back by the caller before it is returned.

use crate::expr::{Constant, Expr, ExprNode};
use crate::functions::{func, Func};
use crate::number::Number;
use crate::poly::as_poly;
use crate::simplify::{add, mul, pow, simplify};
use crate::steps::Steps;

use super::engine::{div, fresh_name, linear_coeffs, Integrator};
use super::holder;

/// Integrates `e` by one of the changes of variable above.
pub(crate) fn change_of_variable(
    eng: &mut Integrator,
    e: &Expr,
    steps: &mut Steps,
) -> Option<Expr> {
    let var = eng.var.clone();
    let x = Expr::sym(&var);
    let u_name = fresh_name(&[e], &var);
    let u = Expr::sym(&u_name);
    let mut tries: Vec<Change> = Vec::new();
    if let Some(a) = exponential_rate(e, &var) {
        // x = ln(u)/a, dx = du/(a u), u = e^{a x}
        tries.push(Change {
            x_of_u: div(func(Func::Ln, vec![u.clone()]), a.clone()),
            dx: div(Expr::one(), mul(vec![a.clone(), u.clone()])),
            back: pow(Expr::e(), mul(vec![a.clone(), x.clone()])),
            rule: "substitution u = e^(ax)",
        });
    }
    for k in 2..=4i64 {
        if has_power_multiple(e, &var, k) {
            // x = u^{1/k}, dx = u^{1/k − 1}/k du, u = x^k
            let inv = Expr::rational(1, k);
            tries.push(Change {
                x_of_u: pow(u.clone(), inv.clone()),
                dx: mul(vec![
                    inv.clone(),
                    pow(u.clone(), add(vec![inv, Expr::int(-1)])),
                ]),
                back: pow(x.clone(), Expr::int(k)),
                rule: "substitution u = x^k",
            });
        }
    }
    if mentions(
        e,
        &|t| matches!(t.node(), ExprNode::Func(Func::Ln, a) if a[0].is_sym(&var)),
    ) {
        // x = e^u, dx = e^u du, u = ln x
        tries.push(Change {
            x_of_u: pow(Expr::e(), u.clone()),
            dx: pow(Expr::e(), u.clone()),
            back: func(Func::Ln, vec![x.clone()]),
            rule: "substitution x = e^u",
        });
    }
    for c in tries {
        let Some(in_u) = c.integrand(e, &var, &u) else {
            continue;
        };
        if let Some(r) = run(eng, e, &in_u, &u_name, &c.back, None, c.rule, steps) {
            return Some(r);
        }
    }
    if let Some((in_u, key, q_x)) = reciprocal(e, &var, &u) {
        let back = div(Expr::one(), x.clone());
        let root = Some((&key, &q_x));
        if let Some(r) = run(
            eng,
            e,
            &in_u,
            &u_name,
            &back,
            root,
            "substitution x = 1/u",
            steps,
        ) {
            return Some(r);
        }
    }
    if let Some(in_t) = weierstrass(e, &var, &u) {
        let back = func(Func::Tan, vec![div(x.clone(), Expr::int(2))]);
        if let Some(r) = run(
            eng,
            e,
            &in_t,
            &u_name,
            &back,
            None,
            "Weierstrass substitution t = tan(x/2)",
            steps,
        ) {
            return Some(r);
        }
    }
    None
}

struct Change {
    x_of_u: Expr,
    dx: Expr,
    back: Expr,
    rule: &'static str,
}

impl Change {
    /// `e(x(u)) · dx/du` when it no longer mentions `x` (or a logarithm of `u` the change
    /// should have removed).
    fn integrand(&self, e: &Expr, var: &str, u: &Expr) -> Option<Expr> {
        let in_u = simplify(&ln_of_exp(&simplify(&mul(vec![
            e.subs(var, &self.x_of_u),
            self.dx.clone(),
        ]))));
        let u_name = u.as_sym()?;
        let leftover_ln = mentions(
            &in_u,
            &|t| matches!(t.node(), ExprNode::Func(Func::Ln, a) if a[0].contains_sym(u_name)),
        );
        (!in_u.contains_sym(var) && !leftover_ln && in_u.size() <= 4 * e.size() + 12)
            .then_some(in_u)
    }
}

/// Integrates the integrand in `u` and puts `u = back` back in.
#[allow(clippy::too_many_arguments)]
fn run(
    eng: &mut Integrator,
    e: &Expr,
    in_u: &Expr,
    u: &str,
    back: &Expr,
    back_root: Option<(&Expr, &Expr)>,
    rule: &str,
    steps: &mut Steps,
) -> Option<Expr> {
    let var = eng.var.clone();
    eng.attempt(steps, |s, l| {
        l.push(
            rule,
            holder(e, &var),
            holder(in_u, u),
            Some(format!("{u} = {back}")),
        );
        let inner = s.int_in(in_u, u, l)?;
        let result = match back_root {
            Some((key, q_x)) => back_with_root(&inner, key, q_x, u, &var),
            None => simplify(&inner.subs(u, back)),
        };
        let result = simplify(&ln_of_exp(&result));
        l.push(
            "substitute back",
            inner,
            result.clone(),
            Some(format!("{u} = {back}")),
        );
        Some(result)
    })
}

/// `u = 1/x` put back with every power `Q_u^q` of the quadratic written as
/// `Q(x)^q · x^{−2q}` (the form that holds for both signs of `x`), then `u = 1/x`.
fn back_with_root(e: &Expr, key: &Expr, q_x: &Expr, u: &str, var: &str) -> Expr {
    fn walk(e: &Expr, key: &Expr, q_x: &Expr, x: &Expr) -> Expr {
        if let ExprNode::Pow(b, q) = e.node() {
            if b == key {
                return mul(vec![
                    pow(q_x.clone(), q.clone()),
                    pow(x.clone(), mul(vec![Expr::int(-2), q.clone()])),
                ]);
            }
        }
        if e == key {
            return mul(vec![q_x.clone(), pow(x.clone(), Expr::int(-2))]);
        }
        crate::simplify::canonical(e.map_children(&mut |c| walk(c, key, q_x, x)))
    }
    let x = Expr::sym(var);
    let rooted = walk(e, key, q_x, &x);
    simplify(&rooted.subs(u, &div(Expr::one(), x)))
}

/// `ln(e^g) = g` and `ln|e^g| = g` (the new variable and the old one are real here).
fn ln_of_exp(e: &Expr) -> Expr {
    let node = e.map_children(&mut |c| ln_of_exp(c));
    let rebuilt = crate::simplify::canonical(node);
    if let ExprNode::Func(Func::Ln, args) = rebuilt.node() {
        let inner = match args[0].node() {
            ExprNode::Func(Func::Abs, a) => &a[0],
            _ => &args[0],
        };
        if let ExprNode::Pow(b, g) = inner.node() {
            if matches!(b.node(), ExprNode::Const(Constant::E)) {
                return g.clone();
            }
        }
    }
    rebuilt
}

fn mentions(e: &Expr, pred: &dyn Fn(&Expr) -> bool) -> bool {
    pred(e) || e.children().into_iter().any(|c| mentions(c, pred))
}

/// The rate `a` of the first exponential `e^{a x}` (exponent linear, no constant term).
fn exponential_rate(e: &Expr, var: &str) -> Option<Expr> {
    if let ExprNode::Pow(b, k) = e.node() {
        if matches!(b.node(), ExprNode::Const(Constant::E)) {
            if let Some((a, c)) = linear_coeffs(k, var) {
                if c.is_zero() {
                    return Some(a);
                }
            }
        }
    }
    e.children()
        .into_iter()
        .find_map(|c| exponential_rate(c, var))
}

/// True when `e` has a power `x^m` with `m` a multiple of `k` greater than `k` (so `x^k`
/// is worth trying as a new variable).
fn has_power_multiple(e: &Expr, var: &str, k: i64) -> bool {
    mentions(e, &|t| match t.node() {
        ExprNode::Pow(b, m) if b.is_sym(var) => m
            .as_number()
            .and_then(Number::as_i64)
            .is_some_and(|m| m.abs() > k && m % k == 0),
        _ => false,
    })
}

/// `x = 1/u` in `xⁿ · (a x² + b x + c)^{p/2}` (n a negative integer), with the root of
/// the quadratic written as `(a + b u + c u²)^{p/2} / u^p` (for x > 0; the caller checks
/// the result by differentiating it).
fn reciprocal(e: &Expr, var: &str, u: &Expr) -> Option<(Expr, Expr, Expr)> {
    let factors = match e.node() {
        ExprNode::Mul(fs) => fs.clone(),
        _ => return None,
    };
    let mut power_of_x: i64 = 0;
    let mut root = None;
    for f in &factors {
        match f.node() {
            ExprNode::Sym(_) if f.is_sym(var) => power_of_x += 1,
            ExprNode::Pow(b, m) if b.is_sym(var) => power_of_x += m.as_number()?.as_i64()?,
            ExprNode::Pow(b, m) if root.is_none() => {
                let p = m.as_number()?.as_rational()?.clone();
                if p.denom() != &2.into() {
                    return None;
                }
                let c = as_poly(b, var)?;
                if c.len() != 3 || c.iter().any(|k| k.contains_sym(var)) {
                    return None;
                }
                root = Some((c, p, b.clone()));
            }
            _ if !f.contains_sym(var) => {}
            _ => return None,
        }
    }
    let (c, p, q_x) = root?;
    if power_of_x >= -1 {
        return None;
    }
    // Q(1/u) = (c₂ + c₁ u + c₀ u²)/u²
    let q_u = add(vec![
        c[2].clone(),
        mul(vec![c[1].clone(), u.clone()]),
        mul(vec![c[0].clone(), pow(u.clone(), Expr::int(2))]),
    ]);
    let p_expr = Expr::ratio(p);
    let consts: Vec<Expr> = factors
        .iter()
        .filter(|f| !f.contains_sym(var))
        .cloned()
        .collect();
    // xⁿ = u^{−n}; root = Q_u^{p} · u^{−2p}; dx = −du/u²
    let mut parts = consts;
    parts.push(pow(u.clone(), Expr::int(-power_of_x)));
    let q_key = q_u.clone();
    parts.push(pow(q_u, p_expr.clone()));
    parts.push(pow(u.clone(), mul(vec![Expr::int(-2), p_expr])));
    parts.push(Expr::int(-1));
    parts.push(pow(u.clone(), Expr::int(-2)));
    Some((simplify(&mul(parts)), simplify(&q_key), q_x))
}

/// `t = tan(x/2)` in a rational function of `sin x` and `cos x` (nothing else depends on
/// `x`): `sin x = 2t/(1+t²)`, `cos x = (1−t²)/(1+t²)`, `dx = 2/(1+t²) dt`.
fn weierstrass(e: &Expr, var: &str, t: &Expr) -> Option<Expr> {
    fn only_sin_cos(e: &Expr, var: &str) -> bool {
        if !e.contains_sym(var) {
            return true;
        }
        match e.node() {
            ExprNode::Func(Func::Sin | Func::Cos, a) => a[0].is_sym(var),
            ExprNode::Add(v) | ExprNode::Mul(v) => v.iter().all(|c| only_sin_cos(c, var)),
            ExprNode::Pow(b, k) => {
                k.as_number().is_some_and(Number::is_integer) && only_sin_cos(b, var)
            }
            _ => false,
        }
    }
    let x = Expr::sym(var);
    let has_cos = mentions(e, &|c| matches!(c.node(), ExprNode::Func(Func::Cos, _)));
    let has_sin = mentions(e, &|c| matches!(c.node(), ExprNode::Func(Func::Sin, _)));
    if !(has_cos || has_sin)
        || !only_sin_cos(e, var)
        || !mentions(
            e,
            &|c| matches!(c.node(), ExprNode::Pow(_, k) if k.as_number().is_some_and(Number::is_negative)),
        )
    {
        return None;
    }
    let one_t2 = add(vec![Expr::one(), pow(t.clone(), Expr::int(2))]);
    let sin = div(mul(vec![Expr::int(2), t.clone()]), one_t2.clone());
    let cos = div(
        add(vec![
            Expr::one(),
            mul(vec![Expr::int(-1), pow(t.clone(), Expr::int(2))]),
        ]),
        one_t2.clone(),
    );
    let sin_x = func(Func::Sin, vec![x.clone()]);
    let cos_x = func(Func::Cos, vec![x]);
    let replaced = e.subs_expr(&sin_x, &sin).subs_expr(&cos_x, &cos);
    let in_t = simplify(&mul(vec![replaced, div(Expr::int(2), one_t2)]));
    // One fraction of polynomials in t, for partial fractions.
    let t_name = t.as_sym()?;
    let (n, d) = super::rational::to_rational(&in_t, t_name)?;
    Some(div(n.to_expr(t_name), d.to_expr(t_name)))
}
