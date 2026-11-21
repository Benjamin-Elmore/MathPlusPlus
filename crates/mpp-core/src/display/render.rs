//! Formatting one number to plain text and LaTeX.

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{Signed, Zero};

use super::{AngleUnit, ComplexForm, Fractions, Notation, NumberFormat, Precision, Style};
use crate::number::{format_f64, rational_sig_parts, rational_to_fixed, Number};

/// How tightly a printed number binds, for parentheses around it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hint {
    /// A plain number.
    Atom,
    /// A negative number.
    Neg,
    /// A fraction, a product or a power of ten: binds like a product.
    Mul,
    /// A sum or a polar form: needs parentheses inside a product.
    Add,
}

/// A real number's body, without its sign.
struct Real {
    neg: bool,
    text: String,
    latex: String,
    /// Written as a product or fraction (not a bare run of digits).
    compound: bool,
    /// Written with `× 10^e`.
    sci: bool,
}

impl Real {
    fn plain(neg: bool, text: String) -> Real {
        Real {
            neg,
            latex: text.clone(),
            text,
            compound: false,
            sci: false,
        }
    }

    fn signed(&self) -> (String, String, Hint) {
        let (s, hint) = if self.neg {
            ("-", Hint::Neg)
        } else {
            ("", if self.compound { Hint::Mul } else { Hint::Atom })
        };
        (
            format!("{s}{}", self.text),
            format!("{s}{}", self.latex),
            hint,
        )
    }
}

/// Plain text and LaTeX of `n` under `fmt`.
pub fn format_number(n: &Number, fmt: &NumberFormat) -> (String, String) {
    let (text, latex, _) = render(n, fmt, 10);
    (text, latex)
}

/// Text, LaTeX and binding strength; `default_sig` is the digit count for
/// [`Precision::Default`].
pub(super) fn render(n: &Number, fmt: &NumberFormat, default_sig: usize) -> (String, String, Hint) {
    match n {
        Number::Rational(q) => real_rational(q, fmt, default_sig).signed(),
        Number::Float(x) => real_float(*x, fmt, default_sig).signed(),
        Number::Complex(..) | Number::ComplexFloat(..) => complex(n, fmt, default_sig),
    }
}

// ---- reals ------------------------------------------------------------------

fn superscript(e: i64) -> String {
    e.to_string()
        .chars()
        .map(|c| match c {
            '-' => '⁻',
            '0' => '⁰',
            '1' => '¹',
            '2' => '²',
            '3' => '³',
            '4' => '⁴',
            '5' => '⁵',
            '6' => '⁶',
            '7' => '⁷',
            '8' => '⁸',
            '9' => '⁹',
            other => other,
        })
        .collect()
}

fn zero_text(fmt: &NumberFormat) -> String {
    match fmt.precision {
        Precision::Places(p) if p > 0 => format!("0.{}", "0".repeat(p as usize)),
        _ => "0".into(),
    }
}

fn real_rational(q: &BigRational, fmt: &NumberFormat, default_sig: usize) -> Real {
    if q.is_zero() {
        let places = matches!(fmt.precision, Precision::Places(_)) && fmt.style == Style::Decimal;
        return Real::plain(false, if places { zero_text(fmt) } else { "0".into() });
    }
    let neg = q.is_negative();
    let a = q.abs();
    let forced = fmt.notation != Notation::Auto;
    let (_, _, k) = rational_sig_parts(&a, 17);
    let outside = k >= i64::from(fmt.sci_high) || k < i64::from(fmt.sci_low);
    if a.is_integer() {
        let decimal_digits = fmt.style == Style::Decimal && fmt.precision != Precision::Default;
        if (forced && outside) || decimal_digits {
            // Exact integers keep every digit unless a precision is asked for.
            let lossless = fmt.precision == Precision::Default;
            return decimal(neg, Src::Q(&a), fmt, default_sig, lossless);
        }
        return Real::plain(neg, a.numer().to_string());
    }
    if fmt.style == Style::Exact && !(forced && outside) {
        return fraction(neg, &a, fmt.fractions);
    }
    decimal(neg, Src::Q(&a), fmt, default_sig, false)
}

