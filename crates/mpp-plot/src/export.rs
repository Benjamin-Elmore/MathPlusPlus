//! SVG and PNG export of a [`PlotScene`].

use std::fmt::Write as _;
use std::sync::{Arc, OnceLock};

use crate::color::Color;
use crate::error::PlotError;
use crate::scene::{Anchor, Dash, Item, Label, Marker, MarkerKind, PlotScene, Polygon, Polyline};

/// Writes a number with at most two decimals, trimming zeros.
fn num(v: f32) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        "0".into()
    } else {
        s.into()
    }
}

fn paint(c: Color) -> String {
    c.to_hex()
}

fn opacity_attr(out: &mut String, name: &str, c: Color) {
    if c.a < 255 {
        let _ = write!(out, " {name}=\"{}\"", num(c.opacity()));
    }
}

fn escape(text: &str) -> String {
    let mut s = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => s.push_str("&amp;"),
            '<' => s.push_str("&lt;"),
            '>' => s.push_str("&gt;"),
            '"' => s.push_str("&quot;"),
            c if (c as u32) < 0x20 && c != '\n' => {}
            c => s.push(c),
        }
    }
    s
}

fn points_attr(points: &[[f32; 2]]) -> String {
    let mut s = String::new();
    for (i, p) in points.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        let _ = write!(s, "{},{}", num(p[0]), num(p[1]));
    }
    s
}

fn polyline(out: &mut String, l: &Polyline) {
    if l.points.len() < 2 {
        return;
    }
    let _ = write!(
        out,
        "<polyline points=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\" stroke-linejoin=\"round\" stroke-linecap=\"round\"",
        points_attr(&l.points),
        paint(l.color),
        num(l.width)
    );
    opacity_attr(out, "stroke-opacity", l.color);
    if let Dash::Dashed(a, b) = l.dash {
        let _ = write!(out, " stroke-dasharray=\"{} {}\"", num(a), num(b));
    }
    out.push_str("/>\n");
}

fn polygon(out: &mut String, p: &Polygon) {
    if p.points.len() < 3 {
        return;
    }
    let _ = write!(out, "<polygon points=\"{}\"", points_attr(&p.points));
    match p.fill {
        Some(c) => {
            let _ = write!(out, " fill=\"{}\"", paint(c));
            opacity_attr(out, "fill-opacity", c);
        }
        None => out.push_str(" fill=\"none\""),
    }
    if let Some((c, w)) = p.stroke {
        let _ = write!(
            out,
            " stroke=\"{}\" stroke-width=\"{}\" stroke-linejoin=\"round\"",
            paint(c),
            num(w)
        );
        opacity_attr(out, "stroke-opacity", c);
    }
    out.push_str("/>\n");
}

fn marker(out: &mut String, m: &Marker) {
    let r = m.size / 2.0;
    let (x, y) = (m.pos[0], m.pos[1]);
    let (fill, stroke_w) = if m.filled {
        (paint(m.color), 0.0)
    } else {
        (paint(m.hollow_fill), (m.size * 0.28).max(1.2))
    };
    let style = if m.filled {
        String::new()
    } else {
        format!(
            " stroke=\"{}\" stroke-width=\"{}\"",
            paint(m.color),
            num(stroke_w)
        )
    };
    match m.kind {
        MarkerKind::Circle => {
            let _ = writeln!(
                out,
                "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{fill}\"{style}/>",
                num(x),
                num(y),
                num(r)
            );
        }
        MarkerKind::Square => {
            let _ = writeln!(
                out,
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{fill}\"{style}/>",
                num(x - r),
                num(y - r),
                num(m.size),
                num(m.size)
            );
        }
        MarkerKind::Diamond => {
            let pts = [[x, y - r], [x + r, y], [x, y + r], [x - r, y]];
            let _ = writeln!(
                out,
                "<polygon points=\"{}\" fill=\"{fill}\"{style}/>",
                points_attr(&pts)
            );
        }
        MarkerKind::Cross => {
            let _ = writeln!(
                out,
                "<path d=\"M{} {}L{} {}M{} {}L{} {}\" stroke=\"{}\" stroke-width=\"1.6\" fill=\"none\"/>",
                num(x - r),
                num(y - r),
                num(x + r),
                num(y + r),
                num(x - r),
                num(y + r),
                num(x + r),
                num(y - r),
                paint(m.color)
            );
        }
    }
}

