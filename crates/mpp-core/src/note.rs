//! Messages with maths in them.
//!
//! A note or an error message is a sentence of words, and the maths in it (a series,
//! an integral, a ratio) must be typeset wherever the message is shown, never printed
//! as flattened text such as `∑_(n=1)^∞ x`. The message stays a `String`, so every
//! error and note keeps its type; each piece of maths in it is marked where the
//! message is made, carrying its LaTeX (for the page, HTML, PDF and TeX) and its
//! Unicode text (the fallback for plain output):
//!
//! ```text
//! START latex SPLIT text END
//! ```
//!
//! The marks are private-use characters, so they never clash with anything a person
//! writes. A place that shows a message reads it with [`pieces`] (or [`map_maths`]),
//! and [`plain`] gives the message as readable Unicode text.

use crate::expr::Expr;

/// Opens a piece of maths in a message.
pub const MATHS_START: char = '\u{E000}';
/// Parts the LaTeX of a piece of maths from its Unicode text.
pub const MATHS_SPLIT: char = '\u{E001}';
/// Closes a piece of maths.
pub const MATHS_END: char = '\u{E002}';

/// `e` as a piece of maths in a message.
pub fn maths(e: &Expr) -> String {
    tex(&e.to_latex(), &e.to_text())
}

/// A piece of maths written by hand: its LaTeX and its Unicode text.
pub fn tex(latex: &str, text: &str) -> String {
    let clean = |s: &str| {
        s.chars()
            .filter(|c| !matches!(*c, MATHS_START | MATHS_SPLIT | MATHS_END))
            .collect::<String>()
    };
    format!(
        "{MATHS_START}{}{MATHS_SPLIT}{}{MATHS_END}",
        clean(latex),
        clean(text)
    )
}

/// One piece of a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Piece<'a> {
    /// Words.
    Text(&'a str),
    /// Maths: its LaTeX and its Unicode text.
    Maths { latex: &'a str, text: &'a str },
}

/// The words and the maths of a message, in order. A message without marks is one
/// piece of words; an unclosed mark is read as words.
pub fn pieces(s: &str) -> Vec<Piece<'_>> {
    let mut out = vec![];
    let mut rest = s;
    while let Some(open) = rest.find(MATHS_START) {
        let after = &rest[open + MATHS_START.len_utf8()..];
        let Some(close) = after.find(MATHS_END) else {
            break;
        };
        if open > 0 {
            out.push(Piece::Text(&rest[..open]));
        }
        let inner = &after[..close];
        let (latex, text) = inner.split_once(MATHS_SPLIT).unwrap_or((inner, inner));
        out.push(Piece::Maths { latex, text });
        rest = &after[close + MATHS_END.len_utf8()..];
    }
    if !rest.is_empty() {
        out.push(Piece::Text(rest));
    }
    out
}

/// True when the message has a piece of maths in it.
pub fn has_maths(s: &str) -> bool {
    s.contains(MATHS_START)
}

/// The message with each piece of maths replaced by `f(latex, text)`.
pub fn map_maths(s: &str, mut f: impl FnMut(&str, &str) -> String) -> String {
    if !has_maths(s) {
        return s.to_string();
    }
    pieces(s)
        .into_iter()
        .map(|p| match p {
            Piece::Text(t) => t.to_string(),
            Piece::Maths { latex, text } => f(latex, text),
        })
        .collect()
}

/// The message as readable Unicode text (each piece of maths as its text).
pub fn plain(s: &str) -> String {
    map_maths(s, |_, text| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maths_is_marked_and_read_back() {
        let x = Expr::sym("x");
        let msg = format!("the terms are the constant {}, not 0", maths(&x.pow(2)));
        assert_eq!(
            pieces(&msg),
            [
                Piece::Text("the terms are the constant "),
                Piece::Maths {
                    latex: "x^{2}",
                    text: "x²"
                },
                Piece::Text(", not 0"),
            ]
        );
        assert_eq!(plain(&msg), "the terms are the constant x², not 0");
        assert_eq!(
            map_maths(&msg, |l, _| format!("${l}$")),
            "the terms are the constant $x^{2}$, not 0"
        );
    }

    #[test]
    fn a_message_without_maths_is_one_piece() {
        assert_eq!(pieces("no maths here"), [Piece::Text("no maths here")]);
        assert_eq!(plain("no maths here"), "no maths here");
        assert!(pieces("").is_empty());
    }

    #[test]
    fn an_unclosed_mark_is_kept_as_words() {
        let s = format!("a {MATHS_START}b");
        assert_eq!(pieces(&s), [Piece::Text(&s)]);
    }

    #[test]
    fn marks_inside_maths_are_dropped() {
        let inner = tex("a", "a");
        let outer = tex(&format!("{inner} + 1"), "a + 1");
        assert_eq!(pieces(&outer).len(), 1, "{outer:?}");
    }
}
