//! Figures: the graphs of `Figure:` blocks, drawn from `mpp-plot` scenes.
//!
//! [`Figures`] keeps one [`FigureState`] per figure of the last evaluation, with
//! the camera of a 3D figure, the window of a panned 2D figure and the slider
//! values, so they survive edits. [`paint_scene`] draws a [`PlotScene`] with the
//! page's own fonts: tick labels in Latin Modern Roman, maths labels typeset with
//! Latin Modern Math. [`figure_ui`] adds the mouse (orbit, pan, zoom), the sliders
//! and the figure menu.

use std::collections::BTreeMap;
use std::path::Path;

use egui::epaint::{Mesh, PathShape, PathStroke, Vertex, WHITE_UV};
use egui::{Color32, Id, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use mpp_eval::Evaluated;
use mpp_plot::scene::{Anchor, Dash, Item, MarkerKind};
use mpp_plot::{Bounds, Camera, Figure, PlotScene, PlotTheme, Slider};
use mpp_render::{FontId, Theme};
use mpp_syntax::document::BlockId;

use crate::paint::{c32, paint_display_list, paint_text};
use crate::theme::Mode;

/// One figure of the document with what the reader did to it.
#[derive(Debug)]
pub struct FigureState {
    pub block: BlockId,
    /// The figure, or why it could not be drawn (a plain sentence).
    pub figure: Result<Figure, String>,
    pub camera: Camera,
    /// The window of a 2D figure the reader panned or zoomed (`None`: automatic).
    pub view: Option<Bounds>,
    /// Slider values by variable name.
    pub values: BTreeMap<String, f64>,
}

impl FigureState {
    /// The sliders this figure uses, at their current values.
    pub fn sliders(&self) -> Vec<Slider> {
        let Ok(f) = &self.figure else {
            return vec![];
        };
        f.sliders()
            .into_iter()
            .map(|mut s| {
                if let Some(v) = self.values.get(&s.name) {
                    s.value = *v;
                }
                s
            })
            .collect()
    }

    /// What the figure computed for its helpers (a Riemann sum, an area, a tangent).
    pub fn readouts(&self) -> Vec<String> {
        self.figure
            .as_ref()
            .map(|f| f.readouts().to_vec())
            .unwrap_or_default()
    }

    /// Rows under the figure: one per slider, then one per readout.
    pub fn rows(&self) -> usize {
        self.sliders().len() + self.readouts().len()
    }

    /// Moves one slider and redraws the figure for it.
    pub fn set_value(&mut self, name: &str, value: f64) {
        self.values.insert(name.to_string(), value);
        if let Ok(f) = &mut self.figure {
            f.rebuild(&self.values);
        }
    }

    /// True for a 3D figure.
    pub fn is_3d(&self) -> bool {
        self.figure.as_ref().is_ok_and(Figure::is_3d)
    }

    /// The figure as a scene of `w × h` pixels.
    pub fn scene(&self, w: f32, h: f32, theme: &PlotTheme) -> Option<PlotScene> {
        let f = self.figure.as_ref().ok()?;
        Some(if f.is_3d() {
            f.scene_3d(&self.camera, w, h, theme)
        } else {
            f.scene_2d(w, h, theme)
        })
    }

    /// Back to the automatic window and the starting camera.
    pub fn reset_view(&mut self) {
        self.view = None;
        if let Ok(f) = &mut self.figure {
            f.set_view(None);
            self.camera = f.default_camera();
        }
    }
}

/// The figures of one document.
#[derive(Debug, Default)]
pub struct Figures {
    pub list: Vec<FigureState>,
}

impl Figures {
    /// Rebuilds the figures from a new evaluation, keeping each figure's camera,
    /// window and slider values (matched by position in the file).
    pub fn sync(&mut self, ev: &Evaluated) {
        let mut old = std::mem::take(&mut self.list);
        for (i, df) in mpp_plot::figures(ev).into_iter().enumerate() {
            let mut figure = df.figure.map_err(|e| e.to_string());
            let prev = (i < old.len()).then(|| old.swap_remove_keep(i)).flatten();
            let mut st = FigureState {
                block: df.block,
                camera: Camera::default(),
                view: None,
                values: BTreeMap::new(),
                figure: Err(String::new()),
            };
            if let Ok(f) = &mut figure {
                st.camera = f.default_camera();
                if let Some(p) = prev {
                    if p.is_3d() == f.is_3d() {
                        st.camera = p.camera;
                    }
                    let names: Vec<String> = f.sliders().into_iter().map(|s| s.name).collect();
                    st.values = p
                        .values
                        .into_iter()
                        .filter(|(n, _)| names.contains(n))
                        .collect();
                    if !f.is_3d() {
                        st.view = p.view;
                    }
                }
                if !st.values.is_empty() {
                    f.rebuild(&st.values);
                }
                if st.view.is_some() {
                    f.set_view(st.view);
                }
            }
            st.figure = figure;
            self.list.push(st);
        }
    }

    /// Slider rows per figure, for the page layout.
    pub fn rows(&self) -> Vec<(BlockId, usize)> {
        self.list
            .iter()
            .map(|f| (f.block, f.rows()))
            .filter(|(_, n)| *n > 0)
            .collect()
    }

    /// The figures drawn in 3D.
    pub fn three_d(&self) -> Vec<BlockId> {
        self.list
            .iter()
            .filter(|f| f.is_3d())
            .map(|f| f.block)
            .collect()
    }

    /// The figure of a block.
    pub fn of_block(&mut self, id: BlockId) -> Option<&mut FigureState> {
        self.list.iter_mut().find(|f| f.block == id)
    }
}

/// `Vec` helper: takes element `i` out, leaving a placeholder behind.
trait TakeAt<T> {
    fn swap_remove_keep(&mut self, i: usize) -> Option<T>;
}

impl TakeAt<FigureState> for Vec<FigureState> {
    fn swap_remove_keep(&mut self, i: usize) -> Option<FigureState> {
        let slot = self.get_mut(i)?;
        let empty = FigureState {
            block: slot.block,
            figure: Err(String::new()),
            camera: Camera::default(),
            view: None,
            values: BTreeMap::new(),
        };
        let taken = std::mem::replace(slot, empty);
        taken.figure.is_ok().then_some(taken)
    }
}

fn pc(c: mpp_render::display::Color) -> mpp_plot::Color {
    mpp_plot::Color {
        r: c.r,
        g: c.g,
        b: c.b,
        a: c.a,
    }
}

fn ec(c: mpp_plot::Color) -> Color32 {
    c32(mpp_render::display::Color {
        r: c.r,
        g: c.g,
        b: c.b,
        a: c.a,
    })
}

/// The plot theme for the page theme: plot colours from `mpp-plot`, with the
/// page's background, ink, grid and axes so a figure sits on the paper.
pub fn plot_theme(mode: Mode) -> PlotTheme {
    let page = mode.theme();
    let mut t = if mode == Mode::Dark {
        PlotTheme::dark()
    } else {
        PlotTheme::light()
    };
    t.background = pc(page.background);
    t.text = pc(page.text);
    t.axes = pc(page.axis);
    t.grid = pc(page.grid);
    t
}

/// Draws a scene with its top-left corner at `origin`.
pub fn paint_scene(painter: &egui::Painter, scene: &PlotScene, origin: Pos2, theme: Theme) {
    let at = |p: [f32; 2]| Pos2::new(origin.x + p[0], origin.y + p[1]);
    let full = painter.clip_rect();
    let mut clip: Option<Rect> = None;
    let mut tris = Mesh::default();
    let flush = |painter: &egui::Painter, tris: &mut Mesh, clip: Option<Rect>| {
        if !tris.is_empty() {
            let p = painter.with_clip_rect(clip.map_or(full, |c| c.intersect(full)));
            p.add(Shape::mesh(std::mem::take(tris)));
        }
    };
    for item in &scene.items {
        if !matches!(item, Item::Triangle(_)) {
            flush(painter, &mut tris, clip);
        }
        let p = painter.with_clip_rect(clip.map_or(full, |c| c.intersect(full)));
        match item {
            Item::ClipStart(r) => {
                clip = Some(Rect::from_min_size(at([r.x, r.y]), Vec2::new(r.w, r.h)));
            }
            Item::ClipEnd => clip = None,
            Item::Polyline(l) => {
                if l.points.len() < 2 {
                    continue;
                }
                let pts: Vec<Pos2> = l.points.iter().map(|q| at(*q)).collect();
                let stroke = Stroke::new(l.width, ec(l.color));
                match l.dash {
                    Dash::Solid => {
                        p.add(PathShape::line(pts, PathStroke::from(stroke)));
                    }
                    Dash::Dashed(on, off) => {
                        p.extend(Shape::dashed_line(&pts, stroke, on.max(1.0), off.max(1.0)));
                    }
                }
            }
            Item::Polygon(poly) => {
                if poly.points.len() < 3 {
                    continue;
                }
                let pts: Vec<Pos2> = poly.points.iter().map(|q| at(*q)).collect();
                if let Some(fill) = poly.fill {
                    p.add(Shape::mesh(fill_mesh(&pts, ec(fill))));
                }
                if let Some((c, w)) = poly.stroke {
                    p.add(PathShape::closed_line(
                        pts,
                        PathStroke::from(Stroke::new(w, ec(c))),
                    ));
                }
            }
            Item::Triangle(t) => {
                let base = tris.vertices.len() as u32;
                for (q, c) in t.points.iter().zip(t.colors) {
                    tris.vertices.push(Vertex {
                        pos: at(*q),
                        uv: WHITE_UV,
                        color: ec(c),
                    });
                }
                tris.indices.extend([base, base + 1, base + 2]);
                if let Some((c, w)) = t.edge {
                    let pts: Vec<Pos2> = t.points.iter().map(|q| at(*q)).collect();
                    flush(painter, &mut tris, clip);
                    p.add(PathShape::closed_line(
                        pts,
                        PathStroke::from(Stroke::new(w, ec(c))),
                    ));
                }
            }
            Item::Marker(m) => paint_marker(&p, m, at(m.pos)),
            Item::Text(label) => paint_label(&p, label, at(label.pos), theme),
        }
    }
    flush(painter, &mut tris, clip);
}

/// A filled polygon of any shape (regions between curves are not convex).
fn fill_mesh(pts: &[Pos2], color: Color32) -> Mesh {
    use lyon_tessellation::geometry_builder::{BuffersBuilder, VertexBuffers};
    use lyon_tessellation::math::point;
    use lyon_tessellation::path::Path;
    use lyon_tessellation::{FillOptions, FillTessellator, FillVertex};
    let mut b = Path::builder();
    b.begin(point(pts[0].x, pts[0].y));
    for q in &pts[1..] {
        b.line_to(point(q.x, q.y));
    }
    b.end(true);
    let path = b.build();
    let mut buffers: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
    let mut mesh = Mesh::default();
    if FillTessellator::new()
        .tessellate_path(
            &path,
            &FillOptions::tolerance(0.1),
            &mut BuffersBuilder::new(&mut buffers, |v: FillVertex| v.position().to_array()),
        )
        .is_err()
    {
        return mesh;
    }
    mesh.vertices = buffers
        .vertices
        .iter()
        .map(|v| Vertex {
            pos: Pos2::new(v[0], v[1]),
            uv: WHITE_UV,
            color,
        })
        .collect();
    mesh.indices = buffers.indices;
    mesh
}

fn paint_marker(p: &egui::Painter, m: &mpp_plot::scene::Marker, c: Pos2) {
    let r = m.size / 2.0;
    let color = ec(m.color);
    let stroke = Stroke::new(1.4, color);
    let fill = if m.filled { color } else { ec(m.hollow_fill) };
    match m.kind {
        MarkerKind::Circle => {
            p.circle(c, r, fill, stroke);
        }
        MarkerKind::Square => {
            let rect = Rect::from_center_size(c, Vec2::splat(m.size));
            p.rect(rect, 0.0, fill, stroke, egui::StrokeKind::Middle);
        }
        MarkerKind::Diamond => {
            let pts = vec![
                c + Vec2::new(0.0, -r),
                c + Vec2::new(r, 0.0),
                c + Vec2::new(0.0, r),
                c + Vec2::new(-r, 0.0),
            ];
            p.add(PathShape::convex_polygon(pts, fill, stroke));
        }
        MarkerKind::Cross => {
            p.line_segment([c + Vec2::new(-r, -r), c + Vec2::new(r, r)], stroke);
            p.line_segment([c + Vec2::new(-r, r), c + Vec2::new(r, -r)], stroke);
        }
    }
}

fn paint_label(p: &egui::Painter, label: &mpp_plot::scene::Label, pos: Pos2, theme: Theme) {
    let color = mpp_render::display::Color {
        r: label.color.r,
        g: label.color.g,
        b: label.color.b,
        a: label.color.a,
    };
    let shift = |w: f32| match label.anchor {
        Anchor::Start => 0.0,
        Anchor::Middle => w / 2.0,
        Anchor::End => w,
    };
    match &label.latex {
        Some(tex) => {
            let dl = crate::typeset::layout(
                tex,
                label.size,
                Theme {
                    text: color,
                    ..theme
                },
            );
            let top = pos.y - dl.baseline;
            paint_display_list(p, &dl, Pos2::new(pos.x - shift(dl.width), top));
        }
        None => {
            let font = if label.italic {
                FontId::TextItalic
            } else {
                FontId::Text
            };
            let w = crate::page::text_width(font, &label.text) * label.size;
            paint_text(
                p,
                &label.text,
                Pos2::new(pos.x - shift(w), pos.y),
                label.size,
                font,
                c32(color),
            );
        }
    }
}

/// What the figure menu asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FigureAction {
    OpenInTile,
    ExportPng,
    ExportSvg,
}

