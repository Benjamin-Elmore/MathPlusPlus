//! SVG output. Glyphs are drawn from their outlines (shared as `<defs>` and
//! placed with `<use>`), so the SVG needs no font installed.

use std::collections::BTreeSet;
use std::fmt::Write;

use crate::display::{Color, DisplayList, PathCmd, Primitive};
use crate::font::{FontId, Fonts};

/// Options for [`to_svg_with`].
#[derive(Debug, Clone, PartialEq)]
pub struct SvgOptions {
    /// Draw glyphs from outlines (true) or as `<text>` in the Latin Modern fonts (false;
    /// smaller and easy to read in tests, but needs the fonts installed).
    pub outlines: bool,
    /// Extra room around the formula, in pixels.
    pub padding: f32,
}

impl Default for SvgOptions {
    fn default() -> Self {
        SvgOptions {
            outlines: true,
            padding: 0.0,
        }
    }
}

/// A standalone SVG document for a formula.
pub fn to_svg(dl: &DisplayList) -> String {
    to_svg_with(dl, &SvgOptions::default())
}

/// A standalone SVG document for a formula, with options.
pub fn to_svg_with(dl: &DisplayList, opts: &SvgOptions) -> String {
    let mut used = BTreeSet::new();
    let body = svg_body(dl, opts.outlines, &mut used, &|c: Color| c.hex());
    let p = if opts.padding.is_finite() {
        opts.padding.max(0.0)
    } else {
        0.0
    };
    let w = dl.width + 2.0 * p;
    let h = dl.total_height() + 2.0 * p;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{}\" height=\"{}\" viewBox=\"{} {} {} {}\" overflow=\"visible\">",
        num(w),
        num(h),
        num(-p),
        num(-p),
        num(w),
        num(h)
    );
    if opts.outlines && !used.is_empty() {
        out.push_str(&glyph_defs(&used));
    }
    out.push_str(&body);
    out.push_str("</svg>\n");
    out
}

/// The drawing elements of a formula; records the glyphs it uses.
/// `paint` turns a colour into an SVG paint (a hex value, or a CSS variable
/// in the HTML page so the page can switch themes).
pub(crate) fn svg_body(
    dl: &DisplayList,
    outlines: bool,
    used: &mut BTreeSet<(FontId, u16)>,
    paint: &dyn Fn(Color) -> String,
) -> String {
    let fonts = Fonts::get();
    let mut out = String::new();
    for item in &dl.items {
        match item {
            Primitive::Glyph {
                font,
                glyph,
                ch,
                x,
                y,
                size,
                color,
            } => {
                if outlines {
                    if fonts.outline(*font, *glyph).is_none() {
                        continue;
                    }
                    used.insert((*font, *glyph));
                    let k = size / fonts.units_per_em(*font);
                    let _ = writeln!(
                        out,
                        "<use xlink:href=\"#{}\" href=\"#{}\" transform=\"matrix({} 0 0 {} {} {})\" fill=\"{}\"{}/>",
                        glyph_id(*font, *glyph),
                        glyph_id(*font, *glyph),
                        num4(k),
                        num4(-k),
                        num(*x),
                        num(*y),
                        paint(*color),
                        opacity(color.opacity()),
                    );
                } else {
                    let _ = writeln!(
                        out,
                        "<text x=\"{}\" y=\"{}\" font-family=\"{}\" font-size=\"{}\" fill=\"{}\"{}>{}</text>",
                        num(*x),
                        num(*y),
                        font.family(),
                        num(*size),
                        paint(*color),
                        style_attr(*font),
                        escape_char(*ch),
                    );
                }
            }
            Primitive::Rule { rect, color } => {
                let _ = writeln!(
                    out,
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>",
                    num(rect.x),
                    num(rect.y),
                    num(rect.w),
                    num(rect.h),
                    paint(*color)
                );
            }
            Primitive::Outline {
                rect,
                color,
                dotted,
            } => {
                let sw = (dl.font_size / 20.0).clamp(0.5, 2.0);
                let dash = if *dotted {
                    format!(" stroke-dasharray=\"{} {}\"", num(sw * 1.5), num(sw * 1.5))
                } else {
                    String::new()
                };
                let _ = writeln!(
                    out,
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\"{}/>",
                    num(rect.x + sw / 2.0),
                    num(rect.y + sw / 2.0),
                    num((rect.w - sw).max(0.0)),
                    num((rect.h - sw).max(0.0)),
                    paint(*color),
                    num(sw),
                    dash
                );
            }
            Primitive::Path {
                cmds,
                color,
                stroke,
            } => {
                let d = path_data(cmds);
                match stroke {
                    Some(w) => {
                        let _ = writeln!(
                            out,
                            "<path d=\"{d}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\"/>",
                            paint(*color),
                            num(*w)
                        );
                    }
                    None => {
                        let _ = writeln!(out, "<path d=\"{d}\" fill=\"{}\"/>", paint(*color));
                    }
                }
            }
        }
    }
    out
}

