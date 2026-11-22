//! Drawing the workspace: activity bar, side bar, tabs and breadcrumbs, status
//! bar, quick open and the close-with-edits question.

use std::path::{Path, PathBuf};

use egui::{
    Align2, Context, FontId, Key, KeyboardShortcut, Modifiers, Rect, Role, Sense, TextEdit, Ui,
    Vec2, WidgetInfo,
};
use mpp_render::Theme;

use super::PendingClose;
use crate::app::IdeApp;
use crate::explorer::outline::{OutlineItem, OutlineKind};
use crate::explorer::panel::text_galley;
use crate::explorer::{self, icons, quick_open, ExplorerCtx, OpenFile};
use crate::paint::c32;
use crate::settings::SideView;
use crate::typeset;

/// Size of the status bar text (Latin Modern Sans).
const STATUS_SIZE: f32 = 14.0;
const TOGGLE_SIDE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::B);
const QUICK_OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::P);
const SEARCH_ALL: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::F);
const SHOW_EXPLORER: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::E);
const BACK: KeyboardShortcut = KeyboardShortcut::new(Modifiers::ALT, Key::ArrowLeft);
const FORWARD: KeyboardShortcut = KeyboardShortcut::new(Modifiers::ALT, Key::ArrowRight);
const NEXT_TAB: KeyboardShortcut = KeyboardShortcut::new(Modifiers::CTRL, Key::Tab);
const PREV_TAB: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::CTRL.plus(Modifiers::SHIFT), Key::Tab);
const CLOSE_TAB: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::W);
const FIND_USES: KeyboardShortcut = KeyboardShortcut::new(Modifiers::SHIFT, Key::F12);
const RENAME_KEY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F2);
const CHEAT_SHEET: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F1);

/// An icon painter for the activity bar.
type IconFn = fn(&egui::Painter, Rect, egui::Color32);

impl IdeApp {
    /// Keys that belong to the workspace. Run before the page sees input.
    pub(crate) fn workspace_shortcuts(&mut self, ctx: &Context) {
        let (side, quick, search, explorer, back, fwd, prev, next, close, uses, rename, cheat) =
            ctx.input_mut(|i| {
                (
                    i.consume_shortcut(&TOGGLE_SIDE),
                    i.consume_shortcut(&QUICK_OPEN),
                    i.consume_shortcut(&SEARCH_ALL),
                    i.consume_shortcut(&SHOW_EXPLORER),
                    i.consume_shortcut(&BACK),
                    i.consume_shortcut(&FORWARD),
                    i.consume_shortcut(&PREV_TAB),
                    i.consume_shortcut(&NEXT_TAB),
                    i.consume_shortcut(&CLOSE_TAB),
                    i.consume_shortcut(&FIND_USES),
                    i.consume_shortcut(&RENAME_KEY),
                    i.consume_shortcut(&CHEAT_SHEET),
                )
            });
        // Ctrl+Shift+Tab is consumed before Ctrl+Tab (a shortcut without Shift
        // also matches when Shift is held).
        if side {
            self.toggle_side();
        }
        if quick {
            self.ws.quick.open = true;
            self.ws.quick.query.clear();
            self.ws.quick.selected = 0;
        }
        if search {
            self.show_side(SideView::Search);
        }
        if explorer {
            self.show_side(SideView::Explorer);
            self.land_in(crate::layout::Pane::Explorer);
        }
        if cheat {
            self.show_cheat_sheet(ctx);
        }
        if back {
            self.nav_back();
        }
        if fwd {
            self.nav_forward();
        }
        if prev {
            self.cycle_tab(false);
        } else if next {
            self.cycle_tab(true);
        }
        if close {
            self.request_close(self.active);
        }
        if uses {
            if let Some(o) = self.doc().map(|d| d.editor.caret()) {
                self.find_uses_at(o);
            }
        }
        if rename {
            let target = self
                .ws
                .explorer
                .selected
                .clone()
                .or_else(|| self.doc().and_then(|d| d.path.clone()));
            if let Some(t) = target {
                self.show_side(SideView::Explorer);
                self.ws.explorer.selected = Some(t.clone());
                self.ws.explorer.start_rename(&t);
            }
        }
    }

