//! The Problems panel (`Ctrl+Shift+M`): every problem in the project, grouped
//! by file, typeset, click to jump. It is a widget any host can show (the
//! Problems tile of the tile manager), plus the counts that tabs, the
//! explorer and the status bar show.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use egui::{Color32, Sense, Ui, Vec2};
use mpp_render::Theme;
use mpp_syntax::Span;

use crate::diagnostics::rich::{self, Rich};
use crate::diagnostics::ui::paint_level_icon;
use crate::diagnostics::{Counts, Kind, Level, Problem};
use crate::doc::Doc;
use crate::paint::c32;

/// One problem in the list.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub span: Span,
    pub level: Level,
    pub message: Rich,
    /// The maths the problem is about, typeset beside the message (a name, a
    /// bracket). Never a line or column number.
    pub excerpt: Option<String>,
    /// The line it is on (from 1), when the file's text is at hand.
    pub line: Option<usize>,
}

impl Item {
    /// The list entry for a problem.
    pub fn of(p: &Problem) -> Item {
        let excerpt = match &p.kind {
            Kind::Undefined { name, .. } | Kind::Unused { name } => Some(name.clone()),
            _ => None,
        };
        Item {
            span: p.span,
            level: p.level,
            message: p.rich.clone(),
            excerpt,
            line: None,
        }
    }

    /// The entry for a problem of a file whose text is `text`, with its line.
    pub fn in_text(p: &Problem, text: &str) -> Item {
        let mut i = Item::of(p);
        let at = p.span.start.min(text.len());
        i.line = text
            .get(..at)
            .map(|before| before.matches('\n').count() + 1);
        i
    }

    /// What a screen reader says for the entry: "Error, line 3: w is not
    /// defined." (R12).
    pub fn spoken(&self) -> String {
        let level = match self.level {
            Level::Error => "Error",
            Level::Warning => "Warning",
            Level::Info => "Hint",
        };
        let message = self.message.plain();
        let message = message.trim();
        match self.line {
            Some(n) => format!("{level}, line {n}: {message}"),
            None => format!("{level}: {message}"),
        }
    }
}

/// Where a file in the list lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// An open tab.
    Open(usize),
    /// A file that is not open (it is imported by an open one).
    File(PathBuf),
}

/// The problems of one file.
#[derive(Debug, Clone, PartialEq)]
pub struct FileProblems {
    pub title: String,
    pub path: Option<PathBuf>,
    pub source: Source,
    pub items: Vec<Item>,
}

impl FileProblems {
    /// The error, warning and note counts of the file.
    pub fn counts(&self) -> Counts {
        let mut c = Counts::default();
        for i in &self.items {
            c.add(i.level, 1);
        }
        c
    }
}

/// Where a click in the list asks to go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Jump {
    /// An open tab, at this span.
    Open { tab: usize, span: Span },
    /// A file to open, at this span.
    File { path: PathBuf, span: Span },
}

/// The problems of every open file and of the files they import, grouped by
/// file. Files without problems are left out.
pub fn gather(docs: &mut [Doc]) -> Vec<FileProblems> {
    let mut out: Vec<FileProblems> = vec![];
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for (i, d) in docs.iter_mut().enumerate() {
        let problems = d.problems();
        let text = d.editor.text();
        if let Some(p) = &d.path {
            seen.insert(p.clone());
        }
        if !problems.is_empty() {
            out.push(FileProblems {
                title: d.title(),
                path: d.path.clone(),
                source: Source::Open(i),
                items: problems.iter().map(|p| Item::in_text(p, text)).collect(),
            });
        }
    }
    for d in docs.iter() {
        let Some(bg) = d.background() else { continue };
        for s in &bg.imports {
            if s.problems.is_empty() || !seen.insert(s.path.clone()) {
                continue;
            }
            out.push(FileProblems {
                title: crate::explorer::display_name(&s.path),
                path: Some(s.path.clone()),
                source: Source::File(s.path.clone()),
                items: s.problems.iter().map(Item::of).collect(),
            });
        }
    }
    out
}

