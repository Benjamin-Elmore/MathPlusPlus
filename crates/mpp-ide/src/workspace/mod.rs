//! The workspace: project folder, open files, navigation and search, joined to
//! the app. Everything here is `impl IdeApp` glue over the explorer, tabs and
//! settings modules, so `app.rs` only calls into it.

mod tools;
mod ui;

use std::path::{Path, PathBuf};

use egui::Context;
use mpp_syntax::Span;

use crate::app::IdeApp;
use crate::doc::Doc;
use crate::explorer::links::{self, map_move};
use crate::explorer::outline::OutlineItem;
use crate::explorer::search::{self, Hit};
use crate::explorer::{ops, ExplorerAction, ExplorerState};
use crate::layout::behavior::TileState;
use crate::settings::{AutosaveMode, Settings, SideView};
use crate::tabs::nav::{NavHistory, Place};

/// The Ctrl+P box.
#[derive(Debug, Default)]
pub struct QuickOpen {
    pub open: bool,
    pub query: String,
    pub selected: usize,
}

/// What the Search view shows.
#[derive(Debug, Default)]
pub struct SearchState {
    pub query: String,
    pub last_run: String,
    /// `Some(name)` when the results are the uses of a name.
    pub uses_of: Option<String>,
    pub hits: Vec<Hit>,
    pub focus: bool,
}

/// A file with edits waiting on the user's answer before it closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PendingClose(pub usize);

/// Project, navigation and side-bar state.
pub struct Workspace {
    pub explorer: ExplorerState,
    pub side_visible: bool,
    pub side_view: SideView,
    pub side_width: f32,
    pub search: SearchState,
    pub nav: NavHistory,
    /// The preview tab (shown slanted), as a document id and the revision it
    /// had when it opened; any edit makes it a real tab.
    pub preview: Option<(u64, u64)>,
    pub quick: QuickOpen,
    pub outline_open: bool,
    pub tiles: TileState,
    pub vars: crate::layout::panes::VarEdit,
    pub autosave: crate::autosave::AutosaveState,
    pub pending_close: Option<PendingClose>,
    pub outline_cache: Option<(u64, u64, Vec<OutlineItem>)>,
    pub settings_dir: Option<PathBuf>,
    pub settings: Settings,
    saved_settings: Option<Settings>,
}

impl Default for Workspace {
    fn default() -> Self {
        let s = Settings::default();
        Workspace {
            explorer: ExplorerState::default(),
            side_visible: s.side_visible,
            side_view: s.side_view,
            side_width: s.side_width,
            search: SearchState::default(),
            nav: NavHistory::default(),
            preview: None,
            quick: QuickOpen::default(),
            outline_open: true,
            tiles: TileState::default(),
            vars: Default::default(),
            autosave: Default::default(),
            pending_close: None,
            outline_cache: None,
            settings_dir: None,
            settings: s,
            saved_settings: None,
        }
    }
}

impl IdeApp {
    /// The project folder, if one is open.
    pub fn project_root(&self) -> Option<&Path> {
        self.ws.explorer.root.as_deref()
    }

    /// Opens a folder as the project.
    pub fn open_folder(&mut self, root: &Path, ctx: Option<&Context>) {
        self.ws.explorer.set_root(Some(root.to_path_buf()), ctx);
        self.ws.settings.last_folder = Some(root.to_path_buf());
        self.message = None;
    }

    /// The text of a file: the open (maybe edited) copy, or the one on disk.
    pub fn read_text(&self, path: &Path) -> Option<String> {
        if let Some(d) = self.docs.iter().find(|d| d.path.as_deref() == Some(path)) {
            return Some(d.editor.text().to_string());
        }
        mpp_eval::read_text(path).ok()
    }

    /// Where the caret is, as a place to come back to.
    pub fn current_place(&self) -> Option<Place> {
        let d = self.doc()?;
        Some(Place {
            path: d.path.clone()?,
            offset: d.editor.caret(),
        })
    }

