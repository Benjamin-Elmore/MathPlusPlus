//! Equation solving: exact roots of polynomials up to degree four, rational equations,
//! equations reducible by isolating the unknown (with general solutions for periodic
//! functions), substitution (`e^{2x} − 3e^x + 2 = 0`), and numeric root finding for
//! everything else; plus systems (see [`system`]).

pub mod isolate;
pub mod numeric;
pub mod poly;
pub mod system;

use std::collections::BTreeSet;

use crate::domains::util::{
    self, expect_args_between, is_zero, numeric as num_value, sym_arg, to_zero_form,
};
use crate::domains::{Ctx, Domain, Value};
use crate::error::{MathError, MathResult};
use crate::expr::{Constant, Expr, ExprNode};
use crate::simplify::mul;

pub use poly::Root;
pub use system::{solve_system, SystemSolution};

/// Default interval scanned by the numeric fall-back.
pub const DEFAULT_SCAN: (f64, f64) = (-100.0, 100.0);
/// Most solutions listed when a periodic family is restricted to an interval.
const MAX_LISTED: usize = 10_000;

/// The solutions of one equation in one unknown.
#[derive(Clone, Debug, PartialEq)]
pub enum Solutions {
    /// Finitely many solutions; `exact` is false when they were found numerically.
    Finite { values: Vec<Expr>, exact: bool },
    /// Families such as `π/6 + 2πk` where the parameter ranges over the integers.
    Periodic { families: Vec<Expr>, param: String },
    /// Every value solves the equation.
    All,
}

impl Solutions {
    /// No solutions.
    pub fn none() -> Solutions {
        Solutions::Finite {
            values: Vec::new(),
            exact: true,
        }
    }

    /// The solutions as a value: a set (empty when there are none), with periodic
    /// families written in terms of their integer parameter, and ℝ as `(−∞, ∞)`.
    pub fn to_expr(&self) -> Expr {
        match self {
            Solutions::Finite { values, .. } => Expr::new(ExprNode::Set(values.clone())),
            Solutions::Periodic { families, .. } => Expr::new(ExprNode::Set(families.clone())),
            Solutions::All => Expr::new(ExprNode::Interval {
                lo: Expr::neg_infinity(),
                hi: Expr::infinity(),
                lo_closed: false,
                hi_closed: false,
            }),
        }
    }

    /// The finite list of values, if there is one.
    pub fn values(&self) -> Option<&[Expr]> {
        match self {
            Solutions::Finite { values, .. } => Some(values),
            _ => None,
        }
    }
}

/// An interval of the real line the solutions must lie in.
#[derive(Clone, Debug, PartialEq)]
pub struct Range {
    pub lo: Expr,
    pub hi: Expr,
    pub lo_closed: bool,
    pub hi_closed: bool,
}

impl Range {
    /// A closed interval.
    pub fn closed(lo: Expr, hi: Expr) -> Range {
        Range {
            lo,
            hi,
            lo_closed: true,
            hi_closed: true,
        }
    }

    /// Reads an interval value.
    pub fn from_expr(e: &Expr) -> Option<Range> {
        match e.node() {
            ExprNode::Interval {
                lo,
                hi,
                lo_closed,
                hi_closed,
            } => Some(Range {
                lo: lo.clone(),
                hi: hi.clone(),
                lo_closed: *lo_closed,
                hi_closed: *hi_closed,
            }),
            _ => None,
        }
    }

    fn bounds_f64(&self) -> (f64, f64) {
        let f = |e: &Expr, inf: f64| {
            if e.is_infinite() {
                if matches!(e.node(), ExprNode::Const(Constant::Infinity)) {
                    f64::INFINITY
                } else {
                    -f64::INFINITY
                }
            } else {
                num_value(e).unwrap_or(inf)
            }
        };
        (f(&self.lo, f64::NEG_INFINITY), f(&self.hi, f64::INFINITY))
    }

