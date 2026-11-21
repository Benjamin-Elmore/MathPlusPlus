//! Brackets that help, not fight (U20).
//!
//! Typing `(` in maths shows a faint ghost `)` after the caret. The ghost is not
//! in the text, so it is never saved; typing `)` writes the real one. Backspace
//! right after `(` takes both away. The bracket next to the caret and its partner
//! are highlighted. The same goes for `[ ]`, `{ }`, `| |` and `\left( \right)`.

use std::ops::Range;

use super::Editor;

/// A closing bracket that is shown but not written: `text` is what typing the
/// closer writes (`)` or `\right)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ghost {
    /// Where the opener starts (its `\left` too).
    pub open: usize,
    /// Just after the opener.
    pub from: usize,
    /// Where the ghost sits: the caret while the user types inside.
    pub at: usize,
    pub close: String,
}

/// The closing character for an opening one.
pub fn closer_for(open: char) -> Option<char> {
    match open {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '|' => Some('|'),
        _ => None,
    }
}

/// The number of backslashes just before `at`.
fn backslashes_before(text: &str, at: usize) -> usize {
    text[..at].bytes().rev().take_while(|&b| b == b'\\').count()
}

impl Editor {
    /// The ghost closer to draw at the caret, if there is one: the text typing its
    /// closer writes.
    pub fn ghost_at_caret(&self) -> Option<&Ghost> {
        self.ghosts.iter().rev().find(|g| self.ghost_live(g))
    }

    fn ghost_live(&self, g: &Ghost) -> bool {
        g.at == self.caret
            && g.from <= g.at
            && self
                .text
                .get(..g.from)
                .and_then(|s| s.chars().next_back())
                .and_then(closer_for)
                .is_some()
    }

    /// Forgets the ghosts the caret has left.
    pub(super) fn prune_ghosts(&mut self) {
        let caret = self.caret;
        self.ghosts.retain(|g| g.from <= caret && caret <= g.at);
    }

    /// Moves the ghosts for an edit of `edit` into `n` bytes.
    pub(super) fn shift_ghosts(&mut self, edit: &Range<usize>, n: usize) {
        self.ghosts.retain_mut(|g| {
            if edit.is_empty() {
                // Typing at the ghost goes before it; typing at the opener's end
                // goes inside.
                if g.at >= edit.start {
                    g.at += n;
                }
                if g.from > edit.start {
                    g.from += n;
                }
                if g.open >= edit.start {
                    g.open += n;
                }
                return true;
            }
            if edit.start < g.from && edit.end > g.open {
                return false;
            }
            g.open = super::move_point(g.open, edit, n);
            g.from = super::move_point(g.from, edit, n);
            g.at = super::move_point(g.at, edit, n);
            g.from <= g.at
        });
    }

    /// A character typed in maths: handles brackets. True when it was dealt with
    /// (the character is already in the text, or wrapped round the selection).
    pub fn type_bracket(&mut self, ch: char) -> bool {
        let is_open = closer_for(ch).is_some();
        // A closer: fills the ghost the caret is at.
        if matches!(ch, ')' | ']' | '}' | '|') {
            if let Some(i) = self
                .ghosts
                .iter()
                .rposition(|g| self.ghost_live(g) && g.close.ends_with(ch))
            {
                let g = self.ghosts.remove(i);
                self.insert_text(&g.close);
                return true;
            }
        }
        if !is_open {
            return false;
        }
        let before = &self.text[..self.caret];
        let left_brace = before.ends_with("\\left\\");
        // `\(`, `\{` and the like are single symbols, not brackets.
        if backslashes_before(&self.text, self.caret) % 2 == 1 && !(left_brace && ch == '{') {
            return false;
        }
        let (open_start, close) = if left_brace {
            (
                self.caret - "\\left\\".len(),
                format!("\\right\\{}", closer_for(ch).unwrap_or(ch)),
            )
        } else if before.ends_with("\\left") {
            (
                self.caret - "\\left".len(),
                format!("\\right{}", closer_for(ch).unwrap_or(ch)),
            )
        } else {
            (self.caret, closer_for(ch).unwrap_or(ch).to_string())
        };
        self.insert_text(&ch.to_string());
        self.prune_ghosts();
        self.ghosts.push(Ghost {
            open: open_start,
            from: self.caret,
            at: self.caret,
            close,
        });
        true
    }

