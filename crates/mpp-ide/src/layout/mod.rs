//! Sizes of the window's parts, and the tile manager: every panel is a tile
//! that can be split, dragged, resized, maximised, closed and reopened.

pub mod behavior;
pub mod docref;
pub mod panes;
pub mod persist;
pub mod tiles;
pub mod tools_tiles;

pub use tiles::{Dir, Pane, Preset};

/// Default width of the left side panel.
pub const SIDE_PANEL_WIDTH: f32 = 230.0;

/// Body text size of the page, in pixels.
pub const BODY_SIZE: f32 = 19.0;

/// Smallest and largest page text size.
pub const MIN_SIZE: f32 = 12.0;
pub const MAX_SIZE: f32 = 40.0;

/// Time with no typing before the evaluator is asked to run.
pub const DEBOUNCE_MS: u64 = 100;

/// How the tree tidies itself after a drag: every panel keeps its own tab bar,
/// and an emptied editor group stays so the next file has somewhere to open.
pub fn simplification() -> egui_tiles::SimplificationOptions {
    egui_tiles::SimplificationOptions {
        all_panes_must_have_tabs: true,
        prune_empty_tabs: false,
        ..egui_tiles::SimplificationOptions::default()
    }
}