    /// The activity bar and the side bar.
    pub(crate) fn side_ui(&mut self, ui: &mut Ui) {
        let theme = self.mode.theme();
        egui::Panel::left("activity")
            .exact_size(46.0)
            .resizable(false)
            .frame(
                egui::Frame::NONE
                    .fill(c32(theme.code_background))
                    .inner_margin(egui::Margin::symmetric(0, 6)),
            )
            .show(ui, |ui| self.activity_bar(ui, &theme));
    }

    fn activity_bar(&mut self, ui: &mut Ui, theme: &Theme) {
        // The only way to switch side panels: click an icon to show its panel,
        // click the lit icon to hide the side column.
        let items: [(SideView, &str, IconFn); 7] = [
            (
                SideView::Explorer,
                "Explorer (Ctrl+Shift+E)",
                icons::activity_explorer,
            ),
            (
                SideView::Search,
                "Search (Ctrl+Shift+F)",
                icons::activity_search,
            ),
            (
                SideView::Outline,
                "Outline: headings and blocks",
                icons::activity_outline,
            ),
            (
                SideView::Variables,
                "Definitions: names, values and imports",
                icons::activity_variables,
            ),
            (
                SideView::Problems,
                "Problems (Ctrl+Shift+M)",
                icons::activity_problems,
            ),
            (
                SideView::Tutorial,
                "Tutorial: the lessons",
                icons::activity_tutorial,
            ),
            (SideView::Help, "Cheat sheet (F1)", icons::activity_help),
        ];
        // Clicks first, painting after: every icon is drawn with the state the
        // click left, in the same frame.
        let laid: Vec<(Rect, egui::Response)> = items
            .iter()
            .map(|_| ui.allocate_exact_size(Vec2::new(46.0, 46.0), Sense::click()))
            .collect();
        for ((view, ..), (_, resp)) in items.iter().zip(&laid) {
            if resp.clicked() {
                self.activity_click(*view);
            }
        }
        for ((view, tip, draw), (rect, resp)) in items.into_iter().zip(laid) {
            let label = view.label();
            let active = self.ws.side_visible && self.ws.side_view == view;
            resp.widget_info(|| WidgetInfo::selected(Role::Button, true, active, label));
            let p = ui.painter_at(rect);
            let color = if active || resp.hovered() {
                c32(theme.text)
            } else {
                c32(theme.muted)
            };
            if active {
                p.line_segment(
                    [rect.left_top(), rect.left_bottom()],
                    egui::Stroke::new(2.5, c32(theme.link)),
                );
            }
            draw(&p, rect, color);
            resp.on_hover_text(tip);
        }
    }

    /// The outline as a tile.
    pub(crate) fn outline_tile(&mut self, ui: &mut Ui, theme: &Theme) {
        egui::ScrollArea::vertical()
            .id_salt("outline-view")
            .auto_shrink([false, false])
            .show(ui, |ui| self.outline_list(ui, theme));
    }

    /// The explorer's New file and New folder buttons: two icons at the right
    /// of `row` (the panel's header, or a row of their own).
    pub(crate) fn explorer_buttons(&mut self, ui: &mut Ui, row: Rect, theme: &Theme) {
        let items: [(&str, bool); 2] = [("New folder", true), ("New file", false)];
        for (i, (label, is_dir)) in items.into_iter().enumerate() {
            let r = Rect::from_center_size(
                egui::pos2(row.right() - 16.0 - 26.0 * i as f32, row.center().y),
                Vec2::splat(22.0),
            );
            let resp = ui.interact(
                r,
                ui.scope_id().with(("explorer-new", label)),
                Sense::click(),
            );
            resp.widget_info(|| WidgetInfo::labeled(Role::Button, true, label));
            let color = if resp.hovered() {
                c32(theme.text)
            } else {
                c32(theme.muted)
            };
            if resp.hovered() {
                ui.painter()
                    .rect_filled(r, 3.0, c32(theme.text).gamma_multiply(0.08));
            }
            let p = ui.painter_at(r);
            let icon = r.shrink(3.0).translate(Vec2::new(-1.5, 1.0));
            if is_dir {
                icons::folder(&p, icon, color);
            } else {
                icons::plain_file(&p, icon, color);
            }
            icons::plus(&p, r, color);
            if resp.on_hover_text(label).clicked() {
                if let Some(d) = self.ws.explorer.target_dir() {
                    self.ws.explorer.start_create(&d, is_dir);
                }
            }
        }
    }