/// Draws a figure in `rect` and handles the mouse: drag orbits a 3D figure (with
/// Shift or the right button it pans) and pans a 2D one; Ctrl+scroll or a pinch
/// zooms (plain scroll too when `scroll_zooms`, as in a graph tile); a double
/// click resets. The right-click menu offers a tile and the exports.
pub fn figure_ui(
    ui: &Ui,
    rect: Rect,
    id: Id,
    st: &mut FigureState,
    mode: Mode,
    scroll_zooms: bool,
) -> Option<FigureAction> {
    let theme = mode.theme();
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    let mut action = None;
    if let Ok(f) = &mut st.figure {
        if resp.dragged() {
            let d = resp.drag_delta();
            let pan =
                ui.input(|i| i.modifiers.shift) || resp.dragged_by(egui::PointerButton::Secondary);
            if f.is_3d() {
                if pan {
                    st.camera.pan_by(d.x, d.y);
                } else {
                    st.camera.orbit(d.x, d.y);
                }
            } else if d != Vec2::ZERO {
                let b = f.layout_2d(rect.width(), rect.height()).panned(d.x, d.y);
                f.set_view(Some(b));
                st.view = Some(b);
            }
        }
        if resp.hovered() {
            let (zoom, scroll) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta.y));
            let mut factor = zoom as f64;
            if scroll_zooms && (factor - 1.0).abs() < 1e-6 && scroll.abs() > 0.0 {
                factor = (f64::from(scroll) * 0.004).exp();
            }
            if (factor - 1.0).abs() > 1e-6 {
                if f.is_3d() {
                    st.camera.zoom_by(factor);
                } else {
                    let about = resp
                        .hover_pos()
                        .map_or([rect.width() / 2.0, rect.height() / 2.0], |p| {
                            [p.x - rect.left(), p.y - rect.top()]
                        });
                    let b = f
                        .layout_2d(rect.width(), rect.height())
                        .zoomed(1.0 / factor, about);
                    f.set_view(Some(b));
                    st.view = Some(b);
                }
            }
        }
        if resp.double_clicked() {
            st.reset_view();
        }
    }
    let painter = ui.painter_at(rect);
    match st.scene(rect.width(), rect.height(), &plot_theme(mode)) {
        Some(scene) => paint_scene(&painter, &scene, rect.min, theme),
        None => paint_message(&painter, rect, &st.figure, theme),
    }
    resp.context_menu(|ui| {
        let item = |ui: &mut Ui, text: &str| {
            ui.button(egui::RichText::new(text).font(crate::theme::sans(14.0)))
                .clicked()
        };
        if item(ui, "Open in tile") {
            action = Some(FigureAction::OpenInTile);
            ui.close();
        }
        if item(ui, "Reset view") {
            st.reset_view();
            ui.close();
        }
        ui.separator();
        if item(ui, "Export as PNG\u{2026}") {
            action = Some(FigureAction::ExportPng);
            ui.close();
        }
        if item(ui, "Export as SVG\u{2026}") {
            action = Some(FigureAction::ExportSvg);
            ui.close();
        }
    });
    action
}

