//! The rule-based search for an antiderivative: dispatch, linearity and recursion guards.

use crate::expand::expand;
use crate::expr::{Expr, ExprNode};
use crate::poly::as_poly;
use crate::printer::display_order;
use crate::simplify::{add, mul, pow, simplify};
use crate::steps::Steps;

use super::{holder, parts, radical, rational, subst, table, trig};

/// Deepest nesting of techniques inside one another.
const MAX_DEPTH: usize = 28;
/// Most sub-integrals one top-level integral may try.
const BUDGET: u32 = 2_500;

/// Search state for one integral (shared with the sub-integrals it spawns).
pub(crate) struct Integrator {
    /// The integration variable.
    pub var: String,
    /// Nesting depth inherited from an enclosing integral in another variable.
    base_depth: usize,
    /// Integrands being worked on (a repeat would loop).
    stack: Vec<Expr>,
    /// Sub-integrals still allowed.
    budget: u32,
}

impl Integrator {
    /// A fresh search in `var`.
    pub fn new(var: &str) -> Integrator {
        Integrator {
            var: var.to_string(),
            base_depth: 0,
            stack: Vec::new(),
            budget: BUDGET,
        }
    }

    /// Integrates `e` (canonical) in another variable, sharing the budget and depth.
    pub fn int_in(&mut self, e: &Expr, var: &str, steps: &mut Steps) -> Option<Expr> {
        let mut sub = Integrator {
            var: var.to_string(),
            base_depth: self.base_depth + self.stack.len() + 1,
            stack: Vec::new(),
            budget: self.budget,
        };
        let r = sub.int(e, steps);
        self.budget = sub.budget;
        r
    }

    fn depth(&self) -> usize {
        self.base_depth + self.stack.len()
    }

    /// An antiderivative of the canonical expression `e`, or `None`.
    pub fn int(&mut self, e: &Expr, steps: &mut Steps) -> Option<Expr> {
        if self.budget == 0 || self.depth() >= MAX_DEPTH {
            return None;
        }
        self.budget -= 1;
        let var = self.var.clone();
        if !e.contains_sym(&var) {
            let result = mul(vec![e.clone(), Expr::sym(&var)]);
            steps.push(
                "constant rule",
                holder(e, &var),
                result.clone(),
                Some(format!("∫ c d{var} = c·{var}")),
            );
            return Some(result);
        }
        if self.stack.iter().any(|s| s == e) {
            return None;
        }
        self.stack.push(e.clone());
        let r = self.int_inner(e, steps);
        self.stack.pop();
        r
    }

    /// Runs one technique with its own step log, keeping the steps only when it succeeds.
    pub fn attempt(
        &mut self,
        steps: &mut Steps,
        technique: impl FnOnce(&mut Integrator, &mut Steps) -> Option<Expr>,
    ) -> Option<Expr> {
        let mut local = Steps::new();
        let r = technique(self, &mut local)?;
        steps.0.extend(local.0);
        Some(r)
    }

    fn int_inner(&mut self, e: &Expr, steps: &mut Steps) -> Option<Expr> {
        let var = self.var.clone();
        if let ExprNode::Add(terms) = e.node() {
            if let Some(r) = self.attempt(steps, |s, l| rational::partial_fractions(s, e, l)) {
                return Some(r);
            }
            return self.attempt(steps, |s, l| s.sum_rule(e, terms, l));
        }
        if let ExprNode::Mul(factors) = e.node() {
            let (consts, rest): (Vec<Expr>, Vec<Expr>) =
                factors.iter().cloned().partition(|f| !f.contains_sym(&var));
            if !consts.is_empty() {
                let c = mul(consts);
                let rest = mul(rest);
                return self.attempt(steps, |s, l| {
                    l.push(
                        "constant multiple",
                        holder(e, &var),
                        Expr::new(ExprNode::Mul(vec![c.clone(), holder(&rest, &var)])),
                        None,
                    );
                    let r = s.int(&rest, l)?;
                    Some(mul(vec![c.clone(), r]))
                });
            }
        }
        if let Some((result, rule, note)) = table::lookup(e, &var) {
            steps.push(rule, holder(e, &var), result.clone(), note);
            return Some(result);
        }
        type Technique = fn(&mut Integrator, &Expr, &mut Steps) -> Option<Expr>;
        let techniques: [Technique; 8] = [
            rational::partial_fractions,
            radical::quadratic_forms,
            trig::trig_forms,
            trig::product_to_sum,
            subst::substitution,
            parts::by_parts,
            Integrator::expand_first,
            trig::hyperbolic_as_exponentials,
        ];
        for t in techniques {
            if let Some(r) = self.attempt(steps, |s, l| t(s, e, l)) {
                return Some(r);
            }
        }
        None
    }

    fn sum_rule(&mut self, e: &Expr, terms: &[Expr], steps: &mut Steps) -> Option<Expr> {
        let var = self.var.clone();
        let terms = display_order(terms);
        steps.push(
            "sum rule",
            holder(e, &var),
            Expr::new(ExprNode::Add(
                terms.iter().map(|t| holder(t, &var)).collect(),
            )),
            Some("integrate term by term".into()),
        );
        let mut parts = Vec::with_capacity(terms.len());
        for t in &terms {
            parts.push(self.int(t, steps)?);
        }
        Some(add(parts))
    }

    /// Multiplies out products and powers of sums, then integrates term by term.
    fn expand_first(&mut self, e: &Expr, steps: &mut Steps) -> Option<Expr> {
        let expanded = expand(e).ok()?;
        if &expanded == e || !matches!(expanded.node(), ExprNode::Add(_)) {
            return None;
        }
        steps.push(
            "expand",
            holder(e, &self.var),
            holder(&expanded, &self.var),
            None,
        );
        self.int(&expanded, steps)
    }
}

/// `(a, b)` with `e = a·var + b`, `a ≠ 0` and neither depending on `var`.
pub(crate) fn linear_coeffs(e: &Expr, var: &str) -> Option<(Expr, Expr)> {
    if e.is_sym(var) {
        return Some((Expr::one(), Expr::zero()));
    }
    let c = as_poly(e, var)?;
    if c.len() != 2 {
        return None;
    }
    let a = simplify(&c[1]);
    if a.is_zero() {
        return None;
    }
    Some((a, simplify(&c[0])))
}

/// A variable name not free in any of `exprs` and different from `avoid`.
pub(crate) fn fresh_name(exprs: &[&Expr], avoid: &str) -> String {
    ["u", "w", "v", "s", "z", "p", "q"]
        .iter()
        .map(|s| s.to_string())
        .chain((1..).map(|k| format!("u_{k}")))
        .find(|n| n != avoid && exprs.iter().all(|e| !e.contains_sym(n)))
        .unwrap_or_else(|| "u_0".into())
}

/// `num / den` built canonically.
pub(crate) fn div(num: Expr, den: Expr) -> Expr {
    mul(vec![num, pow(den, Expr::int(-1))])
}
