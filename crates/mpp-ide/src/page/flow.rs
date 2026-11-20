//! Flowing running text: words, spaces and inline maths broken into lines.

use mpp_render::{
    layout_math, CaretStop, Color, DisplayList, FontId, Fonts, LayoutOptions, Primitive, Theme,
};
use mpp_syntax::document::{Inline, InlineKind};
use mpp_syntax::math::MathAst;
use mpp_syntax::Span;

use super::Canvas;

/// Font, size and colour of a run of text.
#[derive(Debug, Clone, Copy)]
pub struct Style {
    pub font: FontId,
    pub size: f32,
    pub color: Color,
}

/// What the flow needs to know about the document.
pub struct Cx<'a> {
    pub src: &'a str,
    pub theme: Theme,
    /// Labels for the empty boxes of templates being edited, by offset.
    pub slot_labels: &'a [(usize, String)],
}

#[derive(Debug)]
enum Atom {
    Ch {
        ch: char,
        font: FontId,
        glyph: u16,
        size: f32,
        color: Color,
        adv: f32,
        start: Option<usize>,
        end: Option<usize>,
    },
    Space {
        adv: f32,
        size: f32,
        start: usize,
        end: usize,
    },
    Math {
        dl: DisplayList,
        span: Span,
    },
}

impl Atom {
    fn width(&self) -> f32 {
        match self {
            Atom::Ch { adv, .. } | Atom::Space { adv, .. } => *adv,
            Atom::Math { dl, .. } => dl.width,
        }
    }
}

/// The glyph for a character, falling back to the maths font.
pub fn glyph_for(font: FontId, ch: char) -> Option<(FontId, u16, f32)> {
    let fonts = Fonts::get();
    for f in [font, FontId::Text, FontId::Math] {
        if let Some(g) = fonts.glyph(f, ch) {
            return Some((f, g, fonts.metrics(f, g).advance));
        }
    }
    None
}

/// The width in em of a string in one font.
pub fn text_width(font: FontId, s: &str) -> f32 {
    s.chars()
        .filter_map(|c| glyph_for(font, c))
        .map(|(_, _, a)| a)
        .sum()
}

fn push_text(out: &mut Vec<Atom>, text: &str, style: Style, span: Span, mapped: bool) {
    let n = text.chars().count();
    for (i, (bi, ch)) in text.char_indices().enumerate() {
        let (start, end) = if mapped {
            (Some(span.start + bi), Some(span.start + bi + ch.len_utf8()))
        } else {
            (
                if i == 0 { Some(span.start) } else { None },
                if i + 1 == n { Some(span.end) } else { None },
            )
        };
        if ch.is_whitespace() {
            let adv = 0.33 * style.size;
            out.push(Atom::Space {
                adv,
                size: style.size,
                start: start.unwrap_or(span.start),
                end: end.unwrap_or(span.end),
            });
            continue;
        }
        let Some((font, glyph, adv)) = glyph_for(style.font, ch) else {
            continue;
        };
        out.push(Atom::Ch {
            ch,
            font,
            glyph,
            size: style.size,
            color: style.color,
            adv: adv * style.size,
            start,
            end,
        });
    }
}

fn bold(font: FontId) -> FontId {
    match font {
        FontId::Text | FontId::TextItalic => FontId::TextBold,
        f => f,
    }
}

fn italic(font: FontId) -> FontId {
    match font {
        FontId::Text => FontId::TextItalic,
        FontId::TextItalic => FontId::Text,
        f => f,
    }
}

