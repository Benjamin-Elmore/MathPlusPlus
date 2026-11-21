//! Reading the maths marked in a message.
//!
//! The evaluator writes the maths in its notes and error messages between marks
//! (`mpp_core::note`: START latex SPLIT text END), so every place that shows a
//! message can typeset it. This crate does not depend on `mpp-core`, so the marks
//! are repeated here; `mpp-export` tests that the two agree.

/// Opens a piece of maths (the same as `mpp_core::note::MATHS_START`).
pub const MATHS_START: char = '\u{E000}';
/// Parts the LaTeX from the text (the same as `mpp_core::note::MATHS_SPLIT`).
pub const MATHS_SPLIT: char = '\u{E001}';
/// Closes a piece of maths (the same as `mpp_core::note::MATHS_END`).
pub const MATHS_END: char = '\u{E002}';

/// One piece of a message: words, or maths as LaTeX.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotePiece<'a> {
    Text(&'a str),
    Maths(&'a str),
}

/// The words and the maths (as LaTeX) of a message, in order.
pub fn note_pieces(s: &str) -> Vec<NotePiece<'_>> {
    let mut out = vec![];
    let mut rest = s;
    while let Some(open) = rest.find(MATHS_START) {
        let after = &rest[open + MATHS_START.len_utf8()..];
        let Some(close) = after.find(MATHS_END) else {
            break;
        };
        if open > 0 {
            out.push(NotePiece::Text(&rest[..open]));
        }
        let inner = &after[..close];
        let latex = inner.split_once(MATHS_SPLIT).map_or(inner, |(l, _)| l);
        out.push(NotePiece::Maths(latex));
        rest = &after[close + MATHS_END.len_utf8()..];
    }
    if !rest.is_empty() {
        out.push(NotePiece::Text(rest));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marked_maths_is_read_as_latex() {
        let s = format!("whether {MATHS_START}x^{{2}}{MATHS_SPLIT}x²{MATHS_END} converges");
        assert_eq!(
            note_pieces(&s),
            [
                NotePiece::Text("whether "),
                NotePiece::Maths("x^{2}"),
                NotePiece::Text(" converges")
            ]
        );
        assert_eq!(note_pieces("plain"), [NotePiece::Text("plain")]);
    }
}
