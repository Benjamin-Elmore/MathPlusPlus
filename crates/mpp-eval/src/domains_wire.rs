//! The domain registry in documents: `\det A`, `A^{-1}`, `A^T`, `\operatorname{rank} A`,
//! `\operatorname{solve}_x (…)`, `17 \bmod 5`, `\operatorname{mean}(…)` … answer through
//! [`mpp_core::domains::Registry`].
//!
//! Three pieces:
//! - [`evaluate`], a [`crate::HolderEval`] for `Apply` nodes naming a registry operator
//!   (and `\operatorname{op}_{sub}` with the unknowns in the subscript);
//! - lowering helpers ([`matrix_binary`], [`matrix_power`]) that keep products of
//!   matrices in the order they are written. The core's products commute, so `A B`
//!   would otherwise be indistinguishable from `B A`. A product, sum or power is
//!   lowered to an ordered matrix operation when one side is a matrix literal or a
//!   name whose definition is one;
//! - [`finish`], run on every result, which does the matrix arithmetic that is safe in
//!   any order (sums, a scalar times one matrix, integer powers of one matrix) and
//!   refuses a product of several matrices whose order was lost.

use std::sync::OnceLock;

use mpp_core::domains::{Ctx as DomainCtx, FullCalculus, Registry};
use mpp_core::{Expr, ExprNode, MathError, MathResult, Name, Steps};

use crate::holders::Ctx;
use crate::lower::LowerScope;
use crate::value::Binding;

/// Ordered matrix product `(a, b, style)`; style 0 = juxtaposition or `*`, 1 = `\times`
/// (a cross product of two 3-vectors), 2 = `\cdot` (a dot product of two vectors).
pub const MAT_MUL: &str = "matrix product";
/// Matrix sum `(a, b)`.
pub const MAT_ADD: &str = "matrix sum";
/// Matrix difference `(a, b)`.
pub const MAT_SUB: &str = "matrix difference";
/// Matrix power `(a, n)`.
pub const MAT_POW: &str = "matrix power";
/// Transpose `(a)`.
pub const MAT_T: &str = "matrix transpose";

/// `max(x)` / `min(x)` of a data column (the core's `max` of one argument is itself).
pub const LIST_MAX: &str = "largest entry";
/// See [`LIST_MAX`].
pub const LIST_MIN: &str = "smallest entry";

/// `element(column, k)`: the `k`-th entry (from 1) of a data column, `x_k`.
pub const ELEMENT: &str = "element";

/// `set of(x, domain, condition)`: set-builder notation (`domain` is `()` when absent).
pub const SET_OF: &str = "set of";

/// Set operators whose arguments may be written `(a, b)` for an open interval or `ℝ`.
const SET_OPS: &[&str] = &[
    "union",
    "intersect",
    "setminus",
    "complement",
    "subset",
    "card",
];

/// Operators that take their unknowns from the subscript of `\operatorname{op}_{x}`.
const UNKNOWN_OPS: &[&str] = &["solve", "nsolve", "roots", "linsolve"];

/// Deepest chain of definitions followed when deciding whether a name is a matrix.
const MAX_LOOKUP_DEPTH: usize = 16;

/// The standard registry, built once.
pub fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(Registry::standard)
}

// ---- the holder ------------------------------------------------------------

