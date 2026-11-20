//! Painting glyph outlines and display lists as egui meshes.
//!
//! mpp-render describes a formula as glyphs (font + glyph id), rules and paths.
//! Glyph outlines are tessellated once (with lyon) into triangles in em units
//! and cached; painting a glyph is then a cheap transform of those triangles
//! plus a hair-line stroke along the contours, which gives the edges the same
//! anti-aliasing as egui's own shapes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

pub use crate::theme::c32;
use egui::epaint::{Mesh, PathShape, PathStroke};
use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2};
use lyon_tessellation::geom::point;
use lyon_tessellation::path::iterator::PathIterator;
use lyon_tessellation::path::Path;
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, VertexBuffers,
};
use mpp_render::{DisplayList, FontId, Fonts, PathCmd, Primitive};

/// Tessellated glyph outline in em units, y growing downwards from the baseline.
#[derive(Debug, Default)]
pub struct GlyphMesh {
    pub vertices: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    /// Closed contours (flattened), for the anti-aliasing fringe.
    pub contours: Vec<Vec<[f32; 2]>>,
}

/// How much thicker glyph edges are painted in dark mode.
const DARK_INK_BOOST: f32 = 2.6;

type MeshCache = Mutex<HashMap<(FontId, u16), Arc<GlyphMesh>>>;

fn cache() -> &'static MeshCache {
    static CACHE: OnceLock<MeshCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The cached mesh of a glyph (empty for glyphs without an outline).
pub fn glyph_mesh(font: FontId, glyph: u16) -> Arc<GlyphMesh> {
    if let Ok(mut map) = cache().lock() {
        if let Some(m) = map.get(&(font, glyph)) {
            return m.clone();
        }
        let mesh = Arc::new(build_mesh(font, glyph));
        map.insert((font, glyph), mesh.clone());
        return mesh;
    }
    Arc::new(build_mesh(font, glyph))
}

/// Parses `M x y L x y Q a b c d C … Z` path data (absolute, integer units).
fn parse_svg_path(d: &str) -> Vec<(char, Vec<f32>)> {
    let mut out: Vec<(char, Vec<f32>)> = vec![];
    let mut cur: Option<(char, String)> = None;
    let flush = |cur: &mut Option<(char, String)>, out: &mut Vec<(char, Vec<f32>)>| {
        if let Some((c, s)) = cur.take() {
            let nums = s
                .split_whitespace()
                .filter_map(|t| t.parse::<f32>().ok())
                .collect();
            out.push((c, nums));
        }
    };
    for ch in d.chars() {
        if ch.is_ascii_alphabetic() {
            flush(&mut cur, &mut out);
            cur = Some((ch, String::new()));
        } else if let Some((_, s)) = cur.as_mut() {
            s.push(ch);
        }
    }
    flush(&mut cur, &mut out);
    out
}

