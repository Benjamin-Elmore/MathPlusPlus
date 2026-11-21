//! Differential equations whose coefficients and conditions carry units:
//! `m\ddot x + c\dot x + kx = 0` with `m` in kg, `c` in N·s/m, `k` in N/m and
//! `x(0) = 0.1\,\mathrm m`.
//!
//! The variable and each unknown function are given a unit (the variable `t` usually
//! seconds, the unknown the unit its conditions are written in), chosen so that every
//! term of the equation has one dimension and every condition is consistent. The
//! equation is then written in plain numbers in those units (`x = X·⟨m⟩`,
//! `t = τ·⟨s⟩`, so `ẍ = X″·⟨m/s²⟩`), solved by the ordinary solver, and the solution
//! is given its units back: `x(t) = ⟨m⟩ · X(t/⟨s⟩)`.
//!
//! A choice of units is accepted only when the whole problem checks with it; when no
//! choice does, or two different ones do (the units are not determined), the equation
//! is left to the plain solver, which reports what it cannot do.

use mpp_core::units::{Quantity, Unit};
use mpp_core::{Expr, ExprNode, MathError, MathResult, RelOp};

use crate::holders::{evaluate_holder, Ctx};
use crate::units_wire::{contains_units, factor, fold, unit_of_factor, Folded};

/// Most choices of units tried for one equation.
const MAX_TRIALS: usize = 4096;

/// `dsolve(equation or system, conditions…)` with units somewhere in it.
pub fn evaluate(e: &Expr, ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
    let ExprNode::Apply(name, args) = e.node() else {
        return None;
    };
    if !matches!(&**name, "dsolve" | "odesolve" | "ode") || !args.iter().any(contains_units) {
        return None;
    }
    let (first, rest) = args.split_first()?;
    let equations: Vec<Expr> = match first.node() {
        ExprNode::Tuple(items) => items.clone(),
        _ => vec![first.clone()],
    };
    if !equations
        .iter()
        .all(|eq| matches!(eq.node(), ExprNode::Relation(RelOp::Eq, ..)))
    {
        return None;
    }
    let problem = Problem::read(&equations, rest)?;
    let choice = problem.choose_units()?;
    Some(problem.solve(&choice, matches!(first.node(), ExprNode::Tuple(_)), ctx))
}

/// What the equation is about.
struct Problem {
    /// The unknown functions, in the order met.
    unknowns: Vec<String>,
    /// The variable (`t`).
    var: String,
    /// The highest derivative of each unknown.
    orders: Vec<u32>,
    equations: Vec<Expr>,
    conditions: Vec<Cond>,
    /// Every other argument (a method word, an interval), passed on unchanged.
    others: Vec<Expr>,
}

/// `u⁽ᵏ⁾(at) = value`.
struct Cond {
    unknown: usize,
    order: u32,
    at: Expr,
    value: Expr,
}

/// A unit for the variable and one for each unknown.
#[derive(Clone)]
struct Choice {
    var: Unit,
    unknowns: Vec<Unit>,
}

impl Problem {
    fn read(equations: &[Expr], rest: &[Expr]) -> Option<Problem> {
        let mut unknowns: Vec<String> = Vec::new();
        let mut var: Option<String> = None;
        let mut orders: Vec<u32> = Vec::new();
        for eq in equations {
            collect_derivatives(eq, &mut unknowns, &mut orders, &mut var).ok()?;
        }
        let var = var?;
        if unknowns.is_empty() {
            return None;
        }
        let mut conditions = Vec::new();
        let mut others = Vec::new();
        for a in rest {
            match read_condition(a, &unknowns, &var) {
                Some(c) => conditions.push(c),
                None if contains_units(a) => return None,
                None => others.push(a.clone()),
            }
        }
        Some(Problem {
            unknowns,
            var,
            orders,
            equations: equations.to_vec(),
            conditions,
            others,
        })
    }