    /// Backspace right after an opener in maths: removes it and its closer (the
    /// real one right after the caret, or the ghost). False when the caret is not
    /// right after an opener.
    pub fn backspace_pair(&mut self) -> bool {
        if self.selection().is_some() || self.caret == 0 {
            return false;
        }
        let ghost = self.ghost_at_caret().cloned();
        let text = &self.text;
        let caret = self.caret;
        let Some(o) = text[..caret].chars().next_back() else {
            return false;
        };
        let Some(c) = closer_for(o) else {
            return false;
        };
        if backslashes_before(text, caret - 1) % 2 == 1 && !text[..caret - 1].ends_with("\\left") {
            return false;
        }
        let head = &text[..caret - o.len_utf8()];
        let (start, close) = if let Some(s) = head.strip_suffix("\\left\\") {
            (s.len(), format!("\\right\\{c}"))
        } else if let Some(s) = head.strip_suffix("\\left") {
            (s.len(), format!("\\right{c}"))
        } else {
            (head.len(), c.to_string())
        };
        let rest = &text[caret..];
        let real = rest.starts_with(&close);
        if !real && ghost.is_none() {
            return false;
        }
        let end = if real { caret + close.len() } else { caret };
        self.ghosts.retain(|g| g.open != start);
        self.delete_range(start..end);
        true
    }
}

/// What a bracket token is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Round,
    Square,
    Curly,
    Bar,
}

#[derive(Debug, Clone)]
struct Tok {
    span: Range<usize>,
    kind: Kind,
    /// `Some(true)` an opener, `Some(false)` a closer, `None` a bar.
    open: Option<bool>,
}

/// The bracket tokens of `text[range]` (offsets are into `text`).
fn tokens(text: &str, range: Range<usize>) -> Vec<Tok> {
    let mut out = vec![];
    let s = &text[range.clone()];
    let b = s.as_bytes();
    let mut i = 0;
    let delim = |c: u8| match c {
        b'(' => Some((Kind::Round, Some(true))),
        b')' => Some((Kind::Round, Some(false))),
        b'[' => Some((Kind::Square, Some(true))),
        b']' => Some((Kind::Square, Some(false))),
        b'{' => Some((Kind::Curly, Some(true))),
        b'}' => Some((Kind::Curly, Some(false))),
        b'|' => Some((Kind::Bar, None)),
        _ => None,
    };
    while i < b.len() {
        let c = b[i];
        if c == b'\\' {
            // `\left(`, `\right)`, `\left\{`, `\{`: one token with its prefix.
            let rest = &s[i..];
            let (prefix, force) = if rest.starts_with("\\left") {
                ("\\left", Some(true))
            } else if rest.starts_with("\\right") {
                ("\\right", Some(false))
            } else {
                ("\\", None)
            };
            let after = i + prefix.len();
            let mut j = after;
            if b.get(j) == Some(&b'\\') && prefix != "\\" {
                j += 1;
            }
            if prefix == "\\" {
                // An escaped bracket is a symbol: `\{` is a brace, `\|` a norm.
                let Some(&n) = b.get(i + 1) else { break };
                if let Some((k, _)) = delim(n).filter(|(k, _)| *k == Kind::Curly) {
                    let open = n == b'{';
                    out.push(Tok {
                        span: range.start + i..range.start + i + 2,
                        kind: k,
                        open: Some(open),
                    });
                }
                i += 2;
                continue;
            }
            if let Some((k, o)) = b.get(j).copied().and_then(delim) {
                out.push(Tok {
                    span: range.start + i..range.start + j + 1,
                    kind: k,
                    open: if k == Kind::Bar { None } else { force.or(o) },
                });
                i = j + 1;
                continue;
            }
            i = after.max(i + 1);
            continue;
        }
        if let Some((k, o)) = delim(c) {
            out.push(Tok {
                span: range.start + i..range.start + i + 1,
                kind: k,
                open: o,
            });
        }
        i += 1;
    }
    out
}

