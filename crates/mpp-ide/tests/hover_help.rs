//! Hover help (U25): the card over an answer (exact and decimal forms, a Show
//! steps link) and the card over a red error (plain words, one-click fix).
//! Driven through the real app: pointer events go through `ui()`.

use egui::{Event, Modifiers, PointerButton, Pos2, Vec2};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use mpp_ide::diagnostics::Kind;
use mpp_ide::{theme, IdeApp};

const SIZE: Vec2 = Vec2::new(1180.0, 820.0);

fn settled(text: &str) -> Harness<'static, IdeApp> {
    let mut app = IdeApp::empty();
    app.new_file();
    if let Some(d) = app.doc_mut() {
        d.editor.set_text(text);
        d.editor.move_caret(0, false);
    }
    let mut h = Harness::builder()
        .with_size(SIZE)
        .with_max_steps(30)
        .with_context_setup(theme::install_fonts)
        .build_ui_state(|ui, app: &mut IdeApp| app.ui(ui), app);
    h.run_steps(3);
    let start = std::time::Instant::now();
    while !h
        .state()
        .doc()
        .is_some_and(|d| d.eval.is_some() && d.page_is_current())
    {
        assert!(start.elapsed().as_secs() < 20, "the answers never arrived");
        std::thread::sleep(std::time::Duration::from_millis(10));
        h.step();
    }
    h.run_steps(3);
    h
}

fn text(h: &Harness<'static, IdeApp>) -> String {
    h.state().doc().expect("doc").editor.text().to_string()
}

fn move_to(h: &mut Harness<'static, IdeApp>, p: Pos2) {
    h.event(Event::PointerMoved(p));
    h.run_steps(3);
}

fn click_at(h: &mut Harness<'static, IdeApp>, pos: Pos2) {
    move_to(h, pos);
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.run_steps(3);
}

/// The screen centre of the first answer on the page (a result box).
fn answer_pos(h: &Harness<'static, IdeApp>) -> Pos2 {
    let origin = h.state().diag.origin;
    let pc = h
        .state()
        .doc()
        .and_then(|d| d.page_cache())
        .expect("a page");
    let (top, hit) = pc
        .page
        .blocks
        .iter()
        .find_map(|b| {
            b.dl.hits
                .iter()
                .find(|x| x.kind == mpp_render::HitKind::Result)
                .map(|x| (b.top, x))
        })
        .expect("an answer on the page");
    Pos2::new(
        hit.rect.x + hit.rect.w / 2.0,
        top + hit.rect.y + hit.rect.h / 2.0,
    ) + origin.to_vec2()
}

/// The screen centre of the first problem of the page.
fn problem_pos(h: &mut Harness<'static, IdeApp>, want: impl Fn(&Kind) -> bool) -> Pos2 {
    let origin = h.state().diag.origin;
    let ps = h.state_mut().doc_mut().expect("doc").problems();
    let p = ps.iter().find(|p| want(&p.kind)).expect("a problem");
    p.rect.center() + origin.to_vec2()
}

const OFF: Pos2 = Pos2::new(900.0, 700.0);

#[test]
fn hovering_an_answer_shows_the_exact_and_decimal_forms() {
    let mut h = settled("# T\n\n$$ \\frac{1}{3} = ? $$\n");
    let at = answer_pos(&h);
    assert!(h.query_by_label("Exact").is_none());
    move_to(&mut h, at);
    h.get_by_label("Exact");
    h.get_by_label("Decimal");
    h.get_by_label("Show steps");
    // Moving away closes the card.
    move_to(&mut h, OFF);
    assert!(h.query_by_label("Exact").is_none());
}

#[test]
fn show_steps_on_the_card_asks_the_line_for_its_working() {
    let mut h = settled("# T\n\n$$ \\frac{1}{3} + \\frac{1}{6} = ? $$\n");
    let at = answer_pos(&h);
    move_to(&mut h, at);
    let at = h.get_by_label("Show steps");
    let pos = { at.rect().center() };
    click_at(&mut h, pos);
    assert!(text(&h).contains("= ?!"), "{}", text(&h));
}

#[test]
fn hovering_an_undefined_name_offers_define_and_the_fix_works() {
    let mut h = settled("# T\n\n$$ 2 q \\approx ? $$\n");
    let at = problem_pos(&mut h, |k| matches!(k, Kind::Undefined { .. }));
    move_to(&mut h, at);
    // In plain words, not "Undefined".
    let ps = h.state_mut().doc_mut().expect("doc").problems();
    assert!(ps.iter().any(|p| p.message.starts_with("q isn't defined")));
    let fix = h.get_by_label("Define q");
    let pos = { fix.rect().center() };
    // The pointer travels to the card: it stays open on the way.
    move_to(&mut h, Pos2::new(at.x + 8.0, at.y + 10.0));
    h.get_by_label("Define q");
    click_at(&mut h, pos);
    let t = text(&h);
    assert!(t.contains("q := "), "the name was defined: {t}");
}

#[test]
fn a_mistyped_function_suggests_the_closest_one_in_the_card() {
    let mut h = settled("# T\n\n$$ sinn(2) = ? $$\n");
    let at = problem_pos(&mut h, |k| matches!(k, Kind::LetterRun { .. }));
    move_to(&mut h, at);
    let fix = h.get_by_label("Use the name sin");
    let pos = { fix.rect().center() };
    click_at(&mut h, pos);
    assert!(text(&h).contains("\\sin"), "{}", text(&h));
}
