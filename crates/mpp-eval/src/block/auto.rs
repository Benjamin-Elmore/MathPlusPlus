//! Auto-evaluation of display lines (C8): a bare expression, or an equation with an
//! unknown, gets its answer without `= ?`, shown in grey in the view only.
//!
//! - an expression is evaluated, and the answer is shown when it says something new
//!   (`\int_0^1 x^2 \, dx` shows `= 1/3`; `x^2 + 1` with `x` free shows nothing);
//! - a differential equation in an unknown function goes to the ODE toolkit with the
//!   line's conditions (`y'' + y = 0, y(0) = 1` shows `y(x) = cos(x)`);
//! - equations with as many unknowns as equations (the line's conditions count, and a
//!   vector equation counts once per component) go to the solver, an inequality in one
//!   unknown to the inequality solver.
//!
//! Definitions and `\overset{?}{=}` checks are other statements and never come here;
//! `%noeval` and the session toggle switch it off. The work is a [`crate::auto::Job`]
//! cached by its hash and bounded by the session's time budget (see [`crate::auto`]).

use std::sync::Arc;
use std::time::Instant;

use mpp_core::{Expr, ExprNode, RelOp};
use mpp_syntax::math::{Node, NodeKind, QueryMode};

use super::{Run, Scope};
use crate::auto::{self, AutoOutcome, Job};
use crate::holders::contains_holder;
use crate::numeric::{approximate, is_exact_rational};
use crate::result::LineResult;
use crate::value::Value;

/// What an auto result answers, for presenting it.
enum Kind {
    /// The value of an expression; nothing is shown when it equals this.
    Value { trivial: Option<Expr> },
    /// The solutions of equations in these unknowns.
    Solutions(Vec<String>),
    /// The solution of a differential equation.
    Ode,
}

impl Run<'_, '_> {
    /// Works out the auto result of a display line whose input is `input`.
    pub(super) fn auto_evaluate(
        &mut self,
        lr: &mut LineResult,
        node: &Node,
        input: &Expr,
        s: &Scope,
    ) {
        let settings = self.inputs.auto;
        if !settings.enabled {
            return;
        }
        let Some((expr, kind)) = self.auto_job(node, input, s) else {
            return;
        };
        let job = Job {
            expr,
            numeric: false,
            holders: self.inputs.holders.clone(),
            assumptions: Arc::new(self.assumptions()),
        };
        let deadline = settings
            .budget
            .map(|b| *self.auto_deadline.get_or_insert_with(|| Instant::now() + b));
        match auto::answer(settings, job, deadline) {
            Some(out) => self.show_auto(lr, out, &kind),
            None => {
                lr.pending = true;
                // Run the block again on the next refresh, when the answer is in.
                self.volatile = true;
            }
        }
    }

    /// The expression to work out for `input`, with every known name put in.
    fn auto_job(&self, node: &Node, input: &Expr, s: &Scope) -> Option<(Expr, Kind)> {
        if let Some(ode) = self.ode_input(input, s) {
            let job = self.substituted(&ode, s.locals)?;
            return Some((job, Kind::Ode));
        }
        match input.node() {
            ExprNode::Relation(op, _, _) => self.equation_job(*op, input, s),
            ExprNode::Logic(..) | ExprNode::Bool(_) => None,
            _ => {
                if is_literal(node) {
                    return None;
                }
                let job = self.substituted(input, s.locals)?;
                // Nothing new to show when the answer is the expression itself (with
                // free names: `x^2 + 1`).
                let trivial = if input.free_symbols().is_empty() {
                    None
                } else {
                    Some(input.simplify())
                };
                Some((job, Kind::Value { trivial }))
            }
        }
    }

    /// `solve(…)` for an equation (with the line's equations as more of the system) or an
    /// inequality in one unknown; `None` when the line has no unknown or more unknowns
    /// than equations.
    fn equation_job(&self, op: RelOp, input: &Expr, s: &Scope) -> Option<(Expr, Kind)> {
        let first = self.substituted(input, s.locals)?;
        let mut eqs = vec![first];
        if op == RelOp::Eq {
            for c in s.conditions {
                let Ok(e) = self.lower(c, s.names) else {
                    continue;
                };
                if matches!(e.node(), ExprNode::Relation(RelOp::Eq, _, _)) {
                    eqs.push(self.substituted(&e, s.locals)?);
                }
            }
        }
        let mut unknowns: Vec<String> = Vec::new();
        for e in &eqs {
            for name in e.free_symbols() {
                if !unknowns.iter().any(|u| *u == *name) {
                    unknowns.push(name.to_string());
                }
            }
        }
        unknowns.sort();
        if unknowns.is_empty() {
            return None;
        }
        let vars = if unknowns.len() == 1 {
            Expr::sym(&unknowns[0])
        } else {
            Expr::new(ExprNode::Tuple(
                unknowns.iter().map(|u| Expr::sym(u)).collect(),
            ))
        };
        if op != RelOp::Eq {
            let inequality = matches!(op, RelOp::Lt | RelOp::Le | RelOp::Gt | RelOp::Ge);
            if !inequality || unknowns.len() != 1 {
                return None;
            }
            let job = Expr::apply("solve", vec![eqs.swap_remove(0), vars]);
            return Some((job, Kind::Solutions(unknowns)));
        }
        // Equations whose size is only known once worked out (∇f = λ∇g) go to the solver,
        // which says so if they are too few; otherwise too few equations means no answer.
        let opaque = eqs.iter().any(has_operator);
        let count: usize = eqs.iter().map(equation_count).sum();
        if !opaque && count < unknowns.len() {
            return None;
        }
        // `v = 3` just states a value: there is nothing to solve.
        if let [only] = eqs.as_slice() {
            if let ExprNode::Relation(_, l, r) = only.node() {
                let stated = |a: &Expr, b: &Expr| {
                    a.as_sym().is_some() && !unknowns.iter().any(|u| b.contains_sym(u))
                };
                if stated(l, r) || stated(r, l) {
                    return None;
                }
            }
        }
        let system = if eqs.len() == 1 {
            eqs.swap_remove(0)
        } else {
            Expr::new(ExprNode::Tuple(eqs))
        };
        let job = Expr::apply("solve", vec![system, vars]);
        Some((job, Kind::Solutions(unknowns)))
    }