fn fraction(neg: bool, a: &BigRational, mode: Fractions) -> Real {
    let (n, d) = (a.numer(), a.denom());
    let whole: BigInt = a.floor().to_integer();
    let (text, latex) = if mode == Fractions::Mixed && !whole.is_zero() {
        let rem = n - &whole * d;
        (
            format!("{whole} {rem}/{d}"),
            format!("{whole}\\frac{{{rem}}}{{{d}}}"),
        )
    } else {
        (format!("{n}/{d}"), format!("\\frac{{{n}}}{{{d}}}"))
    };
    Real {
        neg,
        text,
        latex,
        compound: true,
        sci: false,
    }
}

fn real_float(x: f64, fmt: &NumberFormat, default_sig: usize) -> Real {
    if x.is_nan() || x.is_infinite() {
        let t = format_f64(x, 10);
        let neg = t.starts_with('-');
        return Real::plain(neg, t.trim_start_matches('-').to_string());
    }
    if x == 0.0 {
        return Real::plain(false, zero_text(fmt));
    }
    decimal(x < 0.0, Src::F(x.abs()), fmt, default_sig, false)
}

/// A high-precision decimal (its digits go past the ones shown) under `fmt`.
pub(super) fn render_decimal(
    d: &crate::hp::Decimal,
    fmt: &NumberFormat,
    default_sig: usize,
) -> (String, String, Hint) {
    decimal(d.neg, Src::D(d), fmt, default_sig, false).signed()
}

/// The magnitude being formatted.
#[derive(Clone, Copy)]
enum Src<'a> {
    Q(&'a BigRational),
    F(f64),
    /// Decimal digits from a high-precision evaluation (sign ignored).
    D(&'a crate::hp::Decimal),
}

impl Src<'_> {
    /// Exactly `sig` significant digits and the exponent of the first.
    fn sig_parts(&self, sig: usize) -> (String, i64) {
        match self {
            Src::Q(q) => {
                let (_, digits, k) = rational_sig_parts(q, sig);
                (digits, k)
            }
            Src::F(x) => {
                let used = sig.clamp(1, 17);
                let text = format!("{:.*e}", used - 1, x);
                let (mantissa, exp) = text.split_once('e').unwrap_or((&text, "0"));
                let mut digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
                while digits.len() < sig {
                    digits.push('0');
                }
                (digits, exp.parse().unwrap_or(0))
            }
            Src::D(d) => {
                let r = d.rounded(sig);
                (r.digits, r.k)
            }
        }
    }

    fn to_ratio(self) -> Option<BigRational> {
        match self {
            Src::Q(q) => Some(q.clone()),
            Src::F(x) => BigRational::from_float(x),
            Src::D(d) => d.to_ratio().map(|q| q.abs()),
        }
    }
}

/// Puts the point after `int_len` digits (which may be ≤ 0 or beyond the digits).
fn place(digits: &str, int_len: i64, keep_zeros: bool) -> String {
    let len = digits.len() as i64;
    let mut s = if int_len <= 0 {
        format!(
            "0.{}{digits}",
            "0".repeat(usize::try_from(-int_len).unwrap_or(0))
        )
    } else if int_len >= len {
        format!(
            "{digits}{}",
            "0".repeat(usize::try_from(int_len - len).unwrap_or(0))
        )
    } else {
        let at = usize::try_from(int_len).unwrap_or(0);
        format!("{}.{}", &digits[..at], &digits[at..])
    };
    if !keep_zeros && s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    s
}

