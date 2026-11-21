//! Units on results: dimension checks, the way quantities are shown, and pointing a
//! dimension error at the term that caused it.

use mpp_core::{Expr, MathError};
use mpp_syntax::math::{visit, BinOp, Node, NodeKind, QueryMode};
use mpp_syntax::Span;

use super::{Locals, Run};
use crate::error::EvalError;
use crate::result::LineResult;
use crate::units_wire;
use crate::value::Value;

impl Run<'_, '_> {
    /// Shows a value with units as a quantity (`686.7 N`, `250/9 m/s ≈ 27.78 m/s`).
    /// Returns false when the value has no units, so the usual presentation applies.
    pub(super) fn present_units(
        &mut self,
        lr: &mut LineResult,
        value: &Expr,
        mode: QueryMode,
        span: Span,
    ) -> bool {
        match units_wire::present(value) {
            // `≈ ?` needs a number: a quantity whose value still has names in it, or a
            // piecewise value whose case could not be decided, is not one, so the usual
            // path reports why (never `≈ 10000·a/24788191 mol/J`).
            Ok(Some(_))
                if mode == QueryMode::Approx
                    && units_wire::approximate(value).ok().flatten().is_none() =>
            {
                false
            }
            Ok(Some(p)) => {
                lr.set_value(Value::from_expr(value.clone()));
                lr.plain = Some(p.text);
                lr.text = Some(p.unicode);
                lr.latex = Some(p.latex);
                lr.approx = match mode {
                    QueryMode::Approx => units_wire::approximate(value).ok().flatten(),
                    QueryMode::Exact => p.approx,
                };
                true
            }
            Ok(None) => false,
            Err(err) => {
                self.error(span, EvalError::Math(err));
                true
            }
        }
    }

    /// Where a dimension error belongs: the smallest part of `node` that fails on its own,
    /// and for a sum or difference, the term on the right ("can't add m and s" points at
    /// the seconds).
    pub(super) fn dimension_span(&self, node: &Node, names: &[String], locals: &Locals) -> Span {
        let mut current = node;
        loop {
            let mut kids: Vec<&Node> = Vec::new();
            visit::children(current, &mut |c| kids.push(c));
            match kids
                .into_iter()
                .find(|c| self.has_dimension_error(c, names, locals))
            {
                Some(c) => current = c,
                None => break,
            }
        }
        match &current.kind {
            NodeKind::Binary {
                op: BinOp::Add | BinOp::Sub,
                rhs,
                ..
            } => rhs.span,
            _ => current.span,
        }
    }

    fn has_dimension_error(&self, node: &Node, names: &[String], locals: &Locals) -> bool {
        let Ok(lowered) = self.lower(node, names) else {
            return false;
        };
        let resolved = self.resolve_full(&lowered, locals, &[], false, false);
        let outcome = resolved.value.and_then(|v| self.finish_value(v));
        matches!(outcome, Err(EvalError::Math(MathError::Dimension(_))))
    }
}