    /// True when the real constant `v` lies in the interval.
    pub fn contains(&self, v: &Expr) -> bool {
        use std::cmp::Ordering::*;
        let lo_ok = match util::compare(v, &self.lo) {
            Some(Greater) => true,
            Some(Equal) => self.lo_closed,
            _ => false,
        };
        let hi_ok = match util::compare(v, &self.hi) {
            Some(Less) => true,
            Some(Equal) => self.hi_closed,
            _ => false,
        };
        lo_ok && hi_ok
    }
}

/// A parameter name not already used in `e`.
fn fresh_param(e: &Expr) -> String {
    let used: BTreeSet<String> = e.free_symbols().iter().map(|s| s.to_string()).collect();
    std::iter::once("k".to_string())
        .chain((1..).map(|i| format!("k_{i}")))
        .find(|n| !used.contains(n))
        .unwrap_or_else(|| "k".into())
}

/// True when a candidate value really solves `f = 0` (drops spurious roots from squaring
/// or from leaving the domain); unknown when the check can't be done numerically.
fn satisfies(f: &Expr, var: &str, value: &Expr) -> bool {
    let at = f.subs(var, value).simplify();
    if is_zero(&at) {
        return true;
    }
    if !at.is_constant() {
        return true; // symbolic: can't refute
    }
    match at.eval_complex(&crate::eval::EmptyEnv) {
        Ok(v) => v.norm() <= 1e-9 * value_scale(value),
        Err(_) => false,
    }
}

fn value_scale(v: &Expr) -> f64 {
    v.eval_complex(&crate::eval::EmptyEnv)
        .map(|z| z.norm().max(1.0))
        .unwrap_or(1.0)
}

/// Solves `eq` (an equation, or an expression meaning `= 0`) for `var`, optionally
/// keeping only real solutions in `range`.
pub fn solve(
    eq: &Expr,
    var: &str,
    range: Option<&Range>,
    ctx: &mut Ctx<'_>,
) -> MathResult<Solutions> {
    let f = to_zero_form(eq);
    if matches!(f.node(), ExprNode::Relation(..)) {
        return Err(MathError::Unsupported(
            "solving inequalities (intervals come with piecewise support)".into(),
        ));
    }
    if !f.contains_sym(var) {
        if is_zero(&f) {
            return Ok(Solutions::All);
        }
        if f.is_constant() {
            return Ok(Solutions::none());
        }
        return Err(MathError::Domain {
            function: "solve".into(),
            detail: format!("the equation does not contain {var}"),
        });
    }
    let found = solve_exact(&f, var, ctx)?;
    let found = match found {
        Some(s) => s,
        None => {
            let (lo, hi) = match range {
                Some(r) => {
                    let (a, b) = r.bounds_f64();
                    (a.max(-1e12), b.min(1e12))
                }
                None => DEFAULT_SCAN,
            };
            let roots = numeric::roots_in(&f, var, lo, hi)?;
            ctx.note(format!(
                "solved numerically; roots searched in [{lo}, {hi}]"
            ));
            ctx.step(
                "numeric root finding",
                &Expr::equation(f.clone(), Expr::zero()),
                &Expr::new(ExprNode::Set(
                    roots.iter().map(|&r| Expr::float(r)).collect(),
                )),
                Some("sign changes bracketed, then bisection and Newton steps".into()),
            );
            Solutions::Finite {
                values: roots.into_iter().map(Expr::float).collect(),
                exact: false,
            }
        }
    };
    let found = match range {
        Some(r) => restrict(found, r, ctx)?,
        None => found,
    };
    if let Solutions::Periodic { param, .. } = &found {
        ctx.note(format!("{param} is any integer"));
    }
    Ok(found)
}

