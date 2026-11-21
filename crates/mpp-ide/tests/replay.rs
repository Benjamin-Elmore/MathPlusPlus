//! Replay tests: the IDE driven the way a person uses it, through the real frame
//! loop. A frame only happens when the app asked for one (or on input), exactly
//! like the desktop event loop, so a missing repaint shows up as a failure.

use std::time::{Duration, Instant};

use egui::{Event, Key, Modifiers, Vec2};
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

// ---- input rules (lane A3): Enter, Backspace, lists and headings ----

fn key(h: &mut Harness<'static, IdeApp>, mods: Modifiers, key: Key) {
    h.key_press_modifiers(mods, key);
    h.step();
}

fn plain(h: &mut Harness<'static, IdeApp>, k: Key) {
    key(h, Modifiers::NONE, k);
}

fn ctrl(h: &mut Harness<'static, IdeApp>, k: Key) {
    key(h, Modifiers::COMMAND, k);
}

fn text(h: &Harness<'static, IdeApp>) -> String {
    h.state().doc().expect("doc").editor.text().to_string()
}

fn fresh() -> Harness<'static, IdeApp> {
    let mut app = IdeApp::empty();
    app.new_file();
    let mut h = harness(app);
    h.run_steps(2);
    h
}

/// Blocks of the parsed file, by kind name, as the real parser sees the text.
fn lists_in(src: &str) -> usize {
    let doc = mpp_syntax::document::parse_document(src);
    doc.blocks
        .iter()
        .filter(|b| matches!(b.kind, mpp_syntax::document::BlockKind::List(_)))
        .count()
}

#[test]
fn enter_ends_the_block_and_the_file_says_so() {
    let mut h = fresh();
    type_text(&mut h, "first");
    plain(&mut h, Key::Enter);
    type_text(&mut h, "second");
    assert_eq!(text(&h), "first\n\nsecond");
}

#[test]
fn shift_enter_is_a_soft_break() {
    let mut h = fresh();
    type_text(&mut h, "first");
    key(&mut h, Modifiers::SHIFT, Key::Enter);
    type_text(&mut h, "second");
    assert_eq!(text(&h), "first\nsecond");
}

#[test]
fn enter_is_one_undo_step() {
    let mut h = fresh();
    type_text(&mut h, "first");
    plain(&mut h, Key::Enter);
    ctrl(&mut h, Key::Z);
    assert_eq!(text(&h), "first");
}

#[test]
fn backspace_at_the_start_of_a_block_joins_it_in_one_step() {
    let mut h = fresh();
    type_text(&mut h, "first");
    plain(&mut h, Key::Enter);
    type_text(&mut h, "second");
    for _ in 0..6 {
        plain(&mut h, Key::ArrowLeft);
    }
    plain(&mut h, Key::Backspace);
    assert_eq!(text(&h), "firstsecond");
    ctrl(&mut h, Key::Z);
    assert_eq!(text(&h), "first\n\nsecond");
}

#[test]
fn enter_inside_display_maths_makes_a_new_row() {
    let mut h = fresh();
    type_text(&mut h, "$$x := 1");
    plain(&mut h, Key::Enter);
    type_text(&mut h, "y := 2$$");
    let t = text(&h);
    assert_eq!(t, "$$x := 1 \\\\\ny := 2$$", "one block, a second row");
    assert!(!t.contains("\n\n"));
}

#[test]
fn a_list_marker_with_a_space_becomes_a_list_and_undo_gives_it_back() {
    let mut h = fresh();
    type_text(&mut h, "1.");
    assert_eq!(lists_in(&text(&h)), 0, "1. alone is only text");
    type_text(&mut h, " ");
    assert_eq!(lists_in(&text(&h)), 1);
    ctrl(&mut h, Key::Z);
    assert_eq!(text(&h), "1.");
    assert_eq!(lists_in(&text(&h)), 0);
}

#[test]
fn bullets_work_the_same_way() {
    let mut h = fresh();
    type_text(&mut h, "- ");
    assert_eq!(lists_in(&text(&h)), 1);
    ctrl(&mut h, Key::Z);
    assert_eq!(text(&h), "-");
    assert_eq!(lists_in(&text(&h)), 0);
}

#[test]
fn numbers_never_become_lists() {
    for s in ["1.3", "1.", "2.5x", "1.321321312 + 5 =", "-3", "*x"] {
        let mut h = fresh();
        type_text(&mut h, s);
        assert_eq!(text(&h), s);
        assert_eq!(lists_in(&text(&h)), 0, "{s} is not a list");
    }
}

#[test]
fn a_maths_line_never_becomes_a_list() {
    let mut h = fresh();
    type_text(&mut h, "$$1. 2 - 3 * 4$$");
    assert_eq!(lists_in(&text(&h)), 0);
}

#[test]
fn hash_then_a_letter_makes_a_heading_with_a_hint() {
    let mut h = fresh();
    type_text(&mut h, "#This is a test");
    assert_eq!(text(&h), "# This is a test");
    let doc = mpp_syntax::document::parse_document(&text(&h));
    assert!(matches!(
        doc.blocks[0].kind,
        mpp_syntax::document::BlockKind::Heading { .. }
    ));
}

#[test]
fn the_first_undo_after_the_heading_conversion_keeps_a_plain_hash() {
    let mut h = fresh();
    type_text(&mut h, "#T");
    assert_eq!(text(&h), "# T");
    let hint = h
        .state()
        .doc()
        .expect("doc")
        .editor
        .hint()
        .map(String::from);
    assert!(
        hint.is_some_and(|s| s.contains("Heading 1")),
        "a hint shows once"
    );
    ctrl(&mut h, Key::Z);
    assert_eq!(text(&h), "#T", "exactly what was typed");
}

#[test]
fn a_hash_without_a_letter_or_not_at_the_line_start_is_left_alone() {
    let mut h = fresh();
    type_text(&mut h, "# ");
    type_text(&mut h, "a #b 1#2");
    assert_eq!(text(&h), "# a #b 1#2");
    let mut h = fresh();
    type_text(&mut h, "x #y");
    assert_eq!(text(&h), "x #y");
}

#[test]
fn ctrl_digits_set_the_heading_level() {
    let mut h = fresh();
    type_text(&mut h, "Title");
    ctrl(&mut h, Key::Num2);
    assert_eq!(text(&h), "## Title");
    ctrl(&mut h, Key::Num1);
    assert_eq!(text(&h), "# Title");
    ctrl(&mut h, Key::Num3);
    assert_eq!(text(&h), "### Title");
    ctrl(&mut h, Key::Num0);
    assert_eq!(text(&h), "Title");
    ctrl(&mut h, Key::Z);
    assert_eq!(text(&h), "### Title");
}
