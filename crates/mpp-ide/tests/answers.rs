//! Answers in their own slot (U16), driven through the
//! real app the way a person uses it: key and text events and pointer clicks, through
//! `ui()` and its `pump()`, with frames only when the app asks for them.

use std::time::{Duration, Instant};

use egui::{Event, Modifiers, PointerButton, Pos2, Vec2};
use egui_kittest::Harness;
use mpp_ide::page::{answers, LinkTarget};
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

/// The person stops: frames run only while the app asks for them.
fn idle(h: &mut Harness<'static, IdeApp>, limit: Duration) {
    let start = Instant::now();
    while start.elapsed() < limit {
        if !h.ctx.has_requested_repaint() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
        h.step();
    }
}

/// Idles until the page shows the run of the text on screen.
fn until_current(h: &mut Harness<'static, IdeApp>) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(20) {
        if h.state().doc().is_some_and(|d| d.page_is_current()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
        h.step();
    }
    panic!("the answers never arrived");
}

/// Every answer on the page: what a click copies, its hover text and its rectangle.
fn answers_on(h: &Harness<'static, IdeApp>) -> Vec<(String, String, egui::Rect)> {
    let doc = h.state().doc().expect("doc");
    let Some(pc) = doc.page_cache() else {
        return vec![];
    };
    pc.page
        .links
        .iter()
        .filter_map(|l| match &l.link.target {
            LinkTarget::Answer(t) => {
                Some((t.clone(), l.link.tip.clone().unwrap_or_default(), l.rect))
            }
            _ => None,
        })
        .collect()
}

/// The top of every block on screen, and where the caret is drawn.
fn layout(h: &Harness<'static, IdeApp>) -> (Vec<f32>, Pos2) {
    let doc = h.state().doc().expect("doc");
    let pc = doc.page_cache().expect("page");
    let tops = pc.page.blocks.iter().map(|b| b.top).collect();
    (tops, doc.caret_screen.expect("caret").center())
}

/// Where the page's origin is on screen.
fn origin(h: &Harness<'static, IdeApp>) -> Vec2 {
    h.state().diag.origin.to_vec2()
}

fn click(h: &mut Harness<'static, IdeApp>, at: Pos2, modifiers: Modifiers) {
    h.event(Event::PointerMoved(at));
    h.step();
    for pressed in [true, false] {
        h.event_modifiers(
            Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers,
            },
            modifiers,
        );
        h.step();
    }
}

const START: &str = "# This is a test version of the math implementation\n\n$$ x := 5 $$\n\n$$ E := 2x + 5 $$\n\n$$ 1.321321312 + 5 $$\n";

#[test]
fn bens_video_lines_answer_by_themselves_and_nothing_moves() {
    // The caret after `+ 5`, where Ben typed the `=`.
    let caret = START.rfind(" $$").expect("the last line");
    let mut h = harness(app_with(START, caret));
    h.run_steps(3);
    until_current(&mut h);
    // `E := 2x + 5` shows `= 15` in grey; `x := 5` shows nothing.
    let got = answers_on(&h);
    assert_eq!(
        got.iter().map(|a| a.0.as_str()).collect::<Vec<_>>(),
        ["15"],
        "{got:?}"
    );
    assert!(got[0].1.contains("= 15"));
    // Ben types ` =`, then stops.
    for c in [" ", "="] {
        h.event(Event::Text(c.into()));
        h.step();
    }
    let typed = layout(&h);
    idle(&mut h, Duration::from_secs(10));
    until_current(&mut h);
    let got = answers_on(&h);
    assert!(
        got.iter().any(|a| a.0 == "6.321321312"),
        "the answer never showed: {got:?}"
    );
    assert_eq!(
        layout(&h),
        typed,
        "a line or the caret moved when the answer arrived"
    );
    let text = h.state().doc().expect("doc").editor.text().to_string();
    assert!(text.ends_with("$$ 1.321321312 + 5 = $$\n"), "{text:?}");
}

#[test]
fn a_click_copies_an_answer_and_ctrl_click_inserts_it() {
    let text = "$$ 2 + 3 = $$\n\nTotal: ";
    let mut h = harness(app_with(text, text.len()));
    h.run_steps(3);
    until_current(&mut h);
    let (copy, _, rect) = answers_on(&h)
        .into_iter()
        .find(|a| a.0 == "5")
        .expect("the answer 5");
    let at = rect.center() + origin(&h);
    click(&mut h, at, Modifiers::NONE);
    let last = h
        .ctx
        .data(|d| d.get_temp::<String>(egui::Id::unique(answers::LAST_ANSWER)));
    assert_eq!(last.as_deref(), Some(copy.as_str()));
    // The caret did not go into the answer: it is still at the end.
    let doc = h.state().doc().expect("doc");
    assert_eq!(doc.editor.caret(), text.len());
    assert_eq!(doc.editor.text(), text);
    // Ctrl+click puts it where the caret is.
    let at = rect.center() + origin(&h);
    click(&mut h, at, Modifiers::COMMAND);
    let doc = h.state().doc().expect("doc");
    assert_eq!(doc.editor.text(), format!("{text}5"));
}
