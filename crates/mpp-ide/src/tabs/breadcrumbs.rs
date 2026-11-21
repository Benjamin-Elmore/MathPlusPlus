//! The folder path above the page: `project > physics > mechanics.mpp`.

use std::path::{Path, PathBuf};

use egui::{Sense, Ui};
use mpp_render::Theme;

use crate::paint::c32;

/// The parts of a path below `root`, each with the path it stands for.
/// The project folder comes first.
pub fn parts(root: &Path, file: &Path) -> Vec<(String, PathBuf)> {
    let mut out = vec![(
        root.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.display().to_string()),
        root.to_path_buf(),
    )];
    if let Ok(rel) = file.strip_prefix(root) {
        let mut acc = root.to_path_buf();
        for c in rel.components() {
            acc.push(c);
            out.push((c.as_os_str().to_string_lossy().into_owned(), acc.clone()));
        }
    }
    out
}

/// Draws the crumbs; returns the one clicked.
pub fn show(ui: &mut Ui, root: &Path, file: &Path, theme: &Theme) -> Option<PathBuf> {
    let mut clicked = None;
    let list = parts(root, file);
    let last = list.len() - 1;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.add_space(10.0);
        for (i, (name, path)) in list.iter().enumerate() {
            let color = if i == last {
                c32(theme.text)
            } else {
                c32(theme.muted)
            };
            let r = ui.add(
                egui::Label::new(egui::RichText::new(name).color(color).size(14.0))
                    .sense(Sense::click()),
            );
            if r.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if r.clicked() {
                clicked = Some(path.clone());
            }
            if i != last {
                ui.label(egui::RichText::new("\u{203a}").color(c32(theme.muted)));
            }
        }
    });
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_start_at_the_project_folder() {
        let p = parts(
            Path::new("/work/my-project"),
            Path::new("/work/my-project/physics/mechanics.mpp"),
        );
        let names: Vec<&str> = p.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["my-project", "physics", "mechanics.mpp"]);
        assert_eq!(p[1].1, PathBuf::from("/work/my-project/physics"));
    }
}
