//! Lists: `- `, `* ` and `1. ` items, nested by indentation.

use super::ast::{List, ListItem};
use super::inline::parse_inlines;
use crate::span::{Diagnostic, Span};

/// How deep nested lists go before deeper items are added to the same list.
const MAX_NEST: usize = 32;

/// A list marker found at the start of a line.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Marker {
    pub indent: usize,
    pub ordered: bool,
    pub number: Option<u32>,
    /// Offset of the marker in the file.
    pub start: usize,
    /// Offset of the first byte of the item text.
    pub content: usize,
}

/// Recognises a list marker at the start of the line `text` that begins at `line_start`.
pub(crate) fn marker(text: &str, line_start: usize) -> Option<Marker> {
    let trimmed = text.trim_start_matches([' ', '\t']);
    let lead = &text[..text.len() - trimmed.len()];
    let indent = lead.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum();
    let start = line_start + lead.len();

    let (ordered, number, marker_len) = match trimmed.as_bytes().first()? {
        b'-' | b'*' => (false, None, 1),
        b'0'..=b'9' => {
            let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 9 || trimmed.as_bytes().get(digits) != Some(&b'.') {
                return None;
            }
            (true, trimmed[..digits].parse().ok(), digits + 1)
        }
        _ => return None,
    };
    let after = &trimmed[marker_len..];
    // A marker needs a space after it (or nothing, for an empty item).
    if !(after.is_empty() || after.starts_with([' ', '\t'])) {
        return None;
    }
    if after.is_empty() {
        // A lone `-`, `*`, `+` or `1.` is not a list (a number being typed is not one).
        return None;
    }
    let gap = after.len() - after.trim_start_matches([' ', '\t']).len();
    Some(Marker {
        indent,
        ordered,
        number,
        start,
        content: start + marker_len + gap,
    })
}

/// One item before nesting is worked out.
pub(crate) struct Flat {
    pub marker: Marker,
    /// End of the item's text, continuation lines included.
    pub end: usize,
}

/// Turns the flat items of one list block into nested lists. Each list at the
/// top level is returned with its span.
pub(crate) fn build_lists(
    src: &str,
    items: &[Flat],
    diags: &mut Vec<Diagnostic>,
) -> Vec<(List, Span)> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < items.len() {
        let first = pos;
        let (list, end) = build(src, items, &mut pos, 0, diags);
        let start = items.get(first).map_or(0, |i| i.marker.start);
        out.push((list, Span::new(start, end)));
    }
    out
}

/// Builds one list from `items[*pos..]`; returns it and the end offset of its last item.
fn build(
    src: &str,
    items: &[Flat],
    pos: &mut usize,
    depth: usize,
    diags: &mut Vec<Diagnostic>,
) -> (List, usize) {
    let first = &items[*pos];
    let base = first.marker.indent;
    let ordered = first.marker.ordered;
    let mut list = List {
        ordered,
        items: Vec::new(),
    };
    let mut end = first.marker.start;
    while let Some(it) = items.get(*pos) {
        if it.marker.indent < base {
            break;
        }
        if it.marker.indent > base && depth < MAX_NEST {
            if let Some(last) = list.items.last_mut() {
                let (child, child_end) = build(src, items, pos, depth + 1, diags);
                end = end.max(child_end);
                last.span = last.span.join(Span::new(last.span.start, child_end));
                last.children.push(child);
                continue;
            }
        }
        if it.marker.indent == base && it.marker.ordered != ordered {
            break;
        }
        let content = Span::new(it.marker.content.min(it.end), it.end);
        list.items.push(ListItem {
            number: it.marker.number,
            content: parse_inlines(src, content, diags),
            children: Vec::new(),
            span: Span::new(it.marker.start, it.end),
        });
        end = end.max(it.end);
        *pos += 1;
    }
    (list, end)
}
