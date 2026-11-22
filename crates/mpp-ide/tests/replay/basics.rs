//! The first replay tests (phase A): the IDE driven the way a person uses it,
//! through the real frame loop. A frame only happens when the app asked for one
//! (or on input), exactly like the desktop event loop, so a missing repaint
//! shows up as a failure.

use std::time::{Duration, Instant};

use egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use mpp_ide::IdeApp;

use super::runner::{app_with, caret_at, click_at, fresh, harness, idle, text_of, type_text};

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
            // Every frame, including the one between the two `$` of the
            // closing `$$` (the editor wrote the closing, so the typed one
            // steps over it and no half-typed `$` is ever drawn).
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
    assert_eq!(text_of(&h), format!("{start}$$ E = 2x + 5 = ? $$"));
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

#[test]
fn left_from_the_start_of_a_line_goes_to_the_end_of_the_line_above() {
    // The video at 38 s: Left from the start of `E = 2x +5` went to an
    // invisible line between the heading and E, and typing went there.
    let src = "# This is a test\n\nE = 2x +5";
    let mut app = app_with(src);
    let e = src.find('E').expect("E");
    app.doc_mut().expect("doc").editor.move_caret(e, false);
    // Only the arrows are tested here: leaving `E = 2x +5` would otherwise
    // turn it into a maths line (auto-detect, tested in maths_lines.rs).
    app.doc_mut().expect("doc").maths.auto_off = true;
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
    // In prose with nothing open, Esc lets go of the keyboard: keys that move
    // the caret no longer reach the page, however many frames go by.
    press(&mut h, egui::Key::Escape);
    h.run_steps(5);
    assert!(!page_moves_the_caret(&mut h), "Esc did not let go");
    // But typing never vanishes (U28, S4): a letter typed then still lands
    // on the page, and the page has the keyboard again.
    assert!(typing_reaches_the_page(&mut h), "a typed letter was lost");
    assert!(page_moves_the_caret(&mut h));
    // A click on the page takes it back too.
    press(&mut h, egui::Key::Escape);
    h.run_steps(3);
    let at = caret_at(&h);
    click_at(&mut h, at);
    assert!(page_moves_the_caret(&mut h), "a click did not take it back");
}

/// True when End moves the caret (the page has the keyboard); the caret is
/// put back where it was.
fn page_moves_the_caret(h: &mut Harness<'static, IdeApp>) -> bool {
    let doc = h.state().doc().expect("doc");
    let before = doc.editor.caret();
    let end = doc.editor.text().len();
    if before == end {
        press(h, egui::Key::Home);
    } else {
        press(h, egui::Key::End);
    }
    let moved = h.state().doc().expect("doc").editor.caret() != before;
    h.state_mut()
        .doc_mut()
        .expect("doc")
        .editor
        .move_caret(before, false);
    h.step();
    moved
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

/// U28 (P1) replaces U10's "Tab only while there is a box": Tab that moved
/// the keyboard off the page lost everything typed after it. With no box to
/// go to, Tab now types nothing and keeps the keyboard; Esc leaves the page.
#[test]
fn tab_never_takes_the_keyboard_off_the_page() {
    let mut h = harness(app_with("Plain prose"));
    h.run_steps(3);
    assert!(typing_reaches_the_page(&mut h));
    press(&mut h, egui::Key::Tab);
    h.run_steps(2);
    assert_eq!(text_of(&h), "Plain prose", "Tab typed something");
    assert!(page_moves_the_caret(&mut h), "Tab took the keyboard away");
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Tab);
    h.run_steps(2);
    assert!(
        page_moves_the_caret(&mut h),
        "Shift+Tab took the keyboard away"
    );
    assert!(typing_reaches_the_page(&mut h));
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
    assert!(!page_moves_the_caret(&mut h), "the page kept the keyboard");
    // A letter typed then is not lost (U28, S4): it goes to the page.
    assert!(typing_reaches_the_page(&mut h), "a typed letter was lost");
}

// ---- input rules (lane A3): Enter, Backspace, lists and headings ----

fn key(h: &mut Harness<'static, IdeApp>, mods: Modifiers, key: Key) {
    h.key_press_modifiers(mods, key);
    h.step();
}

fn plain(h: &mut Harness<'static, IdeApp>, k: Key) {
    key(h, Modifiers::NONE, k);
}

fn ctrl(h: &mut Harness<'static, IdeApp>, k: Key) {
    key(h, Modifiers::COMMAND, k);
}

/// Blocks of the parsed file, by kind name, as the real parser sees the text.
fn lists_in(src: &str) -> usize {
    let doc = mpp_syntax::document::parse_document(src);
    doc.blocks
        .iter()
        .filter(|b| matches!(b.kind, mpp_syntax::document::BlockKind::List(_)))
        .count()
}

#[test]
fn enter_ends_the_block_and_the_file_says_so() {
    let mut h = fresh();
    type_text(&mut h, "first");
    plain(&mut h, Key::Enter);
    type_text(&mut h, "second");
    assert_eq!(text_of(&h), "first\n\nsecond");
}

