//! The Insert dropdown (grouped, searchable, recently used first), the
//! rows × columns picker for matrices, and the little toolbar a template shows
//! while the caret is in it. Everything maths is typeset; nothing shows source.

use egui::{
    Area, Context, Frame, Id, Key, Modifiers, Order, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2,
};
use mpp_render::Theme;

use super::catalogue::{catalogue, find, Control, Group, Template};
use crate::editor::Knobs;
use crate::paint::{c32, paint_display_list, paint_text};
use crate::theme::{self, Mode};
use crate::typeset;

/// Text size of the menu's names.
const NAME_SIZE: f32 = 16.0;
/// Size of the typeset pictures.
const ICON_SIZE: f32 = 15.0;
const ICON_WIDTH: f32 = 78.0;
const MENU_WIDTH: f32 = 330.0;
const MENU_HEIGHT: f32 = 400.0;
const PICKER_CELLS: u8 = 8;

/// The matrix size picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picker {
    pub template: String,
    pub rows: u8,
    pub cols: u8,
}

/// The menu's state.
#[derive(Debug, Clone, Default)]
pub struct MenuState {
    pub open: bool,
    /// Where to open (screen position); `None` opens at the caret.
    pub at: Option<Pos2>,
    pub query: String,
    pub selected: usize,
    pub picker: Option<Picker>,
    /// The citation picker, open with this entry selected.
    pub cite: Option<usize>,
    /// The entries the citation picker offers (the document's references), set by
    /// the view before the menu is shown.
    pub citations: Vec<crate::doc_settings::CiteChoice>,
    just_opened: bool,
}

/// What the menu decided this frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Nothing,
    Insert {
        id: String,
        knobs: Knobs,
    },
    /// A citation was chosen: insert `[@key]`.
    Cite {
        key: String,
    },
    Closed,
}

impl MenuState {
    /// Opens the menu (at a screen position, or at the caret).
    pub fn open_at(&mut self, at: Option<Pos2>) {
        self.open = true;
        self.at = at;
        self.query.clear();
        self.selected = 0;
        self.picker = None;
        self.cite = None;
        self.just_opened = true;
    }

    /// Closes it.
    pub fn close(&mut self) {
        self.open = false;
        self.picker = None;
        self.cite = None;
    }

    /// Chooses an item: a matrix opens the size picker first.
    pub fn choose(&mut self, t: &Template) -> Outcome {
        if t.cite_picker {
            self.cite = Some(0);
            return Outcome::Nothing;
        }
        if t.grid_picker && self.picker.is_none() {
            self.picker = Some(Picker {
                template: t.id.clone(),
                rows: t.knobs.rows.max(1),
                cols: t.knobs.cols.max(1),
            });
            return Outcome::Nothing;
        }
        self.close();
        Outcome::Insert {
            id: t.id.clone(),
            knobs: t.knobs,
        }
    }
}

/// A heading and its items.
pub type Section = (String, Vec<&'static Template>);

/// The menu's sections for a search: recently used first, then the groups.
/// A search lists every match in one section, best (name starts with it) first.
pub fn sections(query: &str, recent: &[String]) -> Vec<Section> {
    let q = query.trim().to_lowercase();
    if !q.is_empty() {
        let mut hits: Vec<&'static Template> =
            catalogue().iter().filter(|t| t.matches(&q)).collect();
        hits.sort_by_key(|t| {
            let name = t.name.to_lowercase();
            let word_start = name
                .split(|c: char| !c.is_alphanumeric())
                .any(|w| w.starts_with(&q));
            if name.starts_with(&q) || t.id == q {
                0
            } else if word_start {
                1
            } else if name.contains(&q) {
                2
            } else {
                3
            }
        });
        return vec![("Results".into(), hits)];
    }
    let mut out: Vec<Section> = vec![];
    let recent: Vec<&'static Template> = recent.iter().filter_map(|id| find(id)).collect();
    if !recent.is_empty() {
        out.push(("Recently used".into(), recent));
    }
    for g in Group::ALL {
        let items: Vec<&'static Template> = catalogue()
            .iter()
            .filter(|t| t.groups.first() == Some(&g) || (t.groups.contains(&g) && g == Group::Sums))
            .collect();
        out.push((g.title().into(), items));
    }
    out
}

/// Every item in the order the arrow keys walk them.
pub fn flat(sections: &[Section]) -> Vec<&'static Template> {
    sections
        .iter()
        .flat_map(|(_, v)| v.iter().copied())
        .collect()
}

fn small(s: &str) -> RichText {
    RichText::new(s).font(theme::sans(13.0))
}

