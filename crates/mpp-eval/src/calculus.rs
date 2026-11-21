//! The calculus holders: derivatives, integrals, limits, sums and products, Taylor series
//! and the ∇ operators, answered by [`mpp_core::calc`].
//!
//! By the time a holder reaches [`evaluate`] the resolver has put the values of known names
//! and the bodies of user functions into it, so `\frac{d}{dx} y(x)` with `y(x) := …`
//! differentiates the body. Names without a value stay symbolic in exact mode.
//!
//! Each core routine records its working; when the statement asks for it (`= ?!`) the
//! steps are appended to the statement's log. A definite integral or series without a
//! closed form falls back to its numerical value, and a remark says so. Divergent
//! integrals and series, limits that do not exist and integrals with no antiderivative are
//! errors whose messages come from the core.

use mpp_core::calc::{
    convergence, diff, integrate, integrate_definite, integrate_multiple, integrate_with_constant,
    limit, product, series, sum,
};
use mpp_core::{
    add, mul, simplify, BigOpKind, Expr, ExprNode, MathError, MathResult, Name, RelOp, Steps,
};

use crate::holders::{Ctx, MAX_EXACT_TERMS};

/// Order of a Taylor series written without one (`\operatorname{series}_{x=0} f`).
pub const DEFAULT_SERIES_ORDER: u32 = 6;

/// Names of the ∇ and related operators lowering produces as `Apply` holders.
const VECTOR_OPS: &[&str] = &["grad", "div", "curl", "laplacian", "hessian", "jacobian"];

/// The remark left when a value had to be computed numerically.
fn numeric_note(what: &str) -> String {
    format!("no closed form was found for {what}, so the value shown is numerical (≈)")
}

/// Evaluates a calculus holder; `None` for nodes that are not calculus.
pub fn evaluate(e: &Expr, ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
    match e.node() {
        ExprNode::Derivative { expr, vars } => Some(derivative(expr, vars, ctx)),
        ExprNode::Integral { .. } => Some(integral(e, ctx)),
        ExprNode::Limit {
            expr,
            var,
            point,
            dir,
        } => Some(logged(ctx, |s| limit(expr, var, point, *dir, s))),
        ExprNode::BigOp {
            kind,
            expr,
            var,
            lower,
            upper,
        } => big_op(*kind, expr, var, lower, upper, ctx),
        ExprNode::Apply(name, args) if VECTOR_OPS.contains(&&**name) => vector_op(name, args, ctx),
        ExprNode::Apply(name, args) if &**name == "operatorname" => operatorname(args, ctx),
        _ => None,
    }
}

/// Names an `Apply` holder binds in its arguments, which must stay symbolic while the
/// arguments are resolved: the variables of `grad(f, (x, y))`, the expansion variable of
/// `\operatorname{series}_{x=a}` and the variable of `subs(inner, t = value)`.
pub fn bound_names(name: &str, args: &[Expr]) -> Vec<Name> {
    let syms = |e: &Expr| -> Vec<Name> {
        match e.node() {
            ExprNode::Tuple(items) => items
                .iter()
                .filter_map(|i| i.as_sym().map(Name::from))
                .collect(),
            ExprNode::Sym(s) => vec![s.clone()],
            _ => Vec::new(),
        }
    };
    match (name, args) {
        (op, [_, vars]) if VECTOR_OPS.contains(&op) => syms(vars),
        ("operatorname", [which, sub, ..])
            if matches!(which.as_sym(), Some("series" | "taylor")) =>
        {
            series_point(sub)
                .map(|(v, _)| vec![Name::from(v.as_str())])
                .unwrap_or_default()
        }
        ("subs", [_, binding]) => match binding.node() {
            ExprNode::Relation(RelOp::Eq, var, _) => syms(var),
            _ => Vec::new(),
        },
        _ => crate::helpers::bound_names(name, args),
    }
}

/// Runs a core routine with a step log when the statement asks for steps.
fn logged<T>(ctx: &Ctx<'_>, f: impl FnOnce(Option<&mut Steps>) -> MathResult<T>) -> MathResult<T> {
    if !ctx.logging() {
        return f(None);
    }
    let mut local = Steps::new();
    let out = f(Some(&mut local));
    ctx.append(local);
    out
}

/// True when `e` holds an approximate (floating-point) number.
fn has_float(e: &Expr) -> bool {
    match e.node() {
        ExprNode::Num(n) => !n.is_exact(),
        _ => e.children().into_iter().any(has_float),
    }
}

