//! Ways out of a structure. The exponent and subscript boxes that `^`, `_` and
//! `**` open are left by `+ − = < > ,` or Space typed at their end (`x^2+`
//! gives x²+), so linear typing keeps working.

use super::convert::{open_of, SUB, SUP};
use super::Editor;

impl Editor {
    /// Where `+ − = < > ,` or Space leave an exponent or subscript the caret is at
    /// the end of (just after its `}`), if it is.
    fn script_exit(&self, caret: usize) -> Option<(Option<u64>, usize)> {
        if let Some((id, i)) = self.box_here(caret) {
            let s = self.session(id)?;
            let b = &s.boxes[i];
            let script = s.template == SUP || s.template == SUB;
            if !script || s.braced || b.range.is_empty() || caret != b.range.end {
                return None;
            }
            return Some((Some(id), s.range.end));
        }
        // A script nobody is tracking (a file opened later): `x^{2|}`.
        let bytes = self.text.as_bytes();
        if bytes.get(caret) != Some(&b'}') {
            return None;
        }
        let open = open_of(&self.text, caret)?;
        let mark = open.checked_sub(1).map(|i| bytes[i]);
        let escaped = open >= 2 && bytes[open - 2] == b'\\';
        (matches!(mark, Some(b'^' | b'_')) && !escaped && open + 1 < caret)
            .then_some((None, caret + 1))
    }

    /// The ways out that typing a character takes, before it is inserted. False
    /// when the character was used up.
    pub(super) fn exit_before(&mut self, ch: char) -> bool {
        let caret = self.caret;
        if matches!(ch, '+' | '-' | '−' | '=' | '<' | '>' | ',' | ' ') {
            if let Some((id, to)) = self.script_exit(caret) {
                self.leave_to(id, to);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::super::convert::TypingCx;
    use super::*;
    use crate::insert::BuildCx;

    fn typed_into(e: &mut Editor, s: &str) {
        let cx = TypingCx {
            build: BuildCx::default(),
            shortcuts: true,
        };
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
    fn scripts_are_left_by_an_operator_or_a_space() {
        assert_eq!(typed("x^2+1"), "x^{2}+1");
        assert_eq!(typed("x^2 + 1"), "x^{2} + 1");
        assert_eq!(typed("x^-1"), "x^{-1}");
        assert_eq!(typed("a_i,a_j"), "a_{i},a_{j}");
        // With a brace typed, the box is the user's: only `}` closes it.
        assert_eq!(typed("x^{n+1}+1"), "x^{n+1}+1");
        // A script in a file opened later is left the same way.
        let mut e = Editor::new("x^{2}");
        e.move_caret(4, false);
        typed_into(&mut e, "+");
        assert_eq!(e.text(), "x^{2}+");
    }

    #[test]
    fn tab_passes_through_an_exponent_to_the_template_around_it() {
        let mut e = Editor::new("");
        typed_into(&mut e, r"\frac ");
        typed_into(&mut e, "x^2");
        assert!(e.tab(true), "out of the exponent and on to the denominator");
        typed_into(&mut e, "3");
        assert_eq!(e.text(), r"\frac{x^{2}}{3}");
    }
}
