//! Selecting inside maths (U21).
//!
//! Shift+Left/Right grow the selection by one structure at a time: a name or
//! number, a bracketed group, a command with its groups, a script with what it
//! holds. Reaching the end of the group the selection is in takes the whole group
//! in. Nothing jumps to the whole formula unless the selection crosses its edge.
//! Alt+Shift+Up grows by levels (character, term, bracket, one side, the line).
//! Double-click picks a name or number, triple-click the line. Typing replaces the
//! selection, and an opening bracket wraps it.

use std::ops::Range;

use super::{brackets, Editor};

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The first char at `i`.
fn at(text: &str, i: usize) -> Option<char> {
    text.get(i..).and_then(|s| s.chars().next())
}

/// The last char before `i`.
fn before(text: &str, i: usize) -> Option<char> {
    text.get(..i).and_then(|s| s.chars().next_back())
}

/// The name or number around `at`, in maths (a `\command` counts as a name) or in
/// prose (a word).
pub fn word_range(text: &str, offset: usize, math: bool) -> Option<Range<usize>> {
    if offset > text.len() || !text.is_char_boundary(offset) {
        return None;
    }
    let part =
        |c: char| is_word(c) || (math && c == '.') || (!math && (c == '\'' || c == '\u{2019}'));
    // Prefer the char after the offset, else the one before.
    let (mut lo, mut hi) = (offset, offset);
    let on_after = at(text, offset).is_some_and(part);
    let on_before = before(text, offset).is_some_and(part);
    if !on_after && !on_before {
        return None;
    }
    while let Some(c) = before(text, lo).filter(|&c| part(c)) {
        lo -= c.len_utf8();
    }
    while let Some(c) = at(text, hi).filter(|&c| part(c)) {
        hi += c.len_utf8();
    }
    if math {
        // A command keeps its backslash; a decimal point must have digits both sides.
        if before(text, lo) == Some('\\') {
            lo -= 1;
        }
        let w = &text[lo..hi];
        let w = w.trim_start_matches('.');
        lo = hi - w.len();
        let w = w.trim_end_matches('.');
        hi = lo + w.len();
    }
    (lo < hi).then_some(lo..hi)
}

/// The line holding `offset`, without the newline, cut to `within` (the maths
/// range) and trimmed of blanks.
pub fn line_range(text: &str, offset: usize, within: Option<Range<usize>>) -> Range<usize> {
    let o = offset.min(text.len());
    let mut lo = text[..o].rfind('\n').map_or(0, |i| i + 1);
    let mut hi = text[o..].find('\n').map_or(text.len(), |i| o + i);
    if let Some(w) = within {
        lo = lo.max(w.start);
        hi = hi.min(w.end);
    }
    let s = &text[lo..hi.max(lo)];
    let t = s.trim_start();
    let lo2 = lo + (s.len() - t.len());
    let hi2 = lo2 + t.trim_end().len();
    lo2..hi2
}

/// What moving one structure found.
#[derive(Debug, PartialEq, Eq)]
pub enum Unit {
    /// Move the caret here.
    To(usize),
    /// The group holding the caret ends here: take the whole group.
    Group(Range<usize>),
    /// The edge of the maths.
    Edge,
}

/// The group (opener..closer, brackets included) whose inside holds `caret`.
fn enclosing_group(text: &str, math: &Range<usize>, caret: usize) -> Option<Range<usize>> {
    let mut best: Option<Range<usize>> = None;
    for (a, b) in brackets::pairs(text, math.clone()) {
        let (lo, hi) = (a.start.min(b.start), a.end.max(b.end));
        let (inner_lo, inner_hi) = (a.end.min(b.end), a.start.max(b.start));
        if inner_lo <= caret && caret <= inner_hi && best.as_ref().is_none_or(|r| hi - lo < r.len())
        {
            best = Some(lo..hi);
        }
    }
    best
}

/// The partner of the bracket token at `span` (its whole extent), as one range.
fn group_from(text: &str, math: &Range<usize>, token: &Range<usize>) -> Option<Range<usize>> {
    brackets::pairs(text, math.clone())
        .into_iter()
        .find(|(a, b)| a == token || b == token)
        .map(|(a, b)| a.start.min(b.start)..a.end.max(b.end))
}

/// The end of a command's name (`\frac`) starting at `i`, the backslash.
fn command_end(text: &str, i: usize) -> usize {
    let mut j = i + 1;
    match at(text, j) {
        Some(c) if c.is_ascii_alphabetic() => {
            while let Some(c) = at(text, j).filter(char::is_ascii_alphabetic) {
                j += c.len_utf8();
            }
        }
        Some(c) => j += c.len_utf8(),
        None => {}
    }
    j
}