/// Notes a numerical fallback: the answer is approximate though the question was exact.
fn note_if_numeric(ctx: &Ctx<'_>, question: &Expr, answer: &Expr, what: &str) {
    if !ctx.numeric && has_float(answer) && !has_float(question) {
        ctx.note(numeric_note(what));
    }
}

// ---- derivatives -----------------------------------------------------------

fn derivative(expr: &Expr, vars: &[(Name, u32)], ctx: &Ctx<'_>) -> MathResult<Expr> {
    let mut current = expr.clone();
    for (var, order) in vars {
        current = logged(ctx, |s| diff(&current, var, *order, s))?;
    }
    Ok(current)
}

// ---- integrals -------------------------------------------------------------

/// One level of a (possibly nested) integral: its variable and bounds.
type Level = (Name, Option<(Expr, Expr)>);

fn integral(e: &Expr, ctx: &Ctx<'_>) -> MathResult<Expr> {
    // Outermost first: `∫_0^1 ∫_0^x f dy dx` is [x, y] with integrand f.
    let mut levels: Vec<Level> = Vec::new();
    let mut body = e;
    while let ExprNode::Integral { expr, var, bounds } = body.node() {
        levels.push((var.clone(), bounds.clone()));
        body = expr;
    }
    let integrand = body.clone();
    let what = if levels.len() > 1 {
        "this multiple integral"
    } else {
        "this integral"
    };
    let all_bounded = levels.iter().all(|(_, b)| b.is_some());
    if all_bounded && levels.len() > 1 {
        let bounds: Vec<(&str, Expr, Expr)> = levels
            .iter()
            .rev()
            .filter_map(|(v, b)| b.as_ref().map(|(lo, hi)| (&**v, lo.clone(), hi.clone())))
            .collect();
        let value = logged(ctx, |s| integrate_multiple(&integrand, &bounds, s))?;
        note_if_numeric(ctx, e, &value, what);
        return Ok(value);
    }
    if let [(var, None)] = levels.as_slice() {
        let result = logged(ctx, |s| integrate_with_constant(&integrand, var, s))?;
        return Ok(with_constant(ctx, &result.antiderivative, &result.constant));
    }
    // Innermost first, one level at a time; an indefinite level gets one constant at the end.
    let mut current = integrand;
    let mut indefinite = false;
    for (var, bounds) in levels.iter().rev() {
        current = match bounds {
            Some((lo, hi)) => logged(ctx, |s| integrate_definite(&current, var, lo, hi, s))?,
            None => {
                indefinite = true;
                logged(ctx, |s| integrate(&current, var, s))?
            }
        };
    }
    if indefinite {
        let with_c = with_constant(ctx, &current, &constant_name(&current));
        ctx.log(
            "add the constant of integration",
            e,
            &with_c,
            Some("C is an arbitrary constant".into()),
        );
        return Ok(with_c);
    }
    note_if_numeric(ctx, e, &current, what);
    Ok(current)
}

/// `F + C`, shown with the constant last.
fn with_constant(ctx: &Ctx<'_>, antiderivative: &Expr, constant: &str) -> Expr {
    let value = simplify(&add(vec![antiderivative.clone(), Expr::sym(constant)]));
    if antiderivative.is_zero() {
        ctx.show_as(&value, constant.to_string(), constant.to_string());
    } else {
        ctx.show_with_constant(&value, antiderivative, constant);
    }
    value
}

/// `C`, or `C_1`, `C_2` … when `C` is already in use.
fn constant_name(e: &Expr) -> String {
    if !e.contains_sym("C") {
        return "C".into();
    }
    (1..)
        .map(|k| format!("C_{k}"))
        .find(|n| !e.contains_sym(n))
        .unwrap_or_else(|| "C".into())
}

// ---- sums and products -----------------------------------------------------

fn small_concrete_range(lower: &Expr, upper: &Expr) -> bool {
    let int = |e: &Expr| e.as_number().and_then(|n| n.as_i64());
    match (int(lower), int(upper)) {
        (Some(lo), Some(hi)) => hi.saturating_sub(lo) < MAX_EXACT_TERMS,
        _ => false,
    }
}