/// Evaluates registry operators and the ordered matrix operations.
pub fn evaluate(e: &Expr, ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
    if let Some(r) = piecewise_calculus(e, ctx) {
        return Some(r);
    }
    if let Some(r) = sum_of_entries(e) {
        return Some(r);
    }
    let ExprNode::Apply(name, args) = e.node() else {
        return None;
    };
    match &**name {
        MAT_MUL => matrix_product(args, ctx),
        MAT_ADD => matrix_sum(args, 1, ctx),
        MAT_SUB => matrix_sum(args, -1, ctx),
        MAT_POW => matrix_pow(args, ctx),
        MAT_T => match args.as_slice() {
            [m] if is_matrix(m) => Some(call("transpose", args, ctx)),
            [m] if m.is_constant() => Some(Ok(m.clone())),
            _ => None,
        },
        "operatorname" => operatorname(args, ctx),
        // `\operatorname{factor}(360)` factors the integer; a polynomial is left to `factor`.
        "factor" => match args.as_slice() {
            [n] if mpp_core::domains::util::integer(n).is_some() => {
                Some(show_factorisation(n, ctx))
            }
            _ => None,
        },
        SET_OF => Some(set_builder(args)),
        ELEMENT => match args.as_slice() {
            [list, k] if k.as_number().is_some() => Some(entry(list, k)),
            _ => None,
        },
        op if SET_OPS.contains(&op) => {
            let args: Vec<Expr> = args.iter().map(as_real_set).collect();
            Some(call(op, &args, ctx))
        }
        "in" if is_decidable_member(args) => {
            let args: Vec<Expr> = args.iter().map(as_real_set).collect();
            Some(call("member", &args, ctx))
        }
        "notin" if is_decidable_member(args) => Some(call("member", args, ctx).map(|b| negate(&b))),
        "subset" | "subseteq" => Some(call("subset", args, ctx)),
        "supset" | "supseteq" => {
            let swapped: Vec<Expr> = args.iter().rev().cloned().collect();
            Some(call("subset", &swapped, ctx))
        }
        LIST_MAX | LIST_MIN => match args.as_slice() {
            [list] => Some(list_extreme(list, &**name == LIST_MAX)),
            _ => None,
        },
        op if registry().handles(op) => {
            if args.iter().any(crate::holders::contains_holder) {
                return None;
            }
            if let Some(r) = statistics_with_units(op, args, ctx) {
                return Some(r);
            }
            Some(call(op, args, ctx))
        }
        _ => None,
    }
}

/// Runs a registry operator with the full calculus and the statement's step log.
fn call(op: &str, args: &[Expr], ctx: &Ctx<'_>) -> MathResult<Expr> {
    let mut steps = Steps::new();
    let mut notes = Vec::new();
    let result = {
        let mut dctx = DomainCtx::new().with_calculus(&FullCalculus);
        if ctx.logging() {
            dctx.steps = Some(&mut steps);
        }
        let out = registry().call(op, args, &mut dctx);
        notes.append(&mut dctx.notes);
        out
    };
    ctx.append(steps);
    for n in notes {
        ctx.note(n);
    }
    result
}

/// `operatorname(op, sub, sup, (args…))`: the subscript names the unknowns.
fn operatorname(args: &[Expr], ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
    let [which, sub, sup, body] = args else {
        return None;
    };
    let op = which.as_sym()?;
    if !registry().handles(op) || !is_empty_tuple(sup) {
        return None;
    }
    let ExprNode::Tuple(items) = body.node() else {
        return None;
    };
    let mut call_args = items.clone();
    if !is_empty_tuple(sub) {
        if !UNKNOWN_OPS.contains(&op) {
            return None;
        }
        // `solve_x (a = b, c = d)`: several equations become one system.
        if call_args.len() > 1 {
            call_args = vec![Expr::new(ExprNode::Tuple(call_args))];
        }
        call_args.push(sub.clone());
    }
    if call_args.iter().any(crate::holders::contains_holder) {
        return None;
    }
    Some(call(op, &call_args, ctx))
}

fn is_empty_tuple(e: &Expr) -> bool {
    matches!(e.node(), ExprNode::Tuple(items) if items.is_empty())
}

fn negate(b: &Expr) -> Expr {
    match b.node() {
        ExprNode::Bool(v) => Expr::new(ExprNode::Bool(!v)),
        _ => Expr::new(ExprNode::Logic(mpp_core::LogicOp::Not, vec![b.clone()])),
    }
}

/// Shows `n` as its prime factorisation `2^3 · 3^2 · 5` (the value stays `n`).
fn show_factorisation(n: &Expr, ctx: &Ctx<'_>) -> MathResult<Expr> {
    let f = call("factorint", std::slice::from_ref(n), ctx)?;
    let ExprNode::Tuple(pairs) = f.node() else {
        return Ok(n.clone());
    };
    let mut text = Vec::with_capacity(pairs.len());
    let mut latex = Vec::with_capacity(pairs.len());
    for pair in pairs {
        let ExprNode::Tuple(pk) = pair.node() else {
            return Ok(n.clone());
        };
        let [p, k] = pk.as_slice() else {
            return Ok(n.clone());
        };
        if k.is_one() {
            text.push(p.to_string());
            latex.push(p.to_latex());
        } else {
            text.push(format!("{p}^{k}"));
            latex.push(format!("{}^{{{}}}", p.to_latex(), k.to_latex()));
        }
    }
    if text.len() > 1 || text.iter().any(|t| t.contains('^')) {
        ctx.show_as(n, text.join(" · "), latex.join(" \\cdot "));
    }
    Ok(n.clone())
}

