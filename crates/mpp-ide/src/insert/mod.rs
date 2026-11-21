//! The Insert menu: templates with empty boxes, put in at the caret.
//!
//! Choosing an item writes its `.mpp` text behind the scenes (`\int_{0}^{1} …`)
//! and starts a template session in the editor, so Tab walks the boxes. In a
//! prose line the template becomes a maths block of its own; symbols in prose
//! become inline maths; graphs become a `Figure:` line.

pub mod catalogue;
pub mod menu;

use std::ops::Range;

use crate::editor::{likely_variable, parse_snippet, Editor, Knobs, Snippet};
use crate::page::Page;
pub use catalogue::{catalogue, controls, find, Action, BuildCx, Control, Group, Template};

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

/// Where a display template goes when the caret is in prose: the range to
/// replace and the text around the template.
fn display_place(text: &str, caret: usize) -> (Range<usize>, String, String) {
    let line = line_at(text, caret);
    let content = &text[line.clone()];
    if content.trim().is_empty() {
        return (line, "$$ ".into(), " $$".into());
    }
    let before = &text[line.start..caret];
    let after = &text[caret..line.end];
    if after.trim().is_empty() {
        let tail = if line.end < text.len() { "" } else { "\n" };
        return (caret..line.end, "\n\n$$ ".into(), format!(" $${tail}"));
    }
    if before.trim().is_empty() {
        return (line.start..line.start, "$$ ".into(), " $$\n\n".into());
    }
    (caret..caret, "\n\n$$ ".into(), " $$\n\n".into())
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
fn insert_block(editor: &mut Editor, page: &Page, id: &str, body: &str, caret: usize) {
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
        let operand_start = operand_before(text, slash);
        let numerator = text[operand_start..slash].to_string();
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

/// The "fill in: …" hint for a maths block (`span`), if it has an empty box
/// that must be filled before it can be worked out.
pub fn fill_in_hint(editor: &Editor, page: &Page, span: &Range<usize>) -> Option<String> {
    let inside = |o: usize| span.start <= o && o <= span.end;
    if let Some((_, label)) = editor.missing_boxes().into_iter().find(|(o, _)| inside(*o)) {
        return Some(format!("fill in: {}", long_label(&label)));
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
        "fill in: {}",
        long_label(first.label.as_deref().unwrap_or(""))
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
