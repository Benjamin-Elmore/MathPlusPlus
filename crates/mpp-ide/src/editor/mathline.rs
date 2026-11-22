//! Maths lines. Every line is either prose or maths, and a maths line is saved
//! as a `$$ … $$` block, so the file format does not change.
//!
//! Ways in: Ctrl+M (or a click on the ∑ marker) flips the caret's line between
//! prose and maths; Alt+= and Insert → Maths line make it maths; typing `$`,
//! `=` or `:=` on an empty line starts a maths line; and a finished prose line
//! that reads as maths (`x := 5`, `E = 2x + 5`, `1.32 + 5 =`) becomes one by
//! itself. Every change is an undo step of its own, so the first Ctrl+Z gives
//! back exactly what was typed.

use std::ops::Range;

use super::{EditKind, Editor};
use crate::hints::Hint;

/// An empty maths line, with the caret going between the two spaces.
const EMPTY_LINE: &str = "$$  $$";

/// What the view remembers about maths lines for one open file.
#[derive(Debug, Clone, Default)]
pub struct MathsState {
    /// Auto-detect is switched off (it is on by default).
    pub auto_off: bool,
    /// Lines auto-detect has already turned into maths, or that were turned
    /// back into prose: never turned into maths again by themselves.
    pub kept: Vec<String>,
}

/// Where the caret was before a frame's input: auto-detect looks at that line
/// once the caret has left it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineMark {
    pub line_start: usize,
    pub revision: u64,
}

/// What kind of line holds an offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// A line with nothing on it.
    Empty,
    /// A maths block: the range from its opening `$$` to the end of its closing one.
    Maths(usize, usize),
    /// A plain prose line.
    Prose,
    /// A heading, list item, table row, fence, title field…: never flipped.
    Other,
}

/// `(start, end)` of the line holding `at`.
pub fn line_bounds(text: &str, at: usize) -> (usize, usize) {
    let at = at.min(text.len());
    let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let end = text[at..].find('\n').map_or(text.len(), |i| at + i);
    (start, end)
}

/// Offsets of every `$$` in the text, skipping escaped dollars.
fn display_marks(text: &str) -> Vec<usize> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut k = 0;
    while k < b.len() {
        match b[k] {
            b'\\' => k += 2,
            b'$' if b.get(k + 1) == Some(&b'$') => {
                out.push(k);
                k += 2;
            }
            _ => k += 1,
        }
    }
    out
}

/// The `$$ … $$` block around `at` (the caret anywhere from its opening `$$`
/// to just after its closing one): the offsets of the two `$$`.
pub fn display_around(text: &str, at: usize) -> Option<(usize, usize)> {
    let marks = display_marks(text);
    marks
        .chunks(2)
        .filter(|p| p.len() == 2)
        .map(|p| (p[0], p[1]))
        .find(|&(o, c)| o <= at && at <= c + 2)
        .or_else(|| {
            // An open block still being typed: from its `$$` to the end of the line.
            let &last = marks.last()?;
            (marks.len() % 2 == 1 && last <= at).then_some((last, usize::MAX))
        })
}

/// True when `at` is inside a fenced code block.
fn in_fence(text: &str, at: usize) -> bool {
    text[..at]
        .lines()
        .filter(|l| l.trim_start().starts_with("```"))
        .count()
        % 2
        == 1
}

