//! Typing in maths: both ways of typing (LaTeX and code) convert as you type.
//!
//! What converts, and to what, comes from the input forms table
//! ([`mpp_syntax::forms`]). Conversions happen only on a delimiter (the
//! character that ends a token: a Space, an operator, a bracket) or on a
//! character that is itself a structure (`^`, `_`, `**`, `/`, `#`). Each one is an
//! undo step of its own, after the typing, so the first Ctrl+Z gives back exactly
//! what was typed.
//!
//! LaTeX typed in full keeps its shape: `{` at the start of an empty box and `}`
//! at the end of one step over the braces the box already has. The other ways
//! out of a structure are in [`super::exits`].

use std::ops::Range;

use mpp_syntax::forms::{call_to_latex, is_operator_char, split_args, CALLS, CODE_OPERATORS};
use mpp_syntax::symbols::{lookup_char, lookup_latex, lookup_name, SymbolKind};

use super::symbols::live_replacement;
use super::{parse_snippet, Editor, Knobs, Snippet};
use crate::insert::{self, BuildCx};

/// What typing in maths needs to know.
#[derive(Debug, Clone, Default)]
pub struct TypingCx {
    /// For templates that depend on the document (the likely variable).
    pub build: BuildCx,
    /// Structures (`^`, `/`, `\frac` …) open boxes. When off only symbols convert.
    pub shortcuts: bool,
}

/// The template ids of the exponent and subscript boxes `^` and `_` open.
pub const SUP: &str = "script.sup";
pub const SUB: &str = "script.sub";

/// LaTeX commands whose braces hold words, not maths: nothing converts inside.
const TEXT_COMMANDS: &[&str] = &[
    "text",
    "textrm",
    "textbf",
    "textit",
    "mathrm",
    "mathbf",
    "mathit",
    "mathsf",
    "mathtt",
    "operatorname",
    "label",
    "ref",
    "eqref",
    "begin",
    "end",
    "unit",
];

/// Signs that take limits; their LaTeX commands are kept as typed.
const BIG_OPERATORS: &[&str] = &[
    "int", "iint", "iiint", "oint", "oiint", "sum", "prod", "coprod", "bigcup", "bigcap",
];

/// The token just before a delimiter.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    /// `\name`.
    Command(String),
    /// A run of ASCII letters.
    Word(String),
    /// A run of operator characters (`<=`).
    Ops(String),
}

/// The token ending at `end`, with its range.
fn token_before(text: &str, end: usize) -> Option<(Range<usize>, Token)> {
    let before = &text[..end];
    let last = before.chars().next_back()?;
    if is_operator_char(last) {
        let start = before
            .char_indices()
            .rev()
            .take_while(|(_, c)| is_operator_char(*c))
            .last()
            .map_or(end, |(i, _)| i);
        return Some((start..end, Token::Ops(before[start..].to_string())));
    }
    if !last.is_ascii_alphabetic() {
        return None;
    }
    let start = before
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_alphabetic())
        .last()
        .map_or(end, |(i, _)| i);
    let word = before[start..].to_string();
    if before[..start].ends_with('\\') && !before[..start].ends_with("\\\\") {
        return Some((start - 1..end, Token::Command(word)));
    }
    Some((start..end, Token::Word(word)))
}

fn replaceable(kind: SymbolKind) -> bool {
    !matches!(
        kind,
        SymbolKind::Function | SymbolKind::Font | SymbolKind::Accent | SymbolKind::Delimiter
    )
}

/// What a token turns into once a delimiter `ch` follows it, if anything.
fn token_text(text: &str, range: &Range<usize>, tok: &Token, ch: char) -> Option<String> {
    match tok {
        Token::Ops(run) => {
            let longer = |t: &str| {
                is_operator_char(ch)
                    && CODE_OPERATORS
                        .iter()
                        .any(|(op, _)| op.len() > t.len() && op.starts_with(&format!("{t}{ch}")))
            };
            if longer(run) {
                return None;
            }
            let (_, to) = CODE_OPERATORS.iter().find(|(op, _)| op == run)?;
            // `x := 1` and `a ?= b`: the `=` belongs to another operator.
            let prev = text[..range.start].chars().next_back();
            if matches!(prev, Some(':' | '?' | '\\')) {
                return None;
            }
            Some((*to).to_string())
        }
        _ if ch.is_ascii_alphanumeric() => None,
        Token::Command(name) => {
            // Big operators keep their command: their limits follow (`\int_0^1`).
            if BIG_OPERATORS.contains(&name.as_str()) {
                return None;
            }
            let sym = lookup_latex(name)?;
            (replaceable(sym.kind) && !sym.unicode.is_empty() && sym.unicode != name.as_str())
                .then(|| sym.unicode.to_string())
        }
        Token::Word(word) => {
            if ch == '\\' || word.len() < 2 {
                return None;
            }
            // A call being typed (`sum(`) is converted when its bracket closes.
            if ch == '(' && CALLS.contains(&word.as_str()) {
                return None;
            }
            let prev = text[..range.start].chars().next_back();
            if prev.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                return None;
            }
            if word == "inf" {
                return Some("∞".to_string());
            }
            let sym = lookup_name(word)?;
            (replaceable(sym.kind) && sym.unicode != word.as_str()).then(|| sym.unicode.to_string())
        }
    }
}