/// Draws the menu at `anchor` (screen) and handles its keys.
pub fn show(
    ctx: &Context,
    state: &mut MenuState,
    recent: &[String],
    anchor: Pos2,
    mode: Mode,
) -> Outcome {
    if !state.open {
        return Outcome::Nothing;
    }
    let theme = mode.theme();
    let palette = mode.palette();
    let secs = sections(&state.query, recent);
    let items = flat(&secs);
    let mut outcome = Outcome::Nothing;

    // Keys first, so the search box does not see them.
    let (up, down, enter, escape) = ctx.input_mut(|i| {
        (
            i.consume_key(Modifiers::NONE, Key::ArrowUp),
            i.consume_key(Modifiers::NONE, Key::ArrowDown),
            i.consume_key(Modifiers::NONE, Key::Enter),
            i.consume_key(Modifiers::NONE, Key::Escape),
        )
    });
    if state.cite.is_some() {
        if escape {
            state.cite = None;
        } else if !state.citations.is_empty() {
            let n = state.citations.len();
            let sel = state.cite.unwrap_or(0).min(n - 1);
            let sel = if up {
                (sel + n - 1) % n
            } else if down {
                (sel + 1) % n
            } else {
                sel
            };
            state.cite = Some(sel);
            if enter {
                let key = state.citations[sel].key.clone();
                state.close();
                return Outcome::Cite { key };
            }
        }
    }
    let (left, right) = if state.picker.is_some() {
        ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowLeft),
                i.consume_key(Modifiers::NONE, Key::ArrowRight),
            )
        })
    } else {
        (false, false)
    };
    if state.cite.is_some() {
        // Handled above.
    } else if let Some(p) = state.picker.as_mut() {
        if escape {
            state.picker = None;
        } else {
            if up {
                p.rows = p.rows.saturating_sub(1).max(1);
            }
            if down {
                p.rows = (p.rows + 1).min(12);
            }
            if left {
                p.cols = p.cols.saturating_sub(1).max(1);
            }
            if right {
                p.cols = (p.cols + 1).min(12);
            }
            if enter {
                let (id, rows, cols) = (p.template.clone(), p.rows, p.cols);
                state.close();
                let knobs = find(&id).map_or(Knobs::default(), |t| t.knobs);
                return Outcome::Insert {
                    id,
                    knobs: Knobs {
                        rows,
                        cols,
                        ..knobs
                    },
                };
            }
        }
    } else {
        if escape {
            state.close();
            return Outcome::Closed;
        }
        let n = items.len();
        if n > 0 {
            if up {
                state.selected = (state.selected + n - 1) % n;
            }
            if down {
                state.selected = (state.selected + 1) % n;
            }
            state.selected = state.selected.min(n - 1);
            if enter {
                if let Some(t) = items.get(state.selected) {
                    outcome = state.choose(t);
                    if matches!(outcome, Outcome::Insert { .. }) {
                        return outcome;
                    }
                }
            }
        }
    }

    let area = Area::new(Id::unique("mpp-insert-menu"))
        .order(Order::Foreground)
        .fixed_pos(anchor)
        .constrain(true)
        .show(ctx, |ui| {
            Frame::popup(ui.style())
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.set_width(MENU_WIDTH);
                    if state.cite.is_some() {
                        if let Some(o) = cite_ui(ui, state, theme, mode) {
                            outcome = o;
                        }
                        return;
                    }
                    if state.picker.is_some() {
                        if let Some(o) = picker_ui(ui, state, theme, mode) {
                            outcome = o;
                        }
                        return;
                    }
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut state.query)
                            .hint_text(small("Search maths\u{2026}"))
                            .font(theme::sans(15.0))
                            .desired_width(f32::INFINITY),
                    );
                    if state.just_opened || !edit.has_focus() {
                        edit.request_focus();
                    }
                    if edit.changed() {
                        state.selected = 0;
                    }
                    ui.add_space(4.0);
                    let mut chosen: Option<&'static Template> = None;
                    // The area remembers its last size: when the list grows
                    // (a new search, the menu opened again) it must not stay
                    // as short as it was.
                    egui::ScrollArea::vertical()
                        .max_height(MENU_HEIGHT)
                        .min_scrolled_height(MENU_HEIGHT)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            let mut index = 0usize;
                            if items.is_empty() {
                                ui.label(small("Nothing matches."));
                            }
                            for (title, list) in &secs {
                                if list.is_empty() {
                                    continue;
                                }
                                ui.add_space(4.0);
                                ui.label(small(title).strong().color(c32(palette.theme.muted)));
                                let tiles = state.query.trim().is_empty()
                                    && list.iter().all(|t| t.is_symbol());
                                if tiles {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing = Vec2::new(2.0, 2.0);
                                        for t in list {
                                            let sel = index == state.selected;
                                            if tile(ui, t, sel, theme, mode).clicked() {
                                                chosen = Some(t);
                                            }
                                            index += 1;
                                        }
                                    });
                                } else {
                                    for t in list {
                                        let sel = index == state.selected;
                                        if row(ui, t, sel, theme, mode).clicked() {
                                            chosen = Some(t);
                                        }
                                        index += 1;
                                    }
                                }
                            }
                        });
                    if let Some(t) = chosen {
                        outcome = state.choose(t);
                    }
                });
        });
    state.just_opened = false;
    // A click outside closes the menu.
    let rect = area.response.rect;
    let clicked_outside = ctx.input(|i| {
        i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p))
    });
    if clicked_outside && outcome == Outcome::Nothing {
        state.close();
        return Outcome::Closed;
    }
    outcome
}

