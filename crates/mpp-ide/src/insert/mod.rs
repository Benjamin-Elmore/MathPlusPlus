//! The Insert menu: templates with empty boxes, put in at the caret.
//!
//! Choosing an item writes its `.mpp` text behind the scenes (`\int_{0}^{1} …`)
//! and starts a template session in the editor, so Tab walks the boxes. In a
//! prose line the template becomes a maths block of its own; symbols in prose
//! become inline maths; graphs become a `Figure:` line.

pub mod catalogue;
pub mod commands;
pub mod menu;

use std::ops::Range;

use crate::editor::{likely_variable, parse_snippet, Editor, Knobs, Snippet};
use crate::page::Page;
pub use catalogue::{
    catalogue, controls, find, mark_current, Action, BuildCx, Control, Group, Template,
};

/// Settings for inserting maths (View → Insert settings), saved between runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsertSettings {
    /// Integral bounds at the side of ∫ instead of above and below it.
    pub integral_side: bool,
    /// `int` + space makes an integral and `/` a fraction.
    pub shortcuts: bool,
    /// Fourier transforms use `k` instead of `ω`.
    pub fourier_k: bool,
    /// Recently used item ids, newest first.
    pub recent: Vec<String>,
}

impl Default for InsertSettings {
    fn default() -> Self {
        InsertSettings {
            integral_side: false,
            shortcuts: true,
            fourier_k: false,
            recent: vec![],
        }
    }
}

/// The Insert menu's state and settings, owned by the app.
#[derive(Debug, Clone, Default)]
pub struct InsertUi {
    pub menu: menu::MenuState,
    pub settings: InsertSettings,
    /// The command list at `\` and `/`.
    pub commands: commands::CommandState,
}

/// How many recent items the menu keeps.
pub const RECENT_LIMIT: usize = 6;

impl InsertSettings {
    /// Moves an item to the front of the recent list.
    pub fn touch(&mut self, id: &str) {
        self.recent.retain(|r| r != id);
        self.recent.insert(0, id.to_string());
        self.recent.truncate(RECENT_LIMIT);
    }

    /// One line for storage.
    pub fn to_stored(&self) -> String {
        format!(
            "{}{}{}|{}",
            u8::from(self.integral_side),
            u8::from(self.shortcuts),
            u8::from(self.fourier_k),
            self.recent.join(",")
        )
    }

    /// Reads what [`InsertSettings::to_stored`] wrote (anything else gives defaults).
    pub fn from_stored(s: &str) -> InsertSettings {
        let mut out = InsertSettings::default();
        let (flags, recent) = s.split_once('|').unwrap_or((s, ""));
        let bit = |i: usize, d: bool| flags.as_bytes().get(i).map_or(d, |b| *b == b'1');
        out.integral_side = bit(0, out.integral_side);
        out.shortcuts = bit(1, out.shortcuts);
        out.fourier_k = bit(2, out.fourier_k);
        out.recent = recent
            .split(',')
            .filter(|id| find(id).is_some())
            .take(RECENT_LIMIT)
            .map(str::to_string)
            .collect();
        out
    }

    /// The builder context at the caret.
    pub fn build_cx(&self, text: &str, caret: usize) -> BuildCx {
        BuildCx {
            var: likely_variable(text, caret),
            fourier_k: self.fourier_k,
        }
    }
}

/// The snippet for an item.
pub fn snippet_for(t: &Template, knobs: Knobs, cx: &BuildCx) -> Option<Snippet> {
    parse_snippet(&t.snippet_source(knobs, cx)).ok()
}

/// The line around `offset`: its range without the newline.
fn line_at(text: &str, offset: usize) -> Range<usize> {
    let o = offset.min(text.len());
    let start = text[..o].rfind('\n').map_or(0, |i| i + 1);
    let end = text[o..].find('\n').map_or(text.len(), |i| o + i);
    start..end
}

/// The newlines in the blanks that end `s` (`None` when `s` is only blanks).
fn newlines_before(s: &str) -> Option<usize> {
    let kept = s.trim_end_matches(char::is_whitespace);
    if kept.is_empty() {
        return None;
    }
    Some(s[kept.len()..].matches('\n').count())
}

