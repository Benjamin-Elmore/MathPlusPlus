//! `mathpp-ide`: the Math++ live editor.

use std::path::PathBuf;

fn main() -> eframe::Result {
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Math++")
            .with_inner_size([1280.0, 860.0])
            .with_min_inner_size([640.0, 420.0]),
        ..eframe::NativeOptions::default()
    };
    eframe::run_native(
        "Math++",
        options,
        Box::new(move |cc| Ok(Box::new(mpp_ide::EframeApp::new(cc, &paths)))),
    )
}
