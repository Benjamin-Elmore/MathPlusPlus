//! The tile manager inside the app: draws the tree, keeps it in step with the
//! open files, and carries the commands (split, maximise, move focus, named
//! layouts).

use std::path::PathBuf;

use egui::{Align2, Color32, Context, Key, KeyboardShortcut, Modifiers, Stroke, Ui, WidgetText};
use egui_tiles::{Behavior, TabState, TileId, Tiles, Tree, UiResponse};
use mpp_render::Theme;

use super::persist::{self, LayoutFile};
use super::tiles::{self, Dir, Pane, Preset};
use crate::app::IdeApp;
use crate::diagnostics::Counts;
use crate::paint::c32;
use crate::settings::SideView;
use crate::tabs::breadcrumbs;
use crate::view::{self, ViewCtx, ViewEvent};
use crate::workspace::PendingClose;

const SPLIT_RIGHT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Backslash);
const SPLIT_DOWN: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Backslash);
const MAXIMISE: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Enter);
const CHORD: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::K);
/// Ctrl+K then Ctrl+T switches the theme.
const CHORD_T: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::T);

/// State of the tile manager.
#[derive(Default)]
pub struct TileState {
    /// The layout. Taken out while it is drawn.
    pub tree: Option<Tree<Pane>>,
    /// The panel with the keyboard's attention.
    pub focused: Option<TileId>,
    /// A panel filling the whole area.
    pub maximised: Option<TileId>,
    /// The file that was active last frame (to notice changes from outside).
    last_active_doc: Option<u64>,
    next_view: u32,
    /// Ctrl+K was pressed; an arrow key (move focus) or Ctrl+T (theme)
    /// finishes the chord.
    pub chord_k: bool,
    /// Saved layouts of the open project.
    pub file: LayoutFile,
    /// The project the layouts were loaded for.
    loaded_for: Option<PathBuf>,
    saved_json: String,
    /// The text of the "save layout as" box, while it is open.
    pub naming: Option<String>,
    last_tab_click: Option<(TileId, std::time::Instant)>,
}

impl std::fmt::Debug for TileState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TileState")
    }
}

/// What a tab menu asked for.
#[derive(Debug, Clone, Copy)]
enum Request {
    Split(TileId, Dir),
    Maximise(TileId),
    Close(TileId),
}

struct AppBehavior<'a> {
    app: &'a mut IdeApp,
    theme: Theme,
    focused: Option<TileId>,
    pressed: Option<TileId>,
    requests: Vec<Request>,
    events: Vec<(usize, ViewEvent)>,
    crumb: Option<PathBuf>,
    empty_groups: Vec<TileId>,
    many_tiles: bool,
    /// Problem counts of the files with problems (for the tab titles).
    counts: Vec<(PathBuf, Counts)>,
    /// The equation view that owns the Insert menu, quick fixes and popups.
    active_view: Option<TileId>,
}

impl AppBehavior<'_> {
    fn side_fill(&self) -> Color32 {
        c32(self.theme.code_background).gamma_multiply(0.7)
    }

    fn doc_index(&self, id: u64) -> Option<usize> {
        self.app.docs.iter().position(|d| d.id == id)
    }
}

