//! The input forms table as the editor uses it: the rows for the cheat sheet,
//! and maths turned into each form for Copy ("as LaTeX", "as code").
//!
//! The table itself lives in [`mpp_syntax::forms`] so the parser's tests read
//! the same rows; typing conversions are in [`super::convert`].

use mpp_syntax::forms::{call_to_latex, split_args, CALLS, CODE_OPERATORS};
pub use mpp_syntax::forms::{Audience, Form, Trigger};
use mpp_syntax::symbols::{
    lookup_char, lookup_latex, lookup_name, subscript_value, superscript_value, SymbolKind,
};

/// Every row of the input forms table, in cheat sheet order (for U23: LaTeX,
/// code, Unicode and UnicodeMath forms, and the picture to typeset).
pub fn table() -> &'static [Form] {
    mpp_syntax::forms::forms()
}

/// Maths source in any form, written as LaTeX: Unicode symbols become their
/// commands (`α` → `\alpha`), code becomes LaTeX (`sqrt(x)` → `\sqrt{x}`,
/// `<=` → `\le`, `x**2` → `x^2`, `# note` → `% note`).
pub fn to_latex(src: &str) -> String {
    let src = calls_to_latex(src);
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        let rest: String = chars[i..chars.len().min(i + 3)].iter().collect();
        if c == '\\' {
            // A command (or an escaped character) is copied as it is.
            out.push(c);
            i += 1;
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                out.push(chars[i]);
                i += 1;
            }
            if i < chars.len() && out.ends_with('\\') {
                out.push(chars[i]);
                i += 1;
            }
            continue;
        }
        if c == '%' || c == '#' || (c == '/' && next == Some('/')) {
            let skip = if c == '/' { 2 } else { 1 };
            out.push('%');
            out.extend(&chars[i + skip..]);
            break;
        }
        if c == '*' && next == Some('*') {
            out.push('^');
            i += 2;
            continue;
        }
        if let Some((op, to)) = CODE_OPERATORS.iter().find(|(op, _)| rest.starts_with(op)) {
            let latex = to
                .chars()
                .next()
                .and_then(lookup_char)
                .map_or("", |s| s.latex);
            out.push_str(&format!(" {latex} "));
            i += op.chars().count();
            continue;
        }
        if c.is_ascii_alphabetic() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            out.push_str(&word_to_latex(&word));
            continue;
        }
        if superscript_value(c).is_some() || subscript_value(c).is_some() {
            let sup = superscript_value(c).is_some();
            let mut run = String::new();
            while let Some(v) = chars.get(i).and_then(|&c| {
                if sup {
                    superscript_value(c)
                } else {
                    subscript_value(c)
                }
            }) {
                run.push(v);
                i += 1;
            }
            out.push_str(&format!("{}{{{run}}}", if sup { '^' } else { '_' }));
            continue;
        }
        i += 1;
        match c {
            '−' => out.push('-'),
            '√' => out.push_str(r"\sqrt"),
            '′' => out.push('\''),
            c if !c.is_ascii() => match lookup_char(c) {
                Some(s) if !s.latex.is_empty() => {
                    out.push_str(s.latex);
                    if chars.get(i).is_some_and(|n| n.is_ascii_alphanumeric()) {
                        out.push(' ');
                    }
                }
                _ => out.push(c),
            },
            c => out.push(c),
        }
    }
    tidy(&out)
}

/// A word of code as LaTeX: `alpha` → `\alpha`, `sin` → `\sin`, `inf` → `\infty`.
fn word_to_latex(word: &str) -> String {
    if word == "inf" {
        return r"\infty ".to_string();
    }
    match lookup_name(word) {
        Some(s)
            if word.len() > 1 && matches!(s.kind, SymbolKind::Letter | SymbolKind::Function) =>
        {
            format!(r"\{word} ")
        }
        _ => word.to_string(),
    }
}

/// The code calls in `src` (`sqrt(x)`, `sum(f, n, 1, 10)`) written as LaTeX,
/// innermost first.
fn calls_to_latex(src: &str) -> String {
    let mut s = src.to_string();
    for _ in 0..64 {
        let Some((range, latex)) = innermost_call(&s) else {
            break;
        };
        s.replace_range(range, &latex);
    }
    s
}

