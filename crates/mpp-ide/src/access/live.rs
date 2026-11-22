//! What a screen reader hears without asking, and how the keyboard gets
//! around the window.
//!
//! - Two live regions (R6): a polite one for arriving answers, Prose/Maths
//!   changes and one-time hints (R9), and an assertive one for new errors.
//!   Nothing is said while typing: answers when a run finishes, the mode when
//!   it changes.
//! - The status bar's path is live and up to date in the frame of the key that
//!   moved the caret (R14); Ctrl+Alt+W says the mode, the path and the line.
//! - F6 / Shift+F6 move between the editor, the side panel, the bottom panel
//!   and the status bar; a panel opened by its shortcut takes the keyboard,
//!   and Esc in a panel gives it back to the editor (R12).
//! - A focus ring (3:1 in both modes) around the widget that has the keyboard,
//!   when it got there by the keyboard (R16).

use egui::{
    accesskit, Context, Id, Key, KeyboardShortcut, LayerId, Modifiers, Order, Pos2, Rect, Stroke,
    StrokeKind, Vec2,
};

use super::editor::is_editor;
use crate::app::IdeApp;
use crate::layout::Pane;

/// "Where am I": the mode, the path to the caret, the file and the line.
pub const WHERE_AM_I: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::ALT), Key::W);
/// The next region of the window.
pub const NEXT_REGION: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F6);
/// The previous region of the window.
pub const PREV_REGION: KeyboardShortcut = KeyboardShortcut::new(Modifiers::SHIFT, Key::F6);

/// The polite live region's node.
pub const POLITE: &str = "mpp-live-polite";
/// The assertive live region's node.
pub const ASSERTIVE: &str = "mpp-live-assertive";

/// The parts of the window F6 moves between, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    Editor,
    Side,
    Bottom,
    Status,
}

const ORDER: [Region; 4] = [Region::Editor, Region::Side, Region::Bottom, Region::Status];

/// A live region's text. Saying the same words twice changes a no-break
/// space at the end, so a screen reader hears them again.
#[derive(Debug, Default, Clone)]
struct Live {
    text: String,
    odd: bool,
}

impl Live {
    fn say(&mut self, s: &str) {
        self.odd = !self.odd;
        self.text = if self.odd {
            format!("{s}\u{a0}")
        } else {
            s.to_string()
        };
    }
}

/// What the last frame showed of the open file, to tell what changed.
#[derive(Debug, Clone)]
struct Seen {
    doc: u64,
    mode: &'static str,
    evaluating: bool,
    /// A run of this file has finished since it was opened.
    settled: bool,
    lines: Vec<(String, bool)>,
    hint: Option<String>,
}

/// The app's accessibility state.
#[derive(Debug, Default)]
pub struct AccessState {
    /// The regions of the last frame, in screen coordinates.
    regions: Vec<(Region, Rect)>,
    /// The panels of the last frame by name (Problems, Explorer), on screen.
    panes: Vec<(&'static str, Rect)>,
    /// A panel to give the keyboard to once it is drawn, and frames tried.
    land: Option<(Pane, u8)>,
    /// The keyboard moved focus last (not the pointer): the ring shows.
    keyboard: bool,
    polite: Live,
    assertive: Live,
    seen: Option<Seen>,
    /// The hints said already (a coder's form on a prose line, P7/P8), by
    /// file: each is said once.
    hinted: std::collections::HashSet<(u64, String)>,
    /// Words to say with the next announcement (after a jump), and frames waited.
    pending: Option<(String, u8)>,
    /// The status bar's path label and the words it drew.
    pub(crate) status_place: Option<(Id, String)>,
    /// Where the status bar is.
    pub(crate) status_rect: Option<Rect>,
}

impl AccessState {
    /// The polite live region's words now.
    pub fn polite(&self) -> &str {
        self.polite.text.trim_end_matches('\u{a0}')
    }