/// True when the line is plain prose a maths line can be made from: not a
/// heading, list item, table row, quote, fence, comment, figure, import or
/// title field.
fn plain_line(line: &str) -> bool {
    let t = line.trim_start();
    let b = t.as_bytes();
    if t.is_empty() {
        return false;
    }
    if matches!(b[0], b'#' | b'|' | b'>' | b'%' | b'!' | b'`') {
        return false;
    }
    if (t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ")) || t.starts_with("[[") {
        return false;
    }
    let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && b.get(digits) == Some(&b'.') && b.get(digits + 1) == Some(&b' ') {
        return false;
    }
    // `Title:`, `Author:`, `Figure:`, `Using …` and other keyword lines.
    let first = t.split([' ', ':']).next().unwrap_or("");
    !matches!(
        first,
        "Title" | "Author" | "Authors" | "Date" | "Abstract" | "Figure" | "Using" | "Let"
    )
}

/// The kind of line at `at`.
pub fn line_kind(text: &str, at: usize) -> LineKind {
    let at = at.min(text.len());
    if let Some((o, c)) = display_around(text, at) {
        let end = if c == usize::MAX {
            line_bounds(text, at).1
        } else {
            c + 2
        };
        return LineKind::Maths(o, end);
    }
    let (ls, le) = line_bounds(text, at);
    let line = &text[ls..le];
    if line.trim().is_empty() {
        return LineKind::Empty;
    }
    if in_fence(text, ls) || !plain_line(line) {
        return LineKind::Other;
    }
    LineKind::Prose
}

/// Names of functions that keep a line maths (`sin(x)^2`).
const FUNCTIONS: &[&str] = &[
    "sin", "cos", "tan", "sec", "csc", "cot", "arcsin", "arccos", "arctan", "sinh", "cosh", "tanh",
    "log", "ln", "lg", "exp", "sqrt", "abs", "max", "min", "sum", "prod", "int", "lim", "det",
    "gcd", "lcm", "floor", "ceil", "mod", "pi", "inf", "diff", "solve", "frac", "cdot", "times",
    "alpha", "beta", "gamma", "delta", "theta", "lambda", "mu", "sigma", "omega", "phi", "rho",
    "tau", "epsilon",
];

/// Names defined in the text (`speed := 5`, `f(x) := …`, `g \equiv 9.81`).
pub fn defined_names(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for mark in [":=", "\\equiv", "≡"] {
        let mut from = 0;
        while let Some(i) = text[from..].find(mark) {
            let at = from + i;
            from = at + mark.len();
            let mut head = text[..at].trim_end();
            // `f(x) :=`: the name before the brackets.
            if head.ends_with(')') {
                if let Some(open) = head.rfind('(') {
                    head = head[..open].trim_end();
                }
            }
            let name: String = head
                .chars()
                .rev()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            if name.chars().count() > 1 && !out.contains(&name) {
                out.push(name);
            }
        }
    }
    out
}

/// True when a finished prose line reads as maths: it has `:=` or `≡`, or ends
/// with `=` or `= ?`, or it is symbols, digits and operators whose words are all
/// functions or defined names. A line with two or more ordinary words stays prose.
pub fn reads_as_maths(line: &str, known: &[String]) -> bool {
    let t = line.trim();
    if t.is_empty() || t.contains('$') || t.contains('`') || t.contains("[[") {
        return false;
    }
    let mut ordinary = 0;
    let mut word = String::new();
    let mut command = false;
    let mut prev = ' ';
    let flush = |word: &mut String, command: bool, ordinary: &mut usize| {
        if word.chars().count() >= 2
            && !command
            && !FUNCTIONS.contains(&word.as_str())
            && !known.iter().any(|k| k == word)
        {
            *ordinary += 1;
        }
        word.clear();
    };
    for c in t.chars() {
        if c.is_alphabetic() {
            if word.is_empty() {
                command = prev == '\\';
            }
            word.push(c);
        } else {
            flush(&mut word, command, &mut ordinary);
        }
        prev = c;
    }
    flush(&mut word, command, &mut ordinary);
    if ordinary >= 2 {
        return false;
    }
    let squeezed: String = t.chars().filter(|c| !c.is_whitespace()).collect();
    let strong = t.contains(":=")
        || t.contains('≡')
        || t.contains("\\equiv")
        || squeezed.ends_with('=')
        || squeezed.ends_with("=?")
        || squeezed.ends_with("≈?");
    if strong {
        return t.chars().any(|c| c.is_alphanumeric());
    }
    if ordinary > 0 {
        return false;
    }
    let op = t.chars().any(|c| "+-*/^=<>()≤≥≠·×÷−±√∫∑".contains(c)) || t.contains('\\');
    op && t.chars().any(|c| c.is_alphanumeric())
}

/// The maths line made from a prose line: `$` signs dropped, spaces trimmed.
/// Returns the new text and a map from each kept byte of the old line to its
/// place in the new one.
fn wrap(line: &str) -> (String, Vec<(usize, usize)>) {
    let lead = line.len() - line.trim_start().len();
    let body = line.trim();
    let mut content = String::new();
    let mut map = Vec::new();
    let mut esc = false;
    for (i, c) in body.char_indices() {
        if c == '$' && !esc {
            continue;
        }
        esc = c == '\\' && !esc;
        map.push((lead + i, 3 + content.len()));
        content.push(c);
    }
    map.push((lead + body.len(), 3 + content.len()));
    (format!("$$ {content} $$"), map)
}

impl Editor {
    /// Replaces `range` with `with` as an undo step of its own; the caret goes to
    /// `caret` when given, or keeps its place relative to the text around it.
    fn maths_step(&mut self, range: Range<usize>, with: &str, caret: Option<usize>) {
        self.record(EditKind::Other, false);
        self.splice(range, with);
        if let Some(c) = caret {
            self.caret = c.min(self.text.len());
        }
        self.anchor = None;
        self.fixup();
        self.revision += 1;
    }

    /// Makes the line at `at` a maths line. False when it already is one or is
    /// not a line maths can be made from (a heading, a list item…).
    pub fn make_maths(&mut self, at: usize) -> bool {
        let (ls, le) = line_bounds(&self.text, at);
        match line_kind(&self.text, at) {
            LineKind::Empty => {
                let caret = (self.caret >= ls && self.caret <= le).then_some(ls + 3);
                self.maths_step(ls..le, EMPTY_LINE, caret);
                if caret.is_none() {
                    self.caret = self.caret.min(self.text.len());
                }
                true
            }
            LineKind::Prose => {
                let line = self.text[ls..le].to_string();
                let (mut new, map) = wrap(&line);
                let mut base = ls;
                // A maths line stands alone: blank lines part it from the
                // paragraph lines around it.
                let before = ls > 0 && !line_blank_before(&self.text, ls);
                let after = le < self.text.len() && !line_blank_after(&self.text, le);
                if before {
                    new.insert(0, '\n');
                    base += 1;
                }
                if after {
                    new.push('\n');
                }
                let caret = (self.caret >= ls && self.caret <= le).then(|| {
                    let rel = self.caret - ls;
                    let to = map
                        .iter()
                        .rfind(|(o, _)| *o <= rel)
                        .map(|(_, n)| *n)
                        .unwrap_or(3);
                    base + to
                });
                self.maths_step(ls..le, &new, caret);
                true
            }
            LineKind::Maths(..) | LineKind::Other => false,
        }
    }

    /// Makes the maths block at `at` prose again: its rows become lines of
    /// their own. False when `at` is not in a maths block.
    pub fn make_prose(&mut self, state: &mut MathsState, at: usize) -> bool {
        let LineKind::Maths(open, end) = line_kind(&self.text, at) else {
            return false;
        };
        let closed = self.text.get(end.saturating_sub(2)..end) == Some("$$") && end >= open + 4;
        let body_end = if closed { end - 2 } else { end };
        let raw = &self.text[open + 2..body_end];
        let lead = raw.len() - raw.trim_start().len();
        let rows: Vec<String> = raw
            .trim()
            .split("\\\\")
            .map(|r| r.trim().to_string())
            .filter(|r| !r.is_empty())
            .collect();
        let new = rows.join("\n\n");
        let caret = (self.caret >= open && self.caret <= end).then(|| {
            let rel = self.caret.saturating_sub(open + 2 + lead);
            open + rel.min(new.len())
        });
        let caret = caret.map(|c| {
            let mut c = c.min(open + new.len());
            while !new.is_char_boundary(c - open) {
                c -= 1;
            }
            c
        });
        if !rows.is_empty() {
            let first = rows[0].clone();
            if !state.kept.contains(&first) {
                state.kept.push(first);
            }
        }
        self.maths_step(open..end, &new, caret);
        true
    }

    /// Ctrl+M or a click on the ∑ marker: flips the line at `at` between prose
    /// and maths. False when nothing changed.
    pub fn flip_maths(&mut self, state: &mut MathsState, at: usize) -> bool {
        match line_kind(&self.text, at) {
            LineKind::Maths(..) => self.make_prose(state, at),
            LineKind::Empty | LineKind::Prose => self.make_maths(at),
            LineKind::Other => false,
        }
    }

    /// Called before a character is typed. Inside a maths line, the closing
    /// `$$` that the editor wrote is stepped over when the person types it, and
    /// a `$` or space typed straight into an empty maths line is taken as part of
    /// opening it (someone typing `$$ ` by hand). True when the character is used up.
    pub fn maths_before_char(&mut self, ch: char) -> bool {
        if (ch != '$' && ch != ' ') || self.selection().is_some() {
            return false;
        }
        let caret = self.caret;
        let (ls, le) = line_bounds(&self.text, caret);
        let line = &self.text[ls..le];
        let lead = line.len() - line.trim_start().len();
        if line[lead..].starts_with("$$") {
            let inner = &line[lead + 2..];
            if let Some(body) = inner.trim_end().strip_suffix("$$") {
                if body.trim().is_empty() && caret == ls + lead + 3 {
                    return true;
                }
            }
        }
        if ch != '$' {
            return false;
        }
        // The block the caret is in (its rows may span several lines).
        let Some((open, close)) = display_around(&self.text, caret) else {
            return false;
        };
        if close == usize::MAX || caret <= open + 1 || caret > close + 1 {
            return false;
        }
        let rest = &self.text[caret..close + 2];
        let closing = rest.trim_start();
        let first = closing == "$$" && caret > open + 2;
        let second = closing == "$" && caret == close + 1;
        if first || second {
            let gap = rest.len() - closing.len();
            // A space typed before the closing `$$` that already has one: keep one.
            let typed = self.text[..caret].len() - self.text[..caret].trim_end().len();
            let drop = if gap > 0 {
                typed.min(caret.saturating_sub(open + 3))
            } else {
                0
            };
            if drop > 0 {
                self.replace_inline(caret - drop..caret, "");
            }
            let to = self.caret + gap + 1;
            self.move_caret(to, false);
            return true;
        }
        false
    }

    /// Called after a character was typed in prose: `$`, `=` or `:=` typed on
    /// an empty line turns it into an empty maths line (an undo step of its own:
    /// the first Ctrl+Z gives back what was typed). True when it did.
    pub fn maths_after_char(&mut self, _state: &mut MathsState, ch: char) -> bool {
        if ch != '$' && ch != '=' {
            return false;
        }
        let (ls, le) = line_bounds(&self.text, self.caret);
        let typed = self.text[ls..le].trim();
        let start = match ch {
            '$' => typed == "$",
            _ => typed == "=" || typed == ":=",
        };
        if !start || in_fence(&self.text, ls) || display_marks(&self.text[..ls]).len() % 2 == 1 {
            return false;
        }
        let shown = typed.to_string();
        self.maths_step(ls..le, EMPTY_LINE, Some(ls + 3));
        self.fire_hint(
            Hint::StartMaths,
            &format!("Maths line. Ctrl+Z gives back the {shown}"),
        );
        true
    }

    /// Auto-detect, after a frame's input: when the caret has left the line it
    /// was on (Enter, an arrow key, a click) and that line is prose that reads as
    /// maths, it becomes a maths line. One undo step, so the first Ctrl+Z keeps
    /// it as text (and it is never turned into maths by itself again).
    pub fn maths_after_frame(
        &mut self,
        state: &mut MathsState,
        was: LineMark,
        enter: bool,
    ) -> bool {
        if state.auto_off || self.selection().is_some() {
            return false;
        }
        if self.revision != was.revision && !enter {
            return false;
        }
        if was.line_start > self.text.len() {
            return false;
        }
        let (ls, le) = line_bounds(&self.text, was.line_start);
        if ls != was.line_start || (self.caret >= ls && self.caret <= le) {
            return false;
        }
        if line_kind(&self.text, ls) != LineKind::Prose {
            return false;
        }
        let line = self.text[ls..le].trim().to_string();
        if state.kept.contains(&line) {
            return false;
        }
        // One line of its own (not part of a longer paragraph).
        if !line_blank_before(&self.text, ls) && ls > 0
            || le < self.text.len() && !line_blank_after(&self.text, le)
        {
            return false;
        }
        let known = defined_names(&self.text);
        if !reads_as_maths(&line, &known) {
            return false;
        }
        state.kept.push(line);
        if !self.make_maths(ls) {
            return false;
        }
        self.fire_hint(
            Hint::AutoMaths,
            "Turned into maths. Ctrl+Z keeps it as text.",
        );
        true
    }

    /// Where the caret is now, for [`Editor::maths_after_frame`].
    pub fn line_mark(&self) -> LineMark {
        LineMark {
            line_start: line_bounds(&self.text, self.caret).0,
            revision: self.revision,
        }
    }
}

/// True when the line before the one starting at `ls` is blank (or there is none).
fn line_blank_before(text: &str, ls: usize) -> bool {
    if ls == 0 {
        return true;
    }
    let (ps, pe) = line_bounds(text, ls - 1);
    text[ps..pe].trim().is_empty()
}

/// True when the line after the one ending at `le` is blank (or there is none).
fn line_blank_after(text: &str, le: usize) -> bool {
    if le >= text.len() {
        return true;
    }
    let (ns, ne) = line_bounds(text, le + 1);
    text[ns..ne].trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ed(text: &str, caret: usize) -> Editor {
        let mut e = Editor::new(text);
        e.move_caret(caret, false);
        e
    }

    #[test]
    fn ctrl_m_flips_a_line_and_undo_flips_it_back() {
        let mut e = ed("Intro\n\nx := 5\n\nEnd", 9);
        assert!(e.flip_maths(&mut MathsState::default(), e.caret()));
        assert_eq!(e.text(), "Intro\n\n$$ x := 5 $$\n\nEnd");
        assert_eq!(e.caret(), 12, "the caret keeps its place in the line");
        assert!(e.flip_maths(&mut MathsState::default(), e.caret()));
        assert_eq!(e.text(), "Intro\n\nx := 5\n\nEnd");
        assert!(e.undo());
        assert_eq!(e.text(), "Intro\n\n$$ x := 5 $$\n\nEnd");
        assert!(e.undo());
        assert_eq!(e.text(), "Intro\n\nx := 5\n\nEnd");
    }

    #[test]
    fn an_empty_line_becomes_an_empty_maths_line() {
        let mut e = ed("Intro\n\n", 7);
        assert!(e.flip_maths(&mut MathsState::default(), 7));
        assert_eq!(e.text(), "Intro\n\n$$  $$");
        assert_eq!(e.caret(), 10);
    }

    #[test]
    fn headings_and_lists_are_not_flipped() {
        let mut e = ed("# Title\n\n- item", 3);
        assert!(!e.flip_maths(&mut MathsState::default(), 3));
        assert!(!e.flip_maths(&mut MathsState::default(), 12));
    }

    #[test]
    fn a_line_inside_a_paragraph_is_parted_from_it() {
        let mut e = ed("one two\nx + 1\nthree four", 9);
        assert!(e.flip_maths(&mut MathsState::default(), 9));
        assert_eq!(e.text(), "one two\n\n$$ x + 1 $$\n\nthree four");
    }

    #[test]
    fn inline_dollars_are_dropped_when_a_line_becomes_maths() {
        let mut e = ed("$x$ + 1", 0);
        assert!(e.flip_maths(&mut MathsState::default(), 0));
        assert_eq!(e.text(), "$$ x + 1 $$");
    }

    #[test]
    fn rows_become_lines_of_their_own() {
        let mut e = ed("$$ a = 1 \\\\\nb = 2 $$", 4);
        assert!(e.flip_maths(&mut MathsState::default(), 4));
        assert_eq!(e.text(), "a = 1\n\nb = 2");
    }

    #[test]
    fn dollar_on_an_empty_line_starts_maths_and_the_closing_is_stepped_over() {
        let mut st = MathsState::default();
        let mut e = ed("", 0);
        for ch in "$$ E = ? $$".chars() {
            if e.maths_before_char(ch) {
                continue;
            }
            e.insert_text(&ch.to_string());
            e.maths_after_char(&mut st, ch);
        }
        assert_eq!(e.text(), "$$ E = ? $$");
        assert_eq!(e.caret(), e.text().len());
    }

    #[test]
    fn equals_on_an_empty_line_starts_maths_and_undo_gives_it_back() {
        let mut st = MathsState::default();
        let mut e = ed("Intro\n\n", 7);
        e.insert_text("=");
        assert!(e.maths_after_char(&mut st, '='));
        assert_eq!(e.text(), "Intro\n\n$$  $$");
        assert!(e.hint().is_some());
        assert!(e.undo());
        assert_eq!(e.text(), "Intro\n\n=");
    }

    #[test]
    fn what_reads_as_maths() {
        let known = vec!["speed".to_string()];
        for yes in [
            "x := 5",
            "E = 2x + 5",
            "1.32 + 5 =",
            "sin(x)^2",
            "E := 2x + 5",
            "speed := 3",
            "speed * 2",
            "a + b = ?",
            "\\frac{1}{2} + 3",
        ] {
            assert!(reads_as_maths(yes, &known), "{yes}");
        }
        for no in [
            "Hello world",
            "The answer is x := 5",
            "2024",
            "x",
            "x-ray",
            "Hi :)",
            "costs $5 = cheap",
            "e.g.",
            "I think so",
        ] {
            assert!(!reads_as_maths(no, &known), "{no}");
        }
    }

    #[test]
    fn auto_detect_turns_a_finished_line_into_maths_once() {
        let mut st = MathsState::default();
        let mut e = ed("# T\n\nx := 5", 11);
        let was = e.line_mark();
        e.enter(false, false);
        assert!(e.maths_after_frame(&mut st, was, true));
        assert_eq!(e.text(), "# T\n\n$$ x := 5 $$\n\n");
        assert_eq!(e.caret(), e.text().len(), "the caret stays on the new line");
        assert!(e.hint().is_some_and(|h| h.contains("Ctrl+Z")));
        // The first Ctrl+Z undoes only the detection, not the Enter.
        assert!(e.undo());
        assert_eq!(e.text(), "# T\n\nx := 5\n\n");
        // Leaving the line again does not turn it into maths a second time.
        e.move_caret(8, false);
        let was = e.line_mark();
        e.move_caret(e.text().len(), false);
        assert!(!e.maths_after_frame(&mut st, was, false));
    }

    #[test]
    fn prose_lines_stay_prose() {
        let mut st = MathsState::default();
        let mut e = ed("Some words here", 15);
        let was = e.line_mark();
        e.enter(false, false);
        assert!(!e.maths_after_frame(&mut st, was, true));
        let mut off = MathsState {
            auto_off: true,
            ..MathsState::default()
        };
        let mut e = ed("x := 5", 6);
        let was = e.line_mark();
        e.enter(false, false);
        assert!(!e.maths_after_frame(&mut off, was, true));
    }

    #[test]
    fn names_defined_in_the_text() {
        let n = defined_names("speed := 3\n\nf(x) := x^2\n\ng \\equiv 9.81");
        assert_eq!(n, vec!["speed".to_string()]);
    }
}