/// Names an operator binds in its arguments (the unknowns of `solve`, the function and
/// variable of `dsolve`), which must stay symbolic while the arguments are resolved.
pub fn bound_names(name: &str, args: &[Expr]) -> Vec<Name> {
    let syms = |e: &Expr| -> Vec<Name> {
        match e.node() {
            ExprNode::Tuple(items) | ExprNode::Set(items) => items
                .iter()
                .filter_map(|i| i.as_sym().map(Name::from))
                .collect(),
            ExprNode::Sym(s) => vec![s.clone()],
            _ => Vec::new(),
        }
    };
    match (name, args) {
        ("solve" | "nsolve" | "roots", [_, vars, ..]) => syms(vars),
        ("dsolve", [_, y, x, ..]) => {
            let mut out = syms(y);
            out.extend(syms(x));
            out
        }
        ("operatorname", [which, sub, ..])
            if which.as_sym().is_some_and(|op| UNKNOWN_OPS.contains(&op)) =>
        {
            syms(sub)
        }
        (SET_OF, [var, ..]) => syms(var),
        _ => Vec::new(),
    }
}

/// Operators whose first argument is a differential equation.
const ODE_OPS: &[&str] = &["dsolve", "rk4", "rk4table"];

/// Keeps `y'` and `dy/dx` in a differential equation from being evaluated as
/// derivatives of a constant before the solver sees them: each first derivative of a
/// bare name becomes the name `y′`, which the solver reads as the derivative.
pub fn protect_ode_args(op: &str, args: &mut [Expr]) {
    // Only the registry's form names the unknown and the variable; the ODE toolkit's form
    // (`dsolve(y' = y, y(0) = 1)`, systems) reads the derivatives themselves.
    if !ODE_OPS.contains(&op) || !crate::dispatch::is_registry_ode_form(op, args) {
        return;
    }
    if let Some(eq) = args.first_mut() {
        *eq = protect(eq);
    }
}

fn protect(e: &Expr) -> Expr {
    match e.node() {
        ExprNode::Derivative { expr, vars } if vars.len() == 1 && vars[0].1 == 1 => {
            match expr.as_sym() {
                Some(y) => Expr::sym(&format!("{y}′")),
                None => e.clone(),
            }
        }
        _ => Expr::new(e.map_children(&mut protect)),
    }
}

// ---- data entries ------------------------------------------------------------------

/// The `k`-th entry (from 1) of a list.
fn entry(list: &Expr, k: &Expr) -> MathResult<Expr> {
    let ExprNode::Tuple(items) = list.node() else {
        return Err(MathError::NotANumber(format!("{list} as a list of data")));
    };
    let i = mpp_core::domains::util::integer(k)
        .and_then(|i| usize::try_from(i).ok())
        .filter(|i| (1..=items.len()).contains(i))
        .ok_or_else(|| MathError::Domain {
            function: "an entry of the data".into(),
            detail: format!(
                "the index {k} must be a whole number from 1 to {}",
                items.len()
            ),
        })?;
    Ok(items[i - 1].clone())
}

/// Replaces every `element(list, k)` with a known `k` by the entry.
fn pick_entries(e: &Expr) -> MathResult<Expr> {
    if let ExprNode::Apply(name, args) = e.node() {
        if &**name == ELEMENT {
            if let [list, k] = args.as_slice() {
                let k = pick_entries(k)?.simplify();
                if k.as_number().is_some() {
                    return entry(list, &k);
                }
            }
        }
    }
    let mut failure = None;
    let node = e.map_children(&mut |c| match pick_entries(c) {
        Ok(v) => v,
        Err(err) => {
            failure.get_or_insert(err);
            c.clone()
        }
    });
    match failure {
        Some(err) => Err(err),
        None => Ok(mpp_core::simplify::canonical(node)),
    }
}

/// `∑_{i=1}^{n} x_i` (and `∏`) over data entries, term by term.
fn sum_of_entries(e: &Expr) -> Option<MathResult<Expr>> {
    let ExprNode::BigOp {
        kind,
        expr,
        var,
        lower,
        upper,
    } = e.node()
    else {
        return None;
    };
    if !mentions_entry(expr) {
        return None;
    }
    let lo = lower.as_number()?.as_i64()?;
    let hi = upper.as_number()?.as_i64()?;
    if hi.saturating_sub(lo) >= crate::holders::MAX_EXACT_TERMS {
        return None;
    }
    let mut terms = Vec::new();
    for k in lo..=hi {
        match pick_entries(&expr.subs(var, &Expr::int(k))) {
            Ok(t) => terms.push(t),
            Err(err) => return Some(Err(err)),
        }
    }
    Some(Ok(match kind {
        mpp_core::BigOpKind::Sum => mpp_core::add(terms),
        mpp_core::BigOpKind::Product => mpp_core::mul(terms),
    }))
}

