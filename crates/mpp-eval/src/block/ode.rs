//! Differential equations written as statements: `y'' + y = 0, y(0) = 1, y'(0) = 0`,
//! `\frac{dy}{dx} = x y = ?` and `\operatorname{solve}_{y(x)} (…)`.
//!
//! Before a line is lowered, its unknown functions are found from the derivatives in it
//! (a name with no value that is differentiated: `y'`, `\frac{dy}{dx}`, `\dot x`, or the
//! `y(x)` under `\operatorname{solve}`). Lowering then reads `y(0)` as the value of `y`
//! at 0 rather than `y·0`, and a differential equation is handed to the ODE toolkit with
//! the line's conditions.

use mpp_core::{Expr, ExprNode, RelOp};
use mpp_syntax::math::{visit, Clause, Line, Node, NodeKind, Statement};

use super::{Run, Scope};
use crate::names::node_key;

impl Run<'_, '_> {
    /// Names differentiated on `line` that have no value of their own.
    pub(super) fn unknown_functions_of(&self, line: &Line) -> Vec<String> {
        let mut found = Vec::new();
        let mut visit_node = |n: &Node| collect_unknowns(n, &mut found);
        match &line.statement {
            // A query is a differential equation only when it holds an equation (or
            // a solver): `J_0'(2) ≈ ?` asks for a derivative, not for an unknown.
            Statement::Query { expr, .. } if has_equation(expr) || !line.clauses.is_empty() => {
                visit_node(expr)
            }
            Statement::Relation(n) => visit_node(n),
            Statement::Check { lhs, rhs } => {
                visit_node(lhs);
                visit_node(rhs);
            }
            _ => {}
        }
        for c in &line.clauses {
            if let Clause::Condition(n) | Clause::Where { value: n, .. } = c {
                collect_unknowns(n, &mut found);
            }
        }
        let view = self.view();
        // A name defined as a function or a value is not an unknown.
        found.retain(|name| view.get(name, false).is_none());
        found.sort();
        found.dedup();
        found
    }

    /// `dsolve(eq, conditions…)` when `input` is a differential equation in an unknown
    /// function of this line (or a system of them), with the line's conditions.
    pub(super) fn ode_input(&self, input: &Expr, s: &Scope) -> Option<Expr> {
        if self.unknown_fns.is_empty() {
            return None;
        }
        let conditions: Vec<Expr> = s
            .conditions
            .iter()
            .filter_map(|c| self.lower(c, s.names).ok())
            .collect();
        if is_ode(input, &self.unknown_fns) {
            let mut args = vec![input.clone()];
            args.extend(conditions);
            return Some(Expr::apply("dsolve", args));
        }
        // `\operatorname{solve}_{y(x)} (y'' + y = 0), y(0) = 1`: the conditions written
        // after the operator join its arguments.
        let ExprNode::Apply(name, args) = input.node() else {
            return None;
        };
        match (&**name, args.as_slice()) {
            ("operatorname", [which, sub, sup, body])
                if matches!(which.as_sym(), Some("solve" | "dsolve")) =>
            {
                let ExprNode::Tuple(items) = body.node() else {
                    return None;
                };
                if !items.first().is_some_and(|f| is_ode(f, &self.unknown_fns)) {
                    return None;
                }
                let mut items = items.clone();
                items.extend(conditions);
                Some(Expr::apply(
                    "operatorname",
                    vec![
                        which.clone(),
                        sub.clone(),
                        sup.clone(),
                        Expr::new(ExprNode::Tuple(items)),
                    ],
                ))
            }
            ("dsolve" | "solve", [first, ..]) if is_ode(first, &self.unknown_fns) => {
                let mut items = args.clone();
                items.extend(conditions);
                Some(Expr::apply(name, items))
            }
            _ => None,
        }
    }
}

/// True when `e` is an equation (or a tuple of equations) with a derivative of one of
/// `unknowns`.
pub(crate) fn is_ode(e: &Expr, unknowns: &[String]) -> bool {
    match e.node() {
        ExprNode::Tuple(items) => !items.is_empty() && items.iter().all(|i| is_ode(i, unknowns)),
        ExprNode::Relation(RelOp::Eq, l, r) => {
            has_unknown_derivative(l, unknowns) || has_unknown_derivative(r, unknowns)
        }
        _ => false,
    }
}

