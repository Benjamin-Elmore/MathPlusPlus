//! Template snippets and the boxes of an inserted template.
//!
//! A template is written as a small snippet language: plain `.mpp` maths text
//! with boxes in `⟨…⟩` and optional parts in `⟦…⟧`.
//!
//! - `⟨label⟩` is a required box, `⟨?label⟩` an optional one.
//! - `⟨label=default⟩` pre-fills the box (selected when Tab reaches it).
//! - `⟨~label⟩` is a *bare* box: the grammar does not allow braces there
//!   (`dx`, `\lim_{x \to …}`, `f(x)`), so the box is written without them and
//!   gets `{}` only while it is empty.
//! - `⟨*label⟩` is where the caret starts (the integrand, the term …).
//! - `⟨label@2⟩` gives the box's place in the Tab order (default: its place in
//!   the text); `⟨label#key⟩` names it for mirrors and rebuilds.
//! - `⟨^key⟩` mirrors another box: it always shows the same text and is skipped
//!   by Tab (the `x` in `x = a` after a derivative in `x`).
//! - `⟦…⟧` is removed when the template is left with every box in it empty
//!   (empty bounds make an indefinite integral).
//!
//! Every non-bare box must sit directly inside `{` and `}` in the snippet, so the
//! text stays parseable while the box is empty (an empty `{}` is an empty slot).

use std::ops::Range;

/// Knobs a template's controls change (rows of a matrix, region or bounds mode …).
/// Each template reads only the ones it needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Knobs {
    pub rows: u8,
    pub cols: u8,
    /// A mode switch (region/bounds, log base, coordinate system …).
    pub mode: u8,
    /// A second switch (coordinates of a triple integral, the side of a limit …).
    pub variant: u8,
    pub flag: bool,
}

/// One box of a parsed snippet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxSpec {
    pub label: String,
    pub default: String,
    pub required: bool,
    pub bare: bool,
    pub primary: bool,
    pub order: Option<i32>,
    pub key: String,
    /// The key of the box this one mirrors.
    pub mirror: Option<String>,
    /// The byte range of the box's text inside the snippet text.
    pub range: Range<usize>,
    /// True when the box is bare, empty, and wrapped in `{}` for now.
    pub wrapped: bool,
}

/// A snippet turned into text with its boxes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snippet {
    pub text: String,
    /// Boxes in text order.
    pub boxes: Vec<BoxSpec>,
    /// Optional parts: their range and the indices of the boxes inside.
    pub groups: Vec<(Range<usize>, Vec<usize>)>,
}

impl Snippet {
    /// Puts `with` into box `i` (and the mirrors of its key), moving the
    /// boxes after it. An empty bare box loses its `{}` once it has text.
    pub fn set_box(&mut self, i: usize, with: &str) {
        let Some(b) = self.boxes.get(i).cloned() else {
            return;
        };
        let key = b.key.clone();
        self.replace(i, with);
        let mirrors: Vec<usize> = (0..self.boxes.len())
            .filter(|&j| self.boxes[j].mirror.as_deref() == Some(key.as_str()))
            .collect();
        for j in mirrors {
            let t = if with.is_empty() { "{}" } else { with };
            self.replace(j, t);
        }
    }

    fn replace(&mut self, i: usize, with: &str) {
        let b = self.boxes[i].clone();
        let mut range = b.range.clone();
        let mut new_range_start = range.start;
        let unwrap = b.wrapped && !with.is_empty();
        if unwrap {
            range = range.start - 1..range.end + 1;
            new_range_start = range.start;
        }
        let rewrap = b.bare && b.mirror.is_none() && with.is_empty() && !b.wrapped;
        let text = if rewrap {
            new_range_start = range.start + 1;
            "{}".to_string()
        } else {
            with.to_string()
        };
        let delta = text.len() as isize - range.len() as isize;
        self.text.replace_range(range.clone(), &text);
        let shift = |x: usize| (x as isize + delta).max(0) as usize;
        for (j, o) in self.boxes.iter_mut().enumerate() {
            if j == i {
                continue;
            }
            if o.range.start >= range.end {
                o.range = shift(o.range.start)..shift(o.range.end);
            }
        }
        for (g, _) in &mut self.groups {
            if g.start >= range.end {
                *g = shift(g.start)..shift(g.end);
            } else if g.end >= range.end {
                g.end = shift(g.end);
            }
        }
        let bx = &mut self.boxes[i];
        bx.range = new_range_start..new_range_start + with.len();
        if unwrap {
            bx.wrapped = false;
        }
        if rewrap {
            bx.wrapped = true;
        }
    }

