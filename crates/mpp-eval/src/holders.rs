//! Unevaluated "holder" nodes and the registry that evaluates them.
//!
//! Lowering turns calculus and operator notation into nodes the core keeps
//! unevaluated: `Derivative`, `Integral`, `BigOp` and `Limit`, plus `Apply` nodes
//! for operators the core does not know (`expand`, `det`, `grad` …). After the
//! evaluator has substituted every known name into such a node it calls
//! [`evaluate_holder`], which asks each registered [`HolderEval`] in turn. The first
//! one that recognises the node answers; when none does, the node stays as written.
//!
//! The calculus evaluator ([`crate::calculus`]) is registered by default; more can be
//! added with [`Holders::register`] (later registrations are asked first).
//!
//! Conventions for `Apply` holders made by lowering:
//! - `grad(f)`, `div(F)`, `curl(F)`, `laplacian(f)` for the ∇ forms;
//! - `name(args…)` for `\operatorname{name} args` without scripts, and
//!   `operatorname(name, sub, sup, (args…))` with scripts (an empty tuple for a missing one);
//! - `subs(inner, t = value)`: evaluate `inner`, then put `value` for `t` (used for `f'(2)`);
//! - `det(M)` for `vmatrix`, `norm(v)`, `double_factorial(n)`, and `union`, `intersect`,
//!   `setminus`, `compose`, `tensor`, `directsum`, `in`, `subset` … for the set and
//!   relation operators the core does not have.

use std::cell::RefCell;
use std::fmt;
use std::sync::Arc;

use mpp_core::domains::Assumptions;
use mpp_core::{add, mul, BigOpKind, Expr, ExprNode, MathError, MathResult, RelOp, Steps};

/// Most terms a finite sum or product is expanded into exactly.
pub const MAX_EXACT_TERMS: i64 = 10_000;

/// What a holder evaluator can see while it works.
pub struct Ctx<'a> {
    /// The registry, so an evaluator can evaluate holders nested in its result.
    pub holders: &'a Holders,
    /// True when the statement asks for a number (`≈ ?`).
    pub numeric: bool,
    steps: Option<RefCell<Steps>>,
    notes: RefCell<Vec<String>>,
    shown: RefCell<Vec<Shown>>,
    assumptions: Option<Arc<Assumptions>>,
}

/// How to show a value whose canonical form reads badly (a Taylor series about
/// `x = 1` is clearer as powers of `x − 1` than expanded).
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    pub value: Expr,
    pub text: String,
    pub latex: String,
    /// The Unicode reading of `text`, when it differs (`x²/2 + C` for `x^2/2 + C`).
    pub unicode: Option<String>,
}

/// What a context collected while a statement was evaluated.
#[derive(Debug, Clone, Default)]
pub struct Collected {
    /// The working, if logging was on.
    pub steps: Option<Steps>,
    /// Remarks for the reader.
    pub notes: Vec<String>,
    /// Preferred ways to show values.
    pub shown: Vec<Shown>,
}

impl<'a> Ctx<'a> {
    /// A context over `holders` without a step log.
    pub fn new(holders: &'a Holders) -> Self {
        Ctx {
            holders,
            numeric: false,
            steps: None,
            notes: RefCell::new(Vec::new()),
            shown: RefCell::new(Vec::new()),
            assumptions: None,
        }
    }

    /// Facts about names in force for this statement (`Assume $x > 0$.`).
    pub fn with_assumptions(mut self, assumptions: Arc<Assumptions>) -> Self {
        if !assumptions.is_empty() {
            self.assumptions = Some(assumptions);
        }
        self
    }

    /// The facts in force (none when nothing is assumed).
    pub fn assumptions(&self) -> Option<&Assumptions> {
        self.assumptions.as_deref()
    }

    /// Turns the step log on.
    pub fn with_steps(mut self) -> Self {
        self.steps = Some(RefCell::new(Steps::new()));
        self
    }

    /// Marks the context as numeric.
    pub fn numeric(mut self, numeric: bool) -> Self {
        self.numeric = numeric;
        self
    }

    /// True when steps are being recorded.
    pub fn logging(&self) -> bool {
        self.steps.is_some()
    }

    /// Records a step when the log is on.
    pub fn log(&self, rule: &str, before: &Expr, after: &Expr, note: Option<String>) {
        if let Some(steps) = &self.steps {
            steps
                .borrow_mut()
                .push(rule, before.clone(), after.clone(), note);
        }
    }

    /// Appends steps recorded elsewhere (by the core) when the log is on.
    pub fn append(&self, more: Steps) {
        if let Some(steps) = &self.steps {
            steps.borrow_mut().0.extend(more.0);
        }
    }

