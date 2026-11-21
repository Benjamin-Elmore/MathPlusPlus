//! `Assume …` sentences and bracketed conditions: facts about names, scoped like
//! definitions (in force from where they are written, for the rest of the document) or
//! to one display block.
//!
//! The facts live in the environment under reserved keys, so they are scoped, imported
//! and tracked for re-runs like any definition: `assume:x` holds the conditions on `x`,
//! and `assume:` lists the names with conditions (every block reads it, so adding an
//! assumption anywhere above re-runs the blocks below).

use std::sync::Arc;

use mpp_core::domains::assume::simplify_with;
use mpp_core::domains::Assumptions;
use mpp_core::{Expr, ExprNode, LogicOp, Steps};
use mpp_syntax::document::Assume;
use mpp_syntax::math::{Clause, MathAst};

use super::{Place, Run};
use crate::error::EvalError;
use crate::result::{LineKind, LineResult};
use crate::value::{Binding, VarKind};

/// Key of the list of names with assumptions.
const INDEX: &str = "assume:";

fn key(name: &str) -> String {
    format!("{INDEX}{name}")
}

impl Run<'_, '_> {
    /// Every fact in force here: the document's assumptions so far and the block's own.
    pub(super) fn assumptions(&self) -> Assumptions {
        let mut facts = Assumptions::new();
        let view = self.view();
        if let Some(Binding::Variable { value, .. }) = view.get(INDEX, false).as_deref() {
            let names: Vec<String> = match value.node() {
                ExprNode::Tuple(items) => items
                    .iter()
                    .filter_map(|i| i.as_sym().map(str::to_string))
                    .collect(),
                _ => Vec::new(),
            };
            for n in names {
                if let Some(Binding::Variable { value, .. }) = view.get(&key(&n), false).as_deref()
                {
                    // Checked when it was assumed; a failure here can only repeat that error.
                    let _ = facts.assume(value);
                }
            }
        }
        for c in &self.local_assumptions {
            let _ = facts.assume(c);
        }
        facts
    }

    /// The final simplification with the facts in force (`√(x²) = x` for `x ≥ 0`).
    pub(super) fn with_assumptions(&self, value: Expr, steps: Option<&mut Steps>) -> Expr {
        let facts = self.assumptions();
        simplify_with(&value, &facts, steps)
    }

    /// A display block: its bracketed conditions hold for every line of the block.
    pub(super) fn display_block(&mut self, ast: &MathAst, noeval: bool) {
        let saved = std::mem::take(&mut self.local_assumptions);
        let mut facts = self.assumptions();
        for line in &ast.lines {
            for clause in &line.clauses {
                let Clause::Assumption(node) = clause else {
                    continue;
                };
                match self.lower(node, &[]) {
                    Ok(cond) => match facts.assume(&cond) {
                        Ok(()) => self.local_assumptions.push(cond),
                        Err(err) => self.error(node.span, EvalError::Math(err)),
                    },
                    Err(l) => self.located(l),
                }
            }
        }
        self.math(ast, Place::Display { noeval });
        self.local_assumptions = saved;
    }

    /// `Assume $x > 0$.`: adds facts for the rest of the document; a fact that
    /// contradicts the ones before is an error.
    pub(super) fn assume_sentence(&mut self, a: &Assume) {
        let mut facts = self.assumptions();
        for node in &a.conditions {
            let mut line = LineResult::new(node.span, LineKind::Relation);
            let cond = match self.lower(node, &[]) {
                Ok(c) => c,
                Err(l) => {
                    self.located(l);
                    continue;
                }
            };
            line.input = Some(cond.clone());
            let names: Vec<String> = cond.free_symbols().iter().map(|s| s.to_string()).collect();
            if let Some(bound) = names.iter().find(|n| self.has_value(n)) {
                self.error(
                    node.span,
                    EvalError::Invalid(format!(
                        "`{bound}` already has a value, so there is nothing to assume about it"
                    )),
                );
                self.push(line);
                continue;
            }
            let mut trial = facts.clone();
            match trial.assume(&cond) {
                Ok(()) => facts = trial,
                Err(err) => {
                    self.error(node.span, EvalError::Math(err));
                    self.push(line);
                    continue;
                }
            }
            for n in &names {
                self.store_assumption(n, &cond, node.span);
            }
            self.push(line);
        }
    }

    fn has_value(&self, name: &str) -> bool {
        matches!(
            self.view().get(name, false).as_deref(),
            Some(Binding::Variable { .. } | Binding::Function { .. } | Binding::Sequence { .. })
        )
    }

    /// Adds `cond` to the conditions on `name` and `name` to the index.
    fn store_assumption(&mut self, name: &str, cond: &Expr, span: mpp_syntax::Span) {
        let k = key(name);
        let combined = match self.view().get(&k, false).as_deref() {
            Some(Binding::Variable { value, .. }) => Expr::new(ExprNode::Logic(
                LogicOp::And,
                vec![value.clone(), cond.clone()],
            )),
            _ => cond.clone(),
        };
        let mut names: Vec<Expr> = match self.view().get(INDEX, false).as_deref() {
            Some(Binding::Variable { value, .. }) => match value.node() {
                ExprNode::Tuple(items) => items.clone(),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        };
        if !names.iter().any(|n| n.is_sym(name)) {
            names.push(Expr::sym(name));
        }
        let origin = self.origin(span);
        for (k, value) in [
            (k, combined),
            (INDEX.to_string(), Expr::new(ExprNode::Tuple(names))),
        ] {
            let b = Arc::new(Binding::Variable {
                kind: VarKind::Variable,
                value,
                origin: origin.clone(),
            });
            self.env.insert(k.clone(), b.clone());
            self.effects.push((k, b));
        }
    }
}