/// True when a power or subscript typed at `at` has something to attach to.
pub(super) fn has_operand(text: &str, at: usize) -> bool {
    let Some(c) = text[..at].trim_end_matches(' ').chars().next_back() else {
        return false;
    };
    if c.is_alphanumeric() || matches!(c, ')' | ']' | '}' | '|' | '\'' | '!' | '.') {
        return true;
    }
    if c.is_ascii() || c == '−' {
        return false;
    }
    lookup_char(c).is_none_or(|s| {
        !matches!(
            s.kind,
            SymbolKind::Operator | SymbolKind::Relation | SymbolKind::Arrow | SymbolKind::Logic
        )
    })
}

/// True when `at` is in a `%` comment on its line or inside the braces of a
/// command that holds words (`\text{…}`): nothing converts there.
fn quiet_at(text: &str, at: usize) -> bool {
    let line = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let bytes = text.as_bytes();
    if (line..at).any(|i| bytes[i] == b'%' && (i == 0 || bytes[i - 1] != b'\\')) {
        return true;
    }
    let lo = at.saturating_sub(2000);
    let mut depth = 0usize;
    let mut i = at;
    while i > lo {
        i -= 1;
        match bytes[i] {
            b'}' => depth += 1,
            b'{' if depth > 0 => depth -= 1,
            b'{' => {
                let head = &text[..i];
                let name_start = head
                    .char_indices()
                    .rev()
                    .take_while(|(_, c)| c.is_ascii_alphabetic())
                    .last()
                    .map_or(i, |(j, _)| j);
                if head[..name_start].ends_with('\\')
                    && TEXT_COMMANDS.contains(&&head[name_start..])
                {
                    return true;
                }
            }
            b'$' => return false,
            _ => {}
        }
    }
    false
}

/// The `{` that opens the group whose `}` is at `close`.
pub(super) fn open_of(text: &str, close: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut i = close;
    while i > 0 {
        i -= 1;
        match bytes[i] {
            b'}' => depth += 1,
            b'{' if depth == 0 => return Some(i),
            b'{' => depth -= 1,
            _ => {}
        }
    }
    None
}

fn script_snippet(sup: bool) -> Option<Snippet> {
    parse_snippet(if sup {
        "^{⟨*exponent⟩}"
    } else {
        "_{⟨*subscript⟩}"
    })
    .ok()
}

impl Editor {
    /// Types one character in maths, converting as it goes (see the module docs).
    pub fn type_maths(&mut self, ch: char, cx: &TypingCx) {
        if self.selection().is_none() {
            if ch == '/' && self.comment_slash() {
                return;
            }
            if !self.before_maths_char(ch) {
                return;
            }
        }
        self.insert_text(&ch.to_string());
        self.after_maths_char(ch, cx);
    }

    /// The innermost template at `caret` and the box holding it.
    pub(super) fn box_here(&self, caret: usize) -> Option<(u64, usize)> {
        let s = self.session_at(caret)?;
        Some((s.id, s.box_at(caret)?))
    }

    /// Moves the caret out of a template to `to`; typing then goes after it.
    pub(super) fn leave_to(&mut self, id: Option<u64>, to: usize) {
        self.move_caret(to, false);
        if let Some(id) = id {
            self.left = Some((id, to));
        }
    }

    /// From box `i` of template `id`: to the start of the next box, or out of
    /// the template after the last one.
    pub(super) fn next_box_or_out(&mut self, id: u64, i: usize) {
        let Some(s) = self.session(id) else {
            return;
        };
        match (i + 1..s.boxes.len()).find(|&j| s.boxes[j].mirror_of.is_none()) {
            Some(j) => {
                let to = s.boxes[j].range.start;
                self.move_caret(to, false);
            }
            None => {
                let caret = self.caret;
                self.finalize(id);
                let to = self.session(id).map_or(caret, |s| s.range.end);
                self.leave_to(Some(id), to);
            }
        }
    }

