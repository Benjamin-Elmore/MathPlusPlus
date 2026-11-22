//! The cheat sheet (F1, Help -> Cheat sheet): a side view that lists what to
//! type and what it becomes. The maths rows come from the input forms table
//! (`editor::forms::table()`), so the sheet cannot drift from what the editor
//! converts; the key, definition and page rows are fixed text below. It is
//! searchable, and a click inserts a row at the caret.

use egui::{Context, FontId, Id, RichText, Sense, Ui, Vec2};
use mpp_render::Theme;

use crate::app::IdeApp;
use crate::doc::Doc;
use crate::editor::forms::{self, Form};
use crate::paint::{c32, paint_display_list};
use crate::typeset;

/// The search box's id (F1 puts the caret in it).
fn filter_id() -> Id {
    Id::NULL.with("cheat-sheet-filter")
}

/// A row of the fixed sections: what to type (or press), what happens.
pub struct Tip {
    pub typed: &'static str,
    pub becomes: &'static str,
    /// LaTeX for a picture, when the result is something to look at.
    pub picture: Option<&'static str>,
    /// Text a click inserts, when it makes sense to insert it.
    pub insert: Option<&'static str>,
}

const fn tip(
    typed: &'static str,
    becomes: &'static str,
    picture: Option<&'static str>,
    insert: Option<&'static str>,
) -> Tip {
    Tip {
        typed,
        becomes,
        picture,
        insert,
    }
}

/// A titled group of fixed rows.
pub struct Section {
    pub title: &'static str,
    pub tips: &'static [Tip],
}

/// The fixed sections, after the table of forms.
pub const SECTIONS: &[Section] = &[
    Section {
        title: "Getting out of a box",
        tips: &[
            tip("Tab", "Next box, then out of the maths", None, None),
            tip("Right (end of a box)", "One level out", None, None),
            tip("Left (start of a box)", "One level back", None, None),
            tip(
                "Space or an operator after a script",
                "Out of the exponent or subscript (x^2 then +)",
                None,
                None,
            ),
            tip(
                "Esc",
                "Closes a popup, then leaves the box, then leaves maths",
                None,
                None,
            ),
            tip("Enter", "A new line", None, None),
        ],
    },
    Section {
        title: "Definitions and answers",
        tips: &[
            tip(
                ":=",
                "Defines a name: x := 5",
                Some(r"x := 5"),
                Some("x := 5"),
            ),
            tip(
                "= at the end of a line",
                "Shows the answer",
                Some(r"1 + 2 ="),
                Some("1 + 2 ="),
            ),
            tip(
                "= ?",
                "Asks for the exact answer",
                Some(r"\frac{1}{3} + \frac{1}{6} = ?"),
                Some(r"\frac{1}{3} + \frac{1}{6} = ?"),
            ),
            tip(
                "\u{2248} ?",
                "Asks for a decimal answer",
                Some(r"\pi \approx ?"),
                Some("\\pi \u{2248} ?"),
            ),
            tip(
                "x = 5",
                "Defines x if it has no value yet; otherwise checks it",
                Some(r"x = 5"),
                Some("x = 5"),
            ),
            tip(
                "==",
                "Checks that two sides are equal (yes or no)",
                Some(r"x \overset{?}{=} 5"),
                Some("x == 5"),
            ),
        ],
    },
    Section {
        title: "Headings, lists and maths lines",
        tips: &[
            tip(
                "# then a letter",
                "A heading (## and ### for smaller)",
                None,
                None,
            ),
            tip(
                "Ctrl+1, Ctrl+2, Ctrl+3",
                "Heading level; Ctrl+0 is body text",
                None,
                None,
            ),
            tip("- or 1. then a space", "A list item", None, None),
            tip("$", "Starts a maths line", None, None),
            tip(
                "= or := on an empty line",
                "Starts a maths line",
                None,
                None,
            ),
            tip("Alt+=", "Starts a maths line", None, None),
            tip("Ctrl+M", "Starts a maths line", None, None),
            tip(
                "/ (in maths)",
                "Opens a fraction",
                Some(r"\frac{a}{b}"),
                None,
            ),
        ],
    },
];

/// True when a row of the forms table is found by the search words.
pub fn form_matches(f: &Form, needle: &str) -> bool {
    let n = needle.trim().to_lowercase();
    n.is_empty()
        || [f.name, f.latex, f.code, f.unicode, f.unicodemath, f.id]
            .iter()
            .chain(f.also.iter())
            .any(|s| s.to_lowercase().contains(&n))
}

fn tip_matches(t: &Tip, needle: &str) -> bool {
    let n = needle.trim().to_lowercase();
    n.is_empty() || t.typed.to_lowercase().contains(&n) || t.becomes.to_lowercase().contains(&n)
}

