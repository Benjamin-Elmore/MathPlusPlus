//! The Steps tile (the working of the line under the caret), the Standard
//! library tile (every formula of `std/`, typeset, with insert and import) and
//! the Tutorial tile (the lessons, one click each).

use egui::{RichText, Ui};
use mpp_render::Theme;

use crate::app::IdeApp;
use crate::doc::Doc;
use crate::library::{self, LibEntry};
use crate::paint::c32;
use crate::typeset;

/// What the reader asked of a library entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibAction {
    Insert,
    Import,
}

impl IdeApp {
    /// The Tutorial: every lesson, a click opens it (the lesson on screen is lit).
    pub(crate) fn tutorial_tile(&mut self, ui: &mut Ui, theme: &Theme) {
        let dir = crate::tutorial::tutorial_dir(self.ws.settings_dir.as_deref());
        let current = self.doc().and_then(|d| d.path.clone());
        let mut open = None;
        let mut whole = false;
        egui::ScrollArea::vertical()
            .id_salt("tutorial-tile")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(4.0);
                for (name, _) in crate::tutorial::lessons::FILES {
                    let title = name.trim_end_matches(".mpp");
                    let lit = current.as_deref() == Some(dir.join(name).as_path());
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        let text = if lit {
                            RichText::new(title).color(c32(theme.link))
                        } else {
                            RichText::new(title)
                        };
                        let r = ui.add(egui::Button::selectable(lit, text));
                        if r.clicked() {
                            open = Some(name);
                        }
                    });
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    if ui
                        .button("Open the tutorial folder")
                        .on_hover_text("Shows the lessons in the Explorer")
                        .clicked()
                    {
                        whole = true;
                    }
                });
            });
        if whole {
            self.open_tutorial(Some(ui.ctx()));
        } else if let Some(name) = open {
            let made = crate::tutorial::install(&dir)
                .map_err(|e| format!("Could not set up the tutorial: {e}"))
                .and_then(|d| self.open_file(&d.join(name), false));
            if let Err(m) = made {
                self.message = Some(m);
            }
            ui.ctx().request_repaint();
        }
    }
}

fn heading(ui: &mut Ui, theme: &Theme, title: &str) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.label(
            RichText::new(title.to_uppercase())
                .size(13.5)
                .color(c32(theme.muted)),
        );
    });
    ui.add_space(6.0);
}

/// Inserts a library entry into a document: in the maths at the caret, or as a
/// new display block when the caret is in prose.
pub fn insert_entry(doc: &mut Doc, e: &LibEntry) {
    let caret = doc.editor.caret();
    let text = e.insert_text();
    let in_math = doc.page_cache().is_some_and(|pc| pc.page.in_math(caret));
    let snippet_src = text.replace("{}", "{\u{27e8}*value\u{27e9}}");
    let snippet = crate::editor::template::parse_snippet(&snippet_src);
    let (before, after) = if in_math {
        ("", "")
    } else {
        ("$$ ", " $$\n\n")
    };
    match snippet {
        Ok(s) if text.contains("{}") => {
            doc.editor.insert_template(
                caret..caret,
                before,
                &s,
                after,
                "library",
                crate::editor::template::Knobs::default(),
            );
        }
        _ => {
            doc.editor.move_caret(caret, false);
            doc.editor.insert_text(&format!("{before}{text}{after}"));
        }
    }
    doc.touched();
}

/// Imports the file of a library entry (a `Using … from [[std/…]].` sentence, or
/// the name added to an existing one).
pub fn import_entry(doc: &mut Doc, e: &LibEntry) {
    let Some(key) = &e.key else {
        let line = format!("![[{}]]\n\n", e.entry.file);
        doc.editor.replace_node(mpp_syntax::Span::point(0), &line);
        doc.touched();
        return;
    };
    let src = doc.editor.text().to_string();
    let parsed = mpp_syntax::document::parse_document(&src);
    let _ = key;
    let latex = e.head_latex();
    let (span, with) = crate::diagnostics::fixes::import_edit(&src, &parsed, &latex, e.entry.file);
    doc.editor.replace_node(span, &with);
    doc.touched();
}