    /// Runs before a character is inserted in maths. False when the character
    /// was used up here (a brace stepped over, a slip refused).
    fn before_maths_char(&mut self, ch: char) -> bool {
        if quiet_at(&self.text, self.caret) {
            return true;
        }
        self.brace_before(ch) && self.exit_before(ch)
    }

    /// LaTeX typed in full into a box that already has its braces: `{` at the
    /// start of an empty box and `}` at the end of a box are already there. A
    /// Space at the start of an empty box does nothing.
    fn brace_before(&mut self, ch: char) -> bool {
        let caret = self.caret;
        let bytes = self.text.as_bytes();
        let Some((id, i)) = self.box_here(caret) else {
            return true;
        };
        let r = self.session(id).expect("session").boxes[i].range.clone();
        match ch {
            '{' | ' ' => {
                let braces = caret > 0
                    && bytes.get(caret - 1) == Some(&b'{')
                    && bytes.get(caret) == Some(&b'}');
                if r.is_empty() && r.start == caret && (ch == ' ' || braces) {
                    if ch == '{' {
                        if let Some(si) = self.session_index(id) {
                            self.sessions[si].braced = true;
                        }
                    }
                    return false;
                }
                true
            }
            '}' if r.end == caret && bytes.get(caret) == Some(&b'}') => {
                self.next_box_or_out(id, i);
                false
            }
            _ => true,
        }
    }

    /// `//` typed straight after `/` made a fraction: the fraction goes back to
    /// the `/` that was typed and both become a `%` comment. True when done.
    fn comment_slash(&mut self) -> bool {
        if self.auto_frac != Some(self.revision) {
            return false;
        }
        let in_empty_box = self.box_here(self.caret).is_some_and(|(id, i)| {
            self.session(id)
                .is_some_and(|s| s.template == "frac" && s.boxes[i].range.is_empty())
        });
        if !in_empty_box || !self.undo() {
            return false;
        }
        self.insert_text("/");
        let c = self.caret;
        if self.text.get(c.saturating_sub(2)..c) == Some("//") {
            self.convert(c - 2..c, "%");
        }
        true
    }

    /// Runs after a character was inserted in maths: the conversions.
    fn after_maths_char(&mut self, ch: char, cx: &TypingCx) {
        let caret = self.caret;
        let Some(at) = caret.checked_sub(ch.len_utf8()) else {
            return;
        };
        if quiet_at(&self.text, at) {
            return;
        }
        let tok = token_before(&self.text, at);
        let conv = tok
            .as_ref()
            .and_then(|(r, t)| token_text(&self.text, r, t, ch).map(|s| (r.clone(), s)));
        match ch {
            '#' => {
                self.convert(at..caret, "%");
                return;
            }
            '/' if self.text[..at].ends_with('/') => {
                self.convert(at - 1..caret, "%");
                return;
            }
            '/' if cx.shortcuts => {
                if insert::shortcut(self, '/', &cx.build) {
                    self.auto_frac = Some(self.revision);
                }
                return;
            }
            '*' if cx.shortcuts && self.text[..at].ends_with('*') => {
                let star = at - 1;
                if has_operand(&self.text, star) {
                    let conv = token_before(&self.text, star).and_then(|(r, t)| {
                        token_text(&self.text, &r, &t, '*').map(|s| (r.clone(), s))
                    });
                    self.open_script(true, star..caret, conv);
                }
                return;
            }
            '^' | '_' if cx.shortcuts => {
                self.open_script(ch == '^', at..caret, conv);
                return;
            }
            ')' => {
                if self.convert_call(at, conv.clone()) {
                    return;
                }
            }
            ' ' | '{' if cx.shortcuts => {
                if let Some((r, Token::Command(name) | Token::Word(name))) = &tok {
                    if self.open_structure(r.clone(), name, ch, &cx.build) {
                        return;
                    }
                }
            }
            _ => {}
        }
        if let Some((r, with)) = conv {
            self.convert(r, &with);
            return;
        }
        if ch.is_ascii_alphabetic() {
            if let Some(r) = live_replacement(&self.text, self.caret, &ch.to_string()) {
                self.replace_inline(r.range, &r.with);
            }
        }
    }

