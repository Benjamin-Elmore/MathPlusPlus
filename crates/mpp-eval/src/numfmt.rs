//! Number display: the `%` directives, and showing a result under a [`NumberFormat`].
//!
//! The evaluator always computes the exact result and the 10-digit approximation.
//! After a run, [`apply`] reads the display directives of the document (`%sf 4`,
//! `%dp 2`, `%sci`, `%eng`, `%exact`, `%decimal`, `%mixed`, `%deg`, `%polar`, and the
//! block-only `%block sf 3`) and re-shows every result under the format in effect at
//! that point in the file. Results are only ever re-shown, never recomputed, so a
//! directive cannot change a value.
//!
//! The IDE can show a result differently without touching the file:
//! [`LineResult::reformat`] says what a line would read under another format and
//! [`LineResult::set_format`] switches it.

use std::collections::HashMap;

use mpp_core::display::{
    format_number, to_degrees, with_format, AngleUnit, ComplexForm, Fractions, NumberFormat,
    Precision, Style,
};
use mpp_core::{Expr, ExprNode, Number};
use mpp_syntax::document::display::{parse_display_directive, DisplayOption};
use mpp_syntax::document::{BlockId, BlockKind, DirectiveKind};
use mpp_syntax::{Diagnostic, Span};

use crate::holders::contains_holder;
use crate::numeric::{approximate, is_exact_rational, scalar};
use crate::result::{Evaluated, LineKind, LineResult};
use crate::units_wire;
use crate::value::Value;
use crate::QueryMode;

/// The format a line was last shown under.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shown {
    pub format: NumberFormat,
    /// False when the line's text is not just its value (a custom presentation), so
    /// it is left as the evaluator wrote it.
    pub derived: bool,
    /// The line's value is an angle (the answer of an inverse trig function), so
    /// `%deg` shows it in degrees.
    pub angle: bool,
}

/// Changes `fmt` the way the directive option says.
pub fn apply_option(fmt: &mut NumberFormat, option: DisplayOption) {
    match option {
        DisplayOption::Exact => fmt.style = Style::Exact,
        DisplayOption::Decimal => fmt.style = Style::Decimal,
        DisplayOption::Sf(n) => fmt.precision = Precision::Sig(n),
        DisplayOption::Dp(n) => fmt.precision = Precision::Places(n),
        DisplayOption::Sci(t) => {
            *fmt = fmt.scientific(t.unwrap_or(4), t.unwrap_or(3));
        }
        DisplayOption::Eng(t) => {
            *fmt = fmt.engineering(t.unwrap_or(4), t.unwrap_or(3));
        }
        DisplayOption::Plain => *fmt = fmt.plain(),
        DisplayOption::Mixed => fmt.fractions = Fractions::Mixed,
        DisplayOption::Improper => fmt.fractions = Fractions::Improper,
        DisplayOption::Deg => fmt.angle = AngleUnit::Degrees,
        DisplayOption::Rad => fmt.angle = AngleUnit::Radians,
        DisplayOption::Polar => fmt.complex = ComplexForm::Polar,
        DisplayOption::Rect => fmt.complex = ComplexForm::Rectangular,
    }
}

/// Reads the display directives of the run document and re-shows every result.
/// Malformed directives become error diagnostics.
pub(crate) fn apply(ev: &mut Evaluated, base: &NumberFormat) {
    let mut formats: HashMap<BlockId, (NumberFormat, bool)> = HashMap::new();
    let mut problems: Vec<Diagnostic> = Vec::new();
    let mut current = *base;
    // `%exact` was asked for: decimals the user typed then give exact answers too.
    let mut exact_asked = false;
    let mut pending: Vec<DisplayOption> = Vec::new();
    // Only the directives are needed from the blocks (cloning every block's syntax
    // tree here used to cost more than the evaluation of an edit).
    type Entry = (BlockId, Span, Option<(DirectiveKind, String)>);
    let mut flat: Vec<Entry> = Vec::new();
    ev.document.walk(&mut |b| {
        let directive = match &b.kind {
            BlockKind::Directive(d) => Some((d.kind, d.text.clone())),
            _ => None,
        };
        flat.push((b.id, b.span, directive));
    });
    let with_values: std::collections::HashSet<BlockId> = ev
        .blocks
        .iter()
        .filter(|r| !r.values.is_empty())
        .map(|r| r.id)
        .collect();
    for (id, span, directive) in flat {
        if let Some((kind, text)) = &directive {
            if *kind == DirectiveKind::Display {
                match parse_display_directive(text) {
                    Some(Ok(dir)) if dir.block_only => pending.push(dir.option),
                    Some(Ok(dir)) => {
                        if matches!(dir.option, DisplayOption::Exact | DisplayOption::Decimal) {
                            exact_asked = dir.option == DisplayOption::Exact;
                        }
                        apply_option(&mut current, dir.option);
                    }
                    Some(Err(message)) => problems.push(Diagnostic::error(span, message)),
                    None => {}
                }
            }
            continue;
        }
        let has_values = with_values.contains(&id);
        let mut fmt = current;
        let mut exact = exact_asked;
        if has_values {
            for option in pending.drain(..) {
                if matches!(option, DisplayOption::Exact | DisplayOption::Decimal) {
                    exact = option == DisplayOption::Exact;
                }
                apply_option(&mut fmt, option);
            }
        }
        formats.insert(id, (fmt, exact));
    }
    ev.diagnostics.extend(problems);
    for block in &mut ev.blocks {
        let (fmt, exact) = formats
            .get(&block.id)
            .copied()
            .unwrap_or((current, exact_asked));
        for line in &mut block.values {
            let angle = is_angle_source(&ev.source, line);
            let mut fmt = fmt;
            // Decimal in, decimal out: `g + 1` after `g \equiv 9.81` is 10.81.
            if line.decimal_input && fmt.style == Style::Exact && !exact {
                fmt.style = Style::Decimal;
            }
            line.refresh(&fmt, Some(angle));
        }
    }
}

