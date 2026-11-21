//! Source positions and parser diagnostics.

use std::fmt;

/// A half-open byte range `start..end` into the whole source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    /// Makes a span; `end` is clamped so it is never before `start`.
    pub fn new(start: usize, end: usize) -> Self {
        Span {
            start,
            end: end.max(start),
        }
    }

    /// An empty span at `at`, used to point between two tokens.
    pub fn point(at: usize) -> Self {
        Span { start: at, end: at }
    }

    /// The smallest span that covers both.
    pub fn join(self, other: Span) -> Span {
        Span::new(self.start.min(other.start), self.end.max(other.end))
    }

    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// True when the span covers no bytes.
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// The span moved right by `offset` bytes.
    pub fn shifted(self, offset: usize) -> Span {
        Span::new(self.start + offset, self.end + offset)
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

/// How serious a diagnostic is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    Error,
    Warning,
    /// Plain information about an answer (a fit's R², what a helper computed): not a
    /// problem, so it never fails a check.
    Info,
}

/// A problem found while reading the source, tied to the exact span it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub span: Span,
    pub message: String,
    /// What the parser would have accepted here, in words (`"a closing `}`"`).
    pub expected: Vec<String>,
    pub severity: Severity,
}

impl Diagnostic {
    /// An error with no expected list.
    pub fn error(span: Span, message: impl Into<String>) -> Self {
        Diagnostic {
            span,
            message: message.into(),
            expected: Vec::new(),
            severity: Severity::Error,
        }
    }

    /// A warning with no expected list.
    pub fn warning(span: Span, message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Warning,
            ..Diagnostic::error(span, message)
        }
    }

    /// A piece of information with no expected list.
    pub fn info(span: Span, message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Info,
            ..Diagnostic::error(span, message)
        }
    }

    /// Adds what was expected at this point.
    pub fn expecting<I, S>(mut self, items: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.expected.extend(items.into_iter().map(Into::into));
        self
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let level = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        };
        write!(f, "{level} at {}: {}", self.span, self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_and_shift() {
        let a = Span::new(2, 5);
        let b = Span::new(7, 9);
        assert_eq!(a.join(b), Span::new(2, 9));
        assert_eq!(a.shifted(10), Span::new(12, 15));
        assert_eq!(Span::new(5, 2), Span::point(5));
        assert!(Span::point(3).is_empty());
        assert_eq!(a.len(), 3);
    }

    #[test]
    fn diagnostic_display() {
        let d = Diagnostic::error(Span::new(1, 2), "bad").expecting(["a name"]);
        assert_eq!(d.to_string(), "error at 1..2: bad");
        assert_eq!(d.expected, vec!["a name".to_string()]);
        assert_eq!(
            Diagnostic::warning(Span::point(0), "w").severity,
            Severity::Warning
        );
    }
}
