//! Maths source as Unicode text: `\int_0^1 x^2 dx` becomes `∫₀¹ x² dx`.
//!
//! Used wherever the command line shows maths to a person: results, error
//! messages and the quick calculator. It never shows a raw span or a backslash
//! command it knows.

use std::collections::HashMap;
use std::sync::OnceLock;

use mpp_syntax::symbols::{
    lookup_latex, styled, subscript_value, superscript_value, Font, SymbolKind,
};

/// Converts maths source (LaTeX-style or already Unicode) to Unicode text.
pub fn to_unicode(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut p = Parser { chars, i: 0 };
    let out = p.seq(None);
    tidy(&out)
}

/// Collapses runs of spaces and trims.
fn tidy(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            space = true;
        } else {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.push(c);
        }
    }
    out
}

fn supers() -> &'static HashMap<char, char> {
    static MAP: OnceLock<HashMap<char, char>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut m = HashMap::new();
        let ranges = [
            '\u{2070}'..='\u{209f}',
            '\u{00b2}'..='\u{00b3}',
            '\u{00b9}'..='\u{00b9}',
            '\u{1d2c}'..='\u{1d6a}',
        ];
        for r in ranges {
            for c in r {
                if let Some(n) = superscript_value(c) {
                    m.entry(n).or_insert(c);
                }
            }
        }
        m
    })
}

fn subs() -> &'static HashMap<char, char> {
    static MAP: OnceLock<HashMap<char, char>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut m = HashMap::new();
        for c in '\u{2080}'..='\u{209f}' {
            if let Some(n) = subscript_value(c) {
                m.entry(n).or_insert(c);
            }
        }
        m
    })
}