fn mentions_entry(e: &Expr) -> bool {
    matches!(e.node(), ExprNode::Apply(n, _) if &**n == ELEMENT)
        || e.children().into_iter().any(mentions_entry)
}

/// Statistics whose result has the data's unit to this power (0: no unit).
const STAT_UNITS: &[(&str, i64)] = &[
    ("mean", 1),
    ("median", 1),
    ("mode", 1),
    ("std", 1),
    ("pstd", 1),
    ("var", 2),
    ("pvar", 2),
    ("quartiles", 1),
    ("iqr", 1),
    ("ci", 1),
    ("corr", 0),
];

/// Regression fits: computed on the plain values when the data have units.
const FIT_OPS: &[&str] = &["linreg", "polyreg", "expreg", "powreg", "logreg"];

/// A statistic of data with units: computed on the plain values in one unit, with the
/// unit put back on the answer (`var` in m², `corr` without one).
fn statistics_with_units(op: &str, args: &[Expr], ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
    let power = STAT_UNITS.iter().find(|(o, _)| *o == op).map(|(_, p)| *p);
    let fit = FIT_OPS.contains(&op);
    if power.is_none() && !fit {
        return None;
    }
    if !args.iter().any(crate::units_wire::contains_units) {
        return None;
    }
    let mut plain = Vec::with_capacity(args.len());
    let mut first_unit = None;
    let mut units = Vec::new();
    for a in args {
        match crate::units_wire::strip_list(a) {
            Ok(Some((values, u))) => {
                units.push(u.text());
                if first_unit.is_none() {
                    first_unit = Some(u);
                }
                plain.push(Expr::new(ExprNode::Tuple(values)));
            }
            Ok(None) => plain.push(a.clone()),
            Err(err) => return Some(Err(err)),
        }
    }
    let result = match call(op, &plain, ctx) {
        Ok(r) => r,
        Err(err) => return Some(Err(err)),
    };
    if fit {
        ctx.note(format!("fitted to the values in {}", units.join(" and ")));
        return Some(Ok(result));
    }
    let unit = match (first_unit, power) {
        (Some(u), Some(1)) => u,
        (Some(u), Some(2)) => match u.mul(&u) {
            Ok(u2) => u2,
            Err(err) => return Some(Err(err)),
        },
        _ => return Some(Ok(result)),
    };
    let f = crate::units_wire::factor(&unit);
    let attach = |v: &Expr| (v * &f).simplify();
    Some(Ok(match result.node() {
        ExprNode::Tuple(items) => Expr::new(ExprNode::Tuple(items.iter().map(attach).collect())),
        ExprNode::Interval {
            lo,
            hi,
            lo_closed,
            hi_closed,
        } => Expr::new(ExprNode::Interval {
            lo: attach(lo),
            hi: attach(hi),
            lo_closed: *lo_closed,
            hi_closed: *hi_closed,
        }),
        _ => attach(&result),
    }))
}

/// The largest or smallest entry of a list (of numbers, or of quantities of one kind).
fn list_extreme(list: &Expr, largest: bool) -> MathResult<Expr> {
    let ExprNode::Tuple(items) = list.node() else {
        return Ok(Expr::new(ExprNode::Func(
            if largest {
                mpp_core::Func::Max
            } else {
                mpp_core::Func::Min
            },
            vec![list.clone()],
        )));
    };
    let (values, unit) = match crate::units_wire::strip_list(list)? {
        Some((v, u)) => (v, Some(u)),
        None => (items.clone(), None),
    };
    let mut best: Option<usize> = None;
    for (i, v) in values.iter().enumerate() {
        best = Some(match best {
            None => i,
            Some(b) => match mpp_core::domains::util::compare(v, &values[b]) {
                Some(std::cmp::Ordering::Greater) if largest => i,
                Some(std::cmp::Ordering::Less) if !largest => i,
                Some(_) => b,
                None => {
                    return Err(MathError::Unsupported(format!(
                        "comparing {v} and {} (the entries must be numbers)",
                        values[b]
                    )))
                }
            },
        });
    }
    let i =
        best.ok_or_else(|| MathError::Undefined("the largest entry of an empty list".into()))?;
    Ok(match unit {
        Some(u) => (values[i].clone() * crate::units_wire::factor(&u)).simplify(),
        None => values[i].clone(),
    })
}

