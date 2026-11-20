//! Tokeniser for maths text in all three input forms (LaTeX, plain, Unicode).
//!
//! Every name in the symbol table becomes its symbol with or without a backslash.
//! A run of ASCII letters that is not a known name is split into single letters
//! (`xy` is `x y`). Spacing commands (`\,`, `\quad` …) and sizing commands
//! (`\big`, `\displaystyle` …) are dropped here.

use crate::math::ast::{Accent, RelOp};
use crate::span::{Diagnostic, Span};
use crate::symbols::{
    lookup_char, lookup_latex, lookup_name, styled, subscript_value, superscript_value, Font,
    Symbol, SymbolKind,
};

/// A token with its span in the whole file.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: Tok,
    pub span: Span,
}

/// Binary and prefix operator tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Plus,
    Minus,
    PlusMinus,
    MinusPlus,
    Star,
    Cdot,
    Times,
    Slash,
    Obelus,
    Compose,
    Tensor,
    DirectSum,
    Cup,
    Cap,
    SetMinus,
    /// `\bmod`, `\mod`: the remainder.
    Mod,
    And,
    Or,
    Not,
}

/// Bracket-like delimiters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delim {
    Paren,
    Square,
    /// `\{ \}`.
    Brace,
    Angle,
    Floor,
    Ceil,
    Bar,
    DoubleBar,
    /// `\left.`.
    None,
}

/// Token kinds.
#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    /// Digits with optional fraction and exponent, as written.
    Number(String),
    /// A single letter that is not a known name.
    Letter(char),
    /// A symbol-table entry that is a letter, function, calculus sign, accent or misc symbol.
    Sym(&'static Symbol),
    /// A plain-form keyword not in the symbol table (`root`, `floor`, `abs` …).
    Word(&'static str),
    /// `\mathbb{R}`, `ℝ`, `\mathbf{F}`, `\mathrm{d}`: letters in a font.
    Styled {
        font: Font,
        text: String,
    },
    /// `\operatorname{name}`.
    OperatorName(String),
    /// `\text{…}`.
    Text(String),
    /// Any other LaTeX command, without the backslash.
    Command(String),
    Begin(String),
    End(String),
    /// `\label{…}`.
    Label(String),
    Op(Op),
    Rel(RelOp),
    Open {
        delim: Delim,
        sized: bool,
    },
    Close {
        delim: Delim,
        sized: bool,
    },
    /// `|`, which may open or close.
    Pipe,
    /// `\|` or `‖`, which may open or close.
    DoublePipe,
    /// TeX group braces `{ }`.
    LBrace,
    RBrace,
    Caret,
    Underscore,
    /// One or more primes.
    Prime(u8),
    Bang,
    Comma,
    Semicolon,
    Amp,
    /// `\\`.
    NewLine,
    /// `:=`.
    Assign,
    /// `\overset{?}{=}`, `≟`, `?=`.
    CheckEq,
    Question,
    Period,
    /// A run of Unicode superscripts, as plain text (`²` is `"2"`, `⁻¹` is `"-1"`).
    Superscript(String),
    /// A run of Unicode subscripts, as plain text.
    Subscript(String),
    /// `√` (2), `∛` (3), `∜` (4).
    Radical(u8),
    /// A Unicode combining accent after a letter.
    Combining(Accent),
    /// `%noeval`.
    NoEval,
    Unknown(char),
}

/// Plain-form keywords that are not in the symbol table.
const WORDS: &[&str] = &["root", "binom", "floor", "ceil", "abs", "norm", "otherwise"];

/// LaTeX commands that only change spacing or size.
const IGNORED_COMMANDS: &[&str] = &[
    "quad",
    "qquad",
    "displaystyle",
    "textstyle",
    "scriptstyle",
    "limits",
    "nolimits",
    "nonumber",
    "notag",
    "thinspace",
    "medspace",
    "thickspace",
    "negthinspace",
    "enspace",
    "hfill",
    "strut",
    "mathstrut",
    "big",
    "Big",
    "bigg",
    "Bigg",
    "bigl",
    "bigr",
    "Bigl",
    "Bigr",
    "biggl",
    "biggr",
    "Biggl",
    "Biggr",
    "middle",
];

/// Splits maths text into tokens. Spans are offset by `base` so they index the whole file.
pub fn lex(src: &str, base: usize) -> (Vec<Token>, Vec<Diagnostic>) {
    let mut lexer = Lexer {
        src,
        pos: 0,
        base,
        tokens: Vec::new(),
        diags: Vec::new(),
    };
    lexer.run();
    (lexer.tokens, lexer.diags)
}

struct Lexer<'a> {
    src: &'a str,
    pos: usize,
    base: usize,
    tokens: Vec<Token>,
    diags: Vec<Diagnostic>,
}

