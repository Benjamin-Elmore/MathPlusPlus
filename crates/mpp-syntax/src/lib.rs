//! Lexer and parser for Math++ source files.
//!
//! [`document::parse_document`] reads a whole `.mpp` file; [`math::parse_math`] reads the maths between `$ … $`; every node carries a
//! [`Span`] into the whole file and problems come back as [`Diagnostic`]s.

pub mod document;
pub mod lexer;
pub mod math;
pub mod span;
pub mod symbols;

pub use span::{Diagnostic, Severity, Span};