/// The counts of every file with problems, as [`gather`] would give them, without
/// copying the problems (the explorer and the tabs ask every frame).
pub fn file_counts(docs: &mut [Doc]) -> Vec<(PathBuf, Counts)> {
    let mut out: Vec<(PathBuf, Counts)> = vec![];
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for d in docs.iter_mut() {
        let counts = d.problem_counts_now();
        if let Some(p) = &d.path {
            seen.insert(p.clone());
            if counts != Counts::default() {
                out.push((p.clone(), counts));
            }
        }
    }
    for d in docs.iter() {
        let Some(bg) = d.background() else { continue };
        for s in &bg.imports {
            if s.problems.is_empty() || !seen.insert(s.path.clone()) {
                continue;
            }
            out.push((s.path.clone(), Counts::of(&s.problems)));
        }
    }
    out
}

/// The counts of a file by path, from a gathered list (for tree and tab badges).
pub fn counts_for(list: &[FileProblems], path: &Path) -> Counts {
    list.iter()
        .filter(|f| f.path.as_deref() == Some(path))
        .map(FileProblems::counts)
        .fold(Counts::default(), Counts::plus)
}

/// The counts of the whole list.
pub fn total(list: &[FileProblems]) -> Counts {
    list.iter()
        .map(FileProblems::counts)
        .fold(Counts::default(), Counts::plus)
}

/// The Problems panel: open or closed, and which files are folded.
#[derive(Debug, Clone, Default)]
pub struct ProblemsView {
    pub open: bool,
    folded: HashSet<String>,
}

/// Height of the panel when it opens.
pub const PANEL_HEIGHT: f32 = 200.0;

impl ProblemsView {
    /// Shows or hides the panel (`Ctrl+Shift+M`).
    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    /// Draws the list into `ui`; returns where a click asks to go.
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        list: &[FileProblems],
        theme: &Theme,
        size: f32,
    ) -> Option<Jump> {
        let mut jump = None;
        let ink = rich::ink(theme);
        let muted = c32(theme.muted);
        ui.horizontal(|ui| {
            let t = Rich::default().i("Problems".to_string());
            words(ui, &t, size, ink, *theme);
            ui.add_space(8.0);
            let c = total(list);
            words(ui, &Rich::text(c.summary()), size * 0.85, muted, *theme);
        });
        ui.separator();
        if list.is_empty() {
            words(
                ui,
                &Rich::text("No problems in the open files."),
                size * 0.9,
                muted,
                *theme,
            );
            return None;
        }
        egui::ScrollArea::vertical()
            .id_salt("mpp-problems")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for f in list {
                    let key = f
                        .path
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_else(|| f.title.clone());
                    let folded = self.folded.contains(&key);
                    let head = ui.horizontal(|ui| {
                        let arrow = if folded { "\u{25b8}" } else { "\u{25be}" };
                        words(ui, &Rich::text(arrow), size * 0.8, muted, *theme);
                        words(
                            ui,
                            &Rich::default().i(f.title.clone()),
                            size * 0.95,
                            ink,
                            *theme,
                        );
                        ui.add_space(6.0);
                        badge(ui, f.counts(), theme, size * 0.8);
                    });
                    let hr = ui.interact(
                        head.response.rect,
                        ui.scope_id().with(("fold", &key)),
                        Sense::click(),
                    );
                    let c = f.counts();
                    let n = c.errors + c.warnings + c.infos;
                    let head_said = format!(
                        "{}, {}, {}",
                        f.title,
                        if n == 1 {
                            "1 item".to_string()
                        } else {
                            format!("{n} items")
                        },
                        if folded { "folded" } else { "open" }
                    );
                    hr.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Button, true, head_said.clone())
                    });
                    if hr.clicked() {
                        if folded {
                            self.folded.remove(&key);
                        } else {
                            self.folded.insert(key.clone());
                        }
                    }
                    if folded {
                        continue;
                    }
                    for (i, item) in f.items.iter().enumerate() {
                        let row = ui.horizontal(|ui| {
                            ui.add_space(size);
                            let (r, painter) =
                                ui.allocate_painter(Vec2::splat(size), Sense::hover());
                            paint_level_icon(
                                &painter,
                                r.rect.center(),
                                size * 0.32,
                                item.level,
                                theme,
                            );
                            ui.add_space(4.0);
                            if let Some(x) = &item.excerpt {
                                tex(ui, x, size, *theme);
                                ui.add_space(8.0);
                            }
                            words(ui, &item.message, size * 0.9, ink, *theme);
                        });
                        let r = row.response.rect;
                        let hit =
                            ui.interact(r, ui.scope_id().with(("row", &key, i)), Sense::click());
                        // A button a screen reader names; Enter goes there (R12).
                        let said = item.spoken();
                        hit.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::Role::Button, true, said.clone())
                        });
                        ui.ctx().accesskit_node_builder(hit.id, |n| {
                            n.set_description("Enter goes to it in the editor");
                        });
                        crate::access::live::note_label(ui.ctx(), hit.id, &said);
                        if hit.hovered() {
                            ui.painter().rect_stroke(
                                r.expand(1.0),
                                3.0,
                                egui::Stroke::new(1.0, muted.gamma_multiply(0.5)),
                                egui::StrokeKind::Outside,
                            );
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if hit.clicked() {
                            jump = Some(match &f.source {
                                Source::Open(tab) => Jump::Open {
                                    tab: *tab,
                                    span: item.span,
                                },
                                Source::File(path) => Jump::File {
                                    path: path.clone(),
                                    span: item.span,
                                },
                            });
                        }
                    }
                    ui.add_space(4.0);
                }
            });
        jump
    }
}

