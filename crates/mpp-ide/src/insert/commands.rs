//! The command list (U22): `\` in maths and `/` at the start of a prose line open
//! a small list of what can be inserted, filtered as you type.
//!
//! In maths the entries are the Insert catalogue's templates and symbols (the same
//! ones the Ctrl+Space menu offers); in prose they are the blocks (maths line,
//! heading, list, table, comment) and the catalogue's theorem and figure items.
//! Each entry shows its picture, its LaTeX and its code form. Enter or Tab accepts,
//! Esc closes and leaves what was typed.

use egui::{Align2, Area, Context, Key, Order, Pos2, Sense, Ui, Vec2};

use super::catalogue::{catalogue, find, Body, BuildCx, Group, Template};
use super::InsertUi;
use crate::editor::Editor;
use crate::page::Page;
use crate::theme::Mode;

/// How many entries the list shows at most.
const LIMIT: usize = 8;

/// A block that can start a prose line (not in the catalogue).
struct BlockCmd {
    id: &'static str,
    name: &'static str,
    icon: &'static str,
    keywords: &'static str,
    /// What it writes, and how many bytes in the caret goes.
    text: &'static str,
    caret: usize,
    latex: &'static str,
    code: &'static str,
}

const BLOCKS: [BlockCmd; 5] = [
    BlockCmd {
        id: "block.math",
        name: "Maths line",
        icon: r"\sum",
        keywords: "math maths equation formula display",
        text: "$$  $$",
        caret: 3,
        latex: "$$ … $$",
        code: "= or $",
    },
    BlockCmd {
        id: "block.heading",
        name: "Heading",
        icon: r"\text{H}",
        keywords: "title section",
        text: "# ",
        caret: 2,
        latex: r"\section{…}",
        code: "# …",
    },
    BlockCmd {
        id: "block.list",
        name: "List",
        icon: r"\bullet\;\bullet",
        keywords: "bullet items numbered",
        text: "- ",
        caret: 2,
        latex: r"\item …",
        code: "- …",
    },
    BlockCmd {
        id: "block.table",
        name: "Table",
        icon: r"\begin{matrix}a&b\\c&d\end{matrix}",
        keywords: "grid rows columns",
        text: "| a | b |\n|---|---|\n|   |   |",
        caret: 2,
        latex: r"\begin{tabular}",
        code: "| a | b |",
    },
    BlockCmd {
        id: "block.comment",
        name: "Comment",
        icon: r"\%",
        keywords: "note remark hidden",
        text: "% ",
        caret: 2,
        latex: r"\% …",
        code: "% …",
    },
];

/// What an entry inserts.
#[derive(Debug, Clone, Copy)]
enum What {
    Template(&'static Template),
    Block(usize),
}

/// One row of the list.
#[derive(Debug, Clone)]
pub struct Entry {
    pub id: String,
    pub name: String,
    /// The picture: LaTeX to typeset.
    pub icon: String,
    pub latex: String,
    pub code: String,
    what: What,
}

/// The list's state, owned by the Insert UI.
#[derive(Debug, Clone, Default)]
pub struct CommandState {
    pub open: bool,
    /// The offset of the `\` or `/` that opened it.
    start: usize,
    /// Opened by `/` in prose.
    pub prose: bool,
    pub query: String,
    pub selected: usize,
    pub items: Vec<Entry>,
}

/// What a key did to the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyOutcome {
    /// Not for the list.
    Ignored,
    /// The list used it.
    Handled,
    /// Insert the selected entry.
    Accept,
}