/// Puts `text` at the caret: bare inside maths, as a new maths line in prose.
pub fn insert_at_caret(doc: &mut Doc, text: &str) {
    let caret = doc.editor.caret();
    let in_math = doc.page_cache().is_some_and(|pc| pc.page.in_math(caret));
    let text = if in_math {
        text.to_string()
    } else {
        format!("$$ {text} $$\n\n")
    };
    doc.editor.move_caret(caret, false);
    doc.editor.insert_text(&text);
    doc.touched();
}

/// One cell: what to type in monospace, with the picture of the result under it.
fn cell(
    ui: &mut Ui,
    theme: &Theme,
    size: f32,
    label: String,
    typed: &str,
    picture: Option<&str>,
) -> bool {
    let width = ui.available_width();
    let galley = ui.painter().layout(
        typed.to_string(),
        FontId::monospace(13.0),
        c32(theme.text),
        width - 8.0,
    );
    let dl = picture.map(|p| typeset::layout(p, size, *theme));
    let pic_h = dl.as_ref().map_or(0.0, |d| d.total_height().max(size));
    let h = galley.size().y + pic_h + 10.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, h), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label.clone()));
    let p = ui.painter_at(rect);
    if resp.hovered() {
        p.rect_filled(rect, 3.0, c32(theme.link).gamma_multiply(0.14));
    }
    let below = galley.size().y + 6.0;
    p.galley(rect.min + Vec2::new(4.0, 3.0), galley, c32(theme.text));
    if let Some(dl) = dl {
        paint_display_list(&p, &dl, rect.min + Vec2::new(4.0, below));
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

impl IdeApp {
    /// F1: shows the cheat sheet with the search box ready.
    pub fn show_cheat_sheet(&mut self, ctx: &Context) {
        self.show_side(crate::settings::SideView::Help);
        ctx.memory_mut(|m| m.request_focus(filter_id()));
    }

    /// The cheat sheet as a side view.
    pub(crate) fn cheat_sheet_tile(&mut self, ui: &mut Ui, theme: &Theme) {
        let text_id = filter_id().with("text");
        let mut filter: String = ui.data(|d| d.get_temp(text_id)).unwrap_or_default();
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            ui.add(
                egui::TextEdit::singleline(&mut filter)
                    .id(filter_id())
                    .hint_text("Search the cheat sheet")
                    .desired_width(ui.available_width() - 14.0),
            )
            .accessible_name("Search the cheat sheet");
        });
        ui.data_mut(|d| d.insert_temp(text_id, filter.clone()));
        let needle = filter;
        let size = (self.size * 0.8).clamp(12.0, 20.0);
        let mut click: Option<&'static str> = None;
        egui::ScrollArea::vertical()
            .id_salt("cheat-sheet")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(4.0);
                ui.columns(2, |cols| {
                    for (col, title) in cols
                        .iter_mut()
                        .zip(["If you know LaTeX", "If you write code"])
                    {
                        col.label(RichText::new(title).strong().color(c32(theme.text)));
                    }
                });
                ui.add_space(2.0);
                let mut shown = 0;
                for f in forms::table().iter().filter(|f| form_matches(f, &needle)) {
                    shown += 1;
                    ui.label(RichText::new(f.name).size(12.5).color(c32(theme.muted)));
                    ui.columns(2, |cols| {
                        let latex = format!("{}: type {} (LaTeX)", f.name, f.latex);
                        if cell(&mut cols[0], theme, size, latex, f.latex, Some(f.picture)) {
                            click = Some(f.writes);
                        }
                        let code = format!("{}: type {} (code)", f.name, f.code);
                        if cell(&mut cols[1], theme, size, code, f.code, Some(f.picture)) {
                            click = Some(f.writes);
                        }
                    });
                }
                for s in SECTIONS {
                    let tips: Vec<&Tip> =
                        s.tips.iter().filter(|t| tip_matches(t, &needle)).collect();
                    if tips.is_empty() {
                        continue;
                    }
                    shown += tips.len();
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(s.title.to_uppercase())
                            .size(12.5)
                            .color(c32(theme.muted)),
                    );
                    for t in tips {
                        ui.label(RichText::new(t.becomes).size(12.5).color(c32(theme.muted)));
                        let label = format!("{}: {}", t.becomes, t.typed);
                        if cell(ui, theme, size, label, t.typed, t.picture) {
                            if let Some(text) = t.insert {
                                click = Some(text);
                            }
                        }
                    }
                }
                if shown == 0 {
                    ui.add_space(8.0);
                    ui.label("Nothing matches. Try a word like fraction, root or sum.");
                }
            });
        if let Some(text) = click {
            if let Some(doc) = self.doc_mut() {
                insert_at_caret(doc, text);
            }
            ui.ctx().request_repaint();
        }
    }
}
