//! The Problems panel (`Ctrl+Shift+M`): every problem in the project, grouped
//! by file, typeset, click to jump. It is a widget any host can show (a bottom
//! panel today, a tile of the tile manager later), plus the counts that tabs,
//! the explorer and the status bar show.

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
        if let Some(p) = &d.path {
            seen.insert(p.clone());
        }
        if !problems.is_empty() {
            out.push(FileProblems {
                title: d.title(),
                path: d.path.clone(),
                source: Source::Open(i),
                items: problems.iter().map(Item::of).collect(),
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
            rich::show(ui, &t, size, ink, *theme);
            ui.add_space(8.0);
            let c = total(list);
            rich::show(ui, &Rich::text(c.summary()), size * 0.85, muted, *theme);
        });
        ui.separator();
        if list.is_empty() {
            rich::show(
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
                        rich::show(ui, &Rich::text(arrow), size * 0.8, muted, *theme);
                        rich::show(
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
                                crate::typeset::show(ui, x, size, *theme);
                                ui.add_space(8.0);
                            }
                            rich::show(ui, &item.message, size * 0.9, ink, *theme);
                        });
                        let r = row.response.rect;
                        let hit =
                            ui.interact(r, ui.scope_id().with(("row", &key, i)), Sense::click());
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
        rich::show(ui, &Rich::text(n.to_string()), size, color, *theme);
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
