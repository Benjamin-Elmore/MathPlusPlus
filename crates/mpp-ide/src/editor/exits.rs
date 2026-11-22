//! One way out of every structure (design rule 4), in every template.
//!
//! - Tab and Shift+Tab go to the next or previous empty box, then out
//!   ([`Editor::tab`]); an exponent or subscript is passed through.
//! - Right at the end of a box goes to the next box or out; Left at its start
//!   goes the other way ([`Editor::arrow_out`]).
//! - `+ − = < > ,` or Space typed at the end of an exponent or subscript leave
//!   it (`x^2+` gives x²+); `^` at the end of a subscript starts the exponent.
//! - A slip such as `x^^2` is refused, with a hint.
//! - The denominator of a fraction made by a typed `/` ends the same way (and
//!   on `)`), when it closes every bracket it opens: `1/2 + 3` is ½ + 3.
//! - Enter in a box starts the new maths line after the structure.
//! - Esc leaves the box, then the maths (`view.rs`, `escape`).

use super::convert::{has_operand, open_of, SUB, SUP};
use super::Editor;

/// True when every bracket opened in `s` is closed in it.
fn balanced(s: &str) -> bool {
    let mut depth = [0i32; 3];
    for c in s.chars() {
        match c {
            '(' => depth[0] += 1,
            ')' => depth[0] -= 1,
            '[' => depth[1] += 1,
            ']' => depth[1] -= 1,
            '{' => depth[2] += 1,
            '}' => depth[2] -= 1,
            _ => {}
        }
    }
    depth == [0; 3]
}

impl Editor {
    /// True when the box holding `caret` (if any) closes every bracket it opens.
    fn box_balanced(&self, caret: usize) -> bool {
        self.box_here(caret).is_none_or(|(id, i)| {
            let r = self.session(id).expect("session").boxes[i].range.clone();
            balanced(self.text.get(r).unwrap_or(""))
        })
    }

    /// Where `+ − = < > , )` or Space leave the denominator of a fraction made
    /// by a typed `/` (R4, S5): at its end, when it holds something and closes
    /// every bracket it opens, so `1/2 + 3` is ½ + 3 and `1/(2+3)` keeps its
    /// brackets. A `\frac{…}{…}` typed in full keeps what is typed inside.
    pub(super) fn linear_exit(&self, caret: usize) -> Option<(Option<u64>, usize)> {
        let (id, i) = self.box_here(caret)?;
        let s = self.session(id)?;
        let b = &s.boxes[i];
        let last = i + 1 == s.boxes.len();
        if !s.linear || !last || b.range.is_empty() || caret != b.range.end {
            return None;
        }
        balanced(&self.text[b.range.clone()]).then_some((Some(id), s.range.end))
    }

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
    /// when the character was used up (a slip refused).
    pub(super) fn exit_before(&mut self, ch: char) -> bool {
        let caret = self.caret;
        match ch {
            '^' | '_' => {
                if let Some((id, i)) = self.box_here(caret) {
                    let s = self.session(id).expect("session");
                    let b = &s.boxes[i];
                    let kind = if s.template == SUP {
                        Some('^')
                    } else if s.template == SUB {
                        Some('_')
                    } else {
                        None
                    };
                    if let (Some(k), false) = (kind, s.braced) {
                        if caret == b.range.end && !b.range.is_empty() {
                            if k == ch {
                                return self.refuse(ch);
                            }
                            // `x_0^`: out of the subscript, into a new exponent.
                            let end = s.range.end;
                            self.leave_to(Some(id), end);
                        }
                    }
                }
                if has_operand(&self.text, self.caret) {
                    true
                } else {
                    self.refuse(ch)
                }
            }
            '+' | '-' | '−' | '=' | '<' | '>' | ',' | ' ' | ')' => {
                // `)` leaves a script only when it closes nothing inside it.
                let script = self
                    .script_exit(caret)
                    .filter(|_| ch != ')' || self.box_balanced(caret));
                if let Some((id, to)) = script {
                    self.leave_to(id, to);
                }
                // Then out of the denominator of a `/` fraction (`1/n^2,`).
                if let Some((id, to)) = self.linear_exit(self.caret) {
                    self.leave_to(id, to);
                }
                true
            }
            _ => true,
        }
    }

