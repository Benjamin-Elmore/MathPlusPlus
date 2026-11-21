//! One front door for operator names that more than one toolkit answers, and for the
//! names the IDE's Insert menu writes.
//!
//! Several evaluators grew operators with the same name: `dsolve` and `rk4` exist in the
//! domain registry (`dsolve(eq, y, x, (x₀, y₀))`, `rk4(f, x, y, x₀, y₀, x₁, n)`) and in the
//! ODE toolkit (`dsolve(eq, conditions…)`, `rk4(f, x₀, y₀, h, n)`); `laplace` exists in the
//! multivariable helpers and in the ODE toolkit. Registration order used to decide which
//! one answered, so the form the other expected failed with a wrong-arity error. This
//! holder is asked first: it reads the arguments' shape and sends the call to the
//! evaluator whose form it is.
//!
//! It also maps the helper names of the Insert menu (`linearise`, `converges`,
//! `directional`, `linearapprox`, `gauss`, `relatedrates`) onto the evaluators' own names,
//! and turns the menu's separate "from" and "to" boxes (`\operatorname{area}_{0}^{1}`) into
//! the interval the toolkits read.

use mpp_core::{Expr, ExprNode, MathError, MathResult, Name, RelOp};

use crate::holders::Ctx;

/// An operator call, in either lowered form (`name(args…)` or
/// `operatorname(name, sub, sup, (args…))`).
#[derive(Clone, Debug)]
struct Call {
    name: String,
    sub: Expr,
    sup: Expr,
    args: Vec<Expr>,
    scripted: bool,
}

fn empty() -> Expr {
    Expr::new(ExprNode::Tuple(Vec::new()))
}

fn is_empty(e: &Expr) -> bool {
    matches!(e.node(), ExprNode::Tuple(items) if items.is_empty())
}

impl Call {
    fn read(e: &Expr) -> Option<Call> {
        let ExprNode::Apply(name, args) = e.node() else {
            return None;
        };
        if &**name == "operatorname" {
            let [which, sub, sup, body] = args.as_slice() else {
                return None;
            };
            let ExprNode::Tuple(items) = body.node() else {
                return None;
            };
            return Some(Call {
                name: which.as_sym()?.to_string(),
                sub: sub.clone(),
                sup: sup.clone(),
                args: items.clone(),
                scripted: true,
            });
        }
        Some(Call {
            name: name.to_string(),
            sub: empty(),
            sup: empty(),
            args: args.clone(),
            scripted: false,
        })
    }

    fn has_scripts(&self) -> bool {
        !is_empty(&self.sub) || !is_empty(&self.sup)
    }

    /// The call written back as a holder (scripted form when it has scripts).
    fn to_expr(&self) -> Expr {
        if !self.scripted && !self.has_scripts() {
            return Expr::apply(&self.name, self.args.clone());
        }
        Expr::apply(
            "operatorname",
            vec![
                Expr::sym(&self.name),
                self.sub.clone(),
                self.sup.clone(),
                Expr::new(ExprNode::Tuple(self.args.clone())),
            ],
        )
    }

    fn renamed(&self, name: &str) -> Call {
        Call {
            name: name.to_string(),
            ..self.clone()
        }
    }
}

/// Which evaluator a shared name goes to.
enum Route {
    Registry,
    OdeToolkit,
}

/// Insert-menu helper names and the evaluator names they stand for.
const ALIASES: &[(&str, &str)] = &[
    ("converges", "tests"),
    ("convergence", "tests"),
    ("directional", "dirderiv"),
    ("linearapprox", "linapprox"),
    ("gauss", "divthm"),
    ("divergencetheorem", "divthm"),
    ("relatedrates", "rates"),
    ("inflections", "inflection"),
    ("eig", "eigenvals"),
    ("eigenvalues", "eigenvals"),
    ("eigenvectors", "eigenvects"),
];

/// Helpers whose Insert-menu template has a "from" box below the name and a "to" box
/// above it; the toolkit reads them as one interval argument.
const FROM_TO: &[&str] = &[
    "area",
    "volume",
    "arclength",
    "surfacearea",
    "average",
    "work",
    "centroid",
    "riemann",
    "midpoint",
    "trapezoid",
    "simpson",
];