/// Keeps the real solutions inside `r`, listing periodic families there.
fn restrict(s: Solutions, r: &Range, ctx: &mut Ctx<'_>) -> MathResult<Solutions> {
    match s {
        Solutions::All => Ok(Solutions::All),
        Solutions::Finite { values, exact } => Ok(Solutions::Finite {
            values: values.into_iter().filter(|v| r.contains(v)).collect(),
            exact,
        }),
        Solutions::Periodic { families, param } => {
            let (lo, hi) = r.bounds_f64();
            if !lo.is_finite() || !hi.is_finite() {
                return Err(MathError::TooLarge(
                    "the list of solutions (there are infinitely many)".into(),
                ));
            }
            let mut out: Vec<Expr> = Vec::new();
            for fam in &families {
                for k in k_range(fam, &param, lo, hi) {
                    let v = fam.subs(&param, &Expr::int(k)).simplify();
                    if r.contains(&v) && !out.iter().any(|o| util::equal(o, &v)) {
                        out.push(v);
                        if out.len() > MAX_LISTED {
                            return Err(MathError::TooLarge("the list of solutions".into()));
                        }
                    }
                }
            }
            out.sort_by(|a, b| {
                num_value(a)
                    .unwrap_or(f64::NAN)
                    .total_cmp(&num_value(b).unwrap_or(f64::NAN))
            });
            ctx.note("principal values in the given interval");
            Ok(Solutions::Finite {
                values: out,
                exact: true,
            })
        }
    }
}

/// Integers `k` worth trying for a family `f(k)` restricted to `[lo, hi]`.
fn k_range(fam: &Expr, param: &str, lo: f64, hi: f64) -> std::ops::RangeInclusive<i64> {
    if let Some(c) = fam.as_poly(param) {
        if let [c0, c1] = c.as_slice() {
            if let (Some(a), Some(b)) = (num_value(c0), num_value(c1)) {
                if b != 0.0 {
                    let (k1, k2) = ((lo - a) / b, (hi - a) / b);
                    let (k1, k2) = (k1.min(k2), k1.max(k2));
                    let clamp = |v: f64| v.clamp(-1e6, 1e6) as i64;
                    return clamp(k1.floor()) - 1..=clamp(k2.ceil()) + 1;
                }
            }
        }
    }
    -1000..=1000
}

/// Exact methods in turn; `Ok(None)` when none applies.
fn solve_exact(f: &Expr, var: &str, ctx: &mut Ctx<'_>) -> MathResult<Option<Solutions>> {
    // 1. Polynomials.
    if let Some(coeffs) = f.as_poly(var) {
        if coeffs.len() >= 2 {
            match poly::roots(&coeffs) {
                Ok(rs) => {
                    let exact = rs.iter().all(|r| r.exact);
                    let degree = coeffs.len() - 1;
                    let method = match degree {
                        1 => "linear equation",
                        2 => "quadratic formula",
                        3 => "rational roots, then Cardano's formula",
                        4 => "rational roots, then Ferrari's method",
                        _ => "rational roots, then numeric roots",
                    };
                    let values: Vec<Expr> = rs.into_iter().map(|r| r.value).collect();
                    ctx.step(
                        method,
                        &Expr::equation(f.clone(), Expr::zero()),
                        &Expr::new(ExprNode::Set(values.clone())),
                        None,
                    );
                    if !exact {
                        ctx.note("some roots are numeric");
                    }
                    return Ok(Some(Solutions::Finite { values, exact }));
                }
                Err(MathError::Unsupported(_)) => {}
                Err(e) => return Err(e),
            }
        }
    }
    // 2. Rational equations: numerator = 0 where the denominator is not 0.
    let (num, den) = numer_denom(f, var);
    if den.contains_sym(var) {
        if let Some(coeffs) = num.as_poly(var) {
            if coeffs.len() >= 2 {
                if let Ok(rs) = poly::roots(&coeffs) {
                    let exact = rs.iter().all(|r| r.exact);
                    let values: Vec<Expr> = rs
                        .into_iter()
                        .map(|r| r.value)
                        .filter(|v| !is_zero(&den.subs(var, v)))
                        .collect();
                    ctx.step(
                        "clear denominators",
                        &Expr::equation(f.clone(), Expr::zero()),
                        &Expr::equation(num.clone(), Expr::zero()),
                        Some(format!("excluding zeros of {den}")),
                    );
                    return Ok(Some(Solutions::Finite { values, exact }));
                }
            }
        }
    }
    let param = fresh_param(f);
    // 3. Isolation: the unknown occurs once.
    if let Some(iso) = isolate_zero(f, var, &param)? {
        return Ok(Some(finish(
            iso,
            f,
            var,
            &param,
            ctx,
            "isolate the unknown",
        )));
    }
    // 4. Substitution: a polynomial in one kernel such as sin x or e^x.
    if let Some(s) = substitute(f, var, &param, ctx)? {
        return Ok(Some(s));
    }
    Ok(None)
}