    /// The one consistent choice of units, preferring seconds for a variable named `t`.
    fn choose_units(&self) -> Option<Choice> {
        let mut found: Vec<Choice> = Vec::new();
        let mut trials = 0;
        for var_unit in self.var_candidates() {
            let lists: Vec<Vec<Unit>> = (0..self.unknowns.len())
                .map(|i| self.unknown_candidates(i, &var_unit))
                .collect();
            for unknowns in product(&lists) {
                trials += 1;
                if trials > MAX_TRIALS {
                    return None;
                }
                let choice = Choice {
                    var: var_unit.clone(),
                    unknowns,
                };
                if self.consistent(&choice) && !found.iter().any(|f| same_dims(f, &choice)) {
                    found.push(choice);
                }
            }
        }
        if found.len() > 1 && is_time_name(&self.var) {
            let second = Unit::named("s")?;
            found.retain(|c| c.var.dimension() == second.dimension());
        }
        match found.as_slice() {
            [one] => Some(one.clone()),
            _ => None,
        }
    }

    /// Seconds, no unit, and the units met in the problem (in SI base units).
    fn var_candidates(&self) -> Vec<Unit> {
        let mut out = Vec::new();
        if let Some(s) = Unit::named("s") {
            out.push(s);
        }
        out.push(Unit::one());
        for u in self.units_met() {
            push_unit(&mut out, u.clone());
            if let Ok(r) = Unit::one().div(&u) {
                push_unit(&mut out, tidy(&r));
            }
        }
        out
    }

    /// No unit, and for each unit met (in the conditions or the equation) that unit
    /// times the variable's unit to each power up to the order.
    fn unknown_candidates(&self, i: usize, var_unit: &Unit) -> Vec<Unit> {
        let mut out = vec![Unit::one()];
        let order = self.orders[i];
        let mut bases: Vec<Unit> = Vec::new();
        for c in self.conditions.iter().filter(|c| c.unknown == i) {
            if let Some(u) = known_unit(&c.value) {
                if let Ok(scaled) = u.mul(&pow(var_unit, c.order)) {
                    push_unit(&mut out, tidy(&scaled));
                }
            }
        }
        bases.extend(self.units_met());
        for u in bases {
            for k in 0..=order {
                if let Ok(scaled) = u.mul(&pow(var_unit, k)) {
                    push_unit(&mut out, tidy(&scaled));
                }
            }
        }
        out
    }

    /// The units of the parts of the equations and conditions, in SI base units.
    fn units_met(&self) -> Vec<Unit> {
        let mut out = Vec::new();
        let mut add = |e: &Expr| units_in(e, &mut out);
        self.equations.iter().for_each(&mut add);
        for c in &self.conditions {
            add(&c.value);
            add(&c.at);
        }
        out
    }

    /// True when every equation has one dimension throughout and every condition fits.
    fn consistent(&self, choice: &Choice) -> bool {
        self.equations
            .iter()
            .all(|eq| self.plain_equation(eq, choice).is_some())
            && self
                .conditions
                .iter()
                .all(|c| self.plain_condition(c, choice).is_some())
    }

    /// The equation in plain numbers: `lhs − rhs` with each unknown and derivative
    /// replaced by a placeholder times its unit, read as one quantity.
    fn plain_equation(&self, eq: &Expr, choice: &Choice) -> Option<Expr> {
        let ExprNode::Relation(RelOp::Eq, l, r) = eq.node() else {
            return None;
        };
        let scaled = self.scale(&(l.clone() - r.clone()), choice);
        // Every term must carry a known quantity once the placeholders are taken as
        // pure numbers, and all terms one dimension.
        let terms = match scaled.expand().ok()?.node() {
            ExprNode::Add(ts) => ts.clone(),
            _ => vec![scaled.clone()],
        };
        let mut dim = None;
        for t in &terms {
            let probe = self.probe(t);
            let Folded::Known { q, certain, .. } = fold(&probe).ok()? else {
                return None;
            };
            if !certain || q.unit.offset().is_some() {
                return None;
            }
            if q.value.simplify().is_zero() {
                continue;
            }
            let d = q.unit.dimension();
            match &dim {
                None => dim = Some(d),
                Some(prev) if *prev == d => {}
                Some(_) => return None,
            }
        }
        let Folded::Known { q, .. } = fold(&scaled).ok()? else {
            return None;
        };
        let plain = (q.value * q.unit.scale()).simplify();
        if contains_units(&plain) {
            return None;
        }
        Some(self.restore(&plain))
    }