    /// The assertive live region's words now.
    pub fn assertive(&self) -> &str {
        self.assertive.text.trim_end_matches('\u{a0}')
    }
}

/// Makes a clickable label a button for a screen reader: its text becomes the
/// button's name and `does` says what it does.
pub fn as_button(ui: &egui::Ui, r: &egui::Response, does: &str) {
    ui.ctx().accesskit_node_builder(r.id, |n| {
        let text = n.value().map(str::to_string);
        n.set_role(accesskit::Role::Button);
        if let Some(t) = text {
            n.set_label(t);
            n.clear_value();
        }
        n.set_description(does.to_string());
    });
}

/// Where the frame notes what painted rows (Problems items) say, by id.
const LABELS: &str = "mpp-access-labels";

/// Notes what the row `id` says, for "where am I" when it has the keyboard.
pub fn note_label(ctx: &Context, id: Id, said: &str) {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<std::collections::HashMap<Id, String>>(Id::unique(LABELS))
            .insert(id, said.to_string());
    });
}

fn label_of(ctx: &Context, id: Id) -> Option<String> {
    ctx.data(|d| {
        d.get_temp::<std::collections::HashMap<Id, String>>(Id::unique(LABELS))
            .and_then(|m| m.get(&id).cloned())
    })
}

/// A file tab's name for a screen reader: "Untitled-1, unsaved, 4 errors".
pub fn tab_name(title: &str, view: u32, dirty: bool, errors: usize) -> String {
    let mut s = title.to_string();
    if view > 0 {
        s.push_str(&format!(", view {}", view + 1));
    }
    if dirty {
        s.push_str(", unsaved");
    }
    match errors {
        0 => {}
        1 => s.push_str(", 1 error"),
        n => s.push_str(&format!(", {n} errors")),
    }
    s
}

/// A hint in words a screen reader says whole (R-N5): a symbol at its end
/// ("Ctrl+Z gives back the $") is named, which a screen reader would skip.
pub fn spoken_hint(h: &str) -> String {
    let h = h.trim_end();
    for (sym, name) in [
        (" :=", " colon equals you typed"),
        (" $", " dollar sign you typed"),
        (" =", " equals sign you typed"),
    ] {
        if let Some(head) = h.strip_suffix(sym) {
            return format!("{head}{name}.");
        }
    }
    h.to_string()
}

/// The status bar's path in words a screen reader says well.
pub fn spoken_place(place: &str) -> String {
    place.replace(" \u{203a} ", ", ")
}

/// The first widget that takes the keyboard inside `rects`, in drawing order,
/// from this frame's widgets or the last frame's.
fn first_focusable(ctx: &Context, rects: &[Rect], this_pass: bool) -> Option<Id> {
    ctx.viewport(|v| {
        let w = if this_pass {
            &v.this_pass.widgets
        } else {
            &v.prev_pass.widgets
        };
        w.layers()
            .filter(|(l, _)| l.order == Order::Background)
            .flat_map(|(_, list)| list.iter())
            .find(|r| {
                r.sense.is_focusable()
                    && r.enabled
                    && r.visible
                    && r.interact_rect.is_positive()
                    && rects.iter().any(|x| x.contains(r.interact_rect.center()))
            })
            .map(|r| r.id)
    })
}

impl IdeApp {
    /// Gives a panel the keyboard as soon as it is drawn (a panel opened by
    /// its shortcut).
    pub fn land_in(&mut self, pane: Pane) {
        self.access.land = Some((pane, 0));
    }

    /// The region holding the widget `id` (by where it was last frame).
    fn region_of(&self, ctx: &Context, id: Id) -> Option<Region> {
        if is_editor(ctx, id) {
            return Some(Region::Editor);
        }
        let r = ctx.viewport(|v| v.prev_pass.widgets.get(id).map(|w| w.interact_rect))?;
        let c = r.center();
        // The status bar first: the editor's tile does not hold it.
        self.access
            .regions
            .iter()
            .filter(|(k, _)| *k != Region::Editor)
            .chain(
                self.access
                    .regions
                    .iter()
                    .filter(|(k, _)| *k == Region::Editor),
            )
            .find(|(_, rr)| rr.contains(c))
            .map(|(k, _)| *k)
    }

