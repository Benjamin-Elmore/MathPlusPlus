//! Ordinary differential equations: recognise the type, solve exactly and name the
//! method; fix the constants from initial or boundary conditions; and fall back to a
//! numerical solution (adaptive RK45, or a Rosenbrock method for stiff equations) when no
//! closed form is found.
//!
//! Exact methods: separable, linear (integrating factor), Bernoulli, homogeneous, exact
//! (with integrating factors), Riccati, Clairaut; constant coefficients of any order,
//! undetermined coefficients, variation of parameters, Cauchy–Euler, reduction of order,
//! reducing an equation without `y`; the Laplace transform (step and delta functions);
//! power series and Frobenius series; linear systems `x′ = A x + f` by the matrix
//! exponential. Every exact solution is substituted back into the equation before it is
//! returned; one that fails the check is discarded.

pub mod first_order;
pub mod ivp;
pub mod laplace;
pub mod linear;
pub mod linsys;
pub mod numeric;
pub mod parse;
pub mod pde;
pub mod series;
pub mod systems;
pub mod verify;

use std::fmt;

use crate::calc::diff::diff;
use crate::calc::integrate::integrate;
use crate::domains::solve::{solve, Solutions};
use crate::domains::Ctx;
use crate::error::{MathError, MathResult};
use crate::expr::Expr;
use crate::simplify::{add, mul, simplify};
use crate::steps::Steps;

pub use numeric::{solve_numeric, NumericSolution};
pub use parse::{is_differential_equation, parse, Ode};
pub use verify::Verified;

/// A condition `y⁽ᵏ⁾(at) = value`.
#[derive(Clone, Debug, PartialEq)]
pub struct Condition {
    /// The derivative order (0 for `y` itself).
    pub order: u32,
    /// The point.
    pub at: Expr,
    /// The value.
    pub value: Expr,
}

impl Condition {
    /// `y(at) = value`.
    pub fn value(at: Expr, value: Expr) -> Condition {
        Condition {
            order: 0,
            at,
            value,
        }
    }

    /// `y⁽ᵏ⁾(at) = value`.
    pub fn derivative(order: u32, at: Expr, value: Expr) -> Condition {
        Condition { order, at, value }
    }
}

impl fmt::Display for Condition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let primes = "′".repeat(self.order as usize);
        write!(f, "y{primes}({}) = {}", self.at, self.value)
    }
}

/// The shape of a solution.
#[derive(Clone, Debug, PartialEq)]
pub enum Form {
    /// `y = expr`.
    Explicit(Expr),
    /// `g(x, y) = 0`.
    Implicit(Expr),
    /// A truncated series `y = terms + O((x − a)^order)`.
    Series {
        terms: Expr,
        centre: Expr,
        order: u32,
    },
}

impl Form {
    /// The solution as a relation: `y = …`, `g(x, y) = 0`.
    pub fn relation(&self, y: &str) -> Expr {
        match self {
            Form::Explicit(e) | Form::Series { terms: e, .. } => {
                Expr::equation(Expr::sym(y), e.clone())
            }
            Form::Implicit(g) => Expr::equation(g.clone(), Expr::zero()),
        }
    }
}

/// A solution found by one method, not yet checked.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub form: Form,
    pub method: &'static str,
    pub notes: Vec<String>,
    /// The working of the method that produced it.
    pub steps: Steps,
}

impl Candidate {
    /// `y = e`.
    pub fn explicit(e: Expr, method: &'static str) -> Candidate {
        Candidate {
            form: Form::Explicit(e),
            method,
            notes: Vec::new(),
            steps: Steps::new(),
        }
    }

    /// `g = 0`.
    pub fn implicit(g: Expr, method: &'static str) -> Candidate {
        Candidate {
            form: Form::Implicit(g),
            method,
            notes: Vec::new(),
            steps: Steps::new(),
        }
    }
}

/// A checked solution.
#[derive(Clone, Debug, PartialEq)]
pub struct OdeSolution {
    /// The unknown function.
    pub y: String,
    /// The independent variable.
    pub x: String,
    /// The solution.
    pub form: Form,
    /// The method used (shown in the steps and beside the answer).
    pub method: String,
    /// The arbitrary constants left in the solution.
    pub constants: Vec<String>,
    /// How the solution was confirmed.
    pub verified: Verified,
    /// Remarks: lost constant solutions, singular solutions, assumptions.
    pub notes: Vec<String>,
}