// ---- sets ------------------------------------------------------------------------

/// `(a, b)` where a set is expected is the open interval, and `ℝ` is the whole line.
fn as_real_set(e: &Expr) -> Expr {
    match e.node() {
        ExprNode::Tuple(items)
            if items.len() == 2
                && items
                    .iter()
                    .all(|i| mpp_core::domains::util::numeric(i).is_some() || i.is_infinite()) =>
        {
            Expr::new(ExprNode::Interval {
                lo: items[0].clone(),
                hi: items[1].clone(),
                lo_closed: false,
                hi_closed: false,
            })
        }
        ExprNode::Sym(s) if matches!(&**s, "ℝ" | "R") => {
            mpp_core::domains::RealSet::reals().to_expr()
        }
        _ => e.clone(),
    }
}

/// Most integers listed for a set-builder over ℤ.
const MAX_LISTED: i64 = 10_000;

/// `\{x \in D : cond\}`: the condition solved for `x`, within `D` (an interval, ℝ, or
/// ℤ / ℕ, whose members are listed when there are finitely many).
fn set_builder(args: &[Expr]) -> MathResult<Expr> {
    use mpp_core::domains::intervals::solve_inequality;
    use mpp_core::domains::RealSet;
    let [var, domain, cond] = args else {
        return Err(MathError::WrongArity {
            function: "a set-builder".into(),
            expected: "3".into(),
            got: args.len(),
        });
    };
    let x = var
        .as_sym()
        .ok_or_else(|| MathError::Unsupported(format!("a set-builder over {var}")))?;
    let set = solve_inequality(cond, x)?;
    match domain.as_sym() {
        Some("ℤ" | "Z" | "ℕ" | "N") => {
            let natural = matches!(domain.as_sym(), Some("ℕ" | "N"));
            let pieces = set.pieces();
            let (Some(first), Some(last)) = (pieces.first(), pieces.last()) else {
                return Ok(Expr::new(ExprNode::Set(Vec::new())));
            };
            let lo = if natural && first.lo.is_infinite() {
                0.0
            } else {
                mpp_core::domains::util::numeric(&first.lo).ok_or_else(|| {
                    MathError::TooLarge("the list of integers (there are infinitely many)".into())
                })?
            };
            let hi = mpp_core::domains::util::numeric(&last.hi).ok_or_else(|| {
                MathError::TooLarge("the list of integers (there are infinitely many)".into())
            })?;
            let (lo, hi) = (lo.floor() as i64, hi.ceil() as i64);
            if hi.saturating_sub(lo) > MAX_LISTED {
                return Err(MathError::TooLarge("the list of integers".into()));
            }
            let start = if natural { lo.max(0) } else { lo };
            let mut members = Vec::new();
            for k in start..=hi {
                if set.contains(&Expr::int(k))? {
                    members.push(Expr::int(k));
                }
            }
            Ok(Expr::new(ExprNode::Set(members)))
        }
        _ => {
            let within = if is_empty_tuple(domain) {
                RealSet::reals()
            } else {
                RealSet::from_expr(&as_real_set(domain))?
            };
            Ok(set.intersection(&within)?.to_expr())
        }
    }
}

/// `A^c` and `A^\complement` for a set.
pub fn set_power(base: &Expr, exp: &Expr) -> Option<Expr> {
    let set_like = matches!(base.node(), ExprNode::Interval { .. } | ExprNode::Set(_))
        || matches!(base.node(), ExprNode::Apply(n, _) if matches!(&**n, "union" | "intersect" | "setminus" | SET_OF));
    if set_like && matches!(exp.as_sym(), Some("c" | "∁" | "complement")) {
        return Some(Expr::apply("complement", vec![base.clone()]));
    }
    None
}

/// `[0, 1) ∪ (2, 3]` for a union of intervals, which the core keeps as `union(…)`.
pub fn present_set(e: &Expr) -> Option<(String, String)> {
    let ExprNode::Apply(name, parts) = e.node() else {
        return None;
    };
    if &**name != "union" || parts.len() < 2 {
        return None;
    }
    let text: Vec<String> = parts.iter().map(ToString::to_string).collect();
    let latex: Vec<String> = parts.iter().map(Expr::to_latex).collect();
    Some((text.join(" ∪ "), latex.join(" \\cup ")))
}