impl IdeApp {
    /// The working of the line under the caret, typeset and unfolded.
    pub(crate) fn steps_tile(&mut self, ui: &mut Ui, theme: &Theme) {
        heading(ui, theme, "Steps");
        let size = self.size * 0.9;
        let tree = self.doc().and_then(|d| {
            let (block, i) = d.line_at(d.editor.caret())?;
            let v = d.eval.as_ref()?.block(block)?.values.get(i)?;
            v.step_tree().filter(|t| !t.roots.is_empty())
        });
        let Some(tree) = tree else {
            ui.horizontal_wrapped(|ui| {
                ui.add_space(12.0);
                ui.label(
                    RichText::new(
                        "Put the cursor on a line that asks for its working (= ?!), or turn on View, Show steps.",
                    )
                    .color(c32(theme.muted)),
                );
            });
            return;
        };
        let muted = Theme {
            text: theme.muted,
            ..*theme
        };
        egui::ScrollArea::vertical()
            .id_salt("steps-tile")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut stack: Vec<(usize, &mpp_core::step_tree::StepNode)> =
                    tree.roots.iter().rev().map(|n| (0, n)).collect();
                while let Some((depth, n)) = stack.pop() {
                    ui.horizontal(|ui| {
                        ui.add_space(12.0 + depth as f32 * 16.0);
                        let rule = library::plain(&n.rule);
                        typeset::show(ui, &format!("\\text{{{rule}}}"), size * 0.85, muted);
                    });
                    ui.horizontal(|ui| {
                        ui.add_space(20.0 + depth as f32 * 16.0);
                        typeset::show(ui, &n.latex_line(), size, *theme);
                    });
                    ui.add_space(4.0);
                    for c in n.children.iter().rev() {
                        stack.push((depth + 1, c));
                    }
                }
            });
    }

    /// Every formula of the standard library, by file, typeset, with a filter.
    pub(crate) fn library_tile(&mut self, ui: &mut Ui, theme: &Theme) {
        heading(ui, theme, "Standard library");
        let filter_id = egui::Id::unique("library-filter");
        let mut filter: String = ui.data(|d| d.get_temp(filter_id)).unwrap_or_default();
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            ui.add(
                egui::TextEdit::singleline(&mut filter)
                    .hint_text("Filter by name or file")
                    .desired_width(ui.available_width() - 14.0),
            );
        });
        ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
        let needle = filter.to_lowercase();
        let size = self.size * 0.85;
        let muted = Theme {
            text: theme.muted,
            ..*theme
        };
        let mut chosen: Option<(usize, LibAction)> = None;
        egui::ScrollArea::vertical()
            .id_salt("library-tile")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let entries = library::entries();
                let mut files: Vec<&str> = entries.iter().map(|e| e.file.as_str()).collect();
                files.dedup();
                for file in files {
                    let shown: Vec<(usize, &LibEntry)> = entries
                        .iter()
                        .enumerate()
                        .filter(|(_, e)| e.file == file)
                        .filter(|(_, e)| {
                            needle.is_empty()
                                || e.file.contains(&needle)
                                || e.key
                                    .as_deref()
                                    .is_some_and(|k| k.to_lowercase().contains(&needle))
                                || e.entry.description.to_lowercase().contains(&needle)
                        })
                        .collect();
                    if shown.is_empty() {
                        continue;
                    }
                    egui::CollapsingHeader::new(RichText::new(file).size(14.0))
                        .id_salt(("lib-file", file))
                        .default_open(!needle.is_empty() || file == "constants")
                        .show(ui, |ui| {
                            for (i, e) in shown.into_iter().take(80) {
                                ui.horizontal(|ui| {
                                    typeset::show(ui, &e.latex(), size, *theme);
                                });
                                let desc = e.description_latex();
                                if !desc.is_empty() {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.add_space(8.0);
                                        typeset::show(ui, &desc, size * 0.8, muted);
                                    });
                                }
                                ui.horizontal(|ui| {
                                    ui.add_space(8.0);
                                    if ui.small_button("Insert").clicked() {
                                        chosen = Some((i, LibAction::Insert));
                                    }
                                    if e.key.is_some() && ui.small_button("Import").clicked() {
                                        chosen = Some((i, LibAction::Import));
                                    }
                                });
                                ui.add_space(6.0);
                            }
                        });
                }
            });
        if let Some((i, action)) = chosen {
            self.library_action(i, action);
        }
    }

    /// Inserts or imports library entry `i` into the active file.
    pub fn library_action(&mut self, i: usize, action: LibAction) {
        let Some(e) = library::entries().get(i) else {
            return;
        };
        let Some(doc) = self.doc_mut() else {
            return;
        };
        match action {
            LibAction::Insert => insert_entry(doc, e),
            LibAction::Import => import_entry(doc, e),
        }
    }
}
