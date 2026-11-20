//! Sentence forms and result finishing that build on the domain registry: `Solve … for …`,
//! and the last pass every value goes through before it is shown.

use mpp_core::{Expr, ExprNode};
use mpp_syntax::document::Solve;
use mpp_syntax::math::{CaseCondition, Node, NodeKind, QueryMode};

use super::{Locals, Run};
use crate::error::EvalError;
use crate::names::node_key;
use crate::result::{LineKind, LineResult};
use crate::value::Value;

impl Run<'_, '_> {
    /// The pass every result goes through: matrix arithmetic whose order doesn't matter,
    /// and the dimension check of values with units.
    pub(super) fn finish_value(&self, value: Expr) -> Result<Expr, EvalError> {
        self.finish_logged(value, None)
    }

    /// [`Run::finish_value`], with the simplifications the assumptions allow recorded
    /// in `steps` (and the reasons the safe form stays, when they don't).
    pub(super) fn finish_logged(
        &self,
        value: Expr,
        steps: Option<&mut mpp_core::Steps>,
    ) -> Result<Expr, EvalError> {
        let value = crate::domains_wire::finish(&value).map_err(EvalError::Math)?;
        crate::units_wire::check(&value).map_err(EvalError::Math)?;
        Ok(self.with_assumptions(value, steps))
    }

    /// `Solve $x^2 = 4$ for $x$.`, `Solve $x + y = 3, x - y = 1$ for $x, y$.` and a
    /// `cases` block of equations: answered like a query.
    pub(super) fn solve_sentence(&mut self, s: &Solve) {
        let mut lr = LineResult::new(
            s.equation.span,
            LineKind::Query {
                mode: QueryMode::Exact,
                steps: false,
            },
        );
        let equations = equations_of(&s.equation);
        let mut unknowns: Vec<String> = Vec::new();
        for n in s.unknowns.iter().flatten() {
            match node_key(n) {
                Some(k) => unknowns.push(k),
                None => {
                    self.error(
                        n.span,
                        EvalError::Invalid("expected the name of an unknown".into()),
                    );
                    return self.push(lr);
                }
            }
        }
        let mut lowered = Vec::with_capacity(equations.len());
        for eq in &equations {
            match self.lower(eq, &unknowns) {
                Ok(e) => lowered.push(e),
                Err(l) => {
                    self.located(l);
                    return self.push(lr);
                }
            }
        }
        let eqs = match lowered.len() {
            1 => lowered.swap_remove(0),
            _ => Expr::new(ExprNode::Tuple(lowered)),
        };
        let mut args = vec![eqs];
        match unknowns.as_slice() {
            [] => {}
            [one] => args.push(Expr::sym(one)),
            many => args.push(Expr::new(ExprNode::Tuple(
                many.iter().map(|u| Expr::sym(u)).collect(),
            ))),
        }
        let input = Expr::apply("solve", args);
        lr.input = Some(input.clone());
        let resolved = self.resolve_full(&input, &Locals::new(), &unknowns, false, false);
        for note in &resolved.notes {
            self.warn(s.equation.span, note.clone());
        }
        match resolved.value {
            Ok(v) if matches!(v.node(), ExprNode::Apply(name, _) if &**name == "solve") => {
                self.error(
                    s.equation.span,
                    EvalError::Unsupported("solving this equation".into()),
                );
            }
            Ok(v) => {
                let shown_for = if unknowns.is_empty() {
                    // `Solve $2x + 1 = 7$.`: the one name without a value is the unknown.
                    input
                        .free_symbols()
                        .iter()
                        .filter(|n| self.env.get(n).is_none())
                        .map(|n| n.to_string())
                        .collect()
                } else {
                    unknowns.clone()
                };
                let shown = solution_text(&shown_for, &v);
                lr.set_value(Value::from_expr(v));
                if let Some((text, latex)) = shown {
                    lr.text = Some(text);
                    lr.latex = Some(latex);
                }
            }
            Err(err) => self.error(s.equation.span, err),
        }
        self.push(lr);
    }
}

/// The equations of a `Solve` sentence: a list, the rows of a `cases` block without
/// conditions, or one equation.
fn equations_of(node: &Node) -> Vec<&Node> {
    match &node.kind {
        NodeKind::Tuple { elems, .. } => elems.iter().collect(),
        NodeKind::Cases { rows }
            if rows
                .iter()
                .all(|r| matches!(r.condition, CaseCondition::Missing)) =>
        {
            rows.iter().map(|r| &r.value).collect()
        }
        NodeKind::Group { inner, .. } => equations_of(inner),
        _ => vec![node],
    }
}

/// `x = 2 or x = 3` for one unknown with finitely many solutions, `no solution` for
/// none; other answers keep the solver's own form.
fn solution_text(unknowns: &[String], v: &Expr) -> Option<(String, String)> {
    let [x] = unknowns else {
        return None;
    };
    let interval_like = matches!(v.node(), ExprNode::Interval { .. })
        || matches!(v.node(), ExprNode::Apply(n, _) if &**n == "union");
    if interval_like {
        let (text, latex) =
            crate::domains_wire::present_set(v).unwrap_or_else(|| (v.to_string(), v.to_latex()));
        let lx = Expr::sym(x).to_latex();
        return Some((format!("{x} ∈ {text}"), format!("{lx} \\in {latex}")));
    }
    let ExprNode::Set(values) = v.node() else {
        return None;
    };
    if values.is_empty() {
        return Some(("no solution".into(), "\\text{no solution}".into()));
    }
    let latex_x = Expr::sym(x).to_latex();
    let text: Vec<String> = values.iter().map(|s| format!("{x} = {s}")).collect();
    let latex: Vec<String> = values
        .iter()
        .map(|s| format!("{latex_x} = {}", s.to_latex()))
        .collect();
    Some((text.join(" or "), latex.join(" \\;\\text{or}\\; ")))
}