// ---- piecewise calculus ------------------------------------------------------------

/// Derivatives and integrals of a piecewise function, piece by piece, with what happens
/// at the joins noted for the reader.
fn piecewise_calculus(e: &Expr, ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
    use mpp_core::domains::piecewise;
    match e.node() {
        ExprNode::Derivative { expr, vars }
            if matches!(expr.node(), ExprNode::Piecewise { .. }) =>
        {
            let [(var, order)] = vars.as_slice() else {
                return None;
            };
            let mut current = expr.clone();
            for _ in 0..*order {
                match piecewise::derivative(&current, var) {
                    Ok(Some((d, joins))) => {
                        for j in &joins {
                            if current == *expr {
                                ctx.note(j.describe(var));
                            }
                        }
                        current = d;
                    }
                    Ok(None) => return None,
                    Err(err) => return Some(Err(err)),
                }
            }
            ctx.log("differentiate each piece", e, &current, None);
            Some(Ok(current))
        }
        ExprNode::Integral { expr, var, bounds }
            if matches!(expr.node(), ExprNode::Piecewise { .. }) =>
        {
            match bounds {
                Some((lo, hi)) => match piecewise::integrate_definite(expr, var, lo, hi) {
                    Ok(Some(v)) => {
                        ctx.log(
                            "integrate each piece over its part of the range",
                            e,
                            &v,
                            None,
                        );
                        Some(Ok(v))
                    }
                    Ok(None) => None,
                    Err(err) => Some(Err(err)),
                },
                None => {
                    match piecewise::antiderivative(expr, var) {
                        Ok(Some(f)) => {
                            let c = if f.contains_sym("C") { "C_1" } else { "C" };
                            let value =
                                mpp_core::simplify(&mpp_core::add(vec![f.clone(), Expr::sym(c)]));
                            ctx.show_as(
                                &value,
                                format!("{f} + {c}"),
                                format!("{} + {c}", f.to_latex()),
                            );
                            ctx.note("the pieces' constants are chosen so the antiderivative is continuous");
                            ctx.log(
                                "integrate each piece and join them continuously",
                                e,
                                &value,
                                None,
                            );
                            Some(Ok(value))
                        }
                        Ok(None) => None,
                        Err(err) => Some(Err(err)),
                    }
                }
            }
        }
        _ => None,
    }
}

// ---- matrices in lowering --------------------------------------------------

/// True when `e` (a lowered expression) is certainly a matrix: a matrix literal, an
/// ordered matrix operation, a matrix-valued registry operator, or a name defined as one.
pub fn matrix_like(e: &Expr, scope: &dyn LowerScope) -> bool {
    matrix_like_at(e, scope, 0)
}

fn matrix_like_at(e: &Expr, scope: &dyn LowerScope, depth: usize) -> bool {
    if depth > MAX_LOOKUP_DEPTH {
        return false;
    }
    match e.node() {
        ExprNode::Matrix { .. } => true,
        ExprNode::Apply(name, args) => match &**name {
            MAT_MUL | MAT_ADD | MAT_SUB | MAT_POW | MAT_T => true,
            "inverse" | "inv" | "transpose" | "rref" | "adjugate" | "matmul" | "matadd"
            | "matsub" | "matpow" | "identity" => true,
            "scale" => args.iter().any(|a| matrix_like_at(a, scope, depth + 1)),
            _ => false,
        },
        ExprNode::Sym(s) => match scope.binding(s).as_deref() {
            Some(Binding::Variable { value, .. }) => matrix_like_at(value, scope, depth + 1),
            _ => false,
        },
        ExprNode::Mul(fs) => fs.iter().any(|f| matrix_like_at(f, scope, depth + 1)),
        ExprNode::Pow(b, _) => matrix_like_at(b, scope, depth + 1),
        _ => false,
    }
}

/// The kind of arithmetic written between two operands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatrixOp {
    /// Juxtaposition or `*`.
    Mul,
    /// `\cdot` (a dot product for two vectors of the same shape).
    Dot,
    /// `\times` (a cross product for two 3-vectors).
    Times,
    Add,
    Sub,
}