fn big_op(
    kind: BigOpKind,
    term: &Expr,
    var: &Name,
    lower: &Expr,
    upper: &Expr,
    ctx: &Ctx<'_>,
) -> Option<MathResult<Expr>> {
    // Short concrete ranges are added term by term by the built-in evaluator.
    if small_concrete_range(lower, upper) && !ctx.logging() {
        return None;
    }
    let shown = Expr::new(ExprNode::BigOp {
        kind,
        expr: term.clone(),
        var: var.clone(),
        lower: lower.clone(),
        upper: upper.clone(),
    });
    let result = match kind {
        BigOpKind::Sum => logged(ctx, |s| sum(term, var, lower, upper, s)),
        BigOpKind::Product => logged(ctx, |s| product(term, var, lower, upper, s)),
    };
    let what = match kind {
        BigOpKind::Sum => "this sum",
        BigOpKind::Product => "this product",
    };
    Some(match result {
        Ok(value) => {
            note_if_numeric(ctx, &shown, &value, what);
            Ok(value)
        }
        // A finite sum the core cannot close stays as written when its range is symbolic.
        Err(MathError::NoClosedForm(_) | MathError::Unsupported(_)) if !upper.is_infinite() => {
            if small_concrete_range(lower, upper) {
                return None;
            }
            ctx.note(format!(
                "no closed form was found for {what}, so it is kept as written"
            ));
            Ok(shown)
        }
        Err(MathError::Divergent(_)) if upper.is_infinite() && kind == BigOpKind::Sum => {
            Err(divergent_series(term, var, lower, &shown))
        }
        Err(err) => Err(err),
    })
}

/// "by the p-series test, p = 1 ≤ 1, so the series … diverges".
fn divergent_series(term: &Expr, var: &str, lower: &Expr, shown: &Expr) -> MathError {
    let report = convergence(term, var, lower);
    if report.converges != Some(false) {
        return MathError::Divergent(format!("the series {shown}"));
    }
    let test = if report.test.contains("test") {
        report.test
    } else {
        format!("{} test", report.test)
    };
    MathError::Divergent(format!(
        "by the {test}, {}, so the series {shown}",
        report.detail
    ))
}

// ---- series ----------------------------------------------------------------

/// The variable and centre of `x = a` (or a bare `x`, about 0).
fn series_point(sub: &Expr) -> Option<(String, Expr)> {
    match sub.node() {
        ExprNode::Relation(RelOp::Eq, v, at) => Some((v.as_sym()?.to_string(), at.clone())),
        ExprNode::Sym(v) => Some((v.to_string(), Expr::zero())),
        _ => None,
    }
}

/// `operatorname(name, sub, sup, (args…))` for the operators calculus knows.
fn operatorname(args: &[Expr], ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
    let [which, sub, sup, body] = args else {
        return None;
    };
    let ExprNode::Tuple(items) = body.node() else {
        return None;
    };
    if !matches!(which.as_sym(), Some("series" | "taylor")) {
        return None;
    }
    Some(taylor(sub, sup, items, ctx))
}

fn is_empty_tuple(e: &Expr) -> bool {
    matches!(e.node(), ExprNode::Tuple(items) if items.is_empty())
}

fn taylor(sub: &Expr, sup: &Expr, items: &[Expr], ctx: &Ctx<'_>) -> MathResult<Expr> {
    let [f] = items else {
        return Err(MathError::WrongArity {
            function: "series".into(),
            expected: "1".into(),
            got: items.len(),
        });
    };
    let (var, at) = if is_empty_tuple(sub) {
        let free = f.free_symbols();
        match free.iter().collect::<Vec<_>>().as_slice() {
            [v] => (v.to_string(), Expr::zero()),
            _ => {
                return Err(MathError::Undefined(
                    "the expansion variable (write it below, as in series_{x=0})".into(),
                ))
            }
        }
    } else {
        series_point(sub)
            .ok_or_else(|| MathError::Undefined("the expansion point (write it as x = a)".into()))?
    };
    let order = if is_empty_tuple(sup) {
        DEFAULT_SERIES_ORDER
    } else {
        sup.as_number()
            .and_then(|n| n.as_i64())
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| MathError::Domain {
                function: "series".into(),
                detail: format!("the order must be a whole number, not {sup}"),
            })?
    };
    let s = logged(ctx, |log| series(f, &var, &at, order, log))?;
    let value = simplify(&add(vec![s.terms.clone(), s.remainder.clone()]));
    ctx.show_as(&value, s.to_string(), s.to_latex());
    Ok(value)
}

// ---- ∇ ---------------------------------------------------------------------