impl OdeSolution {
    /// `y(x) = …` (or the implicit relation) as an expression.
    pub fn to_expr(&self) -> Expr {
        let yx = Expr::apply(&self.y, vec![Expr::sym(&self.x)]);
        match &self.form {
            Form::Explicit(e) => Expr::equation(yx, e.clone()),
            Form::Series { terms, .. } => Expr::equation(yx, terms.clone()),
            Form::Implicit(g) => Expr::equation(g.clone(), Expr::zero()),
        }
    }
}

/// Options for [`dsolve`].
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// The unknown function (found from the derivatives when absent).
    pub y: Option<String>,
    /// The independent variable (found from the derivatives when absent).
    pub x: Option<String>,
    /// A known solution: the particular solution of a Riccati equation, or a solution of
    /// the homogeneous equation for reduction of order.
    pub known: Option<Expr>,
    /// Ask for one method by name (`laplace`, `series`, …).
    pub method: Option<String>,
}

/// Names for the arbitrary constants that are not already used in `e`.
fn constant_names(e: &Expr, n: u32) -> Vec<String> {
    let used = |c: &str| e.contains_sym(c);
    if n == 1 && !used("C") {
        return vec!["C".into()];
    }
    let mut out = Vec::new();
    let mut k = 1;
    while out.len() < n as usize {
        let name = format!("C_{k}");
        if !used(&name) {
            out.push(name);
        }
        k += 1;
    }
    out
}

/// Solves `eq` exactly, applying `conditions`, and checks the answer.
pub fn dsolve(
    eq: &Expr,
    conditions: &[Condition],
    options: &Options,
    steps: Option<&mut Steps>,
) -> MathResult<OdeSolution> {
    let ode = parse(eq, options.y.as_deref(), options.x.as_deref())?;
    let mut local = Steps::new();
    let result = solve_parsed(&ode, conditions, options, &mut local);
    if let Some(log) = steps {
        log.0.extend(local.0);
    }
    result
}

