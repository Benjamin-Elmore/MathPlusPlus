//! Lexer and parser for Math++ source files.
//!
//! [`math::parse_math`] reads the maths between `$ … $`; every node carries a
//! [`Span`] into the whole file and problems come back as [`Diagnostic`]s.

pub mod lexer;
pub mod math;
pub mod span;
pub mod symbols;

pub use span::{Diagnostic, Severity, Span};