/// The evaluator names this module routes, after aliases.
fn routed(name: &str) -> bool {
    matches!(
        name,
        "dsolve"
            | "rk4"
            | "rk4table"
            | "laplace"
            | "invlaplace"
            | "fourier"
            | "invfourier"
            | "euler"
            | "tangentplane"
            | "solve"
    ) || ALIASES.iter().any(|(a, _)| *a == name)
        || FROM_TO.contains(&name)
}

/// Asked before every other holder: routes shared operator names by argument shape.
pub fn evaluate(e: &Expr, ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
    if let Some(v) = step_or_delta(e) {
        return Some(Ok(v));
    }
    let call = Call::read(e)?;
    if !routed(&call.name) {
        return None;
    }
    if let Some((_, to)) = ALIASES.iter().find(|(a, _)| *a == call.name) {
        let renamed = adapt_alias(&call.renamed(to));
        return Some(crate::holders::evaluate_holder(&renamed.to_expr(), ctx));
    }
    if call.name == "solve" {
        // `\operatorname{solve}_{y(x)} (y'' + y = 0)` is the ODE toolkit's; any other
        // `solve` is the algebraic solver's.
        let ode = call
            .args
            .first()
            .is_some_and(crate::helpers_b::is_differential_equation_or_system);
        return if ode {
            crate::helpers_b::evaluate(e, ctx)
        } else {
            None
        };
    }
    if call.name == "tangentplane" {
        // `tangentplane_{(1, 2)} f`: the menu writes the point without its variables.
        if is_empty(&call.sub) || is_relation(&call.sub) {
            return None;
        }
        return crate::helpers::evaluate(&adapt_alias(&call).to_expr(), ctx);
    }
    if FROM_TO.contains(&call.name.as_str()) {
        let adapted = from_to(&call)?;
        return crate::helpers_b::evaluate(&adapted.to_expr(), ctx);
    }
    if call.name == "dsolve" {
        let answer = dsolve_route(&call, e, ctx)?;
        // The units solver (asked first) could not give every quantity a unit; an
        // answer with a unit inside an exponential or a function is meaningless.
        return Some(answer.and_then(|v| {
            if unit_inside_function(&v, false) {
                Err(partial_units())
            } else {
                Ok(v)
            }
        }));
    }
    let route = match call.name.as_str() {
        "rk4" | "rk4table" => {
            if registry_rk4(&call) || call.name == "rk4table" {
                Route::Registry
            } else {
                Route::OdeToolkit
            }
        }
        "euler" => Route::OdeToolkit,
        "laplace" | "invlaplace" | "fourier" | "invfourier" => {
            return Some(transform(&call, e, ctx));
        }
        _ => return None,
    };
    let expr = adapt_initial_condition(&call).to_expr();
    match route {
        Route::Registry => crate::domains_wire::evaluate(&expr, ctx),
        Route::OdeToolkit => crate::helpers_b::evaluate(&expr, ctx),
    }
}

/// `dsolve` in either form. `dsolve(eq, y, t, …)` goes to the ODE toolkit too, which
/// reads `y(0) = 0` or `(0, 0)` as the start, gives `y = …` when it can and keeps the
/// variable as named; the registry's solver is its fallback.
fn dsolve_route(call: &Call, e: &Expr, ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
    if !registry_dsolve(call) {
        return crate::helpers_b::evaluate(e, ctx);
    }
    match crate::helpers_b::evaluate(e, ctx) {
        Some(Ok(v)) => Some(Ok(v)),
        _ => crate::domains_wire::evaluate(e, ctx),
    }
}

/// True when a unit sits inside a function or an exponent (`e^{-t/(0.47 s)}`).
pub(crate) fn unit_inside_function(e: &Expr, inside: bool) -> bool {
    match e.node() {
        ExprNode::Apply(name, _) if name.starts_with(crate::units_wire::UNIT_PREFIX) => inside,
        ExprNode::Func(_, args) => args.iter().any(|a| unit_inside_function(a, true)),
        ExprNode::Pow(base, exp) => {
            unit_inside_function(base, inside) || unit_inside_function(exp, true)
        }
        _ => e
            .children()
            .into_iter()
            .any(|c| unit_inside_function(c, inside)),
    }
}

pub(crate) fn partial_units() -> MathError {
    MathError::Dimension(
        "this differential equation could not be solved with its units (no consistent unit was found for every quantity); give each quantity its unit (`V(0) = 0\\,\\mathrm{V}`, `12\\,\\mathrm{V}`) or write all the numbers without units".into(),
    )
}

