//! Trigonometric forms: `sinᵐ cosⁿ` (odd powers by substitution, even powers by the
//! power-reducing identities), `tanⁿ`, `cotⁿ`, `secⁿ` and `cscⁿ` reduction formulas,
//! `secᵐ tanⁿ` by `u = tan x`, product-to-sum identities, and hyperbolic functions written
//! with exponentials.

use crate::expand::expand;
use crate::expr::{Expr, ExprNode};
use crate::functions::{func, Func};
use crate::simplify::{add, mul, pow, simplify};
use crate::steps::Steps;

use super::engine::{div, fresh_name, linear_coeffs, Integrator};
use super::holder;

fn f1(f: Func, u: &Expr) -> Expr {
    func(f, vec![u.clone()])
}

fn neg(e: Expr) -> Expr {
    mul(vec![Expr::int(-1), e])
}

/// A product `c · Π gᵢ(arg)^kᵢ` of the six trig functions of one argument.
struct Monomial {
    coeff: Expr,
    arg: Expr,
    powers: Vec<(Func, i64)>,
}

impl Monomial {
    /// Exponents `(p, q)` of `sinᵖ cosᵠ`.
    fn sin_cos(&self) -> (i64, i64) {
        let (mut p, mut q) = (0, 0);
        for (f, k) in &self.powers {
            match f {
                Func::Sin => p += k,
                Func::Cos => q += k,
                Func::Tan => {
                    p += k;
                    q -= k;
                }
                Func::Cot => {
                    p -= k;
                    q += k;
                }
                Func::Sec => q -= k,
                _ => p -= k,
            }
        }
        (p, q)
    }

    /// `Some((f, n))` when the product is a single power `fⁿ`.
    fn single(&self) -> Option<(Func, i64)> {
        match self.powers.as_slice() {
            [(f, k)] => Some((*f, *k)),
            _ => None,
        }
    }
}

fn monomial(e: &Expr, var: &str) -> Option<Monomial> {
    let factors: Vec<Expr> = match e.node() {
        ExprNode::Mul(fs) => fs.clone(),
        _ => vec![e.clone()],
    };
    let mut coeff = Vec::new();
    let mut arg: Option<Expr> = None;
    let mut powers: Vec<(Func, i64)> = Vec::new();
    for f in factors {
        if !f.contains_sym(var) {
            coeff.push(f);
            continue;
        }
        let (g, k) = match f.node() {
            ExprNode::Func(g, a) => (Some((*g, a.first()?.clone())), 1),
            ExprNode::Pow(b, x) => match b.node() {
                ExprNode::Func(g, a) => (Some((*g, a.first()?.clone())), x.as_number()?.as_i64()?),
                _ => (None, 0),
            },
            _ => (None, 0),
        };
        let (g, a) = g?;
        if !matches!(
            g,
            Func::Sin | Func::Cos | Func::Tan | Func::Sec | Func::Csc | Func::Cot
        ) || k.abs() > 40
        {
            return None;
        }
        match &arg {
            Some(prev) if *prev != a => return None,
            None => arg = Some(a),
            _ => {}
        }
        powers.push((g, k));
    }
    Some(Monomial {
        coeff: mul(coeff),
        arg: arg?,
        powers,
    })
}

