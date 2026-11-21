//! The quick calculator: one line of maths in, its answer out (`mpp calc "\int_0^1 x^2 dx"`).

use std::fmt;
use std::sync::Arc;

use mpp_syntax::Severity;

use crate::import::NoImports;
use crate::result::LineKind;
use crate::session::Session;

/// Why the calculator gave no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalcError(pub String);

impl fmt::Display for CalcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CalcError {}

/// Works out one line of maths, written as in a `.mpp` maths block (with or without the
/// `$$`): an expression or an equation is answered as auto-evaluation answers it,
/// `= ?` / `≈ ?` lines as queries, and `\overset{?}{=}` lines get their ✓ or ✗.
///
/// The answer comes back as text: `1/3`, `π^2/6 ≈ 1.644934067`, `x = 2 or x = 3`,
/// `y(x) = cos(x)`, `✓ (proved symbolically)`. An expression with nothing to work out
/// comes back simplified.
pub fn calc_line(input: &str) -> Result<String, CalcError> {
    let maths = input.trim();
    let maths = maths
        .strip_prefix("$$")
        .and_then(|m| m.strip_suffix("$$"))
        .or_else(|| maths.strip_prefix('$').and_then(|m| m.strip_suffix('$')))
        .unwrap_or(maths)
        .trim();
    if maths.is_empty() {
        return Err(CalcError("there is nothing to work out".into()));
    }
    if maths.contains("$$") || maths.contains('\n') {
        return Err(CalcError("the calculator takes one line of maths".into()));
    }
    let src = format!("$$ {maths} $$\n");
    let mut session = Session::new().with_importer(Arc::new(NoImports));
    session.set_time_limit(Some(crate::session::DEFAULT_TIME_LIMIT));
    let ev = session.run_str(&src);
    if let Some(d) = ev
        .blocks
        .iter()
        .flat_map(|b| b.diagnostics.iter())
        .chain(ev.diagnostics.iter())
        .find(|d| d.severity == Severity::Error)
    {
        return Err(CalcError(d.message.clone()));
    }
    let Some(line) = ev.blocks.iter().flat_map(|b| b.values.iter()).next() else {
        return Err(CalcError("this is not a line of maths".into()));
    };
    if let Some(hint) = &line.hint {
        return Err(CalcError(hint.clone()));
    }
    if let Some(v) = line.verdict_text() {
        return Ok(v);
    }
    if matches!(line.kind, LineKind::Definition { .. }) {
        return Err(CalcError(
            "a definition has no answer; write the expression, or `= ?`".into(),
        ));
    }
    if let Some(answer) = line.answer() {
        return Ok(answer.strip_prefix("= ").unwrap_or(&answer).to_string());
    }
    match &line.input {
        Some(e) => Ok(e.simplify().to_string()),
        None => Err(CalcError("this could not be read".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_line_answers() {
        assert_eq!(calc_line(r"\int_0^1 x^2 dx").as_deref(), Ok("1/3"));
        assert_eq!(calc_line(r"$$ 2 + 3 $$").as_deref(), Ok("5"));
        assert_eq!(calc_line(r"x^2 = 9").as_deref(), Ok("x = -3 or x = 3"));
        assert_eq!(
            calc_line(r"\sqrt{2} \approx ?").as_deref(),
            Ok("≈ 1.414213562")
        );
        assert_eq!(
            calc_line(r"\sin^2 x + \cos^2 x \overset{?}{=} 1").as_deref(),
            Ok("✓ (proved symbolically)")
        );
        assert_eq!(calc_line(r"x + x").as_deref(), Ok("2*x"));
        assert!(calc_line("").is_err());
        assert!(calc_line(r"\frac{1}{").is_err());
    }
}
