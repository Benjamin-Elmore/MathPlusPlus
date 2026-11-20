//! Integration by parts, `∫ u dv = u·v − ∫ v du`, with `u` chosen by LIATE
//! (logarithmic, inverse trig, algebraic, trigonometric, exponential), repeated as needed,
//! and the cyclic case (`eˣ sin x`) solved for the integral.

use crate::calc::diff::diff;
use crate::expr::{Expr, ExprNode};
use crate::functions::Func;
use crate::poly::is_polynomial_in;
use crate::simplify::{add, mul, simplify};
use crate::steps::Steps;

use super::engine::{div, Integrator};
use super::holder;

/// LIATE classes, in increasing priority for `u`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Liate {
    Exponential,
    Trig,
    Algebraic,
    InverseTrig,
    Log,
}

fn class(f: &Expr, var: &str) -> Option<Liate> {
    match f.node() {
        ExprNode::Func(Func::Ln | Func::Log, _) => Some(Liate::Log),
        ExprNode::Func(
            Func::Asin | Func::Acos | Func::Atan | Func::Asinh | Func::Acosh | Func::Atanh,
            _,
        ) => Some(Liate::InverseTrig),
        ExprNode::Func(
            Func::Sin
            | Func::Cos
            | Func::Tan
            | Func::Sec
            | Func::Csc
            | Func::Cot
            | Func::Sinh
            | Func::Cosh
            | Func::Tanh,
            _,
        ) => Some(Liate::Trig),
        ExprNode::Pow(b, x) if !x.contains_sym(var) => match class(b, var) {
            Some(Liate::Log)
                if x.as_number()
                    .is_some_and(|n| n.is_integer() && n.is_positive()) =>
            {
                Some(Liate::Log)
            }
            Some(Liate::Trig) => Some(Liate::Trig),
            _ if is_polynomial_in(f, var) || b.is_sym(var) => Some(Liate::Algebraic),
            _ => None,
        },
        ExprNode::Pow(b, _) if !b.contains_sym(var) => Some(Liate::Exponential),
        ExprNode::Sym(_) | ExprNode::Add(_) if is_polynomial_in(f, var) => Some(Liate::Algebraic),
        _ => None,
    }
}

/// The split `(u, dv)` of `e` by LIATE, if parts makes sense.
fn choose(e: &Expr, var: &str) -> Option<(Expr, Expr, Liate)> {
    let factors: Vec<Expr> = match e.node() {
        ExprNode::Mul(fs) => fs.clone(),
        _ => vec![e.clone()],
    };
    let classes: Vec<Option<Liate>> = factors.iter().map(|f| class(f, var)).collect();
    let best = classes.iter().flatten().max().copied()?;
    let mut u = Vec::new();
    let mut dv = Vec::new();
    for (f, c) in factors.into_iter().zip(classes) {
        if c == Some(best) {
            u.push(f);
        } else {
            dv.push(f);
        }
    }
    if dv.is_empty() && !matches!(best, Liate::Log | Liate::InverseTrig) {
        return None;
    }
    let u = mul(u);
    let dv = mul(dv);
    Some((u, dv, best))
}

/// Constant factor and the rest of a canonical expression.
fn split_const(e: &Expr, var: &str) -> (Expr, Expr) {
    match e.node() {
        ExprNode::Mul(fs) => {
            let (c, r): (Vec<Expr>, Vec<Expr>) =
                fs.iter().cloned().partition(|f| !f.contains_sym(var));
            (mul(c), mul(r))
        }
        _ if !e.contains_sym(var) => (e.clone(), Expr::one()),
        _ => (Expr::one(), e.clone()),
    }
}

/// One application of the parts formula: `dv`, `u·v` and `v·du`.
struct PartsStep {
    dv: Expr,
    uv: Expr,
    vdu: Expr,
}

fn one_step(
    eng: &mut Integrator,
    e: &Expr,
    u: Expr,
    dv: Expr,
    steps: &mut Steps,
) -> Option<PartsStep> {
    let var = eng.var.clone();
    let mut v_steps = Steps::new();
    let v = simplify(&eng.int(&dv, &mut v_steps)?);
    let du = diff(&u, &var, 1, None).ok()?;
    let uv = simplify(&mul(vec![u.clone(), v.clone()]));
    let vdu = simplify(&mul(vec![v.clone(), du.clone()]));
    steps.push(
        "integration by parts",
        holder(e, &var),
        Expr::new(ExprNode::Add(vec![
            uv.clone(),
            Expr::new(ExprNode::Mul(vec![Expr::int(-1), holder(&vdu, &var)])),
        ])),
        Some(format!(
            "u = {u}, dv = {dv} d{var}, du = {du} d{var}, v = {v}"
        )),
    );
    Some(PartsStep { dv, uv, vdu })
}

/// Integrates `e` by parts.
pub(crate) fn by_parts(eng: &mut Integrator, e: &Expr, steps: &mut Steps) -> Option<Expr> {
    let var = eng.var.clone();
    let (u, dv, best) = choose(e, &var)?;
    let first = one_step(eng, e, u, dv, steps)?;
    if best == Liate::Trig && has_exponential(&first.dv) {
        if let Some(r) = cyclic(eng, e, &first, steps) {
            return Some(r);
        }
    }
    let rest = eng.int(&first.vdu, steps)?;
    Some(add(vec![first.uv, mul(vec![Expr::int(-1), rest])]))
}

fn has_exponential(e: &Expr) -> bool {
    match e.node() {
        ExprNode::Pow(b, x) => b.is_constant() && !x.is_constant(),
        ExprNode::Mul(fs) => fs.iter().any(has_exponential),
        _ => false,
    }
}

/// `I = uv − ∫ v du` where a second integration by parts brings back `k·I`:
/// `I = uv − c(u₂v₂ − k I)`, so `I = (uv − c·u₂v₂)/(1 − c·k)`.
fn cyclic(eng: &mut Integrator, e: &Expr, first: &PartsStep, steps: &mut Steps) -> Option<Expr> {
    let var = eng.var.clone();
    let (c, j) = split_const(&first.vdu, &var);
    let (u2, dv2, best) = choose(&j, &var)?;
    if best != Liate::Trig {
        return None;
    }
    let mut local = Steps::new();
    let second = one_step(eng, &j, u2, dv2, &mut local)?;
    let k = simplify(&div(second.vdu.clone(), e.clone()));
    if k.contains_sym(&var) {
        return None;
    }
    let ck = simplify(&mul(vec![c.clone(), k.clone()]));
    let denom = simplify(&add(vec![
        Expr::one(),
        mul(vec![Expr::int(-1), ck.clone()]),
    ]));
    if denom.is_zero() {
        return None;
    }
    let top = add(vec![
        first.uv.clone(),
        mul(vec![Expr::int(-1), c, second.uv.clone()]),
    ]);
    let result = simplify(&div(top.clone(), denom.clone()));
    let top_shown = simplify(&top);
    steps.0.extend(local.0);
    let i = holder(e, &var);
    steps.push(
        "solve for the integral",
        Expr::equation(
            i.clone(),
            add(vec![top, mul(vec![ck.clone(), i.clone()])]),
        ),
        Expr::equation(i, result.clone()),
        Some(format!(
            "the original integral I reappears after integrating by parts twice, so I = {top_shown} + ({ck})·I and ({denom})·I = {top_shown}"
        )),
    );
    Some(result)
}