struct Parser {
    chars: Vec<char>,
    i: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.i).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn skip_space(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.i += 1;
        }
    }

    /// Reads items until `close` (not consumed) or the end.
    fn seq(&mut self, close: Option<char>) -> String {
        let mut out = String::new();
        while let Some(c) = self.peek() {
            if Some(c) == close {
                break;
            }
            let atom = self.atom();
            let atom = self.scripts(atom);
            out.push_str(&atom);
        }
        out
    }

    /// One operand: a `{group}`, a command or a single character, as Unicode.
    fn operand(&mut self) -> String {
        self.skip_space();
        match self.peek() {
            Some('{') => {
                self.i += 1;
                let s = self.seq(Some('}'));
                self.eat('}');
                tidy(&s)
            }
            Some(_) => self.atom(),
            None => String::new(),
        }
    }

    /// The text of a `{group}` as written (for `\begin{name}`).
    fn raw_group(&mut self) -> String {
        self.skip_space();
        let mut s = String::new();
        if self.eat('{') {
            while let Some(c) = self.peek() {
                self.i += 1;
                if c == '}' {
                    break;
                }
                s.push(c);
            }
        }
        s
    }

    fn scripts(&mut self, mut base: String) -> String {
        loop {
            let save = self.i;
            self.skip_space();
            match self.peek() {
                Some('^') => {
                    self.i += 1;
                    let arg = self.operand();
                    base.push_str(&script(&arg, true));
                }
                Some('_') => {
                    self.i += 1;
                    let arg = self.operand();
                    base.push_str(&script(&arg, false));
                }
                _ => {
                    self.i = save;
                    return base;
                }
            }
        }
    }

    fn atom(&mut self) -> String {
        let Some(c) = self.peek() else {
            return String::new();
        };
        self.i += 1;
        match c {
            '\\' => self.command(),
            '{' => {
                let s = self.seq(Some('}'));
                self.eat('}');
                s
            }
            '}' => String::new(),
            '~' => " ".into(),
            '*' => "*".into(),
            c => c.to_string(),
        }
    }

    fn command(&mut self) -> String {
        let mut name = String::new();
        match self.peek() {
            Some(c) if c.is_ascii_alphabetic() => {
                while let Some(c) = self.peek().filter(char::is_ascii_alphabetic) {
                    name.push(c);
                    self.i += 1;
                }
            }
            Some(c) => {
                self.i += 1;
                return match c {
                    ',' | ';' | ':' | '!' | ' ' => " ".into(),
                    '{' | '}' | '$' | '%' | '&' | '#' | '_' => c.to_string(),
                    '\\' => "; ".into(),
                    '|' => "‖".into(),
                    other => other.to_string(),
                };
            }
            None => return String::new(),
        }
        match name.as_str() {
            "frac" | "dfrac" | "tfrac" => {
                let a = self.operand();
                let b = self.operand();
                // `\frac{\partial}{\partial y}` reads best as `∂/∂y`.
                let b = match b.strip_prefix("∂ ") {
                    Some(rest) => format!("∂{rest}"),
                    None => b,
                };
                format!("{}/{}", paren(&a), paren(&b))
            }
            "sqrt" => {
                self.skip_space();
                let mut index = String::new();
                if self.eat('[') {
                    index = self.seq(Some(']'));
                    self.eat(']');
                }
                let a = self.operand();
                let root = match index.trim() {
                    "" | "2" => "√",
                    "3" => "∛",
                    "4" => "∜",
                    n => return format!("{}√{}", script(n, true), tight(&a)),
                };
                format!("{root}{}", tight(&a))
            }
            "left" | "right" | "big" | "Big" | "bigg" | "Bigg" | "displaystyle" | "textstyle"
            | "limits" | "nolimits" => {
                self.skip_space();
                if self.peek() == Some('.') {
                    self.i += 1;
                }
                String::new()
            }
            "quad" | "qquad" => " ".into(),
            "pmod" => format!("(mod {})", self.operand()),
            "mathrm" | "operatorname" | "text" | "textrm" | "mathit" | "mathsf" | "mathtt"
            | "textit" | "boldsymbol" | "bm" | "mathbf" | "textbf" => self.operand(),
            "mathbb" => font(Font::Blackboard, &self.operand()),
            "mathcal" | "mathscr" => font(Font::Calligraphic, &self.operand()),
            "mathfrak" => font(Font::Fraktur, &self.operand()),
            "vec" => format!("{}\u{20d7}", self.operand()),
            "hat" => format!("{}\u{0302}", self.operand()),
            "bar" | "overline" => format!("{}\u{0304}", self.operand()),
            "tilde" => format!("{}\u{0303}", self.operand()),
            "dot" => format!("{}\u{0307}", self.operand()),
            "ddot" => format!("{}\u{0308}", self.operand()),
            "overset" | "stackrel" => {
                let top = self.operand();
                let base = self.operand();
                if top == "?" && base == "=" {
                    "=?".into()
                } else {
                    format!("{base}{top}")
                }
            }
            "begin" => {
                let env = self.raw_group();
                let body = self.matrix_body(&env);
                match env.as_str() {
                    "pmatrix" | "matrix" => format!("({body})"),
                    "bmatrix" => format!("[{body}]"),
                    "vmatrix" => format!("|{body}|"),
                    "cases" => format!("{{ {body}"),
                    _ => body,
                }
            }
            "end" => {
                self.raw_group();
                String::new()
            }
            "dots" | "ldots" | "cdots" | "hdots" => "…".into(),
            "circ" => "∘".into(),
            "infty" => "∞".into(),
            "to" => "→".into(),
            "le" | "leq" => "≤".into(),
            "ge" | "geq" => "≥".into(),
            "ne" | "neq" => "≠".into(),
            "in" => "∈".into(),
            "cdot" => "·".into(),
            "times" => "×".into(),
            // `\Delta x` is one name, the change in x: `Δx`.
            "Delta" => {
                let save = self.i;
                self.skip_space();
                if !self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
                    self.i = save;
                }
                "Δ".into()
            }
            other => match lookup_latex(other) {
                Some(s) if s.kind == SymbolKind::Function => s.unicode.to_string(),
                Some(s) => s.unicode.to_string(),
                None => other.to_string(),
            },
        }
    }

    fn matrix_body(&mut self, env: &str) -> String {
        let mut out = String::new();
        let amp = if env == "cases" { "  if " } else { ", " };
        while let Some(c) = self.peek() {
            if c == '\\' && self.chars.get(self.i + 1..self.i + 5) == Some(&['e', 'n', 'd', '{']) {
                break;
            }
            if c == '&' {
                self.i += 1;
                out.push_str(amp);
                continue;
            }
            let atom = self.atom();
            let atom = self.scripts(atom);
            out.push_str(&atom);
        }
        // The `\end{env}` is read by the caller's next atom.
        if self.peek() == Some('\\') {
            self.i += 1;
            let _ = self.command();
        }
        out.trim().replace("; ;", ";").replace(" ;", ";")
    }
}