/// A figure that cannot be drawn: a quiet box with the reason, typeset.
fn paint_message(
    painter: &egui::Painter,
    rect: Rect,
    figure: &Result<Figure, String>,
    theme: Theme,
) {
    painter.rect_filled(rect, 6.0, c32(theme.code_background));
    let why = match figure {
        Err(m) if !m.is_empty() => m.clone(),
        _ => "This figure cannot be drawn yet.".to_string(),
    };
    let size = 15.0;
    let text: String = why
        .chars()
        .filter(|c| !matches!(c, '\\' | '$' | '_' | '^' | '`' | '{' | '}'))
        .collect();
    let dl = crate::typeset::layout(
        &format!("\\text{{{text}}}"),
        size,
        Theme {
            text: theme.muted,
            ..theme
        },
    );
    let at = Pos2::new(
        rect.center().x - dl.width.min(rect.width()) / 2.0,
        rect.center().y - dl.total_height() / 2.0,
    );
    paint_display_list(&painter.with_clip_rect(rect), &dl, at);
}

/// The slider rows under a figure: the variable typeset, then the slider. Returns
/// true when a value changed (the figure is redrawn already).
pub fn sliders_ui(
    ui: &mut Ui,
    top_left: Pos2,
    width: f32,
    size: f32,
    st: &mut FigureState,
    mode: Mode,
) -> bool {
    let theme = mode.theme();
    let row_h = crate::page::slider_row_height(size);
    let mut changed = None;
    for (i, s) in st.sliders().into_iter().enumerate() {
        let y = top_left.y + i as f32 * row_h;
        let tex = mpp_plot::view2d::name_latex(&s.name);
        let dl = crate::typeset::layout(&tex, size * 0.95, theme);
        let label_w = size * 3.0;
        let ty = y + (row_h - dl.total_height()) / 2.0;
        paint_display_list(
            ui.painter(),
            &dl,
            Pos2::new(top_left.x + label_w - dl.width - size * 0.3, ty),
        );
        let r = Rect::from_min_size(
            Pos2::new(top_left.x + label_w, y + row_h * 0.15),
            Vec2::new((width - label_w).max(size * 4.0), row_h * 0.7),
        );
        let mut v = s.value;
        let step = if s.integer { 1.0 } else { s.step() };
        ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
            ui.spacing_mut().slider_width = (r.width() - size * 4.5).max(size * 3.0);
            let slider = egui::Slider::new(&mut v, s.min..=s.max)
                .step_by(step)
                .show_value(false)
                .clamping(egui::SliderClamping::Always);
            let resp = ui.add(slider);
            let name = s.name.clone();
            ui.ctx()
                .accesskit_node_builder(resp.id, |n| n.set_label(format!("slider for {name}")));
            if resp.changed() {
                changed = Some((s.name.clone(), v));
            }
            // The value, typeset, after the slider.
            let shown = if s.integer || step >= 1.0 {
                format!("{}", v.round())
            } else {
                let t = format!("{v:.3}");
                t.trim_end_matches('0').trim_end_matches('.').to_string()
            };
            let vd = crate::typeset::layout(&shown, size * 0.9, theme);
            let at = Pos2::new(
                resp.rect.right() + size * 0.6,
                resp.rect.center().y - vd.total_height() / 2.0,
            );
            paint_display_list(ui.painter(), &vd, at);
        });
    }
    // The helpers' readouts, typeset: the label as words, the rest as maths.
    let base = top_left.y + st.sliders().len() as f32 * row_h;
    for (i, r) in st.readouts().iter().enumerate() {
        let tex = readout_latex(r);
        let dl = crate::typeset::layout(&tex, size * 0.9, theme);
        let y = base + i as f32 * row_h + (row_h - dl.total_height()) / 2.0;
        let x = top_left.x + ((width - dl.width) / 2.0).max(0.0);
        paint_display_list(ui.painter(), &dl, Pos2::new(x, y));
    }
    match changed {
        Some((name, v)) => {
            st.set_value(&name, v);
            true
        }
        None => false,
    }
}

