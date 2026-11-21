//! `mathpp` (installed and zipped as `Math++.exe`): the Math++ live editor.
//!
//! Cargo bin names cannot contain `+`, so the build makes `mathpp.exe` and the packaging
//! script (`work/release/build.ps1`) copies it to `Math++.exe`.
//!
//! Release builds on Windows use the GUI subsystem so no console window opens; debug
//! builds keep the console for logging.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::path::PathBuf;

fn main() -> eframe::Result {
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    mpp_ide::run(paths)
}
