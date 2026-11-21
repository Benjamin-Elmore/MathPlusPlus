//! Empty lines between blocks: which of them a person sees.
//!
//! One blank line between two blocks only separates them (the file needs it to
//! end the block above), so it is not a line on screen and has no caret stop.
//! Every further empty line is a line on screen: an empty paragraph, at a body
//! line's height, with a caret stop of its own. At the end of the file every
//! line after the separating one is shown (the line Enter just made). Before the
//! first block every line but the one separating it from the block is shown.

/// The offsets on each visible empty line in `src[start..end]`, which lies
/// between the block before (when `prev`) and the block after (when `next`).
/// For each line: its start, then one offset per space or tab on it. Only the
/// whitespace after the last non-blank character counts (a block that was not
/// drawn, such as a hidden note, is passed over).
pub(crate) fn visible_blank_lines(
    src: &str,
    start: usize,
    end: usize,
    prev: bool,
    next: bool,
) -> Vec<Vec<usize>> {
    let end = end.min(src.len());
    let start = start.min(end);
    let Some(run) = src.get(start..end) else {
        return Vec::new();
    };
    // Where the trailing whitespace begins.
    let (start, prev) = match run.char_indices().rev().find(|(_, c)| !c.is_whitespace()) {
        Some((i, c)) => (start + i + c.len_utf8(), true),
        None => (start, prev),
    };
    let ws = &src[start..end];
    let total = ws.matches('\n').count();
    let mut out = Vec::new();
    let mut line_start = start;
    for (index, line) in ws.split('\n').enumerate() {
        let line_end = line_start + line.len();
        let shown = (!prev || index >= 2) && (!next || index + 2 <= total);
        if shown {
            out.push(
                (line_start..=line_end)
                    .filter(|&o| src.is_char_boundary(o))
                    .collect(),
            );
        }
        line_start = line_end + 1;
    }
    out
}

/// True when the caret at `offset` sits on an empty line a person sees, or at
/// the start of a block. Used by the editor to check what Enter made.
#[cfg(test)]
pub(crate) fn on_visible_line(src: &str, offset: usize) -> bool {
    let doc = mpp_syntax::document::parse_document(src);
    if doc.blocks.iter().any(|b| b.span.start == offset) {
        return true;
    }
    let prev = doc.blocks.iter().rev().find(|b| b.span.end <= offset);
    let next = doc.blocks.iter().find(|b| b.span.start >= offset);
    let start = prev.map_or(0, |b| b.span.end);
    let end = next.map_or(src.len(), |b| b.span.start);
    visible_blank_lines(src, start, end, prev.is_some(), next.is_some())
        .iter()
        .any(|l| l.contains(&offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(src: &str, prev: bool, next: bool) -> Vec<Vec<usize>> {
        visible_blank_lines(src, 0, src.len(), prev, next)
    }

    #[test]
    fn one_blank_line_between_blocks_is_not_a_line_on_screen() {
        // "A\n\nB": the whitespace "\n\n" between them.
        assert!(lines("\n\n", true, true).is_empty());
        assert!(lines("\n\n\n", true, true).is_empty());
    }

    #[test]
    fn an_empty_paragraph_between_blocks_is_one_line() {
        // "A\n\n\n\nB": an empty line between two separators.
        assert_eq!(lines("\n\n\n\n", true, true), vec![vec![2]]);
        assert_eq!(lines("\n\n\n\n\n", true, true), vec![vec![2], vec![3]]);
        assert_eq!(lines("\n\n  \n\n", true, true), vec![vec![2, 3, 4]]);
    }

    #[test]
    fn the_line_enter_made_at_the_end_is_shown() {
        assert_eq!(lines("\n\n", true, false), vec![vec![2]]);
        assert!(lines("\n", true, false).is_empty());
        assert_eq!(lines("", false, false), vec![vec![0]]);
        assert_eq!(lines("\n", false, false), vec![vec![0], vec![1]]);
    }

    #[test]
    fn lines_before_the_first_block() {
        assert_eq!(lines("\n\n", false, true), vec![vec![0]]);
        assert!(lines("\n", false, true).is_empty());
    }

    #[test]
    fn the_caret_after_enter_is_on_a_visible_line() {
        assert!(on_visible_line("Hello\n\n", 7));
        assert!(!on_visible_line("Hello\n\n", 6));
        assert!(on_visible_line("Hello\n\nWorld", 7), "a block start");
    }

    #[test]
    fn hidden_text_is_passed_over() {
        let src = "% note\n\n\n\n";
        assert_eq!(
            visible_blank_lines(src, 0, src.len(), false, true),
            vec![vec![src.len() - 2]]
        );
    }
}
