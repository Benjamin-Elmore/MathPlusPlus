//! Problems building or exporting a figure.

use std::fmt;

/// Why a figure could not be built or exported.
#[derive(Clone, Debug, PartialEq)]
pub enum PlotError {
    /// The figure has nothing to draw.
    Empty,
    /// A figure item could not be understood (with an explanation for the author).
    Item(String),
    /// A range could not be evaluated to two finite numbers.
    Range(String),
    /// Writing SVG or PNG failed.
    Export(String),
}

impl fmt::Display for PlotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlotError::Empty => write!(f, "the figure has nothing to draw"),
            PlotError::Item(m) | PlotError::Range(m) | PlotError::Export(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for PlotError {}