    /// A condition in plain numbers: `u⁽ᵏ⁾(at/⟨t⟩) = value·⟨t⟩ᵏ/⟨u⟩`.
    fn plain_condition(&self, c: &Cond, choice: &Choice) -> Option<Expr> {
        let at = plain_number(&(c.at.clone() / factor(&choice.var)))?;
        let unit = choice.unknowns[c.unknown]
            .div(&pow(&choice.var, c.order))
            .ok()?;
        let value = plain_number(&(c.value.clone() / factor(&unit)))?;
        let name = format!(
            "{}{}",
            self.unknowns[c.unknown],
            "′".repeat(c.order as usize)
        );
        Some(Expr::equation(Expr::apply(&name, vec![at]), value))
    }

    /// `e` with each unknown `u` (and its derivatives `u⁽ᵏ⁾`) replaced by
    /// `⟨u⟩/⟨t⟩ᵏ · #u_k`, a placeholder for the plain function.
    fn scale(&self, e: &Expr, choice: &Choice) -> Expr {
        if let Some((i, k)) = self.unknown_at(e) {
            let unit = choice.unknowns[i]
                .div(&pow(&choice.var, k))
                .unwrap_or_else(|_| Unit::one());
            return factor(&unit) * Expr::sym(&placeholder(i, k));
        }
        Expr::new(e.map_children(&mut |c| self.scale(c, choice)))
    }

    /// `e` with the placeholders as 1 (to read each term's unit).
    fn probe(&self, e: &Expr) -> Expr {
        let mut out = e.clone();
        for (i, order) in self.orders.iter().enumerate() {
            for k in 0..=*order {
                out = out.subs(&placeholder(i, k), &Expr::one());
            }
        }
        out
    }

    /// The placeholders back as the plain unknown and its derivatives.
    fn restore(&self, e: &Expr) -> Expr {
        let mut out = e.clone();
        for (i, order) in self.orders.iter().enumerate() {
            let u = &self.unknowns[i];
            for k in 0..=*order {
                let back = if k == 0 {
                    Expr::sym(u)
                } else {
                    Expr::new(ExprNode::Derivative {
                        expr: Expr::sym(u),
                        vars: vec![(self.var.as_str().into(), k)],
                    })
                };
                out = out.subs(&placeholder(i, k), &back);
            }
        }
        out
    }

    /// Which unknown `e` is, and how many times it is differentiated.
    fn unknown_at(&self, e: &Expr) -> Option<(usize, u32)> {
        let index = |n: &str| self.unknowns.iter().position(|u| u == n);
        match e.node() {
            ExprNode::Sym(s) => index(s).map(|i| (i, 0)),
            ExprNode::Apply(f, args) if args.len() == 1 && args[0].is_sym(&self.var) => {
                index(f).map(|i| (i, 0))
            }
            ExprNode::Derivative { expr, vars } => {
                let name = match expr.node() {
                    ExprNode::Sym(s) => s.to_string(),
                    ExprNode::Apply(f, args) if args.len() == 1 && args[0].is_sym(&self.var) => {
                        f.to_string()
                    }
                    _ => return None,
                };
                let k: u32 = vars.iter().map(|(_, k)| *k).sum();
                index(&name).map(|i| (i, k))
            }
            _ => None,
        }
    }