/// Solves an already parsed equation.
pub fn solve_parsed(
    ode: &Ode,
    conditions: &[Condition],
    options: &Options,
    steps: &mut Steps,
) -> MathResult<OdeSolution> {
    let consts = constant_names(&ode.residual, ode.order);
    let wants = options.method.as_deref().map(str::to_lowercase);
    let mut tried: Vec<String> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    let mut candidates: Vec<MathResult<Candidate>> = Vec::new();
    // The Laplace transform first when asked for, or when the forcing has steps or deltas.
    let has_steps = laplace::has_step_or_delta(&ode.residual);
    let with_steps = |r: MathResult<Candidate>, local: Steps| {
        r.map(|mut cand| {
            let mut all = local;
            all.0.extend(cand.steps.0);
            cand.steps = all;
            cand
        })
    };
    if wants.as_deref() == Some("laplace") || has_steps {
        let mut local = Steps::new();
        let r = laplace::solve_ivp(ode, conditions, &mut local);
        candidates.push(with_steps(r, local));
    }
    if wants.as_deref() == Some("series") || wants.as_deref() == Some("frobenius") {
        let mut local = Steps::new();
        let r = series::solve(ode, conditions, &consts, &mut local);
        candidates.push(with_steps(r, local));
    }
    if ode.order == 1 {
        candidates.extend(first_order::candidates(
            ode,
            &consts[0],
            options.known.as_ref(),
        ));
    }
    if ode.order >= 2 || ode.is_linear() {
        let mut local = Steps::new();
        let found = linear::candidates(ode, &consts, options.known.as_ref(), &mut local);
        // At most one candidate comes from the linear methods; it gets their working.
        for r in found {
            candidates.push(with_steps(r, std::mem::take(&mut local)));
        }
    }
    if ode.order >= 2 {
        let mut local = Steps::new();
        if let Some(c) = reduce_missing_y(ode, &consts, options, &mut local) {
            candidates.push(with_steps(c, local));
        }
    }
    // An explicit solution y = … is tried before an implicit one g(x, y) = 0: a linear
    // equation has one from the integrating factor (RC V′ + V = V₀ is solved for V even
    // though it is also separable), and the logistic y′ = y(1 − y) one from Bernoulli.
    if ode.order == 1 {
        candidates
            .sort_by_key(|c| u8::from(matches!(c, Ok(c) if matches!(c.form, Form::Implicit(_)))));
    }
    let mut first_error = None;
    for cand in candidates {
        let cand = match cand {
            Ok(c) => c,
            Err(e) => {
                first_error.get_or_insert(e);
                continue;
            }
        };
        if let Some(w) = &wants {
            if !cand.method.to_lowercase().contains(w.as_str()) {
                tried.push(cand.method.to_string());
                continue;
            }
        }
        let mut local = Steps::new();
        match accept(ode, &cand, conditions, &consts, &mut local) {
            Ok(sol) => {
                steps.0.extend(local.0);
                return Ok(sol);
            }
            Err(e) => failures.push(format!("{} ({e})", cand.method)),
        }
    }
    // Series solutions as a last exact-ish resort for linear equations.
    if wants.is_none() && ode.is_linear() && ode.order >= 2 {
        let mut local = Steps::new();
        let found = series::solve(ode, conditions, &consts, &mut local);
        if let Ok(cand) = with_steps(found, local) {
            // Only a complete solution (every constant present) is offered unasked.
            let complete = (0..ode.order)
                .filter(|k| !conditions.iter().any(|c| c.order == *k))
                .all(|k| form_contains(&cand.form, &consts[k as usize]));
            let mut log = Steps::new();
            if complete {
                if let Ok(sol) = accept(ode, &cand, conditions, &consts, &mut log) {
                    steps.0.extend(log.0);
                    return Ok(sol);
                }
            }
        }
    }
    if !failures.is_empty() {
        return Err(MathError::NoClosedForm(format!(
            "{} (every exact method that applied failed its check: {})",
            ode.original,
            failures.join("; ")
        )));
    }
    if let Some(e) = first_error.filter(|_| has_steps) {
        return Err(e);
    }
    let _ = tried;
    Err(MathError::NoClosedForm(format!(
        "the differential equation {} (no exact method applies; give initial conditions for a numerical solution)",
        ode.original
    )))
}

/// Checks a candidate, fixes its constants from the conditions and checks again.
fn accept(
    ode: &Ode,
    cand: &Candidate,
    conditions: &[Condition],
    consts: &[String],
    steps: &mut Steps,
) -> MathResult<OdeSolution> {
    let verified = verify_form(ode, &cand.form)?;
    steps.0.extend(cand.steps.0.iter().cloned());
    let mut form = cand.form.clone();
    let mut remaining: Vec<String> = consts
        .iter()
        .filter(|c| form_contains(&form, c))
        .cloned()
        .collect();
    let mut verified = verified;
    if !conditions.is_empty() && !matches!(form, Form::Series { .. }) {
        if !remaining.is_empty() {
            let fixed = apply_conditions(ode, &form, conditions, &remaining)?;
            let shown: Vec<String> = conditions.iter().map(|c| c.to_string()).collect();
            steps.push(
                "apply the conditions",
                form.relation(&ode.y),
                fixed.0.relation(&ode.y),
                Some(format!("{} give {}", shown.join(", "), fixed.1)),
            );
            form = fixed.0;
        }
        verified = match &form {
            Form::Explicit(e) => {
                let v = verify_form(ode, &form)?;
                let first = first_piece(ode, e);
                for c in conditions {
                    if !verify::condition(ode, &first, c.order, &c.at, &c.value) {
                        return Err(MathError::Inconclusive(format!(
                            "the solution {e} does not meet {c}"
                        )));
                    }
                }
                v
            }
            Form::Implicit(g) => verify::implicit(ode, g)?,
            Form::Series { .. } => verified,
        };
        remaining.retain(|c| form_contains(&form, c));
    }
    steps.push(
        "check the solution",
        form.relation(&ode.y),
        form.relation(&ode.y),
        Some(format!("{} ({})", verified.describe(), cand.method)),
    );
    Ok(OdeSolution {
        y: ode.y.clone(),
        x: ode.x.clone(),
        form,
        method: cand.method.to_string(),
        constants: remaining,
        verified,
        notes: cand.notes.clone(),
    })
}

