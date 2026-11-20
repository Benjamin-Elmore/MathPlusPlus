//! The maths helpers as holders: function analysis (`\operatorname{crit} f = ?`,
//! "Find the critical points of $f$ on $[0, 4]$.") answered by
//! [`mpp_core::tools::analysis`], and the multivariable and vector calculus helpers
//! (see [`multivar`]) answered by [`mpp_core::tools::multivar`].
//!
//! Both forms meet here as `Apply` holders: `crit(f)` (or `crit(f, [0, 4])`) for an
//! operator without scripts, and `operatorname(crit, sub, sup, (f, …))` with them. The
//! subscript is an interval (`_{[0,4]}`) or a point (`_{x=1}`); for the intermediate value
//! theorem the superscript is the value (`\operatorname{ivt}_{[0,1]}^{0} f`). Helper
//! sentences are turned into the same holder by [`sentence_holder`].

use mpp_core::tools::analysis::{interval_of, variable_of, Analysis};
use mpp_core::{Expr, ExprNode, MathError, MathResult, Name, RelOp};
use mpp_syntax::document::HelperName;

use crate::holders::Ctx;

mod multivar;

/// What a helper computes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Domain,
    Range,
    Intercepts,
    Critical,
    Extrema,
    LocalExtrema,
    AbsoluteExtrema,
    Monotonicity,
    Concavity,
    Inflection,
    Asymptotes,
    Tangent,
    Normal,
    Linearisation,
    MeanValue,
    IntermediateValue,
    Analyse,
}

/// Operator names, as written after `\operatorname`, and what they compute.
const OPS: &[(&str, Op)] = &[
    ("domain", Op::Domain),
    ("range", Op::Range),
    ("intercepts", Op::Intercepts),
    ("crit", Op::Critical),
    ("critical", Op::Critical),
    ("extrema", Op::Extrema),
    ("localextrema", Op::LocalExtrema),
    ("absextrema", Op::AbsoluteExtrema),
    ("increasing", Op::Monotonicity),
    ("decreasing", Op::Monotonicity),
    ("monotonicity", Op::Monotonicity),
    ("concavity", Op::Concavity),
    ("inflection", Op::Inflection),
    ("asymptotes", Op::Asymptotes),
    ("tangent", Op::Tangent),
    ("normal", Op::Normal),
    ("linearisation", Op::Linearisation),
    ("linearization", Op::Linearisation),
    ("linearise", Op::Linearisation),
    ("mvt", Op::MeanValue),
    ("ivt", Op::IntermediateValue),
    ("analyse", Op::Analyse),
    ("analyze", Op::Analyse),
];

fn op_named(name: &str) -> Option<Op> {
    OPS.iter().find(|(n, _)| *n == name).map(|(_, op)| *op)
}

/// True when `name` is a helper operator.
pub fn is_helper(name: &str) -> bool {
    op_named(name).is_some()
}

/// The operator name a helper sentence stands for.
pub fn sentence_op(h: HelperName) -> &'static str {
    match h {
        HelperName::Domain => "domain",
        HelperName::Range => "range",
        HelperName::Intercepts => "intercepts",
        HelperName::CriticalPoints => "crit",
        HelperName::Extrema => "extrema",
        HelperName::LocalExtrema => "localextrema",
        HelperName::AbsoluteExtrema => "absextrema",
        HelperName::IntervalsOfIncrease => "increasing",
        HelperName::Concavity => "concavity",
        HelperName::InflectionPoints => "inflection",
        HelperName::Asymptotes => "asymptotes",
        HelperName::TangentLine => "tangent",
        HelperName::NormalLine => "normal",
        HelperName::Linearisation => "linearisation",
    }
}

/// `where defined(f, d₁, d₂, …)`: the function a helper analyses with the denominators it
/// was written with, which simplification may have cancelled (see
/// [`crate::lower::Lowerer::lower_helper_target`]).
pub const WHERE_DEFINED: &str = "where defined";