    pub(crate) fn note_place(&mut self) {
        if let Some(p) = self.current_place() {
            self.ws.nav.record(p);
        }
    }

    /// Opens a file as a tab. `preview` makes it a slanted tab that the next
    /// preview replaces unless it is edited. Records the jump for Alt+Left.
    pub fn open_file(&mut self, path: &Path, preview: bool) -> Result<usize, String> {
        self.note_place();
        let known = self
            .docs
            .iter()
            .position(|d| d.path.as_deref() == Some(path));
        let before_ids: Vec<u64> = self.docs.iter().map(|d| d.id).collect();
        let replaced = if known.is_none() && preview {
            self.take_preview_slot()
        } else {
            None
        };
        let i = self.open_path(path)?;
        if let Some(id) = replaced {
            if let Some(pos) = self.docs.iter().position(|d| d.id == id) {
                let new_id = self.docs[i].id;
                self.docs.remove(pos);
                self.active = self
                    .docs
                    .iter()
                    .position(|d| d.id == new_id)
                    .unwrap_or(self.active);
            }
        }
        let i = self.active;
        if known.is_none() && !before_ids.contains(&self.docs[i].id) {
            if preview {
                let d = &self.docs[i];
                self.ws.preview = Some((d.id, d.editor.revision()));
            }
        } else if !preview {
            // Opening a previewed file for real keeps it.
            if self.ws.preview.is_some_and(|(id, _)| id == self.docs[i].id) {
                self.ws.preview = None;
            }
        }
        match (known, self.current_place()) {
            (None, Some(p)) => self.ws.nav.record_opened(p),
            _ => self.note_place(),
        }
        self.ws.explorer.reveal(path);
        Ok(i)
    }

    /// The id of the clean, unedited preview tab that a new preview replaces.
    fn take_preview_slot(&mut self) -> Option<u64> {
        let (id, rev) = self.ws.preview?;
        let d = self.docs.iter().find(|d| d.id == id)?;
        if d.editor.revision() == rev && !d.is_dirty() {
            Some(id)
        } else {
            self.ws.preview = None;
            None
        }
    }

    /// Makes an edited preview tab a real one (call once a frame).
    pub(crate) fn settle_preview(&mut self) {
        if let Some((id, rev)) = self.ws.preview {
            match self.docs.iter().find(|d| d.id == id) {
                Some(d) if d.editor.revision() == rev => {}
                _ => self.ws.preview = None,
            }
        }
    }

    /// True when the document is the preview tab.
    pub fn is_preview(&self, doc: &Doc) -> bool {
        self.ws.preview.is_some_and(|(id, _)| id == doc.id)
    }

    /// Opens a file and puts the caret (and the view) at `span`.
    pub fn jump_to_file(&mut self, path: &Path, span: Span) -> Result<(), String> {
        let i = self.open_file(path, false)?;
        if let Some(d) = self.docs.get_mut(i) {
            let at = span.start.min(d.editor.text().len());
            d.editor.move_caret(at, false);
            d.highlight = Some(span);
            d.scroll_to = Some(span);
        }
        self.note_place();
        Ok(())
    }

    /// Alt+Left.
    pub fn nav_back(&mut self) {
        self.note_place();
        if let Some(p) = self.ws.nav.back() {
            self.go_place(&p);
        }
    }

    /// Alt+Right.
    pub fn nav_forward(&mut self) {
        if let Some(p) = self.ws.nav.forward() {
            self.go_place(&p);
        }
    }

    fn go_place(&mut self, p: &Place) {
        if self.open_path(&p.path).is_ok() {
            if let Some(d) = self.docs.get_mut(self.active) {
                let at = p.offset.min(d.editor.text().len());
                d.editor.move_caret(at, false);
                d.scroll_to = Some(Span::new(at, at));
            }
        }
    }