    /// Gives the editor (the active file's page) the keyboard.
    pub(crate) fn focus_editor(&mut self, ctx: &Context) {
        if let Some(id) = self.doc().map(|d| d.id) {
            ctx.memory_mut(|m| {
                if let Some(f) = m.focused() {
                    m.surrender_focus(f);
                }
            });
            self.focus_doc_view(id);
        }
    }

    /// Moves the keyboard to `region` (from the last frame's widgets). False
    /// when the region is not there or has nothing to take the keyboard.
    fn land_region(&mut self, ctx: &Context, region: Region) -> bool {
        if region == Region::Editor {
            if self.doc().is_none() {
                return false;
            }
            self.focus_editor(ctx);
            return true;
        }
        let rects: Vec<Rect> = self
            .access
            .regions
            .iter()
            .filter(|(k, _)| *k == region)
            .map(|(_, r)| *r)
            .collect();
        if rects.is_empty() {
            return false;
        }
        match first_focusable(ctx, &rects, false) {
            Some(id) => {
                ctx.memory_mut(|m| m.request_focus(id));
                true
            }
            None => false,
        }
    }

    /// F6 / Shift+F6: the next (previous) region that can take the keyboard.
    fn cycle_region(&mut self, ctx: &Context, forward: bool) {
        let here = ctx
            .memory(egui::Memory::focused)
            .and_then(|f| self.region_of(ctx, f))
            .unwrap_or(Region::Status);
        let at = ORDER.iter().position(|r| *r == here).unwrap_or(0);
        for step in 1..=ORDER.len() {
            let i = if forward {
                (at + step) % ORDER.len()
            } else {
                (at + ORDER.len() * 2 - step) % ORDER.len()
            };
            if ORDER[i] == here {
                break;
            }
            if self.land_region(ctx, ORDER[i]) {
                let name = match ORDER[i] {
                    Region::Editor => "Editor",
                    Region::Side => "Side panel",
                    Region::Bottom => "Bottom panel",
                    Region::Status => "Status bar",
                };
                self.access.polite.say(name);
                return;
            }
        }
    }

    /// The words of "where am I": mode and path, file, line.
    pub fn where_am_i(&self) -> Option<String> {
        let d = self.doc()?;
        let place = crate::page::caret_path::status_text(d);
        let text = d.editor.text();
        let caret = d.editor.caret().min(text.len());
        let line = text[..caret].matches('\n').count() + 1;
        Some(format!(
            "{}. {}, line {line}",
            spoken_place(&place),
            d.title()
        ))
    }

    /// Where the keyboard is when it is not in the editor: the panel and what
    /// the focused row says ("Problems: Error, line 3: w is not defined.").
    fn where_focus(&self, ctx: &Context) -> Option<String> {
        let f = ctx.memory(egui::Memory::focused)?;
        if is_editor(ctx, f) {
            return None;
        }
        let at = ctx.viewport(|v| v.prev_pass.widgets.get(f).map(|w| w.interact_rect.center()));
        let pane = at.and_then(|c| {
            self.access
                .panes
                .iter()
                .find(|(_, r)| r.contains(c))
                .map(|(n, _)| *n)
        });
        let pane = pane.or_else(|| match self.region_of(ctx, f) {
            Some(Region::Status) => Some("Status bar"),
            Some(Region::Side) => Some("Side panel"),
            Some(Region::Bottom) => Some("Bottom panel"),
            _ => None,
        });
        match (pane, label_of(ctx, f)) {
            (Some(p), Some(l)) => Some(format!("{p}: {l}")),
            (Some(p), None) => Some(p.to_string()),
            (None, Some(l)) => Some(l),
            (None, None) => None,
        }
    }