/// The variables of a transform: the subscript `t \to s` when written, otherwise `t → s`
/// for Laplace and `x → ω` for Fourier (the inverse swaps them). A function of one other
/// variable than the default keeps its own (`\mathcal L\{e^{-2u}\}` is in `u`).
fn transform_vars(call: &Call) -> (String, String) {
    if let ExprNode::Apply(name, ab) = call.sub.node() {
        if let ([a, b], "to") = (ab.as_slice(), &**name) {
            if let (Some(a), Some(b)) = (a.as_sym(), b.as_sym()) {
                return (a.to_string(), b.to_string());
            }
        }
    }
    let (from, to) = match call.name.as_str() {
        "laplace" => ("t", "s"),
        "invlaplace" => ("s", "t"),
        "fourier" => ("x", "ω"),
        _ => ("ω", "x"),
    };
    let free: Vec<String> = call
        .args
        .first()
        .map(|f| f.free_symbols().iter().map(|s| s.to_string()).collect())
        .unwrap_or_default();
    let from = match free.as_slice() {
        [only] if only != to && !free.iter().any(|v| v == from) => only.clone(),
        _ => from.to_string(),
    };
    (from, to.to_string())
}

/// `\mathcal L\{f\}`, `\mathcal L^{-1}\{F\}`, `\mathcal F\{f\}`, `\mathcal F^{-1}\{F\}` and the
/// operator forms `laplace`, `invlaplace`, `fourier`, `invfourier`.
fn transform(call: &Call, e: &Expr, ctx: &Ctx<'_>) -> MathResult<Expr> {
    use mpp_core::tools_b::fourier;
    use mpp_core::tools_b::ode::laplace as lt;
    let [f] = call.args.as_slice() else {
        return Err(MathError::WrongArity {
            function: call.name.clone(),
            expected: "1".into(),
            got: call.args.len(),
        });
    };
    let (from, to) = transform_vars(call);
    let result = match call.name.as_str() {
        "laplace" => lt::transform(f, &from, &to),
        "invlaplace" => lt::inverse(f, &from, &to),
        "fourier" => fourier::transform(f, &from, &to),
        _ => fourier::inverse(f, &from, &to),
    };
    let (rule, note) = match call.name.as_str() {
        "laplace" | "invlaplace" => (
            "Laplace transform",
            format!("L{{f}}({to}) = ∫₀^∞ f({from}) e^(−{to}{from}) d{from}"),
        ),
        _ => (
            "Fourier transform",
            format!("F{{f}}(ω) = ∫ f(x) e^(−iωx) dx over ℝ, and F⁻¹{{F}}(x) = (1/2π) ∫ F(ω) e^(iωx) dω (with {from} → {to})"),
        ),
    };
    match result {
        Ok(v) => {
            if call.name.contains("fourier") {
                ctx.note(note.clone());
            }
            ctx.log(rule, f, &v, Some(note));
            Ok(v)
        }
        // The multivariable helpers have their own Laplace table; it answers when this
        // one has no entry.
        Err(err @ (MathError::Unsupported(_) | MathError::NoClosedForm(_)))
            if call.name.contains("laplace") && from.as_str() == default_from(&call.name) =>
        {
            match crate::helpers::evaluate(e, ctx) {
                Some(Ok(v)) => Ok(v),
                _ => Err(err),
            }
        }
        Err(err) => Err(err),
    }
}

fn default_from(name: &str) -> &'static str {
    if name == "laplace" {
        "t"
    } else {
        "s"
    }
}

/// `H(c)` and `δ(c)` at a real number `c ≠ 0` (at 0 the conventions differ, so the call
/// stays as written).
fn step_or_delta(e: &Expr) -> Option<Expr> {
    let ExprNode::Apply(name, args) = e.node() else {
        return None;
    };
    let [arg] = args.as_slice() else {
        return None;
    };
    if !matches!(&**name, "H" | "delta") || !arg.is_constant() {
        return None;
    }
    let x = arg.eval_f64(&mpp_core::EmptyEnv).ok()?;
    // An exact number decides its sign exactly; anything else must be clearly away
    // from 0, so rounding (sin π ≈ 1e−16) cannot pick the wrong side.
    let decided = match arg.as_number() {
        Some(n) if n.is_exact() => x != 0.0,
        _ => x.abs() > 1e-9,
    };
    if !x.is_finite() || !decided {
        return None;
    }
    Some(match (&**name, x > 0.0) {
        ("H", true) => Expr::one(),
        _ => Expr::zero(),
    })
}

