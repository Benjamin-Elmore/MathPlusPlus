//! Replay tests: the IDE driven the way a person uses it, through the real frame
//! loop. A frame only happens when the app asked for one (or on input), exactly
//! like the desktop event loop, so a missing repaint shows up as a failure.

use std::time::{Duration, Instant};

use egui::{Event, Vec2};
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

/// One key or piece of text, then the frame the input causes.
fn type_text(h: &mut Harness<'static, IdeApp>, s: &str) {
    for c in s.chars() {
        h.event(Event::Text(c.to_string()));
        h.step();
    }
}

/// The person stops typing. Frames run only while the app asks for them (as the
/// real event loop does), for at most `limit`.
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

fn current(h: &Harness<'static, IdeApp>) -> bool {
    h.state().doc().is_some_and(|d| d.page_is_current())
}

#[test]
fn an_answer_appears_without_another_key_press() {
    let mut app = IdeApp::empty();
    app.new_file();
    let mut h = harness(app);
    h.run_steps(2);
    type_text(&mut h, "$$ 1 + 2 = ? $$");
    // Saved straight away (Ctrl+S, or autosave): nothing else is left to redraw for.
    let dir = std::env::temp_dir().join(format!("mpp-replay-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    h.state_mut()
        .doc_mut()
        .expect("doc")
        .save_as(&dir.join("answer.mpp"))
        .expect("save");
    h.step();
    idle(&mut h, Duration::from_secs(10));
    assert!(
        current(&h),
        "the answer was worked out but the page was never drawn again to show it"
    );
}

/// Where the caret was drawn in the last frame (its middle, in screen pixels).
fn caret_at(h: &Harness<'static, IdeApp>) -> egui::Pos2 {
    h.state()
        .doc()
        .and_then(|d| d.caret_screen)
        .expect("the caret is drawn")
        .center()
}

/// A person typing: one key per frame, and after each word a pause in which
/// frames run only while the app asks for them. After every single frame the
/// caret is checked against `check` (which gets the caret and the text typed
/// so far).
fn type_with_pauses(
    h: &mut Harness<'static, IdeApp>,
    words: &[&str],
    mut check: impl FnMut(egui::Pos2, &str),
) -> usize {
    let mut frames = 0;
    let mut typed = String::new();
    for word in words {
        for c in word.chars() {
            h.event(Event::Text(c.to_string()));
            h.step();
            typed.push(c);
            frames += 1;
            check(caret_at(h), &typed);
        }
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(3) && h.ctx.has_requested_repaint() {
            std::thread::sleep(Duration::from_millis(15));
            h.step();
            frames += 1;
            check(caret_at(h), &typed);
        }
    }
    frames
}

fn app_with(text: &str) -> IdeApp {
    let mut app = IdeApp::empty();
    app.new_file();
    let doc = app.doc_mut().expect("doc");
    doc.editor.set_text(text);
    doc.editor.move_caret(text.len(), false);
    doc.scroll_to = Some(mpp_syntax::Span::new(text.len(), text.len()));
    app
}

#[test]
fn the_caret_stays_on_its_line_while_typing_with_pauses() {
    // The video: a heading, a definition, then a new line typed into with a
    // pause after each word while answers arrive above it.
    let start = "# Typing test\n\n$$ x := 5 $$\n\n";
    let mut h = harness(app_with(start));
    h.run_steps(3);
    let line = caret_at(&h).y;
    let x0 = caret_at(&h).x;
    let frames = type_with_pauses(
        &mut h,
        &["$$ ", "E ", "= ", "2x ", "+ ", "5 ", "= ", "? ", "$$"],
        |at, typed| {
            // Half of the closing `$$` is drawn as a `$` of its own under the
            // line until the second one closes it: the caret is right to be there.
            if typed.ends_with("? $") {
                return;
            }
            assert!(
                (at.y - line).abs() < 6.0,
                "after {typed:?} the caret was drawn at y={} but the line is at y={line}",
                at.y
            );
            // The caret never goes back to the start of the line by itself.
            assert!(
                at.x > x0 + 1.0,
                "after {typed:?} the caret was drawn at the start of the line"
            );
        },
    );
    assert!(frames > 20, "only {frames} frames ran");
    let text = h.state().doc().expect("doc").editor.text().to_string();
    assert_eq!(text, format!("{start}$$ E = 2x + 5 = ? $$"));
}

#[test]
fn paused_frames_never_move_the_caret() {
    let mut h = harness(app_with("# Notes\n\nHello"));
    h.run_steps(3);
    type_with_pauses(&mut h, &[" ", "world ", "and ", "more "], {
        let mut prev: Option<(String, egui::Pos2)> = None;
        move |at, typed| {
            if let Some((t, p)) = &prev {
                if t == typed {
                    assert!(
                        (p.x - at.x).abs() < 0.5 && (p.y - at.y).abs() < 0.5,
                        "the caret moved by itself after {typed:?}: {p:?} -> {at:?}"
                    );
                }
            }
            prev = Some((typed.to_string(), at));
        }
    });
}

#[test]
fn working_arriving_above_the_caret_does_not_move_its_line() {
    // Someone is typing at the bottom of a long file. Between two words they
    // switch on worked steps for every answer; when the run finishes, the
    // working of the derivative near the top makes it several lines taller.
    // The line being typed on must stay where it is on screen.
    let mut text = String::from("# Pinned\n\n$$ \\frac{d}{dx} \\sin(x^2) = ? $$\n\n");
    for i in 0..40 {
        text.push_str(&format!("Line {i} of filler text.\n\n"));
    }
    text.push_str("Typing here");
    let mut h = harness(app_with(&text));
    h.run_steps(3);
    idle(&mut h, Duration::from_secs(10));
    let line = caret_at(&h).y;
    let before = h.state().doc().and_then(|d| d.pin).expect("pinned").top;
    let check = |at: egui::Pos2, typed: &str| {
        assert!(
            (at.y - line).abs() < 2.0,
            "after {typed:?} the caret's line moved on screen: {line} -> {}",
            at.y
        );
    };
    type_with_pauses(&mut h, &[" now"], check);
    h.state_mut().eval_settings.show_steps = true;
    type_with_pauses(&mut h, &[" and", " more"], check);
    assert!(current(&h), "the working never arrived");
    // The line did move down the page (the working above it takes space).
    let after = h.state().doc().and_then(|d| d.pin).expect("pinned").top;
    assert!(
        after - before > 5.0,
        "nothing above the line changed: {before} -> {after}"
    );
}

fn press(h: &mut Harness<'static, IdeApp>, key: egui::Key) {
    h.key_press(key);
    h.step();
}

fn text_of(h: &Harness<'static, IdeApp>) -> String {
    h.state().doc().expect("doc").editor.text().to_string()
}

#[test]
fn left_from_the_start_of_a_line_goes_to_the_end_of_the_line_above() {
    // The video at 38 s: Left from the start of `E = 2x +5` went to an
    // invisible line between the heading and E, and typing went there.
    let src = "# This is a test\n\nE = 2x +5";
    let mut app = app_with(src);
    let e = src.find('E').expect("E");
    app.doc_mut().expect("doc").editor.move_caret(e, false);
    let mut h = harness(app);
    h.run_steps(3);
    let on_e = caret_at(&h);
    press(&mut h, egui::Key::ArrowLeft);
    let after = caret_at(&h);
    assert!(after.y < on_e.y - 10.0, "the caret did not go up a line");
    type_text(&mut h, "!");
    assert_eq!(text_of(&h), "# This is a test!\n\nE = 2x +5");
    // Right from the end of the heading comes back to the start of E.
    press(&mut h, egui::Key::ArrowRight);
    type_text(&mut h, "x");
    assert_eq!(text_of(&h), "# This is a test!\n\nxE = 2x +5");
}

#[test]
fn up_and_down_keep_the_column_and_home_and_end_go_to_the_line_ends() {
    let src = "A first line of prose\n\nShort\n\nA third line of prose";
    let mut app = app_with(src);
    let at = src.find("line").expect("line");
    app.doc_mut().expect("doc").editor.move_caret(at, false);
    let mut h = harness(app);
    h.run_steps(3);
    let x0 = caret_at(&h).x;
    press(&mut h, egui::Key::ArrowDown);
    press(&mut h, egui::Key::ArrowDown);
    let x2 = caret_at(&h).x;
    assert!((x2 - x0).abs() < 12.0, "the column moved from {x0} to {x2}");
    type_text(&mut h, "|");
    let t = text_of(&h);
    assert!(
        t.ends_with("A third |line of prose") || t.ends_with("A third| line of prose"),
        "typed somewhere else: {t}"
    );
    press(&mut h, egui::Key::Home);
    type_text(&mut h, "<");
    press(&mut h, egui::Key::End);
    type_text(&mut h, ">");
    let t = text_of(&h);
    let last = t.rsplit("\n\n").next().expect("a last line");
    assert!(
        last.starts_with("<A third") && last.ends_with("prose>"),
        "{t}"
    );
}

/// A click (press and release) at a screen point (the page's own clicks).
fn click_at(h: &mut Harness<'static, IdeApp>, pos: egui::Pos2) {
    h.event(Event::PointerMoved(pos));
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
    }
    h.step();
}

/// True when typing a letter changes the document (the page has the keyboard).
fn typing_reaches_the_page(h: &mut Harness<'static, IdeApp>) -> bool {
    let before = text_of(h);
    type_text(h, "z");
    let after = text_of(h);
    if after != before {
        // Take the letter back out.
        h.state_mut().doc_mut().expect("doc").editor.undo();
        h.step();
        true
    } else {
        false
    }
}

#[test]
fn escape_does_one_thing_then_lets_go_of_the_keyboard() {
    // In inline maths, Esc leaves the maths: typing goes just after it.
    let src = "Hello $x$ world";
    let mut app = app_with(src);
    app.doc_mut().expect("doc").editor.move_caret(7, false);
    let mut h = harness(app);
    h.run_steps(3);
    press(&mut h, egui::Key::Escape);
    type_text(&mut h, "!");
    assert_eq!(text_of(&h), "Hello $x$! world");
    // In prose with nothing open, Esc lets go of the keyboard, and the page
    // does not take it back by itself, however many frames go by.
    press(&mut h, egui::Key::Escape);
    h.run_steps(5);
    assert!(!typing_reaches_the_page(&mut h), "Esc did not let go");
    // A click on the page takes it back.
    let at = caret_at(&h);
    click_at(&mut h, at);
    assert!(
        typing_reaches_the_page(&mut h),
        "a click did not take it back"
    );
}

#[test]
fn escape_leaves_a_template_box() {
    let mut h = harness(app_with("$$  $$"));
    h.state_mut()
        .doc_mut()
        .expect("doc")
        .editor
        .move_caret(3, false);
    h.run_steps(3);
    for c in ["i", "n", "t", " ", "x"] {
        type_text(&mut h, c);
    }
    assert!(text_of(&h).contains("\\int"), "{}", text_of(&h));
    press(&mut h, egui::Key::Escape);
    type_text(&mut h, " = ?");
    let t = text_of(&h);
    assert!(
        t.contains("dx = ?"),
        "typing did not go after the template: {t}"
    );
}

#[test]
fn tab_is_only_kept_while_there_is_a_box_to_go_to() {
    let mut h = harness(app_with("Plain prose"));
    h.run_steps(3);
    assert!(typing_reaches_the_page(&mut h));
    press(&mut h, egui::Key::Tab);
    h.run_steps(2);
    assert_eq!(text_of(&h), "Plain prose", "Tab typed something");
    assert!(!typing_reaches_the_page(&mut h), "Tab was kept by the page");
}

#[test]
fn the_page_never_takes_the_keyboard_from_another_widget() {
    let mut h = harness(app_with("Some text"));
    h.run_steps(3);
    assert!(typing_reaches_the_page(&mut h));
    // Quick open has a search box: what is typed goes there, frame after frame.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::P);
    h.run_steps(3);
    assert!(h.state().ws.quick.open);
    type_text(&mut h, "abc");
    h.run_steps(5);
    assert_eq!(text_of(&h), "Some text", "the page took the keys");
    assert_eq!(h.state().ws.quick.query, "abc");
    // Closed again, the keyboard comes back to the page.
    press(&mut h, egui::Key::Escape);
    h.run_steps(3);
    assert!(!h.state().ws.quick.open);
    assert!(typing_reaches_the_page(&mut h));
    // A click on a menu, then Esc: typing goes to the page again.
    h.get_by_label("Edit").click();
    h.run_steps(3);
    press(&mut h, egui::Key::Escape);
    h.run_steps(3);
    assert!(typing_reaches_the_page(&mut h), "lost after a menu");
}

#[test]
fn an_arrow_at_the_edge_of_the_document_lets_go() {
    let src = "One line";
    let mut app = app_with(src);
    app.doc_mut().expect("doc").editor.move_caret(0, false);
    let mut h = harness(app);
    h.run_steps(3);
    // Right moves inside the text as usual.
    press(&mut h, egui::Key::ArrowRight);
    assert!(typing_reaches_the_page(&mut h));
    assert_eq!(text_of(&h), src);
    press(&mut h, egui::Key::Home);
    // Up on the first line, already at its start: nowhere to go in the page,
    // so the keyboard moves up to the tabs and menus.
    press(&mut h, egui::Key::ArrowUp);
    h.run_steps(3);
    assert!(
        !typing_reaches_the_page(&mut h),
        "the page kept the keyboard"
    );
}