impl Behavior<Pane> for AppBehavior<'_> {
    fn pane_ui(&mut self, ui: &mut Ui, tile_id: TileId, pane: &mut Pane) -> UiResponse {
        let rect = ui.max_rect();
        if ui.input(|i| i.pointer.any_pressed()) && ui.rect_contains_pointer(rect) {
            self.pressed = Some(tile_id);
        }
        let theme = self.theme;
        let fill = match pane {
            Pane::Equation { .. } | Pane::Graph { .. } => c32(theme.background),
            _ => self.side_fill(),
        };
        ui.painter().rect_filled(rect, 0.0, fill);
        match pane.clone() {
            Pane::Equation { doc, view } => match self.doc_index(doc) {
                Some(idx) => {
                    ui.push_id((doc, view), |ui| {
                        if let Some(root) = self.app.project_root().map(|p| p.to_path_buf()) {
                            if let Some(file) = self.app.docs[idx].path.clone() {
                                ui.add_space(3.0);
                                if let Some(p) = breadcrumbs::show(ui, &root, &file, &theme) {
                                    self.crumb = Some(p);
                                }
                                ui.add_space(3.0);
                            }
                        }
                        let active = self.active_view == Some(tile_id);
                        for e in self.app.equation_ui(ui, idx, active) {
                            self.events.push((idx, e));
                        }
                    });
                }
                None => {
                    ui.weak("This file is closed.");
                }
            },
            Pane::Graph { doc, figure } => self.app.graph_tile(ui, doc, figure, &theme),
            Pane::Explorer => self.app.explorer_view(ui, &theme),
            Pane::Search => self.app.search_view(ui, &theme),
            Pane::Outline => self.app.outline_tile(ui, &theme),
            Pane::Variables => self.app.variables_tile(ui, &theme),
            Pane::Problems => self.app.problems_tile(ui, &theme),
            Pane::Steps => self.app.steps_tile(ui, &theme),
            Pane::Library => self.app.library_tile(ui, &theme),
        }
        if self.many_tiles && self.focused == Some(tile_id) {
            ui.painter().rect_stroke(
                rect.shrink(0.5),
                0.0,
                Stroke::new(1.0, c32(theme.link).gamma_multiply(0.55)),
                egui::StrokeKind::Inside,
            );
        }
        UiResponse::None
    }

    fn tab_title_for_pane(&mut self, pane: &Pane) -> WidgetText {
        match pane {
            Pane::Equation { doc, view } => {
                let Some(d) = self.app.docs.iter().find(|d| d.id == *doc) else {
                    return "Closed".into();
                };
                let mut title = d.title();
                if d.is_dirty() {
                    title.push_str(" \u{25cf}");
                }
                if *view > 0 {
                    title.push_str(&format!(" ({})", view + 1));
                }
                let mut t = egui::RichText::new(title);
                if self.app.is_preview(d) {
                    t = t.italics();
                }
                let c = d
                    .path
                    .as_ref()
                    .and_then(|p| self.counts.iter().find(|(q, _)| q == p))
                    .map(|(_, c)| *c)
                    .unwrap_or_else(|| d.problem_counts());
                if c.errors > 0 {
                    t = egui::RichText::new(format!("{}  {}", t.text(), c.errors))
                        .color(c32(self.theme.error));
                    if self.app.is_preview(d) {
                        t = t.italics();
                    }
                }
                t.into()
            }
            Pane::Graph { figure, .. } => format!("Figure {}", figure + 1).into(),
            other => other.name().into(),
        }
    }

    fn is_tab_closable(&self, _tiles: &Tiles<Pane>, _tile_id: TileId) -> bool {
        true
    }

    fn on_tab_close(&mut self, tiles: &mut Tiles<Pane>, tile_id: TileId) -> bool {
        let Some(Pane::Equation { doc, .. }) = tiles.get_pane(&tile_id).cloned() else {
            return true;
        };
        let views = tiles
            .iter()
            .filter(|(_, t)| matches!(t, egui_tiles::Tile::Pane(Pane::Equation { doc: d, .. }) if *d == doc))
            .count();
        if views > 1 {
            return true;
        }
        let Some(idx) = self.doc_index(doc) else {
            return true;
        };
        if self.app.docs[idx].is_dirty() {
            self.app.ws.pending_close = Some(PendingClose(idx));
            false
        } else {
            self.app.close_doc(idx);
            true
        }
    }

    fn on_tab_button(
        &mut self,
        _tiles: &mut Tiles<Pane>,
        tile_id: TileId,
        button_response: egui::Response,
    ) -> egui::Response {
        if button_response.clicked() || button_response.drag_started() {
            self.pressed = Some(tile_id);
        }
        // egui reports double-clicks itself; also count two clicks on the same tab
        // in quick succession (some input sources give no click timing).
        let mut double = button_response.double_clicked();
        if button_response.clicked() {
            let now = std::time::Instant::now();
            if let Some((t, at)) = self.app.ws.tiles.last_tab_click {
                if t == tile_id && now.duration_since(at).as_millis() < 450 {
                    double = true;
                }
            }
            self.app.ws.tiles.last_tab_click = Some((tile_id, now));
        }
        if double {
            self.app.ws.tiles.last_tab_click = None;
            self.requests.push(Request::Maximise(tile_id));
        }
        button_response.context_menu(|ui| {
            if ui.button("Split right").clicked() {
                self.requests.push(Request::Split(tile_id, Dir::Right));
                ui.close();
            }
            if ui.button("Split down").clicked() {
                self.requests.push(Request::Split(tile_id, Dir::Down));
                ui.close();
            }
            if ui.button("Maximise").clicked() {
                self.requests.push(Request::Maximise(tile_id));
                ui.close();
            }
            if ui.button("Close").clicked() {
                self.requests.push(Request::Close(tile_id));
                ui.close();
            }
        });
        button_response
    }

    fn cycle_tabs_with_keyboard(&self) -> bool {
        false
    }

    fn simplification_options(&self) -> egui_tiles::SimplificationOptions {
        super::simplification()
    }

    fn tab_bar_color(&self, _visuals: &egui::Visuals) -> Color32 {
        c32(self.theme.code_background)
    }

    fn tab_bg_color(
        &self,
        _visuals: &egui::Visuals,
        _tiles: &Tiles<Pane>,
        _tile_id: TileId,
        state: &TabState,
    ) -> Color32 {
        if state.active {
            c32(self.theme.background)
        } else {
            crate::theme::CLEAR
        }
    }

    fn tab_outline_stroke(
        &self,
        _visuals: &egui::Visuals,
        _tiles: &Tiles<Pane>,
        _tile_id: TileId,
        _state: &TabState,
    ) -> Stroke {
        Stroke::NONE
    }

    fn tab_text_color(
        &self,
        _visuals: &egui::Visuals,
        _tiles: &Tiles<Pane>,
        _tile_id: TileId,
        state: &TabState,
    ) -> Color32 {
        if state.active {
            c32(self.theme.text)
        } else {
            c32(self.theme.muted)
        }
    }

    fn tab_bar_hline_stroke(&self, _visuals: &egui::Visuals) -> Stroke {
        Stroke::new(1.0, c32(self.theme.rule))
    }

    fn resize_stroke(&self, _style: &egui::Style, state: egui_tiles::ResizeState) -> Stroke {
        let c = c32(self.theme.text).gamma_multiply(0.16);
        match state {
            egui_tiles::ResizeState::Idle => Stroke::new(1.0, c),
            egui_tiles::ResizeState::Hovering => Stroke::new(2.0, c32(self.theme.link)),
            egui_tiles::ResizeState::Dragging => Stroke::new(3.0, c32(self.theme.link)),
        }
    }

    fn paint_on_top_of_tile(
        &self,
        painter: &egui::Painter,
        _style: &egui::Style,
        tile_id: TileId,
        rect: egui::Rect,
    ) {
        if self.empty_groups.contains(&tile_id) {
            painter.rect_filled(rect, 0.0, c32(self.theme.background));
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "No file open",
                egui::FontId::proportional(16.0),
                c32(self.theme.muted),
            );
        }
    }
}