fn build_mesh(font: FontId, glyph: u16) -> GlyphMesh {
    let fonts = Fonts::get();
    let Some(data) = fonts.outline(font, glyph) else {
        return GlyphMesh::default();
    };
    let upem = fonts.units_per_em(font).max(1.0);
    let s = 1.0 / upem;
    let mut b = Path::builder();
    let mut open = false;
    for (cmd, n) in parse_svg_path(&data) {
        let p = |i: usize| {
            point(
                n.get(i).copied().unwrap_or(0.0) * s,
                -n.get(i + 1).copied().unwrap_or(0.0) * s,
            )
        };
        match cmd {
            'M' => {
                if open {
                    b.end(true);
                }
                b.begin(p(0));
                open = true;
            }
            'L' if open => {
                b.line_to(p(0));
            }
            'Q' if open => {
                b.quadratic_bezier_to(p(0), p(2));
            }
            'C' if open => {
                b.cubic_bezier_to(p(0), p(2), p(4));
            }
            'Z' if open => {
                b.end(true);
                open = false;
            }
            _ => {}
        }
    }
    if open {
        b.end(true);
    }
    let path = b.build();

    let mut buffers: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
    let mut tess = FillTessellator::new();
    let ok = tess
        .tessellate_path(
            &path,
            &FillOptions::tolerance(0.0006).with_fill_rule(FillRule::NonZero),
            &mut BuffersBuilder::new(&mut buffers, |v: FillVertex| v.position().to_array()),
        )
        .is_ok();
    if !ok {
        return GlyphMesh::default();
    }

    let mut contours: Vec<Vec<[f32; 2]>> = vec![];
    let mut current: Vec<[f32; 2]> = vec![];
    for ev in path.iter().flattened(0.0006) {
        use lyon_tessellation::path::Event;
        match ev {
            Event::Begin { at } => {
                current = vec![at.to_array()];
            }
            Event::Line { to, .. } => current.push(to.to_array()),
            Event::End { .. } if current.len() > 2 => {
                contours.push(std::mem::take(&mut current));
            }
            _ => {}
        }
    }
    GlyphMesh {
        vertices: buffers.vertices,
        indices: buffers.indices,
        contours,
    }
}

/// Paints one glyph with its baseline origin at `origin`.
pub fn paint_glyph(
    painter: &Painter,
    font: FontId,
    glyph: u16,
    origin: Pos2,
    size: f32,
    color: Color32,
) {
    let g = glyph_mesh(font, glyph);
    if g.indices.is_empty() {
        return;
    }
    let mut mesh = Mesh::default();
    mesh.vertices.reserve(g.vertices.len());
    for v in &g.vertices {
        mesh.colored_vertex(
            Pos2::new(origin.x + v[0] * size, origin.y + v[1] * size),
            color,
        );
    }
    mesh.indices.extend_from_slice(&g.indices);
    painter.add(Shape::mesh(mesh));
    // Light ink on a dark page looks thinner than dark ink on a light one, so
    // dark mode thickens the edge a little: maths then matches the prose weight.
    let boost = if painter.ctx().global_style().visuals.dark_mode {
        DARK_INK_BOOST
    } else {
        1.0
    };
    let width = (size * 0.012).clamp(0.25, 0.6) * boost;
    for c in &g.contours {
        let pts: Vec<Pos2> = c
            .iter()
            .map(|v| Pos2::new(origin.x + v[0] * size, origin.y + v[1] * size))
            .collect();
        painter.add(Shape::Path(PathShape::closed_line(
            pts,
            PathStroke::new(width, color),
        )));
    }
}

/// Paints a string with its baseline starting at `origin`; returns its width.
pub fn paint_text(
    painter: &Painter,
    text: &str,
    origin: Pos2,
    size: f32,
    font: FontId,
    color: Color32,
) -> f32 {
    let mut x = origin.x;
    for ch in text.chars() {
        if let Some((f, g, adv)) = crate::page::glyph_for(font, ch) {
            paint_glyph(painter, f, g, Pos2::new(x, origin.y), size, color);
            x += adv * size;
        }
    }
    x - origin.x
}

/// Paints a whole display list with its top-left corner at `origin`.
pub fn paint_display_list(painter: &Painter, dl: &DisplayList, origin: Pos2) {
    for item in &dl.items {
        match item {
            Primitive::Glyph {
                font,
                glyph,
                x,
                y,
                size,
                color,
                ..
            } => paint_glyph(
                painter,
                *font,
                *glyph,
                origin + Vec2::new(*x, *y),
                *size,
                c32(*color),
            ),
            Primitive::Rule { rect, color } => {
                let r = Rect::from_min_size(
                    origin + Vec2::new(rect.x, rect.y),
                    Vec2::new(rect.w, rect.h.max(0.8)),
                );
                painter.rect_filled(r, 0.0, c32(*color));
            }
            Primitive::Path {
                cmds,
                color,
                stroke,
            } => paint_path(painter, cmds, c32(*color), *stroke, origin),
            Primitive::Outline {
                rect,
                color,
                dotted,
            } => {
                let r = Rect::from_min_size(
                    origin + Vec2::new(rect.x, rect.y),
                    Vec2::new(rect.w, rect.h),
                );
                paint_outline(painter, r, c32(*color), *dotted);
            }
        }
    }
}