impl<'a> Lexer<'a> {
    fn rest(&self) -> &'a str {
        self.src.get(self.pos..).unwrap_or("")
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.rest().chars().nth(n)
    }

    fn span(&self, start: usize) -> Span {
        Span::new(start + self.base, self.pos + self.base)
    }

    fn push(&mut self, kind: Tok, start: usize) {
        let span = self.span(start);
        self.tokens.push(Token { kind, span });
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.bump();
        }
    }

    fn run(&mut self) {
        while let Some(c) = self.peek() {
            let start = self.pos;
            if c.is_whitespace() || c == '~' {
                self.bump();
            } else if c.is_ascii_digit() {
                self.number(start);
            } else if c.is_ascii_alphabetic() {
                self.letters(start);
            } else if c == '\\' {
                self.backslash(start);
            } else if c.is_ascii() {
                self.ascii(c, start);
            } else {
                self.unicode(c, start);
            }
        }
    }

    fn number(&mut self, start: usize) {
        self.eat_digits();
        if self.peek() == Some('.') && self.peek_at(1).is_some_and(|d| d.is_ascii_digit()) {
            self.bump();
            self.eat_digits();
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            let digit_at = |n| self.peek_at(n).is_some_and(|d: char| d.is_ascii_digit());
            let skip = if digit_at(1) {
                Some(1)
            } else if matches!(self.peek_at(1), Some('-' | '+')) && digit_at(2) {
                Some(2)
            } else {
                None
            };
            if let Some(skip) = skip {
                for _ in 0..skip {
                    self.bump();
                }
                self.eat_digits();
            }
        }
        let text = self.src[start..self.pos].to_string();
        self.push(Tok::Number(text), start);
    }

    fn eat_digits(&mut self) {
        while self.peek().is_some_and(|d| d.is_ascii_digit()) {
            self.bump();
        }
    }

    /// A run of ASCII letters: a known name, a keyword, or single letters.
    fn letters(&mut self, start: usize) {
        while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            self.bump();
        }
        let run = &self.src[start..self.pos];
        if run == "inf" {
            if let Some(sym) = lookup_name("infty") {
                self.push(Tok::Sym(sym), start);
            }
            return;
        }
        if let Some(font) = font_command(run) {
            if let Some(text) = self.braced_plain() {
                self.push(Tok::Styled { font, text }, start);
                return;
            }
        }
        if let Some(sym) = lookup_name(run) {
            let tok = classify(sym);
            self.push(tok, start);
            return;
        }
        if let Some(word) = WORDS.iter().find(|w| **w == run) {
            self.push(Tok::Word(word), start);
            return;
        }
        for (i, c) in run.char_indices() {
            let at = start + i;
            self.tokens.push(Token {
                kind: Tok::Letter(c),
                span: Span::new(at + self.base, at + 1 + self.base),
            });
        }
    }

    fn ascii(&mut self, c: char, start: usize) {
        self.bump();
        let next = self.peek();
        let kind = match c {
            '+' => Tok::Op(Op::Plus),
            '-' if next == Some('>') => {
                self.bump();
                Tok::Rel(RelOp::To)
            }
            '-' => Tok::Op(Op::Minus),
            '*' => Tok::Op(Op::Star),
            '/' => Tok::Op(Op::Slash),
            '^' => Tok::Caret,
            '_' => Tok::Underscore,
            '\'' => {
                let mut count: u8 = 1;
                while self.peek() == Some('\'') {
                    self.bump();
                    count = count.saturating_add(1);
                }
                Tok::Prime(count)
            }
            '!' if next == Some('=') => {
                self.bump();
                Tok::Rel(RelOp::Ne)
            }
            '!' => Tok::Bang,
            '=' if next == Some('>') => {
                self.bump();
                Tok::Rel(RelOp::Implies)
            }
            '=' => Tok::Rel(RelOp::Eq),
            '<' if self.rest().starts_with("=>") => {
                self.bump();
                self.bump();
                Tok::Rel(RelOp::Iff)
            }
            '<' if next == Some('=') => {
                self.bump();
                Tok::Rel(RelOp::Le)
            }
            '<' => Tok::Rel(RelOp::Lt),
            '>' if next == Some('=') => {
                self.bump();
                Tok::Rel(RelOp::Ge)
            }
            '>' => Tok::Rel(RelOp::Gt),
            ':' if next == Some('=') => {
                self.bump();
                Tok::Assign
            }
            ':' => Tok::Rel(RelOp::Other(":")),
            '?' if next == Some('=') => {
                self.bump();
                Tok::CheckEq
            }
            '?' => Tok::Question,
            '|' => Tok::Pipe,
            '(' => Tok::Open {
                delim: Delim::Paren,
                sized: false,
            },
            ')' => Tok::Close {
                delim: Delim::Paren,
                sized: false,
            },
            '[' => Tok::Open {
                delim: Delim::Square,
                sized: false,
            },
            ']' => Tok::Close {
                delim: Delim::Square,
                sized: false,
            },
            '{' => Tok::LBrace,
            '}' => Tok::RBrace,
            ',' => Tok::Comma,
            ';' => Tok::Semicolon,
            '&' => Tok::Amp,
            '.' if self.rest().starts_with("..") => {
                self.pos += 2;
                match lookup_name("ldots") {
                    Some(sym) => Tok::Sym(sym),
                    None => Tok::Period,
                }
            }
            '.' => Tok::Period,
            '%' => {
                if self.rest().starts_with("noeval") {
                    self.pos += "noeval".len();
                    Tok::NoEval
                } else {
                    while self.peek().is_some_and(|c| c != '\n') {
                        self.bump();
                    }
                    return;
                }
            }
            other => {
                self.push(Tok::Unknown(other), start);
                let span = self.span(start);
                self.diags.push(Diagnostic::error(
                    span,
                    format!("unexpected character `{other}`"),
                ));
                return;
            }
        };
        self.push(kind, start);
    }

    fn unicode(&mut self, c: char, start: usize) {
        if superscript_value(c).is_some() {
            let text = self.script_run(superscript_value);
            self.push(Tok::Superscript(text), start);
            return;
        }
        if subscript_value(c).is_some() {
            let text = self.script_run(subscript_value);
            self.push(Tok::Subscript(text), start);
            return;
        }
        self.bump();
        if let Some(accent) = Accent::from_combining(c) {
            self.push(Tok::Combining(accent), start);
            return;
        }
        if let Some((letter, accent)) = decompose(c) {
            self.push(Tok::Letter(letter), start);
            let end = self.pos + self.base;
            self.tokens.push(Token {
                kind: Tok::Combining(accent),
                span: Span::point(end),
            });
            return;
        }
        let special = match c {
            '−' => Some(Tok::Op(Op::Minus)),
            '⋅' => Some(Tok::Op(Op::Cdot)),
            '≔' => Some(Tok::Assign),
            '≟' => Some(Tok::CheckEq),
            '‴' => Some(Tok::Prime(3)),
            '√' => Some(Tok::Radical(2)),
            '∛' => Some(Tok::Radical(3)),
            '∜' => Some(Tok::Radical(4)),
            '…' => lookup_name("ldots").map(Tok::Sym),
            '⩽' => Some(Tok::Rel(RelOp::Le)),
            '⩾' => Some(Tok::Rel(RelOp::Ge)),
            _ => None,
        };
        let kind = if let Some(tok) = special {
            tok
        } else if let Some(sym) = lookup_char(c) {
            classify(sym)
        } else if let Some((font, letter)) = unstyle(c) {
            Tok::Styled {
                font,
                text: letter.to_string(),
            }
        } else if c.is_alphabetic() {
            Tok::Letter(c)
        } else {
            let span = self.span(start);
            self.diags.push(Diagnostic::error(
                span,
                format!("unexpected character `{c}`"),
            ));
            Tok::Unknown(c)
        };
        self.push(kind, start);
    }

    fn script_run(&mut self, value: fn(char) -> Option<char>) -> String {
        let mut text = String::new();
        while let Some(v) = self.peek().and_then(value) {
            text.push(v);
            self.bump();
        }
        text
    }

    fn backslash(&mut self, start: usize) {
        self.bump();
        let Some(c) = self.peek() else {
            self.push(Tok::Unknown('\\'), start);
            let span = self.span(start);
            self.diags.push(Diagnostic::error(
                span,
                "a lone `\\` at the end of the maths",
            ));
            return;
        };
        if !c.is_ascii_alphabetic() {
            self.bump();
            let kind = match c {
                '\\' => {
                    self.skip_line_spacing();
                    Tok::NewLine
                }
                ',' | ';' | ':' | '!' | ' ' | '>' | '\n' | '\t' => return,
                '{' => Tok::Open {
                    delim: Delim::Brace,
                    sized: false,
                },
                '}' => Tok::Close {
                    delim: Delim::Brace,
                    sized: false,
                },
                '|' => Tok::DoublePipe,
                other => Tok::Unknown(other),
            };
            self.push(kind, start);
            return;
        }
        while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            self.bump();
        }
        let name = &self.src[start + 1..self.pos];
        self.command(name, start);
    }

    /// `\\[4pt]`: drop the optional spacing after a line break.
    fn skip_line_spacing(&mut self) {
        let rest = self.rest();
        if let Some(inner) = rest.strip_prefix('[') {
            if let Some(end) = inner.find(']') {
                let body = &inner[..end];
                let is_length = !body.is_empty()
                    && body.starts_with(|c: char| c.is_ascii_digit() || c == '.' || c == '-')
                    && body
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
                if is_length {
                    self.pos += end + 2;
                }
            }
        }
    }

    fn command(&mut self, name: &str, start: usize) {
        if IGNORED_COMMANDS.contains(&name) {
            return;
        }
        let kind = match name {
            "left" | "right" => self.sized_delim(name == "left", start),
            "tag" => {
                self.braced_raw();
                return;
            }
            "begin" | "end" => match self.braced_raw() {
                Some(env) if name == "begin" => Tok::Begin(env),
                Some(env) => Tok::End(env),
                None => Tok::Command(name.to_string()),
            },
            "text" | "textrm" | "mbox" | "textit" | "textbf" | "mathtt" | "textnormal" => {
                match self.braced_raw() {
                    Some(text) => Tok::Text(text),
                    None => Tok::Command(name.to_string()),
                }
            }
            "operatorname" => {
                if self.peek() == Some('*') {
                    self.bump();
                }
                match self.braced_raw() {
                    Some(text) => Tok::OperatorName(text.trim().to_string()),
                    None => Tok::Command(name.to_string()),
                }
            }
            "label" => match self.braced_raw() {
                Some(text) => Tok::Label(text.trim().to_string()),
                None => Tok::Command(name.to_string()),
            },
            "overset" | "stackrel" if self.check_eq_args() => Tok::CheckEq,
            "lvert" => open(Delim::Bar),
            "rvert" => close(Delim::Bar),
            "lVert" => open(Delim::DoubleBar),
            "rVert" => close(Delim::DoubleBar),
            "leqslant" => Tok::Rel(RelOp::Le),
            "geqslant" => Tok::Rel(RelOp::Ge),
            "colon" => Tok::Rel(RelOp::Other(":")),
            "coloneqq" => Tok::Assign,
            "bmod" | "mod" => Tok::Op(Op::Mod),
            // The transpose mark in `A^\top`.
            "top" | "intercal" => Tok::Letter('⊤'),
            _ => {
                if let Some(font) = font_command(name) {
                    if let Some(text) = self.braced_plain() {
                        self.push(Tok::Styled { font, text }, start);
                        return;
                    }
                }
                match lookup_latex(name).or_else(|| lookup_name(name)) {
                    Some(sym) => classify(sym),
                    None => Tok::Command(name.to_string()),
                }
            }
        };
        self.push(kind, start);
    }

    /// The delimiter after `\left` or `\right`.
    fn sized_delim(&mut self, left: bool, start: usize) -> Tok {
        self.skip_ws();
        let delim = self.delimiter();
        let delim = match delim {
            Some(d) => d,
            None => {
                let span = self.span(start);
                let which = if left { "\\left" } else { "\\right" };
                self.diags.push(
                    Diagnostic::error(span, format!("expected a delimiter after `{which}`"))
                        .expecting(["`(`", "`[`", "`\\{`", "`|`", "`.`"]),
                );
                Delim::None
            }
        };
        if left {
            Tok::Open { delim, sized: true }
        } else {
            Tok::Close { delim, sized: true }
        }
    }

    fn delimiter(&mut self) -> Option<Delim> {
        let c = self.peek()?;
        let simple = match c {
            '(' | ')' => Some(Delim::Paren),
            '[' | ']' => Some(Delim::Square),
            '|' => Some(Delim::Bar),
            '.' => Some(Delim::None),
            '<' | '>' | '⟨' | '⟩' => Some(Delim::Angle),
            '‖' => Some(Delim::DoubleBar),
            '⌊' | '⌋' => Some(Delim::Floor),
            '⌈' | '⌉' => Some(Delim::Ceil),
            _ => None,
        };
        if simple.is_some() {
            self.bump();
            return simple;
        }
        if c != '\\' {
            return None;
        }
        let rest = &self.rest()[1..];
        if let Some(next) = rest.chars().next() {
            let found = match next {
                '{' | '}' => Some(Delim::Brace),
                '|' => Some(Delim::DoubleBar),
                _ => None,
            };
            if found.is_some() {
                self.pos += 2;
                return found;
            }
        }
        let len = rest.chars().take_while(char::is_ascii_alphabetic).count();
        let found = match &rest[..len] {
            "langle" | "rangle" => Delim::Angle,
            "lfloor" | "rfloor" => Delim::Floor,
            "lceil" | "rceil" => Delim::Ceil,
            "vert" | "lvert" | "rvert" => Delim::Bar,
            "Vert" | "lVert" | "rVert" => Delim::DoubleBar,
            "lbrace" | "rbrace" => Delim::Brace,
            _ => return None,
        };
        self.pos += 1 + len;
        Some(found)
    }

    /// Reads `{…}` with balanced braces and returns the inside, raw.
    fn braced_raw(&mut self) -> Option<String> {
        let save = self.pos;
        self.skip_ws();
        if self.peek() != Some('{') {
            self.pos = save;
            return None;
        }
        self.bump();
        let inner_start = self.pos;
        let mut depth = 1usize;
        while let Some(c) = self.bump() {
            match c {
                '\\' => {
                    self.bump();
                }
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(self.src[inner_start..self.pos - 1].to_string());
                    }
                }
                _ => {}
            }
        }
        let span = Span::new(save + self.base, self.pos + self.base);
        self.diags
            .push(Diagnostic::error(span, "this `{` is never closed").expecting(["a closing `}`"]));
        Some(self.src[inner_start..].to_string())
    }

    /// Reads `{letters}` when the inside is only ASCII letters, digits and spaces,
    /// or a single letter or digit without braces (`\mathbf r`).
    fn braced_plain(&mut self) -> Option<String> {
        let save = self.pos;
        self.skip_ws();
        let rest = self.rest();
        if let Some(c) = rest.chars().next().filter(char::is_ascii_alphanumeric) {
            self.bump();
            return Some(c.to_string());
        }
        let Some(inner) = rest.strip_prefix('{') else {
            self.pos = save;
            return None;
        };
        let end = inner.find('}');
        let ok = end.is_some_and(|end| {
            let body = &inner[..end];
            !body.trim().is_empty() && body.chars().all(|c| c.is_ascii_alphanumeric() || c == ' ')
        });
        match (ok, end) {
            (true, Some(end)) => {
                let text: String = inner[..end].chars().filter(|c| *c != ' ').collect();
                self.pos += end + 2;
                Some(text)
            }
            _ => {
                self.pos = save;
                None
            }
        }
    }

    /// After `\overset`: true (and consumed) when the arguments are `{?}{=}`.
    fn check_eq_args(&mut self) -> bool {
        let save = self.pos;
        let first = self.braced_raw();
        let second = self.braced_raw();
        if first.as_deref().map(str::trim) == Some("?")
            && second.as_deref().map(str::trim) == Some("=")
        {
            return true;
        }
        self.pos = save;
        false
    }
}