impl IdeApp {
    fn doc_table(&self) -> Vec<(u64, String)> {
        self.docs
            .iter()
            .map(|d| {
                let key = d.path.as_ref().map_or_else(
                    || format!("untitled-{}", d.id),
                    |p| p.to_string_lossy().into(),
                );
                (d.id, key)
            })
            .collect()
    }

    fn doc_ids(&self) -> Vec<u64> {
        self.docs.iter().map(|d| d.id).collect()
    }

    /// Loads the project's layouts the first time the tree is needed (and again
    /// when the project changes).
    fn ensure_tree(&mut self) {
        let root = self.project_root().map(std::path::Path::to_path_buf);
        let stale = self.ws.tiles.tree.is_some() && self.ws.tiles.loaded_for != root;
        if self.ws.tiles.tree.is_some() && !stale {
            return;
        }
        let data = self.ws.settings_dir.clone();
        let file = match (&data, &root) {
            (Some(d), Some(r)) => persist::load(d, r),
            _ => LayoutFile::default(),
        };
        let table = self.doc_table();
        let loaded = file
            .current
            .as_deref()
            .and_then(|j| tiles::from_json(j, table));
        let active = self.doc().map(|d| d.id);
        let tree = loaded
            .filter(|t| t.root().is_some())
            .unwrap_or_else(|| tiles::build(Preset::Writing, &self.doc_ids(), active));
        self.ws.tiles.file = file;
        self.ws.tiles.loaded_for = root;
        self.ws.tiles.tree = Some(tree);
        self.ws.tiles.saved_json.clear();
        self.ws.tiles.focused = None;
        self.ws.tiles.maximised = None;
    }

