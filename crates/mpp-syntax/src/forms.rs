//! The input forms table: each construct the editor understands, in every way
//! people type it.
//!
//! Mathematicians type LaTeX (`\frac{a}{b}`, `\le`), coders type code
//! (`a/b`, `<=`, `sqrt(x)`), Word users type UnicodeMath (`√(x)`), and anyone
//! may paste Unicode (`≤`). The parser reads all of them (a test checks every
//! row); the editor converts the code and LaTeX forms as they are typed into
//! what the file keeps (the `writes` column); the cheat sheet shows the rows
//! side by side. Greek names (`alpha` → α) follow the symbol table
//! ([`crate::symbols`]) and are not repeated here, apart from one example row.

/// When the editor converts what was typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// When the token ends: a Space, an operator, a bracket or any character
    /// that cannot continue it (`<=` then a space gives `≤ `).
    Delimiter,
    /// When the closing bracket of the call is typed (`sqrt(x)` gives `\sqrt{x}`).
    Call,
    /// As soon as it is typed: a box opens (`^`, `**`, `/`) or a comment starts (`#`).
    Immediate,
    /// Never: what was typed is already what the file keeps.
    Kept,
}

/// Who mostly types the code column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Audience {
    Everyone,
    Mathematicians,
    Coders,
}

/// One row of the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Form {
    /// A short stable id (`frac`, `le` …).
    pub id: &'static str,
    /// What it is, in words.
    pub name: &'static str,
    /// What a LaTeX user types.
    pub latex: &'static str,
    /// What a coder types.
    pub code: &'static str,
    /// Other spellings a coder may use (`x^2` for `x**2`).
    pub also: &'static [&'static str],
    /// The same in Unicode characters.
    pub unicode: &'static str,
    /// The UnicodeMath linear form (what Word users type).
    pub unicodemath: &'static str,
    /// LaTeX for the typeset picture.
    pub picture: &'static str,
    /// What the editor writes in the file once `code` is typed.
    pub writes: &'static str,
    pub trigger: Trigger,
    pub audience: Audience,
    /// Every form reads as the same tree. False only where the forms mean the
    /// same thing in different notation (`exp(x)` and `e^{x}`).
    pub same_tree: bool,
}

const fn row(
    id: &'static str,
    name: &'static str,
    latex: &'static str,
    code: &'static str,
    unicode: &'static str,
    writes: &'static str,
    trigger: Trigger,
) -> Form {
    Form {
        id,
        name,
        latex,
        code,
        also: &[],
        unicode,
        unicodemath: unicode,
        picture: latex,
        writes,
        trigger,
        audience: Audience::Everyone,
        same_tree: true,
    }
}

const fn also(mut f: Form, also: &'static [&'static str]) -> Form {
    f.also = also;
    f
}

const fn umath(mut f: Form, unicodemath: &'static str) -> Form {
    f.unicodemath = unicodemath;
    f
}

const fn coders(mut f: Form) -> Form {
    f.audience = Audience::Coders;
    f
}

const fn maths(mut f: Form) -> Form {
    f.audience = Audience::Mathematicians;
    f
}

const fn notation(mut f: Form) -> Form {
    f.same_tree = false;
    f
}

use Trigger::{Call, Delimiter, Immediate, Kept};

