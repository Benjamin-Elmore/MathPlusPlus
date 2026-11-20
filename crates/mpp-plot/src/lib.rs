//! Math++ graphs: 2D curves, implicit curves, shaded inequalities and 3D surfaces as a
//! backend-neutral display list.
//!
//! A [`Figure`] is built from a `Figure:` block and the evaluated session
//! ([`figures`]), or by hand from a [`FigureSpec`]. It samples itself once, again when a
//! slider moves ([`Figure::rebuild`]) and draws into a [`PlotScene`] for a viewport
//! ([`Figure::scene_2d`], [`Figure::scene_3d`]). The IDE paints the scene with egui;
//! [`to_svg`] and [`to_png`] export it. Every colour comes from a [`PlotTheme`].

#![forbid(unsafe_code)]

pub mod build;
pub mod camera;
pub mod color;
pub mod compile;
pub mod contour;
pub mod data;
pub mod error;
pub mod export;
pub mod figure;
pub mod sample;
pub mod scene;
pub mod spec;
pub mod theme;
pub mod ticks;
pub mod view2d;
pub mod view3d;

pub use build::{figure_for_block, figures, spec_for_block, DocFigure};
pub use camera::{Camera, Projected};
pub use color::Color;
pub use compile::Defs;
pub use contour::Bounds;
pub use error::PlotError;
pub use export::{to_png, to_svg};
pub use figure::{Figure, LayerData, Mesh, Quality};
pub use scene::{
    Anchor, Dash, Item, Label as SceneLabel, Marker, MarkerKind, PlotScene, Polygon, Polyline,
    Rect, Triangle,
};
pub use spec::{FigureSpec, Geometry, Label, Layer, Slider};
pub use theme::PlotTheme;
pub use view2d::Layout2d;
pub use view3d::{Options3d, SurfaceStyle};
