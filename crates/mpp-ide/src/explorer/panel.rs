//! The explorer panel: open files, then the project folder as a tree.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use egui::{
    Align2, Color32, FontId, Key, PointerButton, Rect, Response, Role, Sense, TextEdit, Ui, Vec2,
    WidgetInfo,
};
use mpp_render::Theme;

use super::tree::{self, Row};
use super::{icons, links, ops};
use crate::diagnostics::Counts;
use crate::paint::c32;

const ROW_H: f32 = 24.0;
const INDENT: f32 = 14.0;

/// An open file, as the "Open files" section shows it.
#[derive(Debug, Clone)]
pub struct OpenFile {
    pub index: usize,
    pub title: String,
    pub dirty: bool,
    pub active: bool,
    pub preview: bool,
    pub broken: bool,
    /// The file on disk (none for an unsaved new file).
    pub path: Option<PathBuf>,
}

/// What the panel needs to know to draw.
pub struct ExplorerCtx<'a> {
    pub theme: Theme,
    pub open: &'a [OpenFile],
    pub current: Option<&'a Path>,
    /// Files the current file imports.
    pub linked: &'a [PathBuf],
    /// Error, warning and note counts of files with problems.
    pub counts: &'a [(PathBuf, Counts)],
}

impl ExplorerCtx<'_> {
    /// The problem counts of a file (zero when it has none).
    fn counts_of(&self, path: &Path) -> Counts {
        self.counts
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, c)| *c)
            .unwrap_or_default()
    }
}

/// Room kept free for a file's count at the right of its row.
fn count_width(c: Counts) -> f32 {
    let n = if c.errors > 0 { c.errors } else { c.warnings };
    if n == 0 {
        0.0
    } else {
        8.0 + 8.0 * n.to_string().len() as f32
    }
}

/// Paints a file's error count (or its warning count when it has no errors)
/// right-aligned at `right`; returns the width used.
fn paint_count(p: &egui::Painter, right: egui::Pos2, c: Counts, t: &Theme) -> f32 {
    let (n, color) = if c.errors > 0 {
        (c.errors, t.error)
    } else if c.warnings > 0 {
        (c.warnings, t.warning)
    } else {
        return 0.0;
    };
    p.text(
        right,
        Align2::RIGHT_CENTER,
        n.to_string(),
        FontId::proportional(14.0),
        c32(color),
    )
    .width()
}

/// What the user asked for in the panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplorerAction {
    Open {
        path: PathBuf,
        preview: bool,
    },
    Activate(usize),
    Close(usize),
    /// Rename or move (imports are rewritten by the app).
    Move {
        from: PathBuf,
        to: PathBuf,
    },
    /// Delete to the recycle bin (already confirmed).
    Delete(PathBuf),
    /// A file was created and should open.
    Created(PathBuf),
    /// Make the folder in the tree the project.
    Message(String),
}

#[derive(Debug, Clone)]
struct FileInfo {
    stamp: (u64, Option<SystemTime>),
    broken: bool,
}

/// A name being typed for a new file or folder.
#[derive(Debug, Clone)]
pub struct NewItem {
    pub dir: PathBuf,
    pub is_dir: bool,
    pub name: String,
}

/// The explorer's state.
#[derive(Debug, Default)]
pub struct ExplorerState {
    pub root: Option<PathBuf>,
    pub expanded: BTreeSet<PathBuf>,
    pub rows: Vec<Row>,
    pub selected: Option<PathBuf>,
    pub rename: Option<(PathBuf, String)>,
    pub create: Option<NewItem>,
    pub confirm_delete: Option<PathBuf>,
    pub files_collapsed: bool,
    pub tree_collapsed: bool,
    watch: Option<super::watch::FolderWatch>,
    info: HashMap<PathBuf, FileInfo>,
    all: Vec<PathBuf>,
    stale: bool,
    focus_rename: bool,
}

impl ExplorerState {
    /// Makes `root` the project folder and starts watching it.
    pub fn set_root(&mut self, root: Option<PathBuf>, ctx: Option<&egui::Context>) {
        self.watch = root
            .as_deref()
            .and_then(|r| super::watch::FolderWatch::new(r, ctx.cloned()));
        if self.root != root {
            self.expanded.clear();
            self.selected = None;
        }
        self.root = root;
        self.refresh();
    }

