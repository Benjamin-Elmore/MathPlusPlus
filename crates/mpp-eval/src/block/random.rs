//! Random-variable declarations: `X \sim \mathrm{Uniform}[0, 1]`,
//! `X_j, Y_j \sim \mathcal N(0, 1)` (i.i.d. families), and the other distributions of
//! [`mpp_core::domains::probability::random`].
//!
//! A declaration is stored under a hidden key ([`crate::random::RV_KEY`] for one
//! variable, [`crate::random::FAMILY_KEY`] for a family), with the distribution's
//! parameters as written, so they follow the names they use. The variable itself
//! stays a name: `E[X]`, `\operatorname{Var}(X)` and `P(X \le a)` answer exactly from
//! the distribution, and sums over a family's members draw seeded samples (see
//! [`crate::random`]).

use mpp_core::domains::probability::random::{canonical_name, Dist};
use mpp_syntax::math::{BinOp, Clause, Line, Node, NodeKind, RelOp as SynRel, Statement};

use super::{Place, Run};
use crate::error::EvalError;
use crate::names::{base_key, name_key};
use crate::random::{dist_expr, FAMILY_KEY, RV_KEY};
use crate::value::{Binding, VarKind};

/// One declared name: its key and whether it is a family (`X_j`).
struct Declared {
    key: String,
    family: bool,
}

/// The name as declared: `X` (one variable), `X_j` (a family indexed by a letter) or
/// `X_1` (one variable with a subscript).
fn declared(node: &Node) -> Option<Declared> {
    let name = node.ungrouped().as_name()?;
    if let Some(sub) = &name.sub {
        if let NodeKind::Name(s) = &sub.ungrouped().kind {
            if s.sub.is_none() && s.accent.is_none() && s.base.chars().count() == 1 {
                return Some(Declared {
                    key: base_key(&name),
                    family: true,
                });
            }
        }
    }
    Some(Declared {
        key: name_key(&name)?,
        family: false,
    })
}

/// The distribution's written name and its parameter nodes.
fn dist_parts(node: &Node) -> Option<(String, Vec<&Node>)> {
    let node = node.ungrouped();
    match &node.kind {
        NodeKind::Call { callee, args, .. } => {
            let n = callee.ungrouped().as_name()?;
            if n.sub.is_some() || n.accent.is_some() {
                return None;
            }
            canonical_name(&n.base)?;
            Some((n.base.clone(), args.iter().collect()))
        }
        NodeKind::Binary {
            op: BinOp::Mul(_),
            lhs,
            rhs,
        } => {
            let n = lhs.ungrouped().as_name()?;
            if n.sub.is_some() || n.accent.is_some() {
                return None;
            }
            canonical_name(&n.base)?;
            let params = match &rhs.kind {
                NodeKind::Interval { lo, hi, .. } => vec![&**lo, &**hi],
                NodeKind::Tuple { elems, .. } => elems.iter().collect(),
                NodeKind::Group { inner, .. } => vec![&**inner],
                _ => vec![&**rhs],
            };
            Some((n.base.clone(), params))
        }
        _ => None,
    }
}

impl Run<'_, '_> {
    /// Handles `line` when it declares random variables; false when it is not a
    /// declaration (then it is read as an ordinary relation).
    pub(super) fn random_declaration(&mut self, line: &Line, place: Place) -> bool {
        if place == (Place::Display { noeval: true }) || line.noeval {
            return false;
        }
        let Statement::Relation(node) = &line.statement else {
            return false;
        };
        // `X \sim D` or `X, Y \sim D` (the parser reads the names before the last as
        // the statement and conditions of the line).
        let mut names: Vec<&Node> = Vec::new();
        let dist_node: &Node = match &node.kind {
            NodeKind::Relation { first, rest } if line.clauses.is_empty() => {
                let [(SynRel::Sim, d)] = rest.as_slice() else {
                    return false;
                };
                match &first.ungrouped().kind {
                    NodeKind::Tuple { elems, .. } => names.extend(elems.iter()),
                    _ => names.push(first),
                }
                d
            }
            NodeKind::Name(_) => {
                names.push(node);
                let mut last = None;
                for (k, c) in line.clauses.iter().enumerate() {
                    let Clause::Condition(n) = c else {
                        return false;
                    };
                    if k + 1 < line.clauses.len() {
                        if n.ungrouped().as_name().is_none() {
                            return false;
                        }
                        names.push(n);
                    } else {
                        last = Some(n);
                    }
                }
                let Some(NodeKind::Relation { first, rest }) = last.map(|n| &n.kind) else {
                    return false;
                };
                let [(SynRel::Sim, d)] = rest.as_slice() else {
                    return false;
                };
                names.push(first);
                d
            }
            _ => return false,
        };
        let Some((written, params)) = dist_parts(dist_node) else {
            return false;
        };
        let Some(decls) = names
            .iter()
            .map(|n| declared(n))
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        // From here on the line is a declaration: problems are reported, not passed on.
        let mut lowered = Vec::with_capacity(params.len());
        for p in &params {
            match self.lower(p, &[]) {
                Ok(e) => lowered.push(e),
                Err(l) => {
                    self.located(l);
                    return true;
                }
            }
        }
        let mut resolved = Vec::with_capacity(lowered.len());
        for (e, p) in lowered.iter().zip(&params) {
            match self.resolve(e, &Default::default(), p.span) {
                Ok(v) => resolved.push(v),
                Err(l) => {
                    self.located(l);
                    return true;
                }
            }
        }
        // Checks the parameters whose values are known (a < b, 0 ≤ p ≤ 1 …).
        let dist = match Dist::new(&written, resolved) {
            Ok(d) => d,
            Err(err) => {
                self.error(dist_node.span, EvalError::Math(err));
                return true;
            }
        };
        let value = dist_expr(dist.name(), lowered);
        for d in decls {
            if self.blocked(&d.key, node.span) {
                continue;
            }
            let prefix = if d.family { FAMILY_KEY } else { RV_KEY };
            let key = format!("{prefix}{}", d.key);
            let binding = Binding::Variable {
                kind: VarKind::Variable,
                value: value.clone(),
                origin: self.origin(line.span),
            };
            self.define(&key, binding);
        }
        true
    }
}
