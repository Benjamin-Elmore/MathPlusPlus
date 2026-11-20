//! Latin Modern throughout: Roman for prose and every part of the interface,
//! Math for maths, Sans for small interface text, Mono only for the file-text
//! pop-out. The fonts are embedded by mpp-render under the GUST Font License.

use std::sync::Arc;

use egui::{Context, FontData, FontDefinitions, FontFamily, FontId, TextStyle};
use mpp_render::font::{LM_MONO, LM_SANS};
use mpp_render::FontId as RenderFont;

/// The family of Latin Modern Sans (small interface text).
pub fn sans_family() -> FontFamily {
    FontFamily::Name("lm-sans".into())
}

/// Latin Modern Sans at `size`.
pub fn sans(size: f32) -> FontId {
    FontId::new(size, sans_family())
}

/// Our fonts first, then egui's own, which stay behind only for symbols Latin
/// Modern lacks (the emoji on the theme button).
fn family_list(defs: &FontDefinitions, family: &FontFamily, first: &[&str]) -> Vec<String> {
    let mut list: Vec<String> = first.iter().map(|s| (*s).to_string()).collect();
    if let Some(rest) = defs.families.get(family) {
        list.extend(
            rest.iter()
                .filter(|n| !list.contains(n))
                .cloned()
                .collect::<Vec<_>>(),
        );
    }
    list
}

/// Registers the Latin Modern families and the text styles.
pub fn install_fonts(ctx: &Context) {
    let mut defs = FontDefinitions::default();
    let embedded: [(&str, &'static [u8]); 4] = [
        ("lm-roman", RenderFont::Text.bytes()),
        ("lm-math", RenderFont::Math.bytes()),
        ("lm-sans", LM_SANS),
        ("lm-mono", LM_MONO),
    ];
    for (name, bytes) in embedded {
        defs.font_data
            .insert(name.into(), Arc::new(FontData::from_static(bytes)));
    }
    let proportional = family_list(&defs, &FontFamily::Proportional, &["lm-roman", "lm-math"]);
    let monospace = family_list(
        &defs,
        &FontFamily::Monospace,
        &["lm-mono", "lm-roman", "lm-math"],
    );
    let sans_list = family_list(
        &defs,
        &FontFamily::Proportional,
        &["lm-sans", "lm-roman", "lm-math"],
    );
    defs.families.insert(FontFamily::Proportional, proportional);
    defs.families.insert(FontFamily::Monospace, monospace);
    defs.families.insert(sans_family(), sans_list);
    ctx.set_fonts(defs);
    // Text styles live in each theme's own style, so set both.
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        ctx.style_mut_of(theme, |style| {
            let styles = &mut style.text_styles;
            styles.insert(TextStyle::Body, FontId::proportional(16.0));
            styles.insert(TextStyle::Button, FontId::proportional(16.0));
            styles.insert(TextStyle::Small, FontId::proportional(13.0));
            styles.insert(TextStyle::Heading, FontId::proportional(22.0));
            styles.insert(TextStyle::Monospace, FontId::monospace(14.0));
        });
    }
}
