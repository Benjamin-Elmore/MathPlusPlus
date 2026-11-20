//! The one colour table of the IDE. Every colour on screen comes from a
//! [`Palette`]: the render [`Theme`] (page, ink, results, outlines, squiggles,
//! graph colours) plus the interface colours around it (panels, widgets,
//! tooltips, selection, scrollbars). No other module names a colour.

use egui::Color32;
use mpp_render::{Color, Theme};

/// An mpp-render colour as an egui colour.
pub fn c32(c: Color) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

/// Light or dark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Light,
    Dark,
}

impl Mode {
    /// The render theme for this mode.
    pub fn theme(self) -> Theme {
        self.palette().theme
    }

    /// The whole colour table for this mode.
    pub fn palette(self) -> Palette {
        match self {
            Mode::Light => Palette::LIGHT,
            Mode::Dark => Palette::DARK,
        }
    }
}

/// Every colour of one mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// Page, ink, results, outlines, error colours, graph colours.
    pub theme: Theme,
    /// Side panels, menu bar, tab strip, status bar.
    pub panel: Color,
    /// Buttons and other widgets at rest.
    pub widget: Color,
    /// Widgets under the pointer.
    pub widget_hover: Color,
    /// Widgets while pressed.
    pub widget_active: Color,
    /// Outlines of panels and widgets.
    pub border: Color,
    /// Tooltips, menus, dialogs and the autocomplete card.
    pub tooltip: Color,
    /// Selected text.
    pub selection: Color,
    /// The highlighted row in a list (autocomplete).
    pub row_selected: Color,
    /// The wash behind a definition you jumped to.
    pub highlight: Color,
    /// Scrollbar handles.
    pub scrollbar: Color,
    /// Soft shadow along page edges.
    pub edge: Color,
}

impl Palette {
    pub const LIGHT: Palette = Palette {
        theme: Theme::LIGHT,
        panel: Color::rgb(0xf6, 0xf6, 0xf2),
        widget: Color::rgb(0xe9, 0xe9, 0xe5),
        widget_hover: Color::rgb(0xd9, 0xd9, 0xd4),
        widget_active: Color::rgb(0xc9, 0xc9, 0xc3),
        border: Color::rgb(0x8c, 0x8c, 0x8c),
        tooltip: Color::rgb(0xff, 0xff, 0xff),
        selection: Color::rgb(0xc4, 0xd6, 0xf2),
        row_selected: Color::rgb(0xd6, 0xe2, 0xf5),
        highlight: Color::rgb(0xf6, 0xe6, 0xa8),
        scrollbar: Color::rgb(0x8c, 0x8c, 0x8c),
        edge: Color::rgb(0xe4, 0xe4, 0xe0),
    };

    pub const DARK: Palette = Palette {
        theme: Theme::DARK,
        panel: Color::rgb(0x22, 0x23, 0x26),
        widget: Color::rgb(0x2e, 0x2f, 0x33),
        widget_hover: Color::rgb(0x3b, 0x3c, 0x41),
        widget_active: Color::rgb(0x48, 0x49, 0x4f),
        border: Color::rgb(0x80, 0x80, 0x85),
        tooltip: Color::rgb(0x2b, 0x2c, 0x30),
        selection: Color::rgb(0x2f, 0x4a, 0x75),
        row_selected: Color::rgb(0x2a, 0x3f, 0x63),
        highlight: Color::rgb(0x4e, 0x41, 0x1c),
        scrollbar: Color::rgb(0x88, 0x88, 0x8d),
        edge: Color::rgb(0x2a, 0x2b, 0x2e),
    };