static FORMS: &[Form] = &[
    // ----- structures -----
    umath(
        row(
            "frac",
            "Fraction",
            r"\frac{a}{b}",
            "a/b",
            "a/b",
            r"\frac{a}{b}",
            Immediate,
        ),
        "a/b",
    ),
    also(
        row("pow", "Power", r"x^{2}", "x**2", "x²", r"x^{2}", Immediate),
        &["x^2"],
    ),
    row(
        "sub",
        "Subscript",
        r"x_{n}",
        "x_n",
        "xₙ",
        r"x_{n}",
        Immediate,
    ),
    umath(
        row(
            "sqrt",
            "Square root",
            r"\sqrt{x}",
            "sqrt(x)",
            "√x",
            r"\sqrt{x}",
            Call,
        ),
        r"\sqrt(x)",
    ),
    row(
        "abs",
        "Absolute value",
        r"\left| x \right|",
        "abs(x)",
        "|x|",
        "|x|",
        Call,
    ),
    notation(row(
        "exp",
        "Exponential",
        r"e^{x}",
        "exp(x)",
        "exp(x)",
        r"e^{x}",
        Call,
    )),
    notation(row(
        "log",
        "Logarithm to a base",
        r"\log_{b}(x)",
        "log(x, b)",
        "log_b(x)",
        r"\log_{b}(x)",
        Call,
    )),
    umath(
        row(
            "sum",
            "Sum",
            r"\sum_{n=1}^{10} f",
            "sum(f, n, 1, 10)",
            "∑ₙ₌₁¹⁰ f",
            r"\sum_{n=1}^{10} f",
            Call,
        ),
        "∑_(n=1)^10 f",
    ),
    umath(
        row(
            "int",
            "Definite integral",
            r"\int_{0}^{1} f \, dx",
            "int(f, x, 0, 1)",
            "∫₀¹ f dx",
            r"\int_{0}^{1} f \, dx",
            Call,
        ),
        "∫_0^1 f dx",
    ),
    row(
        "int.indef",
        "Integral",
        r"\int f \, dx",
        "int(f, x)",
        "∫ f dx",
        r"\int f \, dx",
        Call,
    ),
    row(
        "diff",
        "Derivative",
        r"\frac{d}{dx} f",
        "diff(f, x)",
        "d/dx f",
        r"\frac{d}{dx} f",
        Call,
    ),
    row("prime", "Derivative (prime)", "x'", "x'", "x′", "x'", Kept),
    coders(row(
        "list",
        "Vector or list",
        "[1, 2, 3]",
        "[1, 2, 3]",
        "[1, 2, 3]",
        "[1, 2, 3]",
        Kept,
    )),
    // ----- operators and relations -----
    also(
        row(
            "cdot",
            "Times",
            r"a \cdot b",
            "a*b",
            "a·b",
            "a·b",
            Delimiter,
        ),
        &["a * b"],
    ),
    row(
        "le",
        "Less or equal",
        r"a \le b",
        "a <= b",
        "a ≤ b",
        "a ≤ b",
        Delimiter,
    ),
    row(
        "ge",
        "Greater or equal",
        r"a \ge b",
        "a >= b",
        "a ≥ b",
        "a ≥ b",
        Delimiter,
    ),
    row(
        "ne",
        "Not equal",
        r"a \ne b",
        "a != b",
        "a ≠ b",
        "a ≠ b",
        Delimiter,
    ),
    row(
        "to", "Tends to", r"x \to 0", "x -> 0", "x → 0", "x → 0", Delimiter,
    ),
    row(
        "implies",
        "Implies",
        r"p \implies q",
        "p => q",
        "p ⇒ q",
        "p ⇒ q",
        Delimiter,
    ),
    row(
        "iff",
        "If and only if",
        r"p \iff q",
        "p <=> q",
        "p ⇔ q",
        "p ⇔ q",
        Delimiter,
    ),
    // ----- constants and letters -----
    row("pi", "Pi", r"\pi", "pi", "π", "π", Delimiter),
    row("inf", "Infinity", r"\infty", "inf", "∞", "∞", Delimiter),
    maths(row(
        "alpha",
        "Greek letter",
        r"\alpha",
        "alpha",
        "α",
        "α",
        Delimiter,
    )),
    // ----- comments -----
    coders(also(
        row(
            "comment",
            "Comment on a maths line",
            r"x + 1 % note",
            "x + 1 # note",
            "x + 1 % note",
            "x + 1 % note",
            Immediate,
        ),
        &["x + 1 // note"],
    )),
];

/// Every row of the table, in the order the cheat sheet shows them.
pub fn forms() -> &'static [Form] {
    FORMS
}

/// The row with this id.
pub fn form(id: &str) -> Option<&'static Form> {
    FORMS.iter().find(|f| f.id == id)
}