/// The last call (so the innermost of nested ones) that converts.
fn innermost_call(s: &str) -> Option<(std::ops::Range<usize>, String)> {
    let mut found = None;
    for name in CALLS {
        for (i, _) in s.match_indices(&format!("{name}(")) {
            let before = s[..i].chars().next_back();
            if before.is_some_and(|c| c.is_alphanumeric() || c == '\\') {
                continue;
            }
            let open = i + name.len();
            let Some(close) = close_paren(s, open) else {
                continue;
            };
            let inner = &s[open + 1..close];
            if CALLS.iter().any(|n| inner.contains(&format!("{n}("))) {
                continue;
            }
            if let Some(latex) = call_to_latex(name, &split_args(inner)) {
                if found
                    .as_ref()
                    .is_none_or(|(r, _): &(std::ops::Range<usize>, String)| i > r.start)
                {
                    found = Some((i..close + 1, latex));
                }
            }
        }
    }
    found
}

fn close_paren(s: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in s[open..].char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return (c == ')').then_some(open + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Single spaces, none at the ends of lines.
fn tidy(s: &str) -> String {
    s.split('\n')
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Maths source in any form, written as code a programmer would type:
/// `\frac{a}{b}` → `a/b`, `x^{2}` → `x**2`, `\sqrt{x}` → `sqrt(x)`,
/// `\int_{0}^{1} f \, dx` → `int(f, x, 0, 1)`, `\le` → `<=`, `α` → `alpha`.
pub fn to_code(src: &str) -> String {
    let latex = to_latex(src);
    let chars: Vec<char> = latex.chars().collect();
    let mut w = Walk { c: &chars, i: 0 };
    tidy(&w.seq(false, false))
}

struct Walk<'a> {
    c: &'a [char],
    i: usize,
}

/// Wraps code in brackets unless it is one name or number.
fn wrap(s: &str) -> String {
    let s = s.trim();
    let simple = !s.is_empty()
        && s.chars()
            .all(|c| c.is_alphanumeric() || c == '.' || c == '_');
    let bracketed = s.starts_with('(') && close_paren(s, 0) == Some(s.len() - 1);
    if simple || bracketed {
        s.to_string()
    } else {
        format!("({s})")
    }
}

impl Walk<'_> {
    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }

    /// Code up to the end of the group (`in_group`) or, for `term`, up to the
    /// next `+ − = < > ,` outside brackets (the term after `\sum`).
    fn seq(&mut self, in_group: bool, term: bool) -> String {
        let mut out = String::new();
        let mut depth = 0usize;
        while let Some(ch) = self.peek() {
            if term && depth == 0 && !out.trim().is_empty() {
                if matches!(ch, '+' | '-' | '=' | '<' | '>' | ',' | '&') {
                    break;
                }
                if ch == '\\' && self.relation_ahead() {
                    break;
                }
            }
            match ch {
                '}' if in_group => break,
                ')' | ']' if term && depth == 0 => break,
                '{' => {
                    self.i += 1;
                    let inner = self.seq(true, false);
                    self.eat('}');
                    out.push_str(&inner);
                }
                '}' => self.i += 1,
                '(' | '[' => {
                    depth += 1;
                    out.push(ch);
                    self.i += 1;
                }
                ')' | ']' => {
                    depth = depth.saturating_sub(1);
                    out.push(ch);
                    self.i += 1;
                }
                '\\' => self.command(&mut out),
                '^' => {
                    self.i += 1;
                    let a = self.argument();
                    out.push_str(&format!("**{}", wrap(&a)));
                }
                '_' => {
                    self.i += 1;
                    let a = self.argument();
                    out.push_str(&format!("_{}", wrap(&a)));
                }
                '%' => {
                    out.push('#');
                    self.i += 1;
                }
                _ => {
                    out.push(ch);
                    self.i += 1;
                }
            }
        }
        out
    }

    /// True when a relation command (`\le`, `\to` …) starts here.
    fn relation_ahead(&self) -> bool {
        let name: String = self.c[self.i + 1..]
            .iter()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect();
        lookup_latex(&name).is_some_and(|s| {
            matches!(
                s.kind,
                SymbolKind::Relation | SymbolKind::Arrow | SymbolKind::Logic
            )
        })
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn skip_spaces(&mut self) {
        while self.peek() == Some(' ') {
            self.i += 1;
        }
    }

    /// One argument: a braced group or a single token.
    fn argument(&mut self) -> String {
        self.skip_spaces();
        match self.peek() {
            Some('{') => {
                self.i += 1;
                let s = self.seq(true, false);
                self.eat('}');
                s
            }
            Some('\\') => {
                let mut s = String::new();
                self.command(&mut s);
                s
            }
            Some(c) => {
                self.i += 1;
                c.to_string()
            }
            None => String::new(),
        }
    }

    /// The raw text of a braced argument, without converting it.
    fn raw_argument(&mut self) -> String {
        self.skip_spaces();
        if self.peek() != Some('{') {
            return self
                .peek()
                .map(|c| {
                    self.i += 1;
                    c.to_string()
                })
                .unwrap_or_default();
        }
        let start = self.i + 1;
        let mut depth = 0usize;
        while let Some(c) = self.peek() {
            self.i += 1;
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return self.c[start..self.i - 1].iter().collect();
                    }
                }
                _ => {}
            }
        }
        self.c[start..].iter().collect()
    }

    /// `_lower` and `^upper` after a big operator, in either order.
    fn limits(&mut self) -> (Option<String>, Option<String>) {
        let (mut lo, mut hi) = (None, None);
        loop {
            self.skip_spaces();
            match self.peek() {
                Some('_') if lo.is_none() => {
                    self.i += 1;
                    lo = Some(self.argument());
                }
                Some('^') if hi.is_none() => {
                    self.i += 1;
                    hi = Some(self.argument());
                }
                _ => break,
            }
        }
        (lo, hi)
    }

    fn name(&mut self) -> String {
        let mut name = String::new();
        while let Some(c) = self.peek().filter(char::is_ascii_alphabetic) {
            name.push(c);
            self.i += 1;
        }
        name
    }

    fn command(&mut self, out: &mut String) {
        self.i += 1;
        let Some(first) = self.peek() else {
            return;
        };
        if !first.is_ascii_alphabetic() {
            self.i += 1;
            match first {
                ',' | ';' | ':' | ' ' | '!' => out.push(' '),
                '\\' => out.push('\n'),
                c => out.push(c),
            }
            return;
        }
        let name = self.name();
        match name.as_str() {
            "frac" | "dfrac" | "tfrac" => {
                let a = self.raw_argument();
                let b = self.raw_argument();
                let var = b.strip_prefix('d').filter(|v| {
                    !v.is_empty() && v.chars().all(|c| c.is_alphanumeric() || c == '\\')
                });
                match (a.trim(), var) {
                    ("d", Some(v)) => {
                        self.skip_spaces();
                        let body = self.seq(false, true);
                        out.push_str(&format!("diff({}, {}) ", strip_brackets(&body), to_code(v)));
                    }
                    _ => out.push_str(&format!("{}/{}", wrap(&to_code(&a)), wrap(&to_code(&b)))),
                }
            }
            "sqrt" => {
                self.skip_spaces();
                if self.eat('[') {
                    let mut n = String::new();
                    while let Some(c) = self.peek().filter(|&c| c != ']') {
                        n.push(c);
                        self.i += 1;
                    }
                    self.eat(']');
                    let x = self.argument();
                    out.push_str(&format!("root({}, {})", n.trim(), x.trim()));
                } else {
                    let x = self.argument();
                    out.push_str(&format!("sqrt({})", x.trim()));
                }
            }
            "int" => {
                let (lo, hi) = self.limits();
                let (body, var) = self.integrand();
                let mut args = vec![body, var];
                if let (Some(lo), Some(hi)) = (lo, hi) {
                    args.push(lo.trim().to_string());
                    args.push(hi.trim().to_string());
                }
                out.push_str(&format!("int({})", args.join(", ")));
            }
            "sum" | "prod" => {
                let (lo, hi) = self.limits();
                self.skip_spaces();
                let body = self.seq(false, true);
                let body = strip_brackets(&body);
                match (lo.as_deref().and_then(|l| l.split_once('=')), hi) {
                    (Some((n, from)), Some(to)) => out.push_str(&format!(
                        "{name}({body}, {}, {}, {}) ",
                        n.trim(),
                        from.trim(),
                        to.trim()
                    )),
                    _ => out.push_str(&format!("{name} {body}")),
                }
            }
            "left" | "right" | "displaystyle" | "textstyle" | "limits" | "nolimits" => {}
            "text" | "mathrm" | "operatorname" | "mathbf" | "mathit" => {
                out.push_str(&self.raw_argument());
            }
            "quad" | "qquad" => out.push(' '),
            "infty" => out.push_str("inf"),
            other => out.push_str(&symbol_code(other)),
        }
    }

    /// The integrand and its variable, up to the differential (`\, dx`).
    fn integrand(&mut self) -> (String, String) {
        let start = self.i;
        let mut depth = 0usize;
        let mut j = self.i;
        while j < self.c.len() {
            let c = self.c[j];
            match c {
                '{' | '(' | '[' => depth += 1,
                '}' | ')' | ']' if depth == 0 => break,
                '}' | ')' | ']' => depth -= 1,
                'd' if depth == 0 => {
                    let prev = j.checked_sub(1).map(|p| self.c[p]);
                    let starts = prev.is_none_or(|p| p == ' ' || p == ',' || p == '}');
                    let var: String = self.c[j + 1..]
                        .iter()
                        .take_while(|c| c.is_alphanumeric())
                        .collect();
                    let after = self.c.get(j + 1 + var.chars().count()).copied();
                    if starts && !var.is_empty() && after.is_none_or(|a| !a.is_alphanumeric()) {
                        let body: String = self.c[start..j].iter().collect();
                        let body = body.trim_end().trim_end_matches("\\,").trim_end();
                        self.i = j + 1 + var.chars().count();
                        return (to_code(body), var);
                    }
                }
                _ => {}
            }
            j += 1;
        }
        let body: String = self.c[start..j].iter().collect();
        self.i = j;
        (to_code(&body), "x".to_string())
    }
}