/// The end of the `{…}` or `[…]` group starting at `i`, if there is one.
fn group_end(text: &str, math: &Range<usize>, i: usize) -> Option<usize> {
    if !matches!(at(text, i), Some('{' | '[' | '(')) {
        return None;
    }
    group_from(text, math, &(i..i + 1)).map(|g| g.end)
}

/// The end of the unit starting at `i` (no leading blanks).
fn unit_end(text: &str, math: &Range<usize>, i: usize) -> usize {
    let Some(c) = at(text, i) else {
        return i;
    };
    if c == '\\' {
        let rest = &text[i..];
        if rest.starts_with("\\left") || rest.starts_with("\\right") {
            if let Some(g) = brackets::pairs(text, math.clone())
                .into_iter()
                .find(|(a, _)| a.start == i)
            {
                return g.1.end;
            }
            return i + 1;
        }
        let mut j = command_end(text, i);
        // `\frac{a}{b}`, `\sqrt[3]{x}`, `\int_0^1`: the groups and scripts after it.
        if text[i + 1..j].chars().all(|c| c.is_ascii_alphabetic()) && j > i + 1 {
            while let Some(e) = group_end(text, math, j).filter(|_| at(text, j) != Some('(')) {
                j = e;
            }
        }
        return j;
    }
    if matches!(c, '(' | '[' | '{') {
        return group_end(text, math, i).unwrap_or(i + 1);
    }
    if c == '^' || c == '_' {
        let j = i + 1;
        return match at(text, j) {
            Some('{') => group_end(text, math, j).unwrap_or(j + 1),
            Some('\\') => command_end(text, j),
            Some(n) if is_word(n) => j + n.len_utf8(),
            _ => j,
        };
    }
    if is_word(c) || c == '.' && at(text, i + 1).is_some_and(|d| d.is_ascii_digit()) {
        let mut j = i;
        while let Some(n) = at(text, j).filter(|&n| {
            is_word(n) || (n == '.' && at(text, j + 1).is_some_and(|d| d.is_ascii_digit()))
        }) {
            j += n.len_utf8();
        }
        return j;
    }
    i + c.len_utf8()
}

/// The start of the unit ending at `i` (no blanks between).
fn unit_start(text: &str, math: &Range<usize>, i: usize) -> usize {
    let Some(c) = before(text, i) else {
        return i;
    };
    if matches!(c, ')' | ']' | '}' | '|') {
        let tok = i - 1;
        // A `\right)` closer: its token starts at the backslash.
        let span = brackets::pairs(text, math.clone())
            .into_iter()
            .find(|(a, b)| a.end == i || b.end == i);
        if let Some((a, b)) = span {
            let _ = tok;
            let mut start = a.start.min(b.start);
            // A command's groups before it: `\frac{a}{b}` ends at its last group.
            while let Some(p) = before(text, start).filter(|p| matches!(p, '}' | ']')) {
                let _ = p;
                match brackets::pairs(text, math.clone())
                    .into_iter()
                    .find(|(x, y)| x.end == start || y.end == start)
                {
                    Some((x, y)) => start = x.start.min(y.start),
                    None => break,
                }
            }
            if let Some(s) = text[..start].rfind('\\') {
                let name = &text[s + 1..start];
                if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphabetic()) {
                    start = s;
                }
            }
            // Scripts before the group: `x^{2}`.
            return start;
        }
        return i - 1;
    }
    if is_word(c) {
        let mut j = i;
        while let Some(p) = before(text, j).filter(|&p| {
            is_word(p) || (p == '.' && before(text, j - 1).is_some_and(|d| d.is_ascii_digit()))
        }) {
            j -= p.len_utf8();
        }
        if before(text, j) == Some('\\') {
            j -= 1;
        }
        // A script: `x^2` selects `^2` backwards when the unit is just that.
        if matches!(before(text, j), Some('^' | '_')) && j == i - c.len_utf8() {
            return j - 1;
        }
        return j;
    }
    i - c.len_utf8()
}