/// `a op b` as an ordered matrix operation when either side is a matrix.
pub fn matrix_binary(op: MatrixOp, a: &Expr, b: &Expr, scope: &dyn LowerScope) -> Option<Expr> {
    if !(matrix_like(a, scope) || matrix_like(b, scope)) {
        return None;
    }
    Some(match op {
        MatrixOp::Mul => Expr::apply(MAT_MUL, vec![a.clone(), b.clone(), Expr::zero()]),
        MatrixOp::Times => Expr::apply(MAT_MUL, vec![a.clone(), b.clone(), Expr::one()]),
        MatrixOp::Dot => Expr::apply(MAT_MUL, vec![a.clone(), b.clone(), Expr::int(2)]),
        MatrixOp::Add => Expr::apply(MAT_ADD, vec![a.clone(), b.clone()]),
        MatrixOp::Sub => Expr::apply(MAT_SUB, vec![a.clone(), b.clone()]),
    })
}

/// `A^{-1}`, `A^n`, `A^T` (`T`, `\top`, `\intercal`, `\mathsf{T}`) when `A` is a matrix.
pub fn matrix_power(base: &Expr, exp: &Expr, scope: &dyn LowerScope) -> Option<Expr> {
    if !matrix_like(base, scope) {
        return None;
    }
    if let Some(s) = exp.as_sym() {
        if matches!(s, "T" | "⊤" | "top" | "intercal" | "t") {
            return Some(Expr::apply(MAT_T, vec![base.clone()]));
        }
    }
    Some(Expr::apply(MAT_POW, vec![base.clone(), exp.clone()]))
}

// ---- matrix holders ----------------------------------------------------------

fn is_matrix(e: &Expr) -> bool {
    matches!(e.node(), ExprNode::Matrix { .. })
}

fn is_vector(e: &Expr) -> Option<usize> {
    match e.node() {
        ExprNode::Matrix { rows, cols, .. } if *rows == 1 || *cols == 1 => Some(rows * cols),
        ExprNode::Tuple(items) => Some(items.len()),
        _ => None,
    }
}

/// A plain value that can scale a matrix: not a matrix, set, tuple, relation or holder.
fn is_scalar(e: &Expr) -> bool {
    !matches!(
        e.node(),
        ExprNode::Matrix { .. }
            | ExprNode::Tuple(_)
            | ExprNode::Set(_)
            | ExprNode::Interval { .. }
            | ExprNode::Relation(..)
            | ExprNode::Logic(..)
            | ExprNode::Bool(_)
    ) && !crate::holders::contains_holder(e)
        && !matches!(e.node(), ExprNode::Apply(n, _) if is_matrix_op(n))
}

fn is_matrix_op(name: &str) -> bool {
    matches!(name, MAT_MUL | MAT_ADD | MAT_SUB | MAT_POW | MAT_T)
}

fn matrix_product(args: &[Expr], ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
    let [a, b, style] = args else {
        return None;
    };
    let times = style.is_one();
    let dot = *style == Expr::int(2);
    match (is_matrix(a), is_matrix(b)) {
        (true, true) => {
            let (na, nb) = (is_vector(a), is_vector(b));
            if let (Some(3), Some(3), true) = (na, nb, times) {
                return Some(call("cross", &[a.clone(), b.clone()], ctx));
            }
            // `u \cdot v` of two vectors of the same length is the dot product.
            let same_shape = matches!(
                (a.node(), b.node()),
                (ExprNode::Matrix { rows: r1, cols: c1, .. }, ExprNode::Matrix { rows: r2, cols: c2, .. })
                    if r1 == r2 && c1 == c2
            );
            if dot && same_shape && na.is_some() && na == nb && na != Some(1) {
                return Some(call("dot", &[a.clone(), b.clone()], ctx));
            }
            Some(call("matmul", &[a.clone(), b.clone()], ctx))
        }
        (false, true) if is_scalar(a) => Some(call("scale", &[a.clone(), b.clone()], ctx)),
        (true, false) if is_scalar(b) => Some(call("scale", &[b.clone(), a.clone()], ctx)),
        (false, false) if is_scalar(a) && is_scalar(b) => Some(Ok(a * b)),
        _ => None,
    }
}

fn matrix_sum(args: &[Expr], sign: i64, ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
    let [a, b] = args else {
        return None;
    };
    match (is_matrix(a), is_matrix(b)) {
        (true, true) => Some(call(
            if sign > 0 { "matadd" } else { "matsub" },
            &[a.clone(), b.clone()],
            ctx,
        )),
        (false, false) if is_scalar(a) && is_scalar(b) => {
            Some(Ok(a + &(Expr::int(sign) * b.clone())))
        }
        (true, false) | (false, true) if is_scalar(a) || is_scalar(b) => {
            let verb = if sign > 0 { "add" } else { "subtract" };
            Some(Err(MathError::NotANumber(format!(
                "a matrix and a number, which can't be added (to {verb} them, multiply the number by the identity matrix)"
            ))))
        }
        _ => None,
    }
}

