//! Maths layout: turns the syntax tree into boxes, then into a [`DisplayList`].
//!
//! The engine follows TeX's rules (styles, atom spacing, fractions, scripts,
//! radicals, limits, delimiters) with every dimension taken from the OpenType
//! `MATH` table of Latin Modern Math.

mod boxes;
mod chars;
mod engine;
mod nodes;
mod statement;
pub(crate) mod style;

pub(crate) use statement::node_at_path;

use mpp_syntax::math::{parse_expr, parse_math, MathAst, Node, RelOp};

use crate::display::DisplayList;
use crate::font::Fonts;
use crate::theme::Theme;

/// Display (an equation on its own line) or text (inline) style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MathStyle {
    #[default]
    Display,
    Text,
}

/// Where the bounds of an integral go in display style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IntegralLimits {
    /// Above and below the sign, like a sum (the default).
    #[default]
    AboveBelow,
    /// At the top-right and bottom-right of the sign.
    Side,
}

/// How a result is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultKind {
    /// The answer to a `= ?` query, in the theme's result colour.
    Filled,
    /// An automatic result nobody asked for, in the theme's grey.
    Ghost,
}

/// The value of a result.
#[derive(Debug, Clone, PartialEq)]
pub enum ResultValue {
    /// A syntax tree (spans are ignored).
    Node(Node),
    /// LaTeX or plain maths text, parsed with the Math++ parser.
    Latex(String),
}

/// A result to show on one statement line.
#[derive(Debug, Clone, PartialEq)]
pub struct LineResult {
    /// Index of the statement line in the maths block.
    pub line: usize,
    pub value: ResultValue,
    pub kind: ResultKind,
    /// The relation in front of the value. `None` uses the query's own `=` or
    /// `≈` (replacing the `?`), or `=` after any other line.
    pub relation: Option<RelOp>,
}

impl LineResult {
    /// A filled-in exact result for line `line`.
    pub fn filled(line: usize, latex: impl Into<String>) -> Self {
        LineResult {
            line,
            value: ResultValue::Latex(latex.into()),
            kind: ResultKind::Filled,
            relation: None,
        }
    }

    /// An automatic grey result for line `line`.
    pub fn ghost(line: usize, latex: impl Into<String>) -> Self {
        LineResult {
            line,
            value: ResultValue::Latex(latex.into()),
            kind: ResultKind::Ghost,
            relation: None,
        }
    }
}

/// Options for laying out one maths span.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutOptions {
    pub style: MathStyle,
    /// Pixels per em.
    pub font_size: f32,
    pub integral_limits: IntegralLimits,
    /// Every colour (ink, results, slots) comes from here.
    pub theme: Theme,
    /// Shown as `(n)` at the right of a display equation.
    pub equation_number: Option<String>,
    /// Line width in pixels: a display equation is centred in it and its number
    /// set flush right. `None` makes the box as wide as the formula.
    pub width: Option<f32>,
    /// Results to fill in, by line.
    pub results: Vec<LineResult>,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        LayoutOptions {
            style: MathStyle::Display,
            font_size: 20.0,
            integral_limits: IntegralLimits::AboveBelow,
            theme: Theme::LIGHT,
            equation_number: None,
            width: None,
            results: vec![],
        }
    }
}

impl LayoutOptions {
    /// Inline (text style) options at a font size.
    pub fn inline(font_size: f32) -> Self {
        LayoutOptions {
            style: MathStyle::Text,
            font_size,
            ..LayoutOptions::default()
        }
    }

    /// Display options at a font size.
    pub fn display(font_size: f32) -> Self {
        LayoutOptions {
            font_size,
            ..LayoutOptions::default()
        }
    }

    fn sanitized_size(&self) -> f32 {
        if self.font_size.is_finite() && self.font_size > 0.0 {
            self.font_size.min(10_000.0)
        } else {
            20.0
        }
    }
}

/// Lays out a parsed maths span (all its lines, clauses and results).
pub fn layout_math(ast: &MathAst, opts: &LayoutOptions) -> DisplayList {
    let mut engine = engine::Engine::new(Fonts::get(), opts);
    let b = statement::layout_ast(&mut engine, ast);
    finish(b, opts)
}

/// Lays out a single expression.
pub fn layout_node(node: &Node, opts: &LayoutOptions) -> DisplayList {
    let mut engine = engine::Engine::new(Fonts::get(), opts);
    let b = statement::layout_single(&mut engine, node);
    finish(b, opts)
}

/// Parses maths text (LaTeX, plain or Unicode) and lays it out. Parse errors
/// show as empty slots where something was missing.
pub fn layout_latex(src: &str, opts: &LayoutOptions) -> DisplayList {
    let (ast, _) = parse_math(src, 0);
    layout_math(&ast, opts)
}

/// Parses a single expression from text.
pub(crate) fn parse_result(src: &str) -> Node {
    parse_expr(src, 0).0
}

fn finish(b: boxes::LBox, opts: &LayoutOptions) -> DisplayList {
    let px = opts.sanitized_size();
    let mut dl = DisplayList {
        width: b.width * px,
        height: b.height * px,
        depth: b.depth * px,
        baseline: b.height * px,
        font_size: px,
        items: vec![],
        hits: vec![],
    };
    b.emit(0.0, b.height, px, 0, &mut dl);
    dl
}