    /// Puts an auto result on the line.
    fn show_auto(&mut self, lr: &mut LineResult, out: AutoOutcome, kind: &Kind) {
        let value = match out
            .value
            .map_err(|m| m.to_string())
            .and_then(|v| self.finish_logged(v, None).map_err(|e| e.to_string()))
        {
            Ok(v) => v,
            Err(message) => {
                lr.hint = Some(message);
                return;
            }
        };
        // A sum of products that expands to something shorter reads better expanded
        // (a flux integral's 11π/4 − π(π² − 2)/2 + … is 4π).
        let value = match value.expand().map(|e| e.simplify()) {
            Ok(expanded) if expanded.size() < value.size() => expanded,
            _ => value,
        };
        if contains_holder(&value) {
            lr.hint = Some("this could not be worked out exactly".into());
            return;
        }
        if let Kind::Value { trivial: Some(t) } = kind {
            if value == *t {
                return;
            }
        }
        for note in &out.notes {
            self.warn(lr.span, note.clone());
        }
        lr.auto = true;
        if let Kind::Solutions(unknowns) = kind {
            let shown = super::sentences::solution_text(unknowns, &value);
            lr.set_value(Value::from_expr(value));
            if let Some((text, latex)) = shown {
                lr.text = Some(text);
                lr.latex = Some(latex);
            }
            return;
        }
        if self.present_units(lr, &value, QueryMode::Exact, lr.span) {
            lr.auto = true;
            return;
        }
        if let Some((text, latex)) = crate::domains_wire::present_set(&value) {
            lr.set_value(Value::from_expr(value));
            lr.text = Some(text);
            lr.latex = Some(latex);
            return;
        }
        let approximate_only = value.as_number().is_some_and(|n| !n.is_exact());
        if (value.is_constant() && !is_exact_rational(&value)) || approximate_only {
            lr.approx = approximate(&value).ok();
        }
        let shown = out.shown.iter().find(|s| s.value == value).cloned();
        lr.set_value(Value::from_expr(value));
        if approximate_only && lr.approx.is_some() {
            lr.text = None;
        }
        if let Some(s) = shown {
            lr.plain = Some(s.text.clone());
            lr.text = Some(s.unicode.unwrap_or(s.text));
            lr.latex = Some(s.latex);
        }
        if matches!(kind, Kind::Ode) {
            lr.approx = None;
        }
    }
}

/// True when `e` holds an operator or calculus node not yet worked out.
fn has_operator(e: &Expr) -> bool {
    contains_holder(e)
        || matches!(e.node(), ExprNode::Apply(..))
        || e.children().into_iter().any(has_operator)
}

/// How many scalar equations `e` is: a vector equation counts once per component.
fn equation_count(e: &Expr) -> usize {
    match e.node() {
        ExprNode::Relation(_, l, r) => {
            let len = |x: &Expr| match x.node() {
                ExprNode::Tuple(items) => Some(items.len()),
                ExprNode::Mul(fs) => fs.iter().find_map(|f| match f.node() {
                    ExprNode::Tuple(items) => Some(items.len()),
                    _ => None,
                }),
                _ => None,
            };
            len(l).or_else(|| len(r)).unwrap_or(1)
        }
        _ => 1,
    }
}

/// True for a line that is already a value as written: a number, a constant,
/// a matrix or a list (it is shown as it is, with no result after it).
fn is_literal(node: &Node) -> bool {
    match &node.ungrouped().kind {
        NodeKind::Number(_)
        | NodeKind::Constant(_)
        | NodeKind::Matrix { .. }
        | NodeKind::Set { .. }
        | NodeKind::Interval { .. }
        | NodeKind::Tuple { .. }
        | NodeKind::Text(_) => true,
        NodeKind::Unary { operand, .. } => is_literal(operand),
        _ => false,
    }
}