/// The newlines in the blanks that start `s` (`None` when `s` is only blanks).
fn newlines_after(s: &str) -> Option<usize> {
    let rest = s.trim_start_matches(char::is_whitespace);
    if rest.is_empty() {
        return None;
    }
    Some(s[..s.len() - rest.len()].matches('\n').count())
}

/// Where a display template goes when the caret is in prose: the range to
/// replace and the text around the template. The block is set off from what is
/// next to it by one blank line, and no more: an empty line is replaced, a line's
/// tail is replaced, a line with words after the caret is split.
fn display_place(text: &str, caret: usize) -> (Range<usize>, String, String) {
    let line = line_at(text, caret);
    let content = &text[line.clone()];
    let blank = content.trim().is_empty();
    let range = if blank {
        line
    } else if text[caret..line.end].trim().is_empty() {
        caret..line.end
    } else {
        caret..caret
    };
    // A blank line between the maths and the text next to it.
    let gap = |have: usize| 2usize.saturating_sub(have);
    let lead = newlines_before(&text[..range.start]).map_or(0, gap);
    let tail = match newlines_after(&text[range.end..]) {
        // The end of the file: one newline closes the block's line.
        None => usize::from(!blank && !text[range.end..].contains('\n')),
        Some(n) => gap(n),
    };
    (
        range,
        format!("{}$$ ", "\n".repeat(lead)),
        format!(" $${}", "\n".repeat(tail)),
    )
}

/// Where a `Figure:` line goes: on an empty line, or after the caret's block.
fn figure_place(text: &str, caret: usize, page: &Page) -> (Range<usize>, String, String) {
    let line = line_at(text, caret);
    if text[line.clone()].trim().is_empty() {
        return (line, String::new(), String::new());
    }
    let end = page
        .block_at(caret)
        .map_or(line.end, |b| b.span.end.max(line.end))
        .min(text.len());
    let end = line_at(text, end).end;
    let tail = if end < text.len() { "" } else { "\n" };
    (end..end, "\n\n".into(), tail.into())
}

/// Inserts an item at the caret (replacing the selection; selected maths goes
/// into the template's first box). Returns the new template's id, if it has boxes.
pub fn insert(
    editor: &mut Editor,
    page: &Page,
    t: &Template,
    knobs: Knobs,
    cx: &BuildCx,
) -> Option<u64> {
    let sel = editor.selection();
    let caret = sel.as_ref().map_or(editor.caret(), |r| r.start);
    let in_math = page.in_math(caret) && sel.as_ref().is_none_or(|r| page.in_math(r.end));
    if let catalogue::Body::Symbol(s) = t.body {
        if in_math {
            editor.insert_text(s);
        } else {
            editor.insert_text(&format!("${s}$"));
        }
        return None;
    }
    if let catalogue::Body::Block(body) = t.body {
        insert_block(editor, page, &t.id, body, caret);
        return None;
    }
    let mut snippet = snippet_for(t, knobs, cx)?;
    if t.is_figure() {
        let (range, before, after) = figure_place(editor.text(), caret, page);
        return Some(editor.insert_template(range, &before, &snippet, &after, &t.id, knobs));
    }
    if in_math {
        let range = sel.unwrap_or(caret..caret);
        if let (Some(text), Some(p)) = (editor.text().get(range.clone()), snippet.primary()) {
            if !text.trim().is_empty() {
                let text = text.trim().to_string();
                snippet.set_box(p, &text);
            }
        }
        let (before, after) = spacing(editor.text(), &range);
        return Some(editor.insert_template(range, before, &snippet, after, &t.id, knobs));
    }
    let (range, before, after) = display_place(editor.text(), caret);
    Some(editor.insert_template(range, &before, &snippet, &after, &t.id, knobs))
}