fn has_unknown_derivative(e: &Expr, unknowns: &[String]) -> bool {
    match e.node() {
        ExprNode::Derivative { expr, vars } => {
            let name = match expr.node() {
                ExprNode::Sym(s) => Some(s.to_string()),
                ExprNode::Apply(f, args) if args.len() == 1 => Some(f.to_string()),
                _ => None,
            };
            if let Some(y) = name {
                if unknowns.contains(&y) && vars.iter().all(|(v, _)| **v != *y) {
                    return true;
                }
            }
            has_unknown_derivative(expr, unknowns)
        }
        ExprNode::Apply(f, _) if unknowns.iter().any(|u| primed_name(f, u)) => true,
        _ => e
            .children()
            .into_iter()
            .any(|c| has_unknown_derivative(c, unknowns)),
    }
}

/// `y′`, `y″`… for the unknown `y`.
fn primed_name(f: &str, y: &str) -> bool {
    f.strip_prefix(y)
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| matches!(c, '′' | '\'')))
}

/// True when `node` holds an equation or a `solve`/`dsolve` operator.
fn has_equation(node: &Node) -> bool {
    match &node.kind {
        NodeKind::Relation { .. } => true,
        NodeKind::NamedOp(op) if matches!(op.name.as_str(), "solve" | "dsolve") => true,
        _ => {
            let mut found = false;
            visit::children(node, &mut |c| found = found || has_equation(c));
            found
        }
    }
}

/// Collects the names differentiated in `node` (and the `y` of `y(x)` under
/// `\operatorname{solve}`).
fn collect_unknowns(node: &Node, out: &mut Vec<String>) {
    match &node.kind {
        NodeKind::Prime { base, .. } => {
            if let Some(k) = node_key(base) {
                out.push(k);
            }
        }
        NodeKind::Derivative(d) if !d.partial => {
            if let Some(k) = d.of.as_ref().and_then(|of| node_key(of.ungrouped())) {
                let vars: Vec<String> = d.vars.iter().filter_map(|v| node_key(&v.var)).collect();
                if !vars.contains(&k) {
                    out.push(k);
                }
            }
        }
        NodeKind::NamedOp(op) if matches!(op.name.as_str(), "solve" | "dsolve") => {
            if let Some(sub) = &op.sub {
                unknowns_in_subscript(sub, out);
            }
        }
        _ => {}
    }
    visit::children(node, &mut |c| collect_unknowns(c, out));
}

/// `y(x)` or `x(t), y(t)` under `\operatorname{solve}`.
fn unknowns_in_subscript(sub: &Node, out: &mut Vec<String>) {
    match &sub.ungrouped().kind {
        NodeKind::Call { callee, .. } => {
            if let Some(k) = node_key(callee) {
                out.push(k);
            }
        }
        NodeKind::Tuple { elems, .. } => elems.iter().for_each(|e| unknowns_in_subscript(e, out)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primes_and_derivatives_mark_odes() {
        let u = vec!["y".to_string()];
        let y = Expr::sym("y");
        let dy = Expr::new(ExprNode::Derivative {
            expr: y.clone(),
            vars: vec![("x".into(), 2)],
        });
        assert!(is_ode(
            &Expr::equation(dy.clone() + y.clone(), Expr::zero()),
            &u
        ));
        assert!(!is_ode(&Expr::equation(y.clone(), Expr::zero()), &u));
        // d/dx(x) is not a derivative of an unknown function.
        let dx = Expr::new(ExprNode::Derivative {
            expr: Expr::sym("x"),
            vars: vec![("x".into(), 1)],
        });
        assert!(!is_ode(
            &Expr::equation(dx, Expr::one()),
            &["x".to_string()]
        ));
        assert!(primed_name("y′′", "y"));
        assert!(!primed_name("yy", "y"));
    }
}