/// Substitutes the solution back, piece by piece when it has steps or deltas.
fn verify_form(ode: &Ode, form: &Form) -> MathResult<Verified> {
    match form {
        Form::Explicit(e)
            if laplace::has_step_or_delta(e) || laplace::has_step_or_delta(&ode.residual) =>
        {
            let mut worst = Verified::Symbolic;
            for (lo, hi, piece, residual) in laplace::pieces(e, &ode.residual, &ode.x) {
                let part = Ode {
                    residual,
                    ..ode.clone()
                };
                if verify::explicit_on(&part, &piece, lo, hi)? == Verified::Numeric {
                    worst = Verified::Numeric;
                }
            }
            Ok(worst)
        }
        Form::Explicit(e) => verify::explicit(ode, e),
        Form::Implicit(g) => verify::implicit(ode, g),
        Form::Series { terms, centre, .. } => verify::series(ode, terms, centre),
    }
}

/// The solution on its first interval (where conditions at the start are checked).
fn first_piece(ode: &Ode, e: &Expr) -> Expr {
    if laplace::has_step_or_delta(e) {
        if let Some((_, _, p, _)) = laplace::pieces(e, &ode.residual, &ode.x).into_iter().next() {
            return p;
        }
    }
    e.clone()
}

fn form_contains(f: &Form, c: &str) -> bool {
    match f {
        Form::Explicit(e) | Form::Implicit(e) | Form::Series { terms: e, .. } => e.contains_sym(c),
    }
}

/// Fixes the constants: a linear system when they enter linearly, otherwise one equation
/// for one constant.
fn apply_conditions(
    ode: &Ode,
    form: &Form,
    conditions: &[Condition],
    consts: &[String],
) -> MathResult<(Form, String)> {
    let not_enough = || MathError::Domain {
        function: "dsolve".into(),
        detail: format!(
            "the conditions do not fix the constants {} (the problem may have no solution or many)",
            consts.join(", ")
        ),
    };
    match form {
        Form::Explicit(sol) => {
            let mut equations = Vec::new();
            for c in conditions {
                let d = diff(sol, &ode.x, c.order, None)?;
                let at = simplify(&d.subs(&ode.x, &c.at));
                equations.push(simplify(&add(vec![
                    at,
                    mul(vec![Expr::int(-1), c.value.clone()]),
                ])));
            }
            let values = solve_constants(&equations, consts).ok_or_else(not_enough)?;
            let pairs: Vec<(&str, Expr)> = consts
                .iter()
                .map(String::as_str)
                .zip(values.iter().cloned())
                .collect();
            let fixed = tidy(&sol.subs_all(&pairs));
            let shown: Vec<String> = pairs.iter().map(|(c, v)| format!("{c} = {v}")).collect();
            Ok((Form::Explicit(fixed), shown.join(", ")))
        }
        Form::Implicit(g) => {
            let [c] = consts else {
                return Err(not_enough());
            };
            let [cond] = conditions else {
                return Err(not_enough());
            };
            if cond.order != 0 {
                return Err(not_enough());
            }
            let at = simplify(&g.subs_all(&[
                (ode.x.as_str(), cond.at.clone()),
                (ode.y.as_str(), cond.value.clone()),
            ]));
            let mut ctx = Ctx::new();
            let v = match solve(&at, c, None, &mut ctx)? {
                Solutions::Finite { values, .. } if values.len() == 1 => values[0].clone(),
                _ => return Err(not_enough()),
            };
            let fixed = simplify(&g.subs(c, &v));
            // Solve for y when that is now possible with one branch through the point.
            Ok((
                explicit_through(ode, &fixed, cond).unwrap_or(Form::Implicit(fixed)),
                format!("{c} = {v}"),
            ))
        }
        Form::Series { .. } => Ok((form.clone(), String::new())),
    }
}

