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

// ---- U21: selection ----

#[test]
fn shift_arrows_grow_one_structure_at_a_time() {
    let mut h = open("$$ (a + b)^2 + c $$", 3);
    key(&mut h, Modifiers::SHIFT, Key::ArrowRight);
    assert_eq!(editor(&h).selected_text(), Some("(a + b)"));
    key(&mut h, Modifiers::SHIFT, Key::ArrowRight);
    assert_eq!(editor(&h).selected_text(), Some("(a + b)^2"));
    key(&mut h, Modifiers::SHIFT, Key::ArrowLeft);
    assert_eq!(editor(&h).selected_text(), Some("(a + b)"));
}

#[test]
fn shift_arrows_never_jump_to_the_whole_formula() {
    let mut h = open("$$ x = a + b $$", 7);
    key(&mut h, Modifiers::SHIFT, Key::ArrowRight);
    assert_eq!(editor(&h).selected_text(), Some("a"));
}

#[test]
fn alt_shift_up_grows_by_levels() {
    let mut h = open("$$ y = (a + 2b) + c $$", 3 + 9);
    let mods = Modifiers::SHIFT | Modifiers::ALT;
    let mut seen = vec![];
    for _ in 0..5 {
        key(&mut h, mods, Key::ArrowUp);
        seen.push(editor(&h).selected_text().unwrap_or("").to_string());
    }
    assert_eq!(
        seen,
        [
            "2b",
            "a + 2b",
            "(a + 2b)",
            "(a + 2b) + c",
            "y = (a + 2b) + c"
        ]
    );
}

#[test]
fn typing_replaces_the_selection_and_an_opener_wraps_it() {
    let mut h = open("$$ a + b $$", 3);
    key(&mut h, Modifiers::SHIFT, Key::ArrowRight);
    type_text(&mut h, "(");
    assert_eq!(text(&h), "$$ (a) + b $$");
    assert_eq!(editor(&h).selected_text(), Some("a"));
    type_text(&mut h, "z");
    assert_eq!(text(&h), "$$ (z) + b $$");
}

fn click_at(h: &mut H, pos: egui::Pos2, times: usize) {
    h.event(Event::PointerMoved(pos));
    for _ in 0..times {
        for pressed in [true, false] {
            h.event(Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            h.step();
        }
    }
    h.step();
}

/// The screen position of the caret stop at `offset` (the caret goes there to
/// find out, then back to the start).
fn pos_of(h: &mut H, offset: usize) -> egui::Pos2 {
    h.state_mut()
        .doc_mut()
        .expect("doc")
        .editor
        .move_caret(offset, false);
    h.step();
    h.step();
    let at = h
        .state()
        .doc()
        .and_then(|d| d.caret_screen)
        .expect("the caret is drawn")
        .center();
    h.state_mut()
        .doc_mut()
        .expect("doc")
        .editor
        .move_caret(0, false);
    h.step();
    at
}

#[test]
fn double_click_selects_a_name_or_number_and_triple_click_the_line() {
    let src = "$$ x := alpha1 + 12.5 $$";
    let mut h = open(src, 0);
    let at = pos_of(&mut h, 10);
    click_at(&mut h, at, 2);
    let sel = editor(&h).selected_text().map(str::to_string);
    assert!(
        matches!(sel.as_deref(), Some("alpha1") | Some("α1")),
        "double-click picked {sel:?}"
    );
    let mut h = open(src, 0);
    let at = pos_of(&mut h, 10);
    click_at(&mut h, at, 3);
    assert_eq!(editor(&h).selected_text(), Some("x := alpha1 + 12.5"));
}
