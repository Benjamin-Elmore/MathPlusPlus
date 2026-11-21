//! The Insert menu driven through the real app with egui_kittest: Ctrl+Space,
//! search, Enter, Tab through the boxes, the matrix picker, and screenshots in
//! light and dark (written to `target/ide-snapshots/`).

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

fn settled(app: IdeApp) -> Harness<'static, IdeApp> {
    let mut h = harness(app);
    h.run_steps(2);
    h.state_mut().settle();
    h.run_steps(3);
    h
}

fn app_with(text: &str, caret: usize, choice: theme::ThemeChoice) -> IdeApp {
    let mut app = IdeApp::empty();
    app.set_theme_choice(choice);
    app.new_file();
    let doc = app.doc_mut().expect("doc");
    doc.editor.set_text(text);
    doc.editor.move_caret(caret, false);
    app
}

fn text(h: &mut Harness<'static, IdeApp>, s: &str) {
    h.event(Event::Text(s.to_string()));
    h.run_steps(2);
}

fn key(h: &mut Harness<'static, IdeApp>, k: Key) {
    h.key_press(k);
    h.run_steps(2);
}

fn ctrl_space(h: &mut Harness<'static, IdeApp>) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::Space);
    h.run_steps(3);
}

fn doc_text(h: &Harness<'static, IdeApp>) -> String {
    h.state()
        .doc()
        .map(|d| d.editor.text().to_string())
        .unwrap_or_default()
}

/// Rendering is done one test at a time (the GPU device is shared).
static RENDER: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn save_png(h: &mut Harness<'static, IdeApp>, name: &str) {
    let _one = RENDER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let img = h.render().expect("render");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ide-snapshots");
    std::fs::create_dir_all(&dir).expect("create dir");
    img.save(dir.join(name)).expect("save png");
}

fn settle(h: &mut Harness<'static, IdeApp>) {
    h.state_mut().settle();
    h.run_steps(3);
}

#[test]
fn ctrl_space_search_and_enter_insert_a_fraction_then_tab_fills_it() {
    let mut h = settled(app_with("", 0, theme::ThemeChoice::Light));
    ctrl_space(&mut h);
    assert!(h.state().insert.menu.open, "Ctrl+Space opens the menu");
    text(&mut h, "fraction");
    key(&mut h, Key::Enter);
    assert!(!h.state().insert.menu.open, "choosing closes it");
    assert_eq!(doc_text(&h), "$$ \\frac{}{} $$");
    assert_eq!(h.state().insert.settings.recent, ["frac"]);
    // Typing goes into the numerator; Tab moves to the denominator.
    text(&mut h, "1");
    key(&mut h, Key::Tab);
    text(&mut h, "2");
    key(&mut h, Key::Tab);
    assert_eq!(doc_text(&h), "$$ \\frac{1}{2} $$");
    let shown = h.state_mut().visible_text(900.0);
    for bad in ["\\", "_", "^", "$", "frac"] {
        assert!(!shown.contains(bad), "{bad:?} visible in {shown}");
    }
}

#[test]
fn escape_closes_the_menu_and_typing_goes_back_to_the_page() {
    let mut h = settled(app_with("$$ x $$\n", 4, theme::ThemeChoice::Light));
    ctrl_space(&mut h);
    assert!(h.state().insert.menu.open);
    key(&mut h, Key::Escape);
    assert!(!h.state().insert.menu.open);
    h.run_steps(2);
    text(&mut h, "y");
    assert_eq!(doc_text(&h), "$$ xy $$\n");
}

#[test]
fn the_integral_shortcut_works_while_typing() {
    let mut h = settled(app_with("$$  $$\n", 3, theme::ThemeChoice::Light));
    for c in ["i", "n", "t", " "] {
        text(&mut h, c);
    }
    assert_eq!(doc_text(&h), "$$ \\int_{}^{} {} \\, dx $$\n");
    // The caret starts in the integrand.
    text(&mut h, "x");
    assert_eq!(doc_text(&h), "$$ \\int_{}^{} {x} \\, dx $$\n");
    // Tab walks reading order from the start: lower, upper, integrand, variable,
    // then out; leaving with the bounds empty makes an indefinite integral.
    for _ in 0..5 {
        key(&mut h, Key::Tab);
    }
    assert_eq!(doc_text(&h), "$$ \\int {x} \\, dx $$\n");
}