    /// Runs `f` on the layout.
    pub fn with_tree<R>(&mut self, f: impl FnOnce(&mut Tree<Pane>) -> R) -> Option<R> {
        self.ensure_tree();
        self.ws.tiles.tree.as_mut().map(f)
    }

    /// Opens a panel, or brings it to the front.
    pub fn open_pane(&mut self, pane: Pane) {
        let side = pane.is_side();
        let view = match pane {
            Pane::Search => Some(SideView::Search),
            Pane::Outline => Some(SideView::Outline),
            Pane::Explorer => Some(SideView::Explorer),
            _ => None,
        };
        self.with_tree(|t| {
            tiles::open_pane(t, pane);
        });
        self.ws.tiles.maximised = None;
        if side {
            self.ws.side_visible = true;
        }
        if let Some(v) = view {
            self.ws.side_view = v;
        }
    }

    /// True when a panel is in the layout.
    pub fn has_pane(&mut self, pane: &Pane) -> bool {
        self.with_tree(|t| tiles::find(t, pane).is_some())
            .unwrap_or(false)
    }

    /// Closes a panel if it is open, otherwise opens it (the View menu).
    pub fn toggle_pane(&mut self, pane: Pane) {
        let found = self.with_tree(|t| tiles::find(t, &pane)).flatten();
        match found {
            Some(id) => {
                self.with_tree(|t| tiles::close_pane(t, id));
            }
            None => self.open_pane(pane),
        }
    }

    /// Ctrl+B: hides or shows the group holding the explorer.
    pub fn toggle_side_tiles(&mut self) {
        let shown = self
            .with_tree(|t| {
                let g = tiles::side_group(t)?;
                let now = t.tiles.is_visible(g);
                t.tiles.set_visible(g, !now);
                Some(!now)
            })
            .flatten();
        match shown {
            Some(v) => self.ws.side_visible = v,
            None => self.open_pane(Pane::Explorer),
        }
    }

    /// Ctrl+\\ and Ctrl+Shift+\\: a second view of the focused file beside it.
    pub fn split_focused(&mut self, dir: Dir) {
        self.ensure_tree();
        let active = self.doc().map(|d| d.id);
        let tiles_state = &mut self.ws.tiles;
        tiles_state.next_view += 1;
        let view = tiles_state.next_view;
        let focused = tiles_state.focused;
        let Some(tree) = tiles_state.tree.as_mut() else {
            return;
        };
        let target = focused
            .filter(|f| tree.tiles.get(*f).is_some())
            .or_else(|| {
                tiles::find_by(
                    tree,
                    |p| matches!(p, Pane::Equation { doc, .. } if Some(*doc) == active),
                )
            })
            .or_else(|| tiles::find_by(tree, |p| matches!(p, Pane::Equation { .. })));
        if let Some(t) = target {
            if let Some(new) = tiles::split_pane(tree, t, dir, view) {
                tiles_state.focused = Some(new);
            }
        }
    }

    /// Opens a graph tile for figure `figure` (counting from 0) of a file.
    pub fn open_graph_tile(&mut self, doc_index: usize, figure: usize) {
        let Some(doc) = self.docs.get(doc_index).map(|d| d.id) else {
            return;
        };
        self.with_tree(|t| {
            let pane = Pane::Graph { doc, figure };
            if let Some(id) = tiles::find(t, &pane) {
                tiles::activate(t, id);
            } else {
                let focus = tiles::find_by(
                    t,
                    |p| matches!(p, Pane::Equation { doc: d, .. } if *d == doc),
                );
                let id = t.tiles.insert_pane(pane);
                let group = t.tiles.insert_tab_tile(vec![id]);
                if let Some(f) = focus.and_then(|f| t.tiles.parent_of(f)) {
                    tiles::split_beside(t, f, group, Dir::Right);
                }
                tiles::activate(t, id);
            }
        });
    }