    /// Fills boxes from `(key#n, text)` pairs (see [`Session::contents`]).
    pub fn fill_keys(&mut self, contents: &[(String, String)]) {
        let mut seen: Vec<String> = vec![];
        let keys: Vec<(usize, String)> = self
            .boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.mirror.is_none())
            .map(|(i, b)| {
                let n = seen.iter().filter(|k| **k == b.key).count();
                seen.push(b.key.clone());
                (i, format!("{}#{n}", b.key))
            })
            .collect();
        for (i, key) in keys {
            if let Some((_, t)) = contents.iter().find(|(k, _)| *k == key) {
                self.set_box(i, t);
            }
        }
    }

    /// The box the caret starts in.
    pub fn primary(&self) -> Option<usize> {
        self.boxes
            .iter()
            .position(|b| b.primary)
            .or_else(|| self.boxes.iter().position(|b| b.mirror.is_none()))
    }
}

/// A problem in a snippet (only the catalogue writes snippets; a test checks them all).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnippetError(pub String);

/// Parses the snippet language (see the module docs).
pub fn parse_snippet(src: &str) -> Result<Snippet, SnippetError> {
    let mut text = String::new();
    let mut boxes: Vec<BoxSpec> = vec![];
    let mut groups: Vec<(Range<usize>, Vec<usize>)> = vec![];
    let mut open_groups: Vec<(usize, usize)> = vec![];
    let mut chars = src.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        match c {
            '⟨' => {
                let rest = &src[i + c.len_utf8()..];
                let Some(end) = rest.find('⟩') else {
                    return Err(SnippetError(format!("unclosed box in {src}")));
                };
                let spec = &rest[..end];
                // Skip the spec and the closing bracket.
                let stop = i + c.len_utf8() + end;
                while chars.peek().is_some_and(|(j, _)| *j <= stop) {
                    chars.next();
                }
                let mut b = parse_box(spec)?;
                let in_group = !open_groups.is_empty();
                if in_group {
                    b.required = false;
                }
                let start = if b.bare && b.default.is_empty() && b.mirror.is_none() {
                    b.wrapped = true;
                    text.push('{');
                    text.len()
                } else {
                    text.len()
                };
                text.push_str(&b.default);
                b.range = start..text.len();
                if b.wrapped {
                    text.push('}');
                }
                boxes.push(b);
            }
            '⟦' => open_groups.push((text.len(), boxes.len())),
            '⟧' => {
                let Some((start, first_box)) = open_groups.pop() else {
                    return Err(SnippetError(format!("unbalanced ⟧ in {src}")));
                };
                groups.push((start..text.len(), (first_box..boxes.len()).collect()));
            }
            _ => text.push(c),
        }
    }
    if !open_groups.is_empty() {
        return Err(SnippetError(format!("unclosed ⟦ in {src}")));
    }
    // Mirrors start with their source's text.
    for i in 0..boxes.len() {
        if let Some(k) = boxes[i].mirror.clone() {
            let Some(src_box) = boxes.iter().find(|b| b.mirror.is_none() && b.key == k) else {
                return Err(SnippetError(format!("mirror of unknown box {k}")));
            };
            if src_box.default != boxes[i].default {
                return Err(SnippetError(format!("mirror {k} must repeat the default")));
            }
        }
    }
    Ok(Snippet {
        text,
        boxes,
        groups,
    })
}

