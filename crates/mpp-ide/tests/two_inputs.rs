//! Typing for mathematicians and coders (U18) and one way out of every structure
//! (U19), driven through the real app: key and text events go through `ui()`,
//! which runs `pump()`, one frame per key, as a person types.

use egui::{Event, Key, Modifiers, Vec2};
use egui_kittest::kittest::Queryable;
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

/// A new file holding `text`, with the caret at `caret`.
fn app_at(text: &str, caret: usize) -> Harness<'static, IdeApp> {
    let mut app = IdeApp::empty();
    app.new_file();
    let doc = app.doc_mut().expect("doc");
    doc.editor.set_text(text);
    doc.editor.move_caret(caret, false);
    let mut h = harness(app);
    h.run_steps(3);
    h
}

/// An empty maths line with the caret in it.
fn maths_line() -> Harness<'static, IdeApp> {
    app_at("$$  $$", 3)
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

fn text(h: &Harness<'static, IdeApp>) -> String {
    h.state().doc().expect("doc").editor.text().to_string()
}

/// The maths between the `$$`s.
fn maths(h: &Harness<'static, IdeApp>) -> String {
    let t = text(h);
    t.trim_start_matches("$$ ")
        .trim_end_matches(" $$")
        .to_string()
}

fn parses(src: &str) {
    let doc = mpp_syntax::document::parse_document(src);
    assert!(doc.errors().next().is_none(), "{src}");
}

#[test]
fn a_coder_types_code_and_gets_maths() {
    let mut h = maths_line();
    type_text(&mut h, "x**2 <= sqrt(y) + abs(z - 1)");
    assert_eq!(maths(&h), r"x^{2} ≤ \sqrt{y} + |z - 1|");
    parses(&text(&h));
    // The first Ctrl+Z gives back exactly what was typed.
    press(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(maths(&h), r"x^{2} ≤ \sqrt{y} + abs(z - 1)");
}

#[test]
fn a_coder_types_sums_integrals_derivatives_and_comments() {
    let mut h = maths_line();
    type_text(&mut h, "sum(n**2, n, 1, 10) + int(x, x, 0, 1) # area");
    assert_eq!(
        maths(&h),
        r"\sum_{n=1}^{10} n^{2} + \int_{0}^{1} x \, dx % area"
    );
    parses(&text(&h));
    let mut h = maths_line();
    type_text(&mut h, "diff(x**3, x) != inf // check");
    assert_eq!(maths(&h), r"\frac{d}{dx} x^{3} ≠ ∞ % check");
    parses(&text(&h));
}

#[test]
fn a_mathematician_types_latex_and_it_stays_latex() {
    let mut h = maths_line();
    type_text(&mut h, r"\frac{a}{b} + \alpha_{0}^{2} \le \sqrt{x}");
    assert_eq!(maths(&h), r"\frac{a}{b} + α_{0}^{2} ≤ \sqrt{x}");
    parses(&text(&h));
    let mut h = maths_line();
    type_text(&mut h, r"\int_{0}^{1} x \cdot y \, dx");
    assert_eq!(maths(&h), r"\int_{0}^{1} x · y \, dx");
    parses(&text(&h));
}

#[test]
fn an_operator_or_a_space_leaves_an_exponent() {
    let mut h = maths_line();
    type_text(&mut h, "x^2+y_1 = 3");
    assert_eq!(maths(&h), "x^{2}+y_{1} = 3");
}

#[test]
fn copy_offers_latex_plain_text_and_code() {
    let src = r"$$ \frac{α}{2} $$";
    let mut h = app_at(src, 3);
    h.state_mut()
        .doc_mut()
        .expect("doc")
        .editor
        .select(3..src.len() - 3);
    h.step();
    let at = h
        .state()
        .doc()
        .and_then(|d| d.caret_screen)
        .expect("caret")
        .center();
    h.event(Event::PointerMoved(at));
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.run_steps(2);
    for label in ["Copy as LaTeX", "Copy as plain text"] {
        assert!(h.query_by_label(label).is_some(), "{label} is offered");
    }
    h.get_by_label("Copy as code").click();
    h.step();
    let copied = h
        .output()
        .platform_output
        .commands
        .iter()
        .find_map(|c| match c {
            egui::OutputCommand::CopyText(t) => Some(t.clone()),
            _ => None,
        });
    assert_eq!(copied.as_deref(), Some("alpha/2"));
}