fn collect(cx: &Cx<'_>, inlines: &[Inline], style: Style, out: &mut Vec<Atom>) {
    for inl in inlines {
        match &inl.kind {
            InlineKind::Text(t) => {
                let mapped = cx.src.get(inl.span.start..inl.span.end) == Some(t.as_str());
                push_text(out, t, style, inl.span, mapped);
            }
            InlineKind::SoftBreak => out.push(Atom::Space {
                adv: 0.33 * style.size,
                size: style.size,
                start: inl.span.start,
                end: inl.span.end,
            }),
            InlineKind::Emphasis(inner) => {
                let s = Style {
                    font: italic(style.font),
                    ..style
                };
                collect(cx, inner, s, out);
            }
            InlineKind::Strong(inner) => {
                let s = Style {
                    font: bold(style.font),
                    ..style
                };
                collect(cx, inner, s, out);
            }
            InlineKind::Math(m) => out.push(math_atom(cx, &m.ast, m.raw, style.size)),
            InlineKind::Code(c) => {
                let s = Style {
                    color: cx.theme.muted,
                    ..style
                };
                push_text(out, c, s, inl.span, false);
            }
            InlineKind::WikiLink { path, alias } => {
                let label = alias.clone().unwrap_or_else(|| display_name(path));
                let s = Style {
                    color: cx.theme.link,
                    ..style
                };
                push_text(out, &label, s, inl.span, false);
            }
            InlineKind::Link { text, .. } => {
                let s = Style {
                    color: cx.theme.link,
                    ..style
                };
                collect(cx, text, s, out);
            }
            InlineKind::FootnoteRef(name) => {
                let s = Style {
                    size: style.size * 0.7,
                    color: cx.theme.link,
                    ..style
                };
                push_text(out, name, s, inl.span, false);
            }
            InlineKind::EqRef { number, label } => {
                let t = match number {
                    Some(n) => format!("({n})"),
                    None => format!("({label})"),
                };
                let s = Style {
                    color: cx.theme.link,
                    ..style
                };
                push_text(out, &t, s, inl.span, false);
            }
            InlineKind::Escape(c) => {
                push_text(out, &c.to_string(), style, inl.span, false);
            }
        }
    }
}

/// A file name without folders or extension, for display.
pub fn display_name(path: &str) -> String {
    let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
    base.strip_suffix(".mpp").unwrap_or(base).to_string()
}

fn math_atom(cx: &Cx<'_>, ast: &MathAst, raw: Span, size: f32) -> Atom {
    let opts = LayoutOptions {
        theme: cx.theme,
        slot_labels: cx
            .slot_labels
            .iter()
            .filter(|(o, _)| raw.start <= *o && *o <= raw.end)
            .cloned()
            .collect(),
        ..LayoutOptions::inline(size)
    };
    Atom::Math {
        dl: layout_math(ast, &opts),
        span: raw,
    }
}

/// Lays `inlines` out in lines of at most `width` pixels starting at `(x0, y)`.
/// Returns the height used.
pub fn flow(
    cx: &Cx<'_>,
    inlines: &[Inline],
    style: Style,
    x0: f32,
    y: f32,
    width: f32,
    canvas: &mut Canvas,
) -> f32 {
    let mut atoms = vec![];
    collect(cx, inlines, style, &mut atoms);
    flow_atoms(&atoms, style, x0, y, width, canvas)
}

/// Lays out one plain string with a source mapping (used for verbatim lines).
pub fn flow_text(
    text: &str,
    span: Span,
    mapped: bool,
    style: Style,
    x0: f32,
    y: f32,
    canvas: &mut Canvas,
) -> f32 {
    let mut atoms = vec![];
    push_text(&mut atoms, text, style, span, mapped);
    flow_atoms(&atoms, style, x0, y, f32::MAX / 4.0, canvas)
}