fn finish(
    iso: isolate::Isolated,
    f: &Expr,
    var: &str,
    param: &str,
    ctx: &mut Ctx<'_>,
    rule: &str,
) -> Solutions {
    let before = Expr::equation(f.clone(), Expr::zero());
    let values: Vec<Expr> = if iso.periodic {
        iso.values.iter().map(tidy).collect()
    } else {
        iso.values
            .into_iter()
            .filter(|v| satisfies(f, var, v))
            .collect()
    };
    ctx.step(
        rule,
        &before,
        &Expr::new(ExprNode::Set(values.clone())),
        None,
    );
    if iso.periodic {
        Solutions::Periodic {
            families: values,
            param: param.to_string(),
        }
    } else {
        Solutions::Finite {
            values: dedup(values),
            exact: true,
        }
    }
}

/// The expanded form when it is no larger (`(2πk + π/2)/2` → `πk + π/4`).
fn tidy(e: &Expr) -> Expr {
    match e.expand() {
        Ok(x) if x.size() <= e.size() => x,
        _ => e.clone(),
    }
}

fn dedup(values: Vec<Expr>) -> Vec<Expr> {
    let mut out: Vec<Expr> = Vec::new();
    for v in values {
        if !out.iter().any(|o| util::equal(o, &v)) {
            out.push(v);
        }
    }
    out
}

/// Isolation on `f = 0`, moving terms without the unknown to the right.
fn isolate_zero(f: &Expr, var: &str, param: &str) -> MathResult<Option<isolate::Isolated>> {
    if isolate::occurrences(f, var) != 1 {
        return Ok(None);
    }
    isolate::isolate(f, &Expr::zero(), var, param)
}

/// Numerator and denominator of `f` as a fraction (only parts that mention `var` are
/// moved to the denominator).
pub fn numer_denom(f: &Expr, var: &str) -> (Expr, Expr) {
    match f.node() {
        ExprNode::Add(ts) => {
            let mut num = Expr::zero();
            let mut den = Expr::one();
            for t in ts {
                let (n, d) = numer_denom(t, var);
                if d == den {
                    num = &num + &n;
                } else {
                    num = &num * &d + &n * &den;
                    den = &den * &d;
                }
            }
            (num.simplify(), den)
        }
        ExprNode::Mul(fs) => {
            let mut nums = Vec::new();
            let mut dens = Vec::new();
            for g in fs {
                let (n, d) = numer_denom(g, var);
                nums.push(n);
                dens.push(d);
            }
            (mul(nums), mul(dens))
        }
        ExprNode::Pow(b, p) if b.contains_sym(var) => {
            match p.as_number().and_then(crate::number::Number::as_i64) {
                Some(k) if k < 0 => {
                    let (n, d) = numer_denom(b, var);
                    (d.pow(Expr::int(-k)), n.pow(Expr::int(-k)))
                }
                Some(k) => {
                    let (n, d) = numer_denom(b, var);
                    (n.pow(Expr::int(k)), d.pow(Expr::int(k)))
                }
                None => (f.clone(), Expr::one()),
            }
        }
        _ => (f.clone(), Expr::one()),
    }
}

