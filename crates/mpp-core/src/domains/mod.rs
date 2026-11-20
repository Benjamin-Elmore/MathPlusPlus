//! Wider maths behind one interface: each field (linear algebra, equation solving,
//! differential equations, sets, number theory, statistics) is a module implementing
//! [`Domain`], and a [`Registry`] dispatches named operators such as `det`, `solve`,
//! `eigen` or `mean` to the domain that owns them.
//!
//! Values are core expressions ([`Expr`]): matrices, tuples, sets, intervals and
//! relations already live in the expression tree, so a domain result can flow straight
//! back into the evaluator, the printer and other domains.

pub mod assume;
pub mod calculus;
pub mod distributions;
pub mod intervals;
pub mod linalg;
pub mod number_theory;
pub mod ode;
pub mod piecewise;
pub mod regression;
pub mod sets;
pub mod solve;
pub mod stats;
pub mod util;

use crate::error::{MathError, MathResult};
use crate::expr::Expr;
use crate::steps::Steps;

pub use assume::Assumptions;
pub use calculus::{BasicCalculus, Calculus, FullCalculus};
pub use distributions::Distributions;
pub use linalg::{LinearAlgebra, Matrix};
pub use number_theory::NumberTheory;
pub use ode::Odes;
pub use regression::{Fit, FitKind, Regression};
pub use sets::{RealSet, Sets};
pub use solve::{Solutions, Solving};
pub use stats::Statistics;

/// What a domain operator returns. Matrices, sets, tuples and relations are all
/// expressions, so the value type is the core expression.
pub type Value = Expr;

/// The calculus used when a context is created without one.
static BASIC_CALCULUS: BasicCalculus = BasicCalculus;

/// Everything an operator may use besides its arguments: an optional step log, the
/// calculus backend (the core calculus lane plugs its `diff`/`integrate` in here) and
/// notes the evaluator shows beside the result ("sample variance", "≈ numeric").
pub struct Ctx<'a> {
    /// Step log ("show the working"), when the caller wants one.
    pub steps: Option<&'a mut Steps>,
    /// Differentiation and integration used by solvers that need them (ODEs).
    pub calculus: &'a dyn Calculus,
    /// Short remarks about how the result was found.
    pub notes: Vec<String>,
}

impl Default for Ctx<'_> {
    fn default() -> Self {
        Ctx::new()
    }
}

impl<'a> Ctx<'a> {
    /// A context with no step log and the basic built-in calculus.
    pub fn new() -> Ctx<'a> {
        Ctx {
            steps: None,
            calculus: &BASIC_CALCULUS,
            notes: Vec::new(),
        }
    }

    /// A context that records steps into `steps`.
    pub fn with_steps(steps: &'a mut Steps) -> Ctx<'a> {
        Ctx {
            steps: Some(steps),
            ..Ctx::new()
        }
    }

    /// Replaces the calculus backend.
    pub fn with_calculus(mut self, calculus: &'a dyn Calculus) -> Ctx<'a> {
        self.calculus = calculus;
        self
    }

    /// Records a step when a log is present.
    pub fn step(&mut self, rule: &str, before: &Expr, after: &Expr, note: Option<String>) {
        Steps::record(&mut self.steps, rule, before, after, note);
    }

    /// Adds a note shown beside the result.
    pub fn note(&mut self, text: impl Into<String>) {
        let text = text.into();
        if !self.notes.contains(&text) {
            self.notes.push(text);
        }
    }
}

/// One field of maths: a set of named operators.
pub trait Domain: Send + Sync {
    /// Short name, such as "linear algebra".
    fn name(&self) -> &'static str;

    /// The operator names this domain handles (`det`, `solve` …).
    fn operators(&self) -> &'static [&'static str];

    /// Applies `op` to `args`. `None` means this domain does not handle `op` (or not
    /// with these kinds of arguments) so the registry can try the next one.
    fn apply(&self, op: &str, args: &[Value], ctx: &mut Ctx<'_>) -> Option<MathResult<Value>>;
}

/// Dispatches named operators to the registered domains, in registration order.
pub struct Registry {
    domains: Vec<Box<dyn Domain>>,
}

impl Default for Registry {
    fn default() -> Self {
        Registry::standard()
    }
}

impl Registry {
    /// A registry with no domains.
    pub fn empty() -> Registry {
        Registry {
            domains: Vec::new(),
        }
    }

    /// Every built-in domain.
    pub fn standard() -> Registry {
        let mut r = Registry::empty();
        r.register(Box::new(LinearAlgebra));
        r.register(Box::new(Solving));
        r.register(Box::new(Odes));
        r.register(Box::new(Sets));
        r.register(Box::new(NumberTheory));
        r.register(Box::new(Statistics));
        r.register(Box::new(Distributions));
        r.register(Box::new(Regression));
        r
    }

    /// Adds a domain; later domains are tried after earlier ones.
    pub fn register(&mut self, domain: Box<dyn Domain>) {
        self.domains.push(domain);
    }

    /// True when some domain lists `op`.
    pub fn handles(&self, op: &str) -> bool {
        self.domain_of(op).is_some()
    }

    /// Name of the first domain listing `op`.
    pub fn domain_of(&self, op: &str) -> Option<&'static str> {
        self.domains
            .iter()
            .find(|d| d.operators().contains(&op))
            .map(|d| d.name())
    }

    /// Every operator name, in registration order.
    pub fn operators(&self) -> Vec<&'static str> {
        self.domains
            .iter()
            .flat_map(|d| d.operators().iter().copied())
            .collect()
    }

    /// The registered domains' names.
    pub fn domain_names(&self) -> Vec<&'static str> {
        self.domains.iter().map(|d| d.name()).collect()
    }

    /// Applies `op`; `None` when no domain handles it.
    pub fn apply(&self, op: &str, args: &[Value], ctx: &mut Ctx<'_>) -> Option<MathResult<Value>> {
        self.domains
            .iter()
            .filter(|d| d.operators().contains(&op))
            .find_map(|d| d.apply(op, args, ctx))
    }

    /// Like [`Registry::apply`], with an error naming the operator when nobody handles it.
    pub fn call(&self, op: &str, args: &[Value], ctx: &mut Ctx<'_>) -> MathResult<Value> {
        self.apply(op, args, ctx)
            .unwrap_or_else(|| Err(MathError::UnknownFunction(op.to_string())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_dispatches_by_name() {
        let r = Registry::standard();
        for op in ["det", "solve", "dsolve", "union", "powmod", "mean", "binom"] {
            assert!(r.handles(op), "{op} is not registered");
        }
        assert_eq!(r.domain_of("det"), Some("linear algebra"));
        assert!(!r.handles("frobnicate"));
        let mut ctx = Ctx::new();
        let m = Expr::matrix(vec![
            vec![Expr::int(1), Expr::int(2)],
            vec![Expr::int(3), Expr::int(4)],
        ])
        .unwrap();
        assert_eq!(r.call("det", &[m], &mut ctx), Ok(Expr::int(-2)));
        assert_eq!(
            r.call("frobnicate", &[], &mut ctx),
            Err(MathError::UnknownFunction("frobnicate".into()))
        );
    }

    #[test]
    fn operator_names_are_unique() {
        let r = Registry::standard();
        let ops = r.operators();
        let mut sorted = ops.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ops.len(), "an operator is listed twice");
    }
}