/// One structure forward (`forward`) or back from `caret`, inside the maths range
/// `math`, for a selection grown with Shift+Right or Shift+Left.
pub fn step(text: &str, math: &Range<usize>, caret: usize, forward: bool) -> Unit {
    if forward {
        let mut i = caret;
        while at(text, i).is_some_and(|c| c == ' ') && i < math.end {
            i += 1;
        }
        if i >= math.end {
            return Unit::Edge;
        }
        // The closer of the group the caret is in: take the whole group.
        if let Some(g) =
            enclosing_group(text, math, caret).filter(|g| text_closer_at(text, math, g, i))
        {
            return Unit::Group(g);
        }
        Unit::To(unit_end(text, math, i).min(math.end))
    } else {
        let mut i = caret;
        while before(text, i).is_some_and(|c| c == ' ') && i > math.start {
            i -= 1;
        }
        if i <= math.start {
            return Unit::Edge;
        }
        if let Some(g) =
            enclosing_group(text, math, caret).filter(|g| g.start + 1 >= i && i > g.start)
        {
            return Unit::Group(g);
        }
        if let Some(g) =
            enclosing_group(text, math, caret).filter(|g| opener_ends_at(text, math, g, i))
        {
            return Unit::Group(g);
        }
        Unit::To(unit_start(text, math, i).max(math.start))
    }
}

/// True when the closer of group `g` starts at `i` (the next thing forward).
fn text_closer_at(text: &str, math: &Range<usize>, g: &Range<usize>, i: usize) -> bool {
    brackets::pairs(text, math.clone())
        .into_iter()
        .any(|(a, b)| {
            a.start.min(b.start) == g.start && a.end.max(b.end) == g.end && {
                let closer = if a.start > b.start { &a } else { &b };
                closer.start == i
            }
        })
}

/// True when the opener of group `g` ends at `i` (the next thing backward).
fn opener_ends_at(text: &str, math: &Range<usize>, g: &Range<usize>, i: usize) -> bool {
    brackets::pairs(text, math.clone())
        .into_iter()
        .any(|(a, b)| {
            a.start.min(b.start) == g.start && a.end.max(b.end) == g.end && {
                let opener = if a.start < b.start { &a } else { &b };
                opener.end == i
            }
        })
}

/// The next larger selection round `sel` inside the maths range `math`:
/// the name, the term, the inside of the bracket, the bracket, one side of the
/// relation, the whole line. `None` when `sel` is already the whole line.
pub fn grow(text: &str, math: &Range<usize>, sel: &Range<usize>) -> Option<Range<usize>> {
    let mut cands: Vec<Range<usize>> = vec![];
    if let Some(w) = word_range(text, sel.start, true) {
        cands.push(w);
    }
    if let Some(w) = word_range(text, sel.end, true) {
        cands.push(w);
    }
    let pairs = brackets::pairs(text, math.clone());
    // Scopes: the whole maths range and the inside of each bracket round the selection.
    let mut scopes = vec![math.clone()];
    for (a, b) in &pairs {
        let (lo, hi) = (a.start.min(b.start), a.end.max(b.end));
        let inner = a.end.min(b.end)..a.start.max(b.start);
        if lo <= sel.start && sel.end <= hi {
            cands.push(inner.clone());
            cands.push(lo..hi);
            scopes.push(inner);
        }
    }
    for scope in scopes {
        if scope.start > sel.start || sel.end > scope.end {
            continue;
        }
        let holds = |r: &&Range<usize>| r.start <= sel.start && sel.end <= r.end;
        let sides = split(text, &scope, &pairs, &RELATIONS);
        let side = sides.iter().find(holds).cloned().unwrap_or(scope.clone());
        if sides.len() > 1 {
            cands.push(side.clone());
        }
        cands.extend(
            split(text, &side, &pairs, &['+', '-'])
                .into_iter()
                .find(|r| holds(&r)),
        );
    }
    cands.push(line_range(text, sel.start, Some(math.clone())));
    cands
        .into_iter()
        .map(|r| trim(text, r))
        .filter(|r| {
            r.start >= math.start
                && r.end <= math.end
                && r.start <= sel.start
                && sel.end <= r.end
                && *r != *sel
                && !r.is_empty()
        })
        .min_by_key(|r| r.len())
}

fn trim(text: &str, r: Range<usize>) -> Range<usize> {
    let s = &text[r.clone()];
    let t = s.trim_start();
    let lo = r.start + (s.len() - t.len());
    lo..lo + t.trim_end().len()
}

const RELATIONS: [char; 7] = ['=', '<', '>', '≤', '≥', '≈', '≠'];

