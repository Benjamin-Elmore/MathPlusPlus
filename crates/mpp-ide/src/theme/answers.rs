//! Answer slot colours (U16, U17), in a block of their own: the answer to a line
//! ending in `=`, a definition's grey value, a dimmed answer from the last run, an
//! error, and a check that holds or fails. Each readable one passes 4.5:1 on its page.

use mpp_render::{Color, Theme};

/// The colours of the answer slot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnswerColors {
    /// An answer to a line ending in `=`.
    pub answer: Color,
    /// A definition's value, shown in grey (`E := 2x + 5` → `= 15`).
    pub preview: Color,
    /// An answer from the last run while a new one is going.
    pub stale: Color,
    /// An error in the slot.
    pub error: Color,
    /// A check that holds (✓).
    pub holds: Color,
    /// A check that fails (✗).
    pub fails: Color,
    /// The quiet "defined" mark of `x = 5` read as a definition.
    pub mark: Color,
}

/// True for a theme with a light page.
fn is_light(theme: &Theme) -> bool {
    let b = theme.background;
    u32::from(b.r) + u32::from(b.g) + u32::from(b.b) > 3 * 128
}

/// The answer colours for a theme.
pub fn answer_colors(theme: &Theme) -> AnswerColors {
    let (answer, holds) = if is_light(theme) {
        (Color::rgb(0x0a, 0x6b, 0x5e), Color::rgb(0x1a, 0x7f, 0x37))
    } else {
        (Color::rgb(0x6f, 0xd6, 0xc4), Color::rgb(0x56, 0xd3, 0x64))
    };
    AnswerColors {
        answer,
        preview: theme.auto_result,
        stale: mix(answer, theme.background, 0.55),
        error: theme.error,
        holds,
        fails: theme.error,
        mark: theme.auto_result,
    }
}

/// `a` moved a fraction `t` of the way to `b`.
fn mix(a: Color, b: Color, t: f32) -> Color {
    let m = |x: u8, y: u8| {
        let v = f32::from(x) + (f32::from(y) - f32::from(x)) * t;
        v.round().clamp(0.0, 255.0) as u8
    };
    Color::rgb(m(a.r, b.r), m(a.g, b.g), m(a.b, b.b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::contrast;

    #[test]
    fn answer_colours_are_readable_on_both_pages() {
        for theme in [Theme::LIGHT, Theme::DARK] {
            let c = answer_colors(&theme);
            for (what, col) in [
                ("answer", c.answer),
                ("preview", c.preview),
                ("holds", c.holds),
                ("error", c.error),
            ] {
                let r = contrast(col, theme.background);
                assert!(r >= 4.5, "{what} on {:?}: {r}", theme.background);
            }
            // A stale answer is dimmed, but still there to read.
            let stale = contrast(c.stale, theme.background);
            assert!(stale < contrast(c.answer, theme.background));
            assert!(stale >= 1.5, "{stale}");
        }
    }
}