/// The operators a coder types and the character the editor writes for each.
/// Longest first, so `<=>` is found before `<=`.
pub const CODE_OPERATORS: &[(&str, &str)] = &[
    ("<=>", "⇔"),
    ("<=", "≤"),
    (">=", "≥"),
    ("!=", "≠"),
    ("->", "→"),
    ("=>", "⇒"),
    ("*", "·"),
];

/// The characters a code operator is made of.
pub fn is_operator_char(c: char) -> bool {
    matches!(c, '<' | '>' | '=' | '!' | '-' | '*')
}

/// The calls the editor converts when their closing bracket is typed.
pub const CALLS: &[&str] = &["sqrt", "abs", "exp", "log", "sum", "prod", "int", "diff"];

/// The file text for a coder's call (`sqrt` with `["x"]` gives `\sqrt{x}`), or
/// `None` when the call has the wrong number of arguments.
pub fn call_to_latex(name: &str, args: &[&str]) -> Option<String> {
    let a: Vec<&str> = args.iter().map(|s| s.trim()).collect();
    if a.iter().any(|s| s.is_empty()) {
        return None;
    }
    let is_name = |s: &str| {
        let mut cs = s.chars();
        cs.next().is_some_and(char::is_alphabetic)
            && cs.all(|c| c.is_alphanumeric() || c == '_')
            && !s.contains("__")
    };
    let out = match (name, a.as_slice()) {
        ("sqrt", [x]) => format!(r"\sqrt{{{x}}}"),
        ("abs", [x]) => format!("|{x}|"),
        ("exp", [x]) => format!("e^{{{x}}}"),
        ("log", [x]) => format!(r"\log({x})"),
        ("log", [x, b]) => format!(r"\log_{{{b}}}({x})"),
        ("sum" | "prod", [f, n, from, to]) if is_name(n) => {
            format!(r"\{name}_{{{n}={from}}}^{{{to}}} {}", term(f))
        }
        ("int", [f, x]) if is_name(x) => format!(r"\int {f} \, d{x}"),
        ("int", [f, x, lo, hi]) if is_name(x) => {
            format!(r"\int_{{{lo}}}^{{{hi}}} {f} \, d{x}")
        }
        ("diff", [f, x]) if is_name(x) => format!(r"\frac{{d}}{{d{x}}} {}", term(f)),
        ("diff", [f, x, n]) if is_name(x) => {
            format!(r"\frac{{d^{{{n}}}}}{{d{x}^{{{n}}}}} {}", term(f))
        }
        _ => return None,
    };
    Some(out)
}

/// A summand or function written after `\sum` or `\frac{d}{dx}`: in brackets
/// when it is a sum itself (those take only the next term).
fn term(f: &str) -> String {
    if has_top_level_sum(f) {
        format!(r"\left({f}\right)")
    } else {
        f.to_string()
    }
}

/// True when `s` has a `+` or `−` outside every bracket (not as a leading sign).
pub fn has_top_level_sum(s: &str) -> bool {
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            '+' | '-' | '−' | '±' if depth == 0 && !s[..i].trim().is_empty() => return true,
            _ => {}
        }
    }
    false
}

/// Splits call arguments at the commas outside every bracket.
pub fn split_args(s: &str) -> Vec<&str> {
    let mut out = vec![];
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_calls_convert() {
        for (i, f) in FORMS.iter().enumerate() {
            assert!(FORMS[..i].iter().all(|g| g.id != f.id), "{} twice", f.id);
        }
        assert_eq!(call_to_latex("sqrt", &["x"]).as_deref(), Some(r"\sqrt{x}"));
        assert_eq!(
            call_to_latex("sum", &["n+1", " n", " 1", " 10"]).as_deref(),
            Some(r"\sum_{n=1}^{10} \left(n+1\right)")
        );
        assert_eq!(call_to_latex("sum", &["f", "n"]), None);
        assert_eq!(call_to_latex("sqrt", &[""]), None);
        assert_eq!(
            split_args("f(a, b), x, {1, 2}"),
            ["f(a, b)", " x", " {1, 2}"]
        );
    }
}