    /// The Explorer: files and folders only (open files, then the folder tree).
    /// `headed` is true when the side header above already holds the buttons.
    pub(crate) fn explorer_view(&mut self, ui: &mut Ui, theme: &Theme, headed: bool) {
        if !headed {
            let (row, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
            self.explorer_buttons(ui, row, theme);
        }
        let open: Vec<OpenFile> = self
            .docs
            .iter()
            .enumerate()
            .map(|(i, d)| OpenFile {
                index: i,
                title: d.title(),
                dirty: d.is_dirty(),
                active: i == self.active,
                preview: self.is_preview(d),
                broken: d.link_targets().iter().any(|(_, exists)| !exists),
                path: d.path.clone(),
            })
            .collect();
        let linked = self.current_imports();
        let current = self.doc().and_then(|d| d.path.clone());
        let counts = self.file_counts();
        let cx = ExplorerCtx {
            theme: *theme,
            open: &open,
            current: current.as_deref(),
            linked: &linked,
            counts: &counts,
        };
        let mut actions = vec![];
        egui::ScrollArea::vertical()
            .id_salt("explorer-view")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                actions = self.ws.explorer.show(ui, &cx);
            });
        self.run_explorer_actions(actions);
    }

    /// The outline of the current file, typeset; clicking scrolls to the entry.
    pub fn outline_items(&mut self) -> Vec<OutlineItem> {
        let Some(d) = self.docs.get(self.active) else {
            return vec![];
        };
        let notes = self.show_comments;
        let key = (
            d.id,
            ((d.editor.revision() * 2 + u64::from(d.eval_fresh())) * 2 + u64::from(notes))
                .wrapping_mul(1_000_003)
                .wrapping_add(d.proof_generation()),
        );
        if let Some((id, k, items)) = &self.ws.outline_cache {
            if (*id, *k) == key {
                return items.clone();
            }
        }
        let src = d.editor.text();
        let parsed = d.parsed();
        let proof = d.proof_view();
        let items = crate::explorer::outline::outline_full(
            &parsed,
            src,
            d.eval.as_deref(),
            notes,
            Some(&proof),
        );
        self.ws.outline_cache = Some((key.0, key.1, items.clone()));
        items
    }

    fn outline_list(&mut self, ui: &mut Ui, theme: &Theme) {
        let items = self.outline_items();
        if items.is_empty() {
            ui.weak("  Nothing to list yet.");
        }
        let mut go = None;
        for it in &items {
            let level = match it.kind {
                OutlineKind::Heading(l) => usize::from(l.saturating_sub(1)),
                OutlineKind::Title | OutlineKind::References => 0,
                _ => 2,
            };
            ui.horizontal(|ui| {
                ui.add_space(12.0 + level as f32 * 12.0);
                let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                let p = ui.painter();
                let c = match it.kind {
                    OutlineKind::Heading(_) => c32(theme.text),
                    OutlineKind::Definition => c32(theme.link),
                    OutlineKind::Figure => c32(theme.warning),
                    OutlineKind::Title | OutlineKind::References => c32(theme.text),
                    OutlineKind::Theorem => c32(theme.result),
                    OutlineKind::Note => c32(theme.note_accent),
                };
                match it.kind {
                    OutlineKind::Heading(_) => {
                        p.line_segment(
                            [r.left_center(), r.right_center()],
                            egui::Stroke::new(2.0, c),
                        );
                    }
                    OutlineKind::Definition => {
                        p.circle_filled(r.center(), 3.0, c);
                    }
                    OutlineKind::Title => {
                        p.line_segment(
                            [r.left_center(), r.right_center()],
                            egui::Stroke::new(3.0, c),
                        );
                    }
                    OutlineKind::Theorem | OutlineKind::References => {
                        p.circle_stroke(r.center(), 3.5, egui::Stroke::new(1.3, c));
                    }
                    OutlineKind::Note => {
                        p.rect_filled(r.shrink(2.0), 1.0, c);
                    }
                    OutlineKind::Figure => {
                        p.rect_stroke(
                            r.shrink(1.5),
                            1.0,
                            egui::Stroke::new(1.3, c),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
                let size = match it.kind {
                    OutlineKind::Heading(1) | OutlineKind::Title => 18.0,
                    _ => 16.0,
                };
                let mut resp = typeset::show(ui, &it.latex, size, *theme);
                if let Some((text, tone)) = &it.status {
                    let tint = Theme {
                        text: tone.color(theme),
                        ..*theme
                    };
                    let latex = crate::proof::prose_latex(text);
                    let r = typeset::show(ui, &latex, size * 0.8, tint);
                    let label = text.clone();
                    r.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Label, true, label.clone())
                    });
                    resp = resp.union(r);
                }
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    ui.painter().rect_filled(
                        resp.rect.expand2(Vec2::new(4.0, 1.0)),
                        2.0,
                        c32(theme.text).gamma_multiply(0.07),
                    );
                }
                if resp.clicked() {
                    go = Some(it.span);
                }
            });
            ui.add_space(2.0);
        }
        if let Some(span) = go {
            self.note_place();
            if let Some(d) = self.doc_mut() {
                d.editor.move_caret(span.start, false);
                d.scroll_to = Some(span);
            }
            self.note_place();
        }
    }

    pub(crate) fn search_view(&mut self, ui: &mut Ui, theme: &Theme) {
        ui.add_space(4.0);
        let mut run = None;
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            let w = ui.available_width() - 10.0;
            let r = ui.add(
                TextEdit::singleline(&mut self.ws.search.query)
                    .hint_text("Search words or a symbol such as \u{222b}")
                    .desired_width(w),
            );
            if self.ws.search.focus {
                r.request_focus();
                self.ws.search.focus = false;
            }
            if r.changed() || (r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))) {
                run = Some(self.ws.search.query.clone());
            }
        });
        if let Some(q) = run {
            self.run_search(&q);
        }
        if let Some(name) = self.ws.search.uses_of.clone() {
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.label(egui::RichText::new("Uses of").color(c32(theme.muted)));
                let latex = defs_name_latex(&name);
                typeset::show(ui, &latex, 17.0, *theme);
            });
        }
        ui.add_space(4.0);
        let hits = self.ws.search.hits.clone();
        if hits.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.weak(if self.ws.search.last_run.trim().is_empty() {
                    "Type to search every file."
                } else {
                    "No results."
                });
            });
            let q = self.ws.search.last_run.trim().to_string();
            if !q.is_empty() {
                let known: Vec<String> = self
                    .doc()
                    .and_then(|d| d.eval.as_ref())
                    .map(|e| e.user_names())
                    .unwrap_or_default();
                let guesses = crate::suggest::Suggester.names(&q, &known);
                if !guesses.is_empty() {
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        ui.label(egui::RichText::new("Did you mean").color(c32(theme.muted)));
                        for g in &guesses {
                            let r = typeset::show(ui, &defs_name_latex(g), 17.0, *theme);
                            if r.clicked() {
                                self.ws.search.query = g.clone();
                                let g = g.clone();
                                self.run_search(&g);
                            }
                        }
                    });
                }
            }
            return;
        }
        let root = self.project_root().map(Path::to_path_buf);
        let mut open = None;
        egui::ScrollArea::vertical()
            .id_salt("search-results")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut last: Option<&PathBuf> = None;
                for h in &hits {
                    if last != Some(&h.file) {
                        let count = hits.iter().filter(|x| x.file == h.file).count();
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.add_space(10.0);
                            let rel = root
                                .as_deref()
                                .and_then(|r| h.file.strip_prefix(r).ok())
                                .unwrap_or(&h.file);
                            ui.label(
                                egui::RichText::new(explorer::search::file_label(rel))
                                    .strong()
                                    .size(14.0),
                            );
                            ui.label(
                                egui::RichText::new(count.to_string()).color(c32(theme.muted)),
                            );
                        });
                        last = Some(&h.file);
                    }
                    ui.horizontal(|ui| {
                        ui.add_space(22.0);
                        let r = typeset::show(ui, &h.latex, 16.0, *theme);
                        if r.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            ui.painter().rect_filled(
                                r.rect.expand(2.0),
                                2.0,
                                c32(theme.text).gamma_multiply(0.07),
                            );
                        }
                        if r.clicked() {
                            open = Some(h.clone());
                        }
                    });
                    ui.add_space(3.0);
                }
            });
        if let Some(h) = open {
            self.open_hit(&h);
        }
    }

    /// The status bar: imports, definitions, problem counts, evaluation, theme.
    pub(crate) fn status_ui(&mut self, ui: &mut Ui) {
        let theme = self.mode.theme();
        let imports = self.current_imports();
        // Calm numbers: they change when a run finishes, and "Working…" shows
        // only for a run that takes more than a second.
        let id = self.doc().map(|d| d.id);
        let running = self.doc().is_some_and(crate::doc::Doc::running);
        let settled = self.doc().is_some_and(|d| !d.evaluating());
        let doc = self.docs.get_mut(self.active);
        let shown = self
            .ws
            .status
            .update(id, running, settled, std::time::Instant::now(), || {
                let Some(d) = doc else {
                    return Default::default();
                };
                let counts = if settled {
                    d.problem_counts_now()
                } else {
                    d.problem_counts()
                };
                let defs = d.eval.as_deref().map_or(0, super::status::definitions);
                (counts, defs)
            });
        if let Some(w) = shown.wake {
            ui.ctx().request_repaint_after(w);
        }
        let (ndefs, evaluating, counts) = (shown.defs, shown.working, shown.counts);
        let mut open_import = None;
        let mut show_outline = false;
        let mut show_problems = false;
        let mut cycle_theme = false;
        let theme_label = self.theme_choice.label();
        // Prose or Maths, and the path to the caret inside maths (U13).
        let place = self.doc().map(crate::page::caret_path::status_text);
        let mut flip_line = false;
        let mut place_id = None;
        let bar = egui::Panel::bottom("status")
            .frame(
                egui::Frame::NONE
                    .fill(c32(theme.code_background))
                    .inner_margin(egui::Margin::symmetric(10, 3)),
            )
            .show(ui, |ui| {
                // The status bar is set in Latin Modern Sans (it reads better small).
                ui.style_mut().override_font_id = Some(crate::theme::sans(STATUS_SIZE));
                ui.horizontal(|ui| {
                    let (label, color) = (
                        plural(imports.len(), "import", "imports"),
                        if imports.iter().any(|p| !p.is_file()) {
                            c32(theme.error)
                        } else {
                            c32(theme.text)
                        },
                    );
                    ui.menu_button(egui::RichText::new(label).color(color), |ui| {
                        if imports.is_empty() {
                            ui.weak("This file imports nothing.");
                        }
                        for p in &imports {
                            let name = p
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            let t = if p.is_file() {
                                egui::RichText::new(name)
                            } else {
                                egui::RichText::new(format!("{name} (missing)"))
                                    .color(c32(theme.error))
                            };
                            if ui.button(t).clicked() {
                                open_import = Some(p.clone());
                                ui.close();
                            }
                        }
                    });
                    ui.label(egui::RichText::new("\u{00b7}").color(c32(theme.muted)));
                    let defs = ui.add(
                        egui::Label::new(plural(ndefs, "definition", "definitions"))
                            .sense(Sense::click()),
                    );
                    // A button that opens the Outline, for a screen reader too.
                    crate::access::live::as_button(ui, &defs, "Opens the Outline");
                    if defs.clicked() {
                        show_outline = true;
                    }
                    ui.label(egui::RichText::new("\u{00b7}").color(c32(theme.muted)));
                    if evaluating {
                        ui.spinner();
                        ui.label("Working\u{2026}");
                    } else {
                        let color = match counts.worst() {
                            Some(crate::diagnostics::Level::Error) => c32(theme.error),
                            Some(crate::diagnostics::Level::Warning) => c32(theme.warning),
                            _ => c32(theme.text),
                        };
                        // The count is for the file on screen; the Problems tile
                        // lists the whole project, so the bar says which it counts.
                        let text = if counts.problems() == 0 {
                            "No problems".to_string()
                        } else {
                            format!("{} in this file", counts.summary())
                        };
                        // Clicking the count opens the Problems tile, like Ctrl+Shift+M.
                        let count = ui
                            .add(
                                egui::Label::new(egui::RichText::new(text).color(color))
                                    .sense(Sense::click()),
                            )
                            .on_hover_text("Problems (Ctrl+Shift+M)");
                        crate::access::live::as_button(ui, &count, "Opens Problems");
                        if count.clicked() {
                            show_problems = true;
                        }
                    }
                    if let Some(m) = &self.message {
                        ui.separator();
                        ui.colored_label(c32(theme.error), m);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let tip = format!("Theme: {theme_label} (Ctrl+K, then Ctrl+T, switches)");
                        if crate::theme::moon_button(ui, STATUS_SIZE)
                            .on_hover_text(tip)
                            .clicked()
                        {
                            cycle_theme = true;
                        }
                        ui.label(theme_label);
                        if let Some(place) = &place {
                            ui.separator();
                            let r = ui
                                .add(egui::Label::new(place.as_str()).sense(Sense::click()))
                                .on_hover_text("Ctrl+M switches this line between prose and maths");
                            place_id = Some(r.id);
                            flip_line = r.clicked();
                        }
                    });
                });
            });
        self.access.status_rect = Some(bar.response.rect);
        self.access.status_place = place_id.zip(place);
        if show_problems {
            self.open_pane(crate::layout::Pane::Problems);
        }
        if flip_line {
            if let Some(d) = self.doc_mut() {
                let at = d.editor.caret();
                if d.editor.flip_maths(&mut d.maths, at) {
                    d.touched();
                }
            }
        }
        if cycle_theme {
            self.set_theme_choice(self.theme_choice.next());
        }
        if let Some(p) = open_import {
            if p.is_file() {
                if let Err(m) = self.open_file(&p, false) {
                    self.message = Some(m);
                }
            }
        }
        if show_outline {
            self.show_side(SideView::Outline);
        }
    }

    /// Ctrl+P and the close-with-unsaved-edits question.
    pub(crate) fn overlays(&mut self, ctx: &Context) {
        self.quick_open_ui(ctx);
        self.layout_name_dialog(ctx);
        self.recovery_dialog(ctx);
        self.close_dialog(ctx);
    }

    fn quick_open_ui(&mut self, ctx: &Context) {
        if !self.ws.quick.open {
            return;
        }
        let theme = self.mode.theme();
        let root = self.project_root().map(Path::to_path_buf);
        let mut files = self.ws.explorer.all_files().to_vec();
        for d in &self.docs {
            if let Some(p) = &d.path {
                if !files.contains(p) {
                    files.push(p.clone());
                }
            }
        }
        let base = root.clone().unwrap_or_default();
        let ranked = quick_open::rank(&base, &files, &self.ws.quick.query, 10);
        let n = ranked.len();
        let (down, up, enter, esc) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if down && n > 0 {
            self.ws.quick.selected = (self.ws.quick.selected + 1) % n;
        }
        if up && n > 0 {
            self.ws.quick.selected = (self.ws.quick.selected + n - 1) % n;
        }
        self.ws.quick.selected = self.ws.quick.selected.min(n.saturating_sub(1));
        let mut chosen = None;
        egui::Area::new(egui::Id::unique("quick-open"))
            .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 60.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .fill(c32(theme.background))
                    .show(ui, |ui| {
                        ui.set_width(460.0);
                        let r = ui.add(
                            TextEdit::singleline(&mut self.ws.quick.query)
                                .hint_text("Go to file")
                                .desired_width(f32::INFINITY),
                        );
                        r.request_focus();
                        if r.changed() {
                            self.ws.quick.selected = 0;
                        }
                        ui.add_space(4.0);
                        for (i, p) in ranked.iter().enumerate() {
                            let rel = p.strip_prefix(&base).unwrap_or(p);
                            let name = explorer::display_name(p);
                            let dir = rel
                                .parent()
                                .map(|d| d.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            let (rect, resp) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 26.0),
                                Sense::click(),
                            );
                            resp.widget_info(|| {
                                WidgetInfo::labeled(Role::Button, true, format!("go to {name}"))
                            });
                            let sel = i == self.ws.quick.selected;
                            if sel || resp.hovered() {
                                ui.painter().rect_filled(
                                    rect,
                                    3.0,
                                    c32(theme.link).gamma_multiply(if sel { 0.22 } else { 0.1 }),
                                );
                            }
                            let pnt = ui.painter_at(rect);
                            icons::mpp_file(
                                &pnt,
                                Rect::from_center_size(
                                    egui::pos2(rect.left() + 14.0, rect.center().y),
                                    Vec2::splat(16.0),
                                ),
                                c32(theme.link),
                            );
                            let g = text_galley(&pnt, &name, 15.5, c32(theme.text), false);
                            let w = g.size().x;
                            pnt.galley(
                                egui::pos2(rect.left() + 30.0, rect.center().y - g.size().y / 2.0),
                                g,
                                c32(theme.text),
                            );
                            if !dir.is_empty() {
                                pnt.text(
                                    egui::pos2(rect.left() + 40.0 + w, rect.center().y),
                                    Align2::LEFT_CENTER,
                                    dir,
                                    FontId::proportional(13.5),
                                    c32(theme.muted),
                                );
                            }
                            if resp.clicked() {
                                chosen = Some(p.clone());
                            }
                        }
                        if ranked.is_empty() {
                            ui.weak("No matching files.");
                        }
                    });
            });
        if enter {
            chosen = ranked.get(self.ws.quick.selected).cloned();
        }
        if esc {
            self.ws.quick.open = false;
        }
        if let Some(p) = chosen {
            self.ws.quick.open = false;
            if let Err(m) = self.open_file(&p, false) {
                self.message = Some(m);
            }
        }
    }

    fn close_dialog(&mut self, ctx: &Context) {
        let Some(PendingClose(i)) = self.ws.pending_close else {
            return;
        };
        let Some(title) = self.docs.get(i).map(crate::doc::Doc::title) else {
            self.ws.pending_close = None;
            return;
        };
        let mut answer = None;
        egui::Window::new("Unsaved changes")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(format!("Save changes to {title} before closing?"));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Don\u{2019}t save").clicked() {
                        answer = Some(false);
                    }
                    if ui.button("Cancel").clicked() {
                        self.ws.pending_close = None;
                    }
                });
            });
        match answer {
            Some(true) => {
                let was = self.active;
                self.active = i;
                self.save();
                self.active = was;
                let saved = self.docs.get(i).is_some_and(|d| !d.is_dirty());
                self.ws.pending_close = None;
                if saved {
                    self.close_doc(i);
                }
            }
            Some(false) => {
                self.ws.pending_close = None;
                self.close_doc(i);
            }
            None => {}
        }
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

fn defs_name_latex(name: &str) -> String {
    mpp_syntax::symbols::lookup_name(name)
        .filter(|s| s.unicode == name)
        .map(|s| s.latex.to_string())
        .unwrap_or_else(|| name.to_string())
}