    /// Leaves a remark for the reader, such as "no closed form was found, so this is
    /// the numerical value". Repeated remarks are kept once.
    pub fn note(&self, message: impl Into<String>) {
        let message = message.into();
        let mut notes = self.notes.borrow_mut();
        if !notes.contains(&message) {
            notes.push(message);
        }
    }

    /// The remarks left so far.
    pub fn notes(&self) -> Vec<String> {
        self.notes.borrow().clone()
    }

    /// The recorded steps, if logging was on.
    pub fn into_steps(self) -> Option<Steps> {
        self.steps.map(RefCell::into_inner)
    }

    /// Asks for `value` to be shown as `text` / `latex` when it is the answer.
    pub fn show_as(&self, value: &Expr, text: String, latex: String) {
        self.shown.borrow_mut().push(Shown {
            value: value.clone(),
            text,
            latex,
            unicode: None,
        });
    }

    /// [`Ctx::show_as`] for `F + C`: the antiderivative with its constant last.
    pub fn show_with_constant(&self, value: &Expr, f: &Expr, constant: &str) {
        self.shown.borrow_mut().push(Shown {
            value: value.clone(),
            text: format!("{f} + {constant}"),
            latex: format!("{} + {constant}", f.to_latex()),
            unicode: Some(format!(
                "{} + {}",
                f.to_text(),
                Expr::sym(constant).to_text()
            )),
        });
    }

    /// Everything collected: steps, remarks and preferred presentations.
    pub fn finish(self) -> Collected {
        Collected {
            steps: self.steps.map(RefCell::into_inner),
            notes: self.notes.into_inner(),
            shown: self.shown.into_inner(),
        }
    }
}

/// Evaluates holder nodes it recognises.
pub trait HolderEval: Send + Sync {
    /// `None` when `e` is not a node this evaluator handles.
    fn evaluate(&self, e: &Expr, ctx: &Ctx<'_>) -> Option<MathResult<Expr>>;
}

impl<F> HolderEval for F
where
    F: Fn(&Expr, &Ctx<'_>) -> Option<MathResult<Expr>> + Send + Sync,
{
    fn evaluate(&self, e: &Expr, ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
        self(e, ctx)
    }
}

/// The registry of holder evaluators.
#[derive(Clone)]
pub struct Holders {
    evals: Vec<Arc<dyn HolderEval>>,
    /// The fields of maths behind the named operators (`det`, `mean` …); a new field
    /// registers here without any change to the core or the evaluator.
    domains: Arc<mpp_core::Registry>,
}

impl fmt::Debug for Holders {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Holders({} evaluators)", self.evals.len())
    }
}

impl Default for Holders {
    fn default() -> Self {
        let mut h = Holders::empty();
        h.register(subs_holder);
        h.register(finite_big_op);
        h.register(named_ops);
        h.register(crate::calculus::evaluate);
        h.register(crate::helpers::evaluate);
        h.register(crate::helpers::evaluate_operators);
        h.register(crate::domains_wire::evaluate);
        h.register(crate::units_wire::evaluate);
        h.register(crate::assume::evaluate);
        h.register(crate::dispatch::evaluate);
        h.register(crate::fields_wire::evaluate);
        // Registered last, so asked first: an equation with units, before any solver.
        h.register(crate::units_wire::evaluate_solve);
        // A differential equation with units, before the plain ODE solver.
        h.register(crate::units_ode::evaluate);
        h.register(crate::ivp_wire::evaluate);
        h.register(crate::extremum::evaluate);
        h
    }
}

impl Holders {
    /// A registry with the built-in evaluators (`expand`, `factor`, `simplify`,
    /// finite sums and products, `subs`) and calculus.
    pub fn new() -> Self {
        Holders::default()
    }

    /// A registry with no evaluators: every holder stays as written.
    pub fn empty() -> Self {
        Holders {
            evals: Vec::new(),
            domains: Arc::new(mpp_core::Registry::standard()),
        }
    }

    /// The domain registry the named operators are looked up in.
    pub fn domains(&self) -> &mpp_core::Registry {
        &self.domains
    }

    /// Replaces the domain registry ([`mpp_core::Registry::standard`] by default).
    pub fn set_domains(&mut self, registry: mpp_core::Registry) {
        self.domains = Arc::new(registry);
    }

    /// Adds a field of maths; it is asked after the domains already registered.
    pub fn register_domain(&mut self, domain: Box<dyn mpp_core::Domain>) {
        Arc::make_mut(&mut self.domains).register(domain);
    }

    /// Adds an evaluator; it is asked before the ones registered earlier.
    pub fn register(&mut self, eval: impl HolderEval + 'static) {
        self.evals.insert(0, Arc::new(eval));
    }

    /// Number of evaluators.
    pub fn len(&self) -> usize {
        self.evals.len()
    }