    /// Solves the plain problem and puts the units back.
    fn solve(&self, choice: &Choice, system: bool, ctx: &Ctx<'_>) -> MathResult<Expr> {
        let undetermined =
            || MathError::Unsupported("these units in a differential equation".into());
        let eqs: Vec<Expr> = self
            .equations
            .iter()
            .map(|eq| {
                self.plain_equation(eq, choice)
                    .map(|p| Expr::equation(p, Expr::zero()))
            })
            .collect::<Option<_>>()
            .ok_or_else(undetermined)?;
        let mut args = vec![if system {
            Expr::new(ExprNode::Tuple(eqs))
        } else {
            eqs.into_iter().next().ok_or_else(undetermined)?
        }];
        for c in &self.conditions {
            args.push(self.plain_condition(c, choice).ok_or_else(undetermined)?);
        }
        args.extend(self.others.iter().cloned());
        let names: Vec<String> = choice
            .unknowns
            .iter()
            .zip(&self.unknowns)
            .map(|(u, n)| format!("{n} in {}", unit_name(u)))
            .collect();
        ctx.note(format!(
            "solved in units: {} in {}, {}",
            self.var,
            unit_name(&choice.var),
            names.join(", ")
        ));
        let plain = evaluate_holder(&Expr::apply("dsolve", args), ctx)?;
        Ok(self.with_units(&plain, choice))
    }

    /// `u(t) = X(t)` becomes `u(t) = ⟨u⟩ · X(t/⟨t⟩)`.
    fn with_units(&self, e: &Expr, choice: &Choice) -> Expr {
        match e.node() {
            ExprNode::Tuple(items) => Expr::new(ExprNode::Tuple(
                items.iter().map(|i| self.with_units(i, choice)).collect(),
            )),
            ExprNode::Relation(op, l, r) => {
                let ExprNode::Apply(f, args) = l.node() else {
                    return e.clone();
                };
                let Some(i) = self.unknowns.iter().position(|u| **u == **f) else {
                    return e.clone();
                };
                let [at] = args.as_slice() else {
                    return e.clone();
                };
                let unit = factor(&choice.unknowns[i]);
                let var_unit = factor(&choice.var);
                if at.is_sym(&self.var) {
                    let t = Expr::sym(&self.var) / var_unit;
                    let body = (unit * r.subs(&self.var, &t)).simplify();
                    Expr::relation(*op, l.clone(), body)
                } else {
                    // A numerical value at one point: `u(t₁) ≈ v`.
                    let at = (at.clone() * var_unit).simplify();
                    Expr::relation(*op, Expr::apply(f, vec![at]), (unit * r.clone()).simplify())
                }
            }
            _ => e.clone(),
        }
    }
}

fn placeholder(i: usize, k: u32) -> String {
    format!("#u{i}_{k}")
}

fn is_time_name(v: &str) -> bool {
    matches!(v, "t" | "τ")
}

fn unit_name(u: &Unit) -> String {
    if u.is_one() {
        "plain numbers".into()
    } else {
        u.text()
    }
}

fn pow(u: &Unit, k: u32) -> Unit {
    let mut out = Unit::one();
    for _ in 0..k {
        out = out.mul(u).unwrap_or_else(|_| Unit::one());
    }
    out
}

fn push_unit(out: &mut Vec<Unit>, u: Unit) {
    if !out.iter().any(|v| v.dimension() == u.dimension()) {
        out.push(u);
    }
}

fn same_dims(a: &Choice, b: &Choice) -> bool {
    a.var.dimension() == b.var.dimension()
        && a.unknowns.len() == b.unknowns.len()
        && a.unknowns
            .iter()
            .zip(&b.unknowns)
            .all(|(x, y)| x.dimension() == y.dimension())
}

/// Every combination of one entry from each list.
fn product(lists: &[Vec<Unit>]) -> Vec<Vec<Unit>> {
    let mut out: Vec<Vec<Unit>> = vec![Vec::new()];
    for list in lists {
        let mut next = Vec::with_capacity(out.len() * list.len());
        for prefix in &out {
            for u in list {
                let mut p = prefix.clone();
                p.push(u.clone());
                next.push(p);
            }
        }
        out = next;
        if out.len() > MAX_TRIALS {
            break;
        }
    }
    out
}

/// The unit of a known quantity, tidied (`None` for a plain number).
fn known_unit(e: &Expr) -> Option<Unit> {
    match fold(e).ok()? {
        Folded::Known {
            q, certain: true, ..
        } if !q.unit.is_one() => Some(tidy(&q.unit)),
        _ => None,
    }
}