    /// Ctrl+Shift+Enter, or a double-click on a tab.
    pub fn toggle_maximise(&mut self) {
        let f = self.ws.tiles.focused;
        self.maximise(f);
    }

    fn maximise(&mut self, tile: Option<TileId>) {
        let st = &mut self.ws.tiles;
        st.maximised = if st.maximised.is_some() { None } else { tile };
    }

    /// The panel with focus is maximised.
    pub fn is_maximised(&self) -> bool {
        self.ws.tiles.maximised.is_some()
    }

    /// Ctrl+K then an arrow: moves focus to the neighbouring panel.
    pub fn focus_dir(&mut self, dir: Dir) {
        let from = self.ws.tiles.focused;
        let target = self
            .with_tree(|t| {
                let from =
                    from.or_else(|| tiles::find_by(t, |p| matches!(p, Pane::Equation { .. })))?;
                tiles::neighbour(t, from, dir)
            })
            .flatten();
        if let Some(t) = target {
            self.ws.tiles.focused = Some(t);
            let doc = self
                .with_tree(|tr| tiles::pane_of(tr, t).and_then(Pane::doc))
                .flatten();
            if let Some(i) = doc.and_then(|d| self.docs.iter().position(|x| x.id == d)) {
                self.active = i;
            }
            // Make sure the panel is the shown tab of its group.
            self.with_tree(|tr| tiles::activate(tr, t));
        }
    }

    /// Puts a preset in place.
    pub fn apply_preset(&mut self, preset: Preset) {
        let active = self.doc().map(|d| d.id);
        let tree = tiles::build(preset, &self.doc_ids(), active);
        self.set_tree(tree);
    }

    /// View → Reset layout.
    pub fn reset_layout(&mut self) {
        self.apply_preset(Preset::Writing);
    }

    fn set_tree(&mut self, tree: Tree<Pane>) {
        self.ensure_tree();
        self.ws.tiles.tree = Some(tree);
        self.ws.tiles.maximised = None;
        self.ws.tiles.focused = None;
        self.ws.side_visible = true;
    }

    /// Saves the current layout under a name.
    pub fn save_named_layout(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() || Preset::ALL.iter().any(|p| p.name() == name) {
            self.message = Some("Pick a name that is not one of the built-in layouts.".into());
            return;
        }
        let table = self.doc_table();
        let json = self
            .with_tree(|t| tiles::to_json(t, table))
            .unwrap_or_default();
        self.ws.tiles.file.named.insert(name.to_string(), json);
        self.persist_layout();
    }

    /// Switches to a built-in or saved layout by name.
    pub fn switch_layout(&mut self, name: &str) {
        if let Some(p) = Preset::ALL.iter().find(|p| p.name() == name) {
            self.apply_preset(*p);
            return;
        }
        let json = self.ws.tiles.file.named.get(name).cloned();
        let table = self.doc_table();
        if let Some(tree) = json.and_then(|j| tiles::from_json(&j, table)) {
            self.set_tree(tree);
        }
    }

    /// Removes a saved layout.
    pub fn delete_named_layout(&mut self, name: &str) {
        self.ws.tiles.file.named.remove(name);
        self.persist_layout();
    }

    /// Writes the layout when it changed (per project, in the app-data folder).
    pub fn persist_layout(&mut self) {
        let (Some(dir), Some(root)) = (
            self.ws.settings_dir.clone(),
            self.project_root().map(std::path::Path::to_path_buf),
        ) else {
            return;
        };
        let table = self.doc_table();
        let Some(json) = self
            .ws
            .tiles
            .tree
            .as_ref()
            .map(|t| tiles::to_json(t, table))
        else {
            return;
        };
        if json != self.ws.tiles.saved_json || self.ws.tiles.file.current.as_deref() != Some(&json)
        {
            self.ws.tiles.file.current = Some(json.clone());
            if persist::save(&dir, &root, &self.ws.tiles.file).is_ok() {
                self.ws.tiles.saved_json = json;
            }
        }
    }