    /// A slip such as `x^^2`: nothing is typed and a hint says why.
    fn refuse(&mut self, ch: char) -> bool {
        let what = if ch == '_' { "A subscript" } else { "A power" };
        self.hint = Some((
            format!("{what} needs something before it, as in x{ch}2."),
            self.revision,
        ));
        false
    }

    /// Right (`forward`) at the end of a template box goes to the start of the
    /// next box, or out of the template after the last one; Left at the start of
    /// a box goes to the end of the box before, or out before the template.
    /// False when the caret is not at the edge of a box (the page moves it).
    pub fn arrow_out(&mut self, forward: bool) -> bool {
        if self.selection().is_some() {
            return false;
        }
        let caret = self.caret;
        let Some((id, i)) = self.box_here(caret) else {
            return false;
        };
        if self.left == Some((id, caret)) {
            return false;
        }
        let s = self.session(id).expect("session");
        let b = &s.boxes[i];
        if forward && caret == b.range.end {
            self.next_box_or_out(id, i);
            return true;
        }
        if !forward && caret == b.range.start {
            let to = match (0..i).rev().find(|&j| s.boxes[j].mirror_of.is_none()) {
                Some(j) => s.boxes[j].range.end,
                None => s.range.start,
            };
            if to == caret {
                return false;
            }
            self.move_caret(to, false);
            return true;
        }
        false
    }

    /// Before Enter in maths: out of every template around the caret, so the new
    /// line starts after the structure, not inside one of its boxes (nor in a
    /// last box that ends where the structure does, the `x` of `dx`). True when
    /// the caret was in a template.
    pub(super) fn leave_templates(&mut self) -> bool {
        let mut guard = 0;
        let mut left = None;
        while let Some(s) = self.session_at(self.caret) {
            let (id, end) = (s.id, s.range.end);
            self.finalize(id);
            let end = self.session(id).map_or(end, |s| s.range.end);
            left = Some((id, end));
            if end == self.caret || guard > 16 {
                break;
            }
            self.move_caret(end, false);
            guard += 1;
        }
        self.left = left;
        left.is_some()
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
        assert_eq!(typed("x_0^2"), "x_{0}^{2}");
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
    fn slips_are_refused() {
        assert_eq!(typed("x^^2"), "x^{2}");
        assert_eq!(typed("^2"), "2");
        assert_eq!(typed("x + _1"), "x + 1");
        assert_eq!(typed("x^2^3"), "x^{23}");
        let mut e = Editor::new("");
        typed_into(&mut e, "x^^");
        assert!(e.hint().is_some_and(|h| h.contains("power")));
    }

    #[test]
    fn right_left_and_tab_walk_a_fraction_then_leave_it() {
        let mut e = Editor::new("");
        typed_into(&mut e, "a/b");
        assert_eq!(e.text(), r"\frac{a}{b}");
        assert!(e.arrow_out(true), "Right at the end of the denominator");
        assert!(!e.arrow_out(true), "already out: the page moves on");
        typed_into(&mut e, "+c");
        assert_eq!(e.text(), r"\frac{a}{b}+c");
        let mut e = Editor::new("");
        typed_into(&mut e, r"\frac ");
        typed_into(&mut e, "1");
        assert!(e.arrow_out(true), "numerator to denominator");
        typed_into(&mut e, "2");
        assert!(e.tab(true));
        typed_into(&mut e, "=");
        assert_eq!(e.text(), r"\frac{1}{2}=");
        // Left at the start of the denominator goes back into the numerator.
        let mut e = Editor::new("");
        typed_into(&mut e, "a/");
        assert!(e.arrow_out(false));
        typed_into(&mut e, "b");
        assert_eq!(e.text(), r"\frac{ab}{}");
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

    #[test]
    fn enter_in_a_box_leaves_the_template_first() {
        let mut e = Editor::new("$$ x $$");
        e.move_caret(4, false);
        typed_into(&mut e, "^2");
        e.enter(true, false);
        assert_eq!(e.text(), "$$ x^{2} \\\\\n $$");
    }
}
