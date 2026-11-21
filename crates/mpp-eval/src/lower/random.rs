//! `E[g]`, `\operatorname{Var}(g)` and `P(A)` when their argument uses a declared
//! random variable (see [`crate::random`]); anything else is left to the usual rules,
//! so `E[x]` with no random variable stays whatever it meant before.

use mpp_core::Expr;
use mpp_syntax::math::{BinOp, Name, Node, NodeKind};
use mpp_syntax::symbols::Font;

use super::{Located, Lowerer};
use crate::names::{base_key, name_key};
use crate::random::{EXPECTATION, FAMILY_KEY, PROBABILITY, RV_KEY, VARIANCE};

/// Which operator a written name stands for.
fn operator_of(n: &Name) -> Option<&'static str> {
    if n.sub.is_some() || n.accent.is_some() {
        return None;
    }
    let plain_or = |f: Font| n.font.is_none() || n.font == Some(f) || n.font == Some(Font::Roman);
    match n.base.as_str() {
        "E" if plain_or(Font::Blackboard) => Some(EXPECTATION),
        "𝔼" => Some(EXPECTATION),
        "P" if plain_or(Font::Blackboard) => Some(PROBABILITY),
        "ℙ" | "Pr" => Some(PROBABILITY),
        "Var" => Some(VARIANCE),
        _ => None,
    }
}

impl Lowerer<'_> {
    /// True when `node` mentions a declared random variable or a family member.
    fn mentions_random(&self, node: &Node) -> bool {
        if let NodeKind::Name(n) = &node.kind {
            if n.sub.is_some()
                && self
                    .scope
                    .binding(&format!("{FAMILY_KEY}{}", base_key(n)))
                    .is_some()
            {
                return true;
            }
            if let Some(k) = name_key(n) {
                if !self.is_local(&k) && self.scope.binding(&format!("{RV_KEY}{k}")).is_some() {
                    return true;
                }
            }
        }
        let mut found = false;
        mpp_syntax::math::visit::children(node, &mut |c| {
            if !found && self.mentions_random(c) {
                found = true;
            }
        });
        found
    }

    /// `E[g]`, `Var(g)` or `P(A)` over random variables, or `None`.
    pub(super) fn random_operator(&mut self, node: &Node) -> Result<Option<Expr>, Located> {
        let (op, arg): (&str, &Node) = match &node.kind {
            NodeKind::Call { callee, args, .. } => {
                let Some(n) = callee.ungrouped().as_name() else {
                    return Ok(None);
                };
                match (operator_of(&n), args.as_slice()) {
                    (Some(op), [a]) => (op, a),
                    _ => return Ok(None),
                }
            }
            NodeKind::Binary {
                op: BinOp::Mul(_),
                lhs,
                rhs,
            } => {
                let Some(n) = lhs.ungrouped().as_name() else {
                    return Ok(None);
                };
                let Some(op) = operator_of(&n) else {
                    return Ok(None);
                };
                match &rhs.kind {
                    NodeKind::Group { inner, .. } => (op, &**inner),
                    _ => return Ok(None),
                }
            }
            NodeKind::NamedOp(o) if o.sub.is_none() && o.sup.is_none() => {
                let op = match o.name.as_str() {
                    "E" => EXPECTATION,
                    "Var" => VARIANCE,
                    "P" | "Pr" => PROBABILITY,
                    _ => return Ok(None),
                };
                match o.args.as_slice() {
                    [a] => (op, a),
                    _ => return Ok(None),
                }
            }
            NodeKind::FuncApp(app)
                if app.func == "Pr" && app.sub.is_none() && app.power.is_none() =>
            {
                match app.args.as_slice() {
                    [a] => (PROBABILITY, a),
                    _ => return Ok(None),
                }
            }
            _ => return Ok(None),
        };
        if !self.mentions_random(arg) {
            return Ok(None);
        }
        let inner = self.lower(arg)?;
        Ok(Some(Expr::apply(op, vec![inner])))
    }
}
