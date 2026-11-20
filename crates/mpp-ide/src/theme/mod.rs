//! Colours and fonts. Every colour comes from mpp-render's [`Theme`]; the UI
//! chrome is set in Latin Modern Roman.

use std::sync::Arc;

use egui::{Color32, Context, FontData, FontDefinitions, FontFamily, FontId, TextStyle};
use mpp_render::{FontId as RenderFont, Theme};

use crate::paint::c32;

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
        match self {
            Mode::Light => Theme::LIGHT,
            Mode::Dark => Theme::DARK,
        }
    }
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (f32::from(x) * (1.0 - t) + f32::from(y) * t).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

/// Registers Latin Modern Roman as the interface font.
pub fn install_fonts(ctx: &Context) {
    let mut defs = FontDefinitions::default();
    defs.font_data.insert(
        "lm-roman".into(),
        Arc::new(FontData::from_static(RenderFont::Text.bytes())),
    );
    defs.font_data.insert(
        "lm-math".into(),
        Arc::new(FontData::from_static(RenderFont::Math.bytes())),
    );
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        let list = defs.families.entry(family).or_default();
        list.insert(0, "lm-roman".into());
        list.push("lm-math".into());
    }
    ctx.set_fonts(defs);
    ctx.global_style_mut(|style| {
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(16.0));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::proportional(16.0));
        style
            .text_styles
            .insert(TextStyle::Small, FontId::proportional(13.0));
        style
            .text_styles
            .insert(TextStyle::Heading, FontId::proportional(22.0));
        style
            .text_styles
            .insert(TextStyle::Monospace, FontId::proportional(15.0));
    });
}

/// Fonts and colours in one go.
pub fn setup(ctx: &Context, mode: Mode) {
    install_fonts(ctx);
    apply(ctx, mode);
}

/// Applies a mode's colours to egui's widgets.
pub fn apply(ctx: &Context, mode: Mode) {
    let t = mode.theme();
    let bg = c32(t.background);
    let ink = c32(t.text);
    let mut v = match mode {
        Mode::Light => egui::Visuals::light(),
        Mode::Dark => egui::Visuals::dark(),
    };
    let panel = mix(bg, c32(t.code_background), 0.6);
    v.panel_fill = panel;
    v.window_fill = bg;
    v.extreme_bg_color = bg;
    v.faint_bg_color = c32(t.code_background);
    v.override_text_color = Some(ink);
    v.hyperlink_color = c32(t.link);
    v.error_fg_color = c32(t.error);
    v.warn_fg_color = c32(t.warning);
    v.selection.bg_fill = mix(bg, c32(t.link), 0.28);
    v.selection.stroke = egui::Stroke::new(1.0, c32(t.link));
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, mix(bg, ink, 0.18));
    v.widgets.inactive.bg_fill = mix(panel, ink, 0.05);
    v.widgets.inactive.weak_bg_fill = mix(panel, ink, 0.05);
    v.widgets.hovered.bg_fill = mix(panel, ink, 0.12);
    v.widgets.hovered.weak_bg_fill = mix(panel, ink, 0.12);
    ctx.set_visuals_of(
        match mode {
            Mode::Light => egui::Theme::Light,
            Mode::Dark => egui::Theme::Dark,
        },
        v,
    );
    ctx.set_theme(match mode {
        Mode::Light => egui::ThemePreference::Light,
        Mode::Dark => egui::ThemePreference::Dark,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_use_the_render_themes() {
        assert_eq!(Mode::Light.theme(), Theme::LIGHT);
        assert_eq!(Mode::Dark.theme(), Theme::DARK);
    }
}
