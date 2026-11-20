//! A small scanner over one range of the source, shared by the sentence forms
//! and figures. It matches words without regard to case and finds `$…$` spans.

use crate::span::Span;

pub(crate) struct Cursor<'a> {
    src: &'a str,
    pub pos: usize,
    end: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(src: &'a str, range: Span) -> Self {
        let end = range.end.min(src.len());
        Cursor {
            src,
            pos: range.start.min(end),
            end,
        }
    }

    pub fn rest(&self) -> &'a str {
        self.src.get(self.pos..self.end).unwrap_or("")
    }

    pub fn skip_ws(&mut self) {
        let rest = self.rest();
        self.pos += rest.len() - rest.trim_start().len();
    }

    pub fn at_end(&mut self) -> bool {
        self.skip_ws();
        self.pos >= self.end
    }

    /// Eats the character `c` (after whitespace).
    pub fn eat(&mut self, c: char) -> bool {
        self.skip_ws();
        if self.rest().starts_with(c) {
            self.pos += c.len_utf8();
            true
        } else {
            false
        }
    }

    /// Eats the words of `phrase` ignoring case, with any whitespace between
    /// them. The last word must end at a word boundary.
    pub fn word(&mut self, phrase: &str) -> bool {
        let save = self.pos;
        self.skip_ws();
        for (n, w) in phrase.split(' ').enumerate() {
            if n > 0 {
                let before = self.pos;
                self.skip_ws();
                if self.pos == before {
                    self.pos = save;
                    return false;
                }
            }
            let Some(head) = self.src.get(self.pos..self.pos + w.len()) else {
                self.pos = save;
                return false;
            };
            if self.pos + w.len() > self.end || !head.eq_ignore_ascii_case(w) {
                self.pos = save;
                return false;
            }
            self.pos += w.len();
        }
        if self
            .rest()
            .chars()
            .next()
            .is_some_and(char::is_alphanumeric)
        {
            self.pos = save;
            return false;
        }
        true
    }

    /// Eats a `$…$` span (after whitespace) and returns the text between the
    /// dollars. `$$` is not a span.
    pub fn math(&mut self) -> Option<Span> {
        let save = self.pos;
        self.skip_ws();
        let bytes = self.src.as_bytes();
        if bytes.get(self.pos) != Some(&b'$') || bytes.get(self.pos + 1) == Some(&b'$') {
            self.pos = save;
            return None;
        }
        let start = self.pos + 1;
        let mut k = start;
        while k < self.end {
            match bytes.get(k) {
                Some(b'\\') => k += 2,
                Some(b'$') => {
                    self.pos = k + 1;
                    return Some(Span::new(start, k));
                }
                _ => k += 1,
            }
        }
        self.pos = save;
        None
    }

    /// Eats `[[path]]` (after whitespace) and returns the path span.
    pub fn wiki_path(&mut self) -> Option<Span> {
        let save = self.pos;
        self.skip_ws();
        if !self.rest().starts_with("[[") {
            self.pos = save;
            return None;
        }
        let start = self.pos + 2;
        let Some(rel) = self.src.get(start..self.end).and_then(|s| s.find("]]")) else {
            self.pos = save;
            return None;
        };
        self.pos = start + rel + 2;
        Some(Span::new(start, start + rel))
    }

    /// Eats an optional final `.` and reports whether only whitespace is left.
    pub fn finish_sentence(&mut self) -> bool {
        self.eat('.');
        self.at_end()
    }
}