fn parse_box(spec: &str) -> Result<BoxSpec, SnippetError> {
    if let Some(k) = spec.strip_prefix('^') {
        let (key, default) = match k.split_once('=') {
            Some((a, b)) => (a, b),
            None => (k, ""),
        };
        return Ok(BoxSpec {
            label: String::new(),
            default: default.to_string(),
            required: false,
            bare: true,
            primary: false,
            order: None,
            key: key.to_string(),
            mirror: Some(key.to_string()),
            range: 0..0,
            wrapped: false,
        });
    }
    let mut rest = spec;
    let (mut required, mut bare, mut primary) = (true, false, false);
    loop {
        match rest.chars().next() {
            Some('?') => required = false,
            Some('~') => bare = true,
            Some('*') => primary = true,
            _ => break,
        }
        rest = &rest[1..];
    }
    // `@order` and `#key` may come in either order at the end.
    let mut order = None;
    let mut key = None;
    for _ in 0..2 {
        if let Some((a, o)) = rest.rsplit_once('@') {
            if let Ok(n) = o.parse::<i32>() {
                order = Some(n);
                rest = a;
                continue;
            }
        }
        if let Some((a, k)) = rest.rsplit_once('#') {
            if !k.contains(['=', '@']) {
                key = Some(k.to_string());
                rest = a;
            }
        }
    }
    let (label, default) = match rest.split_once('=') {
        Some((l, d)) => (l, d),
        None => (rest, ""),
    };
    if label.contains(['⟨', '⟩', '⟦', '⟧']) {
        return Err(SnippetError(format!("bad box {spec}")));
    }
    Ok(BoxSpec {
        label: label.to_string(),
        default: default.to_string(),
        required,
        bare,
        primary,
        order,
        key: key.unwrap_or_else(|| label.to_string()),
        mirror: None,
        range: 0..0,
        wrapped: false,
    })
}

/// One box of a template in the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TBox {
    /// The box's text (inside its braces), in document offsets.
    pub range: Range<usize>,
    pub label: String,
    pub default: String,
    pub required: bool,
    pub bare: bool,
    /// Bare and empty: `{}` was added around it for now.
    pub wrapped: bool,
    pub key: String,
    /// Index (in `boxes`) of the box this one mirrors.
    pub mirror_of: Option<usize>,
}

impl TBox {
    /// True for an empty box, or a mirror (which only repeats another box).
    pub fn is_open(&self) -> bool {
        self.mirror_of.is_some() || self.range.is_empty()
    }

    /// True when nothing was typed in the box (it is empty or holds its default).
    pub fn untouched(&self, text: &str) -> bool {
        let t = text.get(self.range.clone()).unwrap_or("");
        t.is_empty() || t == self.default
    }
}

/// A template in the document, with its boxes, kept in step with the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// Unique while the editor lives (set by the editor).
    pub id: u64,
    /// The catalogue id of the template, for rebuilds.
    pub template: String,
    pub knobs: Knobs,
    /// The whole template's text.
    pub range: Range<usize>,
    /// Boxes in text order.
    pub boxes: Vec<TBox>,
    /// Tab order: indices into `boxes` (mirrors are left out).
    pub tab: Vec<usize>,
    /// Optional parts: their range and the indices of their boxes.
    pub groups: Vec<(Range<usize>, Vec<usize>)>,
    /// The box the caret starts in.
    pub primary: Option<usize>,
}

impl Session {
    /// A session for `snippet` inserted at `at`.
    pub fn new(template: &str, knobs: Knobs, snippet: &Snippet, at: usize) -> Session {
        let boxes: Vec<TBox> = snippet
            .boxes
            .iter()
            .map(|b| TBox {
                range: b.range.start + at..b.range.end + at,
                label: b.label.clone(),
                default: b.default.clone(),
                required: b.required,
                bare: b.bare,
                wrapped: b.wrapped,
                key: b.key.clone(),
                mirror_of: None,
            })
            .collect();
        let mut s = Session {
            id: 0,
            template: template.to_string(),
            knobs,
            range: at..at + snippet.text.len(),
            boxes,
            tab: vec![],
            groups: snippet
                .groups
                .iter()
                .map(|(r, b)| (r.start + at..r.end + at, b.clone()))
                .collect(),
            primary: None,
        };
        for (i, b) in snippet.boxes.iter().enumerate() {
            if let Some(k) = &b.mirror {
                s.boxes[i].mirror_of = snippet
                    .boxes
                    .iter()
                    .position(|o| o.mirror.is_none() && &o.key == k);
            }
        }
        let mut order: Vec<(i32, usize)> = snippet
            .boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.mirror.is_none())
            .enumerate()
            .map(|(n, (i, b))| (b.order.unwrap_or(n as i32), i))
            .collect();
        order.sort_by_key(|(o, _)| *o);
        s.tab = order.into_iter().map(|(_, i)| i).collect();
        s.primary = snippet
            .boxes
            .iter()
            .position(|b| b.primary)
            .or_else(|| s.tab.first().copied());
        s
    }

    /// The boxes in Tab order.
    pub fn tab_boxes(&self) -> impl Iterator<Item = &TBox> {
        self.tab.iter().filter_map(|&i| self.boxes.get(i))
    }

    /// The box (index into `boxes`, not a mirror) holding the offset.
    pub fn box_at(&self, offset: usize) -> Option<usize> {
        self.tab
            .iter()
            .copied()
            .filter(|&i| {
                let r = &self.boxes[i].range;
                r.start <= offset && offset <= r.end
            })
            // The innermost when ranges touch: the shortest.
            .min_by_key(|&i| self.boxes[i].range.len())
    }

    /// The position of a box in the Tab order.
    pub fn tab_pos(&self, index: usize) -> Option<usize> {
        self.tab.iter().position(|&i| i == index)
    }

    /// True when the offset is strictly inside the template (or in one of its boxes).
    pub fn holds(&self, offset: usize) -> bool {
        (self.range.start <= offset && offset < self.range.end) || self.box_at(offset).is_some()
    }

    /// The text of each box, by key (with `#n` for repeated keys).
    pub fn contents(&self, text: &str) -> Vec<(String, String)> {
        let mut seen: Vec<String> = vec![];
        let mut out = vec![];
        for b in &self.boxes {
            if b.mirror_of.is_some() {
                continue;
            }
            let n = seen.iter().filter(|k| **k == b.key).count();
            seen.push(b.key.clone());
            let t = text.get(b.range.clone()).unwrap_or("").to_string();
            out.push((format!("{}#{n}", b.key), t));
        }
        out
    }
}