/// What a line reads as under a format.
struct Presented {
    text: Option<String>,
    latex: Option<String>,
    approx: Option<String>,
    /// The plain form, when `text` is a Unicode reading of its own.
    plain: Option<String>,
}

const ANGLE_FUNCTIONS: &[&str] = &[
    r"\arcsin",
    r"\arccos",
    r"\arctan",
    r"\operatorname{atan2}",
    r"\operatorname{arg}",
    r"\arg",
    r"\sin^{-1}",
    r"\cos^{-1}",
    r"\tan^{-1}",
];

/// True when the line's value is an angle: what is written is exactly one inverse trig
/// function (`\arcsin(1/2) = ?`), found from the line's source because the evaluator has
/// already turned the call into its value.
fn is_angle_source(src: &str, lr: &LineResult) -> bool {
    let Some(line) = src.get(lr.span.start..lr.span.end) else {
        return false;
    };
    let mut text = line.trim();
    if matches!(lr.kind, LineKind::Definition { .. }) {
        match text
            .rfind(":=")
            .map(|i| (i, 2))
            .or_else(|| text.rfind(r"\equiv").map(|i| (i, 6)))
        {
            Some((i, n)) => text = text[i + n..].trim(),
            None => return false,
        }
    } else {
        text = text.trim_end_matches(['?', '!']).trim_end();
        for marker in [r"\approx", "="] {
            if let Some(stripped) = text.strip_suffix(marker) {
                text = stripped.trim_end();
                break;
            }
        }
    }
    let text = text.trim_start_matches('$').trim();
    let Some(name) = ANGLE_FUNCTIONS.iter().find(|n| text.starts_with(**n)) else {
        return false;
    };
    // No operation outside brackets after the function name.
    let mut depth = 0i32;
    for c in text[name.len()..].chars() {
        match c {
            '(' | '{' | '[' => depth += 1,
            ')' | '}' | ']' => depth -= 1,
            '+' | '-' | '*' | '/' | '^' | '\\' if depth == 0 => return false,
            _ => {}
        }
    }
    true
}

fn number_from(e: &Expr) -> Option<Number> {
    let z = scalar(e).ok()?;
    Some(if z.im.abs() <= 1e-12 * z.re.abs().max(1.0) {
        Number::float(z.re)
    } else {
        Number::complex_float(z.re, z.im)
    })
}

/// The line as it reads under `fmt` (`None` for the plain way).
fn present(lr: &LineResult, fmt: Option<&NumberFormat>) -> Option<Presented> {
    let value = lr.value.as_ref()?;
    let run = |f: &dyn Fn() -> Option<Presented>| match fmt {
        Some(fmt) => with_format(fmt, f),
        None => f(),
    };
    if let Value::Function { params, body } = value {
        return run(&|| {
            Some(Presented {
                text: Some(format!("({}) ↦ {body}", params.join(", "))),
                latex: Some(format!(
                    "({}) \\mapsto {}",
                    params.join(", "),
                    body.to_latex()
                )),
                approx: lr.approx.clone(),
                plain: None,
            })
        });
    }
    let e = value.expr();
    let mode = match &lr.kind {
        LineKind::Query { mode, .. } => Some(*mode),
        _ => None,
    };
    run(&|| shown(lr, &e, mode, fmt))
}

