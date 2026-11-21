//! One front door for the operator names whose argument forms are answered by different
//! parts of Math++, and for the names the IDE's Insert menu writes.
//!
//! [`FORMS`] is the single table of those operators: each row names the operators, the
//! argument form it accepts (as a user would write it) and who answers that form. The
//! first row whose form matches the call answers it; a row that answers [`Answer::Pass`]
//! leaves the call to the holders after this one (the algebraic `solve`, for example).
//! Every maths engine exists once (one `dsolve`, one RK4 stepper, one Laplace transform);
//! the table only decides which front end reads the arguments.
//!
//! The helper names of the Insert menu (`linearise`, `converges`, `directional`,
//! `linearapprox`, `gauss`, `relatedrates`) are renamed by [`ALIASES`] onto the
//! evaluators' own names, and the menu's separate "from" and "to" boxes
//! (`\operatorname{area}_{0}^{1}`) become the interval the integral applications read.

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

/// Who answers a form in [`FORMS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    /// The domain registry's operators (`crate::domains_wire`).
    Registry,
    /// The numerical methods and the differential-equations toolkit (`crate::helpers::evaluate_operators`);
    /// a starting value written `y(x₀) = y₀` becomes the pair `(x₀, y₀)` first.
    Toolkit,
    /// The multivariable helpers (`crate::helpers`), once a bare point below the name has
    /// been given its variables.
    Multivar,
    /// The integral applications (`crate::helpers::evaluate_operators`), once separate "from" and "to"
    /// boxes have become one interval.
    FromTo,
    /// The Laplace and Fourier transforms ([`transform`]).
    Transform,
    /// Not routed here: the holders after this one answer.
    Pass,
}

/// One accepted argument form of one or more operators.
struct Form {
    /// The operator names the row covers.
    ops: &'static [&'static str],
    /// The form, as written in a document.
    accepts: &'static str,
    /// True when a call has this form.
    when: fn(&Call) -> bool,
    /// Who answers it.
    answer: Answer,
}

fn any(_: &Call) -> bool {
    true
}

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

/// Every operator routed here, the argument forms it accepts and who answers each; the
/// first matching row wins.
const FORMS: &[Form] = &[
    Form {
        ops: &["dsolve"],
        accepts: "dsolve(eq, conditions…), dsolve(eq, y, t, conditions…), dsolve((eq₁, eq₂, …), conditions…)",
        when: any,
        answer: Answer::Toolkit,
    },
    Form {
        ops: &["solve"],
        accepts: "solve_{y(x)}(y'' + y = 0), solve(differential equation or system, …)",
        when: first_is_differential_equation,
        answer: Answer::Toolkit,
    },
    Form {
        ops: &["solve"],
        accepts: "solve(equation, …) of any other kind: the algebraic solver",
        when: any,
        answer: Answer::Pass,
    },
    Form {
        ops: &["rk4"],
        accepts: "rk4(f, x, y, x₀, y₀, x₁[, n]): y(x₁) after n steps (100 by default)",
        when: named_rk4,
        answer: Answer::Registry,
    },
    Form {
        ops: &["rk4", "euler"],
        accepts: "rk4(f or y' = f, x₀, y₀ or (x₀, y₀) or y(x₀) = y₀, h, n), and the same for euler",
        when: any,
        answer: Answer::Toolkit,
    },
    Form {
        ops: &["rk4table"],
        accepts: "rk4table(f, x, y, x₀, y₀, x₁[, n]): every point",
        when: any,
        answer: Answer::Registry,
    },
    Form {
        ops: &["laplace", "invlaplace", "fourier", "invfourier"],
        accepts: r"laplace(f), \mathcal L_{t \to s}\{f\}, \mathcal L^{-1}\{F\}, and the Fourier forms",
        when: any,
        answer: Answer::Transform,
    },
    Form {
        ops: &["tangentplane"],
        accepts: "tangentplane_{(1, 2)} f: a bare point below the name",
        when: bare_point_below,
        answer: Answer::Multivar,
    },
    Form {
        ops: &["tangentplane"],
        accepts: "tangentplane_{(x, y) = (1, 2)} f and the other forms of the helpers",
        when: any,
        answer: Answer::Pass,
    },
    Form {
        ops: FROM_TO,
        accepts: "area_{a}^{b}(f, g), volume_{a}^{b}(…), …: \"from\" and \"to\" boxes, or an interval argument",
        when: any,
        answer: Answer::FromTo,
    },
];

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

/// Whether this module routes `op` (a form, an area-style helper or an alias).
pub fn knows_operator(op: &str) -> bool {
    FROM_TO.contains(&op)
        || ALIASES.iter().any(|(a, _)| *a == op)
        || FORMS.iter().any(|f| f.ops.contains(&op))
}

/// The argument forms `op` accepts here, in the order they are tried (empty for an
/// operator this module does not route).
pub fn accepted_forms(op: &str) -> Vec<&'static str> {
    FORMS
        .iter()
        .filter(|f| f.ops.contains(&op))
        .map(|f| f.accepts)
        .collect()
}