    /// Ctrl+Tab / Ctrl+Shift+Tab: the next tab of the group on screen.
    pub fn cycle_tab(&mut self, forward: bool) {
        let focused = self.ws.tiles.focused;
        let active = self.doc().map(|d| d.id);
        let order = self
            .with_tree(|t| {
                let from = focused
                    .filter(|f| t.tiles.get(*f).is_some())
                    .or_else(|| {
                        crate::layout::tiles::find_by(t, |p| {
                            matches!(p, crate::layout::Pane::Equation { doc, .. } if Some(*doc) == active)
                        })
                    })?;
                Some(crate::layout::tiles::group_docs(t, from))
            })
            .flatten()
            .unwrap_or_default();
        let Some(now) = active.and_then(|a| order.iter().position(|d| *d == a)) else {
            return;
        };
        let n = order.len();
        let next = if forward {
            (now + 1) % n
        } else {
            (now + n - 1) % n
        };
        if let Some(i) = self.docs.iter().position(|d| d.id == order[next]) {
            self.note_place();
            self.active = i;
            self.note_place();
        }
    }

    /// Closes tab `i`, asking first when it has unsaved edits.
    pub fn request_close(&mut self, i: usize) {
        if self.docs.get(i).is_some_and(Doc::is_dirty) {
            self.ws.pending_close = Some(PendingClose(i));
        } else {
            self.close_doc(i);
        }
    }

    /// Closes tab `i` now, dropping any unsaved edits.
    pub fn close_doc(&mut self, i: usize) {
        if i >= self.docs.len() {
            return;
        }
        let id = self.docs[i].id;
        self.forget_recovery(i);
        self.docs.remove(i);
        if self.ws.preview.is_some_and(|(p, _)| p == id) {
            self.ws.preview = None;
        }
        if i < self.active || self.active >= self.docs.len() {
            self.active = self
                .active
                .saturating_sub(1)
                .min(self.docs.len().saturating_sub(1));
        }
    }

    /// Moves tab `from` to the place of tab `to`.
    pub fn move_tab(&mut self, from: usize, to: usize) {
        let n = self.docs.len();
        if from >= n || to >= n || from == to {
            return;
        }
        let active_id = self.docs.get(self.active).map(|d| d.id);
        let d = self.docs.remove(from);
        self.docs.insert(to, d);
        if let Some(id) = active_id {
            self.active = self.docs.iter().position(|d| d.id == id).unwrap_or(0);
        }
    }

    /// Moves or renames a file or folder, rewriting every import that points
    /// at it (or out of it) across the project, and keeping open tabs.
    pub fn move_item(&mut self, from: &Path, to: &Path) -> Result<(), String> {
        ops::check_move(from, to)?;
        let root = self
            .ws
            .explorer
            .root
            .clone()
            .unwrap_or_else(|| from.parent().map(Path::to_path_buf).unwrap_or_default());
        let read = |p: &Path| self.read_text(p);
        let plan = ops::plan_move(&root, from, to, &read);
        ops::move_on_disk(from, to)?;
        // Open files: follow the move, and take the new text.
        let mut written: Vec<PathBuf> = vec![];
        for d in &mut self.docs {
            let Some(old) = d.path.clone() else {
                continue;
            };
            let moved = map_move(&old, from, to);
            let edit = plan.iter().find(|p| p.old == old);
            if let Some(e) = edit {
                let was_dirty = d.is_dirty();
                let caret = d.editor.caret();
                let end = d.editor.text().len();
                d.editor.replace_node(Span::new(0, end), &e.text);
                d.editor.move_caret(caret, false);
                d.touched();
                d.path = Some(moved.clone());
                if !was_dirty {
                    let _ = d.save();
                }
                written.push(moved);
            } else {
                d.path = Some(moved);
            }
        }
        for e in &plan {
            if !written.contains(&e.new) {
                std::fs::write(&e.new, &e.text).map_err(|er| {
                    format!("Could not update imports in {}: {er}", e.new.display())
                })?;
            }
        }
        if let Some(sel) = self.ws.explorer.selected.clone() {
            self.ws.explorer.selected = Some(map_move(&sel, from, to));
        }
        self.ws.explorer.expanded = self
            .ws
            .explorer
            .expanded
            .iter()
            .map(|p| map_move(p, from, to))
            .collect();
        self.ws.explorer.refresh();
        Ok(())
    }