fn shown(
    lr: &LineResult,
    e: &Expr,
    mode: Option<QueryMode>,
    fmt: Option<&NumberFormat>,
) -> Option<Presented> {
    // Angles in degrees.
    let in_degrees = fmt.is_some_and(|f| f.angle == AngleUnit::Degrees)
        && lr.number_format.is_some_and(|s| s.angle);
    if let Some(deg) = to_degrees(e).filter(|_| in_degrees) {
        let approx = (mode.is_some() && !is_exact_rational(&deg))
            .then(|| approximate(&deg).ok().map(|a| format!("{a}°")))
            .flatten();
        return Some(Presented {
            text: Some(format!("{deg}°")),
            latex: Some(format!("{}^\\circ", deg.to_latex())),
            approx,
            plain: None,
        });
    }
    // Units.
    match units_wire::present(e) {
        Ok(Some(p)) => {
            let approx = match mode {
                Some(QueryMode::Approx) => units_wire::approximate(e).ok().flatten(),
                _ => p.approx,
            };
            return Some(Presented {
                text: Some(p.unicode),
                latex: Some(p.latex),
                approx,
                plain: Some(p.text),
            });
        }
        Ok(None) => {}
        Err(_) => return None,
    }
    if let Some((text, latex)) = crate::domains_wire::present_set(e) {
        return Some(Presented {
            text: Some(text),
            latex: Some(latex),
            approx: None,
            plain: None,
        });
    }
    let Some(mode) = mode else {
        // A definition: the value only.
        return Some(Presented {
            text: Some(e.to_string()),
            latex: Some(e.to_latex()),
            approx: lr.approx.clone(),
            plain: None,
        });
    };
    let mut approx = None;
    match mode {
        QueryMode::Exact => {
            if e.is_constant() && !is_exact_rational(e) && !contains_holder(e) {
                approx = approximate(e).ok();
            }
        }
        QueryMode::Approx => approx = approximate(e).ok(),
    }
    let approximate_only = e.as_number().is_some_and(|n| !n.is_exact());
    if approximate_only && approx.is_none() {
        approx = approximate(e).ok();
    }
    let (mut text, mut latex) = (Some(e.to_string()), Some(e.to_latex()));
    // A decimal or polar display of a closed expression that is not a number yet.
    if let Some(f) = fmt {
        let wants_number = f.style == Style::Decimal || f.complex == ComplexForm::Polar;
        if wants_number
            && e.as_number().is_none()
            && e.is_constant()
            && !contains_holder(e)
            && matches!(
                e.node(),
                ExprNode::Add(_)
                    | ExprNode::Mul(_)
                    | ExprNode::Pow(..)
                    | ExprNode::Func(..)
                    | ExprNode::Const(_)
            )
        {
            if let Some(n) = number_from(e) {
                if f.style == Style::Decimal || !n.is_real() {
                    let (t, l) = format_number(&n, f);
                    text = Some(t);
                    latex = Some(l);
                    approx = None;
                }
            }
        }
    }
    if approximate_only && approx.is_some() {
        text = None;
    }
    Some(Presented {
        text,
        latex,
        approx,
        plain: None,
    })
}

/// The part of an answer that can go back into a file: scientific notation as LaTeX.
fn file_safe(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find(" × 10") {
        out.push_str(&rest[..i]);
        let after = &rest[i + " × 10".len()..];
        let sup: String = after
            .chars()
            .take_while(|c| "⁻⁰¹²³⁴⁵⁶⁷⁸⁹".contains(*c))
            .collect();
        let digits: String = sup
            .chars()
            .map(|c| match c {
                '⁻' => '-',
                '⁰' => '0',
                '¹' => '1',
                '²' => '2',
                '³' => '3',
                '⁴' => '4',
                '⁵' => '5',
                '⁶' => '6',
                '⁷' => '7',
                '⁸' => '8',
                '⁹' => '9',
                other => other,
            })
            .collect();
        out.push_str(&format!(" \\times 10^{{{digits}}}"));
        rest = &after[sup.len()..];
    }
    out.push_str(rest);
    out
}

impl LineResult {
    /// True when the line's text is its value shown the usual way, so it can be
    /// shown under another format.
    fn is_derived(&self) -> bool {
        if let Some(s) = &self.number_format {
            return s.derived;
        }
        match present(self, None) {
            Some(p) => {
                p.text == self.plain.clone().or_else(|| self.text.clone())
                    && p.latex == self.latex
                    && (p.approx == self.approx || !matches!(self.kind, LineKind::Query { .. }))
            }
            None => false,
        }
    }

    /// The format the line is shown under now, once a run has applied one.
    pub fn format(&self) -> Option<&NumberFormat> {
        self.number_format.as_ref().map(|s| &s.format)
    }