/// Wraps `f` with its denominators (just `f` when there are none).
pub fn where_defined(f: Expr, denominators: Vec<Expr>) -> Expr {
    if denominators.is_empty() {
        return f;
    }
    let mut args = vec![f];
    args.extend(denominators);
    Expr::apply(WHERE_DEFINED, args)
}

/// `(f, denominators)` from a [`where_defined`] wrapper (no denominators otherwise).
pub fn unwrap_where_defined(e: &Expr) -> (Expr, Vec<Expr>) {
    match e.node() {
        ExprNode::Apply(name, args) if &**name == WHERE_DEFINED && !args.is_empty() => {
            (args[0].clone(), args[1..].to_vec())
        }
        _ => (e.clone(), Vec::new()),
    }
}

/// The environment key under which a user function's denominators are kept.
pub fn denominators_key(function: &str) -> String {
    format!("denominators:{function}")
}

fn empty() -> Expr {
    Expr::new(ExprNode::Tuple(Vec::new()))
}

fn is_empty(e: &Expr) -> bool {
    matches!(e.node(), ExprNode::Tuple(items) if items.is_empty())
}

/// The holder a helper sentence means: `operatorname(op, sub, (), (target))`, with the
/// interval or the `at` point as the subscript.
pub fn sentence_holder(op: &str, target: Expr, interval: Option<Expr>, at: Option<Expr>) -> Expr {
    let sub = at.or(interval).unwrap_or_else(empty);
    Expr::apply(
        "operatorname",
        vec![
            Expr::sym(op),
            sub,
            empty(),
            Expr::new(ExprNode::Tuple(vec![target])),
        ],
    )
}

/// Names a helper binds: the variable of `f(x)` and of a point `x = a`, which must stay
/// symbolic while the arguments are resolved.
pub fn bound_names(name: &str, args: &[Expr]) -> Vec<Name> {
    let several = multivar::bound_names(name, args);
    if !several.is_empty() {
        return several;
    }
    let (sub, items): (Option<&Expr>, &[Expr]) = match (name, args) {
        ("operatorname", [which, sub, _, body]) if which.as_sym().is_some_and(is_helper) => {
            match body.node() {
                ExprNode::Tuple(items) => (Some(sub), items.as_slice()),
                _ => return Vec::new(),
            }
        }
        (n, items) if is_helper(n) => (None, items),
        _ => return Vec::new(),
    };
    let mut out: Vec<Name> = Vec::new();
    if let Some(ExprNode::Relation(RelOp::Eq, v, _)) = sub.map(Expr::node) {
        if let Some(s) = v.as_sym() {
            out.push(Name::from(s));
        }
    }
    if let Some(ExprNode::Apply(_, call_args)) = items.first().map(Expr::node) {
        for a in call_args {
            if let Some(s) = a.as_sym() {
                if !out.iter().any(|o| &**o == s) {
                    out.push(Name::from(s));
                }
            }
        }
    }
    out
}

/// Evaluates a helper holder; `None` for other nodes.
pub fn evaluate(e: &Expr, ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
    if let Some(r) = multivar::evaluate(e, ctx) {
        return Some(r);
    }
    let ExprNode::Apply(name, args) = e.node() else {
        return None;
    };
    if let Some(op) = op_named(name) {
        // `crit(f)`, `extrema(f, [0, 4])`, `tangent(f, 1)`, `ivt(f, [0, 1], 0)`.
        let (f, rest) = args.split_first()?;
        let sub = rest.first().cloned().unwrap_or_else(empty);
        let sup = rest.get(1).cloned().unwrap_or_else(empty);
        return Some(run(op, f, &sub, &sup, ctx));
    }
    if &**name != "operatorname" {
        return None;
    }
    let [which, sub, sup, body] = args.as_slice() else {
        return None;
    };
    let op = op_named(which.as_sym()?)?;
    let ExprNode::Tuple(items) = body.node() else {
        return None;
    };
    let result = match items.as_slice() {
        [f] => run(op, f, sub, sup, ctx),
        _ => Err(MathError::WrongArity {
            function: which.to_string(),
            expected: "1".into(),
            got: items.len(),
        }),
    };
    Some(result)
}