fn paint_outline(painter: &Painter, r: Rect, color: Color32, dotted: bool) {
    let stroke = Stroke::new(1.0, color);
    if dotted {
        let corners = [
            r.left_top(),
            r.right_top(),
            r.right_bottom(),
            r.left_bottom(),
            r.left_top(),
        ];
        for w in corners.windows(2) {
            painter.extend(Shape::dashed_line(&[w[0], w[1]], stroke, 2.5, 2.5));
        }
    } else {
        painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Inside);
    }
}

fn paint_path(
    painter: &Painter,
    cmds: &[PathCmd],
    color: Color32,
    stroke: Option<f32>,
    origin: Pos2,
) {
    let at = |x: f32, y: f32| Pos2::new(origin.x + x, origin.y + y);
    let mut contours: Vec<Vec<Pos2>> = vec![];
    let mut cur: Vec<Pos2> = vec![];
    let mut last = Pos2::ZERO;
    for c in cmds {
        match *c {
            PathCmd::MoveTo(x, y) => {
                if !cur.is_empty() {
                    contours.push(std::mem::take(&mut cur));
                }
                last = at(x, y);
                cur.push(last);
            }
            PathCmd::LineTo(x, y) => {
                last = at(x, y);
                cur.push(last);
            }
            PathCmd::QuadTo(a, b, x, y) => {
                let (p0, p1, p2) = (last, at(a, b), at(x, y));
                for i in 1..=12 {
                    let t = i as f32 / 12.0;
                    let u = 1.0 - t;
                    cur.push(Pos2::new(
                        u * u * p0.x + 2.0 * u * t * p1.x + t * t * p2.x,
                        u * u * p0.y + 2.0 * u * t * p1.y + t * t * p2.y,
                    ));
                }
                last = p2;
            }
            PathCmd::CubicTo(a, b, c2, d, x, y) => {
                let (p0, p1, p2, p3) = (last, at(a, b), at(c2, d), at(x, y));
                for i in 1..=16 {
                    let t = i as f32 / 16.0;
                    let u = 1.0 - t;
                    cur.push(Pos2::new(
                        u * u * u * p0.x
                            + 3.0 * u * u * t * p1.x
                            + 3.0 * u * t * t * p2.x
                            + t * t * t * p3.x,
                        u * u * u * p0.y
                            + 3.0 * u * u * t * p1.y
                            + 3.0 * u * t * t * p2.y
                            + t * t * t * p3.y,
                    ));
                }
                last = p3;
            }
            PathCmd::Close => {
                if !cur.is_empty() {
                    contours.push(std::mem::take(&mut cur));
                }
            }
        }
    }
    if !cur.is_empty() {
        contours.push(cur);
    }
    for pts in contours {
        match stroke {
            Some(w) => {
                painter.add(Shape::line(pts, Stroke::new(w, color)));
            }
            None => {
                // Filled paths in a display list are small convex shapes (arrow heads).
                painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_path_data() {
        let cmds = parse_svg_path("M10 20L30 40Q1 2 3 4Z");
        assert_eq!(cmds.len(), 4);
        assert_eq!(cmds[0], ('M', vec![10.0, 20.0]));
        assert_eq!(cmds[2].1.len(), 4);
    }

    #[test]
    fn letters_have_triangles() {
        let fonts = Fonts::get();
        let g = fonts.glyph(FontId::Text, 'o').expect("glyph o");
        let m = glyph_mesh(FontId::Text, g);
        assert!(!m.indices.is_empty());
        assert!(m.contours.len() >= 2, "o has an outer and an inner contour");
    }
}