fn font(f: Font, s: &str) -> String {
    s.chars()
        .map(|c| styled(f, c).unwrap_or(c))
        .collect::<String>()
}

fn paren(s: &str) -> String {
    let simple = s.chars().all(|c| {
        c.is_alphanumeric() || matches!(c, '.' | '°' | '′' | '√' | '∂' | '∞') || is_script(c)
    }) || (s.starts_with('(') && s.ends_with(')') && !s[1..s.len() - 1].contains('('));
    if simple || s.is_empty() {
        s.to_string()
    } else {
        format!("({s})")
    }
}

/// Brackets for the argument of a root: only a single symbol or number goes without.
fn tight(s: &str) -> String {
    let single = s.chars().count() == 1 || s.chars().all(|c| c.is_ascii_digit());
    if single
        || s.is_empty()
        || (s.starts_with('(') && s.ends_with(')') && !s[1..s.len() - 1].contains('('))
    {
        s.to_string()
    } else {
        format!("({s})")
    }
}

fn is_script(c: char) -> bool {
    ('\u{2070}'..='\u{209f}').contains(&c) || matches!(c, '²' | '³' | '¹')
}

fn script(arg: &str, up: bool) -> String {
    if up && arg == "∘" {
        return "°".into();
    }
    if up && arg == "′" {
        return "′".into();
    }
    let map = if up { supers() } else { subs() };
    let mapped: Option<String> = arg.chars().map(|c| map.get(&c).copied()).collect();
    match mapped {
        Some(s) if !s.is_empty() => s,
        _ if arg.is_empty() => String::new(),
        _ => {
            let mark = if up { '^' } else { '_' };
            if arg.chars().count() == 1 {
                format!("{mark}{arg}")
            } else {
                format!("{mark}({arg})")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::to_unicode;

    #[test]
    fn integrals_and_powers() {
        assert_eq!(to_unicode(r"\int_0^1 x^2 dx"), "∫₀¹ x² dx");
        assert_eq!(to_unicode(r"\sin(30^\circ)"), "sin(30°)");
    }

    #[test]
    fn fractions_and_roots() {
        assert_eq!(to_unicode(r"\frac{1}{3}"), "1/3");
        assert_eq!(to_unicode(r"\frac{x+1}{2}"), "(x+1)/2");
        assert_eq!(to_unicode(r"\sqrt{x+1}"), "√(x+1)");
        assert_eq!(to_unicode(r"\sqrt{2}"), "√2");
    }

    #[test]
    fn sums_and_greek() {
        assert_eq!(
            to_unicode(r"\sum_{n=1}^{\infty} \frac{1}{n^2}"),
            "∑ₙ₌₁^∞ 1/n²"
        );
        assert_eq!(to_unicode(r"\alpha + \beta"), "α + β");
        // `\Delta x` is one name: no space inside it.
        assert_eq!(to_unicode(r"\frac{\Delta x}{\Delta t}"), "Δx/Δt");
        assert_eq!(to_unicode(r"\Delta + 1"), "Δ + 1");
    }

    #[test]
    fn unknown_script_falls_back() {
        assert_eq!(to_unicode("x^{abc}"), "x^(abc)");
    }
}