/// Collects the maximal subexpressions holding `var` that are not sums, products or
/// integer powers.
fn kernels(e: &Expr, var: &str, out: &mut Vec<Expr>) {
    if !e.contains_sym(var) {
        return;
    }
    match e.node() {
        ExprNode::Add(v) | ExprNode::Mul(v) => v.iter().for_each(|c| kernels(c, var, out)),
        ExprNode::Pow(b, p)
            if !p.contains_sym(var) && p.as_number().is_some_and(|n| n.is_integer()) =>
        {
            kernels(b, var, out)
        }
        _ => {
            if !out.contains(e) {
                out.push(e.clone());
            }
        }
    }
}

/// Rewrites exponentials `b^(a x + c)` as `b^c · u^(a/g)` with `u = b^(g x)`.
fn exp_kernel(f: &Expr, var: &str, ks: &[Expr]) -> Option<(Expr, Expr)> {
    let mut base: Option<Expr> = None;
    let mut slopes = Vec::new();
    for k in ks {
        let ExprNode::Pow(b, p) = k.node() else {
            return None;
        };
        if b.contains_sym(var) {
            return None;
        }
        if base.as_ref().is_some_and(|x| x != b) {
            return None;
        }
        base = Some(b.clone());
        let c = p.as_poly(var)?;
        let [_, a] = c.as_slice() else { return None };
        slopes.push(a.as_number()?.as_rational()?.clone());
    }
    let base = base?;
    let g = slopes
        .iter()
        .skip(1)
        .fold(slopes.first()?.clone(), |acc, s| {
            crate::number::gcd_rational(&acc, s)
        });
    let u = Expr::sym("\u{1d462}"); // a private placeholder name
    let kernel = base.pow(util::rat_expr(g.clone()) * Expr::sym(var));
    let mut rewritten = f.clone();
    for k in ks {
        let ExprNode::Pow(b, p) = k.node() else {
            return None;
        };
        let c = p.as_poly(var)?;
        let (c0, a) = (
            c.first()?.clone(),
            c.get(1)?.as_number()?.as_rational()?.clone(),
        );
        let repl = b.pow(c0) * u.pow(util::rat_expr(a / &g));
        rewritten = rewritten.subs_expr(k, &repl);
    }
    Some((rewritten.simplify(), kernel))
}

fn substitute(
    f: &Expr,
    var: &str,
    param: &str,
    ctx: &mut Ctx<'_>,
) -> MathResult<Option<Solutions>> {
    let mut ks = Vec::new();
    kernels(f, var, &mut ks);
    let placeholder = "\u{1d462}";
    let (g, kernel) = if ks.len() == 1 {
        let k = ks[0].clone();
        if k.is_sym(var) {
            return Ok(None);
        }
        (f.subs_expr(&k, &Expr::sym(placeholder)).simplify(), k)
    } else {
        match exp_kernel(f, var, &ks) {
            Some(pair) => pair,
            None => return Ok(None),
        }
    };
    if g.contains_sym(var) {
        return Ok(None);
    }
    let Some(coeffs) = g.as_poly(placeholder) else {
        return Ok(None);
    };
    if coeffs.len() < 2 {
        return Ok(None);
    }
    let Ok(us) = poly::roots(&coeffs) else {
        return Ok(None);
    };
    let mut values = Vec::new();
    let mut periodic = false;
    for u in us {
        let Some(iso) = isolate::isolate(&kernel, &u.value, var, param)? else {
            return Ok(None);
        };
        periodic |= iso.periodic;
        values.extend(iso.values);
    }
    let iso = isolate::Isolated { values, periodic };
    let note = format!("substitute u = {kernel}");
    ctx.step(
        "substitution",
        &Expr::equation(f.clone(), Expr::zero()),
        &Expr::equation(g.subs(placeholder, &Expr::sym("u")), Expr::zero()),
        Some(note),
    );
    Ok(Some(finish(
        iso,
        f,
        var,
        param,
        ctx,
        "solve for the kernel",
    )))
}

/// The solving domain: `solve`, `nsolve` and `roots`.
pub struct Solving;

