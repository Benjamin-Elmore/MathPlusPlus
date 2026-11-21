//! A calm status bar (U8): "Working…" only for a run that takes more than a
//! second, the definitions count from the evaluator's results, and a problems
//! count that changes only when a run finishes. Driven through the real frame
//! loop: keys arrive with real time between them, and frames run only on input
//! or when the app asks for one (the app pumps its evaluator in each frame).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::{Event, Vec2};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use mpp_ide::{theme, IdeApp};

const SIZE: Vec2 = Vec2::new(1180.0, 760.0);

fn harness(app: IdeApp) -> Harness<'static, IdeApp> {
    Harness::builder()
        .with_size(SIZE)
        .with_max_steps(40)
        .with_context_setup(theme::install_fonts)
        .build_ui_state(|ui, app: &mut IdeApp| app.ui(ui), app)
}

fn folder(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mpp-status-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("mkdir");
    d
}

/// Frames run only while the app asks for them, for at most `limit`.
fn idle(h: &mut Harness<'static, IdeApp>, limit: Duration) {
    let start = Instant::now();
    while start.elapsed() < limit {
        if !h.ctx.has_requested_repaint() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
        h.step();
    }
}

/// The status bar's problems label ("No problems" or "… in this file").
fn problems_label(h: &Harness<'static, IdeApp>) -> Option<String> {
    if h.query_all_by_label("No problems").next().is_some() {
        return Some("No problems".into());
    }
    h.query_all_by_label_contains("in this file")
        .next()
        .map(|n| format!("{n:?}"))
}

fn working(h: &Harness<'static, IdeApp>) -> bool {
    h.query_all_by_label_contains("Working").next().is_some()
}

#[test]
fn the_definitions_count_comes_from_the_evaluator() {
    let d = folder("defs");
    let file = d.join("two.mpp");
    std::fs::write(&file, "# Two\n\n$$ a := 1 $$\n\n$$ b := a + 1 $$\n").expect("w");
    let mut app = IdeApp::empty();
    app.open_folder(&d, None);
    app.open_file(&file, false).expect("open");
    let mut h = harness(app);
    h.step();
    idle(&mut h, Duration::from_secs(10));
    assert!(
        h.query_by_label("2 definitions").is_some(),
        "two definitions counted"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn typing_keeps_the_bar_still_until_the_run_finishes() {
    let d = folder("typing");
    let file = d.join("calm.mpp");
    std::fs::write(&file, "# Calm\n\n$$ a := 1 $$\n").expect("w");
    let mut app = IdeApp::empty();
    app.open_folder(&d, None);
    app.open_file(&file, false).expect("open");
    let mut h = harness(app);
    h.step();
    idle(&mut h, Duration::from_secs(10));
    let before = problems_label(&h).expect("problems label");
    assert_eq!(before, "No problems");
    assert!(h.query_by_label("1 definition").is_some());
    // Type a new line whose middle states do not parse ("b := 2 +" ...), with
    // the small gaps of real typing (shorter than the pause before a run).
    {
        let doc = h.state_mut().doc_mut().expect("doc");
        let end = doc.editor.text().len();
        doc.editor.select(end..end);
    }
    let mut seen = vec![];
    for c in "\n\n$$ b := 2 + 3 $$".chars() {
        h.event(Event::Text(c.to_string()));
        h.step();
        std::thread::sleep(Duration::from_millis(30));
        h.step();
        assert!(!working(&h), "no Working… while typing (after {c:?})");
        seen.push(problems_label(&h).expect("label"));
    }
    assert!(
        seen.iter().all(|s| s == &before),
        "the problems count stays put between keys: {seen:?}"
    );
    // The run finishes: the counts catch up, and Working… never showed.
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        if !h.ctx.has_requested_repaint() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        h.step();
        assert!(!working(&h), "a quick run never says Working…");
    }
    assert!(
        h.query_by_label("2 definitions").is_some(),
        "the new definition is counted once the run is done"
    );
    assert_eq!(problems_label(&h).as_deref(), Some("No problems"));
    let _ = std::fs::remove_dir_all(&d);
}
