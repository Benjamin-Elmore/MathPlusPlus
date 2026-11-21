//! Replay tests: the IDE driven the way a person uses it, through the real frame
//! loop. A frame only happens when the app asked for one (or on input), exactly
//! like the desktop event loop, so a missing repaint shows up as a failure.

use std::time::{Duration, Instant};

use egui::{Event, Vec2};
use egui_kittest::Harness;
use mpp_ide::{theme, IdeApp};

const SIZE: Vec2 = Vec2::new(1180.0, 820.0);

fn harness(app: IdeApp) -> Harness<'static, IdeApp> {
    Harness::builder()
        .with_size(SIZE)
        .with_max_steps(30)
        .with_context_setup(theme::install_fonts)
        .build_ui_state(|ui, app: &mut IdeApp| app.ui(ui), app)
}

/// One key or piece of text, then the frame the input causes.
fn type_text(h: &mut Harness<'static, IdeApp>, s: &str) {
    for c in s.chars() {
        h.event(Event::Text(c.to_string()));
        h.step();
    }
}

/// The person stops typing. Frames run only while the app asks for them (as the
/// real event loop does), for at most `limit`.
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

fn current(h: &Harness<'static, IdeApp>) -> bool {
    h.state().doc().is_some_and(|d| d.page_is_current())
}

#[test]
fn an_answer_appears_without_another_key_press() {
    let mut app = IdeApp::empty();
    app.new_file();
    let mut h = harness(app);
    h.run_steps(2);
    type_text(&mut h, "$$ 1 + 2 = ? $$");
    // Saved straight away (Ctrl+S, or autosave): nothing else is left to redraw for.
    let dir = std::env::temp_dir().join(format!("mpp-replay-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    h.state_mut()
        .doc_mut()
        .expect("doc")
        .save_as(&dir.join("answer.mpp"))
        .expect("save");
    h.step();
    idle(&mut h, Duration::from_secs(10));
    assert!(
        current(&h),
        "the answer was worked out but the page was never drawn again to show it"
    );
}