/// The LaTeX of an entry, written without the box markers (`\frac{}{}`).
fn latex_of(t: &Template) -> String {
    let src = match t.body {
        Body::Symbol(_) => {
            return t
                .id
                .strip_prefix("sym.")
                .map_or(t.icon.clone(), |n| format!("\\{n}"))
        }
        Body::Block(s) => s.to_string(),
        _ => t.snippet_source(t.knobs, &BuildCx::default()),
    };
    let mut out = String::new();
    let mut skip = false;
    for c in src.chars() {
        match c {
            '⟨' => skip = true,
            '⟩' => skip = false,
            _ if !skip => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The code form of a catalogue entry: how it is typed as code. This is the one
/// place to point at the shared forms table (`editor/forms.rs`); until then it
/// holds the few forms the catalogue knows about, and an empty string for an entry
/// that has none.
pub fn code_form(t: &Template) -> String {
    let id = t.id.as_str();
    let fixed = match id {
        "frac" => "a/b",
        "sqrt" => "sqrt(x)",
        "root" => "root(x, n)",
        "pow" => "x^2",
        "abs" => "abs(x)",
        "ln" => "ln(x)",
        "logb" => "log(x, b)",
        "int.def" => "int(f, x, 0, 1)",
        "int.indef" => "int(f, x)",
        "sum" => "sum(f, n, 1, 10)",
        "prod" => "prod(f, n, 1, 10)",
        "deriv" => "diff(f, x)",
        _ => "",
    };
    if !fixed.is_empty() {
        return fixed.to_string();
    }
    if let Some(name) = id.strip_prefix("sym.") {
        return match name {
            "le" | "leq" => "<=",
            "ge" | "geq" => ">=",
            "ne" | "neq" => "!=",
            "to" | "rightarrow" => "->",
            "Rightarrow" | "implies" => "=>",
            "infty" => "inf",
            "cdot" => "*",
            other => other,
        }
        .to_string();
    }
    String::new()
}

fn entry_of(t: &'static Template) -> Entry {
    Entry {
        id: t.id.clone(),
        name: t.name.clone(),
        icon: t.icon.clone(),
        latex: latex_of(t),
        code: code_form(t),
        what: What::Template(t),
    }
}

fn entry_of_block(i: usize) -> Entry {
    let b = &BLOCKS[i];
    Entry {
        id: b.id.into(),
        name: b.name.into(),
        icon: b.icon.into(),
        latex: b.latex.into(),
        code: b.code.into(),
        what: What::Block(i),
    }
}

/// How well `query` fits: lower is better, `None` is no match.
fn rank(query: &str, name: &str, latex: &str, keywords: &str) -> Option<(u8, bool)> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Some((4, true));
    }
    let name = name.to_lowercase();
    let cmd = latex
        .trim_start_matches('\\')
        .split(|c: char| !c.is_ascii_alphabetic())
        .next()
        .unwrap_or("")
        .to_lowercase();
    let named = name.starts_with(&q);
    let r = if cmd == q {
        0
    } else if cmd.starts_with(&q) {
        1
    } else if named {
        2
    } else if name
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w.starts_with(&q))
    {
        3
    } else if keywords
        .to_lowercase()
        .split_whitespace()
        .any(|w| w.starts_with(&q))
    {
        5
    } else {
        return None;
    };
    // Among equals the entry named for the query comes first.
    Some((r, !named))
}

/// The entries for `query`: in maths the catalogue's templates and symbols, in
/// prose the blocks. At most [`LIMIT`], best first.
pub fn entries(query: &str, prose: bool) -> Vec<Entry> {
    let mut scored: Vec<((u8, bool), usize, Entry)> = vec![];
    if prose {
        for (i, b) in BLOCKS.iter().enumerate() {
            if let Some(r) = rank(query, b.name, b.latex, b.keywords) {
                scored.push((r, scored.len(), entry_of_block(i)));
            }
        }
        for t in catalogue().iter().filter(|t| {
            t.is_figure() || (t.is_text() && t.groups.iter().any(|g| matches!(g, Group::Document)))
        }) {
            if let Some(r) = rank(query, &t.name, &latex_of(t), &t.keywords) {
                scored.push((r, scored.len(), entry_of(t)));
            }
        }
    } else {
        for t in catalogue()
            .iter()
            .filter(|t| !t.is_text() && !t.is_figure() && !t.cite_picker)
        {
            let latex = latex_of(t);
            if let Some(r) = rank(query, &t.name, &latex, &t.keywords) {
                scored.push((r, scored.len(), entry_of(t)));
            }
        }
    }
    scored.sort_by_key(|(r, i, _)| (*r, *i));
    scored.into_iter().take(LIMIT).map(|(_, _, e)| e).collect()
}

fn line_before(text: &str, caret: usize) -> &str {
    let start = text[..caret].rfind('\n').map_or(0, |i| i + 1);
    &text[start..caret]
}

impl CommandState {
    /// Closes the list (the typed text stays).
    pub fn close(&mut self) {
        self.open = false;
        self.items.clear();
        self.query.clear();
        self.selected = 0;
    }

    /// Called after text was typed: opens the list on `\` in maths or `/` at the
    /// start of a prose line, and keeps it in step with what follows. `in_math` is
    /// whether the caret was in maths before the typing, `last` the last character
    /// typed.
    pub fn typed(&mut self, text: &str, caret: usize, in_math: bool, last: Option<char>) {
        match last {
            Some('\\') if in_math && caret > 0 => {
                // `\\` is a row break, not a command.
                if !text[..caret - 1].ends_with('\\') {
                    self.begin(caret - 1, false);
                }
            }
            Some('/') if !in_math && caret > 0 && line_before(text, caret).trim() == "/" => {
                self.begin(caret - 1, true);
            }
            _ => {}
        }
        self.refresh(text, caret);
    }

    fn begin(&mut self, start: usize, prose: bool) {
        self.open = true;
        self.start = start;
        self.prose = prose;
        self.selected = 0;
    }

