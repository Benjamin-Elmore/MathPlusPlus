//! Input rules: what Enter, Backspace, `#`, `- ` and `1. ` do while typing in prose.
//! The file keeps the strict grammar; the editor writes what the grammar needs
//! (a blank line after Enter, the space after `#`) and makes each conversion an
//! undo step of its own, so the first Ctrl+Z gives back what was typed.

use crate::hints::Hint;
use std::ops::Range;

use super::{EditKind, Editor};

/// `(start, end)` of the line holding `at`.
fn line_bounds(text: &str, at: usize) -> (usize, usize) {
    let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let end = text[at..].find('\n').map_or(text.len(), |i| at + i);
    (start, end)
}

/// True when `at` is inside a fenced code block (an odd number of fences before it).
fn in_fence(text: &str, at: usize) -> bool {
    text[..at]
        .lines()
        .filter(|l| l.trim_start().starts_with("```"))
        .count()
        % 2
        == 1
}

/// True when `at` is inside a `$$ ... $$` block (an odd number of `$$` before it).
fn in_display(text: &str, at: usize) -> bool {
    text[..at].matches("$$").count() % 2 == 1
}

/// A list marker at the start of a line: (indent length, the next item's marker,
/// start of the item text in the line).
fn list_marker(line: &str) -> Option<(usize, String, usize)> {
    let t = line.trim_start_matches([' ', '\t']);
    let indent = line.len() - t.len();
    let b = t.as_bytes();
    match b.first()? {
        b'-' | b'*' if b.get(1) == Some(&b' ') => Some((indent, t[..1].to_string(), indent + 2)),
        b'0'..=b'9' => {
            let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
            if digits <= 9 && b.get(digits) == Some(&b'.') && b.get(digits + 1) == Some(&b' ') {
                let n: u64 = t[..digits].parse().ok()?;
                Some((indent, format!("{}.", n + 1), indent + digits + 2))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// The length of the `#`s and the spaces after them at the start of `line`.
fn heading_prefix(line: &str) -> usize {
    let hashes = line.bytes().take_while(|&b| b == b'#').count();
    if hashes == 0 || hashes > 6 {
        return 0;
    }
    let gap = line[hashes..]
        .bytes()
        .take_while(|&b| b == b' ' || b == b'\t')
        .count();
    hashes + gap
}

impl Editor {
    /// Applies an edit as an undo step of its own, keeping the caret relative to
    /// the text after the range.
    pub(super) fn convert(&mut self, range: Range<usize>, with: &str) {
        self.record(EditKind::Other, false);
        let caret = self.caret;
        let end = range.end;
        self.splice(range.clone(), with);
        self.caret = if caret >= end {
            caret - (end - range.start) + with.len()
        } else {
            caret.min(range.start + with.len())
        };
        self.anchor = None;
        self.fixup();
        self.revision += 1;
    }

    /// Shows `text` as a hint under the caret, once per install (see
    /// [`crate::hints`]); later firings stay quiet.
    pub(super) fn fire_hint(&mut self, which: Hint, text: &str) {
        if self.hints.is_seen(which) {
            return;
        }
        self.hints.mark(which);
        self.hint = Some((text.to_string(), self.revision));
    }

    /// The hint to show now, if the last edit made one (it goes with the next edit).
    pub fn hint(&self) -> Option<&str> {
        self.hint
            .as_ref()
            .filter(|(_, rev)| *rev == self.revision)
            .map(|(s, _)| s.as_str())
    }

    /// Starts a new undo step with the next edit.
    pub fn break_step(&mut self) {
        self.last_kind = None;
    }

    /// Enter. A new block in prose (the blank line is written into the file), a
    /// new row in display maths, the next item in a list; `soft` (Shift+Enter) is
    /// a plain line break inside the block. One undo step.
    pub fn enter(&mut self, in_math: bool, soft: bool) {
        // The new line goes after the structure being typed in, not in its box.
        let left = in_math && !soft && self.selection().is_none() && self.leave_templates();
        let at = self.selection().map_or(self.caret, |r| r.start);
        let (ls, le) = line_bounds(&self.text, at);
        let line = self.text[ls..le].to_string();
        let before_blank = self.text[ls..at].trim().is_empty();
        let mut insert = "\n\n".to_string();
        if soft {
            insert = "\n".into();
        } else if in_math {
            let row = (left || self.session_at(at).is_none())
                && in_display(&self.text, at)
                && !before_blank;
            insert = if row { " \\\\\n".into() } else { "\n".into() };
        } else if in_fence(&self.text, at) || line.trim_start().starts_with(['|', '>']) {
            insert = "\n".into();
        } else if let Some((indent, next, content)) = list_marker(&line) {
            if at >= ls + content {
                if line.trim_end().len() <= content {
                    // An empty item ends the list.
                    self.replace_node(mpp_syntax::Span::new(ls, le), "\n");
                    return;
                }
                insert = format!("\n{}{} ", &line[..indent], next);
            }
        } else if line.trim().is_empty() {
            insert = "\n".into();
        }
        self.insert_text(&insert);
    }

    /// Backspace in maths takes one character, as in any text editor (E1): a
    /// digit of `20`, not the whole number. What is drawn as one symbol goes
    /// whole (a command such as `\pi`, an operator such as `:=`), and spaces go
    /// with the character before them so every press shows. Just after a
    /// structure's closing braces (`\frac{\pi}{6}`) the last character inside
    /// goes. False where the page decides instead: just after an opening brace,
    /// a script mark or the maths' own `$$`.
    pub fn backspace_maths(&mut self) -> bool {
        if self.selection().is_some() {
            return false;
        }
        let head = &self.text[..self.caret];
        let mut end = head.trim_end_matches(' ').len();
        let closers = head[..end].len() - head[..end].trim_end_matches('}').len();
        if closers > 0 {
            // Inside the structure: only its last character, never a space.
            end -= closers;
            if head[..end].ends_with([' ', '{']) {
                return false;
            }
        }
        let body = &head[..end];
        let Some(last) = body.chars().next_back() else {
            return false;
        };
        if matches!(last, '{' | '}' | '$' | '\n' | '\\' | '^' | '_' | '&') {
            return false;
        }
        let mut start = end - last.len_utf8();
        if last.is_ascii_alphabetic() {
            let name = body.len()
                - body
                    .trim_end_matches(|c: char| c.is_ascii_alphabetic())
                    .len();
            let before = &body[..end - name];
            let slashes = before.len() - before.trim_end_matches('\\').len();
            if slashes % 2 == 1 {
                start = end - name - 1;
            }
        }
        for op in [":=", "<=", ">=", "!=", "->", "=="] {
            if body.ends_with(op) {
                start = end - op.len();
            }
        }
        let to = if closers > 0 { end } else { self.caret };
        self.delete_range(start..to);
        true
    }

    /// Backspace at the start of a block in prose: joins it to the block before
    /// (removing the blank line Enter wrote) in one step. False when not applicable.
    pub fn backspace_block(&mut self, in_math: bool) -> bool {
        if in_math || self.selection().is_some() || self.caret == 0 {
            return false;
        }
        let bytes = self.text.as_bytes();
        let mut p = self.caret;
        while p > 0 && matches!(bytes[p - 1], b'\n' | b'\r') {
            p -= 1;
        }
        let newlines = self.text[p..self.caret]
            .bytes()
            .filter(|&b| b == b'\n')
            .count();
        if newlines < 2 || p == 0 {
            return false;
        }
        let head = self.text[..p].trim_end();
        let keep = head.ends_with("$$") || head.ends_with("```");
        let from = if keep { p + 1 } else { p };
        if from >= self.caret {
            return false;
        }
        self.delete_range(from..self.caret);
        true
    }

    /// Called just before a character is typed in prose: a space that completes a
    /// list marker (`- `, `1. `) is an undo step of its own. True when `ch` is
    /// used up.
    pub fn before_type(&mut self, ch: char) -> bool {
        if ch != ' ' || self.selection().is_some() {
            return false;
        }
        let (ls, le) = line_bounds(&self.text, self.caret);
        let t = self.text[ls..self.caret].trim_start_matches([' ', '\t']);
        let is_marker = |t: &str| {
            matches!(t, "-" | "*")
                || (t.len() > 1
                    && t.len() <= 10
                    && t.ends_with('.')
                    && t[..t.len() - 1].bytes().all(|b| b.is_ascii_digit()))
        };
        // On a list item, a marker typed again at the start of its text (Enter
        // already wrote one) is swallowed, so the bullet is not doubled (T9).
        if self.caret == le && !in_fence(&self.text, ls) {
            if let Some((_, _, content)) = list_marker(&self.text[ls..le]) {
                let typed = &self.text[ls + content..self.caret];
                if is_marker(typed) {
                    self.delete_range(ls + content..self.caret);
                    return true;
                }
            }
        }
        if is_marker(t) {
            self.break_step();
        }
        false
    }

    /// The caret left the line starting at `line_start`: an empty list item
    /// there at the end of the file (left behind by Enter) is removed. True
    /// when it was.
    pub fn drop_empty_item(&mut self, line_start: usize) -> bool {
        if line_start == 0 || line_start > self.text.len() || self.selection().is_some() {
            return false;
        }
        let (ls, le) = line_bounds(&self.text, line_start);
        if ls != line_start || le != self.text.len() || self.caret >= ls {
            return false;
        }
        let line = &self.text[ls..le];
        let empty = list_marker(line)
            .is_some_and(|(_, _, c)| line.get(c..).is_none_or(|r| r.trim().is_empty()));
        if !empty || in_fence(&self.text, ls) || !self.text[..ls].ends_with('\n') {
            return false;
        }
        self.convert(ls - 1..le, "");
        true
    }

    /// Called just after a character was typed in prose: `#` then a letter at the
    /// start of a line becomes a heading (the editor writes the space).
    pub fn after_type(&mut self, ch: char) {
        if ch.is_ascii_digit() {
            let at = self.caret - 1;
            let (ls, _) = line_bounds(&self.text, at);
            let t = self.text[ls..at].trim_start_matches([' ', '\t']);
            if t.len() > 1
                && t.ends_with('.')
                && t[..t.len() - 1].bytes().all(|b| b.is_ascii_digit())
                && !in_fence(&self.text, at)
            {
                self.fire_hint(
                    Hint::NumberNotList,
                    "A number stays a number. For a list, type 1. and a space.",
                );
            }
            return;
        }
        if !ch.is_alphabetic() {
            return;
        }
        let at = self.caret - ch.len_utf8();
        let (ls, _) = line_bounds(&self.text, at);
        let before = &self.text[ls..at];
        let hashes = before.len();
        if hashes == 0
            || hashes > 6
            || !before.bytes().all(|b| b == b'#')
            || in_fence(&self.text, at)
        {
            return;
        }
        self.convert(at..at, " ");
        self.fire_hint(
            Hint::Heading,
            &format!("Heading {hashes}. Ctrl+Z keeps the # as plain text."),
        );
    }

    /// Ctrl+1/2/3 (up to 6): makes the caret's line a heading of `level`; level 0
    /// makes it body text. False when the line is not prose.
    pub fn set_heading(&mut self, level: usize, in_math: bool) -> bool {
        if in_math || level > 6 {
            return false;
        }
        let (ls, le) = line_bounds(&self.text, self.caret);
        if in_fence(&self.text, ls) {
            return false;
        }
        let old = heading_prefix(&self.text[ls..le]);
        let new = if level == 0 {
            String::new()
        } else {
            format!("{} ", "#".repeat(level))
        };
        if self.text[ls..ls + old] != new {
            self.convert(ls..ls + old, &new);
        }
        true
    }
}
