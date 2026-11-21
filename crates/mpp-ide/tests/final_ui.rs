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

/// The glyphs of a laid-out block: (character, x, baseline y, size).
fn glyphs(dl: &mpp_render::DisplayList) -> Vec<(char, f32, f32, f32)> {
    dl.items
        .iter()
        .filter_map(|p| match p {
            mpp_render::Primitive::Glyph { ch, x, y, size, .. } => Some((*ch, *x, *y, *size)),
            _ => None,
        })
        .collect()
}

#[test]
fn the_maths_in_a_convergence_note_is_typeset() {
    let mut h = open_text(
        "sumnote-maths",
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
        let text = b.dl.text();
        // Never the flattened text the series prints as.
        for code in ["(n=1)", "n=1)", "^", "_", "\\"] {
            assert!(!text.contains(code), "{code:?} in {text:?}");
        }
        let all = glyphs(&b.dl);
        // The equation comes first, then the note ("Whether ∑ … converges …").
        let start = all
            .windows(7)
            .position(|w| w.iter().map(|g| g.0).collect::<String>() == "Whether")
            .expect("the note starts with Whether");
        let (eq, note) = all.split_at(start);
        // The series in the note is laid out by the maths layout: a summation sign
        // with its limits set as smaller scripts, not as text after it.
        let word_size = note[0].3;
        let sigma = note
            .iter()
            .find(|g| g.0 == '∑' || g.0 == '\u{2211}')
            .expect("a summation sign in the note");
        let infinity = note
            .iter()
            .find(|g| g.0 == '∞')
            .expect("the upper limit in the note");
        assert!(
            infinity.3 < word_size * 0.9,
            "the limit is a script: {} vs {word_size}",
            infinity.3
        );
        assert!(
            infinity.2 < sigma.2,
            "the upper limit sits above the baseline"
        );
        // Wrapped within the page, and below the equation, never over it.
        assert!(
            ink_right(&b.dl) <= pc.page.width + 0.5,
            "note runs off the page at {width}: {} > {}",
            ink_right(&b.dl),
            pc.page.width
        );
        let eq_bottom = eq.iter().map(|g| g.2 + g.3 * 0.3).fold(f32::MIN, f32::max);
        let note_top = note
            .iter()
            .map(|g| g.2 - g.3 * 0.75)
            .fold(f32::MAX, f32::min);
        assert!(
            note_top >= eq_bottom - 1.0,
            "the note overlaps the equation at {width}: top {note_top} < {eq_bottom}"
        );
        if width < 500.0 {
            let lines: std::collections::BTreeSet<i32> = note
                .iter()
                .filter(|g| (g.3 - word_size).abs() < 0.01)
                .map(|g| g.2.round() as i32)
                .collect();
            assert!(lines.len() >= 2, "the note wraps at {width}: {lines:?}");
        }
    }
    let shot = h.render().expect("render");
    let dir = std::env::var_os("MPP_SHOT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let _ = std::fs::create_dir_all(&dir);
    shot.save(dir.join("sum-note-maths.png")).expect("png");
}