    /// Sends a file or folder to the recycle bin and closes its tabs.
    pub fn delete_item(&mut self, path: &Path) -> Result<(), String> {
        ops::delete(path)?;
        self.docs
            .retain(|d| d.path.as_deref().is_none_or(|p| !p.starts_with(path)));
        self.active = self.active.min(self.docs.len().saturating_sub(1));
        self.ws.nav.forget(path);
        self.ws.explorer.selected = None;
        self.ws.explorer.refresh();
        Ok(())
    }

    /// Adds `![[file]]` to the current page for a file dropped onto it.
    pub fn add_import(&mut self, file: &Path) {
        let Some(doc) = self.docs.get_mut(self.active) else {
            return;
        };
        let dir = doc
            .path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .or_else(|| self.ws.explorer.root.clone());
        let Some(dir) = dir else {
            return;
        };
        if !crate::explorer::tree::is_mpp(file) {
            return;
        }
        let rel = links::relative(&dir, file, false);
        let line = format!("![[{rel}]]\n\n");
        doc.editor.move_caret(0, false);
        doc.editor.insert_text(&line);
        doc.touched();
    }

    /// Runs what the explorer panel asked for.
    pub(crate) fn run_explorer_actions(&mut self, actions: Vec<ExplorerAction>) {
        for a in actions {
            let r = match a {
                ExplorerAction::Open { path, preview } => self.open_file(&path, preview).map(drop),
                ExplorerAction::Created(path) => self.open_file(&path, false).map(drop),
                ExplorerAction::Activate(i) => {
                    if i < self.docs.len() {
                        self.note_place();
                        self.active = i;
                        self.note_place();
                        // Its tab comes to the front and its page takes the keyboard.
                        self.ws.tiles.focus_doc = Some(self.docs[i].id);
                    }
                    Ok(())
                }
                ExplorerAction::Close(i) => {
                    self.request_close(i);
                    Ok(())
                }
                ExplorerAction::Move { from, to } => self.move_item(&from, &to),
                ExplorerAction::Delete(p) => self.delete_item(&p),
                ExplorerAction::Message(m) => Err(m),
            };
            if let Err(m) = r {
                self.message = Some(m);
            }
        }
    }

    /// Searches every file in the project.
    pub fn run_search(&mut self, query: &str) {
        let files = self.search_files();
        let read = |p: &Path| self.read_text(p);
        let hits = search::search_files(&files, &read, query);
        self.ws.search.hits = hits;
        self.ws.search.uses_of = None;
        self.ws.search.last_run = query.to_string();
    }

    /// Shift+F12: every use of the name at `offset`, in every file.
    pub fn find_uses_at(&mut self, offset: usize) {
        let Some(name) = self.doc().and_then(|d| {
            crate::defs::name_candidates(d.editor.text(), offset)
                .into_iter()
                .next()
        }) else {
            return;
        };
        let files = self.search_files();
        let read = |p: &Path| self.read_text(p);
        let hits = search::find_uses(&files, &read, &name);
        self.ws.search.hits = hits;
        self.ws.search.query = name.clone();
        self.ws.search.last_run = name.clone();
        self.ws.search.uses_of = Some(name);
        self.show_side(SideView::Search);
    }

    fn search_files(&self) -> Vec<PathBuf> {
        let mut files = self.ws.explorer.all_files().to_vec();
        for d in &self.docs {
            if let Some(p) = &d.path {
                if !files.contains(p) {
                    files.push(p.clone());
                }
            }
        }
        files
    }

    /// Shows a side-bar view (and the side bar).
    pub fn show_side(&mut self, view: SideView) {
        self.open_pane(crate::layout::Pane::of_side(view));
        self.ws.side_view = view;
        self.ws.side_visible = true;
        if view == SideView::Search {
            self.ws.search.focus = true;
        }
    }

