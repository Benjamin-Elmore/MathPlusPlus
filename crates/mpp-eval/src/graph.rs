//! The dependency graph: which block defines and uses which names.

use std::collections::{BTreeMap, BTreeSet};

use mpp_syntax::document::BlockId;

use crate::result::BlockResult;

/// Definitions and uses per block, in document order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DepGraph {
    order: Vec<BlockId>,
    defines: BTreeMap<BlockId, Vec<String>>,
    uses: BTreeMap<BlockId, Vec<String>>,
}

impl DepGraph {
    /// Builds the graph from block results in document order.
    pub fn build(blocks: &[BlockResult]) -> Self {
        let mut g = DepGraph::default();
        for b in blocks {
            g.order.push(b.id);
            if !b.defines.is_empty() {
                g.defines.insert(b.id, b.defines.clone());
            }
            if !b.uses.is_empty() {
                g.uses.insert(b.id, b.uses.clone());
            }
        }
        g
    }

    fn position(&self, id: BlockId) -> Option<usize> {
        self.order.iter().position(|b| *b == id)
    }

    /// Blocks that define `name`, in document order.
    pub fn definers(&self, name: &str) -> Vec<BlockId> {
        self.order
            .iter()
            .filter(|b| {
                self.defines
                    .get(b)
                    .is_some_and(|d| d.iter().any(|n| n == name))
            })
            .copied()
            .collect()
    }

    /// Blocks that use `name`, in document order.
    pub fn users(&self, name: &str) -> Vec<BlockId> {
        self.order
            .iter()
            .filter(|b| {
                self.uses
                    .get(b)
                    .is_some_and(|u| u.iter().any(|n| n == name))
            })
            .copied()
            .collect()
    }

    /// The block whose definition of `name` is in force at `block` (the last
    /// definition before it, or in it) — what go-to-definition jumps to.
    pub fn definition_at(&self, name: &str, block: BlockId) -> Option<BlockId> {
        let limit = self.position(block)?;
        self.order[..=limit]
            .iter()
            .rev()
            .find(|b| {
                self.defines
                    .get(b)
                    .is_some_and(|d| d.iter().any(|n| n == name))
            })
            .copied()
    }

    /// Blocks after `block` that depend on it, directly or through other blocks.
    pub fn downstream(&self, block: BlockId) -> Vec<BlockId> {
        let Some(start) = self.position(block) else {
            return Vec::new();
        };
        let mut names: BTreeSet<&str> = self
            .defines
            .get(&block)
            .map(|d| d.iter().map(String::as_str).collect())
            .unwrap_or_default();
        let mut out = Vec::new();
        for id in &self.order[start + 1..] {
            let uses = self
                .uses
                .get(id)
                .is_some_and(|u| u.iter().any(|n| names.contains(n.as_str())));
            if uses {
                out.push(*id);
                if let Some(d) = self.defines.get(id) {
                    names.extend(d.iter().map(String::as_str));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mpp_syntax::Span;

    fn block(id: u64, defines: &[&str], uses: &[&str]) -> BlockResult {
        let mut b = BlockResult::new(BlockId(id), Span::point(0));
        b.defines = defines.iter().map(|s| s.to_string()).collect();
        b.uses = uses.iter().map(|s| s.to_string()).collect();
        b
    }

    #[test]
    fn queries() {
        let g = DepGraph::build(&[
            block(1, &["m"], &[]),
            block(2, &["E"], &["m"]),
            block(3, &[], &["E"]),
            block(4, &[], &["v"]),
            block(5, &["m"], &[]),
        ]);
        assert_eq!(g.definers("m"), vec![BlockId(1), BlockId(5)]);
        assert_eq!(g.users("E"), vec![BlockId(3)]);
        assert_eq!(g.downstream(BlockId(1)), vec![BlockId(2), BlockId(3)]);
        assert_eq!(g.definition_at("m", BlockId(4)), Some(BlockId(1)));
        assert_eq!(g.definition_at("m", BlockId(5)), Some(BlockId(5)));
        assert_eq!(g.definition_at("q", BlockId(5)), None);
    }
}
