//! Colours as 8-bit RGBA, with the few operations the plots need.

/// An sRGB colour with alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// An opaque colour.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b, a: 255 }
    }

    /// A colour with alpha.
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color { r, g, b, a }
    }

    /// Parses `#rrggbb` or `#rgb` (the `#` is optional).
    pub fn from_hex(text: &str) -> Option<Color> {
        let t = text.trim().trim_start_matches('#');
        if !t.is_ascii() {
            return None;
        }
        let byte = |s: &str| u8::from_str_radix(s, 16).ok();
        match t.len() {
            6 => Some(Color::rgb(
                byte(&t[0..2])?,
                byte(&t[2..4])?,
                byte(&t[4..6])?,
            )),
            3 => {
                let d = |i: usize| byte(&t[i..=i]).map(|v| v * 17);
                Some(Color::rgb(d(0)?, d(1)?, d(2)?))
            }
            _ => None,
        }
    }

    /// A colour from a hex literal that is known to be valid; black otherwise.
    pub fn hex(text: &str) -> Color {
        Color::from_hex(text).unwrap_or(Color::rgb(0, 0, 0))
    }

    /// `#rrggbb`.
    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// The same colour with another alpha (0 to 255).
    pub fn with_alpha(self, a: u8) -> Color {
        Color { a, ..self }
    }

    /// Opacity from 0.0 to 1.0.
    pub fn opacity(self) -> f32 {
        f32::from(self.a) / 255.0
    }

    /// Linear interpolation in sRGB space (`t` is clamped to 0..=1).
    pub fn lerp(self, other: Color, t: f32) -> Color {
        let t = if t.is_finite() {
            t.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let mix = |a: u8, b: u8| {
            (f32::from(a) + (f32::from(b) - f32::from(a)) * t)
                .round()
                .clamp(0.0, 255.0) as u8
        };
        Color {
            r: mix(self.r, other.r),
            g: mix(self.g, other.g),
            b: mix(self.b, other.b),
            a: mix(self.a, other.a),
        }
    }

    /// Scales the brightness (`k` > 1 brightens, clamped at white).
    pub fn scaled(self, k: f32) -> Color {
        let f = |c: u8| (f32::from(c) * k).round().clamp(0.0, 255.0) as u8;
        Color {
            r: f(self.r),
            g: f(self.g),
            b: f(self.b),
            a: self.a,
        }
    }

    /// `self` drawn over `under`, as one opaque colour.
    pub fn over(self, under: Color) -> Color {
        let t = self.opacity();
        Color {
            a: 255,
            ..under.lerp(self.with_alpha(255), t)
        }
    }

    /// WCAG relative luminance.
    pub fn luminance(self) -> f64 {
        let lin = |c: u8| {
            let s = f64::from(c) / 255.0;
            if s <= 0.039_28 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(self.r) + 0.7152 * lin(self.g) + 0.0722 * lin(self.b)
    }

    /// WCAG contrast ratio between two colours (1 to 21).
    pub fn contrast(self, other: Color) -> f64 {
        let (a, b) = (self.luminance(), other.luminance());
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        assert_eq!(Color::hex("#1f5fbf").to_hex(), "#1f5fbf");
        assert_eq!(Color::from_hex("fff"), Some(Color::rgb(255, 255, 255)));
        assert_eq!(Color::from_hex("#12"), None);
        assert_eq!(Color::from_hex("é"), None);
    }

    #[test]
    fn contrast_of_black_on_white_is_21() {
        let c = Color::rgb(0, 0, 0).contrast(Color::rgb(255, 255, 255));
        assert!((c - 21.0).abs() < 1e-9);
    }

    #[test]
    fn lerp_ends() {
        let (a, b) = (Color::rgb(0, 0, 0), Color::rgb(200, 100, 50));
        assert_eq!(a.lerp(b, 0.0), a);
        assert_eq!(a.lerp(b, 1.0), b);
        assert_eq!(a.lerp(b, f32::NAN), a);
    }
}