    /// Opens an exponent (`sup`) or subscript box in place of `range` (the `^`,
    /// `_` or `**` just typed), converting the token before it too. One undo step.
    fn open_script(
        &mut self,
        sup: bool,
        range: Range<usize>,
        conv: Option<(Range<usize>, String)>,
    ) {
        let Some(snippet) = script_snippet(sup) else {
            return;
        };
        let (replace, before) = match conv {
            Some((r, with)) if r.end == range.start => (r.start..range.end, with),
            _ => (range, String::new()),
        };
        let id = if sup { SUP } else { SUB };
        self.insert_template(replace, &before, &snippet, "", id, Knobs::default());
    }

    /// `\frac`, `\sqrt` (then Space or `{`), `\int`, `\sum`, `\prod`, `int`,
    /// `sum`, `prod` (then Space): the template, in place of the command and the
    /// delimiter. True when one opened.
    fn open_structure(&mut self, range: Range<usize>, name: &str, ch: char, cx: &BuildCx) -> bool {
        let command = self.text[range.clone()].starts_with('\\');
        let id = match (name, ch, command) {
            ("frac" | "sqrt", ' ' | '{', true) => name,
            ("int", ' ', _) => "int.def",
            ("sum" | "prod", ' ', _) => name,
            _ => return false,
        };
        if !command {
            let prev = self.text[..range.start].chars().next_back();
            if prev.is_some_and(|c| c.is_alphanumeric() || c == '\\') {
                return false;
            }
        }
        let Some(t) = insert::find(id) else {
            return false;
        };
        let Some(snippet) = insert::snippet_for(t, t.knobs, cx) else {
            return false;
        };
        let caret = self.caret;
        self.insert_template(range.start..caret, "", &snippet, "", &t.id, t.knobs);
        true
    }

    /// `)` closed a coder's call (`sqrt(x)`, `sum(f, n, 1, 10)`): the call becomes
    /// its file form. `conv` converts the token before the bracket first (one
    /// undo step for both). True when the call was converted.
    fn convert_call(&mut self, at: usize, conv: Option<(Range<usize>, String)>) -> bool {
        let caret = self.caret;
        let mut text = self.text.clone();
        let mut close = at;
        if let Some((r, with)) = &conv {
            text.replace_range(r.clone(), with);
            close = close + with.len() - r.len();
        }
        let Some(open) = open_paren(&text, close) else {
            return false;
        };
        let head = &text[..open];
        let name_start = head
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_ascii_alphabetic())
            .last()
            .map_or(open, |(i, _)| i);
        let name = &head[name_start..];
        let before = head[..name_start].chars().next_back();
        if !CALLS.contains(&name) || before.is_some_and(|c| c.is_alphanumeric() || c == '\\') {
            return false;
        }
        let args = split_args(&text[open + 1..close]);
        let Some(latex) = call_to_latex(name, &args) else {
            return false;
        };
        let end_in_text = close + 1;
        text.replace_range(name_start..end_in_text, &latex);
        // One step: the whole edited stretch, from the call's start to the caret.
        let shift = text.len() as isize - self.text.len() as isize;
        let new_end = (caret as isize + shift) as usize;
        let with = text[name_start..new_end].to_string();
        self.convert(name_start..caret, &with);
        true
    }
}

