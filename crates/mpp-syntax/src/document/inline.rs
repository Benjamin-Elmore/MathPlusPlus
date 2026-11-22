//! Inline markup: emphasis, strong, `$…$` maths, code, links, footnote
//! references, `\eqref` and escapes.
//!
//! Every construct is found by looking ahead for its closer first, so a
//! delimiter without a partner is plain text (with an error for the ones that
//! must close) and nothing needs backtracking.

use super::ast::{Cite, Inline, InlineKind, InlineMath};
use crate::math::parse_math;
use crate::span::{Diagnostic, Span};

/// How deep emphasis and links may nest before the rest reads as plain text.
const MAX_DEPTH: usize = 24;

/// How far ahead the search for an emphasis or link closer looks, so a run of
/// unmatched openers stays linear.
const MAX_SPAN: usize = 4096;

/// The characters a backslash can escape.
const ESCAPABLE: &[char] = &['$', '*', '[', ']', '\\', '|', '#', '%', '`'];

/// Parses the inlines in `src[range]`. Spans index the whole of `src`.
pub(crate) fn parse_inlines(src: &str, range: Span, diags: &mut Vec<Diagnostic>) -> Vec<Inline> {
    let end = range.end.min(src.len());
    let start = range.start.min(end);
    let mut p = Parser { src, diags };
    p.run(start, end, 0)
}

struct Parser<'a> {
    src: &'a str,
    diags: &'a mut Vec<Diagnostic>,
}