/// How an edit moves a range. Returns `None` when the edit cuts across it.
///
/// An inclusive range grows when text is inserted at either end (a box), an
/// exclusive one does not (a template: typing just before it is outside).
pub fn shift_range(
    r: &Range<usize>,
    edit: &Range<usize>,
    new_len: usize,
    inclusive: bool,
) -> Option<Range<usize>> {
    let (p, q) = (edit.start, edit.end);
    let removed = q - p;
    let apply = |x: usize| x - removed + new_len;
    let insertion = p == q;
    if q < r.start || (q == r.start && !(inclusive && insertion)) {
        return Some(apply(r.start)..apply(r.end));
    }
    if p > r.end || (p == r.end && !(inclusive && insertion)) {
        return Some(r.clone());
    }
    if r.start <= p && q <= r.end {
        if !inclusive && p == r.start && q == r.end {
            // The whole template was replaced or deleted.
            return None;
        }
        return Some(r.start..apply(r.end));
    }
    None
}

/// The likely variable at `offset`: the argument of the last function defined
/// before it (`f(t) := …` gives `t`), otherwise `x`.
pub fn likely_variable(text: &str, offset: usize) -> String {
    let end = offset.min(text.len());
    let mut cut = end;
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    let before = &text[..cut];
    let mut best: Option<String> = None;
    for (i, _) in before.match_indices(":=") {
        let head = before[..i].trim_end();
        let Some(head) = head.strip_suffix(')') else {
            continue;
        };
        let Some(open) = head.rfind('(') else {
            continue;
        };
        let name = head[..open].trim_end();
        if !name
            .chars()
            .last()
            .is_some_and(|c| c.is_alphabetic() || c == '}')
        {
            continue;
        }
        let first = head[open + 1..].split(',').next().unwrap_or("").trim();
        let first = first.trim_start_matches('{').trim_end_matches('}').trim();
        if is_variable(first) {
            best = Some(first.to_string());
        }
    }
    best.unwrap_or_else(|| "x".to_string())
}

