//! Math++ proofs: a checker that gives every proof step a status, a bridge that exports
//! a proof to Lean 4 + Mathlib, and a client for the proof agent.
//!
//! - [`check_source`] / [`check_document`]: the checker. Statuses are preview data for
//!   the equation view; nothing is written into the file.
//! - [`model`]: what the checker reports.

#![forbid(unsafe_code)]

mod algebra;
mod cas;
pub mod checker;
mod fields;
pub mod lean;
pub mod model;
mod notation;

pub use checker::{check_document, check_source};
pub use model::{
    ClaimCheck, ClaimOutcome, DocumentCheck, ProofCheck, ResultCheck, StepCheck, StepStatus,
    Summary, Verdict,
};
