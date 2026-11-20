//! The document model behind the equation view: the `.mpp` text, a caret and
//! an optional selection, all as byte offsets into the source.
//!
//! The view maps screen positions to offsets with the page's caret stops; this
//! module only edits text. Its small API (`insert_text`, `delete`, `move_caret`,
//! `replace_node`) is what the template and box editor builds on.

pub mod symbols;

use std::ops::Range;

use mpp_syntax::Span;

const HISTORY_LIMIT: usize = 500;

/// How consecutive edits group into one undo step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditKind {
    Typing,
    Other,
}

#[derive(Debug, Clone)]
struct Snapshot {
    text: String,
    caret: usize,
}

/// The text of a document with its caret and selection.
#[derive(Debug, Clone, Default)]
pub struct Editor {
    text: String,
    caret: usize,
    anchor: Option<usize>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last_kind: Option<EditKind>,
    revision: u64,
}

impl Editor {
    /// An editor holding `text`, with the caret at the start.
    pub fn new(text: impl Into<String>) -> Self {
        Editor {
            text: text.into(),
            ..Editor::default()
        }
    }

    /// The whole text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The caret's byte offset.
    pub fn caret(&self) -> usize {
        self.caret
    }

    /// Bumps on every change to the text.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The selected range, if any.
    pub fn selection(&self) -> Option<Range<usize>> {
        let a = self.anchor?;
        if a == self.caret {
            return None;
        }
        Some(a.min(self.caret)..a.max(self.caret))
    }

    /// The selected text.
    pub fn selected_text(&self) -> Option<&str> {
        self.selection().and_then(|r| self.text.get(r))
    }