/// Integrates trigonometric monomials.
pub(crate) fn trig_forms(eng: &mut Integrator, e: &Expr, steps: &mut Steps) -> Option<Expr> {
    let var = eng.var.clone();
    let m = monomial(e, &var)?;
    if !m.coeff.is_one() {
        return None;
    }
    if !m.arg.is_sym(&var) {
        // A linear inner function: substitute u = a x + b first.
        let (a, _) = linear_coeffs(&m.arg, &var)?;
        let u = fresh_name(&[e], &var);
        let in_u = simplify(&div(e.subs_expr(&m.arg, &Expr::sym(&u)), a.clone()));
        if in_u.contains_sym(&var) {
            return None;
        }
        steps.push(
            "substitution",
            holder(e, &var),
            holder(&in_u, &u),
            Some(format!("{u} = {}, d{u} = {a} d{var}", m.arg)),
        );
        let r = eng.int_in(&in_u, &u, steps)?;
        let back = simplify(&r.subs(&u, &m.arg));
        steps.push(
            "substitute back",
            r,
            back.clone(),
            Some(format!("{u} = {}", m.arg)),
        );
        return Some(back);
    }
    let x = Expr::sym(&var);
    if let Some((f, n)) = m.single() {
        if let Some(r) = reduction(eng, e, f, n, &x, steps) {
            return Some(r);
        }
    }
    let (p, q) = m.sin_cos();
    let u = fresh_name(&[e], &var);
    let us = Expr::sym(&u);
    let one_minus_u2 = add(vec![Expr::one(), neg(pow(us.clone(), Expr::int(2)))]);
    let one_plus_u2 = add(vec![Expr::one(), pow(us.clone(), Expr::int(2))]);
    let sub = |eng: &mut Integrator,
               steps: &mut Steps,
               in_u: Expr,
               g: Expr,
               note: String|
     -> Option<Expr> {
        let in_u = simplify(&in_u);
        steps.push(
            "substitution",
            holder(e, &var),
            holder(&in_u, &u),
            Some(note),
        );
        let r = eng.int_in(&in_u, &u, steps)?;
        let back = simplify(&r.subs(&u, &g));
        steps.push(
            "substitute back",
            r,
            back.clone(),
            Some(format!("{u} = {g}")),
        );
        Some(back)
    };
    let sin = f1(Func::Sin, &x);
    let cos = f1(Func::Cos, &x);
    if p > 0 && p % 2 == 1 {
        let in_u = neg(mul(vec![
            pow(one_minus_u2, Expr::int((p - 1) / 2)),
            pow(us.clone(), Expr::int(q)),
        ]));
        let note = format!(
            "write sin^{}({var}) = (1 − cos²({var}))^{}; {u} = {cos}, d{u} = −{sin} d{var}",
            p - 1,
            (p - 1) / 2
        );
        return sub(eng, steps, in_u, cos, note);
    }
    if q > 0 && q % 2 == 1 {
        let in_u = mul(vec![
            pow(one_minus_u2, Expr::int((q - 1) / 2)),
            pow(us.clone(), Expr::int(p)),
        ]);
        let note = format!(
            "write cos^{}({var}) = (1 − sin²({var}))^{}; {u} = {sin}, d{u} = {cos} d{var}",
            q - 1,
            (q - 1) / 2
        );
        return sub(eng, steps, in_u, sin, note);
    }
    if p >= 0 && q >= 0 && p % 2 == 0 && q % 2 == 0 {
        let two_x = mul(vec![Expr::int(2), x.clone()]);
        let c2 = f1(Func::Cos, &two_x);
        let s2 = mul(vec![
            Expr::rational(1, 2),
            add(vec![Expr::one(), neg(c2.clone())]),
        ]);
        let k2 = mul(vec![Expr::rational(1, 2), add(vec![Expr::one(), c2])]);
        let rewritten = mul(vec![pow(s2, Expr::int(p / 2)), pow(k2, Expr::int(q / 2))]);
        let expanded = expand(&rewritten).ok()?;
        steps.push(
            "power-reducing identities",
            holder(e, &var),
            holder(&expanded, &var),
            Some(format!(
                "sin²({var}) = (1 − cos(2{var}))/2, cos²({var}) = (1 + cos(2{var}))/2"
            )),
        );
        return eng.int(&expanded, steps);
    }
    if (p + q) % 2 == 0 && q < 0 && p >= 0 {
        let j = -(p + q) / 2;
        let tan = f1(Func::Tan, &x);
        let in_u = mul(vec![
            pow(us.clone(), Expr::int(p)),
            pow(one_plus_u2, Expr::int(j - 1)),
        ]);
        let note = format!(
            "write the integrand as tan^{p}({var})·sec^{}({var})·sec²({var}) with sec² = 1 + tan²; {u} = {tan}, d{u} = sec²({var}) d{var}",
            2 * j - 2
        );
        return sub(eng, steps, in_u, tan, note);
    }
    if (p + q) % 2 == 0 && p < 0 && q >= 0 {
        let j = -(p + q) / 2;
        let cot = f1(Func::Cot, &x);
        let in_u = neg(mul(vec![
            pow(us.clone(), Expr::int(q)),
            pow(one_plus_u2, Expr::int(j - 1)),
        ]));
        let note = format!(
            "write the integrand as cot^{q}({var})·csc^{}({var})·csc²({var}) with csc² = 1 + cot²; {u} = {cot}, d{u} = −csc²({var}) d{var}",
            2 * j - 2
        );
        return sub(eng, steps, in_u, cot, note);
    }
    if q % 2 != 0 {
        let in_u = mul(vec![
            pow(us.clone(), Expr::int(p)),
            pow(one_minus_u2, Expr::int((q - 1) / 2)),
        ]);
        let note = format!("{u} = {sin}, d{u} = {cos} d{var}, cos²({var}) = 1 − {u}²");
        return sub(eng, steps, in_u, sin, note);
    }
    if p % 2 != 0 {
        let in_u = neg(mul(vec![
            pow(us.clone(), Expr::int(q)),
            pow(one_minus_u2, Expr::int((p - 1) / 2)),
        ]));
        let note = format!("{u} = {cos}, d{u} = −{sin} d{var}, sin²({var}) = 1 − {u}²");
        return sub(eng, steps, in_u, cos, note);
    }
    None
}