    /// View menu entries for panels and layouts.
    pub(crate) fn layout_menu(&mut self, ui: &mut Ui) {
        ui.menu_button("Panels", |ui| {
            for p in super::tiles::MENU_PANES {
                let mut open = self.has_pane(&p);
                if ui.checkbox(&mut open, p.name()).changed() {
                    self.toggle_pane(p.clone());
                }
            }
        });
        ui.menu_button("Layout", |ui| {
            for p in Preset::ALL {
                if ui.button(p.name()).clicked() {
                    self.apply_preset(p);
                    ui.close();
                }
            }
            let saved: Vec<String> = self.ws.tiles.file.named.keys().cloned().collect();
            if !saved.is_empty() {
                ui.separator();
            }
            for name in saved {
                ui.horizontal(|ui| {
                    if ui.button(&name).clicked() {
                        self.switch_layout(&name);
                        ui.close();
                    }
                    if ui.small_button("Delete").clicked() {
                        self.delete_named_layout(&name);
                    }
                });
            }
            ui.separator();
            if ui.button("Save current layout as\u{2026}").clicked() {
                self.ws.tiles.naming = Some(String::new());
                ui.close();
            }
            if ui.button("Reset layout").clicked() {
                self.reset_layout();
                ui.close();
            }
        });
        if ui
            .add(egui::Button::new("Split right").shortcut_text("Ctrl+\\"))
            .clicked()
        {
            self.split_focused(Dir::Right);
            ui.close();
        }
        if ui
            .add(egui::Button::new("Split down").shortcut_text("Ctrl+Shift+\\"))
            .clicked()
        {
            self.split_focused(Dir::Down);
            ui.close();
        }
        if ui
            .add(egui::Button::new("Maximise or restore").shortcut_text("Ctrl+Shift+Enter"))
            .clicked()
        {
            self.toggle_maximise();
            ui.close();
        }
    }