#[test]
fn tab_walks_a_definite_integral_in_reading_order() {
    let mut h = settled(app_with("$$  $$\n", 3, theme::ThemeChoice::Light));
    for c in ["i", "n", "t", " ", "x"] {
        text(&mut h, c);
    }
    // From the integrand the caret starts in, Tab goes to the lower bound, then
    // the upper bound, back to the integrand, the variable, and out.
    key(&mut h, Key::Tab);
    text(&mut h, "0");
    key(&mut h, Key::Tab);
    text(&mut h, "1");
    key(&mut h, Key::Tab);
    key(&mut h, Key::Tab);
    text(&mut h, "t");
    key(&mut h, Key::Tab);
    assert_eq!(doc_text(&h), "$$ \\int_{0}^{1} {x} \\, dt $$\n");
    // The caret has left the template: typing goes after it.
    text(&mut h, " = ?");
    assert_eq!(doc_text(&h), "$$ \\int_{0}^{1} {x} \\, dt = ? $$\n");
}

#[test]
fn snapshots_of_the_insert_menu_templates_and_matrix_picker() {
    for (suffix, choice) in [
        ("light", theme::ThemeChoice::Light),
        ("dark", theme::ThemeChoice::Dark),
    ] {
        let src = "# Inserting maths\n\nPress Ctrl+Space anywhere to insert maths.\n\n$$ f(x) := x^2 $$\n\n";
        let mut app = app_with(src, src.len(), choice);
        app.insert.settings.recent = vec!["int.def".into(), "frac".into(), "sum".into()];
        let mut h = settled(app);

        // The menu, open at the caret.
        ctrl_space(&mut h);
        h.run_steps(3);
        save_png(&mut h, &format!("insert-menu-{suffix}.png"));

        // A definite integral, empty: grey labels, bounds above and below.
        text(&mut h, "definite");
        key(&mut h, Key::Enter);
        settle(&mut h);
        assert!(
            doc_text(&h).contains("\\int_{}^{} {} \\, dx"),
            "{}",
            doc_text(&h)
        );
        save_png(&mut h, &format!("insert-integral-empty-{suffix}.png"));

        // Filled in: integrand first, then the bounds with Shift+Tab.
        text(&mut h, "x^2");
        h.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
        h.run_steps(2);
        text(&mut h, "1");
        h.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
        h.run_steps(2);
        text(&mut h, "0");
        for _ in 0..4 {
            key(&mut h, Key::Tab);
        }
        text(&mut h, " = ?");
        settle(&mut h);
        assert!(
            doc_text(&h).contains("\\int_{0}^{1} {x^2} \\, dx = ?"),
            "{}",
            doc_text(&h)
        );
        save_png(&mut h, &format!("insert-integral-filled-{suffix}.png"));

        // The matrix picker: choose Matrix, grow it to 2 × 3 with the arrow keys.
        let end = doc_text(&h).len();
        h.state_mut()
            .doc_mut()
            .expect("doc")
            .editor
            .move_caret(end, false);
        h.run_steps(2);
        ctrl_space(&mut h);
        text(&mut h, "matrix");
        key(&mut h, Key::Enter);
        assert!(
            h.state().insert.menu.picker.is_some(),
            "the size picker opens"
        );
        key(&mut h, Key::ArrowRight);
        h.run_steps(2);
        save_png(&mut h, &format!("insert-matrix-picker-{suffix}.png"));
        key(&mut h, Key::Enter);
        settle(&mut h);
        assert!(
            doc_text(&h).contains("\\begin{pmatrix} {} & {} & {} \\\\ {} & {} & {} \\end{pmatrix}"),
            "{}",
            doc_text(&h)
        );
        save_png(&mut h, &format!("insert-matrix-{suffix}.png"));
    }
}