fn decimal(
    neg: bool,
    src: Src<'_>,
    fmt: &NumberFormat,
    default_sig: usize,
    lossless: bool,
) -> Real {
    let keep = fmt.precision != Precision::Default;
    let places_mode = match fmt.precision {
        Precision::Places(p) => Some(p as usize),
        _ => None,
    };
    let sig = match fmt.precision {
        Precision::Default => default_sig,
        Precision::Sig(n) => n as usize,
        Precision::Places(_) => 17,
    };
    let (probe, k0) = src.sig_parts(sig.max(1));
    let high = i64::from(fmt.sci_high);
    let low = i64::from(fmt.sci_low);
    let sci = match (fmt.notation, places_mode) {
        (Notation::Auto, Some(_)) => k0 >= high,
        _ => k0 >= high || k0 < low,
    };
    if sci {
        let (digits, k) = match places_mode {
            Some(p) => src.sig_parts(p + 1),
            None if lossless => {
                let all = match src {
                    Src::Q(q) => q.numer().to_string(),
                    Src::F(_) | Src::D(_) => probe.clone(),
                };
                let trimmed = all.trim_end_matches('0');
                let k = i64::try_from(all.len()).unwrap_or(1) - 1;
                (
                    if trimmed.is_empty() { "0" } else { trimmed }.to_string(),
                    k,
                )
            }
            None => (probe, k0),
        };
        let e = if fmt.notation == Notation::Engineering {
            k.div_euclid(3) * 3
        } else {
            k
        };
        let mantissa = place(&digits, k - e + 1, keep);
        return Real {
            neg,
            text: format!("{mantissa} × 10{}", superscript(e)),
            latex: format!("{mantissa} \\times 10^{{{e}}}"),
            compound: true,
            sci: true,
        };
    }
    let body = match places_mode {
        Some(p) => match src.to_ratio() {
            Some(q) => rational_to_fixed(&q, p),
            None => place(&probe, k0 + 1, true),
        },
        None => place(&probe, k0 + 1, keep),
    };
    let body = body.trim_start_matches('-').to_string();
    let zero = body.chars().all(|c| c == '0' || c == '.');
    Real::plain(neg && !zero, body)
}

// ---- complex ----------------------------------------------------------------

fn complex(n: &Number, fmt: &NumberFormat, default_sig: usize) -> (String, String, Hint) {
    if fmt.complex == ComplexForm::Polar {
        return polar(n, fmt, default_sig);
    }
    let re = n.re();
    let im = n.im();
    let im_neg = im.is_negative();
    let mag = im.abs_real().unwrap_or_else(|| im.clone());
    let imag = if mag.is_one() && mag.is_exact() {
        ("i".to_string(), "i".to_string())
    } else {
        let part = match &mag {
            Number::Rational(q) => real_rational(q, fmt, default_sig),
            Number::Float(x) => real_float(*x, fmt, default_sig),
            _ => Real::plain(false, mag.to_string()),
        };
        let text = if part.compound {
            format!("({})*i", part.text)
        } else {
            format!("{}*i", part.text)
        };
        let latex = if part.sci {
            format!("\\left({}\\right) i", part.latex)
        } else {
            format!("{} i", part.latex)
        };
        (text, latex)
    };
    if re.is_zero() {
        let s = if im_neg { "-" } else { "" };
        let hint = if im_neg { Hint::Neg } else { Hint::Mul };
        return (format!("{s}{}", imag.0), format!("{s}{}", imag.1), hint);
    }
    let (rt, rl, _) = render(&re, fmt, default_sig);
    let sign = if im_neg { "-" } else { "+" };
    (
        format!("{rt} {sign} {}", imag.0),
        format!("{rl} {sign} {}", imag.1),
        Hint::Add,
    )
}

fn polar(n: &Number, fmt: &NumberFormat, default_sig: usize) -> (String, String, Hint) {
    let z = n.to_complex64();
    let r = real_float(z.norm(), fmt, default_sig).signed();
    if fmt.angle == AngleUnit::Degrees {
        let deg = real_float(z.arg().to_degrees(), fmt, default_sig).signed();
        return (
            format!("{}∠{}°", r.0, deg.0),
            format!("{}\\angle {}^\\circ", r.1, deg.1),
            Hint::Add,
        );
    }
    let t = real_float(z.arg(), fmt, default_sig);
    let sign = if t.neg { "-" } else { "" };
    (
        format!("{}∠{}", r.0, t.signed().0),
        format!("{} e^{{{sign}{} i}}", r.1, t.latex),
        Hint::Add,
    )
}