/// A message drawn as words, not as a widget: the keyboard and a screen
/// reader stop on the row (which names it), not on each piece of it (R12).
fn words(ui: &mut Ui, r: &Rich, size: f32, color: Color32, theme: Theme) {
    let max = ui.available_width().max(80.0);
    let s = rich::measure(r, size, theme, max);
    let (resp, painter) = ui.allocate_painter(s.max(Vec2::splat(1.0)), Sense::hover());
    rich::paint(&painter, r, resp.rect.min, size, color, theme, max);
}

/// Typeset maths drawn the same way (see [`words`]).
fn tex(ui: &mut Ui, latex: &str, size: f32, theme: Theme) {
    let dl = crate::typeset::layout(latex, size, theme);
    let (resp, painter) = ui.allocate_painter(
        Vec2::new(dl.width.max(1.0), dl.total_height().max(size)),
        Sense::hover(),
    );
    let dy = ((resp.rect.height() - dl.total_height()) / 2.0).max(0.0);
    crate::paint::paint_display_list(&painter, &dl, resp.rect.min + Vec2::new(0.0, dy));
}

/// Small coloured counts ("2 ✕ 1 !") beside a file name.
pub fn badge(ui: &mut Ui, c: Counts, theme: &Theme, size: f32) {
    for (n, level) in [
        (c.errors, Level::Error),
        (c.warnings, Level::Warning),
        (c.infos, Level::Info),
    ] {
        if n == 0 {
            continue;
        }
        let (r, painter) = ui.allocate_painter(Vec2::splat(size), Sense::hover());
        paint_level_icon(&painter, r.rect.center(), size * 0.36, level, theme);
        let color: Color32 = c32(level.color(theme));
        words(ui, &Rich::text(n.to_string()), size, color, *theme);
        ui.add_space(4.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_add_up_by_file() {
        let item = |level| Item {
            span: Span::point(0),
            level,
            message: Rich::text("x"),
            excerpt: None,
            line: None,
        };
        let list = vec![
            FileProblems {
                title: "a.mpp".into(),
                path: Some(PathBuf::from("a.mpp")),
                source: Source::Open(0),
                items: vec![item(Level::Error), item(Level::Error), item(Level::Warning)],
            },
            FileProblems {
                title: "b.mpp".into(),
                path: Some(PathBuf::from("b.mpp")),
                source: Source::File(PathBuf::from("b.mpp")),
                items: vec![item(Level::Info)],
            },
        ];
        assert_eq!(counts_for(&list, Path::new("a.mpp")).errors, 2);
        let t = total(&list);
        assert_eq!((t.errors, t.warnings, t.infos), (2, 1, 1));
        assert_eq!(t.summary(), "2 errors, 1 warning");
    }
}