/// One list row: the typeset picture, then the name.
fn row(ui: &mut Ui, t: &Template, selected: bool, theme: Theme, mode: Mode) -> egui::Response {
    let dl = typeset::layout(&t.icon, ICON_SIZE, theme);
    let h = (dl.total_height() + 8.0).max(28.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::click());
    if selected || resp.hovered() {
        let fill = if selected {
            c32(mode.palette().row_selected)
        } else {
            c32(mode.palette().widget_hover)
        };
        ui.painter().rect_filled(rect, 4.0, fill);
    }
    if selected {
        resp.scroll_to_me(None);
    }
    let scale = if dl.width > ICON_WIDTH - 8.0 {
        (ICON_WIDTH - 8.0) / dl.width
    } else {
        1.0
    };
    if scale < 1.0 {
        let small = typeset::layout(&t.icon, ICON_SIZE * scale, theme);
        let at = Pos2::new(
            rect.left() + 6.0,
            rect.center().y - small.total_height() / 2.0,
        );
        paint_display_list(ui.painter(), &small, at);
    } else {
        let at = Pos2::new(
            rect.left() + 6.0 + (ICON_WIDTH - 8.0 - dl.width) / 2.0,
            rect.center().y - dl.total_height() / 2.0,
        );
        paint_display_list(ui.painter(), &dl, at);
    }
    ui.painter().text(
        Pos2::new(rect.left() + ICON_WIDTH + 6.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        &t.name,
        egui::FontId::proportional(NAME_SIZE),
        c32(theme.text),
    );
    resp
}

/// A square tile with one symbol (Greek letters and symbols).
fn tile(ui: &mut Ui, t: &Template, selected: bool, theme: Theme, mode: Mode) -> egui::Response {
    let size = Vec2::splat(30.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let fill = if selected {
        Some(mode.palette().row_selected)
    } else if resp.hovered() {
        Some(mode.palette().widget_hover)
    } else {
        None
    };
    if let Some(f) = fill {
        ui.painter().rect_filled(rect, 4.0, c32(f));
    }
    if selected {
        resp.scroll_to_me(None);
    }
    // The glyph alone in Latin Modern Math (a relation on its own would be laid
    // out with empty boxes either side).
    let size = 19.0;
    let w = crate::page::text_width(mpp_render::FontId::Math, &t.icon) * size;
    let at = Pos2::new(rect.center().x - w / 2.0, rect.center().y + size * 0.3);
    paint_text(
        ui.painter(),
        &t.icon,
        at,
        size,
        mpp_render::FontId::Math,
        c32(theme.text),
    );
    resp.on_hover_text(small(&t.name))
}

/// The citation picker: the document's references, typeset, one per row. Returns an
/// outcome when one was chosen.
fn cite_ui(ui: &mut Ui, state: &mut MenuState, theme: Theme, mode: Mode) -> Option<Outcome> {
    ui.label(RichText::new("Citation").size(NAME_SIZE).strong());
    if state.citations.is_empty() {
        ui.label(small(
            "This document has no references yet. Insert a Reference list first, then cite its entries.",
        ));
        return None;
    }
    ui.label(small(
        "Choose the source to cite (or use the arrow keys and Enter).",
    ));
    ui.add_space(6.0);
    let palette = mode.palette();
    let selected = state.cite.unwrap_or(0);
    let mut chosen = None;
    egui::ScrollArea::vertical()
        .max_height(MENU_HEIGHT - 60.0)
        .show(ui, |ui| {
            for (i, c) in state.citations.iter().enumerate() {
                let mut frame = Frame::NONE;
                if i == selected {
                    frame = frame.fill(c32(palette.selection));
                }
                let resp = frame
                    .inner_margin(egui::Margin::symmetric(6, 3))
                    .show(ui, |ui| {
                        ui.set_width(MENU_WIDTH - 24.0);
                        typeset::show(ui, &c.latex, 15.0, theme);
                    })
                    .response
                    .interact(Sense::click());
                if resp.hovered() {
                    state.cite = Some(i);
                }
                if resp.clicked() {
                    chosen = Some(c.key.clone());
                }
            }
        });
    let key = chosen?;
    state.close();
    Some(Outcome::Cite { key })
}

/// The rows × columns picker. Returns an outcome when something was chosen.
fn picker_ui(ui: &mut Ui, state: &mut MenuState, theme: Theme, mode: Mode) -> Option<Outcome> {
    let p = state.picker.clone()?;
    let name = find(&p.template).map_or("Matrix", |t| t.name.as_str());
    ui.label(RichText::new(name).size(NAME_SIZE).strong());
    ui.label(small(
        "Choose the size: hover, then click (or use the arrow keys and Enter).",
    ));
    ui.add_space(6.0);
    let cell = 22.0;
    let gap = 3.0;
    let n = PICKER_CELLS.max(p.rows).max(p.cols);
    let side = n as f32 * (cell + gap);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(side, side), Sense::click());
    let palette = mode.palette();
    let mut rows = p.rows;
    let mut cols = p.cols;
    if let Some(pos) = resp.hover_pos() {
        let c = (((pos.x - rect.left()) / (cell + gap)).floor() as i32 + 1).clamp(1, n as i32);
        let r = (((pos.y - rect.top()) / (cell + gap)).floor() as i32 + 1).clamp(1, n as i32);
        rows = r as u8;
        cols = c as u8;
    }
    for r in 0..n {
        for c in 0..n {
            let min = rect.min + Vec2::new(c as f32 * (cell + gap), r as f32 * (cell + gap));
            let cr = Rect::from_min_size(min, Vec2::splat(cell));
            let on = r < rows && c < cols;
            let fill = if on {
                c32(palette.selection)
            } else {
                c32(palette.widget)
            };
            ui.painter().rect_filled(cr, 3.0, fill);
            ui.painter().rect_stroke(
                cr,
                3.0,
                Stroke::new(1.0, c32(palette.border)),
                egui::StrokeKind::Inside,
            );
        }
    }
    ui.add_space(6.0);
    typeset::show(ui, &format!("{rows} \\times {cols}"), 18.0, theme);
    if let Some(st) = state.picker.as_mut() {
        st.rows = rows;
        st.cols = cols;
    }
    if resp.clicked() {
        let id = p.template.clone();
        state.close();
        let knobs = find(&id).map_or(Knobs::default(), |t| t.knobs);
        return Some(Outcome::Insert {
            id,
            knobs: Knobs {
                rows,
                cols,
                ..knobs
            },
        });
    }
    None
}

/// The toolbar's "Insert ▾" button (the arrow is drawn: Latin Modern has no ▾).
pub fn insert_button(ui: &mut Ui) -> egui::Response {
    let resp = ui
        .button("Insert    ")
        .on_hover_text(small("Insert maths at the cursor (Ctrl+Space)"));
    let r = resp.rect;
    let c = Pos2::new(r.right() - 12.0, r.center().y + 1.0);
    let color = ui.visuals().widgets.style(&resp).fg_stroke.color;
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(c.x - 4.0, c.y - 2.5),
            Pos2::new(c.x + 4.0, c.y - 2.5),
            Pos2::new(c.x, c.y + 2.5),
        ],
        color,
        Stroke::NONE,
    ));
    resp
}