/// The components of a vector: a tuple or a one-row/one-column matrix.
fn components(e: &Expr) -> Option<Vec<Expr>> {
    match e.node() {
        ExprNode::Tuple(items) => Some(items.clone()),
        ExprNode::Matrix { rows, cols, data } if *rows == 1 || *cols == 1 => Some(data.clone()),
        _ => None,
    }
}

/// The variables to differentiate a scalar by: `x, y, z` if it uses any of them,
/// otherwise every free name in alphabetical order.
fn infer_vars(e: &Expr) -> Vec<String> {
    let free = e.free_symbols();
    let xyz: Vec<String> = ["x", "y", "z"]
        .iter()
        .filter(|v| free.iter().any(|f| &**f == **v))
        .map(|v| v.to_string())
        .collect();
    if !xyz.is_empty() {
        return xyz;
    }
    free.iter().map(|f| f.to_string()).collect()
}

/// `x, y, z` (or the free names) for a vector field of `n` components.
fn field_vars(fields: &[Expr], n: usize) -> MathResult<Vec<String>> {
    if n <= 3 {
        return Ok(["x", "y", "z"][..n].iter().map(|v| v.to_string()).collect());
    }
    let free: Vec<String> = fields
        .iter()
        .flat_map(|f| f.free_symbols())
        .map(|f| f.to_string())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    if free.len() == n {
        Ok(free)
    } else {
        Err(MathError::Undefined(format!(
            "the variables of this {n}-component field (define it as a function F(…) := (…) to name them)"
        )))
    }
}

fn vector_op(name: &str, args: &[Expr], ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
    let (target, given) = match args {
        [t] => (t, None),
        [t, vars] => {
            let ExprNode::Tuple(items) = vars.node() else {
                return None;
            };
            let names: Option<Vec<String>> = items
                .iter()
                .map(|v| v.as_sym().map(str::to_string))
                .collect();
            (t, Some(names?))
        }
        _ => return None,
    };
    // `∇f` for a name that is not a function stays as written.
    if given.is_none() && target.as_sym().is_some() {
        return None;
    }
    let result = match name {
        "grad" | "laplacian" | "hessian" => {
            if components(target).is_some() {
                return Some(Err(MathError::NotANumber(format!(
                    "∇ of the vector {target} (use ∇· or ∇× for a vector field)"
                ))));
            }
            let vars = given.unwrap_or_else(|| infer_vars(target));
            match name {
                "grad" => gradient(target, &vars, ctx),
                "laplacian" => laplacian(target, &vars, ctx),
                _ => hessian(target, &vars, ctx),
            }
        }
        _ => {
            let Some(fields) = components(target) else {
                return Some(Err(MathError::NotANumber(format!(
                    "{target} as a vector field (write it as (P, Q, R))"
                ))));
            };
            let vars = match given {
                Some(v) => Ok(v),
                None => field_vars(&fields, fields.len()),
            };
            vars.and_then(|vars| match name {
                "div" => divergence(&fields, &vars, ctx),
                "curl" => curl(&fields, &vars, ctx),
                _ => jacobian(&fields, &vars, ctx),
            })
        }
    };
    let e = Expr::apply(name, args.to_vec());
    if let Ok(v) = &result {
        ctx.log(name_of(name), &e, v, None);
    }
    Some(result)
}

fn name_of(op: &str) -> &'static str {
    match op {
        "grad" => "gradient",
        "div" => "divergence",
        "curl" => "curl",
        "laplacian" => "Laplacian",
        "hessian" => "Hessian matrix",
        _ => "Jacobian matrix",
    }
}

fn partial(e: &Expr, var: &str, ctx: &Ctx<'_>) -> MathResult<Expr> {
    logged(ctx, |s| diff(e, var, 1, s))
}

fn gradient(f: &Expr, vars: &[String], ctx: &Ctx<'_>) -> MathResult<Expr> {
    if vars.is_empty() {
        return Ok(Expr::zero());
    }
    let parts = vars
        .iter()
        .map(|v| partial(f, v, ctx))
        .collect::<MathResult<Vec<_>>>()?;
    Ok(Expr::new(ExprNode::Tuple(parts)))
}

fn laplacian(f: &Expr, vars: &[String], ctx: &Ctx<'_>) -> MathResult<Expr> {
    let parts = vars
        .iter()
        .map(|v| logged(ctx, |s| diff(f, v, 2, s)))
        .collect::<MathResult<Vec<_>>>()?;
    Ok(add(parts))
}

fn hessian(f: &Expr, vars: &[String], ctx: &Ctx<'_>) -> MathResult<Expr> {
    let firsts = vars
        .iter()
        .map(|v| partial(f, v, ctx))
        .collect::<MathResult<Vec<_>>>()?;
    jacobian(&firsts, vars, ctx)
}