/// Puts a prose block (a note, a theorem, the title block) in a block of its own
/// after the caret's block, with the caret where `⟨⟩` marks (or at its end). The
/// title block goes at the top of the file.
/// The next free footnote number: one more than the largest `[^n]` in `text`.
fn next_footnote(text: &str) -> u32 {
    text.match_indices("[^")
        .filter_map(|(i, _)| {
            let rest = &text[i + 2..];
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            rest[digits.len()..]
                .starts_with(']')
                .then(|| digits.parse::<u32>().ok())
                .flatten()
        })
        .max()
        .map_or(1, |n| n.saturating_add(1))
}

/// A footnote: the mark `[^n]` at the caret (or at the end of the prose line
/// above, when the caret is on an empty line) and its note `[^n]: ` as a block
/// after the paragraph, with the caret in the note. One edit, one undo step.
fn insert_footnote(editor: &mut Editor, page: &Page, caret: usize) {
    let text = editor.text().to_string();
    let n = next_footnote(&text);
    let mark = format!("[^{n}]");
    let note = format!("[^{n}]: ");
    let (at, before, after) = figure_place(&text, caret, page);
    let line = line_at(&text, caret);
    let in_prose = !page.in_math(caret) && !text[line.clone()].trim().is_empty();
    // Where the mark goes: at the caret in prose, else the end of the prose line
    // above (not a display, a heading or a note of its own).
    let mark_at = if in_prose {
        // Never inside a word: after the word the caret is in.
        let rest = &text[caret..line.end];
        let word = rest
            .find(|c: char| !c.is_alphanumeric())
            .unwrap_or(rest.len());
        Some(caret + word)
    } else {
        let mut end = line.start;
        let mut found = None;
        while end > 0 {
            let l = line_at(&text, end - 1);
            let t = text[l.clone()].trim();
            if !t.is_empty() {
                let prose = !t.starts_with("$$")
                    && !t.ends_with("$$")
                    && !t.starts_with('#')
                    && !t.starts_with("[^")
                    && !t.starts_with("Figure:");
                if prose {
                    found = Some(l.start + text[l.clone()].trim_end().len());
                }
                break;
            }
            end = l.start;
        }
        found
    };
    match mark_at.filter(|&m| m <= at.start) {
        Some(m) => {
            let between = &text[m..at.start];
            // The note is a block of its own: a blank line before it.
            let gap = if before.is_empty() && !between.ends_with("\n\n") {
                "\n"
            } else {
                ""
            };
            let new = format!("{mark}{between}{before}{gap}{note}{after}");
            editor.replace_node(mpp_syntax::Span::new(m, at.end), &new);
            editor.move_caret(m + new.len() - after.len(), false);
        }
        None => {
            let start = at.start;
            editor.replace_node(
                mpp_syntax::Span::new(at.start, at.end),
                &format!("{before}{note}{after}"),
            );
            editor.move_caret(start + before.len() + note.len(), false);
        }
    }
}

fn insert_block(editor: &mut Editor, page: &Page, id: &str, body: &str, caret: usize) {
    if id == "doc.footnote" {
        insert_footnote(editor, page, caret);
        return;
    }
    let (cut, rest) = body.split_once("⟨⟩").unwrap_or((body, ""));
    let text = format!("{cut}{rest}");
    let (at, before, after) = if id == "doc.title" {
        (0..0, String::new(), "\n\n".to_string())
    } else {
        figure_place(editor.text(), caret, page)
    };
    let start = at.start;
    editor.replace_node(
        mpp_syntax::Span::new(at.start, at.end),
        &format!("{before}{text}{after}"),
    );
    editor.move_caret(start + before.len() + cut.len(), false);
}

/// A space around a template that would otherwise touch a letter (`2\int`).
fn spacing(text: &str, range: &Range<usize>) -> (&'static str, &'static str) {
    let before = text[..range.start].chars().next_back();
    let after = text[range.end..].chars().next();
    let wordy = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '}');
    (
        if wordy(before) { " " } else { "" },
        if wordy(after) { " " } else { "" },
    )
}

