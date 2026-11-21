//! Ctrl+/ on the current block: comment it out, or take the comment markers away.
//!
//! A maths block becomes `%{ … %}` (its `$$` lines stay readable and the block is
//! no longer evaluated); prose becomes `% ` lines; several blocks become one
//! `%{ … %}`. On a block that is already a comment the markers come off again.

use std::ops::Range;

use mpp_syntax::document::{parse_document, Block, BlockKind};

/// The edit Ctrl+/ makes for the caret (or the selection): the range to replace
/// and its new text. `None` when the caret is not on a block.
pub fn toggle(
    text: &str,
    caret: usize,
    sel: Option<Range<usize>>,
) -> Option<(Range<usize>, String)> {
    let doc = parse_document(text);
    let (lo, hi) = sel.map_or((caret, caret), |r| (r.start, r.end));
    let blocks: Vec<&Block> = doc
        .blocks
        .iter()
        .filter(|b| b.span.start <= hi && lo <= b.span.end)
        .collect();
    let (first, last) = (blocks.first()?, blocks.last()?);
    let start = line_start(text, first.span.start);
    let end = line_end(text, last.span.end);
    let src = &text[start..end];
    if let [b] = blocks.as_slice() {
        if let BlockKind::Comment(c) = &b.kind {
            return Some((start..end, uncomment(src, c.braces)));
        }
    }
    let maths = blocks
        .iter()
        .any(|b| matches!(b.kind, BlockKind::DisplayMath(_) | BlockKind::Figure(_)));
    let new = if blocks.len() > 1 || maths {
        format!("%{{\n{src}\n%}}")
    } else {
        src.split('\n')
            .map(|l| {
                if l.trim().is_empty() {
                    "%".to_string()
                } else {
                    format!("% {l}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    Some((start..end, new))
}

/// The text of a comment without its markers.
fn uncomment(src: &str, braces: bool) -> String {
    if braces {
        let inner = src.trim();
        let inner = inner.strip_prefix("%{").unwrap_or(inner);
        let inner = inner.strip_suffix("%}").unwrap_or(inner);
        let inner = inner.strip_prefix('\n').unwrap_or(inner);
        let inner = inner.strip_suffix('\n').unwrap_or(inner);
        return inner.trim_start_matches(' ').to_string();
    }
    src.split('\n')
        .map(|l| {
            l.strip_prefix("% ")
                .or_else(|| l.strip_prefix('%'))
                .unwrap_or(l)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn line_start(text: &str, at: usize) -> usize {
    text[..at.min(text.len())].rfind('\n').map_or(0, |i| i + 1)
}

fn line_end(text: &str, at: usize) -> usize {
    let at = at.min(text.len());
    // A span that ends just after a newline ends on the line before it.
    let at = if at > 0 && text.as_bytes()[at - 1] == b'\n' {
        at - 1
    } else {
        at
    };
    text[at..].find('\n').map_or(text.len(), |i| at + i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(text: &str, caret: usize) -> String {
        let (r, new) = toggle(text, caret, None).expect("an edit");
        format!("{}{new}{}", &text[..r.start], &text[r.end..])
    }

    #[test]
    fn prose_becomes_percent_lines_and_back() {
        let text = "# Title\n\nSome words\nover two lines.\n\nMore.\n";
        let caret = text.find("words").unwrap();
        let once = apply(text, caret);
        assert_eq!(
            once,
            "# Title\n\n% Some words\n% over two lines.\n\nMore.\n"
        );
        let twice = apply(&once, once.find("words").unwrap());
        assert_eq!(twice, text);
    }

    #[test]
    fn a_maths_block_becomes_a_brace_comment_and_back() {
        let text = "Intro.\n\n$$ v = ? $$\n\nEnd.\n";
        let caret = text.find("v =").unwrap();
        let once = apply(text, caret);
        assert_eq!(once, "Intro.\n\n%{\n$$ v = ? $$\n%}\n\nEnd.\n");
        let doc = parse_document(&once);
        assert!(doc
            .blocks
            .iter()
            .any(|b| matches!(&b.kind, BlockKind::Comment(c) if c.is_commented_out_maths())));
        let twice = apply(&once, once.find("v =").unwrap());
        assert_eq!(twice, text);
    }

    #[test]
    fn several_blocks_become_one_comment() {
        let text = "One.\n\nTwo.\n\nThree.\n";
        let sel = text.find("One").unwrap()..text.find("Two").unwrap() + 2;
        let (r, new) = toggle(text, sel.end, Some(sel)).unwrap();
        let out = format!("{}{new}{}", &text[..r.start], &text[r.end..]);
        assert_eq!(out, "%{\nOne.\n\nTwo.\n%}\n\nThree.\n");
    }
}