const OPS: &[&str] = &["solve", "nsolve", "roots"];

impl Domain for Solving {
    fn name(&self) -> &'static str {
        "equation solving"
    }

    fn operators(&self) -> &'static [&'static str] {
        OPS
    }

    fn apply(&self, op: &str, args: &[Value], ctx: &mut Ctx<'_>) -> Option<MathResult<Value>> {
        Some(match op {
            "solve" => solve_op(args, ctx),
            "nsolve" => nsolve_op(args, ctx),
            "roots" => roots_op(args, ctx),
            _ => return None,
        })
    }
}

/// Equations and unknowns from a list argument.
fn list(e: &Expr) -> Vec<Expr> {
    match e.node() {
        ExprNode::Tuple(v) | ExprNode::Set(v) => v.clone(),
        ExprNode::Logic(crate::expr::LogicOp::And, v) => v.clone(),
        _ => vec![e.clone()],
    }
}

/// The single unknown of an equation when it has one free symbol.
fn guess_var(eq: &Expr) -> MathResult<String> {
    let syms = eq.free_symbols();
    match syms.len() {
        1 => Ok(syms
            .iter()
            .next()
            .map(|s| s.to_string())
            .unwrap_or_default()),
        0 => Err(MathError::Domain {
            function: "solve".into(),
            detail: "there is no unknown to solve for".into(),
        }),
        _ => Err(MathError::Domain {
            function: "solve".into(),
            detail: "name the unknown to solve for".into(),
        }),
    }
}

fn solve_op(args: &[Value], ctx: &mut Ctx<'_>) -> MathResult<Value> {
    expect_args_between("solve", args, 1, 3)?;
    let eqs = list(&args[0]);
    if eqs.len() > 1 {
        let vars: Vec<String> = match args.get(1) {
            Some(v) => list(v)
                .iter()
                .map(|e| sym_arg("solve", e))
                .collect::<MathResult<_>>()?,
            None => {
                let mut all = BTreeSet::new();
                for e in &eqs {
                    all.extend(e.free_symbols().iter().map(|s| s.to_string()));
                }
                all.into_iter().collect()
            }
        };
        return Ok(solve_system(&eqs, &vars, ctx)?.to_expr());
    }
    let var = match args.get(1) {
        Some(v) => sym_arg("solve", v)?,
        None => guess_var(&args[0])?,
    };
    let range = match args.get(2) {
        Some(r) => Some(Range::from_expr(r).ok_or_else(|| MathError::Domain {
            function: "solve".into(),
            detail: format!("{r} is not an interval"),
        })?),
        None => None,
    };
    Ok(solve(&args[0], &var, range.as_ref(), ctx)?.to_expr())
}

/// `nsolve(eq, x, [a, b])` or `nsolve(eq, x, x0)`; systems take tuples.
fn nsolve_op(args: &[Value], ctx: &mut Ctx<'_>) -> MathResult<Value> {
    expect_args_between("nsolve", args, 2, 3)?;
    let eqs: Vec<Expr> = list(&args[0]).iter().map(to_zero_form).collect();
    let vars: Vec<String> = list(&args[1])
        .iter()
        .map(|e| sym_arg("nsolve", e))
        .collect::<MathResult<_>>()?;
    ctx.note("numeric solution");
    if eqs.len() == 1 && vars.len() == 1 {
        let (lo, hi, start) = match args.get(2) {
            None => (DEFAULT_SCAN.0, DEFAULT_SCAN.1, None),
            Some(a) => match Range::from_expr(a) {
                Some(r) => {
                    let (lo, hi) = r.bounds_f64();
                    (lo, hi, None)
                }
                None => (0.0, 0.0, Some(util::real_arg("nsolve", a)?)),
            },
        };
        let roots = match start {
            Some(x0) => numeric::newton(&eqs[0], &vars[0], x0).into_iter().collect(),
            None => numeric::roots_in(&eqs[0], &vars[0], lo, hi)?,
        };
        return Ok(Expr::new(ExprNode::Set(
            roots.into_iter().map(Expr::float).collect(),
        )));
    }
    let start: Vec<f64> = match args.get(2) {
        Some(s) => list(s)
            .iter()
            .map(|e| util::real_arg("nsolve", e))
            .collect::<MathResult<_>>()?,
        None => vec![1.0; vars.len()],
    };
    let sol = numeric::newton_system(&eqs, &vars, &start).ok_or_else(|| {
        MathError::Undefined("a numeric solution from this starting point".into())
    })?;
    Ok(Expr::new(ExprNode::Tuple(
        vars.iter()
            .zip(sol)
            .map(|(v, x)| Expr::equation(Expr::sym(v), Expr::float(x)))
            .collect(),
    )))
}