    /// Hides the side column (the activity bar stays).
    pub fn hide_side(&mut self) {
        self.ws.side_visible = false;
        if let Some(t) = self.ws.tiles.tree.as_mut() {
            crate::layout::tiles::sync_side(t, self.ws.side_view, false);
        }
    }

    /// Ctrl+B: hides the side column, or shows it again with its last view.
    pub fn toggle_side(&mut self) {
        if self.ws.side_visible {
            self.hide_side();
        } else {
            self.show_side(self.ws.side_view);
        }
    }

    /// A click on an activity-bar icon: shows that view, or hides the side
    /// column when the view is already showing (VS Code's behaviour).
    pub fn activity_click(&mut self, view: SideView) {
        if self.ws.side_visible && self.ws.side_view == view {
            self.hide_side();
        } else {
            self.show_side(view);
        }
    }

    /// Opens a hit of a search.
    pub fn open_hit(&mut self, hit: &Hit) {
        if let Err(m) = self.jump_to_file(&hit.file, hit.span) {
            self.message = Some(m);
        }
    }

    /// Opens the file named by the import at `offset` in the current page.
    pub fn open_link_at(&mut self, offset: usize) {
        let target = self.doc().and_then(|d| {
            let dir = d.path.as_deref().and_then(Path::parent)?;
            let text = d.editor.text();
            let at = offset.min(text.len());
            let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
            let line_end = text[at..].find('\n').map_or(text.len(), |i| at + i);
            // A whole `![[file]]` line opens its file; elsewhere the click has to
            // land on the written name.
            let whole_line = text[line_start..line_end].trim_start().starts_with("![[");
            let l = links::scan_links(text).into_iter().find(|l| {
                if whole_line {
                    l.range.start >= line_start && l.range.end <= line_end
                } else {
                    l.range.start <= offset && offset <= l.range.end
                }
            })?;
            Some(links::resolve(dir, &l.path))
        });
        if let Some(t) = target {
            if t.is_file() {
                if let Err(m) = self.open_file(&t, false) {
                    self.message = Some(m);
                }
            } else {
                self.message = Some(self.missing_import_message(&t));
            }
        }
    }

    /// The files the current page imports.
    pub fn current_imports(&self) -> Vec<PathBuf> {
        let Some(d) = self.doc() else {
            return vec![];
        };
        d.link_targets().iter().map(|(p, _)| p.clone()).collect()
    }

    /// Reloads open files that were changed on disk and have no unsaved edits.
    pub(crate) fn reload_changed(&mut self) {
        for d in &mut self.docs {
            if d.is_dirty() {
                continue;
            }
            let Some(p) = d.path.clone() else {
                continue;
            };
            if let Ok(text) = std::fs::read_to_string(&p) {
                if text != d.editor.text() {
                    d.reload(&text);
                }
            }
        }
    }

    /// The settings as they are now.
    pub fn settings_snapshot(&self) -> Settings {
        let mut s = self.ws.settings.clone();
        s.side_visible = self.ws.side_visible;
        s.side_view = self.ws.side_view;
        s.side_width = self.ws.side_width;
        s.size = self.size;
        s.autosave = self.autosave_mode();
        s.theme = self.theme_choice.to_stored().into();
        s.insert = self.insert.settings.to_stored();
        s.copy_latex = self.copy_format == crate::copy::CopyFormat::Latex;
        s.auto_eval = self.eval_settings.auto_eval;
        s.show_steps = self.eval_settings.show_steps;
        s.decimals = self.eval_settings.number_format.style == mpp_core::display::Style::Decimal;
        s.proof_cap = self.proof.agent.cap;
        s.agent_id = self.proof.settings.agent_id.clone();
        s.environment_id = self.proof.settings.environment_id.clone();
        s.last_folder = self.ws.explorer.root.clone();
        s.tabs = self.docs.iter().filter_map(|d| d.path.clone()).collect();
        s.active = self
            .doc()
            .and_then(|d| d.path.as_ref())
            .and_then(|p| s.tabs.iter().position(|t| t == p))
            .unwrap_or(0);
        s
    }

