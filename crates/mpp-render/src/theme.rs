//! Colour themes. Every colour the layout, SVG and HTML output use comes from
//! a [`Theme`]; nothing else in the crate names a colour.

use crate::display::Color;

/// The colours of a page and the maths on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Theme {
    /// Ordinary text and maths ink (also fraction bars and radical rules).
    pub text: Color,
    /// Filled-in `= ?` results.
    pub result: Color,
    /// Automatic ("ghost") results nobody asked for.
    pub auto_result: Color,
    /// Dotted outline of an empty slot.
    pub placeholder_outline: Color,
    /// Grey label inside an empty slot (`lower`, `upper` …).
    pub placeholder_label: Color,
    /// Lines on the page: horizontal rules, table rules.
    pub rule: Color,
    /// Page background.
    pub background: Color,
    /// Background of code blocks.
    pub code_background: Color,
    /// Links.
    pub link: Color,
    /// Secondary prose (import lines, helper sentences, quotations).
    pub muted: Color,
    pub error: Color,
    pub warning: Color,
}

impl Theme {
    /// Dark ink on paper white.
    pub const LIGHT: Theme = Theme {
        text: Color::rgb(0x11, 0x11, 0x11),
        result: Color::rgb(0x1f, 0x5f, 0xb8),
        auto_result: Color::rgb(0x9a, 0x9a, 0x9a),
        placeholder_outline: Color::rgb(0xa8, 0xa8, 0xa8),
        placeholder_label: Color::rgb(0x9c, 0x9c, 0x9c),
        rule: Color::rgb(0x11, 0x11, 0x11),
        background: Color::rgb(0xfd, 0xfd, 0xfb),
        code_background: Color::rgb(0xf2, 0xf2, 0xee),
        link: Color::rgb(0x1f, 0x4f, 0x98),
        muted: Color::rgb(0x44, 0x44, 0x44),
        error: Color::rgb(0xc0, 0x1c, 0x28),
        warning: Color::rgb(0xb5, 0x83, 0x0a),
    };

    /// Light ink on a dark grey page.
    pub const DARK: Theme = Theme {
        text: Color::rgb(0xe6, 0xe6, 0xe3),
        result: Color::rgb(0x7a, 0xb4, 0xff),
        auto_result: Color::rgb(0x80, 0x80, 0x80),
        placeholder_outline: Color::rgb(0x6c, 0x6c, 0x6c),
        placeholder_label: Color::rgb(0x88, 0x88, 0x88),
        rule: Color::rgb(0xe6, 0xe6, 0xe3),
        background: Color::rgb(0x1b, 0x1c, 0x1e),
        code_background: Color::rgb(0x26, 0x27, 0x2a),
        link: Color::rgb(0x8a, 0xb8, 0xff),
        muted: Color::rgb(0xb0, 0xb0, 0xb0),
        error: Color::rgb(0xff, 0x6b, 0x6b),
        warning: Color::rgb(0xf0, 0xc0, 0x50),
    };

    /// Each colour with its CSS custom property name (`--mpp-text` …).
    pub fn css_vars(&self) -> [(&'static str, Color); 12] {
        [
            ("--mpp-text", self.text),
            ("--mpp-result", self.result),
            ("--mpp-auto-result", self.auto_result),
            ("--mpp-placeholder", self.placeholder_outline),
            ("--mpp-placeholder-label", self.placeholder_label),
            ("--mpp-rule", self.rule),
            ("--mpp-background", self.background),
            ("--mpp-code-background", self.code_background),
            ("--mpp-link", self.link),
            ("--mpp-muted", self.muted),
            ("--mpp-error", self.error),
            ("--mpp-warning", self.warning),
        ]
    }

    /// The CSS variable that holds `c` in this theme, if any.
    pub fn var_for(&self, c: Color) -> Option<&'static str> {
        self.css_vars()
            .into_iter()
            .find(|(_, v)| *v == c)
            .map(|(name, _)| name)
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme::LIGHT
    }
}