/// Where the template toolbar goes: to the left of the template `beside` when a
/// centred equation leaves room there, else to its right (a figure line starts at
/// the left margin), so it never covers the box being typed in. `width` is the
/// toolbar's width and `bounds` the visible page.
pub fn toolbar_anchor(beside: Rect, width: f32, bounds: Rect) -> (Pos2, egui::Align2) {
    const GAP: f32 = 18.0;
    if beside.left() - GAP - width >= bounds.left() {
        (
            Pos2::new(beside.left() - GAP, beside.top()),
            egui::Align2::RIGHT_TOP,
        )
    } else if beside.right() + GAP + width <= bounds.right() {
        (
            Pos2::new(beside.right() + GAP, beside.top()),
            egui::Align2::LEFT_TOP,
        )
    } else {
        // No room at either side: under the template.
        (
            Pos2::new(beside.left(), beside.bottom() + 6.0),
            egui::Align2::LEFT_TOP,
        )
    }
}

/// The template toolbar, a small column beside the template `beside` (screen),
/// where nothing is covered (see [`toolbar_anchor`]). Returns the index of a
/// clicked control.
pub fn toolbar(ctx: &Context, controls: &[Control], beside: Rect, bounds: Rect) -> Option<usize> {
    if controls.is_empty() {
        return None;
    }
    let mut clicked = None;
    let ink = ctx.global_style().visuals.text_color();
    let widest = ctx.fonts_mut(|f| {
        controls
            .iter()
            .map(|c| {
                f.layout_no_wrap(c.label.clone(), theme::sans(13.0), ink)
                    .size()
                    .x
            })
            .fold(0.0, f32::max)
    });
    // Label, button padding and the frame's margins.
    let width = widest + 40.0;
    let (at, pivot) = toolbar_anchor(beside, width, bounds);
    // Placed by its top-left corner: an area's own pivot needs the size of the
    // frame before, and a first frame drawn over the template is what this avoids.
    let at = if pivot == egui::Align2::RIGHT_TOP {
        Pos2::new(at.x - width, at.y)
    } else {
        at
    };
    Area::new(Id::unique("mpp-template-toolbar"))
        .order(Order::Foreground)
        .fixed_pos(at)
        .constrain(true)
        .show(ctx, |ui| {
            Frame::popup(ui.style())
                .inner_margin(egui::Margin::symmetric(4, 4))
                .show(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        for (i, c) in controls.iter().enumerate() {
                            let r = ui
                                .selectable_label(c.active, small(&c.label))
                                .on_hover_text(small(&c.tip));
                            if r.clicked() {
                                clicked = Some(i);
                            }
                        }
                    });
                });
        });
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_toolbar_never_covers_a_template_at_the_left_margin() {
        let bounds = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(900.0, 700.0));
        // A figure line starting at the margin: the toolbar goes to its right.
        let figure = Rect::from_min_max(Pos2::new(40.0, 100.0), Pos2::new(400.0, 130.0));
        let (at, pivot) = toolbar_anchor(figure, 90.0, bounds);
        assert_eq!(pivot, egui::Align2::LEFT_TOP);
        assert!(at.x >= figure.right());
        // A centred equation keeps it on the left.
        let eq = Rect::from_min_max(Pos2::new(380.0, 100.0), Pos2::new(520.0, 130.0));
        let (at, pivot) = toolbar_anchor(eq, 90.0, bounds);
        assert_eq!(pivot, egui::Align2::RIGHT_TOP);
        assert!(at.x <= eq.left());
    }

    #[test]
    fn recent_items_come_first_and_search_filters() {
        let s = sections("", &["frac".to_string()]);
        assert_eq!(s[0].0, "Recently used");
        assert_eq!(s[0].1[0].id, "frac");
        let groups: Vec<&str> = s.iter().skip(1).map(|(t, _)| t.as_str()).collect();
        assert_eq!(
            groups,
            [
                "Calculus",
                "Sums and products",
                "Algebra",
                "Linear algebra",
                "Greek",
                "Symbols",
                "Helpers",
                "Graphs",
                "Units",
                "Text",
                "Document"
            ]
        );
        let r = sections("matrix", &[]);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].1[0].id, "matrix");
        // A whole word beats the inside of a word.
        assert_eq!(sections("definite", &[])[0].1[0].id, "int.def");
        assert_eq!(sections("alpha", &[])[0].1[0].id, "sym.alpha");
        let sums = &s
            .iter()
            .find(|(t, _)| t == "Sums and products")
            .expect("sums")
            .1;
        assert!(
            sums.iter().any(|t| t.id == "taylor"),
            "series is listed under sums"
        );
    }

    #[test]
    fn choosing_a_matrix_opens_the_picker_first() {
        let mut m = MenuState::default();
        m.open_at(None);
        let t = find("matrix").expect("matrix");
        assert_eq!(m.choose(t), Outcome::Nothing);
        assert!(m.picker.is_some());
        assert!(matches!(m.choose(t), Outcome::Insert { .. }));
        assert!(!m.open);
    }

    #[test]
    fn citation_opens_the_key_picker() {
        let mut m = MenuState::default();
        m.open_at(None);
        let t = find("doc.citation").expect("citation");
        assert!(sections("cite", &[])
            .iter()
            .any(|(_, v)| v.iter().any(|x| x.id == t.id)));
        assert_eq!(m.choose(t), Outcome::Nothing);
        assert_eq!(m.cite, Some(0));
        assert!(m.open);
        m.close();
        assert!(m.cite.is_none());
    }
}