/// Pairs the tokens: (index of one, index of the other).
fn pair_up(toks: &[Tok]) -> Vec<(usize, usize)> {
    let mut pairs = vec![];
    let mut stack: Vec<usize> = vec![];
    let mut bar: Option<usize> = None;
    for (i, t) in toks.iter().enumerate() {
        match t.open {
            Some(true) => stack.push(i),
            Some(false) => {
                if let Some(p) = stack.iter().rposition(|&o| toks[o].kind == t.kind) {
                    pairs.push((stack[p], i));
                    stack.truncate(p);
                }
            }
            None => match bar.take() {
                Some(o) => pairs.push((o, i)),
                None => bar = Some(i),
            },
        }
    }
    pairs
}

/// Every bracket pair in `text[range]`: (opener span, closer span).
pub fn pairs(text: &str, range: Range<usize>) -> Vec<(Range<usize>, Range<usize>)> {
    let toks = tokens(text, range);
    let mut out: Vec<_> = pair_up(&toks)
        .into_iter()
        .map(|(a, b)| (toks[a].span.clone(), toks[b].span.clone()))
        .collect();
    out.sort_by_key(|(a, _)| a.start);
    out
}

/// The bracket next to the caret and its partner, in the maths range holding the
/// caret: just before the caret wins over just after.
pub fn matching(
    text: &str,
    math: Range<usize>,
    caret: usize,
) -> Option<(Range<usize>, Range<usize>)> {
    let toks = tokens(text, math);
    let pairs = pair_up(&toks);
    let near = |pick: &dyn Fn(&Tok) -> bool| toks.iter().position(pick);
    let i = near(&|t| t.span.end == caret).or_else(|| near(&|t| t.span.start == caret))?;
    let (a, b) = pairs.iter().find(|(a, b)| *a == i || *b == i)?;
    let other = if *a == i { *b } else { *a };
    Some((toks[i].span.clone(), toks[other].span.clone()))
}

/// The smallest of `ranges` holding `caret`.
fn smallest_around(ranges: &[mpp_syntax::Span], caret: usize) -> Option<Range<usize>> {
    ranges
        .iter()
        .filter(|r| r.start <= caret && caret <= r.end)
        .min_by_key(|r| r.end - r.start)
        .map(|r| r.start..r.end)
}