/// The units of the parts of `e` with a known unit, as written.
fn units_in(e: &Expr, out: &mut Vec<Unit>) {
    if let Some(u) = unit_of_factor(e) {
        if !u.is_dimensionless() {
            push_unit(out, u);
        }
        return;
    }
    if let Some(u) = known_unit(e) {
        if !u.is_dimensionless() {
            push_unit(out, u);
        }
    }
    for c in e.children() {
        units_in(c, out);
    }
}

/// A known dimensionless quantity as a plain number (`0.5 s / ⟨s⟩` → `1/2`).
fn plain_number(e: &Expr) -> Option<Expr> {
    match fold(e).ok()? {
        Folded::Known { q, certain, .. } => {
            if !q.unit.is_dimensionless() || (!certain && !q.value.free_symbols().is_empty()) {
                return None;
            }
            let v = Quantity::new(q.value.clone(), q.unit.clone())
                .require_dimensionless("a condition")
                .ok()?
                .simplify();
            (!contains_units(&v)).then_some(v)
        }
        Folded::Unknown => None,
    }
}

/// Finds the unknowns (names differentiated), their orders and the variable.
fn collect_derivatives(
    e: &Expr,
    unknowns: &mut Vec<String>,
    orders: &mut Vec<u32>,
    var: &mut Option<String>,
) -> Result<(), ()> {
    if let ExprNode::Derivative { expr, vars } = e.node() {
        let name = match expr.node() {
            ExprNode::Sym(s) => Some(s.to_string()),
            ExprNode::Apply(f, args) if args.len() == 1 => Some(f.to_string()),
            _ => None,
        };
        if let (Some(n), [(v, k)]) = (name, vars.as_slice()) {
            if var.as_deref().is_some_and(|w| w != &**v) {
                return Err(());
            }
            *var = Some(v.to_string());
            match unknowns.iter().position(|u| *u == n) {
                Some(i) => orders[i] = orders[i].max(*k),
                None => {
                    unknowns.push(n);
                    orders.push(*k);
                }
            }
        } else {
            return Err(());
        }
        return Ok(());
    }
    for c in e.children() {
        collect_derivatives(c, unknowns, orders, var)?;
    }
    Ok(())
}

/// `u(a) = v`, `u′(a) = v` or `subs(dᵏu/dtᵏ, t = a) = v`.
fn read_condition(e: &Expr, unknowns: &[String], var: &str) -> Option<Cond> {
    let ExprNode::Relation(RelOp::Eq, l, value) = e.node() else {
        return None;
    };
    let ExprNode::Apply(f, args) = l.node() else {
        return None;
    };
    let index = |n: &str| unknowns.iter().position(|u| u == n);
    if &**f == "subs" {
        let [d, at] = args.as_slice() else {
            return None;
        };
        let ExprNode::Derivative { expr, vars } = d.node() else {
            return None;
        };
        let name = match expr.node() {
            ExprNode::Sym(s) => s.to_string(),
            ExprNode::Apply(g, _) => g.to_string(),
            _ => return None,
        };
        let ExprNode::Relation(RelOp::Eq, v, point) = at.node() else {
            return None;
        };
        if !v.is_sym(var) {
            return None;
        }
        let order: u32 = vars.iter().map(|(_, k)| *k).sum();
        return Some(Cond {
            unknown: index(&name)?,
            order,
            at: point.clone(),
            value: value.clone(),
        });
    }
    let [at] = args.as_slice() else {
        return None;
    };
    let base = f.trim_end_matches(['′', '\'']);
    let order = (f.chars().count() - base.chars().count()) as u32;
    Some(Cond {
        unknown: index(base)?,
        order,
        at: at.clone(),
        value: value.clone(),
    })
}

/// A unit in its tidy form (`V·s/s` → `V`, `kg·m/s²` → `N`); only its dimension
/// matters for the solution, and this is the form the answer is shown in.
fn tidy(u: &Unit) -> Unit {
    u.simplified().0
}
