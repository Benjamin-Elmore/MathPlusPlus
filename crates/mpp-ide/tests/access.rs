//! U26: accessibility. Contrast of every colour the maths-line work added, the
//! spoken form of formulas read back from the AccessKit tree, zoom, and the
//! keyboard-only paths.

use egui::{Key, Modifiers, Vec2};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use mpp_ide::theme::{contrast, Mode};
use mpp_ide::{theme, IdeApp};

const SIZE: Vec2 = Vec2::new(1180.0, 820.0);

fn harness(app: IdeApp) -> Harness<'static, IdeApp> {
    Harness::builder()
        .with_size(SIZE)
        .with_max_steps(40)
        .with_context_setup(|ctx| {
            theme::install_fonts(ctx);
            ctx.enable_accesskit();
        })
        .build_ui_state(|ui, app: &mut IdeApp| app.ui(ui), app)
}

fn app_with(text: &str) -> IdeApp {
    let mut app = IdeApp::empty();
    app.new_file();
    let doc = app.doc_mut().expect("doc");
    doc.editor.set_text(text);
    doc.editor.move_caret(0, false);
    app
}

#[test]
fn every_colour_of_the_maths_lines_has_enough_contrast() {
    // Text 4.5:1, markers and outlines 3:1, in both modes.
    let want_text = [
        "text in a maths box",
        "maths marker on its tint",
        "answer",
        "defined value",
        "defined mark",
        "check holds",
        "check fails",
        "answer error",
        "error in a maths box",
    ];
    let want_lines = [
        "maths outline on page",
        "maths caret in a maths box",
        "focus box in a maths box",
        "parent box in a maths box",
    ];
    for mode in [Mode::Light, Mode::Dark] {
        let p = mode.palette();
        let text = p.text_pairs();
        let lines = p.line_pairs();
        for name in want_text {
            let pairs: Vec<_> = text.iter().filter(|(n, _, _)| *n == name).collect();
            assert!(!pairs.is_empty(), "{mode:?}: no pair named {name}");
            for (n, fg, bg) in pairs {
                let r = contrast(*fg, *bg);
                assert!(r >= 4.5, "{mode:?}: {n} is {r:.2}, needs 4.5");
            }
        }
        for name in want_lines {
            let (_, fg, bg) = lines
                .iter()
                .find(|(n, _, _)| *n == name)
                .unwrap_or_else(|| panic!("{mode:?}: no pair named {name}"));
            let r = contrast(*fg, *bg);
            assert!(r >= 3.0, "{mode:?}: {name} is {r:.2}, needs 3");
        }
    }
}

/// The text of every node in the AccessKit tree that has a label.
fn labels(h: &Harness<'static, IdeApp>) -> Vec<String> {
    h.query_all_by_label_contains("")
        .filter_map(|n| n.accesskit_node().label())
        .collect()
}

#[test]
fn formulas_are_spoken_in_the_accessibility_tree() {
    let src = "Some words.\n\n$$ x^2 + 5 $$\n\n$$ \\frac{1}{2} + \\frac{1}{3} = ? $$\n\nAnd $y := 3$ inline.";
    let mut h = harness(app_with(src));
    h.run_steps(4);
    let all = labels(&h);
    for want in [
        "x squared plus 5",
        "1 over 2 plus 1 over 3 equals",
        "y is defined as 3",
    ] {
        assert!(
            all.iter().any(|l| l == want),
            "no node says {want:?}; labels: {all:?}"
        );
    }
}

fn size(h: &Harness<'static, IdeApp>) -> f32 {
    h.state().size
}

fn band_height(h: &Harness<'static, IdeApp>) -> f32 {
    let pc = h.state().doc().and_then(|d| d.page_cache()).expect("page");
    let b = pc
        .page
        .blocks
        .iter()
        .find(|b| b.tag == mpp_ide::page::BlockTag::Math)
        .expect("a maths block");
    b.height
}

fn prose_gap(h: &Harness<'static, IdeApp>) -> f32 {
    // The top of the maths block is below one prose line: it grows with the zoom.
    let pc = h.state().doc().and_then(|d| d.page_cache()).expect("page");
    pc.page
        .blocks
        .iter()
        .find(|b| b.tag == mpp_ide::page::BlockTag::Math)
        .expect("a maths block")
        .top
}

#[test]
fn zoom_scales_maths_and_prose_together_and_reset_is_ctrl_alt_0() {
    let mut h = harness(app_with("Some words here.\n\n$$ x^2 + 5 $$\n"));
    h.run_steps(3);
    let (s0, m0, p0) = (size(&h), band_height(&h), prose_gap(&h));
    h.key_press_modifiers(Modifiers::COMMAND, Key::Equals);
    h.run_steps(3);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Equals);
    h.run_steps(3);
    let (s1, m1, p1) = (size(&h), band_height(&h), prose_gap(&h));
    assert!(s1 > s0, "Ctrl+= made it bigger: {s0} -> {s1}");
    assert!(m1 > m0, "maths grew: {m0} -> {m1}");
    assert!(p1 > p0, "prose grew: {p0} -> {p1}");
    let (rm, rp, rs) = (m1 / m0, p1 / p0, s1 / s0);
    assert!(
        (rm - rs).abs() < 0.15 && (rp - rs).abs() < 0.15,
        "maths x{rm:.2} and prose x{rp:.2} follow the size x{rs:.2}"
    );
    h.key_press_modifiers(Modifiers::COMMAND, Key::Minus);
    h.run_steps(3);
    assert!(size(&h) < s1, "Ctrl+- made it smaller");
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::ALT, Key::Num0);
    h.run_steps(3);
    assert_eq!(size(&h), s0, "Ctrl+Alt+0 puts the zoom back");
}

#[test]
fn a_formula_is_reachable_and_editable_with_the_keyboard_alone() {
    // No pointer at all: Ctrl+M, typing, Esc, arrows.
    let mut h = harness(app_with("x := 5"));
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::M);
    h.run_steps(2);
    let t = h.state().doc().expect("doc").editor.text().to_string();
    assert_eq!(t, "$$ x := 5 $$");
    h.key_press(Key::Escape);
    h.run_steps(2);
    h.key_press(Key::ArrowDown);
    h.run_steps(2);
    assert!(h
        .state()
        .doc()
        .is_some_and(|d| d.editor.caret() <= d.editor.text().len()));
    // The menu bar is in the tree, so a screen reader can reach it.
    let help = h.get_by_label("Help");
    assert!(!help.accesskit_node().is_hidden());
}