    /// Pairs of (name, foreground, background) that carry text and must reach
    /// 4.5:1 (WCAG AA for normal text).
    pub fn text_pairs(&self) -> Vec<(&'static str, Color, Color)> {
        let t = &self.theme;
        vec![
            ("text on page", t.text, t.background),
            ("text on panel", t.text, self.panel),
            ("text on code block", t.text, t.code_background),
            ("text on tooltip", t.text, self.tooltip),
            ("text on widget", t.text, self.widget),
            ("text on hovered widget", t.text, self.widget_hover),
            ("text on pressed widget", t.text, self.widget_active),
            ("text on selection", t.text, self.selection),
            ("text on selected row", t.text, self.row_selected),
            ("text on highlight", t.text, self.highlight),
            ("muted on page", t.muted, t.background),
            ("muted on panel", t.muted, self.panel),
            ("muted on tooltip", t.muted, self.tooltip),
            ("muted on code block", t.muted, t.code_background),
            ("result on page", t.result, t.background),
            ("result on code block", t.result, t.code_background),
            ("ghost result on page", t.auto_result, t.background),
            (
                "placeholder label on page",
                t.placeholder_label,
                t.background,
            ),
            ("link on page", t.link, t.background),
            ("link on panel", t.link, self.panel),
            ("error on page", t.error, t.background),
            ("error on panel", t.error, self.panel),
            ("error on tooltip", t.error, self.tooltip),
            ("warning on page", t.warning, t.background),
            ("warning on panel", t.warning, self.panel),
            ("warning on tooltip", t.warning, self.tooltip),
            ("info on page", t.info, t.background),
            ("info on panel", t.info, self.panel),
        ]
    }

    /// Pairs of lines and strokes against what they sit on; each must reach
    /// 3:1 (WCAG AA for graphics and user-interface components).
    pub fn line_pairs(&self) -> Vec<(&'static str, Color, Color)> {
        let t = &self.theme;
        let mut v = vec![
            ("rule on page", t.rule, t.background),
            (
                "placeholder outline on page",
                t.placeholder_outline,
                t.background,
            ),
            ("border on page", self.border, t.background),
            ("border on panel", self.border, self.panel),
            ("border on tooltip", self.border, self.tooltip),
            ("scrollbar on panel", self.scrollbar, self.panel),
            ("scrollbar on page", self.scrollbar, t.background),
            ("axis on page", t.axis, t.background),
        ];
        const NAMES: [&str; 8] = [
            "curve 1", "curve 2", "curve 3", "curve 4", "curve 5", "curve 6", "curve 7", "curve 8",
        ];
        for (name, s) in NAMES.iter().zip(t.series) {
            v.push((name, s, t.background));
        }
        v
    }
}

/// WCAG relative luminance of an sRGB colour.
pub fn luminance(c: Color) -> f64 {
    let lin = |v: u8| {
        let s = f64::from(v) / 255.0;
        if s <= 0.039_28 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// WCAG contrast ratio between two colours, from 1 to 21.
pub fn contrast(a: Color, b: Color) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_of_black_on_white_is_21() {
        let r = contrast(Color::rgb(0, 0, 0), Color::rgb(255, 255, 255));
        assert!((r - 21.0).abs() < 0.01, "{r}");
    }

    #[test]
    fn every_text_pair_meets_wcag_aa() {
        for (name, mode) in [("light", Mode::Light), ("dark", Mode::Dark)] {
            for (pair, fg, bg) in mode.palette().text_pairs() {
                let r = contrast(fg, bg);
                assert!(r >= 4.5, "{name}: {pair} has contrast {r:.2}, needs 4.5");
            }
        }
    }

    #[test]
    fn every_line_pair_meets_wcag_aa() {
        for (name, mode) in [("light", Mode::Light), ("dark", Mode::Dark)] {
            for (pair, fg, bg) in mode.palette().line_pairs() {
                let r = contrast(fg, bg);
                assert!(r >= 3.0, "{name}: {pair} has contrast {r:.2}, needs 3");
            }
        }
    }

    #[test]
    fn graph_grid_is_quieter_than_the_axes_but_visible() {
        for mode in [Mode::Light, Mode::Dark] {
            let t = mode.theme();
            let grid = contrast(t.grid, t.background);
            let axis = contrast(t.axis, t.background);
            assert!(grid >= 1.2 && grid < axis, "grid {grid:.2} axis {axis:.2}");
        }
    }
}