    /// The "save layout as" box.
    pub(crate) fn layout_name_dialog(&mut self, ctx: &Context) {
        let Some(mut name) = self.ws.tiles.naming.clone() else {
            return;
        };
        let mut done = None;
        egui::Window::new("Save layout")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut name).hint_text("Layout name"));
                r.request_focus();
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    if ui.button("Save").clicked() || enter {
                        done = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        done = Some(false);
                    }
                });
            });
        match done {
            Some(true) => {
                self.ws.tiles.naming = None;
                self.save_named_layout(&name);
            }
            Some(false) => self.ws.tiles.naming = None,
            None => self.ws.tiles.naming = Some(name),
        }
    }

    /// The tiles' keys.
    pub(crate) fn tile_shortcuts(&mut self, ctx: &Context) {
        let (down, right, max, chord) = ctx.input_mut(|i| {
            (
                i.consume_shortcut(&SPLIT_DOWN),
                i.consume_shortcut(&SPLIT_RIGHT),
                i.consume_shortcut(&MAXIMISE),
                i.consume_shortcut(&CHORD),
            )
        });
        if down {
            self.split_focused(Dir::Down);
        } else if right {
            self.split_focused(Dir::Right);
        }
        if max {
            self.toggle_maximise();
        }
        if chord {
            self.ws.tiles.chord_k = true;
        } else if self.ws.tiles.chord_k && ctx.input_mut(|i| i.consume_shortcut(&CHORD_T)) {
            self.set_theme_choice(self.theme_choice.next());
            self.ws.tiles.chord_k = false;
        } else if self.ws.tiles.chord_k {
            let dir = ctx.input_mut(|i| {
                [
                    (Key::ArrowLeft, Dir::Left),
                    (Key::ArrowRight, Dir::Right),
                    (Key::ArrowUp, Dir::Up),
                    (Key::ArrowDown, Dir::Down),
                ]
                .into_iter()
                .find(|(k, _)| i.consume_key(Modifiers::NONE, *k))
                .map(|(_, d)| d)
            });
            let any_key = ctx.input(|i| {
                i.events
                    .iter()
                    .any(|e| matches!(e, egui::Event::Key { pressed: true, .. }))
            });
            if let Some(d) = dir {
                self.focus_dir(d);
            }
            if dir.is_some() || any_key {
                self.ws.tiles.chord_k = false;
            }
        }
    }

    /// Draws the tiles in the space that is left.
    pub(crate) fn tiles_ui(&mut self, ui: &mut Ui) {
        self.ensure_tree();
        let ids = self.doc_ids();
        let active_doc = self.doc().map(|d| d.id);
        let focused = self.ws.tiles.focused;
        let Some(mut tree) = self.ws.tiles.tree.take() else {
            return;
        };
        tiles::sync_docs(&mut tree, &ids, focused);
        if active_doc != self.ws.tiles.last_active_doc {
            if let Some(a) = active_doc {
                tiles::show_doc(&mut tree, a, focused);
            }
            self.ws.tiles.last_active_doc = active_doc;
        }
        if let Some(f) = self.ws.tiles.focused {
            if tree.tiles.get(f).is_none() {
                self.ws.tiles.focused = None;
            }
        }
        let maximised = self
            .ws
            .tiles
            .maximised
            .filter(|m| tree.tiles.get(*m).is_some());
        self.ws.tiles.maximised = maximised;
        let theme = self.mode.theme();
        let empty_groups: Vec<TileId> = tree
            .tiles
            .iter()
            .filter_map(|(id, t)| match t {
                egui_tiles::Tile::Container(egui_tiles::Container::Tabs(tabs))
                    if tabs.children.is_empty() =>
                {
                    Some(*id)
                }
                _ => None,
            })
            .collect();
        let many = tree
            .tiles
            .iter()
            .filter(|(id, t)| matches!(t, egui_tiles::Tile::Pane(_)) && tree.is_visible(**id))
            .count()
            > 1;
        let counts = self.file_counts();
        let active_view = active_view(&tree, focused, maximised, active_doc);
        let mut b = AppBehavior {
            app: self,
            theme,
            focused,
            pressed: None,
            requests: vec![],
            events: vec![],
            crumb: None,
            empty_groups,
            many_tiles: many,
            counts,
            active_view,
        };
        match maximised {
            Some(m) => draw_maximised(&mut b, &mut tree, ui, m),
            None => tree.ui(&mut b, ui),
        }
        let (pressed, requests, events, crumb) = (b.pressed, b.requests, b.events, b.crumb);
        self.ws.tiles.tree = Some(tree);
        if let Some(t) = pressed {
            self.ws.tiles.focused = Some(t);
            let doc = self
                .ws
                .tiles
                .tree
                .as_ref()
                .and_then(|tr| tiles::pane_of(tr, t))
                .and_then(Pane::doc);
            if let Some(i) = doc.and_then(|d| self.docs.iter().position(|x| x.id == d)) {
                if i != self.active {
                    self.note_place();
                    self.active = i;
                    self.note_place();
                }
                self.ws.tiles.last_active_doc = self.docs.get(i).map(|d| d.id);
            }
            if let Some(v) =
                self.ws
                    .tiles
                    .tree
                    .as_ref()
                    .and_then(|tr| match tiles::pane_of(tr, t) {
                        Some(Pane::Explorer) => Some(SideView::Explorer),
                        Some(Pane::Search) => Some(SideView::Search),
                        Some(Pane::Outline) => Some(SideView::Outline),
                        _ => None,
                    })
            {
                self.ws.side_view = v;
            }
        }
        for r in requests {
            match r {
                Request::Split(t, d) => {
                    self.ws.tiles.focused = Some(t);
                    self.split_focused(d);
                }
                Request::Maximise(t) => self.maximise(Some(t)),
                Request::Close(t) => {
                    let pane = self
                        .with_tree(|tr| tiles::pane_of(tr, t).cloned())
                        .flatten();
                    if let Some(Pane::Equation { doc, .. }) = pane {
                        if let Some(i) = self.docs.iter().position(|d| d.id == doc) {
                            self.request_close(i);
                        }
                    } else {
                        self.with_tree(|tr| tiles::close_pane(tr, t));
                    }
                }
            }
        }
        for (idx, e) in events {
            if idx < self.docs.len() {
                self.active = idx;
            }
            match e {
                ViewEvent::GotoAt(o) => self.goto_at(o),
                ViewEvent::OpenLinkAt(o) => self.open_link_at(o),
                ViewEvent::Save => self.save(),
                ViewEvent::OpenFigureTile(f) => self.open_graph_tile(idx, f),
                ViewEvent::ExportFigure(f, svg) => self.export_figure(idx, f, svg),
                ViewEvent::Open(p) => {
                    if let Err(m) = self.open_file(&p, false) {
                        self.message = Some(m);
                    }
                }
                ViewEvent::Prove(name) => self.prove(&name),
                ViewEvent::AlignLean(name) => self.align_with_lean(&name),
                ViewEvent::Conjecture => self.propose_conjectures(),
            }
        }
        if let Some(p) = crumb {
            self.reveal_in_explorer(&p);
        }
        // Keep the activity bar and Ctrl+B in step with the layout.
        let side = self
            .ws
            .tiles
            .tree
            .as_ref()
            .and_then(tiles::side_group)
            .and_then(|g| self.ws.tiles.tree.as_ref().map(|t| t.tiles.is_visible(g)));
        if let Some(v) = side {
            self.ws.side_visible = v;
        }
    }

    /// One equation view: the typeset page of document `idx`.
    /// `active` marks the view that owns the Insert menu, the quick-fix menu
    /// and the other pop-ups (the focused view, or the active file's).
    pub(crate) fn equation_ui(&mut self, ui: &mut Ui, idx: usize, active: bool) -> Vec<ViewEvent> {
        let (mode, size, copy_format) = (self.mode, self.size, self.copy_format);
        let Some(doc) = self.docs.get_mut(idx) else {
            return vec![];
        };
        doc.integral_limits = if self.insert.settings.integral_side {
            mpp_render::IntegralLimits::Side
        } else {
            mpp_render::IntegralLimits::AboveBelow
        };
        let mut cx = ViewCtx {
            mode,
            size,
            completion: &mut self.completion,
            clip: &mut self.clip,
            copy_format,
            insert: &mut self.insert,
            diag: &mut self.diag,
            active,
        };
        let events = view::show(ui, doc, &mut cx);
        let dropped = ui.interact(
            ui.max_rect(),
            ui.scope_id().with("import-drop"),
            egui::Sense::hover(),
        );
        if let Some(src) = dropped.dnd_release_payload::<PathBuf>() {
            self.active = idx;
            self.add_import(&src);
        }
        events
    }

    pub(crate) fn reveal_in_explorer(&mut self, p: &std::path::Path) {
        self.open_pane(Pane::Explorer);
        self.ws.explorer.reveal(p);
        self.ws.explorer.selected = Some(p.to_path_buf());
    }
}