/// Applies a template control (mode switch, add a row, ±∞ …) to template `id`.
pub fn apply(editor: &mut Editor, id: u64, action: &Action, cx: &BuildCx) -> Option<u64> {
    let s = editor.session(id)?.clone();
    let t = find(&s.template)?;
    match action {
        Action::Rebuild { knobs, reset } => {
            let mut snippet = snippet_for(t, *knobs, cx)?;
            let contents: Vec<(String, String)> = s
                .contents(editor.text())
                .into_iter()
                .filter(|(k, _)| !reset.iter().any(|r| k.starts_with(&format!("{r}#"))))
                .collect();
            snippet.fill_keys(&contents);
            editor.rebuild_template(id, &snippet, *knobs)
        }
        Action::Fill { keys, text } => {
            let caret = editor.caret();
            let current = s
                .box_at(caret)
                .filter(|&i| keys.iter().any(|k| *k == s.boxes[i].key));
            let target = current.or_else(|| {
                keys.iter().find_map(|k| {
                    s.boxes
                        .iter()
                        .position(|b| &b.key == k && b.mirror_of.is_none())
                })
            })?;
            editor.set_box_text(id, target, text);
            Some(id)
        }
        Action::Swap(pairs) => {
            let mut contents = s.contents(editor.text());
            for (a, b) in pairs {
                let (ka, kb) = (format!("{a}#0"), format!("{b}#0"));
                let ta = contents
                    .iter()
                    .find(|(k, _)| *k == ka)
                    .map(|(_, t)| t.clone());
                let tb = contents
                    .iter()
                    .find(|(k, _)| *k == kb)
                    .map(|(_, t)| t.clone());
                if let (Some(ta), Some(tb)) = (ta, tb) {
                    for (k, t) in &mut contents {
                        if *k == ka {
                            *t = tb.clone();
                        } else if *k == kb {
                            *t = ta.clone();
                        }
                    }
                }
            }
            let mut snippet = snippet_for(t, s.knobs, cx)?;
            snippet.fill_keys(&contents);
            editor.rebuild_template(id, &snippet, s.knobs)
        }
    }
}

/// Fast-typist shortcuts, checked after each character typed in maths:
/// `int` + space becomes a definite integral and `/` a fraction (with the
/// operand before it as the numerator). Returns true when one fired.
pub fn shortcut(editor: &mut Editor, typed: char, cx: &BuildCx) -> bool {
    let caret = editor.caret();
    let text = editor.text();
    if typed == ' ' {
        let Some(start) = caret.checked_sub(4) else {
            return false;
        };
        if text.get(start..caret) != Some("int ") {
            return false;
        }
        let prev = text[..start].chars().next_back();
        if prev.is_some_and(|c| c.is_alphanumeric() || c == '\\') {
            return false;
        }
        let Some(t) = find("int.def") else {
            return false;
        };
        let Some(snippet) = snippet_for(t, t.knobs, cx) else {
            return false;
        };
        editor.insert_template(start..caret, "", &snippet, "", &t.id, t.knobs);
        return true;
    }
    if typed == '/' {
        let Some(slash) = caret.checked_sub(1) else {
            return false;
        };
        if text.get(slash..caret) != Some("/") {
            return false;
        }
        // `a / b` with spaces: the operand is still `a`.
        let end = slash - (text[..slash].len() - text[..slash].trim_end_matches(' ').len());
        let mut operand_start = operand_before(text, end);
        let numerator = text[operand_start..end].to_string();
        if numerator.is_empty() {
            // Nothing to take: the spaces before the `/` stay.
            operand_start = slash;
        }
        let Some(t) = find("frac") else {
            return false;
        };
        let Some(mut snippet) = snippet_for(t, t.knobs, cx) else {
            return false;
        };
        snippet.set_box(0, &numerator);
        let id = editor.insert_template(operand_start..caret, "", &snippet, "", &t.id, t.knobs);
        if !numerator.is_empty() {
            editor.focus_box(id, 1);
        }
        return true;
    }
    false
}

