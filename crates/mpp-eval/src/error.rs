//! Evaluation errors with user-facing messages.

use std::fmt;

use mpp_core::MathError;
use mpp_syntax::{Diagnostic, Span};

pub use mpp_core::printer::readable;

/// Internal representations that must never reach a reader: unit factors written as
/// `unit:m()`, core node names, hidden environment keys, sampling placeholders and the
/// ASCII product sign between factors. Returns the first one found in `text` (used by
/// the tests that scan every page, answer and export).
pub fn internal_marker(text: &str) -> Option<String> {
    const MARKERS: &[&str] = &[
        "unit:",
        "unit as",
        "unit convert",
        "zzunit",
        "Apply(",
        "Sym(",
        "Num(",
        "Func(",
        "BigOp {",
        "Piecewise {",
        "document seed",
        "#rv",
        "#ind",
        "denominators:",
        "seed:",
        "rv:",
        "rvs:",
        "operatorname(",
    ];
    if let Some(m) = MARKERS.iter().find(|m| text.contains(**m)) {
        return Some((*m).to_string());
    }
    let chars: Vec<char> = text.chars().collect();
    chars
        .windows(3)
        .find(|w| {
            w[1] == '*'
                && (w[0].is_alphanumeric() || w[0] == ')')
                && (w[2].is_alphanumeric() || w[2] == '(')
        })
        .map(|w| w.iter().collect())
}

/// Every way evaluating a statement can fail.
#[derive(Debug, Clone, PartialEq)]
pub enum EvalError {
    /// The core could not compute something.
    Math(MathError),
    /// A numeric answer was asked for, but these names have no value.
    Unbound(Vec<String>),
    /// A name is defined in terms of itself.
    Circular(Vec<String>),
    /// A constant (`≡`) was defined again.
    ConstantRedefined {
        name: String,
        line: usize,
        /// The file that defined it, when it was imported.
        file: Option<String>,
    },
    /// Something that is not a function was called with several arguments.
    NotAFunction(String),
    /// A user function got the wrong number of arguments.
    Arity {
        name: String,
        expected: usize,
        got: usize,
    },
    /// A function or sequence calls itself too deeply.
    TooDeep(String),
    /// The statement needs more work than one statement may take.
    TooMuchWork,
    /// Valid notation this version cannot evaluate yet.
    Unsupported(String),
    /// Notation that has no meaning here.
    Invalid(String),
    /// The parser already reported the problem at this spot.
    Silent,
    /// An empty template box (C9) that must be filled in first; no answer is given and
    /// the line shows the hint "fill in: …" instead of an error.
    EmptyBox(String),
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EvalError::Math(e) => write!(f, "{}", readable(&e.to_string())),
            EvalError::Unbound(names) => {
                let list: Vec<String> = names.iter().map(|n| format!("`{n}`")).collect();
                if names.len() == 1 {
                    write!(
                        f,
                        "{} has no value, so there is no number to give",
                        list.join("")
                    )
                } else {
                    write!(
                        f,
                        "{} have no values, so there is no number to give",
                        list.join(", ")
                    )
                }
            }
            EvalError::Circular(chain) => {
                if chain.len() <= 2 {
                    let name = chain.first().map(String::as_str).unwrap_or("?");
                    write!(f, "`{name}` is defined in terms of itself")
                } else {
                    write!(f, "circular definition: {}", chain.join(" → "))
                }
            }
            EvalError::ConstantRedefined { name, line, file } => {
                let place = match file {
                    Some(file) => format!("line {line} of {file}"),
                    None => format!("line {line}"),
                };
                write!(
                    f,
                    "`{name}` is a constant (≡) defined on {place} and cannot be redefined"
                )
            }
            EvalError::NotAFunction(name) => write!(f, "`{name}` is not a known function"),
            EvalError::Arity {
                name,
                expected,
                got,
            } => write!(
                f,
                "{name} takes {expected} argument{}, but got {got}",
                if *expected == 1 { "" } else { "s" }
            ),
            EvalError::TooDeep(name) => write!(f, "`{name}` calls itself too deeply"),
            EvalError::TooMuchWork => write!(f, "this needs too much work to evaluate"),
            EvalError::Unsupported(what) => write!(f, "{what} is not supported yet"),
            EvalError::Invalid(what) => write!(f, "{what}"),
            EvalError::Silent => write!(f, "this could not be read"),
            EvalError::EmptyBox(label) => write!(f, "fill in: {label}"),
        }
    }
}

impl std::error::Error for EvalError {}

impl From<MathError> for EvalError {
    fn from(e: MathError) -> Self {
        EvalError::Math(e)
    }
}

/// An error tied to the source span it comes from.
#[derive(Debug, Clone, PartialEq)]
pub struct Located {
    pub error: EvalError,
    pub span: Span,
}

impl Located {
    /// Ties `error` to `span`.
    pub fn new(span: Span, error: EvalError) -> Self {
        Located { error, span }
    }

    /// The diagnostic, or `None` when the parser already reported it.
    pub fn diagnostic(&self) -> Option<Diagnostic> {
        match self.error {
            EvalError::Silent | EvalError::EmptyBox(_) => None,
            _ => Some(Diagnostic::error(self.span, self.error.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages() {
        assert_eq!(
            EvalError::Unbound(vec!["x".into()]).to_string(),
            "`x` has no value, so there is no number to give"
        );
        assert_eq!(
            EvalError::Circular(vec!["a".into(), "b".into(), "a".into()]).to_string(),
            "circular definition: a → b → a"
        );
        assert_eq!(
            EvalError::ConstantRedefined {
                name: "g".into(),
                line: 3,
                file: None,
            }
            .to_string(),
            "`g` is a constant (≡) defined on line 3 and cannot be redefined"
        );
        assert!(Located::new(Span::point(0), EvalError::Silent)
            .diagnostic()
            .is_none());
    }
}