/// `roots(p, x)`: each root with its multiplicity, as `(root, multiplicity)` pairs.
fn roots_op(args: &[Value], ctx: &mut Ctx<'_>) -> MathResult<Value> {
    expect_args_between("roots", args, 1, 2)?;
    let p = to_zero_form(&args[0]);
    let var = match args.get(1) {
        Some(v) => sym_arg("roots", v)?,
        None => guess_var(&p)?,
    };
    let coeffs = p
        .as_poly(&var)
        .ok_or_else(|| MathError::NotPolynomial { var: var.clone() })?;
    let rs = poly::roots(&coeffs)?;
    if rs.iter().any(|r| !r.exact) {
        ctx.note("some roots are numeric");
    }
    Ok(Expr::new(ExprNode::Tuple(
        rs.into_iter()
            .map(|r| {
                Expr::new(ExprNode::Tuple(vec![
                    r.value,
                    Expr::int(i64::from(r.multiplicity)),
                ]))
            })
            .collect(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::EmptyEnv;

    fn x() -> Expr {
        Expr::sym("x")
    }

    fn values(s: &Solutions) -> Vec<f64> {
        let mut v: Vec<f64> = s
            .values()
            .unwrap()
            .iter()
            .map(|e| e.eval_f64(&EmptyEnv).unwrap())
            .collect();
        v.sort_by(f64::total_cmp);
        v
    }

    fn close(a: &[f64], b: &[f64]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    #[test]
    fn polynomial_equations() {
        let mut ctx = Ctx::new();
        let eq = Expr::equation(x().pow(2), Expr::int(4));
        let s = solve(&eq, "x", None, &mut ctx).unwrap();
        assert_eq!(s.values().unwrap(), &[Expr::int(-2), Expr::int(2)]);
        let s = solve(
            &(x().pow(3) - Expr::int(6) * x().pow(2) + Expr::int(11) * x() - Expr::int(6)),
            "x",
            None,
            &mut ctx,
        )
        .unwrap();
        assert!(close(&values(&s), &[1.0, 2.0, 3.0]));
    }

    #[test]
    fn rational_equations_skip_poles() {
        let mut ctx = Ctx::new();
        // (x² − 1)/(x − 1) = 0 has only x = −1.
        let f = (x().pow(2) - Expr::one()) / (x() - Expr::one());
        let s = solve(&f, "x", None, &mut ctx).unwrap();
        assert_eq!(s.values().unwrap(), &[Expr::int(-1)]);
        // 1/x + 1/(x+1) = 0 → x = −1/2
        let f = x().recip() + (x() + Expr::one()).recip();
        let s = solve(&f, "x", None, &mut ctx).unwrap();
        assert_eq!(s.values().unwrap(), &[Expr::rational(-1, 2)]);
    }

    #[test]
    fn trig_general_and_principal() {
        let mut ctx = Ctx::new();
        let eq = Expr::equation(Expr::call("sin", vec![x()]).unwrap(), Expr::rational(1, 2));
        let s = solve(&eq, "x", None, &mut ctx).unwrap();
        assert!(matches!(s, Solutions::Periodic { ref families, .. } if families.len() == 2));
        assert!(ctx.notes.iter().any(|n| n.contains("integer")));
        let range = Range::closed(Expr::zero(), Expr::int(2) * Expr::pi());
        let s = solve(&eq, "x", Some(&range), &mut ctx).unwrap();
        let pi = std::f64::consts::PI;
        assert!(close(&values(&s), &[pi / 6.0, 5.0 * pi / 6.0]));
        assert_eq!(s.values().unwrap()[0], Expr::pi() / Expr::int(6));
    }

    #[test]
    fn spurious_roots_are_dropped() {
        let mut ctx = Ctx::new();
        // √x = −1 has no solution.
        let eq = Expr::equation(x().sqrt(), Expr::int(-1));
        let s = solve(&eq, "x", None, &mut ctx).unwrap();
        assert_eq!(s.values().unwrap().len(), 0);
        let eq = Expr::equation(x().sqrt() + Expr::one(), Expr::int(4));
        let s = solve(&eq, "x", None, &mut ctx).unwrap();
        assert_eq!(s.values().unwrap(), &[Expr::int(9)]);
    }

    #[test]
    fn substitution_kernels() {
        let mut ctx = Ctx::new();
        let ex = Expr::e().pow(x());
        let f = Expr::e().pow(Expr::int(2) * x()) - Expr::int(3) * ex + Expr::int(2);
        let s = solve(&f, "x", None, &mut ctx).unwrap();
        assert!(close(&values(&s), &[0.0, 2f64.ln()]));
        let sin = Expr::call("sin", vec![x()]).unwrap();
        let f = Expr::int(2) * sin.pow(2) - sin.clone();
        let range = Range {
            lo: Expr::zero(),
            hi: Expr::int(2) * Expr::pi(),
            lo_closed: true,
            hi_closed: false,
        };
        let s = solve(&f, "x", Some(&range), &mut ctx).unwrap();
        let pi = std::f64::consts::PI;
        assert!(
            close(&values(&s), &[0.0, pi / 6.0, 5.0 * pi / 6.0, pi]),
            "{s:?}"
        );
    }

    #[test]
    fn numeric_fallback() {
        let mut ctx = Ctx::new();
        let eq = Expr::equation(Expr::call("cos", vec![x()]).unwrap(), x());
        let s = solve(&eq, "x", None, &mut ctx).unwrap();
        assert!(matches!(s, Solutions::Finite { exact: false, .. }));
        assert!(close(&values(&s), &[0.739_085_133_215_160_6]));
        assert!(ctx.notes.iter().any(|n| n.contains("numerically")));
    }

    #[test]
    fn identities_and_contradictions() {
        let mut ctx = Ctx::new();
        let eq = Expr::equation(Expr::int(2) * x(), x() + x());
        assert_eq!(solve(&eq, "x", None, &mut ctx).unwrap(), Solutions::All);
        let eq = Expr::equation(x() + Expr::one(), x());
        assert_eq!(solve(&eq, "x", None, &mut ctx).unwrap(), Solutions::none());
    }

    #[test]
    fn domain_operators() {
        let mut ctx = Ctx::new();
        let r = Solving
            .apply(
                "solve",
                &[Expr::equation(x().pow(2), Expr::int(9))],
                &mut ctx,
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            r,
            Expr::new(ExprNode::Set(vec![Expr::int(-3), Expr::int(3)]))
        );
        let r = Solving
            .apply("roots", &[(x() - Expr::one()).pow(2)], &mut ctx)
            .unwrap()
            .unwrap();
        assert_eq!(r.to_string(), "((1, 2))");
        let r = Solving
            .apply(
                "nsolve",
                &[x().pow(2) - Expr::int(2), x(), Expr::one()],
                &mut ctx,
            )
            .unwrap()
            .unwrap();
        let ExprNode::Set(v) = r.node() else { panic!() };
        assert!((v[0].eval_f64(&EmptyEnv).unwrap() - 2f64.sqrt()).abs() < 1e-12);
    }
}