fn is_variable(s: &str) -> bool {
    let mut it = s.chars();
    match (it.next(), it.next()) {
        (Some(c), None) => c.is_alphabetic(),
        (Some('\\'), Some(_)) => {
            let name = &s[1..];
            name.chars().all(|c| c.is_ascii_alphabetic())
                && mpp_syntax::symbols::lookup_name(name)
                    .is_some_and(|sym| sym.kind == mpp_syntax::symbols::SymbolKind::Letter)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippets_parse_into_text_and_boxes() {
        let s = parse_snippet(r"\int⟦_{⟨lower⟩}^{⟨upper⟩}⟧ {⟨*integrand⟩} \, d⟨~variable=x⟩")
            .expect("snippet");
        assert_eq!(s.text, r"\int_{}^{} {} \, dx");
        let labels: Vec<&str> = s.boxes.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels, ["lower", "upper", "integrand", "variable"]);
        assert!(!s.boxes[0].required && !s.boxes[1].required);
        assert!(s.boxes[2].required && s.boxes[2].primary);
        assert_eq!(&s.text[s.boxes[3].range.clone()], "x");
        assert_eq!(s.groups.len(), 1);
        assert_eq!(s.groups[0].1, vec![0, 1]);
    }

    #[test]
    fn empty_bare_boxes_get_braces() {
        let s = parse_snippet(r"\lim_{⟨~variable⟩ \to {⟨value⟩}} {⟨expression⟩}").expect("ok");
        assert_eq!(s.text, r"\lim_{{} \to {}} {}");
        assert!(s.boxes[0].wrapped);
        assert_eq!(s.boxes[0].range, 7..7);
    }

    #[test]
    fn tab_order_follows_explicit_places() {
        let s = parse_snippet(
            r"\frac{d}{d{⟨variable=x@1⟩}} {⟨*function@0⟩}⟦, \quad ⟨^variable=x⟩ = {⟨at@2⟩}⟧",
        )
        .expect("ok");
        let sess = Session::new("d", Knobs::default(), &s, 10);
        let order: Vec<&str> = sess.tab_boxes().map(|b| b.label.as_str()).collect();
        assert_eq!(order, ["function", "variable", "at"]);
        assert_eq!(sess.boxes[2].mirror_of, Some(0));
        assert_eq!(sess.range.start, 10);
    }

    #[test]
    fn filling_boxes_moves_the_rest() {
        let mut s = parse_snippet(
            r"\frac{d}{d⟨~variable=x@1⟩} {⟨*function@0⟩}⟦, \quad ⟨^variable=x⟩ = {⟨at@2⟩}⟧",
        )
        .expect("ok");
        s.set_box(0, "t");
        s.set_box(1, "t^2");
        assert_eq!(s.text, r"\frac{d}{dt} {t^2}, \quad t = {}");
        assert_eq!(&s.text[s.boxes[1].range.clone()], "t^2");
        let mut l = parse_snippet(r"\lim_{⟨~variable⟩ \to {⟨value⟩}} {⟨*expression⟩}").expect("ok");
        l.fill_keys(&[
            ("variable#0".into(), "h".into()),
            ("expression#0".into(), "1".into()),
        ]);
        assert_eq!(l.text, r"\lim_{h \to {}} {1}");
        assert!(!l.boxes[0].wrapped);
        l.set_box(0, "");
        assert_eq!(l.text, r"\lim_{{} \to {}} {1}");
    }

    #[test]
    fn bad_snippets_are_reported() {
        assert!(parse_snippet("⟨open").is_err());
        assert!(parse_snippet("⟦x").is_err());
        assert!(parse_snippet("x⟧").is_err());
        assert!(parse_snippet("⟨^nothing⟩").is_err());
    }

    #[test]
    fn ranges_follow_edits() {
        let r = 5..8;
        // Before.
        assert_eq!(shift_range(&r, &(0..1), 3, true), Some(7..10));
        // After.
        assert_eq!(shift_range(&r, &(9..9), 2, true), Some(5..8));
        // Inside a box.
        assert_eq!(shift_range(&r, &(6..7), 0, true), Some(5..7));
        // Insertion at a box's ends grows it.
        assert_eq!(shift_range(&r, &(5..5), 1, true), Some(5..9));
        assert_eq!(shift_range(&r, &(8..8), 1, true), Some(5..9));
        // …but not a template's.
        assert_eq!(shift_range(&r, &(5..5), 1, false), Some(6..9));
        assert_eq!(shift_range(&r, &(8..8), 1, false), Some(5..8));
        // Across a boundary.
        assert_eq!(shift_range(&r, &(4..6), 0, true), None);
        assert_eq!(shift_range(&r, &(7..9), 0, true), None);
        // Replacing a whole box's text.
        assert_eq!(shift_range(&r, &(5..8), 1, true), Some(5..6));
    }

    #[test]
    fn the_likely_variable_comes_from_the_last_function() {
        assert_eq!(likely_variable("", 0), "x");
        let t = "$$ f(t) := t^2 $$\n$$ g(s, u) := s $$\n";
        assert_eq!(likely_variable(t, 5), "x");
        assert_eq!(likely_variable(t, 18), "t");
        assert_eq!(likely_variable(t, t.len()), "s");
        assert_eq!(likely_variable(r"$$ h(\theta) := 1 $$", 20), r"\theta");
        assert_eq!(likely_variable("a := 3", 6), "x");
    }
}
