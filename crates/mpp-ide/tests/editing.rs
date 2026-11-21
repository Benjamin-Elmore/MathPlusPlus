//! Editing tests (lane C2): brackets, selection and the command list, driven the
//! way a person does it, through the real frame loop (`pump` included).

use egui::{Event, Key, Modifiers, Vec2};
use egui_kittest::Harness;
use mpp_ide::{theme, IdeApp};

const SIZE: Vec2 = Vec2::new(1180.0, 820.0);

type H = Harness<'static, IdeApp>;

fn harness(app: IdeApp) -> H {
    Harness::builder()
        .with_size(SIZE)
        .with_max_steps(30)
        .with_step_dt(0.02)
        .with_context_setup(theme::install_fonts)
        .build_ui_state(|ui, app: &mut IdeApp| app.ui(ui), app)
}

/// A new file holding `text`, the caret at `caret`, the page drawn.
fn open(text: &str, caret: usize) -> H {
    let mut app = IdeApp::empty();
    app.new_file();
    let doc = app.doc_mut().expect("doc");
    doc.editor.set_text(text);
    doc.editor.move_caret(caret, false);
    doc.scroll_to = Some(mpp_syntax::Span::new(caret, caret));
    let mut h = harness(app);
    h.run_steps(3);
    h
}

fn type_text(h: &mut H, s: &str) {
    for c in s.chars() {
        h.event(Event::Text(c.to_string()));
        h.step();
    }
}

fn key(h: &mut H, mods: Modifiers, k: Key) {
    h.key_press_modifiers(mods, k);
    h.step();
}

fn plain(h: &mut H, k: Key) {
    key(h, Modifiers::NONE, k);
}

fn text(h: &H) -> String {
    h.state().doc().expect("doc").editor.text().to_string()
}

fn editor(h: &H) -> &mpp_ide::editor::Editor {
    &h.state().doc().expect("doc").editor
}

/// A maths line `$$ <inner> $$` with the caret at the end of `inner`.
fn maths(inner: &str) -> H {
    let src = format!("$$ {inner} $$");
    open(&src, 3 + inner.len())
}

// ---- U20: brackets ----

#[test]
fn an_opener_shows_a_ghost_that_is_never_saved() {
    let mut h = maths("");
    type_text(&mut h, "(a");
    assert_eq!(text(&h), "$$ (a $$", "the ghost is not in the file");
    let ghost = editor(&h).ghost_at_caret().map(|g| g.close.clone());
    assert_eq!(ghost.as_deref(), Some(")"));
}

#[test]
fn typing_the_closer_fills_the_ghost_in() {
    let mut h = maths("");
    type_text(&mut h, "(a)+b");
    assert_eq!(text(&h), "$$ (a)+b $$");
    assert!(editor(&h).ghost_at_caret().is_none());
}

#[test]
fn backspace_right_after_an_opener_removes_both() {
    let mut h = maths("x+");
    type_text(&mut h, "(");
    assert!(editor(&h).ghost_at_caret().is_some());
    plain(&mut h, Key::Backspace);
    assert_eq!(text(&h), "$$ x+ $$");
    assert!(editor(&h).ghost_at_caret().is_none());
    // A real pair goes together too.
    let mut h = maths("f()");
    h.state_mut()
        .doc_mut()
        .expect("doc")
        .editor
        .move_caret(5, false);
    h.step();
    plain(&mut h, Key::Backspace);
    assert_eq!(text(&h), "$$ f $$");
}

#[test]
fn square_curly_bars_and_left_right_work_the_same() {
    for (open, close) in [("[", "]"), ("{", "}"), ("|", "|")] {
        let mut h = maths("");
        type_text(&mut h, &format!("{open}x{close}"));
        assert_eq!(text(&h), format!("$$ {open}x{close} $$"));
        assert!(editor(&h).ghost_at_caret().is_none());
    }
    let mut h = maths("");
    type_text(&mut h, r"\left(x");
    assert_eq!(
        editor(&h)
            .ghost_at_caret()
            .map(|g| g.close.clone())
            .as_deref(),
        Some(r"\right)")
    );
    type_text(&mut h, ")");
    assert_eq!(text(&h), r"$$ \left(x\right) $$");
}

#[test]
fn the_ghost_goes_when_the_caret_moves_away() {
    let mut h = maths("");
    type_text(&mut h, "(a");
    plain(&mut h, Key::ArrowLeft);
    plain(&mut h, Key::ArrowRight);
    // Back where it was: the ghost is still there, nothing was saved.
    assert_eq!(text(&h), "$$ (a $$");
    plain(&mut h, Key::Home);
    assert!(editor(&h).ghost_at_caret().is_none());
}

#[test]
fn prose_gets_no_ghost() {
    let mut h = open("", 0);
    type_text(&mut h, "see (a");
    assert_eq!(text(&h), "see (a");
    assert!(editor(&h).ghost_at_caret().is_none());
}