fn label(out: &mut String, l: &Label, font: &str, math_font: &str) {
    if l.text.is_empty() {
        return;
    }
    let anchor = match l.anchor {
        Anchor::Start => "start",
        Anchor::Middle => "middle",
        Anchor::End => "end",
    };
    let family = if l.latex.is_some() { math_font } else { font };
    let _ = write!(
        out,
        "<text x=\"{}\" y=\"{}\" font-family=\"{}\" font-size=\"{}\" text-anchor=\"{anchor}\" fill=\"{}\"",
        num(l.pos[0]),
        num(l.pos[1]),
        escape(family),
        num(l.size),
        paint(l.color)
    );
    if l.italic {
        out.push_str(" font-style=\"italic\"");
    }
    if l.rotation != 0.0 {
        let _ = write!(
            out,
            " transform=\"rotate({} {} {})\"",
            num(-l.rotation),
            num(l.pos[0]),
            num(l.pos[1])
        );
    }
    let _ = writeln!(out, ">{}</text>", escape(&l.text));
}

/// Renders the scene as a standalone SVG document.
pub fn to_svg(scene: &PlotScene) -> String {
    let (w, h) = (scene.width.max(1.0), scene.height.max(1.0));
    let mut out = String::with_capacity(4096 + scene.items.len() * 80);
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">",
        num(w),
        num(h),
        num(w),
        num(h)
    );
    let _ = writeln!(
        out,
        "<rect width=\"{}\" height=\"{}\" fill=\"{}\"/>",
        num(w),
        num(h),
        paint(scene.background)
    );
    let mut clip_id = 0usize;
    for item in &scene.items {
        match item {
            Item::Polyline(l) => polyline(&mut out, l),
            Item::Polygon(p) => polygon(&mut out, p),
            Item::Triangle(t) => {
                let c = t.flat_color();
                let (stroke, sw) = match t.edge {
                    Some((e, w)) => (e, w),
                    // A hairline in the fill colour hides the seams between triangles.
                    None => (c, 0.6),
                };
                let _ = write!(
                    out,
                    "<polygon points=\"{}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\" stroke-linejoin=\"round\"",
                    points_attr(&t.points),
                    paint(c),
                    paint(stroke),
                    num(sw)
                );
                opacity_attr(&mut out, "fill-opacity", c);
                opacity_attr(&mut out, "stroke-opacity", stroke);
                out.push_str("/>\n");
            }
            Item::Marker(m) => marker(&mut out, m),
            Item::Text(l) => label(&mut out, l, &scene.font, &scene.math_font),
            Item::ClipStart(r) => {
                clip_id += 1;
                let _ = writeln!(
                    out,
                    "<clipPath id=\"clip{clip_id}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath>\n<g clip-path=\"url(#clip{clip_id})\">",
                    num(r.x),
                    num(r.y),
                    num(r.w),
                    num(r.h)
                );
            }
            Item::ClipEnd => out.push_str("</g>\n"),
        }
    }
    // Close any clip groups left open so the document is well formed.
    let open = scene
        .items
        .iter()
        .filter(|i| matches!(i, Item::ClipStart(_)))
        .count() as isize
        - scene
            .items
            .iter()
            .filter(|i| matches!(i, Item::ClipEnd))
            .count() as isize;
    for _ in 0..open.max(0) {
        out.push_str("</g>\n");
    }
    out.push_str("</svg>\n");
    out
}

/// The system fonts, loaded once (scanning them takes a while).
fn system_fonts() -> Arc<resvg::usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = resvg::usvg::fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        })
        .clone()
}

/// Rasterises the scene to PNG bytes at `scale` times its pixel size.
pub fn to_png(scene: &PlotScene, scale: f32) -> Result<Vec<u8>, PlotError> {
    let svg = to_svg(scene);
    let options = resvg::usvg::Options {
        fontdb: system_fonts(),
        ..resvg::usvg::Options::default()
    };
    let tree = resvg::usvg::Tree::from_str(&svg, &options)
        .map_err(|e| PlotError::Export(format!("could not read the SVG: {e}")))?;
    let scale = if scale.is_finite() && scale > 0.0 {
        scale.min(8.0)
    } else {
        1.0
    };
    let w = (scene.width * scale).ceil().max(1.0) as u32;
    let h = (scene.height * scale).ceil().max(1.0) as u32;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)
        .ok_or_else(|| PlotError::Export(format!("a {w}×{h} image is too large")))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap
        .encode_png()
        .map_err(|e| PlotError::Export(format!("could not encode the PNG: {e}")))
}
