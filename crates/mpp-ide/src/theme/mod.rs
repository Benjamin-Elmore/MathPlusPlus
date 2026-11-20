//! Colours and fonts. Every colour comes from the [`Palette`] table and every
//! piece of text is set in Latin Modern.

mod fonts;
mod moon;
mod palette;

use egui::Context;

pub use fonts::{install_fonts, sans, sans_family};
pub use moon::moon_button;
pub use palette::{c32, contrast, luminance, Mode, Palette, CLEAR};

/// What the user chose in Settings: a fixed theme, or follow Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeChoice {
    Light,
    Dark,
    /// Use the Windows app theme (the default).
    #[default]
    System,
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 3] = [ThemeChoice::Light, ThemeChoice::Dark, ThemeChoice::System];

    /// The mode to draw in, given the system theme when it is known.
    pub fn resolve(self, system: Option<egui::Theme>) -> Mode {
        match self {
            ThemeChoice::Light => Mode::Light,
            ThemeChoice::Dark => Mode::Dark,
            ThemeChoice::System => match system {
                Some(egui::Theme::Dark) => Mode::Dark,
                _ => Mode::Light,
            },
        }
    }

    /// The name shown in menus.
    pub fn label(self) -> &'static str {
        match self {
            ThemeChoice::Light => "Light",
            ThemeChoice::Dark => "Dark",
            ThemeChoice::System => "Follow Windows",
        }
    }

    /// The next choice in the cycle Light, Dark, Follow Windows.
    pub fn next(self) -> ThemeChoice {
        match self {
            ThemeChoice::Light => ThemeChoice::Dark,
            ThemeChoice::Dark => ThemeChoice::System,
            ThemeChoice::System => ThemeChoice::Light,
        }
    }

    /// The word saved in settings.
    pub fn to_stored(self) -> &'static str {
        match self {
            ThemeChoice::Light => "light",
            ThemeChoice::Dark => "dark",
            ThemeChoice::System => "system",
        }
    }

    /// Reads a saved choice.
    pub fn from_stored(s: &str) -> Option<ThemeChoice> {
        match s {
            "light" => Some(ThemeChoice::Light),
            "dark" => Some(ThemeChoice::Dark),
            "system" => Some(ThemeChoice::System),
            _ => None,
        }
    }
}

/// Fonts and colours in one go.
pub fn setup(ctx: &Context, mode: Mode) {
    install_fonts(ctx);
    apply(ctx, mode);
}

/// Applies a mode's colours to egui's widgets.
pub fn apply(ctx: &Context, mode: Mode) {
    let p = mode.palette();
    let t = p.theme;
    let (bg, ink, muted) = (c32(t.background), c32(t.text), c32(t.muted));
    let mut v = match mode {
        Mode::Light => egui::Visuals::light(),
        Mode::Dark => egui::Visuals::dark(),
    };
    v.panel_fill = c32(p.panel);
    v.window_fill = c32(p.tooltip);
    v.window_stroke = egui::Stroke::new(1.0, c32(p.border));
    v.extreme_bg_color = bg;
    v.faint_bg_color = c32(t.code_background);
    v.code_bg_color = c32(t.code_background);
    v.override_text_color = Some(ink);
    v.weak_text_color = Some(muted);
    v.hyperlink_color = c32(t.link);
    v.error_fg_color = c32(t.error);
    v.warn_fg_color = c32(t.warning);
    v.selection.bg_fill = c32(p.selection);
    v.selection.stroke = egui::Stroke::new(1.0, c32(t.link));
    v.widgets.noninteractive.bg_fill = c32(p.panel);
    v.widgets.noninteractive.weak_bg_fill = c32(p.panel);
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, c32(p.border));
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, ink);
    for (w, fill) in [
        (&mut v.widgets.inactive, p.widget),
        (&mut v.widgets.hovered, p.widget_hover),
        (&mut v.widgets.active, p.widget_active),
        (&mut v.widgets.open, p.widget_hover),
    ] {
        w.bg_fill = c32(fill);
        w.weak_bg_fill = c32(fill);
        w.bg_stroke = egui::Stroke::new(1.0, c32(p.border));
        w.fg_stroke = egui::Stroke::new(1.5, ink);
    }
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
    use mpp_render::Theme;
    use std::path::{Path, PathBuf};

    #[test]
    fn modes_use_the_render_themes() {
        assert_eq!(Mode::Light.theme(), Theme::LIGHT);
        assert_eq!(Mode::Dark.theme(), Theme::DARK);
    }

    #[test]
    fn follow_windows_reads_the_system_theme() {
        let c = ThemeChoice::System;
        assert_eq!(c.resolve(Some(egui::Theme::Dark)), Mode::Dark);
        assert_eq!(c.resolve(Some(egui::Theme::Light)), Mode::Light);
        assert_eq!(c.resolve(None), Mode::Light);
        assert_eq!(
            ThemeChoice::Dark.resolve(Some(egui::Theme::Light)),
            Mode::Dark
        );
        assert_eq!(ThemeChoice::default(), ThemeChoice::System);
    }

    #[test]
    fn choices_cycle_and_round_trip() {
        let mut c = ThemeChoice::Light;
        for _ in 0..3 {
            assert_eq!(ThemeChoice::from_stored(c.to_stored()), Some(c));
            c = c.next();
        }
        assert_eq!(c, ThemeChoice::Light);
    }

    fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).expect("read dir").flatten() {
            let p = e.path();
            if p.is_dir() {
                rust_files(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }

    /// No colour is written in the IDE outside `theme/`: the palette is the
    /// only place that names one.
    #[test]
    fn no_colour_literal_outside_the_theme_module() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = vec![];
        rust_files(&src, &mut files);
        assert!(files.len() > 10);
        let banned = [
            "Color32::",
            "Color::rgb",
            "from_rgb",
            "from_gray",
            "Rgba::",
            "Hsva",
        ];
        let mut hits = vec![];
        for f in files {
            if f.components().any(|c| c.as_os_str() == "theme") {
                continue;
            }
            let text = std::fs::read_to_string(&f).expect("read source");
            for (n, line) in text.lines().enumerate() {
                let code = line.split("//").next().unwrap_or("");
                if code.trim_start().starts_with("use ") {
                    continue;
                }
                if banned.iter().any(|b| code.contains(b)) {
                    hits.push(format!("{}:{}: {}", f.display(), n + 1, line.trim()));
                }
            }
        }
        assert!(
            hits.is_empty(),
            "colour literals outside theme/:\n{}",
            hits.join("\n")
        );
    }
}