    /// After a jump from the Problems panel: the editor takes the keyboard
    /// again and says the line it is on (R12).
    pub(crate) fn back_to_editor_at(&mut self, ctx: &Context) {
        // The Enter (or Space) that chose the row is not typed into the page.
        ctx.input_mut(|i| {
            i.consume_key(Modifiers::NONE, Key::Enter);
            i.consume_key(Modifiers::NONE, Key::Space);
        });
        self.focus_editor(ctx);
        let line = self.doc().map(|d| {
            let t = d.editor.text();
            t[..d.editor.caret().min(t.len())].matches('\n').count() + 1
        });
        if let Some(n) = line {
            self.access.pending = Some((format!("Editor, line {n}"), 0));
        }
    }

    /// Start of a frame, before any other key is seen: Ctrl+Alt+W, F6, Esc in
    /// a panel, and ∑ markers clicked by a screen reader.
    pub(crate) fn access_keys(&mut self, ctx: &Context) {
        let (pressed, keyed) = ctx.input(|i| {
            (
                i.pointer.any_pressed(),
                i.events
                    .iter()
                    .any(|e| matches!(e, egui::Event::Key { pressed: true, .. })),
            )
        });
        if pressed {
            self.access.keyboard = false;
        } else if keyed {
            self.access.keyboard = true;
        }
        for at in super::take_marker_clicks(ctx) {
            if let Some(d) = self.doc_mut() {
                if d.editor.flip_maths(&mut d.maths, at) {
                    d.touched();
                }
            }
        }
        // Before Ctrl+W (close tab), which would match it too.
        let (where_key, back, fwd) = ctx.input_mut(|i| {
            (
                i.consume_shortcut(&WHERE_AM_I),
                i.consume_shortcut(&PREV_REGION),
                i.consume_shortcut(&NEXT_REGION),
            )
        });
        if where_key {
            let w = match self.where_focus(ctx) {
                Some(f) => Some(match self.where_am_i() {
                    Some(w) => format!("{f}. Editor: {w}"),
                    None => f,
                }),
                None => self.where_am_i(),
            };
            if let Some(w) = w {
                self.access.polite.say(&w);
            }
        }
        if back || fwd {
            // Shift+F6 is consumed first: F6 alone also matches it.
            self.cycle_region(ctx, !back);
        }
        // Esc in a panel (not a text box, which uses Esc itself) gives the
        // keyboard back to the editor.
        let esc = ctx.input(|i| i.key_pressed(Key::Escape) && i.modifiers.is_none());
        if esc && !ctx.any_popup_open() {
            if let Some(f) = ctx.memory(egui::Memory::focused) {
                let text_box = egui::TextEdit::load_state(ctx, f).is_some();
                let in_panel = matches!(
                    self.region_of(ctx, f),
                    Some(Region::Side | Region::Bottom | Region::Status)
                );
                if in_panel && !text_box {
                    ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
                    self.focus_editor(ctx);
                    self.access.polite.say("Editor");
                }
            }
        }
    }

    /// The regions of the window as this frame drew them.
    fn measure_regions(&mut self) {
        let mut regions = Vec::new();
        let mut panes = Vec::new();
        if let Some(tree) = &self.ws.tiles.tree {
            for id in tree.active_tiles() {
                let Some(egui_tiles::Tile::Pane(p)) = tree.tiles.get(id) else {
                    continue;
                };
                let Some(r) = tree.tiles.rect(id) else {
                    continue;
                };
                panes.push((p.name(), r));
                let kind = match p {
                    Pane::Equation { .. } => Region::Editor,
                    p if p.is_side() => Region::Side,
                    _ => Region::Bottom,
                };
                regions.push((kind, r));
            }
        }
        if let Some(r) = self.access.status_rect {
            regions.push((Region::Status, r));
        }
        self.access.regions = regions;
        self.access.panes = panes;
    }