/// The segments of `scope` between the top-level `seps` (not inside a bracket),
/// trimmed, the separators left out. A `:=` is one separator.
fn split(
    text: &str,
    scope: &Range<usize>,
    pairs: &[(Range<usize>, Range<usize>)],
    seps: &[char],
) -> Vec<Range<usize>> {
    let mut out = vec![];
    let mut from = scope.start;
    let mut skip_until = 0;
    for (i, c) in text[scope.clone()].char_indices() {
        let i = scope.start + i;
        if i < skip_until {
            continue;
        }
        if let Some((_, b)) = pairs.iter().find(|(a, _)| a.start == i) {
            skip_until = b.end;
            continue;
        }
        if !seps.contains(&c) || i == scope.start {
            continue;
        }
        // `x^-1`, `e^{-x}` and `a = -b`: a sign after `^`, `_`, a bracket or an
        // operator is not a cut.
        let prev = text[scope.start..i].trim_end().chars().next_back();
        if matches!(
            prev,
            None | Some('^' | '_' | '(' | '[' | '{' | '=' | '+' | '-' | '<' | '>' | ',')
        ) {
            continue;
        }
        let colon = c == '=' && text[..i].ends_with(':');
        let sign_start = if colon { i - 1 } else { i };
        out.push(from..sign_start);
        from = i + c.len_utf8();
    }
    out.push(from..scope.end);
    out.into_iter().map(|r| trim(text, r)).collect()
}

impl Editor {
    /// Wraps the selection in brackets (`(` with a selection): one undo step, the
    /// text stays selected inside so another bracket wraps it again.
    pub fn wrap_selection(&mut self, open: char) -> bool {
        let Some(r) = self.selection() else {
            return false;
        };
        let Some(close) = brackets::closer_for(open) else {
            return false;
        };
        let Some(inner) = self.text.get(r.clone()).map(str::to_string) else {
            return false;
        };
        self.replace_node(
            mpp_syntax::Span::new(r.start, r.end),
            &format!("{open}{inner}{close}"),
        );
        self.select(r.start + 1..r.end + 1);
        true
    }

    /// Sets the selection with the anchor at `anchor` and the caret at `caret`.
    pub fn select_from(&mut self, anchor: usize, caret: usize) {
        self.select(anchor..caret);
    }

    /// The anchor of the selection (the caret when there is none).
    pub fn anchor_or_caret(&self) -> usize {
        self.anchor.unwrap_or(self.caret)
    }
}

/// Shift+Left or Shift+Right in maths: the new `(anchor, caret)`, or `None` when
/// the caret is at the edge of the maths and the key should step as usual.
pub fn extend(
    text: &str,
    math: &Range<usize>,
    anchor: usize,
    caret: usize,
    forward: bool,
) -> Option<(usize, usize)> {
    match step(text, math, caret, forward) {
        Unit::To(t) => Some((anchor, t)),
        Unit::Group(g) => Some(if forward {
            (anchor.min(g.start), g.end)
        } else {
            (anchor.max(g.end), g.start)
        }),
        Unit::Edge => None,
    }
}

/// The smallest maths range holding `offset`.
fn math_around(page: &crate::page::Page, offset: usize) -> Option<Range<usize>> {
    page.math_ranges
        .iter()
        .filter(|r| r.start <= offset && offset <= r.end)
        .min_by_key(|r| r.end - r.start)
        .map(|r| r.start..r.end)
}

/// Shift+Left or Shift+Right with the caret in maths: grows the selection one
/// structure. False at the edge of the maths (the key then steps as usual, which
/// is how a selection crosses the edge).
pub fn shift_arrow(editor: &mut Editor, page: &crate::page::Page, forward: bool) -> bool {
    let caret = editor.caret();
    let Some(math) = math_around(page, caret) else {
        return false;
    };
    let anchor = editor.anchor_or_caret();
    match extend(editor.text(), &math, anchor, caret, forward) {
        Some((a, c)) => {
            editor.select_from(a, c);
            true
        }
        None => false,
    }
}

/// Alt+Shift+Up with the caret in maths: the next larger selection (name, term,
/// bracket, one side, the line).
pub fn grow_selection(editor: &mut Editor, page: &crate::page::Page) -> bool {
    let caret = editor.caret();
    let Some(math) = math_around(page, caret) else {
        return false;
    };
    let sel = editor.selection().unwrap_or(caret..caret);
    match grow(editor.text(), &math, &sel) {
        Some(r) => {
            editor.select(r);
            true
        }
        None => false,
    }
}

