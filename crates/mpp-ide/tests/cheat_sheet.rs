//! The cheat sheet (U23): F1 opens it, it is generated from the forms table,
//! it is searchable and a click inserts a row at the caret. Driven through the
//! real app (`ui()` runs `pump()`).

use egui::{Event, Key, Modifiers, PointerButton, Pos2, Vec2};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use mpp_ide::editor::forms;
use mpp_ide::settings::SideView;
use mpp_ide::{theme, IdeApp};

const SIZE: Vec2 = Vec2::new(1280.0, 4000.0);

fn fresh() -> Harness<'static, IdeApp> {
    let mut app = IdeApp::empty();
    app.new_file();
    let mut h = Harness::builder()
        .with_size(SIZE)
        .with_max_steps(30)
        .with_context_setup(theme::install_fonts)
        .build_ui_state(|ui, app: &mut IdeApp| app.ui(ui), app);
    h.run_steps(2);
    h
}

fn f1(h: &mut Harness<'static, IdeApp>) {
    h.key_press(Key::F1);
    h.step();
    h.run_steps(3);
}

fn type_text(h: &mut Harness<'static, IdeApp>, s: &str) {
    for c in s.chars() {
        h.event(Event::Text(c.to_string()));
        h.step();
    }
    h.run_steps(2);
}

fn click_at(h: &mut Harness<'static, IdeApp>, pos: Pos2) {
    h.event(Event::PointerMoved(pos));
    h.step();
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.run_steps(2);
}

fn label_of(f: &forms::Form, code: bool) -> String {
    if code {
        format!("{}: type {} (code)", f.name, f.code)
    } else {
        format!("{}: type {} (LaTeX)", f.name, f.latex)
    }
}

#[test]
fn f1_opens_the_cheat_sheet() {
    let mut h = fresh();
    assert!(h.query_by_label("Cheat sheet panel").is_none());
    f1(&mut h);
    assert!(h.state().ws.side_visible);
    assert_eq!(h.state().ws.side_view, SideView::Help);
    assert!(h.query_by_label("Cheat sheet panel").is_some());
    h.get_by_label("If you know LaTeX");
    h.get_by_label("If you write code");
}

#[test]
fn every_row_of_the_forms_table_is_on_the_sheet() {
    let mut h = fresh();
    f1(&mut h);
    for f in forms::table() {
        for code in [false, true] {
            let l = label_of(f, code);
            assert!(h.query_by_label(&l).is_some(), "missing row: {l}");
        }
    }
    // The fixed sections are there too.
    h.get_by_label("Defines a name: x := 5: :=");
    h.get_by_label("Closes a popup, then leaves the box, then leaves maths: Esc");
}

#[test]
fn searching_filters_the_sheet() {
    let mut h = fresh();
    f1(&mut h);
    let frac = forms::table()
        .iter()
        .find(|f| f.id == "frac")
        .expect("frac");
    let sqrt = forms::table()
        .iter()
        .find(|f| f.id == "sqrt")
        .expect("sqrt");
    // F1 leaves the search box ready: typing goes there.
    type_text(&mut h, "fraction");
    assert!(h.query_by_label(&label_of(frac, true)).is_some());
    assert!(h.query_by_label(&label_of(sqrt, true)).is_none());
    assert_eq!(h.state().doc().expect("doc").editor.text(), "");
}

#[test]
fn clicking_a_row_inserts_it_at_the_caret() {
    let mut h = fresh();
    f1(&mut h);
    type_text(&mut h, "square root");
    let sqrt = forms::table()
        .iter()
        .find(|f| f.id == "sqrt")
        .expect("sqrt");
    let pos = h.get_by_label(&label_of(sqrt, true)).rect().center();
    click_at(&mut h, pos);
    let text = h.state().doc().expect("doc").editor.text().to_string();
    assert!(
        text.contains(sqrt.writes),
        "the row was inserted, got {text:?}"
    );
}

/// T-N7, S-N7: the sheet's text keeps off the panel's left edge, and the
/// column headings stay on one line.
#[test]
fn the_sheet_has_a_left_margin_and_one_line_headings() {
    let mut h = fresh();
    f1(&mut h);
    let panel = h.get_by_label("Cheat sheet panel").rect();
    let latex = h.get_by_label("If you know LaTeX").rect();
    let code = h.get_by_label("If you write code").rect();
    assert!(
        latex.left() >= panel.left() + 8.0,
        "the heading starts at {} by the panel's edge at {}",
        latex.left(),
        panel.left()
    );
    assert!(latex.height() < 24.0, "the heading wrapped: {latex:?}");
    assert!(code.height() < 24.0, "the heading wrapped: {code:?}");
}

#[test]
fn start_here_header_is_sentence_case_on_one_line_p12() {
    let mut h = fresh();
    f1(&mut h);
    let heads: Vec<_> = h.query_all_by_label_contains("Start here").collect();
    assert!(!heads.is_empty(), "the Start here header is on the sheet");
    assert!(
        h.query_all_by_label_contains("START HERE").next().is_none(),
        "the header is not in capitals"
    );
    let r = heads[0].rect();
    assert!(r.height() < 24.0, "the header is one line: {r:?}");
}

#[test]
fn the_note_key_ctrl_slash_is_on_the_sheet_p15() {
    let mut h = fresh();
    f1(&mut h);
    let rows: Vec<String> = h
        .query_all_by_label_contains("Ctrl+/")
        .filter_map(|n| n.accesskit_node().label())
        .collect();
    assert!(
        rows.iter().any(|l| l.to_lowercase().contains("note")),
        "Ctrl+/ is named on the sheet: {rows:?}"
    );
}