fn open(delim: Delim) -> Tok {
    Tok::Open {
        delim,
        sized: false,
    }
}

fn close(delim: Delim) -> Tok {
    Tok::Close {
        delim,
        sized: false,
    }
}

fn font_command(name: &str) -> Option<Font> {
    Some(match name {
        "mathbb" => Font::Blackboard,
        "mathcal" | "mathscr" => Font::Calligraphic,
        "mathbf" | "boldsymbol" | "bm" => Font::Bold,
        "mathfrak" => Font::Fraktur,
        "mathrm" | "mathit" | "mathsf" => Font::Roman,
        _ => return None,
    })
}

/// Maps a symbol-table entry to the token the parser works with.
pub fn classify(sym: &'static Symbol) -> Tok {
    use SymbolKind as K;
    match sym.kind {
        K::Operator => match sym.name {
            "pm" => Tok::Op(Op::PlusMinus),
            "mp" => Tok::Op(Op::MinusPlus),
            "times" => Tok::Op(Op::Times),
            "div" => Tok::Op(Op::Obelus),
            "cdot" => Tok::Op(Op::Cdot),
            "circ" => Tok::Op(Op::Compose),
            "otimes" => Tok::Op(Op::Tensor),
            "oplus" => Tok::Op(Op::DirectSum),
            "ast" => Tok::Op(Op::Star),
            _ => Tok::Sym(sym),
        },
        K::Relation => Tok::Rel(RelOp::from_symbol(sym.unicode)),
        K::Set => match sym.name {
            "cup" => Tok::Op(Op::Cup),
            "cap" => Tok::Op(Op::Cap),
            "setminus" => Tok::Op(Op::SetMinus),
            "emptyset" | "varnothing" => Tok::Sym(sym),
            _ => Tok::Rel(RelOp::from_symbol(sym.unicode)),
        },
        K::Logic => match sym.name {
            "neg" | "lnot" => Tok::Op(Op::Not),
            "land" | "wedge" => Tok::Op(Op::And),
            "lor" | "vee" => Tok::Op(Op::Or),
            "implies" | "Rightarrow" | "iff" | "Leftrightarrow" => {
                Tok::Rel(RelOp::from_symbol(sym.unicode))
            }
            _ => Tok::Sym(sym),
        },
        K::Arrow => Tok::Rel(RelOp::from_symbol(sym.unicode)),
        K::Delimiter => match sym.name {
            "langle" => open(Delim::Angle),
            "rangle" => close(Delim::Angle),
            "lfloor" => open(Delim::Floor),
            "rfloor" => close(Delim::Floor),
            "lceil" => open(Delim::Ceil),
            "rceil" => close(Delim::Ceil),
            "lbrace" => open(Delim::Brace),
            "rbrace" => close(Delim::Brace),
            "Vert" => Tok::DoublePipe,
            _ => Tok::Pipe,
        },
        K::Calculus if sym.name == "prime" => Tok::Prime(1),
        K::Calculus if sym.name == "dprime" => Tok::Prime(2),
        K::Font => Tok::Command(sym.name.to_string()),
        _ => Tok::Sym(sym),
    }
}