    /// A panel opened by its shortcut takes the keyboard once it is drawn.
    fn land_pending(&mut self, ctx: &Context) {
        let Some((pane, tries)) = self.access.land.take() else {
            return;
        };
        let rect = self
            .ws
            .tiles
            .tree
            .as_ref()
            .and_then(|t| crate::layout::tiles::find(t, &pane).and_then(|id| t.tiles.rect(id)));
        match rect.and_then(|r| first_focusable(ctx, &[r], true)) {
            Some(id) => {
                ctx.memory_mut(|m| m.request_focus(id));
                self.access.keyboard = true;
                ctx.request_repaint();
            }
            None if tries < 3 => {
                self.access.land = Some((pane, tries + 1));
                ctx.request_repaint();
            }
            None => {}
        }
    }

    /// What changed in the open file since the last frame, said in the live
    /// regions: the mode, a hint, answers and errors from a finished run.
    fn announce(&mut self) {
        let Some(d) = self.docs.get(self.active) else {
            return;
        };
        let Some(pc) = d.page_cache().cloned() else {
            return;
        };
        let fresh = pc.revision() == d.editor.revision();
        let mode = if pc.page.in_math(d.editor.caret()) {
            "Maths"
        } else {
            "Prose"
        };
        let evaluating = d.evaluating();
        let hint = d.editor.hint().map(str::to_string);
        let same = self.access.seen.as_ref().filter(|s| s.doc == d.id).cloned();
        let lines = if fresh && !evaluating {
            super::answers::page_lines(d, &pc)
        } else {
            same.as_ref().map(|s| s.lines.clone()).unwrap_or_default()
        };
        let mut polite: Vec<String> = Vec::new();
        let mut assertive: Vec<String> = Vec::new();
        if let Some(s) = &same {
            if fresh && s.mode != mode {
                polite.push(format!("In {}", mode.to_lowercase()));
            }
            if let (Some(h), true) = (&hint, hint != s.hint) {
                polite.push(format!("Hint: {}", spoken_hint(h)));
            }
            // The answers of a file just opened are not news: only what a
            // later run changes is said.
            if s.settled && s.evaluating && !evaluating && fresh {
                for (l, error) in lines.iter().filter(|l| !s.lines.contains(l)) {
                    if *error {
                        assertive.push(l.clone());
                    } else {
                        polite.push(l.clone());
                    }
                }
            }
        }
        // A hint the page marks on a prose line (P7, P8: `print(R)`), which a
        // screen reader cannot hover: said once, when the page has settled.
        // Those of a file just opened are only noted.
        if fresh && !evaluating {
            let settled = same.as_ref().is_some_and(|s| s.settled);
            for p in pc.problems.iter().filter(|p| {
                p.level == crate::diagnostics::Level::Info
                    && matches!(p.kind, crate::diagnostics::Kind::Other)
            }) {
                // Not while the line is being typed (`R` on its way to
                // `Range`): once the caret has left it.
                let caret = d.editor.caret();
                if p.span.start <= caret && caret <= p.span.end {
                    continue;
                }
                let m = p.rich.plain();
                let m = m.trim().trim_end_matches('.').to_string();
                if self.access.hinted.insert((d.id, m.clone())) && settled {
                    polite.push(format!("Hint: {m}"));
                }
            }
        }
        // Words held back (a jump's line) go first, with what the page says
        // once it has settled, or alone after a few frames.
        if let Some((p, n)) = self.access.pending.take() {
            if !polite.is_empty() || n >= 3 {
                polite.insert(0, p);
            } else {
                self.access.pending = Some((p, n + 1));
            }
        }
        polite.truncate(4);
        assertive.truncate(2);
        if !polite.is_empty() {
            self.access.polite.say(&polite.join(". "));
        }
        if !assertive.is_empty() {
            self.access.assertive.say(&assertive.join(". "));
        }
        self.access.seen = Some(Seen {
            doc: d.id,
            mode: if fresh {
                mode
            } else {
                same.as_ref().map_or(mode, |s| s.mode)
            },
            evaluating: evaluating || !fresh,
            settled: same.as_ref().is_some_and(|s| s.settled) || (fresh && !evaluating),
            lines,
            hint,
        });
    }

