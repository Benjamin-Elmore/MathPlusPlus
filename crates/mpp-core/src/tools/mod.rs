//! Tools built on the CAS that read like a textbook: the step tree behind "show the
//! working" ([`steps`]), single-variable function analysis for curve sketching
//! ([`analysis`]) and multivariable and vector calculus helpers ([`multivar`]).
//!
//! Every tool answers exactly when the CAS can (derivatives, the equation solver,
//! integrals); when it falls back to numbers the result says so (`≈`), and when it cannot
//! decide it says that instead of guessing.

pub mod analysis;
pub mod steps;
pub mod table;
#[cfg(test)]
pub(crate) mod testkit;

pub use analysis::Analysis;
pub use steps::{StepNode, StepTree};