    /// Replaces the whole text (a file was loaded); clears the history.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.caret = 0;
        self.anchor = None;
        self.undo.clear();
        self.redo.clear();
        self.last_kind = None;
        self.revision += 1;
    }

    fn clamp(&self, offset: usize) -> usize {
        let mut o = offset.min(self.text.len());
        while o > 0 && !self.text.is_char_boundary(o) {
            o -= 1;
        }
        o
    }

    fn record(&mut self, kind: EditKind, contiguous: bool) {
        let group =
            kind == EditKind::Typing && self.last_kind == Some(EditKind::Typing) && contiguous;
        if !group {
            self.undo.push(Snapshot {
                text: self.text.clone(),
                caret: self.caret,
            });
            if self.undo.len() > HISTORY_LIMIT {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.last_kind = Some(kind);
    }

    /// Moves the caret to `offset`, extending the selection when `extend`.
    pub fn move_caret(&mut self, offset: usize, extend: bool) {
        let o = self.clamp(offset);
        if extend {
            self.anchor.get_or_insert(self.caret);
        } else {
            self.anchor = None;
        }
        self.caret = o;
        self.last_kind = None;
    }

    /// Selects a range.
    pub fn select(&mut self, range: Range<usize>) {
        self.anchor = Some(self.clamp(range.start));
        self.caret = self.clamp(range.end);
        self.last_kind = None;
    }

    /// Inserts text at the caret, replacing the selection.
    pub fn insert_text(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        let typing = !s.chars().any(char::is_whitespace);
        let contiguous = self.selection().is_none();
        self.record(
            if typing {
                EditKind::Typing
            } else {
                EditKind::Other
            },
            contiguous,
        );
        let range = self.selection().unwrap_or(self.caret..self.caret);
        self.text.replace_range(range.clone(), s);
        self.caret = range.start + s.len();
        self.anchor = None;
        self.revision += 1;
    }

    /// Deletes the selection, or the text between the caret and `target`
    /// (the offset the view says is the previous or next visible position).
    /// When `balanced` is set and that text holds an unmatched brace, the caret
    /// moves to `target` instead, so a structure is entered before it is taken apart.
    pub fn delete(&mut self, target: usize, balanced: bool) {
        if let Some(r) = self.selection() {
            self.record(EditKind::Other, false);
            self.text.replace_range(r.clone(), "");
            self.caret = r.start;
            self.anchor = None;
            self.revision += 1;
            return;
        }
        let t = self.clamp(target);
        if t == self.caret {
            return;
        }
        let range = t.min(self.caret)..t.max(self.caret);
        if balanced && !braces_balanced(&self.text[range.clone()]) {
            self.caret = t;
            self.last_kind = None;
            return;
        }
        self.record(EditKind::Other, false);
        self.text.replace_range(range.clone(), "");
        self.caret = range.start;
        self.revision += 1;
    }

    /// Deletes a range of the text and puts the caret where it was.
    pub fn delete_range(&mut self, range: Range<usize>) {
        let start = self.clamp(range.start);
        let end = self.clamp(range.end).max(start);
        if start == end {
            return;
        }
        self.record(EditKind::Other, false);
        self.text.replace_range(start..end, "");
        self.caret = start;
        self.anchor = None;
        self.revision += 1;
    }

    /// Replaces the source of a node (or any span) and puts the caret after it.
    pub fn replace_node(&mut self, span: Span, text: &str) {
        let start = self.clamp(span.start);
        let end = self.clamp(span.end).max(start);
        self.record(EditKind::Other, false);
        self.text.replace_range(start..end, text);
        self.caret = start + text.len();
        self.anchor = None;
        self.revision += 1;
    }

    /// Replaces a range without making an undo step of its own (live symbol
    /// replacement joins the typing that caused it); the caret keeps its place
    /// relative to the text after the range.
    pub fn replace_inline(&mut self, range: Range<usize>, with: &str) {
        let start = self.clamp(range.start);
        let end = self.clamp(range.end).max(start);
        self.text.replace_range(start..end, with);
        if self.caret >= end {
            self.caret = self.caret - (end - start) + with.len();
        }
        self.anchor = None;
        self.revision += 1;
    }

    /// Steps back one undo step.
    pub fn undo(&mut self) -> bool {
        let Some(s) = self.undo.pop() else {
            return false;
        };
        self.redo.push(Snapshot {
            text: std::mem::replace(&mut self.text, s.text),
            caret: self.caret,
        });
        self.caret = s.caret.min(self.text.len());
        self.anchor = None;
        self.last_kind = None;
        self.revision += 1;
        true
    }

    /// Steps forward again.
    pub fn redo(&mut self) -> bool {
        let Some(s) = self.redo.pop() else {
            return false;
        };
        self.undo.push(Snapshot {
            text: std::mem::replace(&mut self.text, s.text),
            caret: self.caret,
        });
        self.caret = s.caret.min(self.text.len());
        self.anchor = None;
        self.last_kind = None;
        self.revision += 1;
        true
    }
}

fn braces_balanced(s: &str) -> bool {
    let mut depth = 0i32;
    let mut prev = '\0';
    for c in s.chars() {
        if prev != '\\' {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
        }
        prev = c;
    }
    depth == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_inserts_at_the_caret() {
        let mut e = Editor::new("ab");
        e.move_caret(1, false);
        e.insert_text("x");
        assert_eq!(e.text(), "axb");
        assert_eq!(e.caret(), 2);
    }

    #[test]
    fn typing_groups_into_one_undo_step() {
        let mut e = Editor::new("");
        for c in ["a", "b", "c"] {
            e.insert_text(c);
        }
        assert!(e.undo());
        assert_eq!(e.text(), "");
        assert!(e.redo());
        assert_eq!(e.text(), "abc");
    }

    #[test]
    fn delete_to_target_and_selection() {
        let mut e = Editor::new("hello");
        e.move_caret(5, false);
        e.delete(4, false);
        assert_eq!(e.text(), "hell");
        e.select(0..2);
        e.delete(0, false);
        assert_eq!(e.text(), "ll");
    }

    #[test]
    fn unbalanced_delete_enters_the_structure_first() {
        let mut e = Editor::new("\\frac{a}{b}");
        e.move_caret(11, false);
        e.delete(10, true);
        assert_eq!(e.text(), "\\frac{a}{b}");
        assert_eq!(e.caret(), 10);
        e.delete(9, true);
        assert_eq!(e.text(), "\\frac{a}{}");
    }

    #[test]
    fn replace_node_and_multibyte() {
        let mut e = Editor::new("x = α + 1");
        e.replace_node(Span::new(4, 6), "beta");
        assert_eq!(e.text(), "x = beta + 1");
        assert_eq!(e.caret(), 8);
        e.move_caret(5, false);
        assert!(e.text().is_char_boundary(e.caret()));
    }

    #[test]
    fn moves_never_split_a_character() {
        let mut e = Editor::new("αβ");
        e.move_caret(1, false);
        assert_eq!(e.caret(), 0);
    }
}