/// For an implicit relation fixed by `y(x₀) = y₀`, the explicit branch through the point.
fn explicit_through(ode: &Ode, g: &Expr, cond: &Condition) -> Option<Form> {
    let mut ctx = Ctx::new();
    let Ok(Solutions::Finite {
        values,
        exact: true,
    }) = solve(g, &ode.y, None, &mut ctx)
    else {
        return None;
    };
    for v in values {
        if verify::condition(ode, &v, 0, &cond.at, &cond.value) && verify::explicit(ode, &v).is_ok()
        {
            return Some(Form::Explicit(v));
        }
    }
    None
}

/// Solves the condition equations for the constants.
fn solve_constants(equations: &[Expr], consts: &[String]) -> Option<Vec<Expr>> {
    if consts.is_empty() {
        return equations
            .iter()
            .all(|e| simplify(e).is_zero())
            .then(Vec::new);
    }
    let mut rows = Vec::new();
    let mut rhs = Vec::new();
    let mut linear = true;
    for e in equations {
        match linsys::linear_form(e, consts) {
            Some((row, b)) => {
                rows.push(row);
                rhs.push(b);
            }
            None => {
                linear = false;
                break;
            }
        }
    }
    if linear {
        return linsys::solve(rows, rhs);
    }
    if let ([c], [e]) = (consts, equations) {
        let mut ctx = Ctx::new();
        if let Ok(Solutions::Finite { values, .. }) = solve(e, c, None, &mut ctx) {
            if values.len() == 1 {
                return Some(values);
            }
        }
    }
    None
}

/// Simplifies, and expands when that is shorter.
pub(crate) fn tidy(e: &Expr) -> Expr {
    let s = crate::tools_b::util::pythagorean_tidy(&simplify(e));
    match crate::expand::expand(&s) {
        Ok(x) => {
            let x = simplify(&x);
            if x.size() < s.size() {
                x
            } else {
                s
            }
        }
        Err(_) => s,
    }
}

/// An equation of order ≥ 2 with no `y` (only its derivatives from `y⁽ᵏ⁾` up): solve for
/// `v = y⁽ᵏ⁾`, then integrate `k` times.
fn reduce_missing_y(
    ode: &Ode,
    consts: &[String],
    options: &Options,
    steps: &mut Steps,
) -> Option<MathResult<Candidate>> {
    let k = (0..ode.order).find(|j| ode.residual.contains_sym(&ode.d_name(*j)))?;
    if k == 0 {
        return None;
    }
    let v = format!("{}ᵥ", ode.y);
    let pairs: Vec<(String, Expr)> = (k..=ode.order)
        .map(|j| (ode.d_name(j), Expr::sym(&parse::placeholder(&v, j - k))))
        .collect();
    let refs: Vec<(&str, Expr)> = pairs.iter().map(|(a, b)| (a.as_str(), b.clone())).collect();
    let reduced = Ode::from_residual(ode.residual.subs_all(&refs), &ode.x, &v);
    let inner_consts: Vec<String> = consts[..(ode.order - k) as usize].to_vec();
    let mut sub_steps = Steps::new();
    let mut inner_options = options.clone();
    inner_options.y = Some(v.clone());
    let sol = solve_parsed(&reduced, &[], &inner_options, &mut sub_steps).ok()?;
    let Form::Explicit(mut cur) = sol.form else {
        return None;
    };
    // Rename the inner constants to ours.
    for (i, c) in sol.constants.iter().enumerate() {
        if let Some(target) = inner_consts.get(i) {
            cur = cur.subs(c, &Expr::sym(target));
        }
    }
    for j in 0..k {
        let next = integrate(&cur, &ode.x, None).ok()?;
        let c = &consts[(ode.order - k + j) as usize];
        cur = simplify(&add(vec![next, Expr::sym(c)]));
    }
    steps.0.extend(sub_steps.0);
    steps.push(
        "reduce the order",
        ode.original.clone(),
        Expr::equation(Expr::sym(&ode.y), cur.clone()),
        Some(format!(
            "{} does not appear, so v = {} satisfies an equation of order {}; solve it ({}), then integrate {k} time(s)",
            ode.y,
            ode.d_name(k),
            ode.order - k,
            sol.method
        )),
    );
    let mut cand = Candidate::explicit(cur, "reduction (missing y)");
    cand.notes.extend(sol.notes);
    Some(Ok(cand))
}

#[cfg(test)]
mod tests;
