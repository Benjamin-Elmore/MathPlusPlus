//! Number display options: exact or decimal, significant figures or decimal places,
//! scientific and engineering notation, mixed fractions, degrees and polar form.
//!
//! A [`NumberFormat`] says how numbers are shown. [`format_number`] formats one
//! [`Number`] to plain text and LaTeX. [`with_format`] makes a format the active one
//! for the current thread: while it runs, every expression printed with `Display` or
//! `to_latex`, and every numeric answer from the evaluator, uses it. Nothing is
//! active by default, so the plain printers are unchanged.
//!
//! Plain text never shows `6.02e23`: it shows `6.02 × 10²³`. LaTeX shows
//! `6.02 \times 10^{23}`.

mod render;

use std::cell::Cell;

use num_complex::Complex64;

use crate::expr::{Expr, ExprNode};
use crate::number::Number;

pub use render::{format_number, Hint};

/// Exact values kept exact, or everything as a decimal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// Fractions and integers stay exact (decimals only where the value is approximate).
    Exact,
    /// Every number is shown as a decimal.
    Decimal,
}

/// How many digits a decimal shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Precision {
    /// Ten significant figures, trailing zeros trimmed.
    Default,
    /// This many significant figures, trailing zeros kept.
    Sig(u32),
    /// This many digits after the point, trailing zeros kept.
    Places(u32),
}

/// When a decimal is written as `m × 10^e`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notation {
    /// Plain digits, except for very large and very small numbers.
    Auto,
    /// `m × 10^e` with `1 ≤ |m| < 10` outside the thresholds.
    Scientific,
    /// `m × 10^e` with `e` a multiple of 3 outside the thresholds.
    Engineering,
}

/// How a fraction larger than one is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fractions {
    /// `7/2`.
    Improper,
    /// `3 1/2`.
    Mixed,
}

/// The unit angle results are shown in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AngleUnit {
    Radians,
    Degrees,
}

/// How complex numbers are written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComplexForm {
    /// `a + bi`.
    Rectangular,
    /// `r∠θ` in text, `r e^{iθ}` in LaTeX.
    Polar,
}

/// Every display option.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NumberFormat {
    pub style: Style,
    pub precision: Precision,
    pub notation: Notation,
    /// Scientific form from `10^sci_high` up (the exponent of the leading digit).
    pub sci_high: i32,
    /// Scientific form below `10^sci_low`.
    pub sci_low: i32,
    pub fractions: Fractions,
    pub angle: AngleUnit,
    pub complex: ComplexForm,
}

impl Default for NumberFormat {
    fn default() -> Self {
        NumberFormat {
            style: Style::Exact,
            precision: Precision::Default,
            notation: Notation::Auto,
            sci_high: 21,
            sci_low: -7,
            fractions: Fractions::Improper,
            angle: AngleUnit::Radians,
            complex: ComplexForm::Rectangular,
        }
    }
}

impl NumberFormat {
    /// Scientific notation from `10^high` up and below `10^-low` (`%sci`).
    pub fn scientific(mut self, high: i32, low: i32) -> Self {
        self.notation = Notation::Scientific;
        self.sci_high = high;
        self.sci_low = -low;
        self
    }

    /// Engineering notation with the same thresholds (`%eng`).
    pub fn engineering(mut self, high: i32, low: i32) -> Self {
        self.notation = Notation::Engineering;
        self.sci_high = high;
        self.sci_low = -low;
        self
    }

    /// Plain digits again and the default ten significant figures.
    pub fn plain(mut self) -> Self {
        let d = NumberFormat::default();
        self.notation = d.notation;
        self.sci_high = d.sci_high;
        self.sci_low = d.sci_low;
        self.precision = d.precision;
        self
    }

    /// True when exact numbers print the way the plain printers print them.
    pub(crate) fn exact_is_plain(&self) -> bool {
        self.style == Style::Exact
            && self.notation == Notation::Auto
            && self.fractions == Fractions::Improper
            && self.complex == ComplexForm::Rectangular
    }
}

thread_local! {
    static ACTIVE: Cell<Option<NumberFormat>> = const { Cell::new(None) };
}

struct Restore(Option<NumberFormat>);

impl Drop for Restore {
    fn drop(&mut self) {
        ACTIVE.with(|a| a.set(self.0));
    }
}

/// Runs `f` with `fmt` as the active format of this thread.
pub fn with_format<T>(fmt: &NumberFormat, f: impl FnOnce() -> T) -> T {
    let _restore = Restore(ACTIVE.with(|a| a.replace(Some(*fmt))));
    f()
}

/// The active format, when one is set.
pub fn active() -> Option<NumberFormat> {
    ACTIVE.with(Cell::get)
}

/// The hook the printers call: `None` means "print it the usual way".
pub(crate) fn render_active(n: &Number, latex: bool) -> Option<(String, Hint)> {
    let fmt = active()?;
    if matches!(n, Number::Rational(_) | Number::Complex(..)) && fmt.exact_is_plain() {
        return None;
    }
    let (text, tex, hint) = render::render(n, &fmt, 10);
    Some((if latex { tex } else { text }, hint))
}

/// A numeric answer as text (`≈ 3.141592654`) under the active format, with
/// `digits` significant figures unless the format asks for others. `None` when no
/// format is active.
pub fn format_approx(z: Complex64, digits: usize) -> Option<String> {
    let fmt = active()?;
    let n = if z.im.abs() <= 1e-12 * z.re.abs().max(1.0) {
        Number::float(z.re)
    } else {
        Number::complex_float(z.re, z.im)
    };
    Some(render::render(&n, &fmt, digits.max(1)).0)
}

/// A high-precision decimal as text under the active format, with `digits`
/// significant figures unless the format asks for others. `None` when no format is set.
pub fn format_decimal(d: &crate::hp::Decimal, digits: usize) -> Option<String> {
    let fmt = active()?;
    Some(render::render_decimal(d, &fmt, digits.max(1)).0)
}

/// How many significant digits the active format shows for a value whose first digit
/// is at `10^k` (`digits` when no format asks for more).
pub fn digits_needed(k: i64, digits: usize) -> usize {
    match active().map(|f| f.precision) {
        Some(Precision::Sig(n)) => n as usize,
        Some(Precision::Places(p)) => usize::try_from(k + 1 + i64::from(p)).unwrap_or(0).max(1),
        _ => digits,
    }
}

/// `e` (an angle in radians) as degrees: an exact number when `e` is a rational
/// multiple of π, otherwise a decimal. `None` unless `e` is a closed expression.
pub fn to_degrees(e: &Expr) -> Option<Expr> {
    if !e.is_constant() {
        return None;
    }
    let exact = (e * &Expr::int(180) / Expr::pi()).simplify();
    if matches!(exact.node(), ExprNode::Num(_)) {
        return Some(exact);
    }
    let x = e.eval_f64(&crate::eval::EmptyEnv).ok()?;
    x.is_finite()
        .then(|| Expr::new(ExprNode::Num(Number::float(x.to_degrees()))))
}

#[cfg(test)]
mod tests;