impl Parser<'_> {
    fn byte(&self, i: usize) -> Option<u8> {
        self.src.as_bytes().get(i).copied()
    }

    fn starts(&self, i: usize, pat: &str) -> bool {
        self.src
            .as_bytes()
            .get(i..)
            .is_some_and(|rest| rest.starts_with(pat.as_bytes()))
    }

    fn char_at(&self, i: usize) -> Option<char> {
        self.src.get(i..).and_then(|s| s.chars().next())
    }

    fn is_space_at(&self, i: usize) -> bool {
        self.char_at(i).is_none_or(char::is_whitespace)
    }

    /// The char that ends just before `i`.
    fn char_before(&self, i: usize) -> Option<char> {
        self.src.get(..i).and_then(|s| s.chars().next_back())
    }

    fn run(&mut self, start: usize, end: usize, depth: usize) -> Vec<Inline> {
        let mut out: Vec<Inline> = Vec::new();
        let mut text_start = start;
        let mut i = start;
        while i < end {
            let Some(b) = self.byte(i) else { break };
            let step = match b {
                b'\n' => Some((InlineKind::SoftBreak, i + 1)),
                b'\r' if self.byte(i + 1) == Some(b'\n') && i + 1 < end => {
                    Some((InlineKind::SoftBreak, i + 2))
                }
                b'\\' => self.escape(i, end),
                b'$' => self.math(i, end),
                b'`' => self.code(i, end),
                b'[' => self.bracket(i, end, depth),
                b'*' => self.emphasis(i, end, depth),
                _ => None,
            };
            match step {
                Some((kind, next)) => {
                    flush_text(self.src, &mut out, text_start, i);
                    out.push(Inline {
                        kind,
                        span: Span::new(i, next),
                    });
                    i = next;
                    text_start = i;
                }
                None => {
                    // `$$` inside running text is two literal dollars.
                    i += if b == b'$' && self.byte(i + 1) == Some(b'$') {
                        2
                    } else {
                        1
                    };
                }
            }
        }
        flush_text(self.src, &mut out, text_start, end);
        out
    }

    fn escape(&self, i: usize, end: usize) -> Option<(InlineKind, usize)> {
        if self.starts(i, "\\eqref{") {
            let name_start = i + "\\eqref{".len();
            let close = self.src.get(name_start..end)?.find('}')? + name_start;
            let label = self.src[name_start..close].trim().to_string();
            return Some((
                InlineKind::EqRef {
                    label,
                    number: None,
                },
                close + 1,
            ));
        }
        let c = self.char_at(i + 1).filter(|c| ESCAPABLE.contains(c))?;
        let next = i + 1 + c.len_utf8();
        (next <= end).then_some((InlineKind::Escape(c), next))
    }

    /// The end of the `$…$` that opens at `i`, as the index of its closing `$`.
    fn math_close(&self, i: usize, end: usize) -> Option<usize> {
        let mut k = i + 1;
        while k < end {
            match self.byte(k)? {
                b'\\' => k += 2,
                b'$' => return Some(k),
                _ => k += 1,
            }
        }
        None
    }

    /// The money rule (as in Typora): a `$` followed by a space, or by nothing,
    /// never opens maths, and a `$` straight before a digit opens it only when its
    /// closing `$` hugs the maths (no space before it) and is not itself the
    /// start of a price (no digit after it). So "costs $5" and "$5 or $10" stay
    /// text, while `$0$` and `$1 + 1 = ?$` are still maths. `Err(())` when the
    /// `$` is plain text; `Ok(None)` when it opens maths that never closes.
    fn math_open(&self, i: usize, end: usize) -> Result<Option<usize>, ()> {
        match self.byte(i + 1) {
            Some(b'$') | None => return Err(()),
            Some(b) if b.is_ascii_whitespace() => return Err(()),
            _ => {}
        }
        let digit = self.byte(i + 1).is_some_and(|b| b.is_ascii_digit());
        let close = self.math_close(i, end);
        if digit {
            let Some(c) = close else { return Err(()) };
            let hugs = self.char_before(c).is_some_and(|ch| !ch.is_whitespace());
            let price = self.byte(c + 1).is_some_and(|b| b.is_ascii_digit());
            if !hugs || price {
                return Err(());
            }
        }
        Ok(close)
    }

    fn math(&mut self, i: usize, end: usize) -> Option<(InlineKind, usize)> {
        let Ok(close) = self.math_open(i, end) else {
            return None;
        };
        let Some(close) = close else {
            self.diags.push(
                Diagnostic::error(Span::new(i, i + 1), "this `$` is never closed")
                    .expecting(["a closing `$`"]),
            );
            return None;
        };
        let content = Span::new(i + 1, close);
        let (ast, diags) = parse_math(&self.src[content.start..content.end], content.start);
        self.diags.extend(diags);
        Some((
            InlineKind::Math(InlineMath { ast, raw: content }),
            close + 1,
        ))
    }

    fn code(&mut self, i: usize, end: usize) -> Option<(InlineKind, usize)> {
        let close = self.src.get(i + 1..end)?.find('`')? + i + 1;
        Some((
            InlineKind::Code(self.src[i + 1..close].to_string()),
            close + 1,
        ))
    }

    /// Skips a construct that hides its contents from delimiter matching:
    /// an escape, a `$…$` span or a code span. Returns the index after it.
    fn skip_atomic(&self, i: usize, end: usize) -> Option<usize> {
        match self.byte(i)? {
            b'\\' => Some((i + 2).min(end)),
            b'$' if self.byte(i + 1) != Some(b'$') => self.math_open(i, end).ok()?.map(|c| c + 1),
            b'`' => self.src.get(i + 1..end)?.find('`').map(|c| i + 1 + c + 1),
            _ => None,
        }
    }

    // ---- emphasis and strong -------------------------------------------

    fn emphasis(&mut self, i: usize, end: usize, depth: usize) -> Option<(InlineKind, usize)> {
        if depth >= MAX_DEPTH {
            return None;
        }
        if self.byte(i + 1) == Some(b'*') {
            if i + 2 >= end || self.is_space_at(i + 2) {
                return None;
            }
            let close = self.find_close(i + 2, end, true)?;
            let inner = self.run(i + 2, close, depth + 1);
            return Some((InlineKind::Strong(inner), close + 2));
        }
        if i + 1 >= end || self.is_space_at(i + 1) {
            return None;
        }
        let close = self.find_close(i + 1, end, false)?;
        let inner = self.run(i + 1, close, depth + 1);
        Some((InlineKind::Emphasis(inner), close + 1))
    }

    /// Finds the `*` or `**` that closes a span whose content starts at `from`.
    /// A closer must follow a non-space character; stars of the other width
    /// are skipped as units.
    fn find_close(&self, from: usize, end: usize, strong: bool) -> Option<usize> {
        let end = end.min(from.saturating_add(MAX_SPAN));
        let mut k = from;
        while k < end {
            if let Some(next) = self.skip_atomic(k, end) {
                k = next;
                continue;
            }
            if self.byte(k) != Some(b'*') {
                k += 1;
                continue;
            }
            let double = self.byte(k + 1) == Some(b'*') && k + 1 < end;
            let after_space = k == from || self.char_before(k).is_none_or(char::is_whitespace);
            if double == strong && !after_space {
                return Some(k);
            }
            k += if double { 2 } else { 1 };
        }
        None
    }

    // ---- brackets: wiki links, footnotes, links -------------------------

    fn bracket(&mut self, i: usize, end: usize, depth: usize) -> Option<(InlineKind, usize)> {
        if self.starts(i, "[[") {
            return self.wiki_link(i, end);
        }
        if self.starts(i, "[@") {
            if let Some(found) = self.citation(i, end) {
                return Some(found);
            }
        }
        if self.starts(i, "[^") {
            let close = self.src.get(i + 2..end)?.find(']')? + i + 2;
            let name = &self.src[i + 2..close];
            let valid = !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-');
            return valid.then(|| (InlineKind::FootnoteRef(name.to_string()), close + 1));
        }
        if depth >= MAX_DEPTH {
            return None;
        }
        // `[text](url)`
        let text_end = self.matching_bracket(i, end)?;
        if self.byte(text_end + 1) != Some(b'(') {
            return None;
        }
        let url_start = text_end + 2;
        let url_end = self.src.get(url_start..end)?.find(')')? + url_start;
        let text = self.run(i + 1, text_end, depth + 1);
        let url = self.src[url_start..url_end].trim().to_string();
        Some((InlineKind::Link { text, url }, url_end + 1))
    }

    fn matching_bracket(&self, open: usize, end: usize) -> Option<usize> {
        let end = end.min(open.saturating_add(MAX_SPAN));
        let mut level = 0usize;
        let mut k = open;
        while k < end {
            if let Some(next) = self.skip_atomic(k, end) {
                k = next;
                continue;
            }
            match self.byte(k)? {
                b'[' => level += 1,
                b']' => {
                    level = level.saturating_sub(1);
                    if level == 0 {
                        return Some(k);
                    }
                }
                _ => {}
            }
            k += 1;
        }
        None
    }

    /// `[@key]`, `[@a; @b]`: a citation. Anything else after `[@` is plain text.
    fn citation(&self, i: usize, end: usize) -> Option<(InlineKind, usize)> {
        let close = self.src.get(i + 2..end)?.find(']')? + i + 2;
        let mut cites = Vec::new();
        let mut at = i + 1;
        for part in self.src[i + 1..close].split([';', ',']) {
            let lead = part.len() - part.trim_start().len();
            let key = part.trim().strip_prefix('@')?;
            if key.is_empty() || !key.chars().all(super::refs::is_key_char) {
                return None;
            }
            let start = at + lead;
            cites.push(Cite {
                key: key.to_string(),
                span: Span::new(start, start + 1 + key.len()),
            });
            at += part.len() + 1;
        }
        Some((InlineKind::Cite(cites), close + 1))
    }

    fn wiki_link(&mut self, i: usize, end: usize) -> Option<(InlineKind, usize)> {
        let Some(rel) = self.src.get(i + 2..end).and_then(|s| s.find("]]")) else {
            self.diags.push(
                Diagnostic::error(Span::new(i, i + 2), "this `[[` is never closed")
                    .expecting(["a closing `]]`"]),
            );
            return None;
        };
        let close = i + 2 + rel;
        let inner = &self.src[i + 2..close];
        let (path, alias) = match inner.find('|') {
            Some(bar) => {
                let path = inner[..bar].trim_end_matches('\\');
                (path, Some(inner[bar + 1..].trim().to_string()))
            }
            None => (inner, None),
        };
        Some((
            InlineKind::WikiLink {
                path: path.trim().to_string(),
                alias,
            },
            close + 2,
        ))
    }
}

fn flush_text(src: &str, out: &mut Vec<Inline>, from: usize, to: usize) {
    if from < to {
        if let Some(text) = src.get(from..to) {
            out.push(Inline {
                kind: InlineKind::Text(text.to_string()),
                span: Span::new(from, to),
            });
        }
    }
}