/// Precomposed accented letters (`ẋ`, `â`, `ā`, `ñ`) as a letter plus an accent.
fn decompose(c: char) -> Option<(char, Accent)> {
    const TABLE: &[(&str, &str, Accent)] = &[
        ("ȧḃċḋėḟġḣṁṅȯṗṙṡṫẇẋẏż", "abcdefghmnoprstwxyz", Accent::Dot),
        ("äëḧïöẗüẅẍÿ", "aehiotuwxy", Accent::DDot),
        ("âĉêĝĥîĵôŝûŵŷẑ", "aceghijosuwyz", Accent::Hat),
        ("āēḡīōūȳ", "aegiouy", Accent::Bar),
        ("ãẽĩñõũṽỹ", "aeinouvy", Accent::Tilde),
    ];
    TABLE.iter().find_map(|(accented, plain, accent)| {
        let i = accented.chars().position(|a| a == c)?;
        plain.chars().nth(i).map(|p| (p, *accent))
    })
}

/// Reverse of [`styled`]: `ℝ` is blackboard `R`, `𝐫` is bold `r`.
fn unstyle(c: char) -> Option<(Font, char)> {
    let code = c as u32;
    if !(0x2100..=0x214F).contains(&code) && !(0x1D400..=0x1D7FF).contains(&code) {
        return None;
    }
    let fonts = [
        Font::Blackboard,
        Font::Calligraphic,
        Font::Bold,
        Font::Fraktur,
    ];
    let plain = ('A'..='Z').chain('a'..='z').chain('0'..='9');
    for letter in plain {
        for font in fonts {
            if styled(font, letter) == Some(c) {
                return Some((font, letter));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<Tok> {
        lex(src, 0).0.into_iter().map(|t| t.kind).collect()
    }

    fn sym(name: &str) -> Tok {
        Tok::Sym(lookup_name(name).expect("known symbol"))
    }

    #[test]
    fn three_forms_of_alpha() {
        assert_eq!(kinds("\\alpha"), vec![sym("alpha")]);
        assert_eq!(kinds("alpha"), vec![sym("alpha")]);
        assert_eq!(kinds("α"), vec![sym("alpha")]);
    }

    #[test]
    fn letter_runs_split() {
        assert_eq!(kinds("xy"), vec![Tok::Letter('x'), Tok::Letter('y')]);
        assert_eq!(kinds("sin"), vec![sym("sin")]);
        assert_eq!(kinds("dx"), vec![Tok::Letter('d'), Tok::Letter('x')]);
    }

    #[test]
    fn numbers() {
        assert_eq!(kinds("2.5e-3"), vec![Tok::Number("2.5e-3".into())]);
        assert_eq!(kinds("2e"), vec![Tok::Number("2".into()), Tok::Letter('e')]);
        assert_eq!(kinds("3."), vec![Tok::Number("3".into()), Tok::Period]);
    }

    #[test]
    fn plain_operators() {
        assert_eq!(
            kinds("<= >= != -> := ?= =>"),
            vec![
                Tok::Rel(RelOp::Le),
                Tok::Rel(RelOp::Ge),
                Tok::Rel(RelOp::Ne),
                Tok::Rel(RelOp::To),
                Tok::Assign,
                Tok::CheckEq,
                Tok::Rel(RelOp::Implies),
            ]
        );
        assert_eq!(kinds("inf"), vec![sym("infty")]);
        assert_eq!(kinds("\\inf"), vec![sym("inf")]);
    }

    #[test]
    fn unicode_scripts_and_styles() {
        assert_eq!(
            kinds("x²₁ℝ"),
            vec![
                Tok::Letter('x'),
                Tok::Superscript("2".into()),
                Tok::Subscript("1".into()),
                Tok::Styled {
                    font: Font::Blackboard,
                    text: "R".into()
                },
            ]
        );
        assert_eq!(
            kinds("x⁻¹"),
            vec![Tok::Letter('x'), Tok::Superscript("-1".into())]
        );
        assert_eq!(
            kinds("x\u{0307}"),
            vec![Tok::Letter('x'), Tok::Combining(Accent::Dot)]
        );
    }

    #[test]
    fn latex_commands() {
        assert_eq!(kinds("\\, \\quad \\displaystyle"), vec![]);
        assert_eq!(
            kinds("\\left( \\right\\|"),
            vec![
                Tok::Open {
                    delim: Delim::Paren,
                    sized: true
                },
                Tok::Close {
                    delim: Delim::DoubleBar,
                    sized: true
                }
            ]
        );
        assert_eq!(kinds("\\overset{?}{=}"), vec![Tok::CheckEq]);
        assert_eq!(
            kinds("\\text{if } x"),
            vec![Tok::Text("if ".into()), Tok::Letter('x')]
        );
        assert_eq!(
            kinds("\\mathbb{R}"),
            vec![Tok::Styled {
                font: Font::Blackboard,
                text: "R".into()
            }]
        );
        assert_eq!(
            kinds("\\begin{pmatrix}"),
            vec![Tok::Begin("pmatrix".into())]
        );
        assert_eq!(kinds("\\frac"), vec![Tok::Command("frac".into())]);
        assert_eq!(kinds("x %noeval"), vec![Tok::Letter('x'), Tok::NoEval]);
        assert_eq!(
            kinds("x % comment\ny"),
            vec![Tok::Letter('x'), Tok::Letter('y')]
        );
    }

    #[test]
    fn spans_use_base_offset() {
        let (tokens, _) = lex("α+x", 10);
        assert_eq!(tokens[0].span, Span::new(10, 12));
        assert_eq!(tokens[1].span, Span::new(12, 13));
        assert_eq!(tokens[2].span, Span::new(13, 14));
    }

    #[test]
    fn bad_input_reports() {
        let (tokens, diags) = lex("x # \\left", 0);
        assert_eq!(tokens.len(), 3);
        assert_eq!(diags.len(), 2);
    }
}