/// Reduction formulas for `tanⁿ`, `cotⁿ`, `secⁿ` and `cscⁿ` (including `cos⁻ⁿ`, `sin⁻ⁿ`).
fn reduction(
    eng: &mut Integrator,
    e: &Expr,
    f: Func,
    n: i64,
    x: &Expr,
    steps: &mut Steps,
) -> Option<Expr> {
    let var = eng.var.clone();
    let (f, n) = match (f, n) {
        (Func::Cos, k) if k < 0 => (Func::Sec, -k),
        (Func::Sin, k) if k < 0 => (Func::Csc, -k),
        other => other,
    };
    let n1 = Expr::int(n - 1);
    let g = |h: Func| f1(h, x);
    let (first, coeff, lower, rule, note) = match f {
        Func::Tan if n >= 2 => (
            div(pow(g(Func::Tan), n1.clone()), n1),
            Expr::int(-1),
            pow(g(Func::Tan), Expr::int(n - 2)),
            "reduction formula for tanⁿ",
            "∫ tanⁿ x dx = tanⁿ⁻¹ x/(n − 1) − ∫ tanⁿ⁻² x dx (from tan² = sec² − 1)",
        ),
        Func::Cot if n >= 2 => (
            neg(div(pow(g(Func::Cot), n1.clone()), n1)),
            Expr::int(-1),
            pow(g(Func::Cot), Expr::int(n - 2)),
            "reduction formula for cotⁿ",
            "∫ cotⁿ x dx = −cotⁿ⁻¹ x/(n − 1) − ∫ cotⁿ⁻² x dx (from cot² = csc² − 1)",
        ),
        Func::Sec if n >= 3 => (
            div(
                mul(vec![pow(g(Func::Sec), Expr::int(n - 2)), g(Func::Tan)]),
                n1.clone(),
            ),
            div(Expr::int(n - 2), n1),
            pow(g(Func::Sec), Expr::int(n - 2)),
            "reduction formula for secⁿ",
            "∫ secⁿ x dx = secⁿ⁻² x·tan x/(n − 1) + (n − 2)/(n − 1)·∫ secⁿ⁻² x dx (by parts with u = secⁿ⁻² x, dv = sec² x dx)",
        ),
        Func::Csc if n >= 3 => (
            neg(div(
                mul(vec![pow(g(Func::Csc), Expr::int(n - 2)), g(Func::Cot)]),
                n1.clone(),
            )),
            div(Expr::int(n - 2), n1),
            pow(g(Func::Csc), Expr::int(n - 2)),
            "reduction formula for cscⁿ",
            "∫ cscⁿ x dx = −cscⁿ⁻² x·cot x/(n − 1) + (n − 2)/(n − 1)·∫ cscⁿ⁻² x dx (by parts with u = cscⁿ⁻² x, dv = csc² x dx)",
        ),
        _ => return None,
    };
    steps.push(
        rule,
        holder(e, &var),
        Expr::new(ExprNode::Add(vec![
            first.clone(),
            Expr::new(ExprNode::Mul(vec![coeff.clone(), holder(&lower, &var)])),
        ])),
        Some(note.into()),
    );
    let rest = eng.int(&lower, steps)?;
    Some(add(vec![first, mul(vec![coeff, rest])]))
}

