//! Sizes of the window's parts. A tiling layout manager takes over this module
//! later; for now it only names the numbers the app uses.

/// Default width of the left side panel.
pub const SIDE_PANEL_WIDTH: f32 = 230.0;

/// Body text size of the page, in pixels.
pub const BODY_SIZE: f32 = 19.0;

/// Smallest and largest page text size.
pub const MIN_SIZE: f32 = 12.0;
pub const MAX_SIZE: f32 = 40.0;

/// Time with no typing before the evaluator is asked to run.
pub const DEBOUNCE_MS: u64 = 100;