fn matrix_pow(args: &[Expr], ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
    let [m, n] = args else {
        return None;
    };
    if !is_matrix(m) {
        return if is_scalar(m) && is_scalar(n) {
            Some(Ok(m.pow(n.clone())))
        } else {
            None
        };
    }
    if mpp_core::domains::util::integer(n).is_none() {
        return Some(Err(MathError::Domain {
            function: "a matrix power".into(),
            detail: format!("the exponent {n} must be a whole number"),
        }));
    }
    if n.as_number().is_some_and(|k| k.as_i64() == Some(-1)) {
        return Some(call("inverse", std::slice::from_ref(m), ctx));
    }
    Some(call("matpow", args, ctx))
}

// ---- results -----------------------------------------------------------------

/// Matrix arithmetic left in the core's commutative form after resolution: a sum of
/// matrices, a scalar times one matrix and an integer power of one matrix are done
/// here (their order doesn't matter); a product of several matrices is an error, since
/// its order is no longer known.
pub fn finish(e: &Expr) -> MathResult<Expr> {
    let e = &spread_lists(e);
    if !contains_matrix(e) {
        return Ok(e.clone());
    }
    let mut failure = None;
    let node = e.map_children(&mut |c| match finish(c) {
        Ok(v) => v,
        Err(err) => {
            failure.get_or_insert(err);
            c.clone()
        }
    });
    if let Some(err) = failure {
        return Err(err);
    }
    let e = Expr::new(node);
    let mut dctx = DomainCtx::new();
    let reg = registry();
    match e.node() {
        ExprNode::Add(terms) if terms.iter().any(is_matrix) => {
            if !terms.iter().all(is_matrix) {
                return Err(MathError::NotANumber(
                    "a matrix and a number, which can't be added".into(),
                ));
            }
            let mut acc = terms[0].clone();
            for t in &terms[1..] {
                acc = reg.call("matadd", &[acc, t.clone()], &mut dctx)?;
            }
            Ok(acc)
        }
        ExprNode::Mul(factors) if factors.iter().any(is_matrix) => {
            let (mats, scalars): (Vec<&Expr>, Vec<&Expr>) =
                factors.iter().partition(|f| is_matrix(f));
            let [m] = mats.as_slice() else {
                return Err(MathError::Unsupported(
                    "a product of matrices whose order is not known (define each matrix with := and multiply the names, or use \\operatorname{matmul})".into(),
                ));
            };
            if !scalars.iter().all(|s| is_scalar(s)) {
                return Err(MathError::NotANumber(
                    "a product of a matrix with something that is not a number".into(),
                ));
            }
            let k = mpp_core::mul(scalars.into_iter().cloned().collect());
            reg.call("scale", &[k, (*m).clone()], &mut dctx)
        }
        ExprNode::Pow(b, k) if is_matrix(b) => {
            if mpp_core::domains::util::integer(k).is_none() {
                return Err(MathError::Domain {
                    function: "a matrix power".into(),
                    detail: format!("the exponent {k} must be a whole number"),
                });
            }
            reg.call("matpow", &[b.clone(), k.clone()], &mut dctx)
        }
        _ => Ok(e),
    }
}

/// `max(x)` and `min(x)` of a data column: the largest or smallest entry.
fn spread_lists(e: &Expr) -> Expr {
    let e = Expr::new(e.map_children(&mut spread_lists));
    match e.node() {
        ExprNode::Func(f @ (mpp_core::Func::Min | mpp_core::Func::Max), args) => {
            match args.as_slice() {
                [list] => match list.node() {
                    ExprNode::Tuple(items) if !items.is_empty() => {
                        Expr::func(*f, items.clone()).unwrap_or(e.clone())
                    }
                    _ => e,
                },
                _ => e,
            }
        }
        _ => e.simplify(),
    }
}

fn contains_matrix(e: &Expr) -> bool {
    is_matrix(e) || e.children().into_iter().any(contains_matrix)
}

/// `x ∈ S` is answered only for a known element (a condition such as `x ∈ [0, 1]` in a
/// piecewise definition stays a condition).
fn is_decidable_member(args: &[Expr]) -> bool {
    matches!(args, [x, _] if x.is_constant())
}