    /// The live nodes: both live regions and the status bar's path, the path
    /// as it is after this frame's keys.
    fn live_nodes(&mut self, ctx: &Context) {
        let at = self
            .access
            .status_rect
            .unwrap_or_else(|| Rect::from_min_size(Pos2::ZERO, Vec2::splat(1.0)));
        let bounds = accesskit::Rect {
            x0: at.min.x.into(),
            y0: at.min.y.into(),
            x1: at.max.x.into(),
            y1: at.max.y.into(),
        };
        let polite = self.access.polite.text.clone();
        let assertive = self.access.assertive.text.clone();
        for (id, role, live, text) in [
            (
                POLITE,
                accesskit::Role::Status,
                accesskit::Live::Polite,
                polite,
            ),
            (
                ASSERTIVE,
                accesskit::Role::Alert,
                accesskit::Live::Assertive,
                assertive,
            ),
        ] {
            ctx.accesskit_node_builder(Id::unique(id), |n| {
                n.set_role(role);
                n.set_live(live);
                n.set_bounds(bounds);
                if !text.is_empty() {
                    n.set_label(text);
                }
            });
        }
        let fresh = self.doc().map(crate::page::caret_path::status_text);
        if let (Some((id, drawn)), Some(fresh)) = (&self.access.status_place, fresh) {
            ctx.accesskit_node_builder(*id, |n| {
                n.set_label(spoken_place(&fresh));
                n.set_value(fresh.clone());
                n.set_live(accesskit::Live::Polite);
            });
            if *drawn != fresh {
                // The bar was drawn before the page saw this frame's keys.
                ctx.request_repaint();
            }
        }
    }

    /// The ring around the widget that has the keyboard.
    fn focus_ring(&self, ctx: &Context) {
        if !self.access.keyboard {
            return;
        }
        let Some(id) = ctx.memory(egui::Memory::focused) else {
            return;
        };
        if is_editor(ctx, id) {
            return;
        }
        let Some(r) = ctx.read_response(id) else {
            return;
        };
        let color = crate::theme::c32(self.mode.palette().focus_ring);
        let painter = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::unique("mpp-focus-ring")));
        let rect = match ctx.layer_transform_to_global(r.layer_id) {
            Some(t) => t * r.rect,
            None => r.rect,
        };
        painter.rect_stroke(
            rect.expand(1.0),
            3.0,
            Stroke::new(2.0, color),
            StrokeKind::Outside,
        );
    }

    /// End of a frame, after everything is drawn.
    pub(crate) fn access_after(&mut self, ctx: &Context) {
        self.measure_regions();
        self.land_pending(ctx);
        self.announce();
        self.live_nodes(ctx);
        self.focus_ring(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_words_are_said_again() {
        let mut l = Live::default();
        l.say("Maths");
        let first = l.text.clone();
        l.say("Maths");
        assert_ne!(first, l.text);
        assert_eq!(l.text.trim_end_matches('\u{a0}'), "Maths");
    }

    #[test]
    fn a_hint_ending_in_a_symbol_names_it() {
        assert_eq!(
            spoken_hint("Maths line. Ctrl+Z gives back the $"),
            "Maths line. Ctrl+Z gives back the dollar sign you typed."
        );
        assert_eq!(
            spoken_hint("Maths line. Ctrl+Z gives back the :="),
            "Maths line. Ctrl+Z gives back the colon equals you typed."
        );
        assert_eq!(spoken_hint("Plain words."), "Plain words.");
    }

    #[test]
    fn the_path_is_spoken_with_commas() {
        assert_eq!(
            spoken_place("Maths \u{203a} fraction \u{203a} denominator"),
            "Maths, fraction, denominator"
        );
    }
}