    /// Rescans the folder (call after any change).
    pub fn refresh(&mut self) {
        self.stale = false;
        let Some(root) = self.root.clone() else {
            self.rows.clear();
            self.all.clear();
            return;
        };
        self.expanded.retain(|p| p.is_dir());
        self.rows = tree::rows(&root, &self.expanded);
        self.all = tree::all_mpp(&root);
        let mut info = HashMap::new();
        for f in &self.all {
            let meta = std::fs::metadata(f).ok();
            let stamp = (
                meta.as_ref().map(std::fs::Metadata::len).unwrap_or(0),
                meta.and_then(|m| m.modified().ok()),
            );
            let fi = match self.info.get(f) {
                Some(old) if old.stamp == stamp => old.clone(),
                _ => FileInfo {
                    stamp,
                    broken: file_has_broken_import(f),
                },
            };
            info.insert(f.clone(), fi);
        }
        self.info = info;
    }

    /// Picks up changes made outside the app; call once a frame.
    pub fn poll(&mut self) -> bool {
        let changed = self
            .watch
            .as_ref()
            .is_some_and(super::watch::FolderWatch::changed);
        if changed || self.stale {
            self.refresh();
        }
        changed
    }

    /// Marks the tree as needing a rescan.
    pub fn mark_stale(&mut self) {
        self.stale = true;
    }

    /// Every `.mpp` file in the project, in tree order.
    pub fn all_files(&self) -> &[PathBuf] {
        &self.all
    }

    /// True when a file in the project has an import that points nowhere.
    pub fn has_broken_import(&self, p: &Path) -> bool {
        self.info.get(p).is_some_and(|i| i.broken)
    }

    /// Opens or closes a folder.
    pub fn toggle(&mut self, dir: &Path) {
        if !self.expanded.remove(dir) {
            self.expanded.insert(dir.to_path_buf());
        }
        self.refresh();
    }