    /// True when no evaluators are registered.
    pub fn is_empty(&self) -> bool {
        self.evals.is_empty()
    }
}

/// Evaluates a holder node, or returns it unchanged when nothing recognises it.
pub fn evaluate_holder(e: &Expr, ctx: &Ctx<'_>) -> MathResult<Expr> {
    for eval in &ctx.holders.evals {
        if let Some(result) = eval.evaluate(e, ctx) {
            return result;
        }
    }
    Ok(e.clone())
}

/// True when `e` contains an unevaluated calculus node.
pub fn contains_holder(e: &Expr) -> bool {
    match e.node() {
        ExprNode::Derivative { .. }
        | ExprNode::Integral { .. }
        | ExprNode::BigOp { .. }
        | ExprNode::Limit { .. } => true,
        _ => e.children().into_iter().any(contains_holder),
    }
}

fn one_arg<'e>(name: &str, args: &'e [Expr]) -> MathResult<&'e Expr> {
    match args {
        [a] => Ok(a),
        _ => Err(MathError::WrongArity {
            function: name.into(),
            expected: "1".into(),
            got: args.len(),
        }),
    }
}

/// `expand(x)`, `factor(x)`, `simplify(x)`.
fn named_ops(e: &Expr, ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
    let ExprNode::Apply(name, args) = e.node() else {
        return None;
    };
    let result = match &**name {
        "expand" => one_arg(name, args).and_then(Expr::expand),
        "factor" => one_arg(name, args).map(Expr::factor),
        "simplify" => one_arg(name, args).map(Expr::simplify),
        _ => return None,
    };
    if let (Ok(after), Ok(before)) = (&result, one_arg(name, args)) {
        ctx.log(name, before, after, None);
    }
    Some(result)
}

/// Sums and products with exact integer bounds and at most [`MAX_EXACT_TERMS`] terms.
fn finite_big_op(e: &Expr, ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
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
    let lo = lower.as_number()?.as_integer()?;
    let hi = upper.as_number()?.as_integer()?;
    let lo = i64::try_from(lo).ok()?;
    let hi = i64::try_from(hi).ok()?;
    if hi.saturating_sub(lo) >= MAX_EXACT_TERMS {
        return None;
    }
    let terms: Vec<Expr> = (lo..=hi).map(|k| expr.subs(var, &Expr::int(k))).collect();
    let result = match kind {
        BigOpKind::Sum => add(terms),
        BigOpKind::Product => mul(terms),
    };
    let rule = match kind {
        BigOpKind::Sum => "add the terms",
        BigOpKind::Product => "multiply the factors",
    };
    ctx.log(rule, e, &result, Some(format!("{var} = {lo}, …, {hi}")));
    Some(Ok(result))
}

/// `subs(inner, t = value)` once `inner` has been evaluated.
fn subs_holder(e: &Expr, _ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
    let ExprNode::Apply(name, args) = e.node() else {
        return None;
    };
    if &**name != "subs" {
        return None;
    }
    let [inner, binding] = args.as_slice() else {
        return None;
    };
    let ExprNode::Relation(RelOp::Eq, var, value) = binding.node() else {
        return None;
    };
    let var = var.as_sym()?;
    if contains_holder(inner) {
        return None;
    }
    Some(Ok(inner.subs(var, value)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_ins() {
        let holders = Holders::new();
        let ctx = Ctx::new(&holders);
        let x = Expr::sym("x");
        let sq = Expr::apply("expand", vec![(x.clone() + Expr::one()).pow(2)]);
        assert_eq!(
            evaluate_holder(&sq, &ctx).unwrap(),
            x.pow(2) + Expr::int(2) * x.clone() + Expr::one()
        );
        let sum = Expr::new(ExprNode::BigOp {
            kind: BigOpKind::Sum,
            expr: Expr::sym("k").pow(2),
            var: "k".into(),
            lower: Expr::one(),
            upper: Expr::int(10),
        });
        assert_eq!(evaluate_holder(&sum, &ctx).unwrap(), Expr::int(385));
        let unknown = Expr::apply("grad", vec![x.clone()]);
        assert_eq!(evaluate_holder(&unknown, &ctx).unwrap(), unknown);
    }

    #[test]
    fn later_registrations_win() {
        let mut holders = Holders::empty();
        assert!(holders.is_empty());
        holders.register(|_: &Expr, _: &Ctx<'_>| Some(Ok(Expr::int(1))));
        holders.register(|_: &Expr, _: &Ctx<'_>| Some(Ok(Expr::int(2))));
        let ctx = Ctx::new(&holders).with_steps();
        assert_eq!(
            evaluate_holder(&Expr::sym("x"), &ctx).unwrap(),
            Expr::int(2)
        );
        assert_eq!(holders.len(), 2);
        assert!(ctx.logging());
    }
}