#[test]
fn shift_enter_is_a_soft_break() {
    let mut h = fresh();
    type_text(&mut h, "first");
    key(&mut h, Modifiers::SHIFT, Key::Enter);
    type_text(&mut h, "second");
    assert_eq!(text_of(&h), "first\nsecond");
}

#[test]
fn enter_is_one_undo_step() {
    let mut h = fresh();
    type_text(&mut h, "first");
    plain(&mut h, Key::Enter);
    ctrl(&mut h, Key::Z);
    assert_eq!(text_of(&h), "first");
}

#[test]
fn backspace_at_the_start_of_a_block_joins_it_in_one_step() {
    let mut h = fresh();
    type_text(&mut h, "first");
    plain(&mut h, Key::Enter);
    type_text(&mut h, "second");
    for _ in 0..6 {
        plain(&mut h, Key::ArrowLeft);
    }
    plain(&mut h, Key::Backspace);
    assert_eq!(text_of(&h), "firstsecond");
    ctrl(&mut h, Key::Z);
    assert_eq!(text_of(&h), "first\n\nsecond");
}

#[test]
fn enter_inside_display_maths_makes_a_new_row() {
    let mut h = fresh();
    type_text(&mut h, "$$x := 1");
    plain(&mut h, Key::Enter);
    type_text(&mut h, "y := 2$$");
    let t = text_of(&h);
    // `$` on the empty line opened the maths line (U12): the editor wrote the
    // spaces and the closing `$$`, which the typed `$$` stepped over.
    assert_eq!(t, "$$ x := 1 \\\\\ny := 2 $$", "one block, a second row");
    assert!(!t.contains("\n\n"));
}

#[test]
fn a_list_marker_with_a_space_becomes_a_list_and_undo_gives_it_back() {
    let mut h = fresh();
    type_text(&mut h, "1.");
    assert_eq!(lists_in(&text_of(&h)), 0, "1. alone is only text");
    type_text(&mut h, " ");
    assert_eq!(lists_in(&text_of(&h)), 1);
    ctrl(&mut h, Key::Z);
    assert_eq!(text_of(&h), "1.");
    assert_eq!(lists_in(&text_of(&h)), 0);
}

#[test]
fn bullets_work_the_same_way() {
    let mut h = fresh();
    type_text(&mut h, "- ");
    assert_eq!(lists_in(&text_of(&h)), 1);
    ctrl(&mut h, Key::Z);
    assert_eq!(text_of(&h), "-");
    assert_eq!(lists_in(&text_of(&h)), 0);
}

#[test]
fn numbers_never_become_lists() {
    for s in ["1.3", "1.", "2.5x", "1.321321312 + 5 =", "-3", "*x"] {
        let mut h = fresh();
        type_text(&mut h, s);
        assert_eq!(text_of(&h), s);
        assert_eq!(lists_in(&text_of(&h)), 0, "{s} is not a list");
    }
}

#[test]
fn a_maths_line_never_becomes_a_list() {
    let mut h = fresh();
    type_text(&mut h, "$$1. 2 - 3 * 4$$");
    assert_eq!(lists_in(&text_of(&h)), 0);
}

#[test]
fn hash_then_a_letter_makes_a_heading_with_a_hint() {
    let mut h = fresh();
    type_text(&mut h, "#This is a test");
    assert_eq!(text_of(&h), "# This is a test");
    let doc = mpp_syntax::document::parse_document(&text_of(&h));
    assert!(matches!(
        doc.blocks[0].kind,
        mpp_syntax::document::BlockKind::Heading { .. }
    ));
}

#[test]
fn the_first_undo_after_the_heading_conversion_keeps_a_plain_hash() {
    let mut h = fresh();
    type_text(&mut h, "#T");
    assert_eq!(text_of(&h), "# T");
    let hint = h
        .state()
        .doc()
        .expect("doc")
        .editor
        .hint()
        .map(String::from);
    assert!(
        hint.is_some_and(|s| s.contains("Heading 1")),
        "a hint shows once"
    );
    ctrl(&mut h, Key::Z);
    assert_eq!(text_of(&h), "#T", "exactly what was typed");
}

#[test]
fn a_hash_without_a_letter_or_not_at_the_line_start_is_left_alone() {
    let mut h = fresh();
    type_text(&mut h, "# ");
    type_text(&mut h, "a #b 1#2");
    assert_eq!(text_of(&h), "# a #b 1#2");
    let mut h = fresh();
    type_text(&mut h, "x #y");
    assert_eq!(text_of(&h), "x #y");
}

#[test]
fn ctrl_digits_set_the_heading_level() {
    let mut h = fresh();
    type_text(&mut h, "Title");
    ctrl(&mut h, Key::Num2);
    assert_eq!(text_of(&h), "## Title");
    ctrl(&mut h, Key::Num1);
    assert_eq!(text_of(&h), "# Title");
    ctrl(&mut h, Key::Num3);
    assert_eq!(text_of(&h), "### Title");
    ctrl(&mut h, Key::Num0);
    assert_eq!(text_of(&h), "Title");
    ctrl(&mut h, Key::Z);
    assert_eq!(text_of(&h), "### Title");
}