/// `sin A cos B`, `sin A sin B`, `cos A cos B` with different linear arguments, as sums.
pub(crate) fn product_to_sum(eng: &mut Integrator, e: &Expr, steps: &mut Steps) -> Option<Expr> {
    let var = eng.var.clone();
    let ExprNode::Mul(fs) = e.node() else {
        return None;
    };
    let [p, q] = fs.as_slice() else {
        return None;
    };
    let (ExprNode::Func(f, fa), ExprNode::Func(g, ga)) = (p.node(), q.node()) else {
        return None;
    };
    let (a, b) = (fa.first()?, ga.first()?);
    if a == b || linear_coeffs(a, &var).is_none() || linear_coeffs(b, &var).is_none() {
        return None;
    }
    let sum = add(vec![a.clone(), b.clone()]);
    let diff = add(vec![a.clone(), neg(b.clone())]);
    let half = Expr::rational(1, 2);
    let (rewritten, identity) = match (f, g) {
        (Func::Sin, Func::Cos) => (
            mul(vec![
                half,
                add(vec![f1(Func::Sin, &sum), f1(Func::Sin, &diff)]),
            ]),
            "sin A·cos B = (sin(A + B) + sin(A − B))/2",
        ),
        (Func::Cos, Func::Sin) => (
            mul(vec![
                half,
                add(vec![f1(Func::Sin, &sum), neg(f1(Func::Sin, &diff))]),
            ]),
            "cos A·sin B = (sin(A + B) − sin(A − B))/2",
        ),
        (Func::Sin, Func::Sin) => (
            mul(vec![
                half,
                add(vec![f1(Func::Cos, &diff), neg(f1(Func::Cos, &sum))]),
            ]),
            "sin A·sin B = (cos(A − B) − cos(A + B))/2",
        ),
        (Func::Cos, Func::Cos) => (
            mul(vec![
                half,
                add(vec![f1(Func::Cos, &diff), f1(Func::Cos, &sum)]),
            ]),
            "cos A·cos B = (cos(A − B) + cos(A + B))/2",
        ),
        _ => return None,
    };
    let rewritten = expand(&rewritten).ok()?;
    steps.push(
        "product-to-sum identity",
        holder(e, &var),
        holder(&rewritten, &var),
        Some(identity.into()),
    );
    eng.int(&rewritten, steps)
}

fn has_hyperbolic(e: &Expr) -> bool {
    matches!(e.node(), ExprNode::Func(Func::Sinh | Func::Cosh, _))
        || e.children().iter().any(|c| has_hyperbolic(c))
}

fn to_exponentials(e: &Expr) -> Expr {
    match e.node() {
        ExprNode::Func(f @ (Func::Sinh | Func::Cosh), args) if args.len() == 1 => {
            let u = to_exponentials(&args[0]);
            let ep = pow(Expr::e(), u.clone());
            let em = pow(Expr::e(), neg(u));
            let sign = if *f == Func::Sinh { neg(em) } else { em };
            mul(vec![Expr::rational(1, 2), add(vec![ep, sign])])
        }
        _ => crate::simplify::canonical(e.map_children(&mut to_exponentials)),
    }
}

/// Rewrites `sinh` and `cosh` with exponentials, expands and integrates.
pub(crate) fn hyperbolic_as_exponentials(
    eng: &mut Integrator,
    e: &Expr,
    steps: &mut Steps,
) -> Option<Expr> {
    if !has_hyperbolic(e) {
        return None;
    }
    let var = eng.var.clone();
    let rewritten = expand(&to_exponentials(e)).ok()?;
    if &rewritten == e {
        return None;
    }
    steps.push(
        "rewrite with exponentials",
        holder(e, &var),
        holder(&rewritten, &var),
        Some("sinh u = (eᵘ − e⁻ᵘ)/2, cosh u = (eᵘ + e⁻ᵘ)/2".into()),
    );
    eng.int(&rewritten, steps)
}