fn jacobian(fields: &[Expr], vars: &[String], ctx: &Ctx<'_>) -> MathResult<Expr> {
    let mut rows = Vec::with_capacity(fields.len());
    for f in fields {
        rows.push(
            vars.iter()
                .map(|v| partial(f, v, ctx))
                .collect::<MathResult<Vec<_>>>()?,
        );
    }
    Expr::matrix(rows)
        .ok_or_else(|| MathError::NotANumber("a Jacobian with no functions or variables".into()))
}

fn arity(what: &str, expected: &str, got: usize) -> MathError {
    MathError::WrongArity {
        function: what.into(),
        expected: expected.into(),
        got,
    }
}

fn divergence(fields: &[Expr], vars: &[String], ctx: &Ctx<'_>) -> MathResult<Expr> {
    if fields.len() != vars.len() {
        return Err(arity(
            "the divergence",
            &vars.len().to_string(),
            fields.len(),
        ));
    }
    let parts = fields
        .iter()
        .zip(vars)
        .map(|(f, v)| partial(f, v, ctx))
        .collect::<MathResult<Vec<_>>>()?;
    Ok(add(parts))
}

fn curl(fields: &[Expr], vars: &[String], ctx: &Ctx<'_>) -> MathResult<Expr> {
    let d = |i: usize, j: usize| partial(&fields[i], &vars[j], ctx);
    let minus = |a: Expr, b: Expr| add(vec![a, mul(vec![Expr::int(-1), b])]);
    match (fields.len(), vars.len()) {
        (3, 3) => Ok(Expr::new(ExprNode::Tuple(vec![
            minus(d(2, 1)?, d(1, 2)?),
            minus(d(0, 2)?, d(2, 0)?),
            minus(d(1, 0)?, d(0, 1)?),
        ]))),
        // The scalar curl ∂Q/∂x − ∂P/∂y of a plane field.
        (2, 2) => Ok(minus(d(1, 0)?, d(0, 1)?)),
        (n, _) => Err(arity("the curl", "2 or 3", n)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::holders::{evaluate_holder, Holders};

    fn x() -> Expr {
        Expr::sym("x")
    }
    fn y() -> Expr {
        Expr::sym("y")
    }

    fn eval(e: &Expr) -> MathResult<Expr> {
        let holders = Holders::new();
        let ctx = Ctx::new(&holders);
        evaluate_holder(e, &ctx)
    }

    #[test]
    fn derivative_and_integral_holders() {
        let d = Expr::new(ExprNode::Derivative {
            expr: x().pow(3),
            vars: vec![("x".into(), 2)],
        });
        assert_eq!(eval(&d).unwrap(), Expr::int(6) * x());
        let i = Expr::new(ExprNode::Integral {
            expr: x().pow(2),
            var: "x".into(),
            bounds: Some((Expr::zero(), Expr::int(3))),
        });
        assert_eq!(eval(&i).unwrap(), Expr::int(9));
    }

    #[test]
    fn vector_operators() {
        let g = Expr::apply("grad", vec![x().pow(2) * y()]);
        assert_eq!(eval(&g).unwrap().to_string(), "(2*x*y, x^2)");
        let field = Expr::new(ExprNode::Tuple(vec![y(), -x(), Expr::zero()]));
        assert_eq!(
            eval(&Expr::apply("curl", vec![field.clone()]))
                .unwrap()
                .to_string(),
            "(0, 0, -2)"
        );
        assert_eq!(
            eval(&Expr::apply("div", vec![field])).unwrap(),
            Expr::zero()
        );
        // A bare unknown name stays as written.
        let f = Expr::apply("grad", vec![Expr::sym("f")]);
        assert_eq!(eval(&f).unwrap(), f);
    }

    #[test]
    fn bound_names_of_holders() {
        let vars = Expr::new(ExprNode::Tuple(vec![x(), y()]));
        assert_eq!(bound_names("grad", &[x(), vars]).len(), 2);
        let sub = Expr::equation(x(), Expr::zero());
        let empty = Expr::new(ExprNode::Tuple(Vec::new()));
        assert_eq!(
            bound_names(
                "operatorname",
                &[Expr::sym("series"), sub, empty.clone(), empty]
            ),
            vec![Name::from("x")]
        );
        assert!(bound_names("expand", &[x()]).is_empty());
    }
}