/// True when `op(args…)` (without scripts) is the domain registry's form of an ODE
/// operator: `dsolve(eq, y, x[, (x₀, y₀)])`, `rk4(f, x, y, x₀, y₀, x₁[, n])` or `rk4table`.
pub fn is_registry_ode_form(op: &str, args: &[Expr]) -> bool {
    let call = Call {
        name: op.to_string(),
        sub: empty(),
        sup: empty(),
        args: args.to_vec(),
        scripted: false,
    };
    match op {
        "dsolve" => registry_dsolve(&call),
        "rk4" => registry_rk4(&call),
        "rk4table" => true,
        _ => false,
    }
}

/// `dsolve(eq, y, x)` / `dsolve(eq, y, x, (x₀, y₀))`: the unknown and the variable are
/// bare names in the second and third places.
fn registry_dsolve(call: &Call) -> bool {
    !call.has_scripts()
        && (3..=4).contains(&call.args.len())
        && call.args[1].as_sym().is_some()
        && call.args[2].as_sym().is_some()
}

/// `rk4(f, x, y, x₀, y₀, x₁[, n])`: names in the second and third places.
fn registry_rk4(call: &Call) -> bool {
    !call.has_scripts()
        && (6..=7).contains(&call.args.len())
        && call.args[1].as_sym().is_some()
        && call.args[2].as_sym().is_some()
}

/// The Insert-menu argument order for the aliased helpers, in the form their evaluator
/// reads.
fn adapt_alias(call: &Call) -> Call {
    match call.name.as_str() {
        // `directional(f, (1, 2), (3, 4))` → `dirderiv_{(x,y)=(1,2)}(f, (3, 4))`.
        "dirderiv" if is_empty(&call.sub) && call.args.len() == 3 => {
            let f = &call.args[0];
            let point = &call.args[1];
            let n = match point.node() {
                ExprNode::Tuple(p) => p.len(),
                _ => 1,
            };
            let vars = variables_of(f, n);
            let sub = Expr::equation(
                Expr::new(ExprNode::Tuple(vars.iter().map(|v| Expr::sym(v)).collect())),
                point.clone(),
            );
            Call {
                name: call.name.clone(),
                sub,
                sup: empty(),
                args: vec![f.clone(), call.args[2].clone()],
                scripted: true,
            }
        }
        // `linapprox_{(1, 2)} f`: a bare point below the name gets the variables.
        "linapprox" | "tangentplane" if !is_empty(&call.sub) && !is_relation(&call.sub) => {
            let n = match call.sub.node() {
                ExprNode::Tuple(p) => p.len(),
                _ => 1,
            };
            let f = call.args.first().cloned().unwrap_or_else(empty);
            let vars = variables_of(&f, n);
            Call {
                sub: Expr::equation(
                    Expr::new(ExprNode::Tuple(vars.iter().map(|v| Expr::sym(v)).collect())),
                    call.sub.clone(),
                ),
                ..call.clone()
            }
        }
        _ => call.clone(),
    }
}

fn is_relation(e: &Expr) -> bool {
    matches!(e.node(), ExprNode::Relation(..))
}

/// The variables of `f` (x, y, z first), `n` of them.
fn variables_of(f: &Expr, n: usize) -> Vec<String> {
    let free: Vec<String> = f.free_symbols().iter().map(|s| s.to_string()).collect();
    let standard = ["x", "y", "z"];
    if free.iter().all(|s| standard.contains(&s.as_str())) || free.is_empty() {
        return standard.iter().take(n).map(|s| s.to_string()).collect();
    }
    let mut out = free;
    out.sort();
    out.truncate(n);
    out
}