    /// Opens every folder on the way to `path` and selects it.
    pub fn reveal(&mut self, path: &Path) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let mut p = path.parent();
        while let Some(d) = p {
            if d == root || !d.starts_with(&root) {
                break;
            }
            self.expanded.insert(d.to_path_buf());
            p = d.parent();
        }
        self.selected = Some(path.to_path_buf());
        self.refresh();
    }

    /// Starts renaming a file or folder in place.
    pub fn start_rename(&mut self, path: &Path) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.rename = Some((path.to_path_buf(), name));
        self.focus_rename = true;
    }

    /// Starts typing the name of a new file or folder inside `dir`.
    pub fn start_create(&mut self, dir: &Path, is_dir: bool) {
        self.expanded.insert(dir.to_path_buf());
        self.refresh();
        self.create = Some(NewItem {
            dir: dir.to_path_buf(),
            is_dir,
            name: String::new(),
        });
        self.focus_rename = true;
    }

    /// The folder new items go in: the selected folder, the folder of the
    /// selected file, or the project root.
    pub fn target_dir(&self) -> Option<PathBuf> {
        match &self.selected {
            Some(p) if p.is_dir() => Some(p.clone()),
            Some(p) => p.parent().map(Path::to_path_buf),
            None => self.root.clone(),
        }
    }

    /// Draws the open files and the folder tree.
    pub fn show(&mut self, ui: &mut Ui, cx: &ExplorerCtx<'_>) -> Vec<ExplorerAction> {
        let mut actions = vec![];
        let t = cx.theme;
        let ink = c32(t.text);
        let muted = c32(t.muted);

        // OPEN FILES
        if section_header(ui, "OPEN FILES", !self.files_collapsed, &t).clicked() {
            self.files_collapsed = !self.files_collapsed;
        }
        if !self.files_collapsed {
            for f in cx.open {
                let (rect, resp) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
                resp.widget_info(|| {
                    WidgetInfo::labeled(Role::Button, true, format!("open {}", f.title))
                });
                paint_row_bg(ui, rect, &resp, f.active, &t);
                let p = ui.painter_at(rect);
                let x = rect.left() + 10.0;
                let icon =
                    Rect::from_center_size(egui::pos2(x + 8.0, rect.center().y), Vec2::splat(16.0));
                icons::mpp_file(&p, icon, c32(t.link));
                let color = if f.broken { c32(t.error) } else { ink };
                let galley = text_galley(&p, &f.title, 15.0, color, f.preview);
                let text_pos = egui::pos2(x + 24.0, rect.center().y - galley.size().y / 2.0);
                // The name stops short of the count and the close button.
                let room = f
                    .path
                    .as_ref()
                    .map_or(0.0, |path| count_width(cx.counts_of(path)));
                let clip = Rect::from_min_max(
                    rect.min,
                    egui::pos2(rect.right() - 24.0 - room, rect.bottom()),
                );
                p.with_clip_rect(clip).galley(text_pos, galley, color);
                let close = Rect::from_center_size(
                    egui::pos2(rect.right() - 12.0, rect.center().y),
                    Vec2::splat(16.0),
                );
                if f.dirty {
                    icons::dot(&p, close, ink);
                } else if resp.hovered() {
                    icons::cross(&p, close, muted);
                }
                if let Some(path) = &f.path {
                    paint_count(
                        &p,
                        egui::pos2(rect.right() - 26.0, rect.center().y),
                        cx.counts_of(path),
                        &t,
                    );
                }
                let on_close = resp
                    .interact_pointer_pos()
                    .or_else(|| ui.input(|i| i.pointer.latest_pos()))
                    .is_some_and(|pos| close.contains(pos));
                if resp.clicked() && on_close {
                    actions.push(ExplorerAction::Close(f.index));
                } else if resp.clicked() {
                    actions.push(ExplorerAction::Activate(f.index));
                }
                if resp.clicked_by(PointerButton::Middle) {
                    actions.push(ExplorerAction::Close(f.index));
                }
            }
            if cx.open.is_empty() {
                ui.add_space(2.0);
                ui.weak("  No files open");
            }
        }
        ui.add_space(6.0);

        // PROJECT
        let Some(root) = self.root.clone() else {
            ui.add_space(8.0);
            ui.weak("  No folder open.");
            ui.weak("  File \u{2192} Open Folder");
            return actions;
        };
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_uppercase())
            .unwrap_or_else(|| "PROJECT".into());
        let head = section_header(ui, &name, !self.tree_collapsed, &t);
        if head.clicked() {
            self.tree_collapsed = !self.tree_collapsed;
        }
        if let Some(src) = head.dnd_release_payload::<PathBuf>() {
            drop_on(&src, &root, &mut actions);
        }
        head.context_menu(|ui| {
            self.folder_menu(ui, &root);
        });
        if self.tree_collapsed {
            return actions;
        }

        let rows = self.rows.clone();
        if self.create.as_ref().is_some_and(|c| c.dir == root) {
            self.create_row(ui, 0, &mut actions, &t);
        }
        for r in &rows {
            self.tree_row(ui, r, cx, &mut actions);
            if let Some(c) = &self.create {
                if r.is_dir && r.expanded && c.dir == r.path {
                    self.create_row(ui, r.depth + 1, &mut actions, &t);
                }
            }
        }
        if rows.is_empty() && self.create.is_none() {
            ui.weak("  This folder is empty.");
        }

        // Keys: F2 renames the selected item.
        if ui.input(|i| i.key_pressed(Key::F2)) && self.rename.is_none() {
            if let Some(s) = self.selected.clone() {
                self.start_rename(&s);
            }
        }
        self.delete_dialog(ui, &mut actions);
        actions
    }

    fn folder_menu(&mut self, ui: &mut Ui, dir: &Path) {
        if ui.button("New file").clicked() {
            self.start_create(dir, false);
            ui.close();
        }
        if ui.button("New folder").clicked() {
            self.start_create(dir, true);
            ui.close();
        }
        ui.separator();
        self.common_menu(ui, dir);
    }

    fn common_menu(&mut self, ui: &mut Ui, path: &Path) {
        if ui
            .add(egui::Button::new("Rename").shortcut_text("F2"))
            .clicked()
        {
            self.start_rename(path);
            ui.close();
        }
        if ui.button("Delete").clicked() {
            self.confirm_delete = Some(path.to_path_buf());
            ui.close();
        }
        ui.separator();
        if ui.button("Copy path").clicked() {
            ui.ctx().copy_text(path.to_string_lossy().into_owned());
            ui.close();
        }
        if ui.button("Reveal in File Explorer").clicked() {
            ops::reveal(path);
            ui.close();
        }
    }

    fn tree_row(
        &mut self,
        ui: &mut Ui,
        r: &Row,
        cx: &ExplorerCtx<'_>,
        actions: &mut Vec<ExplorerAction>,
    ) {
        let t = cx.theme;
        let (rect, resp) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), ROW_H),
            Sense::click_and_drag(),
        );
        resp.widget_info(|| WidgetInfo::labeled(Role::Button, true, r.name.clone()));
        let selected = self.selected.as_deref() == Some(r.path.as_path());
        let active = cx.current == Some(r.path.as_path());
        paint_row_bg(ui, rect, &resp, selected || active, &t);
        let p = ui.painter_at(rect);
        let mut x = rect.left() + 6.0 + r.depth as f32 * INDENT;
        let ink = c32(t.text);
        let muted = c32(t.muted);
        if r.is_dir {
            let a = Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(14.0, ROW_H));
            icons::arrow(&p, a, r.expanded, muted);
        }
        x += 16.0;
        let icon = Rect::from_center_size(egui::pos2(x + 8.0, rect.center().y), Vec2::splat(16.0));
        let broken = r.is_mpp() && self.has_broken_import(&r.path);
        let mut text_color = ink;
        if r.is_dir {
            icons::folder(&p, icon, muted);
        } else if r.is_mpp() {
            icons::mpp_file(&p, icon, c32(t.link));
        } else {
            icons::plain_file(&p, icon, muted.gamma_multiply(0.7));
            text_color = muted.gamma_multiply(0.85);
        }
        if broken {
            text_color = c32(t.error);
        }
        x += 22.0;

        let renaming = self.rename.as_ref().is_some_and(|(p, _)| p == &r.path);
        if renaming {
            let edit_rect = Rect::from_min_max(
                egui::pos2(x - 2.0, rect.top() + 2.0),
                egui::pos2(rect.right() - 4.0, rect.bottom() - 2.0),
            );
            let mut commit = None;
            if let Some((from, text)) = &mut self.rename {
                let out = ui.put(
                    edit_rect,
                    TextEdit::singleline(text).font(FontId::proportional(15.0)),
                );
                if self.focus_rename {
                    out.request_focus();
                    select_stem(ui, &out, text);
                    self.focus_rename = false;
                }
                if out.lost_focus() {
                    if ui.input(|i| i.key_pressed(Key::Enter)) {
                        commit = Some(Some((from.clone(), text.clone())));
                    } else {
                        commit = Some(None);
                    }
                }
            }
            if let Some(c) = commit {
                self.rename = None;
                if let Some((from, text)) = c {
                    self.finish_rename(&from, &text, actions);
                }
            }
        } else {
            let galley = text_galley(&p, &r.name, 15.0, text_color, false);
            let ty = rect.center().y - galley.size().y / 2.0;
            let w = galley.size().x;
            let room = if r.is_dir {
                0.0
            } else {
                count_width(cx.counts_of(&r.path))
            };
            let clip = Rect::from_min_max(
                rect.min,
                egui::pos2(rect.right() - 6.0 - room, rect.bottom()),
            );
            p.with_clip_rect(clip)
                .galley(egui::pos2(x, ty), galley, text_color);
            if cx.linked.iter().any(|l| l == &r.path) {
                let m = Rect::from_center_size(
                    egui::pos2(x + w + 12.0, rect.center().y),
                    Vec2::splat(16.0),
                );
                icons::link(&p, m, c32(t.link));
            }
            let counts = cx.counts_of(&r.path);
            if broken && counts.errors == 0 {
                p.text(
                    egui::pos2(rect.right() - 10.0, rect.center().y),
                    Align2::RIGHT_CENTER,
                    "!",
                    FontId::proportional(15.0),
                    c32(t.error),
                );
            } else if !r.is_dir {
                paint_count(
                    &p,
                    egui::pos2(rect.right() - 10.0, rect.center().y),
                    counts,
                    &t,
                );
            }
            if let Some(d) = cx
                .open
                .iter()
                .find(|o| o.dirty && o.title == tree_title(&r.path))
            {
                let _ = d;
                let m = Rect::from_center_size(
                    egui::pos2(rect.right() - 24.0, rect.center().y),
                    Vec2::splat(14.0),
                );
                icons::dot(&p, m, ink);
            }
        }

        // Interaction.
        if resp.drag_started() {
            resp.dnd_set_drag_payload(r.path.clone());
        }
        if r.is_dir {
            if let Some(src) = resp.dnd_release_payload::<PathBuf>() {
                drop_on(&src, &r.path, actions);
            }
            if resp.dnd_hover_payload::<PathBuf>().is_some() {
                ui.painter().rect_stroke(
                    rect,
                    2.0,
                    egui::Stroke::new(1.5, c32(t.link)),
                    egui::StrokeKind::Inside,
                );
            }
        }
        if resp.clicked() && !renaming {
            self.selected = Some(r.path.clone());
            if r.is_dir {
                self.toggle(&r.path);
            } else {
                actions.push(ExplorerAction::Open {
                    path: r.path.clone(),
                    preview: true,
                });
            }
        }
        if resp.double_clicked() && !r.is_dir {
            actions.push(ExplorerAction::Open {
                path: r.path.clone(),
                preview: false,
            });
        }
        if resp.secondary_clicked() {
            self.selected = Some(r.path.clone());
        }
        resp.context_menu(|ui| {
            if r.is_dir {
                self.folder_menu(ui, &r.path);
            } else {
                if r.is_mpp() && ui.button("Open").clicked() {
                    actions.push(ExplorerAction::Open {
                        path: r.path.clone(),
                        preview: false,
                    });
                    ui.close();
                }
                self.common_menu(ui, &r.path);
            }
        });
    }

    fn create_row(
        &mut self,
        ui: &mut Ui,
        depth: usize,
        actions: &mut Vec<ExplorerAction>,
        t: &Theme,
    ) {
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::hover());
        let p = ui.painter_at(rect);
        let x = rect.left() + 6.0 + depth as f32 * INDENT + 16.0;
        let icon = Rect::from_center_size(egui::pos2(x + 8.0, rect.center().y), Vec2::splat(16.0));
        let mut done = None;
        if let Some(c) = &mut self.create {
            if c.is_dir {
                icons::folder(&p, icon, c32(t.muted));
            } else {
                icons::mpp_file(&p, icon, c32(t.link));
            }
            let edit_rect = Rect::from_min_max(
                egui::pos2(x + 22.0, rect.top() + 2.0),
                egui::pos2(rect.right() - 4.0, rect.bottom() - 2.0),
            );
            let out = ui.put(
                edit_rect,
                TextEdit::singleline(&mut c.name)
                    .font(FontId::proportional(15.0))
                    .hint_text(if c.is_dir { "folder name" } else { "file name" }),
            );
            if self.focus_rename {
                out.request_focus();
                self.focus_rename = false;
            }
            if out.lost_focus() {
                done = Some(ui.input(|i| i.key_pressed(Key::Enter)));
            }
        }
        if let Some(enter) = done {
            if let Some(c) = self.create.take() {
                if enter {
                    let made = if c.is_dir {
                        ops::create_dir(&c.dir, &c.name)
                    } else {
                        ops::create_file(&c.dir, &c.name)
                    };
                    match made {
                        Ok(p) => {
                            self.refresh();
                            self.reveal(&p);
                            if !c.is_dir {
                                actions.push(ExplorerAction::Created(p));
                            }
                        }
                        Err(m) => actions.push(ExplorerAction::Message(m)),
                    }
                }
            }
        }
    }

    fn finish_rename(&mut self, from: &Path, text: &str, actions: &mut Vec<ExplorerAction>) {
        if let Err(m) = ops::valid_name(text) {
            actions.push(ExplorerAction::Message(m));
            return;
        }
        let mut name = text.trim().to_string();
        if from.is_file() && tree::is_mpp(from) && !name.to_lowercase().ends_with(".mpp") {
            name.push_str(".mpp");
        }
        let Some(dir) = from.parent() else {
            return;
        };
        let to = dir.join(name);
        if to != from {
            actions.push(ExplorerAction::Move {
                from: from.to_path_buf(),
                to,
            });
        }
    }

    fn delete_dialog(&mut self, ui: &mut Ui, actions: &mut Vec<ExplorerAction>) {
        let Some(path) = self.confirm_delete.clone() else {
            return;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut close = false;
        egui::Window::new("Delete")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ui.ctx(), |ui| {
                ui.label(format!("Move \u{201c}{name}\u{201d} to the recycle bin?"));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Delete").clicked() {
                        actions.push(ExplorerAction::Delete(path.clone()));
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            });
        if close {
            self.confirm_delete = None;
        }
    }
}