/// `(f)` → `f` (a call's argument needs no brackets of its own).
fn strip_brackets(s: &str) -> String {
    let t = s.trim();
    if t.starts_with('(') && close_paren(t, 0) == Some(t.len() - 1) {
        t[1..t.len() - 1].trim().to_string()
    } else {
        t.to_string()
    }
}

/// A LaTeX command as code: operators and relations as typed on a keyboard
/// (`\le` → `<=`, `\cdot` → `*`), letters and functions by name.
fn symbol_code(name: &str) -> String {
    let Some(sym) = lookup_latex(name) else {
        return format!(r"\{name}");
    };
    if let Some((op, _)) = CODE_OPERATORS.iter().find(|(_, u)| *u == sym.unicode) {
        return format!(" {op} ");
    }
    match (sym.unicode, sym.kind) {
        ("×", _) => " * ".to_string(),
        ("÷", _) => " / ".to_string(),
        (_, SymbolKind::Letter | SymbolKind::Function) => format!("{} ", sym.name),
        (u, _) if !u.is_empty() => u.to_string(),
        _ => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_public_for_the_cheat_sheet() {
        assert!(table().iter().any(|f| f.id == "frac" && f.code == "a/b"));
    }

    #[test]
    fn copy_as_latex() {
        assert_eq!(to_latex("α + β"), r"\alpha + \beta");
        assert_eq!(to_latex("a ≤ b"), r"a \leq b");
        assert_eq!(to_latex("sqrt(x) <= 2"), r"\sqrt{x} \leq 2");
        assert_eq!(to_latex("x**2 # note"), "x^2 % note");
        assert_eq!(to_latex("x²"), "x^{2}");
        assert_eq!(to_latex(r"\frac{a}{b}"), r"\frac{a}{b}");
        assert_eq!(to_latex("sum(n**2, n, 1, 10)"), r"\sum_{n=1}^{10} n^2");
    }

    #[test]
    fn copy_as_code() {
        assert_eq!(to_code(r"\frac{a}{b+1}"), "a/(b+1)");
        assert_eq!(to_code(r"x^{2} + y_{n}"), "x**2 + y_n");
        assert_eq!(to_code(r"\sqrt{x+1}"), "sqrt(x+1)");
        assert_eq!(to_code(r"\int_{0}^{1} x^{2} \, dx"), "int(x**2, x, 0, 1)");
        assert_eq!(to_code(r"\int f \, dt"), "int(f, t)");
        assert_eq!(to_code(r"\sum_{n=1}^{10} n + 1"), "sum(n, n, 1, 10) + 1");
        assert_eq!(
            to_code(r"\frac{d}{dx} \left(x^{2} + 1\right)"),
            "diff(x**2 + 1, x)"
        );
        assert_eq!(to_code("a ≤ b · c"), "a <= b * c");
        assert_eq!(to_code(r"\alpha + \pi \cdot \infty"), "alpha + pi * inf");
        assert_eq!(to_code("x + 1 % note"), "x + 1 # note");
    }

    #[test]
    fn code_round_trips_through_the_parser() {
        use mpp_syntax::math::{parse_math, sexpr};
        let tree = |s: &str| {
            let (ast, d) = parse_math(s, 0);
            assert!(d.is_empty(), "{s}: {d:?}");
            sexpr::ast(&ast, sexpr::Mode::Semantic)
        };
        for src in [
            r"\frac{a}{b+1}",
            r"\int_{0}^{1} x^{2} \, dx",
            r"\sum_{n=1}^{10} n^{2}",
            r"\sqrt{x} \le \alpha",
            r"\frac{d}{dx} \left(x^{2} + 1\right)",
        ] {
            assert_eq!(tree(&to_code(src)), tree(src), "{src} → {}", to_code(src));
        }
    }
}