/// `<defs>` with the outline of every glyph in `used`, in font units.
pub(crate) fn glyph_defs(used: &BTreeSet<(FontId, u16)>) -> String {
    let fonts = Fonts::get();
    let mut out = String::from("<defs>\n");
    for (font, glyph) in used {
        if let Some(d) = fonts.outline(*font, *glyph) {
            let _ = writeln!(out, "<path id=\"{}\" d=\"{d}\"/>", glyph_id(*font, *glyph));
        }
    }
    out.push_str("</defs>\n");
    out
}

fn glyph_id(font: FontId, glyph: u16) -> String {
    format!("mpp-{}{}", font.tag(), glyph)
}

fn style_attr(font: FontId) -> &'static str {
    match font {
        FontId::TextItalic => " font-style=\"italic\"",
        FontId::TextBold => " font-weight=\"bold\"",
        _ => "",
    }
}

fn opacity(o: f32) -> String {
    if o >= 0.999 {
        String::new()
    } else {
        format!(" fill-opacity=\"{}\"", num(o))
    }
}

fn path_data(cmds: &[PathCmd]) -> String {
    let mut d = String::new();
    for c in cmds {
        let _ = match *c {
            PathCmd::MoveTo(x, y) => write!(d, "M{} {}", num(x), num(y)),
            PathCmd::LineTo(x, y) => write!(d, "L{} {}", num(x), num(y)),
            PathCmd::QuadTo(a, b, x, y) => {
                write!(d, "Q{} {} {} {}", num(a), num(b), num(x), num(y))
            }
            PathCmd::CubicTo(a, b, c2, e, x, y) => write!(
                d,
                "C{} {} {} {} {} {}",
                num(a),
                num(b),
                num(c2),
                num(e),
                num(x),
                num(y)
            ),
            PathCmd::Close => write!(d, "Z"),
        };
    }
    d
}

/// Formats a number with at most two decimals and no trailing zeros.
pub(crate) fn num(v: f32) -> String {
    fmt_fixed(v, 100.0)
}

fn num4(v: f32) -> String {
    fmt_fixed(v, 100_000.0)
}

fn fmt_fixed(v: f32, k: f32) -> String {
    if !v.is_finite() {
        return "0".to_string();
    }
    let r = (f64::from(v) * f64::from(k)).round() / f64::from(k);
    let r = if r == 0.0 { 0.0 } else { r };
    let s = format!("{r}");
    s
}

/// XML-escapes a character for text content.
fn escape_char(c: char) -> String {
    match c {
        '<' => "&lt;".to_string(),
        '>' => "&gt;".to_string(),
        '&' => "&amp;".to_string(),
        c => c.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_compact() {
        assert_eq!(num(1.0), "1");
        assert_eq!(num(1.234_56), "1.23");
        assert_eq!(num(-0.001), "0");
        assert_eq!(num(f32::NAN), "0");
    }
}