fn tree_title(p: &Path) -> String {
    super::display_name(p)
}

fn drop_on(src: &Path, folder: &Path, actions: &mut Vec<ExplorerAction>) {
    let Some(name) = src.file_name() else {
        return;
    };
    let to = folder.join(name);
    if src.parent() != Some(folder) && !folder.starts_with(src) {
        actions.push(ExplorerAction::Move {
            from: src.to_path_buf(),
            to,
        });
    }
}

fn select_stem(ui: &Ui, out: &Response, text: &str) {
    let stem_len = text.rfind('.').unwrap_or(text.len());
    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), out.id) {
        let range = egui::text::CCursorRange::two(
            egui::text::CCursor::new(0),
            egui::text::CCursor::new(text[..stem_len].chars().count()),
        );
        state.cursor.set_char_range(Some(range));
        state.store(ui.ctx(), out.id);
    }
}

fn paint_row_bg(ui: &Ui, rect: Rect, resp: &Response, selected: bool, t: &Theme) {
    let ink = c32(t.text);
    if selected {
        ui.painter()
            .rect_filled(rect, 0.0, c32(t.link).gamma_multiply(0.18));
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, ink.gamma_multiply(0.07));
    }
}

/// A small-caps section title with an arrow, like VS Code's.
pub fn section_header(ui: &mut Ui, title: &str, open: bool, t: &Theme) -> Response {
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 24.0),
        Sense::click_and_drag(),
    );
    resp.widget_info(|| WidgetInfo::labeled(Role::Button, true, title));
    let p = ui.painter_at(rect);
    if resp.hovered() {
        p.rect_filled(rect, 0.0, c32(t.text).gamma_multiply(0.06));
    }
    icons::arrow(
        &p,
        Rect::from_min_size(
            rect.min + Vec2::new(4.0, 0.0),
            Vec2::new(16.0, rect.height()),
        ),
        open,
        c32(t.muted),
    );
    p.text(
        egui::pos2(rect.left() + 22.0, rect.center().y),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(13.5),
        c32(t.text),
    );
    resp
}

/// True when any import in the file at `path` points at a missing file.
pub fn file_has_broken_import(path: &Path) -> bool {
    let Ok(text) = mpp_eval::read_text(path) else {
        return false;
    };
    has_broken_import(&text, path.parent().unwrap_or(Path::new(".")))
}

/// True when any import in `text` (a file in `dir`) points at a missing file.
pub fn has_broken_import(text: &str, dir: &Path) -> bool {
    links::targets(text, dir).iter().any(|t| !t.is_file())
}

/// Text laid out for painting; `italic` slants it (preview tabs).
pub fn text_galley(
    p: &egui::Painter,
    text: &str,
    size: f32,
    color: Color32,
    italic: bool,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: FontId::proportional(size),
            color,
            italics: italic,
            ..Default::default()
        },
    );
    job.wrap.max_width = f32::INFINITY;
    p.layout_job(job)
}