    /// Re-reads the typed query; closes when the caret left it, a character that
    /// cannot be part of a name was typed, or nothing matches.
    pub fn refresh(&mut self, text: &str, caret: usize) {
        if !self.open {
            return;
        }
        let trigger = if self.prose { '/' } else { '\\' };
        let ok = caret > self.start
            && text
                .get(self.start..)
                .is_some_and(|s| s.starts_with(trigger))
            && text.get(self.start + 1..caret).is_some();
        if !ok {
            self.close();
            return;
        }
        let query = &text[self.start + 1..caret];
        if !query.chars().all(|c| c.is_alphanumeric()) {
            self.close();
            return;
        }
        if query != self.query || self.items.is_empty() {
            self.query = query.to_string();
            self.items = entries(query, self.prose);
            self.selected = 0;
        }
        if self.items.is_empty() {
            self.close();
        }
    }

    /// A key pressed while the list may be open.
    pub fn key(&mut self, key: Key, shift: bool) -> KeyOutcome {
        if !self.open || self.items.is_empty() {
            return KeyOutcome::Ignored;
        }
        let n = self.items.len();
        match key {
            Key::ArrowDown => {
                self.selected = (self.selected + 1) % n;
                KeyOutcome::Handled
            }
            Key::ArrowUp => {
                self.selected = (self.selected + n - 1) % n;
                KeyOutcome::Handled
            }
            Key::Enter | Key::Tab if !shift => KeyOutcome::Accept,
            Key::Escape => {
                self.close();
                KeyOutcome::Handled
            }
            _ => KeyOutcome::Ignored,
        }
    }
}

/// Inserts the selected entry in place of what was typed (`\fr`, `/head`).
/// True when something was inserted.
pub fn accept(editor: &mut Editor, page: &Page, ui: &mut InsertUi) -> bool {
    let state = &mut ui.commands;
    let Some(entry) = state.items.get(state.selected).cloned() else {
        state.close();
        return false;
    };
    let range = state.start..editor.caret();
    state.close();
    if range.start > range.end || editor.text().get(range.clone()).is_none() {
        return false;
    }
    match entry.what {
        What::Block(i) => {
            let b = &BLOCKS[i];
            let line_start = editor.text()[..range.start]
                .rfind('\n')
                .map_or(0, |p| p + 1);
            // The block replaces the typed `/…`; anything before it on the line stays.
            let head = editor.text()[line_start..range.start].to_string();
            let at = range.start;
            editor.replace_node(mpp_syntax::Span::new(at, range.end), b.text);
            editor.move_caret(at + b.caret.min(b.text.len()), false);
            let _ = head;
        }
        What::Template(t) => {
            editor.delete_range(range);
            let caret = editor.caret();
            let cx = ui.settings.build_cx(editor.text(), caret);
            super::insert(editor, page, t, t.knobs, &cx);
            ui.settings.touch(&t.id);
        }
    }
    true
}

/// Draws the list under `anchor` (screen). Returns the row clicked, if any.
pub fn popup(ctx: &Context, state: &CommandState, anchor: Pos2, mode: Mode) -> Option<usize> {
    if !state.open || state.items.is_empty() {
        return None;
    }
    let theme = mode.theme();
    let palette = mode.palette();
    let mut pick = None;
    Area::new(egui::Id::unique("mpp-commands"))
        .order(Order::Foreground)
        .pivot(Align2::LEFT_TOP)
        .fixed_pos(anchor)
        .show(ctx, |ui: &mut Ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(330.0);
                for (i, e) in state.items.iter().enumerate() {
                    let backdrop = ui.painter().add(egui::Shape::Noop);
                    let row = ui.horizontal(|ui| {
                        ui.allocate_ui(Vec2::new(54.0, 22.0), |ui| {
                            crate::typeset::show(ui, &e.icon, 18.0, theme);
                        });
                        ui.vertical(|ui| {
                            ui.label(&e.name);
                            let mut forms = e.latex.clone();
                            if !e.code.is_empty() {
                                forms.push_str("   ·   ");
                                forms.push_str(&e.code);
                            }
                            ui.add(
                                egui::Label::new(egui::RichText::new(forms).monospace().weak())
                                    .extend(),
                            );
                        });
                    });
                    let r = row.response.rect.expand2(Vec2::new(4.0, 1.0));
                    if i == state.selected {
                        ui.painter().set(
                            backdrop,
                            egui::Shape::rect_filled(
                                r,
                                3.0,
                                crate::paint::c32(palette.row_selected),
                            ),
                        );
                    }
                    if ui
                        .interact(r, ui.scope_id().with(("mpp-command", i)), Sense::click())
                        .clicked()
                    {
                        pick = Some(i);
                    }
                }
            });
        });
    pick
}