/// The `(` matching the `)` at `close`.
fn open_paren(text: &str, close: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in text[..close].char_indices().rev() {
        match c {
            ')' | ']' | '}' => depth += 1,
            '(' if depth == 0 => return Some(i),
            '(' | '[' | '{' => depth = depth.checked_sub(1)?,
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use mpp_syntax::forms::{forms, Trigger};

    fn cx() -> TypingCx {
        TypingCx {
            build: BuildCx::default(),
            shortcuts: true,
        }
    }

    /// Types `s` into an empty maths line, one character at a time.
    fn typed_into(e: &mut Editor, s: &str) {
        let cx = cx();
        for ch in s.chars() {
            e.type_maths(ch, &cx);
        }
    }

    fn typed(s: &str) -> String {
        let mut e = Editor::new("");
        typed_into(&mut e, s);
        e.text().to_string()
    }

    #[test]
    fn every_code_form_in_the_table_types_into_what_the_file_keeps() {
        let mut failures = vec![];
        for f in forms() {
            for code in std::iter::once(&f.code).chain(f.also) {
                // A Space at the end is the delimiter that ends the last token.
                let end = if f.trigger == Trigger::Delimiter {
                    " "
                } else {
                    ""
                };
                let got = typed(&format!("{code}{end}"));
                let want = match f.trigger {
                    Trigger::Kept => code.to_string(),
                    _ => f.writes.to_string(),
                };
                let squash = |s: &str| s.replace(' ', "");
                if squash(&got) != squash(&want) {
                    failures.push(format!(
                        "{}: `{code}` typed gives `{got}`, want `{want}`",
                        f.id
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    }

    #[test]
    fn latex_typed_in_full_stays_as_typed() {
        for (src, want) in [
            (r"\frac{a}{b}", r"\frac{a}{b}"),
            (r"\sqrt{x}", r"\sqrt{x}"),
            (r"x^{2}", r"x^{2}"),
            (r"x_{n}", r"x_{n}"),
            (r"\int_{0}^{1} x \, dx", r"\int_{0}^{1} x \, dx"),
            (r"\sum_{n=1}^{10} n", r"\sum_{n=1}^{10} n"),
            (r"\lim_{x \to 0} x", r"\lim_{x → 0} x"),
        ] {
            assert_eq!(typed(src), want, "{src}");
        }
    }

    #[test]
    fn mathematician_symbols_convert_on_a_delimiter() {
        assert_eq!(typed(r"\alpha + \beta "), "α + β ");
        assert_eq!(typed(r"a \cdot b"), "a · b");
        assert_eq!(typed(r"a \le b"), "a ≤ b");
        assert_eq!(typed(r"\alpha_1"), "α_{1}");
        // A command that is a word, not a symbol, is left alone.
        assert_eq!(typed(r"\sin(x)"), r"\sin(x)");
        assert_eq!(typed(r"\left( x \right)"), r"\left( x \right)");
    }

    #[test]
    fn structures_open_boxes_on_space() {
        assert_eq!(typed(r"\frac a"), r"\frac{a}{}");
        assert_eq!(typed(r"\sqrt 2"), r"\sqrt{2}");
        let mut e = Editor::new("");
        typed_into(&mut e, r"\int ");
        assert!(e.text().starts_with(r"\int"), "{}", e.text());
        assert!(e.session_at(e.caret()).is_some());
    }

    #[test]
    fn the_first_undo_gives_back_what_was_typed() {
        for (src, back) in [
            ("x**", "x**"),
            ("a <= ", "a <= "),
            ("sqrt(x)", "sqrt(x)"),
            ("a*b", "a*b"),
            ("x^", "x^"),
            (r"\alpha ", r"\alpha "),
            ("x #", "x #"),
        ] {
            let mut e = Editor::new("");
            typed_into(&mut e, src);
            assert_ne!(e.text(), back, "{src}: nothing converted");
            assert!(e.undo(), "{src}");
            assert_eq!(e.text(), back, "{src}");
        }
    }

    #[test]
    fn operators_wait_for_the_longest_form() {
        assert_eq!(typed("p <=> q"), "p ⇔ q");
        assert_eq!(typed("x<=-1"), "x≤-1");
        assert_eq!(typed("a := 1"), "a := 1");
        assert_eq!(typed("n! = 1"), "n! = 1");
        assert_eq!(typed("x->0"), "x→0");
    }

    #[test]
    fn words_convert_only_when_they_end() {
        assert_eq!(typed("inf "), "∞ ");
        assert_eq!(typed("info"), "info");
        assert_eq!(typed("2 pi r"), "2 π r");
        // A call keeps its name until its bracket closes.
        assert_eq!(typed("sum(n, n, 1, 3)"), r"\sum_{n=1}^{3} n");
        assert_eq!(typed("sum(n**2, n, 1, 3)"), r"\sum_{n=1}^{3} n^{2}");
        assert_eq!(typed("int(x + 1, x, 0, 1)"), r"\int_{0}^{1} x + 1 \, dx");
        assert_eq!(
            typed("diff(x^2 + 1, x)"),
            r"\frac{d}{dx} \left(x^{2} + 1\right)"
        );
        // A function of the user's is not a call the editor knows.
        assert_eq!(typed("f(x)"), "f(x)");
    }

    #[test]
    fn nothing_converts_in_text_or_comments() {
        assert_eq!(typed(r"\text{a_b <= c}"), r"\text{a_b <= c}");
        assert_eq!(typed("x % a^b <= c"), "x % a^b <= c");
    }

    #[test]
    fn spaced_division_takes_the_operand_before_the_spaces() {
        assert_eq!(typed("a / b"), r"\frac{a}{b}");
    }

    #[test]
    fn coder_comments_become_percent_comments() {
        assert_eq!(typed("x + 1 # note"), "x + 1 % note");
        assert_eq!(typed("x + 1 // note"), "x + 1 % note");
        let mut e = Editor::new("");
        typed_into(&mut e, "x//");
        assert!(e.undo());
        assert_eq!(e.text(), "x//");
    }
}