fn flow_atoms(
    atoms: &[Atom],
    style: Style,
    x0: f32,
    y: f32,
    width: f32,
    canvas: &mut Canvas,
) -> f32 {
    // Break into lines at spaces.
    let mut lines: Vec<Vec<usize>> = vec![vec![]];
    let mut line_w = 0.0f32;
    let mut i = 0;
    while i < atoms.len() {
        if matches!(atoms[i], Atom::Space { .. }) {
            if let Some(l) = lines.last_mut() {
                l.push(i);
            }
            line_w += atoms[i].width();
            i += 1;
            continue;
        }
        let mut j = i;
        let mut unit_w = 0.0;
        while j < atoms.len() && !matches!(atoms[j], Atom::Space { .. }) {
            unit_w += atoms[j].width();
            j += 1;
        }
        let line_has_content = lines
            .last()
            .map(|l| l.iter().any(|&k| !matches!(atoms[k], Atom::Space { .. })))
            .unwrap_or(false);
        if line_has_content && line_w + unit_w > width {
            lines.push(vec![]);
            line_w = 0.0;
        }
        for k in i..j {
            if let Some(l) = lines.last_mut() {
                l.push(k);
            }
        }
        line_w += unit_w;
        i = j;
    }

    let mut cy = y;
    let mut total = 0.0;
    let n_lines = lines.len();
    for (li, line) in lines.iter().enumerate() {
        let mut size = style.size;
        let (mut asc, mut desc) = (0.0f32, 0.0f32);
        for &k in line {
            match &atoms[k] {
                Atom::Ch { size: s, .. } | Atom::Space { size: s, .. } => size = size.max(*s),
                Atom::Math { dl, .. } => {
                    asc = asc.max(dl.height + 2.0);
                    desc = desc.max(dl.depth + 2.0);
                }
            }
        }
        asc = asc.max(size * 0.98);
        desc = desc.max(size * 0.42);
        let line_h = (asc + desc).max(size * 1.5);
        let baseline = cy + (line_h - asc - desc) / 2.0 + asc;
        let mut x = x0;
        let last_line = li + 1 == n_lines;
        for (pos, &k) in line.iter().enumerate() {
            let top = baseline - size * 0.82;
            let bottom = baseline + size * 0.28;
            match &atoms[k] {
                Atom::Ch {
                    ch,
                    font,
                    glyph,
                    size: s,
                    color,
                    adv,
                    start,
                    end,
                } => {
                    if let (Some(a), Some(b)) = (start, end) {
                        canvas.chars.push(Span::new(*a, *b));
                    }
                    canvas.items.push(Primitive::Glyph {
                        font: *font,
                        glyph: *glyph,
                        ch: *ch,
                        x,
                        y: baseline,
                        size: *s,
                        color: *color,
                    });
                    if let Some(o) = start {
                        canvas.stops.push(CaretStop {
                            x,
                            top,
                            bottom,
                            offset: *o,
                        });
                    }
                    if let Some(o) = end {
                        canvas.stops.push(CaretStop {
                            x: x + adv,
                            top,
                            bottom,
                            offset: *o,
                        });
                    }
                    x += adv;
                }
                Atom::Space {
                    adv, start, end, ..
                } => {
                    canvas.chars.push(Span::new(*start, *end));
                    canvas.stops.push(CaretStop {
                        x,
                        top,
                        bottom,
                        offset: *start,
                    });
                    // A space that ends a wrapped line: the caret after it
                    // belongs to the start of the next line, so only the last
                    // stop of the line keeps the offset.
                    let end_of_line = pos + 1 == line.len() && !last_line;
                    if !end_of_line {
                        canvas.stops.push(CaretStop {
                            x: x + adv,
                            top,
                            bottom,
                            offset: *end,
                        });
                    }
                    x += adv;
                }
                Atom::Math { dl, span } => {
                    let dy = baseline - dl.baseline;
                    let before = canvas.stops.len();
                    canvas.merge(dl, x, dy, true);
                    if canvas.stops.len() == before {
                        canvas.stops.push(CaretStop {
                            x,
                            top: dy,
                            bottom: dy + dl.total_height(),
                            offset: span.start,
                        });
                    }
                    x += dl.width;
                }
            }
        }
        total += line_h;
        cy += line_h;
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use mpp_syntax::document::{parse_document, BlockKind};
    use mpp_syntax::Span;

    #[test]
    fn prose_has_stops_between_characters() {
        let src = "Hello *big* world.";
        let doc = parse_document(src);
        let BlockKind::Paragraph(content) = &doc.blocks[0].kind else {
            panic!("paragraph expected");
        };
        let cx = Cx {
            src,
            theme: Theme::LIGHT,
            slot_labels: &[],
        };
        let style = Style {
            font: FontId::Text,
            size: 18.0,
            color: Theme::LIGHT.text,
        };
        let mut canvas = Canvas::default();
        let h = flow(&cx, content, style, 0.0, 0.0, 400.0, &mut canvas);
        assert!(h > 18.0);
        // The asterisks are hidden: no visible character covers them.
        let offsets: Vec<usize> = canvas.stops.iter().map(|s| s.offset).collect();
        assert!(offsets.contains(&7), "start of 'big'");
        assert!(canvas
            .chars
            .iter()
            .all(|c| !src[c.start..c.end].contains('*')));
        let text: String = canvas
            .items
            .iter()
            .filter_map(|p| match p {
                Primitive::Glyph { ch, .. } => Some(*ch),
                _ => None,
            })
            .collect();
        assert_eq!(text, "Hellobigworld.");
        let _ = Span::new(0, 0);
    }

    #[test]
    fn long_text_wraps() {
        let src = "word ".repeat(60);
        let doc = parse_document(&src);
        let BlockKind::Paragraph(content) = &doc.blocks[0].kind else {
            panic!("paragraph expected");
        };
        let cx = Cx {
            src: &src,
            theme: Theme::LIGHT,
            slot_labels: &[],
        };
        let style = Style {
            font: FontId::Text,
            size: 18.0,
            color: Theme::LIGHT.text,
        };
        let mut canvas = Canvas::default();
        let h = flow(&cx, content, style, 0.0, 0.0, 300.0, &mut canvas);
        assert!(h > 18.0 * 3.0, "wrapped into several lines, got {h}");
    }
}
