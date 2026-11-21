//! Small typeset snippets for cards, lists and panels (cached).

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use egui::{Response, Sense, Ui, Vec2};
use mpp_render::{layout_latex, DisplayList, LayoutOptions, Theme};

use crate::paint::paint_display_list;

type Key = (String, u32, Theme);

thread_local! {
    static CACHE: RefCell<HashMap<Key, Arc<DisplayList>>> = RefCell::new(HashMap::new());
}

/// The layout of a LaTeX snippet at `size` pixels, inline style.
pub fn layout(latex: &str, size: f32, theme: Theme) -> Arc<DisplayList> {
    let key = (latex.to_string(), size.to_bits(), theme);
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(d) = c.get(&key) {
            return d.clone();
        }
        if c.len() > 1024 {
            c.clear();
        }
        let opts = LayoutOptions {
            theme,
            ..LayoutOptions::inline(size)
        };
        let d = Arc::new(layout_latex(latex, &opts));
        c.insert(key, d.clone());
        d
    })
}

/// Draws a snippet in the UI, left aligned, and returns its response.
pub fn show(ui: &mut Ui, latex: &str, size: f32, theme: Theme) -> Response {
    let dl = layout(latex, size, theme);
    let (resp, painter) = ui.allocate_painter(
        Vec2::new(dl.width.max(1.0), dl.total_height().max(size)),
        Sense::click(),
    );
    let dy = ((resp.rect.height() - dl.total_height()) / 2.0).max(0.0);
    paint_display_list(&painter, &dl, resp.rect.min + Vec2::new(0.0, dy));
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_are_cached() {
        let a = layout("x^2", 18.0, Theme::LIGHT);
        let b = layout("x^2", 18.0, Theme::LIGHT);
        assert!(Arc::ptr_eq(&a, &b));
        assert!(a.width > 0.0);
    }
}
