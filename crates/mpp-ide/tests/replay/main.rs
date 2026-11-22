//! Replay tests (U27): key-by-key scripts of real people typing, run through the
//! real frame loop (`IdeApp::ui`, which pumps the evaluator), with pauses as
//! frames that run only when the app asks for them. After every frame the
//! scripts check the caret's line, what the page shows and the answers; at the
//! milestones the page is compared against the images in `tests/snapshots/`
//! (`UPDATE_SNAPSHOTS=1` writes them).

mod basics;
mod escape;
mod persona;
mod persona2;
mod recheck_editing;
mod recheck_maths;
mod recheck_units;
mod results;
mod runner;
mod side;
mod timing;
mod video;
mod worksheets;