/// The equation view that owns the pop-ups this frame: the focused tile when
/// it is an equation view, otherwise a visible view of the active file.
fn active_view(
    tree: &Tree<Pane>,
    focused: Option<TileId>,
    maximised: Option<TileId>,
    active_doc: Option<u64>,
) -> Option<TileId> {
    let is_view = |t: TileId| matches!(tiles::pane_of(tree, t), Some(Pane::Equation { .. }));
    if let Some(m) = maximised {
        return is_view(m).then_some(m);
    }
    let shown = tree.active_tiles();
    if let Some(f) = focused.filter(|f| is_view(*f) && shown.contains(f)) {
        return Some(f);
    }
    shown.into_iter().find(|t| {
        matches!(tiles::pane_of(tree, *t), Some(Pane::Equation { doc, .. }) if Some(*doc) == active_doc)
    })
}

fn draw_maximised(b: &mut AppBehavior<'_>, tree: &mut Tree<Pane>, ui: &mut Ui, id: TileId) {
    let title = tree
        .tiles
        .get_pane(&id)
        .map(|p| b.tab_title_for_pane(p))
        .map(|t| t.text().to_string())
        .unwrap_or_default();
    let theme = b.theme;
    let mut restore = false;
    egui::Panel::top("maximised-bar")
        .frame(
            egui::Frame::NONE
                .fill(c32(theme.code_background))
                .inner_margin(egui::Margin::symmetric(10, 4)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(title).color(c32(theme.text)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Restore").clicked() {
                        restore = true;
                    }
                });
            });
        });
    if let Some(egui_tiles::Tile::Pane(p)) = tree.tiles.get_mut(id) {
        let mut pane = p.clone();
        let _ = b.pane_ui(ui, id, &mut pane);
    }
    if restore {
        b.requests.push(Request::Maximise(id));
    }
}