/// The start of the operand just before `end`: a bracketed group with the function
/// name before it (`\sin(x)`, `(x + 1)`, `\sqrt{x}`), or a run of letters and
/// digits (with the backslash of a command such as `\pi`).
fn operand_before(text: &str, end: usize) -> usize {
    let before = &text[..end];
    if before.ends_with([')', ']', '}']) {
        let Some(open) = matching_open(before) else {
            return end;
        };
        // A power or index (`v^{2}`, `(x+1)^{2}`): the operand is its base with it.
        if before.ends_with('}') && before[..open].ends_with(['^', '_']) {
            let base = operand_before(text, open - 1);
            return if base < open - 1 { base } else { end };
        }
        let mut start = open;
        if before[..start].ends_with(r"\left") {
            start -= r"\left".len();
        }
        // A command's argument (`\sqrt{x}`, `\frac{a}{b}`): take the whole command.
        if before.ends_with('}') {
            while before[..start].ends_with('}') {
                match matching_open(&before[..start]) {
                    Some(o) => start = o,
                    None => break,
                }
            }
        }
        // The function the brackets belong to: `\sin`, `f`, `\sin^2`.
        let name = word_before(before, start);
        return if name < start { name } else { start };
    }
    word_before(before, end)
}

/// The start of the letters and digits just before `end`, with a command's
/// backslash and its scripts (`\sin^2`).
fn word_before(text: &str, end: usize) -> usize {
    let mut end = end;
    // Scripts written before the bracket: `\sin^{2}(x)`, `f^2(x)`.
    loop {
        let t = &text[..end];
        if let Some(o) = t.strip_suffix('}').and_then(|_| matching_open(t)) {
            if text[..o].ends_with(['^', '_']) {
                end = o - 1;
                continue;
            }
        }
        let mut cs = t.char_indices().rev();
        if let (Some((_, d)), Some((i, m))) = (cs.next(), cs.next()) {
            if d.is_ascii_alphanumeric() && (m == '^' || m == '_') {
                end = i;
                continue;
            }
        }
        break;
    }
    let mut start = end;
    for (i, c) in text[..end].char_indices().rev() {
        if c.is_ascii_alphanumeric() || c == '.' || (!c.is_ascii() && c.is_alphabetic()) {
            start = i;
        } else {
            if c == '\\' && start < end {
                start = i;
            }
            break;
        }
    }
    start
}