/// Draws the ghost closer at the caret and the highlight on the bracket pair the
/// caret is next to.
pub fn paint(
    painter: &egui::Painter,
    page: &crate::page::Page,
    origin: egui::Pos2,
    editor: &Editor,
    size: f32,
    mode: crate::theme::Mode,
) {
    let theme = mode.theme();
    let caret = editor.caret();
    if !page.in_math(caret) {
        return;
    }
    if let Some(math) = smallest_around(&page.math_ranges, caret) {
        if let Some((a, b)) = matching(editor.text(), math, caret) {
            let fill = crate::paint::c32(mode.palette().highlight);
            for r in [a, b] {
                let (Some(s), Some(e)) = (page.caret_stop(r.start), page.caret_stop(r.end)) else {
                    continue;
                };
                let rect = egui::Rect::from_min_max(
                    origin + egui::vec2(s.x.min(e.x), s.top),
                    origin + egui::vec2(s.x.max(e.x).max(s.x + 3.0), s.bottom),
                );
                painter.rect_filled(rect.expand(1.0), 2.0, fill);
            }
        }
    }
    if let (Some(g), Some(s)) = (editor.ghost_at_caret(), page.caret_stop(caret)) {
        let ch = g.close.chars().next_back().map(String::from);
        if let Some(ch) = ch {
            let baseline = s.bottom - (s.bottom - s.top) * 0.28;
            crate::paint::paint_text(
                painter,
                &ch,
                origin + egui::vec2(s.x + 2.0, baseline),
                size,
                mpp_render::FontId::Text,
                crate::paint::c32(theme.muted).gamma_multiply(0.7),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(e: &mut Editor, s: &str) {
        for ch in s.chars() {
            if !e.type_bracket(ch) {
                e.insert_text(&ch.to_string());
            }
        }
    }

    #[test]
    fn an_opener_makes_a_ghost_that_is_not_in_the_text() {
        let mut e = Editor::new("");
        typed(&mut e, "(a");
        assert_eq!(e.text(), "(a");
        assert_eq!(e.ghost_at_caret().map(|g| g.close.as_str()), Some(")"));
    }

    #[test]
    fn typing_the_closer_fills_the_ghost() {
        let mut e = Editor::new("");
        typed(&mut e, "(a)b");
        assert_eq!(e.text(), "(a)b");
        assert!(e.ghost_at_caret().is_none());
    }

    #[test]
    fn nested_ghosts_fill_from_the_inside() {
        let mut e = Editor::new("");
        typed(&mut e, "[(a)");
        assert_eq!(e.text(), "[(a)");
        assert_eq!(e.ghost_at_caret().map(|g| g.close.as_str()), Some("]"));
        typed(&mut e, "]");
        assert_eq!(e.text(), "[(a)]");
    }

    #[test]
    fn backspace_after_an_opener_takes_the_ghost_with_it() {
        let mut e = Editor::new("");
        typed(&mut e, "x+(");
        assert!(e.backspace_pair());
        assert_eq!(e.text(), "x+");
        assert!(e.ghost_at_caret().is_none());
    }

    #[test]
    fn backspace_after_an_opener_takes_a_real_closer_too() {
        let mut e = Editor::new("f()");
        e.move_caret(2, false);
        assert!(e.backspace_pair());
        assert_eq!(e.text(), "f");
        // Not after an opener: nothing to do.
        let mut e = Editor::new("(a)");
        e.move_caret(2, false);
        assert!(!e.backspace_pair());
    }

    #[test]
    fn left_right_gets_a_right_ghost() {
        let mut e = Editor::new("");
        typed(&mut e, "\\left(a");
        assert_eq!(
            e.ghost_at_caret().map(|g| g.close.as_str()),
            Some("\\right)")
        );
        typed(&mut e, ")");
        assert_eq!(e.text(), "\\left(a\\right)");
        let mut e = Editor::new("");
        typed(&mut e, "\\left(");
        assert!(e.backspace_pair());
        assert_eq!(e.text(), "");
    }

    #[test]
    fn the_ghost_goes_when_the_caret_leaves() {
        let mut e = Editor::new("");
        typed(&mut e, "(a");
        e.move_caret(0, false);
        assert!(e.ghost_at_caret().is_none());
        e.move_caret(2, false);
        assert!(e.ghost_at_caret().is_none());
    }

    #[test]
    fn bars_pair_up() {
        let mut e = Editor::new("");
        typed(&mut e, "|x|");
        assert_eq!(e.text(), "|x|");
        assert!(e.ghost_at_caret().is_none());
    }

    #[test]
    fn an_escaped_bracket_is_a_symbol() {
        let mut e = Editor::new("");
        typed(&mut e, "\\{");
        assert!(e.ghost_at_caret().is_none());
    }

    #[test]
    fn the_matching_bracket_is_found_next_to_the_caret() {
        let t = "f(a[b]+c)";
        let all = 0..t.len();
        // After the first `(`.
        assert_eq!(matching(t, all.clone(), 2), Some((1..2, 8..9)));
        // Just before the last `)`.
        assert_eq!(matching(t, all.clone(), 8), Some((8..9, 1..2)));
        // Just after `]`.
        assert_eq!(matching(t, all.clone(), 6), Some((5..6, 3..4)));
        assert_eq!(matching(t, all, 0), None);
    }

    #[test]
    fn left_right_and_braces_and_bars_match() {
        let t = r"\left( x \right) + \left\{ y \right\} + |z|";
        let all = 0..t.len();
        let p = pairs(t, all.clone());
        assert_eq!(p.len(), 3);
        assert_eq!(&t[p[0].0.clone()], r"\left(");
        assert_eq!(&t[p[0].1.clone()], r"\right)");
        assert_eq!(&t[p[1].0.clone()], r"\left\{");
        assert_eq!(&t[p[2].1.clone()], "|");
        assert_eq!(matching(t, all, 6), Some((0..6, 9..16)));
    }

    #[test]
    fn mismatched_brackets_have_no_partner() {
        assert_eq!(matching("(a", 0..2, 1), None);
    }
}
