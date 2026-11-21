//! Regression tests for the final IDE and display fixes.

use std::path::PathBuf;

use egui::Vec2;
use egui_kittest::Harness;
use mpp_ide::{theme, IdeApp};

const SIZE: Vec2 = Vec2::new(1000.0, 900.0);

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mpp-final-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    dir
}

fn open_text(tag: &str, src: &str) -> Harness<'static, IdeApp> {
    let file = scratch(tag).join("doc.mpp");
    std::fs::write(&file, src).expect("write");
    let mut app = IdeApp::empty();
    app.set_theme_choice(theme::ThemeChoice::Light);
    app.open_file(&file, false).expect("open");
    let mut h = Harness::builder()
        .with_size(SIZE)
        .with_max_steps(40)
        .with_context_setup(theme::install_fonts)
        .build_ui_state(|ui, app: &mut IdeApp| app.ui(ui), app);
    h.run_steps(3);
    h.state_mut().settle();
    h.run_steps(4);
    h
}

fn tex(s: &str) -> String {
    s.replace('@', "\\")
}

/// The right edge of the glyphs and rules of a laid-out block.
fn ink_right(dl: &mpp_render::DisplayList) -> f32 {
    dl.items
        .iter()
        .filter_map(|p| match p {
            mpp_render::Primitive::Glyph { x, size, .. } => Some(x + size * 0.75),
            mpp_render::Primitive::Rule { rect, .. } => Some(rect.x + rect.w),
            _ => None,
        })
        .fold(0.0f32, f32::max)
}

#[test]
fn a_convergence_note_is_wrapped_to_the_page() {
    let mut h = open_text(
        "sumnote",
        &tex("# T

$$ @sum_{n=1}^{@infty} x + 1 $$
"),
    );
    for width in [800.0f32, 420.0] {
        let pc = h.state_mut().page_at(width).expect("page");
        let b = pc
            .page
            .blocks
            .iter()
            .find(|b| b.dl.text().contains("converges"))
            .expect("the note is on the page");
        assert!(
            ink_right(&b.dl) <= pc.page.width + 0.5,
            "note runs off the page at {width}: {} > {}",
            ink_right(&b.dl),
            pc.page.width
        );
        assert!(b.height > 40.0, "the note adds a line of its own");
    }
    let shot = h.render().expect("render");
    shot.save(std::env::temp_dir().join("final-sum-note.png"))
        .expect("png");
}

#[test]
fn an_equation_answer_does_not_follow_a_second_equals_sign() {
    let mut h = open_text(
        "lin",
        &tex("# T\n\n$$ f(x) := x^3 - 2 x $$\n\n$$ @operatorname{linearisation}_{x=3} f = ? $$\n"),
    );
    let text = h.state_mut().visible_text(800.0);
    let line = text
        .lines()
        .find(|l| l.contains("Linearisation"))
        .unwrap_or_else(|| panic!("no linearisation line in {text}"));
    assert!(line.contains('\u{2192}'), "{line}");
    assert_eq!(line.matches('=').count(), 2, "{line}");
}
