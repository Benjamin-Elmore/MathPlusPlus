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

fn type_text(h: &mut Harness<'static, IdeApp>, s: &str) {
    for c in s.chars() {
        h.event(Event::Text(c.to_string()));
        h.step();
    }
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

#[test]
fn a_finished_line_that_reads_as_maths_becomes_maths_and_ctrl_z_keeps_it_as_text() {
    // Ben's video: a heading, then `x := 5` typed on a new line and Enter.
    let src = "# This is a test\n\n";
    let mut h = harness(app_with(src, src.len()));
    h.run_steps(3);
    type_text(&mut h, "x := 5");
    assert_eq!(
        text_of(&h),
        "# This is a test\n\nx := 5",
        "nothing changes while typing"
    );
    press(&mut h, Modifiers::NONE, Key::Enter);
    assert_eq!(text_of(&h), "# This is a test\n\n$$ x := 5 $$\n\n");
    h.run_steps(2);
    assert_eq!(bands(&h).len(), 1);
    let hint = h
        .state()
        .doc()
        .and_then(|d| d.editor.hint().map(str::to_string));
    assert!(
        hint.is_some_and(|t| t.contains("Ctrl+Z")),
        "a one-time hint"
    );
    // The caret is on the new line, in prose.
    let doc = h.state().doc().expect("doc");
    assert_eq!(doc.editor.caret(), doc.editor.text().len());
    // The first Ctrl+Z undoes only the detection.
    press(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(text_of(&h), "# This is a test\n\nx := 5\n\n");
    // Prose stays prose.
    type_text(&mut h, "Some ordinary words");
    press(&mut h, Modifiers::NONE, Key::Enter);
    assert!(text_of(&h).ends_with("Some ordinary words\n\n"));
}

#[test]
fn equals_or_dollar_on_an_empty_line_starts_a_maths_line() {
    let src = "Intro words.\n\n";
    let mut h = harness(app_with(src, src.len()));
    h.run_steps(3);
    type_text(&mut h, "=");
    assert_eq!(text_of(&h), "Intro words.\n\n$$  $$");
    type_text(&mut h, "1 + 2 = ?");
    assert_eq!(text_of(&h), "Intro words.\n\n$$ 1 + 2 = ? $$");
    // A LaTeX user typing the dollars by hand gets the same line, once.
    let mut h = harness(app_with(src, src.len()));
    h.run_steps(3);
    type_text(&mut h, "$$ y := 2 $$");
    assert_eq!(text_of(&h), "Intro words.\n\n$$ y := 2 $$");
    h.run_steps(2);
    let doc = h.state().doc().expect("doc");
    assert_eq!(
        doc.editor.caret(),
        doc.editor.text().len(),
        "after the closing $$"
    );
}

#[test]
fn alt_equals_makes_the_line_maths() {
    let src = "Intro words.\n\na + b";
    let mut h = harness(app_with(src, src.len()));
    h.run_steps(3);
    press(&mut h, Modifiers::ALT, Key::Equals);
    assert_eq!(text_of(&h), "Intro words.\n\n$$ a + b $$");
    // Already maths: nothing more happens.
    press(&mut h, Modifiers::ALT, Key::Equals);
    assert_eq!(text_of(&h), "Intro words.\n\n$$ a + b $$");
}

#[test]
fn an_empty_file_says_how_to_start() {
    let mut h = harness(app_with("", 0));
    h.run_steps(3);
    let img = {
        let _one = RENDER
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        h.render().expect("render")
    };
    // The hint is drawn: some grey ink to the right of the caret.
    let r = h.state().doc().and_then(|d| d.caret_screen).expect("caret");
    let ppp = h.ctx.pixels_per_point();
    let (y0, y1) = ((r.top() * ppp) as u32, (r.bottom() * ppp) as u32);
    let x0 = ((r.right() + 8.0) * ppp) as u32;
    let bg = img.get_pixel(x0 + 400, y1 + 40).0;
    let inked = (x0..x0 + 200)
        .flat_map(|x| (y0..y1).map(move |y| (x, y)))
        .filter(|&(x, y)| img.get_pixel(x, y).0 != bg)
        .count();
    assert!(
        inked > 50,
        "no hint beside the caret ({inked} inked pixels)"
    );
    // Typing makes it go.
    type_text(&mut h, "Hello");
    assert_eq!(text_of(&h), "Hello");
}

/// True when the status bar shows exactly this place label.
fn shows(h: &Harness<'static, IdeApp>, label: &str) -> bool {
    use egui_kittest::kittest::Queryable;
    h.query_by_label(label).is_some()
}

#[test]
fn the_status_bar_shows_prose_or_maths_and_the_path_to_the_caret() {
    use egui_kittest::kittest::Queryable;
    let src = "Some words.\n\n$$ y := \\frac{1}{x + 2} $$";
    let mut h = harness(app_with(src, 3));
    h.run_steps(3);
    assert!(shows(&h, "Prose"));
    let den = src.find("x + 2").expect("x") + 1;
    h.state_mut()
        .doc_mut()
        .expect("doc")
        .editor
        .move_caret(den, false);
    h.run_steps(3);
    assert!(shows(&h, "Maths \u{203a} fraction \u{203a} denominator"));
    // Clicking the label flips the line, as Ctrl+M does.
    h.state_mut()
        .doc_mut()
        .expect("doc")
        .editor
        .move_caret(3, false);
    h.run_steps(3);
    h.get_by_label("Prose").click();
    h.run_steps(3);
    assert_eq!(
        text_of(&h),
        "$$ Some words. $$\n\n$$ y := \\frac{1}{x + 2} $$"
    );
}

#[test]
fn the_part_holding_the_caret_is_boxed() {
    let _one = RENDER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let src = "$$ y := \\frac{1}{x + 2} $$";
    let den = src.find("x + 2").expect("x") + 1;
    for (choice, name) in [
        (ThemeChoice::Light, "focus_box_light"),
        (ThemeChoice::Dark, "focus_box_dark"),
    ] {
        let mut app = app_with(src, den);
        app.set_theme_choice(choice);
        let mut h = harness(app);
        let img = crop_band(&mut h);
        // The focus colour is on screen (and it is not the selection's).
        let pal = h.state().mode.palette();
        let focus = theme::c32(pal.focus_box);
        assert_ne!(focus, theme::c32(pal.selection));
        let near = |p: &image::Rgba<u8>| {
            let d = |a: u8, b: u8| (i32::from(a) - i32::from(b)).abs();
            d(p.0[0], focus.r()) + d(p.0[1], focus.g()) + d(p.0[2], focus.b()) < 40
        };
        assert!(
            img.pixels().filter(|p| near(p)).count() > 20,
            "{name}: no focus box"
        );
        let opts = SnapshotOptions::new().threshold(2.0).max_failed_pixels(60);
        egui_kittest::try_image_snapshot_options(&img, name, &opts)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
    }
}

#[test]
fn empty_parts_show_what_goes_in_them() {
    let src = "$$ \\frac{}{} $$";
    let mut h = harness(app_with(src, 9));
    h.run_steps(3);
    let doc = h.state().doc().expect("doc");
    let pc = doc.page_cache().expect("page");
    let labels: Vec<String> = pc
        .page
        .blocks
        .iter()
        .flat_map(|b| b.dl.hits.iter())
        .filter_map(|h| h.label.clone())
        .collect();
    assert!(labels.iter().any(|l| l == "numerator"), "{labels:?}");
    assert!(labels.iter().any(|l| l == "denominator"), "{labels:?}");
}

/// The glyphs of the first maths block on the page: (char, x, size).
fn maths_glyphs(h: &Harness<'static, IdeApp>) -> Vec<(char, f32, f32)> {
    let doc = h.state().doc().expect("doc");
    let pc = doc.page_cache().expect("page");
    let b = pc
        .page
        .blocks
        .iter()
        .find(|b| b.tag == mpp_ide::page::BlockTag::Math)
        .expect("a maths block");
    b.dl.items
        .iter()
        .filter_map(|p| match p {
            mpp_render::Primitive::Glyph { ch, x, size, .. } => Some((*ch, *x, *size)),
            _ => None,
        })
        .collect()
}

fn maths_height(h: &Harness<'static, IdeApp>) -> f32 {
    let doc = h.state().doc().expect("doc");
    let pc = doc.page_cache().expect("page");
    pc.page
        .blocks
        .iter()
        .find(|b| b.tag == mpp_ide::page::BlockTag::Math)
        .expect("a maths block")
        .height
}

#[test]
fn maths_looks_like_maths_while_typing() {
    // Typed the way Ben did in the video, without a space after the `+`.
    let src = "$$ E = 2x +5 $$";
    let mut h = harness(app_with(src, 12));
    h.run_steps(3);
    let g = maths_glyphs(&h);
    let chars: String = g.iter().map(|c| c.0).collect();
    // Single-letter variables are italic, numbers upright.
    assert!(
        chars.contains('\u{1d438}') && chars.contains('\u{1d465}'),
        "{chars}"
    );
    assert!(chars.contains('2') && chars.contains('5'), "{chars}");
    // `+5` gets the spacing of a binary plus straight away.
    let x = |c: char| g.iter().find(|t| t.0 == c).expect("glyph").1;
    let em = g[0].2;
    assert!(x('5') - x('+') > 0.9 * em, "+5 is tight: {g:?}");
    // Functions are upright.
    h.state_mut()
        .doc_mut()
        .expect("doc")
        .editor
        .set_text("$$ \\sin(x) $$");
    h.run_steps(3);
    let chars: String = maths_glyphs(&h).iter().map(|c| c.0).collect();
    assert!(chars.starts_with("sin"), "{chars}");
}

#[test]
fn scripts_never_change_the_line_height() {
    let mut h = harness(app_with("$$ x $$", 4));
    h.run_steps(3);
    let plain = maths_height(&h);
    for src in ["$$ x^2 $$", "$$ x_1 $$", "$$ x_1^2 $$"] {
        h.state_mut().doc_mut().expect("doc").editor.set_text(src);
        h.run_steps(3);
        assert!(
            (maths_height(&h) - plain).abs() < 0.5,
            "{src}: {} vs {plain}",
            maths_height(&h)
        );
    }
    // A line is about one and a half lines of maths tall.
    let fs = h.state().size * 1.1;
    assert!(plain >= 1.4 * fs && plain <= 1.9 * fs, "{plain} for {fs}");
}

#[test]
fn scripts_are_never_tiny() {
    let dl = mpp_render::layout_latex("x^{2^{2}}", &mpp_render::LayoutOptions::inline(12.0));
    for p in &dl.items {
        if let mpp_render::Primitive::Glyph { ch, size, .. } = p {
            assert!(*size >= 9.0 - 1e-3, "{ch} at {size}px");
        }
    }
}

#[test]
fn rows_line_up_on_their_equals_signs() {
    let src = "$$ a = 1 \\\\\nbb + c = 22 $$";
    let mut h = harness(app_with(src, 4));
    h.run_steps(3);
    let xs: Vec<f32> = maths_glyphs(&h)
        .iter()
        .filter(|g| g.0 == '=')
        .map(|g| g.1)
        .collect();
    assert_eq!(xs.len(), 2, "{xs:?}");
    assert!(
        (xs[0] - xs[1]).abs() < 0.5,
        "the = signs are not aligned: {xs:?}"
    );
}
