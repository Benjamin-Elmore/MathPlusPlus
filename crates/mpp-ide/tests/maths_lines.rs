//! Maths lines (U11–U15), driven through the real app the way a person uses it:
//! keys and clicks go through `IdeApp::ui` (which pumps the evaluator), one
//! frame per input. Screenshots of a maths box are compared against the images
//! stored in `tests/snapshots/` (run with `UPDATE_SNAPSHOTS=1` to write them).

use egui::{Event, Key, Modifiers, Pos2, Rect, Vec2};
use egui_kittest::{Harness, SnapshotOptions};
use mpp_ide::page::mode;
use mpp_ide::theme::ThemeChoice;
use mpp_ide::{theme, IdeApp};

const SIZE: Vec2 = Vec2::new(1180.0, 820.0);

fn harness(app: IdeApp) -> Harness<'static, IdeApp> {
    Harness::builder()
        .with_size(SIZE)
        .with_max_steps(30)
        .with_context_setup(theme::install_fonts)
        .build_ui_state(|ui, app: &mut IdeApp| app.ui(ui), app)
}

fn app_with(text: &str, caret: usize) -> IdeApp {
    let mut app = IdeApp::empty();
    app.new_file();
    let doc = app.doc_mut().expect("doc");
    doc.editor.set_text(text);
    doc.editor.move_caret(caret, false);
    app
}

fn text_of(h: &Harness<'static, IdeApp>) -> String {
    h.state().doc().expect("doc").editor.text().to_string()
}

fn press(h: &mut Harness<'static, IdeApp>, mods: Modifiers, key: Key) {
    h.key_press_modifiers(mods, key);
    h.step();
}

/// The screen position of the page's origin (its column's top-left), worked
/// out from where the caret was drawn.
fn origin(h: &Harness<'static, IdeApp>) -> Pos2 {
    let doc = h.state().doc().expect("doc");
    let pc = doc.page_cache().expect("page");
    let s = pc
        .page
        .caret_stop_on(doc.editor.caret(), doc.caret_side)
        .expect("stop");
    let r = doc.caret_screen.expect("caret drawn");
    r.center() - Vec2::new(s.x, (s.top + s.bottom) / 2.0)
}

fn bands(h: &Harness<'static, IdeApp>) -> Vec<mode::Band> {
    let doc = h.state().doc().expect("doc");
    let pc = doc.page_cache().expect("page");
    mode::bands(&pc.page, h.state().size)
}

fn click_at(h: &mut Harness<'static, IdeApp>, pos: Pos2) {
    h.event(Event::PointerMoved(pos));
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.step();
}

#[test]
fn ctrl_m_flips_a_line_and_undo_flips_it_back() {
    let src = "Some words.\n\nx := 5\n\nMore words.";
    let mut h = harness(app_with(src, 15));
    h.run_steps(3);
    assert!(bands(&h).is_empty(), "no maths box yet");
    press(&mut h, Modifiers::COMMAND, Key::M);
    assert_eq!(text_of(&h), "Some words.\n\n$$ x := 5 $$\n\nMore words.");
    h.run_steps(2);
    assert_eq!(bands(&h).len(), 1, "the line is in a maths box now");
    // The caret is still where it was in the line, and in maths.
    let doc = h.state().doc().expect("doc");
    let pc = doc.page_cache().expect("page");
    assert!(pc.page.in_math(doc.editor.caret()));
    // Ctrl+M again makes it prose; undo flips it back.
    press(&mut h, Modifiers::COMMAND, Key::M);
    assert_eq!(text_of(&h), src);
    press(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(text_of(&h), "Some words.\n\n$$ x := 5 $$\n\nMore words.");
    press(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(text_of(&h), src);
}

#[test]
fn a_click_on_the_marker_flips_its_line() {
    let src = "Intro words here.\n\n$$ E = 2x + 5 $$";
    let mut h = harness(app_with(src, 3));
    h.run_steps(3);
    let b = bands(&h);
    assert_eq!(b.len(), 1);
    let at = b[0].marker.center() + origin(&h).to_vec2();
    click_at(&mut h, at);
    assert_eq!(text_of(&h), "Intro words here.\n\nE = 2x + 5");
    h.run_steps(2);
    assert!(bands(&h).is_empty(), "the box went with the maths");
}

/// The part of the screen around the first maths box, marker included.
fn crop_band(h: &mut Harness<'static, IdeApp>) -> image::RgbaImage {
    h.run_steps(3);
    let b = bands(h)[0];
    let o = origin(h).to_vec2();
    let r = b.rect.union(b.marker).translate(o).expand(6.0);
    let ppp = h.ctx.pixels_per_point();
    let img = h.render().expect("render");
    let clip = Rect::from_min_size(Pos2::ZERO, SIZE).intersect(r);
    let (x, y) = ((clip.left() * ppp) as u32, (clip.top() * ppp) as u32);
    let (w, hgt) = ((clip.width() * ppp) as u32, (clip.height() * ppp) as u32);
    image::imageops::crop_imm(&img, x, y, w, hgt).to_image()
}

/// Rendering is done one test at a time (the GPU device is shared).
static RENDER: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn a_maths_box_looks_the_same_in_light_and_dark() {
    let _one = RENDER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let src = "Prose with $x + 1$ inside.\n\n$$ E = 2x + 5 $$\n\n$$ y := 3 $$";
    for (choice, name) in [
        (ThemeChoice::Light, "maths_box_light"),
        (ThemeChoice::Dark, "maths_box_dark"),
    ] {
        let mut app = app_with(src, 3);
        app.set_theme_choice(choice);
        let mut h = harness(app);
        let img = crop_band(&mut h);
        let opts = SnapshotOptions::new().threshold(2.0).max_failed_pixels(60);
        egui_kittest::try_image_snapshot_options(&img, name, &opts)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
    }
}