/// The catalogue entry with this id (for tests and callers that hold an id).
pub fn entry_for(id: &str) -> Option<Entry> {
    find(id).map(entry_of)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(q: &str, prose: bool) -> Vec<String> {
        entries(q, prose).into_iter().map(|e| e.id).collect()
    }

    #[test]
    fn fr_finds_the_fraction_first() {
        let got = names("fr", false);
        assert_eq!(got.first().map(String::as_str), Some("frac"), "{got:?}");
    }

    #[test]
    fn al_finds_alpha() {
        let got = names("al", false);
        assert!(got.iter().any(|i| i == "sym.alpha"), "{got:?}");
        assert_eq!(
            got.first().map(String::as_str),
            Some("sym.alpha"),
            "{got:?}"
        );
    }

    #[test]
    fn an_empty_query_lists_something() {
        assert_eq!(entries("", false).len(), LIMIT);
        assert_eq!(entries("", true).len(), LIMIT.min(entries("", true).len()));
    }

    #[test]
    fn prose_offers_blocks() {
        let got = names("", true);
        for want in [
            "block.math",
            "block.heading",
            "block.list",
            "block.table",
            "block.comment",
        ] {
            assert!(got.iter().any(|i| i == want), "{want} in {got:?}");
        }
        let got = names("theo", true);
        assert!(got.iter().any(|i| i == "doc.theorem"), "{got:?}");
        let got = names("graph", true);
        assert!(got.iter().any(|i| i.starts_with("graph.")), "{got:?}");
    }

    #[test]
    fn entries_show_latex_and_code() {
        let f = entry_for("frac").expect("frac");
        assert_eq!(f.latex, r"\frac{}{}");
        assert_eq!(f.code, "a/b");
        let a = entry_for("sym.alpha").expect("alpha");
        assert_eq!(a.latex, r"\alpha");
        assert_eq!(a.code, "alpha");
    }

    #[test]
    fn the_list_follows_the_typing_and_closes_on_a_space() {
        let mut s = CommandState::default();
        s.typed("x + \\", 5, true, Some('\\'));
        assert!(s.open);
        s.typed("x + \\fr", 7, true, Some('r'));
        assert!(s.open);
        assert_eq!(s.query, "fr");
        assert_eq!(s.items[0].id, "frac");
        s.typed("x + \\fr ", 8, true, Some(' '));
        assert!(!s.open);
    }

    #[test]
    fn a_double_backslash_is_a_row_break() {
        let mut s = CommandState::default();
        s.typed("a \\\\", 4, true, Some('\\'));
        assert!(!s.open);
    }

    #[test]
    fn a_slash_opens_only_at_the_start_of_a_prose_line() {
        let mut s = CommandState::default();
        s.typed("hello /", 7, false, Some('/'));
        assert!(!s.open);
        s.typed("hello\n/", 7, false, Some('/'));
        assert!(s.open && s.prose);
        let mut s = CommandState::default();
        s.typed("1/", 2, true, Some('/'));
        assert!(!s.open);
    }

    #[test]
    fn keys_move_accept_and_close() {
        let mut s = CommandState::default();
        s.typed("\\", 1, true, Some('\\'));
        assert_eq!(s.key(Key::ArrowDown, false), KeyOutcome::Handled);
        assert_eq!(s.selected, 1);
        assert_eq!(s.key(Key::Enter, false), KeyOutcome::Accept);
        assert_eq!(s.key(Key::Tab, false), KeyOutcome::Accept);
        assert_eq!(s.key(Key::Escape, false), KeyOutcome::Handled);
        assert!(!s.open);
        assert_eq!(s.key(Key::Enter, false), KeyOutcome::Ignored);
    }

    #[test]
    fn accepting_a_symbol_replaces_the_typed_text() {
        let mut ui = InsertUi::default();
        let mut e = Editor::new("$$ x = \\al $$");
        e.move_caret(10, false);
        ui.commands.typed(e.text(), 10, true, Some('\\'));
        // Opened on a stale offset: open it at the backslash properly.
        ui.commands.begin(7, false);
        ui.commands.refresh(e.text(), 10);
        assert_eq!(ui.commands.items[0].id, "sym.alpha");
        let page = Page {
            math_ranges: vec![mpp_syntax::Span::new(3, 11)],
            ..Page::default()
        };
        assert!(accept(&mut e, &page, &mut ui));
        assert_eq!(e.text(), "$$ x = α $$");
        assert!(!ui.commands.open);
    }

    #[test]
    fn accepting_a_block_replaces_the_slash() {
        let mut ui = InsertUi::default();
        let mut e = Editor::new("Hello\n\n/head");
        e.move_caret(12, false);
        ui.commands.begin(7, true);
        ui.commands.refresh(e.text(), 12);
        assert_eq!(ui.commands.items[0].id, "block.heading");
        let page = Page::default();
        assert!(accept(&mut e, &page, &mut ui));
        assert_eq!(e.text(), "Hello\n\n# ");
        assert_eq!(e.caret(), 9);
    }
}