/// The subscript read as a point `x = a` (the variable and the value).
fn point_of(sub: &Expr) -> Option<(String, Expr)> {
    match sub.node() {
        ExprNode::Relation(RelOp::Eq, v, a) => Some((v.as_sym()?.to_string(), a.clone())),
        _ => None,
    }
}

fn needs_point(op: Op) -> bool {
    matches!(op, Op::Tangent | Op::Normal | Op::Linearisation)
}

fn needs_interval(op: Op) -> bool {
    matches!(op, Op::MeanValue | Op::IntermediateValue)
}

fn run(op: Op, f: &Expr, sub: &Expr, sup: &Expr, ctx: &Ctx<'_>) -> MathResult<Expr> {
    let (f, denominators) = unwrap_where_defined(f);
    let f = &f;
    let point = point_of(sub);
    let at =
        match (&point, needs_point(op)) {
            (Some((_, a)), true) => Some(a.clone()),
            (None, true) if !is_empty(sub) && sub.is_constant() => Some(sub.clone()),
            (None, true) => return Err(MathError::Undefined(
                "the point (write it as a subscript, as in tangent_{x=1}, or say \"at $x = 1$\")"
                    .into(),
            )),
            _ => None,
        };
    let interval = if is_empty(sub) || needs_point(op) || point.is_some() {
        None
    } else {
        Some(interval_of(sub)?)
    };
    if needs_interval(op) && interval.is_none() {
        return Err(MathError::Undefined(
            "the interval (write it as a subscript, as in mvt_{[0, 2]}, or say \"on $[0, 2]$\")"
                .into(),
        ));
    }
    let var = variable_of(f, point.as_ref().map(|(v, _)| v.as_str()))?;
    let theorem = needs_interval(op);
    let mut a = Analysis::new(f, &var, if theorem { None } else { interval.clone() })?
        .with_excluded(denominators);
    if ctx.logging() {
        a = a.with_steps();
    }
    let result = match op {
        Op::Domain => a.domain().map(|d| d.to_expr()),
        Op::Range => a.range().map(|r| r.to_expr()),
        Op::Intercepts => a.intercepts_table(),
        Op::Critical => a.critical_set(),
        Op::Extrema => {
            let with_absolute = interval.is_some();
            a.extrema_table(true, with_absolute)
        }
        Op::LocalExtrema => a.extrema_table(true, false),
        Op::AbsoluteExtrema => a.extrema_table(false, true),
        Op::Monotonicity => a.monotonicity_table(),
        Op::Concavity => a.concavity_table(),
        Op::Inflection => a.inflection_points().map(|p| Expr::new(ExprNode::Set(p))),
        Op::Asymptotes => a.asymptotes_table(),
        Op::Tangent => a.tangent(at.as_ref().unwrap_or(&Expr::zero())),
        Op::Normal => a.normal(at.as_ref().unwrap_or(&Expr::zero())),
        Op::Linearisation => a.linearisation(at.as_ref().unwrap_or(&Expr::zero())),
        Op::MeanValue => match &interval {
            Some(i) => a.mean_value(i),
            None => Err(MathError::Undefined("the interval".into())),
        },
        Op::IntermediateValue => {
            let target = if is_empty(sup) {
                Expr::zero()
            } else {
                sup.clone()
            };
            match &interval {
                Some(i) => a.intermediate_value(i, &target),
                None => Err(MathError::Undefined("the interval".into())),
            }
        }
        Op::Analyse => a.summary(),
    };
    for n in &a.notes {
        ctx.note(n.clone());
    }
    if let Some(steps) = a.steps.take() {
        ctx.append(steps);
    }
    result
}