/// The row of [`FORMS`] that answers `call`.
fn form_of(call: &Call) -> Option<&'static Form> {
    FORMS
        .iter()
        .find(|f| f.ops.contains(&call.name.as_str()) && (f.when)(call))
}

fn first_is_differential_equation(call: &Call) -> bool {
    call.args
        .first()
        .is_some_and(crate::helpers::is_differential_equation_or_system)
}

fn bare_point_below(call: &Call) -> bool {
    !is_empty(&call.sub) && !is_relation(&call.sub)
}

/// Asked before every other holder: answers the operators of [`FORMS`] and [`ALIASES`].
pub fn evaluate(e: &Expr, ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
    if let Some(v) = step_or_delta(e) {
        return Some(Ok(v));
    }
    let call = Call::read(e)?;
    if let Some((_, to)) = ALIASES.iter().find(|(a, _)| *a == call.name) {
        let renamed = adapt_alias(&call.renamed(to));
        return Some(crate::holders::evaluate_holder(&renamed.to_expr(), ctx));
    }
    match form_of(&call)?.answer {
        Answer::Registry => {
            crate::domains_wire::evaluate(&adapt_initial_condition(&call).to_expr(), ctx)
        }
        Answer::Toolkit => {
            crate::helpers::evaluate_operators(&adapt_initial_condition(&call).to_expr(), ctx)
        }
        Answer::Multivar => crate::helpers::evaluate(&adapt_alias(&call).to_expr(), ctx),
        Answer::FromTo => crate::helpers::evaluate_operators(&from_to(&call)?.to_expr(), ctx),
        Answer::Transform => Some(transform(&call, ctx)),
        Answer::Pass => None,
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
fn transform(call: &Call, ctx: &Ctx<'_>) -> MathResult<Expr> {
    use mpp_core::transforms::fourier;
    use mpp_core::transforms::laplace as lt;
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
        Err(err) => Err(err),
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

/// True when `op(args…)` (without scripts) names the unknown and the variable after the
/// equation: `dsolve(eq, y, x[, (x₀, y₀)])`, `rk4(f, x, y, x₀, y₀, x₁[, n])` or `rk4table`.
pub fn is_named_ode_form(op: &str, args: &[Expr]) -> bool {
    let call = Call {
        name: op.to_string(),
        sub: empty(),
        sup: empty(),
        args: args.to_vec(),
        scripted: false,
    };
    match op {
        "dsolve" => named_dsolve(&call),
        "rk4" => named_rk4(&call),
        "rk4table" => true,
        _ => false,
    }
}

/// `dsolve(eq, y, x)` / `dsolve(eq, y, x, (x₀, y₀))`: the unknown and the variable are
/// bare names in the second and third places.
fn named_dsolve(call: &Call) -> bool {
    !call.has_scripts()
        && (3..=4).contains(&call.args.len())
        && call.args[1].as_sym().is_some()
        && call.args[2].as_sym().is_some()
}

/// `rk4(f, x, y, x₀, y₀, x₁[, n])`: names in the second and third places.
fn named_rk4(call: &Call) -> bool {
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
    fn named_shapes() {
        let y = Expr::sym("y");
        let x = Expr::sym("x");
        let eq = Expr::equation(Expr::sym("y′"), y.clone());
        let c = Call::read(&op("dsolve", vec![eq.clone(), y.clone(), x.clone()])).unwrap();
        assert!(named_dsolve(&c));
        let c = Call::read(&op("dsolve", vec![eq.clone()])).unwrap();
        assert!(!named_dsolve(&c));
        let pair = Expr::new(ExprNode::Tuple(vec![Expr::zero(), Expr::one()]));
        let c = Call::read(&op("dsolve", vec![eq, pair])).unwrap();
        assert!(!named_dsolve(&c));
    }

    #[test]
    fn every_routed_operator_documents_its_forms() {
        for form in FORMS {
            assert!(
                form.ops.iter().any(|op| form.accepts.contains(op)) || form.ops == FROM_TO,
                "{:?}: {}",
                form.ops,
                form.accepts
            );
        }
        assert_eq!(accepted_forms("rk4").len(), 2);
        assert!(accepted_forms("det").is_empty());
        // The two rk4 forms go to different front ends, by the names in places 2 and 3.
        let named = Call::read(&op(
            "rk4",
            vec![
                Expr::sym("y"),
                Expr::sym("x"),
                Expr::sym("y"),
                Expr::zero(),
                Expr::one(),
                Expr::one(),
            ],
        ))
        .unwrap();
        assert_eq!(form_of(&named).unwrap().answer, Answer::Registry);
        let stepped = Call::read(&op(
            "rk4",
            vec![
                Expr::sym("y"),
                Expr::zero(),
                Expr::one(),
                Expr::float(0.1),
                Expr::int(10),
            ],
        ))
        .unwrap();
        assert_eq!(form_of(&stepped).unwrap().answer, Answer::Toolkit);
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
