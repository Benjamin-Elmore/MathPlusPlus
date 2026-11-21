//! `mathpp-ide`: the Math++ live editor.

use std::path::PathBuf;

fn main() -> eframe::Result {
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    mpp_ide::run(paths)
}