    /// Writes the settings when they changed since the last write.
    pub fn persist_settings(&mut self) {
        let Some(dir) = self.ws.settings_dir.clone() else {
            return;
        };
        let now = self.settings_snapshot();
        if self.ws.saved_settings.as_ref() != Some(&now) && now.save(&dir).is_ok() {
            self.ws.settings = now.clone();
            self.ws.saved_settings = Some(now);
        }
    }

    /// Reads the settings folder and reopens the last folder and tabs.
    /// Returns true on the very first launch (no settings file yet).
    pub fn restore_session(&mut self, dir: &Path, ctx: Option<&Context>) -> bool {
        let first = Settings::is_first_launch(dir);
        let s = Settings::load(dir);
        self.ws.settings_dir = Some(dir.to_path_buf());
        self.ws.side_visible = s.side_visible;
        self.ws.side_view = s.side_view;
        self.ws.side_width = s.side_width;
        self.size = s.size;
        self.set_autosave_mode(s.autosave);
        if let Some(c) = crate::theme::ThemeChoice::from_stored(&s.theme) {
            self.set_theme_choice(c);
        }
        if !s.insert.is_empty() {
            self.insert.settings = crate::insert::InsertSettings::from_stored(&s.insert);
        }
        if s.copy_latex {
            self.copy_format = crate::copy::CopyFormat::Latex;
        }
        self.eval_settings.auto_eval = s.auto_eval;
        self.eval_settings.show_steps = s.show_steps;
        self.proof.agent.cap = s.proof_cap;
        self.proof.settings.agent_id = s.agent_id.clone();
        self.proof.settings.environment_id = s.environment_id.clone();
        self.eval_settings.number_format.style = if s.decimals {
            mpp_core::display::Style::Decimal
        } else {
            mpp_core::display::Style::Exact
        };
        if let Some(folder) = s.last_folder.clone().filter(|f| f.is_dir()) {
            self.open_folder(&folder, ctx);
        }
        let existing: Vec<PathBuf> = s.tabs.iter().filter(|p| p.is_file()).cloned().collect();
        if !existing.is_empty() {
            self.docs.clear();
            for p in &existing {
                let _ = self.open_path(p);
            }
            let want = s
                .tabs
                .get(s.active)
                .and_then(|a| existing.iter().position(|e| e == a));
            self.active = want.unwrap_or(0).min(self.docs.len().saturating_sub(1));
        }
        self.ws.settings = s;
        self.ws.saved_settings = Some(self.settings_snapshot());
        first
    }

    /// Autosave setting (kept in settings).
    pub fn autosave_mode(&self) -> AutosaveMode {
        self.ws.settings.autosave
    }

    /// Changes the autosave setting.
    pub fn set_autosave_mode(&mut self, m: AutosaveMode) {
        self.ws.settings.autosave = m;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mpp-ws-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("mkdir");
        d
    }

    #[test]
    fn rename_rewrites_imports_in_open_and_closed_files() {
        let d = temp("rename");
        std::fs::write(d.join("m.mpp"), "Let $x = 1$.\n").expect("w");
        std::fs::write(d.join("a.mpp"), "![[m]]\n").expect("w");
        std::fs::write(d.join("b.mpp"), "Using $x$ from [[m.mpp]].\n").expect("w");
        let mut app = IdeApp::empty();
        app.open_folder(&d, None);
        app.open_file(&d.join("a.mpp"), false).expect("open");
        app.move_item(&d.join("m.mpp"), &d.join("mech.mpp"))
            .expect("move");
        assert_eq!(app.doc().expect("doc").editor.text(), "![[mech]]\n");
        assert!(
            !app.doc().expect("doc").is_dirty(),
            "saved after the rewrite"
        );
        assert_eq!(
            std::fs::read_to_string(d.join("a.mpp")).expect("r"),
            "![[mech]]\n"
        );
        assert_eq!(
            std::fs::read_to_string(d.join("b.mpp")).expect("r"),
            "Using $x$ from [[mech.mpp]].\n"
        );
        assert!(d.join("mech.mpp").is_file() && !d.join("m.mpp").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
