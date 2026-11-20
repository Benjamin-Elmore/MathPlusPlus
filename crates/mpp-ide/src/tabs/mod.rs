//! The tab strip above the page: one tab per open file.

/// What the user did in the strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabAction {
    Select(usize),
    Close(usize),
}

/// One tab to draw.
pub struct Tab {
    pub title: String,
    pub dirty: bool,
    pub errors: usize,
}

/// Draws the tabs; returns what was clicked.
pub fn show(
    ui: &mut egui::Ui,
    tabs: &[Tab],
    active: usize,
    error_color: egui::Color32,
) -> Option<TabAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        for (i, t) in tabs.iter().enumerate() {
            let mut title = t.title.clone();
            if t.dirty {
                title.push_str(" \u{25cf}");
            }
            let text = if t.errors > 0 {
                egui::RichText::new(title).color(error_color)
            } else {
                egui::RichText::new(title)
            };
            if ui.selectable_label(i == active, text).clicked() {
                action = Some(TabAction::Select(i));
            }
            if ui.small_button("\u{00d7}").clicked() {
                action = Some(TabAction::Close(i));
            }
            ui.add_space(6.0);
        }
    });
    action
}
