//! `%seed n` and the seeds of sampling calls (see [`crate::fields_wire`]).
//!
//! The seed lives in the environment under [`SEED_KEY`], so it is scoped from its
//! line down, and a block that samples reads it (and re-runs when it changes).

use mpp_core::Expr;
use mpp_syntax::document::Directive;
use mpp_syntax::Span;

use super::Run;
use crate::error::EvalError;
use crate::fields_wire::{needs_seed, seed_calls, seed_directive, SEED_KEY};
use crate::value::{Binding, VarKind};

impl Run<'_, '_> {
    /// `%seed 42`: the seed for every sampling call below it.
    pub(super) fn seed_directive(&mut self, d: &Directive, span: Span) {
        match seed_directive(&d.text) {
            Some(Ok(seed)) => {
                let binding = Binding::Variable {
                    kind: VarKind::Variable,
                    value: Expr::int(seed),
                    origin: self.origin(span),
                };
                self.define(SEED_KEY, binding);
                self.result.defines.retain(|n| n != SEED_KEY);
            }
            Some(Err(message)) => self.error(span, EvalError::Invalid(message)),
            None => {}
        }
    }

    /// `e` (lowered from the node at `span`) with a seed for each sampling call that
    /// was written without one.
    pub(super) fn seeded(&self, e: Expr, span: Span) -> Expr {
        if !needs_seed(&e) {
            return e;
        }
        let seed = match self.view().get(SEED_KEY, false).as_deref() {
            Some(Binding::Variable { value, .. }) => value
                .as_number()
                .and_then(|n| n.as_integer())
                .and_then(|n| i64::try_from(n).ok()),
            _ => None,
        };
        let place = span.start.saturating_sub(self.result.span.start) as u64;
        seed_calls(&e, seed, self.block.0, place)
    }
}