/// The byte offset of the bracket that opens the one `text` ends with.
fn matching_open(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in text.char_indices().rev() {
        match c {
            ')' | ']' | '}' => depth += 1,
            '(' | '[' | '{' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// The longer name used in the "fill in" hint.
pub fn long_label(label: &str) -> String {
    match label {
        "lower" => "lower bound".into(),
        "upper" => "upper bound".into(),
        "" => "the entries".into(),
        other => other.into(),
    }
}

/// What the hint beside an equation with an empty box says to type, in plain
/// words (S13): "type the top", "type what is summed".
pub fn plain_label(label: &str) -> String {
    match label {
        "numerator" => "the top".into(),
        "denominator" => "the bottom".into(),
        "term" => "what is summed".into(),
        "integrand" => "what is integrated".into(),
        "" => "in the empty boxes".into(),
        other => format!("the {}", long_label(other)),
    }
}

/// The hint for a maths block (`span`), if it has an empty box
/// that must be filled before it can be worked out.
pub fn fill_in_hint(editor: &Editor, page: &Page, span: &Range<usize>) -> Option<String> {
    let inside = |o: usize| span.start <= o && o <= span.end;
    let missing: Vec<(usize, String)> = editor
        .missing_boxes()
        .into_iter()
        .filter(|(o, _)| inside(*o))
        .collect();
    // The box being typed in comes first (an improper integral's integrand, not
    // its lower bound), then the first one in the text.
    let caret = editor.caret();
    let first = missing
        .iter()
        .find(|(o, _)| *o == caret)
        .or_else(|| missing.first());
    if let Some((_, label)) = first {
        return Some(format!("type {}", plain_label(label)));
    }
    // Empty boxes nobody is tracking (a file opened later): every one counts,
    // except optional boxes of a template that is being edited.
    let tracked: Vec<usize> = editor
        .sessions()
        .iter()
        .flat_map(|s| s.boxes.iter().map(|b| b.range.start))
        .collect();
    let block = page.blocks.iter().find(|b| b.span.start == span.start)?;
    let mut slots: Vec<&mpp_render::HitBox> = block
        .dl
        .hits
        .iter()
        .filter(|h| h.kind == mpp_render::HitKind::Slot && !tracked.contains(&h.span.start))
        .collect();
    slots.sort_by_key(|h| h.span.start);
    let first = slots.first()?;
    Some(format!(
        "type {}",
        plain_label(first.label.as_deref().unwrap_or(""))
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip() {
        let mut s = InsertSettings {
            integral_side: true,
            shortcuts: false,
            fourier_k: true,
            recent: vec![],
        };
        s.touch("frac");
        s.touch("int.def");
        s.touch("frac");
        assert_eq!(s.recent, ["frac", "int.def"]);
        let back = InsertSettings::from_stored(&s.to_stored());
        assert_eq!(back, s);
        assert_eq!(
            InsertSettings::from_stored("junk"),
            InsertSettings {
                integral_side: false,
                shortcuts: false,
                fourier_k: false,
                recent: vec![],
            }
        );
        for i in 0..20 {
            s.touch(if i % 2 == 0 { "sum" } else { "prod" });
        }
        assert!(s.recent.len() <= RECENT_LIMIT);
    }

    #[test]
    fn slash_takes_the_group_or_function_before_it() {
        for (text, operand) in [
            (r"y = \sin(x)", r"\sin(x)"),
            (r"\sin\left(x\right)", r"\sin\left(x\right)"),
            ("2 + (x + 1)", "(x + 1)"),
            ("f(x)", "f(x)"),
            (r"\sin^{2}(x)", r"\sin^{2}(x)"),
            (r"1 + \sqrt{x}", r"\sqrt{x}"),
            (r"a + \pi", r"\pi"),
            ("3 + x2", "x2"),
        ] {
            let start = operand_before(text, text.len());
            assert_eq!(&text[start..], operand, "{text}");
        }
    }

    #[test]
    fn a_display_is_set_off_by_one_blank_line_and_no_more() {
        // An empty line between two paragraphs: replaced, nothing added.
        let t = "A\n\n\n\nB";
        let (r, b, a) = display_place(t, 3);
        assert_eq!((r, b.as_str(), a.as_str()), (3..3, "$$ ", " $$"));
        // The only blank line between two paragraphs: it is replaced, and a
        // newline each side keeps a blank line round the block.
        let t = "A\n\nB";
        let (_, b, a) = display_place(t, 2);
        assert_eq!((b.as_str(), a.as_str()), ("\n$$ ", " $$\n"));
        let t = "A\n\nB";
        let (r, b, a) = display_place(t, 1);
        assert_eq!((r, b.as_str(), a.as_str()), (1..1, "\n\n$$ ", " $$"));
        // Words after the caret: the paragraph is split with one blank line each side.
        let t = "ab cd";
        let (_, b, a) = display_place(t, 2);
        assert_eq!((b.as_str(), a.as_str()), ("\n\n$$ ", " $$\n\n"));
    }

    #[test]
    fn prose_lines_become_maths_blocks() {
        let (r, b, a) = display_place("", 0);
        assert_eq!((r, b.as_str(), a.as_str()), (0..0, "$$ ", " $$"));
        let t = "Hello world";
        let (r, b, a) = display_place(t, t.len());
        assert_eq!((r, b.as_str(), a.as_str()), (11..11, "\n\n$$ ", " $$\n"));
        let (r, b, _) = display_place(t, 5);
        assert_eq!((r, b.as_str()), (5..5, "\n\n$$ "));
        let (r, _, a) = display_place(t, 0);
        assert_eq!((r, a.as_str()), (0..0, " $$\n\n"));
    }

    #[test]
    fn operands_before_a_slash() {
        assert_eq!(operand_before("1 + 23", 6), 4);
        assert_eq!(operand_before("x + α", "x + α".len()), 4);
        // A bracketed group is the numerator (it used to leave an empty one).
        assert_eq!(operand_before("(a)", 3), 0);
    }
}