/// A double-click (a name or number) or triple-click (the line) at the page
/// position `local`. True when something was selected.
pub fn multi_click(
    editor: &mut Editor,
    page: &crate::page::Page,
    local: egui::Pos2,
    triple: bool,
) -> bool {
    let Some((offset, _)) = page.side_at(local) else {
        return false;
    };
    let math = math_around(page, offset);
    let range = if triple {
        Some(line_range(editor.text(), offset, math))
    } else {
        word_range(editor.text(), offset, math.is_some())
    };
    match range.filter(|r| !r.is_empty()) {
        Some(r) => {
            editor.select(r);
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(text: &str, from: usize, forward: bool, n: usize) -> Range<usize> {
        let math = 0..text.len();
        let (mut a, mut c) = (from, from);
        for _ in 0..n {
            let (na, nc) = extend(text, &math, a, c, forward).expect("not at the edge");
            (a, c) = (na, nc);
        }
        a.min(c)..a.max(c)
    }

    #[test]
    fn shift_right_takes_a_name_then_the_next() {
        let t = "xy + 12.5*z";
        assert_eq!(&t[sel(t, 0, true, 1)], "xy");
        assert_eq!(&t[sel(t, 0, true, 2)], "xy +");
        assert_eq!(&t[sel(t, 0, true, 3)], "xy + 12.5");
    }

    #[test]
    fn a_bracket_is_one_step() {
        let t = "(a + b)^2 + c";
        assert_eq!(&t[sel(t, 0, true, 1)], "(a + b)");
        assert_eq!(&t[sel(t, 0, true, 2)], "(a + b)^2");
    }

    #[test]
    fn a_command_with_its_groups_is_one_step() {
        let t = r"\frac{a}{b} + 1";
        assert_eq!(&t[sel(t, 0, true, 1)], r"\frac{a}{b}");
        let t = r"1 + \frac{a}{b}";
        assert_eq!(&t[sel(t, t.len(), false, 1)], r"\frac{a}{b}");
    }

    #[test]
    fn the_end_of_a_group_takes_the_whole_group() {
        let t = "(a + b) c";
        // Caret after `a + `, selecting `b` goes to the closer: the group.
        let math = 0..t.len();
        let (a, c) = extend(t, &math, 5, 5, true).expect("step");
        assert_eq!(&t[a..c], "b");
        let (a, c) = extend(t, &math, a, c, true).expect("step");
        assert_eq!(&t[a.min(c)..a.max(c)], "(a + b)");
    }

    #[test]
    fn never_the_whole_formula_in_the_middle() {
        let t = "a = b + c";
        let r = sel(t, 4, true, 1);
        assert_eq!(&t[r], "b");
    }

    #[test]
    fn the_edge_of_the_maths_is_reported() {
        let t = "a + b";
        let math = 0..t.len();
        assert_eq!(extend(t, &math, 5, 5, true), None);
        assert_eq!(extend(t, &math, 0, 0, false), None);
    }

    #[test]
    fn grow_goes_name_term_bracket_side_line() {
        let t = "y = (a + 2b) + c";
        let math = 0..t.len();
        // From the caret in `b`: the name `2b`, then the inside of the bracket's
        // term, the bracket's inside, the bracket, the right side, the line.
        let mut s = 10..10;
        let mut seen = vec![];
        while let Some(g) = grow(t, &math, &s) {
            seen.push(t[g.clone()].to_string());
            s = g;
        }
        assert_eq!(
            seen,
            vec![
                "2b",
                "a + 2b",
                "(a + 2b)",
                "(a + 2b) + c",
                "y = (a + 2b) + c"
            ]
        );
    }

    #[test]
    fn double_click_picks_a_name_or_number() {
        let t = "x := alpha1 + 12.5";
        assert_eq!(word_range(t, 7, true), Some(5..11));
        assert_eq!(word_range(t, 15, true), Some(14..18));
        assert_eq!(word_range(t, 2, true), None);
        assert_eq!(word_range("it's here", 1, false), Some(0..4));
        assert_eq!(word_range(r"a + \alpha", 6, true), Some(4..10));
    }

    #[test]
    fn triple_click_picks_the_line() {
        let t = "intro\n\n$$ x := 5 $$\n\nmore";
        assert_eq!(line_range(t, 10, Some(10..16)), 10..16);
        assert_eq!(&t[line_range(t, 1, None)], "intro");
    }

    #[test]
    fn an_opener_wraps_the_selection() {
        let mut e = Editor::new("a + b");
        e.select(4..5);
        assert!(e.type_bracket('('));
        assert_eq!(e.text(), "a + (b)");
        assert_eq!(e.selection(), Some(5..6));
        // One step to undo.
        assert!(e.undo());
        assert_eq!(e.text(), "a + b");
    }

    #[test]
    fn typing_replaces_the_selection() {
        let mut e = Editor::new("a + b");
        e.select(4..5);
        e.insert_text("c");
        assert_eq!(e.text(), "a + c");
    }
}