    /// What the line's text, LaTeX and approximation would be under `fmt`. A line that
    /// is not just its value (a custom presentation) comes back unchanged.
    pub fn reformat_all(
        &self,
        fmt: &NumberFormat,
    ) -> (Option<String>, Option<String>, Option<String>) {
        let (text, latex, approx, _) = self.reformat_full(fmt);
        (text, latex, approx)
    }

    /// [`LineResult::reformat_all`] with the plain form, when the shown text is a
    /// Unicode reading of its own (a quantity with units).
    fn reformat_full(
        &self,
        fmt: &NumberFormat,
    ) -> (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) {
        if self.is_derived() {
            if let Some(p) = present(self, Some(fmt)) {
                return (p.text, p.latex, p.approx, p.plain);
            }
        }
        (
            self.text.clone(),
            self.latex.clone(),
            self.approx.clone(),
            self.plain.clone(),
        )
    }

    /// The text and LaTeX of the result under `fmt`, without changing the line. The
    /// IDE uses it for "exact ↔ decimal, more or fewer digits".
    pub fn reformat(&self, fmt: &NumberFormat) -> (Option<String>, Option<String>) {
        let (text, latex, _) = self.reformat_all(fmt);
        (text, latex)
    }

    /// Shows the line under `fmt` (text, LaTeX and approximation).
    pub fn set_format(&mut self, fmt: &NumberFormat) {
        self.refresh(fmt, None);
    }

    fn refresh(&mut self, fmt: &NumberFormat, angle: Option<bool>) {
        let angle = angle.unwrap_or_else(|| self.number_format.is_some_and(|s| s.angle));
        // Already shown under this format (a block the session reused): nothing to do.
        if self
            .number_format
            .is_some_and(|s| s.format == *fmt && s.angle == angle)
        {
            return;
        }
        let derived = self.is_derived();
        self.number_format = Some(Shown {
            format: *fmt,
            derived,
            angle,
        });
        if derived {
            let (text, latex, approx, plain) = self.reformat_full(fmt);
            self.text = text;
            self.latex = latex;
            if matches!(self.kind, LineKind::Query { .. }) {
                self.approx = approx;
            }
            self.plain = plain;
            self.present_text();
        }
    }

    /// The answer to write into the file when the user keeps it (Ctrl+Enter):
    /// the answer in its plain form (which the parser reads back exactly), with powers
    /// of ten written as `\times 10^{n}`.
    pub fn keep_text(&self) -> Option<String> {
        self.plain_answer().map(|a| file_safe(&a))
    }
}

/// The settings screen's "more digits": one more digit than now.
pub fn more_digits(fmt: &NumberFormat) -> NumberFormat {
    step_digits(fmt, 1)
}

/// The settings screen's "fewer digits": one digit fewer than now.
pub fn fewer_digits(fmt: &NumberFormat) -> NumberFormat {
    step_digits(fmt, -1)
}

fn step_digits(fmt: &NumberFormat, by: i32) -> NumberFormat {
    let mut out = *fmt;
    let step = |n: u32, min: u32| {
        u32::try_from((i64::from(n) + i64::from(by)).clamp(i64::from(min), 50)).unwrap_or(n)
    };
    out.precision = match fmt.precision {
        Precision::Default => Precision::Sig(step(10, 1)),
        Precision::Sig(n) => Precision::Sig(step(n, 1)),
        Precision::Places(p) => Precision::Places(step(p, 0)),
    };
    out
}

/// Switches between exact and decimal display.
pub fn toggle_exact_decimal(fmt: &NumberFormat) -> NumberFormat {
    let mut out = *fmt;
    out.style = match fmt.style {
        Style::Exact => Style::Decimal,
        Style::Decimal => Style::Exact,
    };
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powers_of_ten_go_back_to_latex() {
        assert_eq!(file_safe("6.02 × 10²³"), "6.02 \\times 10^{23}");
        assert_eq!(
            file_safe("= 1/3 ≈ 1.2 × 10⁻⁴"),
            "= 1/3 ≈ 1.2 \\times 10^{-4}"
        );
        assert_eq!(file_safe("x × y"), "x × y");
    }

    #[test]
    fn digit_steps() {
        let d = NumberFormat::default();
        assert_eq!(more_digits(&d).precision, Precision::Sig(11));
        assert_eq!(fewer_digits(&d).precision, Precision::Sig(9));
        let one = NumberFormat {
            precision: Precision::Sig(1),
            ..d
        };
        assert_eq!(fewer_digits(&one).precision, Precision::Sig(1));
        let p = NumberFormat {
            precision: Precision::Places(0),
            ..d
        };
        assert_eq!(fewer_digits(&p).precision, Precision::Places(0));
        assert_eq!(more_digits(&p).precision, Precision::Places(1));
        assert_eq!(toggle_exact_decimal(&d).style, Style::Decimal);
    }
}
