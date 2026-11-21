//! u-substitution: find an inner function `g` with `f = h(g)·g′` up to a constant factor.

use num_traits::{One, Signed};

use crate::calc::diff::diff;
use crate::expr::{Expr, ExprNode};
use crate::factor::factor;
use crate::number::Number;
use crate::simplify::{add, mul, pow, simplify};
use crate::steps::Steps;

use super::engine::{div, fresh_name, linear_coeffs, Integrator};
use super::holder;

/// Most candidate inner functions tried for one integrand.
const MAX_CANDIDATES: usize = 14;

/// Integrates `e` by a substitution `u = g(x)`, if one turns it into an integral in `u` alone.
pub(crate) fn substitution(eng: &mut Integrator, e: &Expr, steps: &mut Steps) -> Option<Expr> {
    let var = eng.var.clone();
    let mut tagged = Vec::new();
    collect(e, &var, &mut tagged);
    tagged.retain(|(_, g)| g != e && !g.is_sym(&var));
    tagged.sort_by_key(|(k, g)| (*k, std::cmp::Reverse(g.size())));
    let mut cands: Vec<Expr> = Vec::new();
    for (_, g) in tagged {
        if !cands.contains(&g) {
            cands.push(g);
        }
    }
    cands.truncate(MAX_CANDIDATES);
    let u = fresh_name(&[e], &var);
    for g in cands {
        let Some((integrand, dg)) = rewrite(e, &g, &var, &u) else {
            continue;
        };
        if let Some(r) = eng.attempt(steps, |s, l| {
            let note = format!("{u} = {g}, d{u} = {dg} d{var}");
            l.push(
                "substitution",
                holder(e, &var),
                holder(&integrand, &u),
                Some(note),
            );
            let inner = s.int_in(&integrand, &u, l)?;
            let back = simplify(&inner.subs(&u, &g));
            l.push(
                "substitute back",
                inner,
                back.clone(),
                Some(format!("{u} = {g}")),
            );
            Some(back)
        }) {
            return Some(r);
        }
    }
    None
}

/// The integrand in `u` and `g′` when `e = h(g)·g′` (up to a constant), else `None`.
fn rewrite(e: &Expr, g: &Expr, var: &str, u: &str) -> Option<(Expr, Expr)> {
    let dg = diff(g, var, 1, None).ok()?;
    if dg.is_zero() || dg.is_undefined() {
        return None;
    }
    let ratio = simplify(&div(e.clone(), dg.clone()));
    let usym = Expr::sym(u);
    let mut forms = vec![ratio.clone()];
    let factored = factor(&ratio);
    if factored != ratio {
        forms.push(factored);
    }
    for r in &forms {
        let in_u = simplify(&r.subs_expr(g, &usym));
        if !in_u.contains_sym(var) {
            return Some((in_u, dg));
        }
        // Invert simple inner functions: g = a x + b, or g = (a x + b)^(1/n).
        if let Some(x_of_u) = inverse(g, var, &usym) {
            let in_u = simplify(&r.subs(var, &x_of_u));
            if !in_u.contains_sym(var) && in_u.size() <= 4 * e.size() + 8 {
                return Some((in_u, dg));
            }
        }
    }
    None
}

/// `x` as a function of `u` for `u = a x + b` or `u = (a x + b)^(1/n)`.
fn inverse(g: &Expr, var: &str, u: &Expr) -> Option<Expr> {
    if let Some((a, b)) = linear_coeffs(g, var) {
        return Some(div(add(vec![u.clone(), mul(vec![Expr::int(-1), b])]), a));
    }
    if let ExprNode::Pow(base, exp) = g.node() {
        let r = exp.as_number().and_then(Number::as_rational)?;
        if r.is_integer() || !r.numer().abs().is_one() {
            return None;
        }
        let (a, b) = linear_coeffs(base, var)?;
        let n = Expr::ratio(r.recip());
        let base_of_u = pow(u.clone(), n);
        return Some(div(add(vec![base_of_u, mul(vec![Expr::int(-1), b])]), a));
    }
    None
}

/// Inner functions worth trying as `u`, tagged with a preference (lower first): arguments
/// and exponents (the textbook "inner function"), then bases of powers, then whole
/// functions and exponentials.
fn collect(e: &Expr, var: &str, out: &mut Vec<(u8, Expr)>) {
    if !e.contains_sym(var) {
        return;
    }
    match e.node() {
        ExprNode::Func(_, args) => {
            out.push((2, e.clone()));
            for a in args {
                out.push((0, a.clone()));
                collect(a, var, out);
            }
        }
        ExprNode::Pow(b, x) => {
            if b.contains_sym(var) {
                out.push((1, b.clone()));
                let fractional = x
                    .as_number()
                    .and_then(Number::as_rational)
                    .is_some_and(|r| !r.is_integer());
                if fractional {
                    out.push((2, e.clone()));
                }
                collect(b, var, out);
            }
            if x.contains_sym(var) {
                out.push((0, x.clone()));
                if !b.contains_sym(var) {
                    out.push((2, e.clone()));
                }
                collect(x, var, out);
            }
        }
        ExprNode::Add(v) | ExprNode::Mul(v) => {
            for c in v {
                collect(c, var, out);
            }
        }
        _ => {}
    }
}