/// A readout such as `tangent at x = 1: y = 2x - 1` as LaTeX: words before the
/// colon in text, the rest as maths.
pub fn readout_latex(r: &str) -> String {
    let clean = |s: &str| {
        s.chars()
            .filter(|c| !matches!(c, '\\' | '$' | '{' | '}' | '`'))
            .collect::<String>()
    };
    match r.split_once(':') {
        Some((label, rest)) => {
            let (words, maths) = split_words(label.trim());
            let mut out = format!("\\text{{{}}}", clean(words));
            if !maths.is_empty() {
                out.push_str(&format!("\\; {}", clean(maths)));
            }
            out.push_str(&format!("\\text{{:}}\\; {}", clean(rest.trim())));
            out
        }
        None => format!("\\text{{{}}}", clean(r)),
    }
}

/// `tangent at x = 1` → (`tangent at`, `x = 1`): the leading words and the maths.
fn split_words(s: &str) -> (&str, &str) {
    let mut end = 0;
    for w in s.split(' ') {
        if w.chars().count() > 2 && w.chars().all(char::is_alphabetic) {
            end += w.len() + 1;
        } else {
            break;
        }
    }
    let end = end.min(s.len());
    (s[..end].trim(), s[end..].trim())
}

/// Writes the figure to a PNG or SVG file (by the extension), at a print size.
pub fn export(st: &FigureState, path: &Path, mode: Mode) -> Result<(), String> {
    let (w, h) = if st.is_3d() {
        (720.0, 560.0)
    } else {
        (800.0, 500.0)
    };
    let scene = st
        .scene(w, h, &plot_theme(mode))
        .ok_or_else(|| "This figure cannot be drawn yet.".to_string())?;
    let svg = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"));
    let bytes = if svg {
        mpp_plot::to_svg(&scene).into_bytes()
    } else {
        mpp_plot::to_png(&scene, 2.0).map_err(|e| e.to_string())?
    };
    std::fs::write(path, bytes).map_err(|e| format!("The figure could not be saved: {e}."))
}
