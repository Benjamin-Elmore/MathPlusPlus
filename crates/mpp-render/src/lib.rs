//! Maths typesetting for Math++.
//!
//! [`layout_math`] sets a parsed maths span the way a paper does, with a
//! TeX-style box model driven by the OpenType `MATH` table of the embedded Latin
//! Modern Math font, and returns a [`DisplayList`]: glyphs, rules and slot outlines
//! in pixels, plus hit boxes that map every box back to its source span and
//! node path (for clicks and the caret in the IDE). [`to_svg`] draws a display
//! list as a self-contained SVG, and [`render_html`] turns a whole document into
//! a static page.
//!
//! Results of `= ?` queries are passed in as [`LineResult`]s (or, for a whole
//! page, through the [`ResultProvider`] trait), so the evaluator plugs in
//! without this crate depending on it.

pub mod display;
pub mod font;
mod html;
mod layout;
mod svg;
pub mod theme;

pub use display::{
    CaretStop, Color, DisplayList, HitBox, HitKind, NodePath, PathCmd, Primitive, Rect,
};
pub use font::{FontId, Fonts};
pub use html::{render_html, render_html_with, HtmlOptions, NoResults, ResultProvider};
pub use layout::{
    layout_latex, layout_math, layout_node, IntegralLimits, LayoutOptions, LineResult, MathStyle,
    ResultKind, ResultValue,
};
pub use svg::{to_svg, to_svg_with, SvgOptions};
pub use theme::Theme;

/// The node a [`NodePath`] from a hit box points at.
pub fn node_at_path<'a>(
    ast: &'a mpp_syntax::math::MathAst,
    path: &NodePath,
) -> Option<&'a mpp_syntax::math::Node> {
    layout::node_at_path(ast, path)
}