/// `area_{a}^{b}(f, g)` → `area(f, g, [a, b])`: separate "from" and "to" boxes become one
/// interval (a subscript that is a method word or a name stays a script).
fn from_to(call: &Call) -> Option<Call> {
    if is_empty(&call.sub) || is_empty(&call.sup) {
        return Some(call.clone());
    }
    let script_like = |e: &Expr| {
        e.as_sym().is_some_and(|s| s.chars().count() > 1)
            || matches!(e.node(), ExprNode::Interval { .. } | ExprNode::Relation(..))
    };
    if script_like(&call.sub) || script_like(&call.sup) {
        return Some(call.clone());
    }
    let mut args = call.args.clone();
    args.push(Expr::new(ExprNode::Interval {
        lo: call.sub.clone(),
        hi: call.sup.clone(),
        lo_closed: true,
        hi_closed: true,
    }));
    Some(Call {
        name: call.name.clone(),
        sub: empty(),
        sup: empty(),
        args,
        scripted: call.scripted,
    })
}

/// `euler(y' = f, y(0) = 1, h, n)`: a starting value written as a condition becomes the
/// tuple `(x₀, y₀)` the steppers read.
fn adapt_initial_condition(call: &Call) -> Call {
    if !matches!(call.name.as_str(), "euler" | "rk4") {
        return call.clone();
    }
    let args = call
        .args
        .iter()
        .map(|a| match a.node() {
            ExprNode::Relation(RelOp::Eq, l, r) => match l.node() {
                ExprNode::Apply(_, at) if at.len() == 1 && at[0].is_constant() => {
                    Expr::new(ExprNode::Tuple(vec![at[0].clone(), r.clone()]))
                }
                _ => a.clone(),
            },
            _ => a.clone(),
        })
        .collect();
    Call {
        args,
        ..call.clone()
    }
}

/// Names bound by the aliased and routed forms (the point's variables of `directional`).
pub fn bound_names(name: &str, args: &[Expr]) -> Vec<Name> {
    let Some(call) = Call::read(&Expr::new(ExprNode::Apply(Name::from(name), args.to_vec())))
    else {
        return Vec::new();
    };
    if matches!(
        call.name.as_str(),
        "laplace" | "invlaplace" | "fourier" | "invfourier"
    ) {
        // The transform's own variable stays symbolic (a document may define `t`).
        let (from, _) = transform_vars(&call);
        return vec![Name::from(from.as_str())];
    }
    match ALIASES.iter().find(|(a, _)| *a == call.name) {
        Some((_, to)) => {
            let adapted = adapt_alias(&call.renamed(to)).to_expr();
            match adapted.node() {
                ExprNode::Apply(n, a) => crate::calculus::bound_names(n, a),
                _ => Vec::new(),
            }
        }
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(name: &str, args: Vec<Expr>) -> Expr {
        Expr::apply(
            "operatorname",
            vec![
                Expr::sym(name),
                empty(),
                empty(),
                Expr::new(ExprNode::Tuple(args)),
            ],
        )
    }

    #[test]
    fn registry_shapes() {
        let y = Expr::sym("y");
        let x = Expr::sym("x");
        let eq = Expr::equation(Expr::sym("y′"), y.clone());
        let c = Call::read(&op("dsolve", vec![eq.clone(), y.clone(), x.clone()])).unwrap();
        assert!(registry_dsolve(&c));
        let c = Call::read(&op("dsolve", vec![eq.clone()])).unwrap();
        assert!(!registry_dsolve(&c));
        let pair = Expr::new(ExprNode::Tuple(vec![Expr::zero(), Expr::one()]));
        let c = Call::read(&op("dsolve", vec![eq, pair])).unwrap();
        assert!(!registry_dsolve(&c));
    }

    #[test]
    fn from_and_to_become_an_interval() {
        let e = Expr::apply(
            "operatorname",
            vec![
                Expr::sym("area"),
                Expr::zero(),
                Expr::one(),
                Expr::new(ExprNode::Tuple(vec![Expr::sym("x")])),
            ],
        );
        let c = from_to(&Call::read(&e).unwrap()).unwrap();
        assert_eq!(c.args.len(), 2);
        assert!(matches!(c.args[1].node(), ExprNode::Interval { .. }));
        // A method word below the name stays a script.
        let e = Expr::apply(
            "operatorname",
            vec![
                Expr::sym("riemann"),
                Expr::sym("left"),
                Expr::one(),
                Expr::new(ExprNode::Tuple(vec![Expr::sym("x")])),
            ],
        );
        let c = from_to(&Call::read(&e).unwrap()).unwrap();
        assert_eq!(c.args.len(), 1);
    }
}
