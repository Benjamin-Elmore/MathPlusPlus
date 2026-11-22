//! U28 fix round (screen-reader persona R): what NVDA can read from the
//! AccessKit tree of the real app, driven by key events through the real
//! frame loop. One test per finding.

use std::time::{Duration, Instant};

use egui::accesskit::{Live, Role};
use egui::{Event, Key, Modifiers, Vec2};
use egui_kittest::kittest::{by, NodeT, Queryable};
use egui_kittest::Harness;
use mpp_ide::theme::{contrast, Mode};
use mpp_ide::{theme, IdeApp};

type H = Harness<'static, IdeApp>;

const SIZE: Vec2 = Vec2::new(1180.0, 820.0);

fn harness(app: IdeApp) -> H {
    Harness::builder()
        .with_size(SIZE)
        .with_max_steps(40)
        .with_context_setup(|ctx| {
            theme::install_fonts(ctx);
            ctx.enable_accesskit();
        })
        .build_ui_state(|ui, app: &mut IdeApp| app.ui(ui), app)
}

fn app_with(text: &str, caret: usize) -> H {
    let mut app = IdeApp::empty();
    app.new_file();
    let doc = app.doc_mut().expect("doc");
    doc.editor.set_text(text);
    doc.editor.move_caret(caret, false);
    let mut h = harness(app);
    h.run_steps(3);
    h
}

fn key(h: &mut H, m: Modifiers, k: Key) {
    h.key_press_modifiers(m, k);
    h.step();
}

fn type_text(h: &mut H, s: &str) {
    for c in s.chars() {
        h.event(Event::Text(c.to_string()));
        h.step();
    }
}

/// Runs frames until the run for the current text has finished and the page
/// shows it.
fn wait(h: &mut H) {
    let start = Instant::now();
    let mut settled = 0;
    while start.elapsed() < Duration::from_secs(8) {
        std::thread::sleep(Duration::from_millis(15));
        h.step();
        let done = h
            .state()
            .doc()
            .is_some_and(|d| !d.evaluating() && d.page_is_current());
        settled = if done { settled + 1 } else { 0 };
        if settled >= 4 {
            return;
        }
    }
}

struct Seen {
    role: Role,
    label: String,
    value: String,
}

fn seen(n: &egui_kittest::kittest::AccessKitNode<'_>) -> Seen {
    Seen {
        role: n.role(),
        label: n.label().unwrap_or_default(),
        value: n.value().unwrap_or_default(),
    }
}

fn focused(h: &H) -> Option<Seen> {
    h.query_all(by().predicate(|n| n.is_focused()))
        .next()
        .map(|n| seen(&n.accesskit_node()))
}

/// The words of every node with a label.
fn labels(h: &H) -> Vec<String> {
    h.query_all(by().predicate(|n| n.label().is_some()))
        .filter_map(|n| n.accesskit_node().label())
        .collect()
}

fn live(h: &H, kind: Live) -> String {
    h.query_all(by().predicate(move |n| n.live() == kind && n.role() != Role::Label))
        .filter_map(|n| n.accesskit_node().label())
        .collect::<Vec<_>>()
        .join(" | ")
}

fn polite(h: &H) -> String {
    h.state().access.polite().to_string()
}

// ---- R1 -------------------------------------------------------------------

#[test]
fn r1_the_editor_is_a_text_control_with_its_line_and_caret() {
    let src = "Hello world\n\nSecond line here\n\n$$ x^2 + 5 $$\n";
    let mut h = app_with(src, 3);
    let f = focused(&h).expect("the editor has the keyboard");
    assert_eq!(f.role, Role::MultilineTextInput);
    assert_eq!(f.label, "Untitled-1");
    assert_eq!(f.value, "Hello world");
    // The caret is exposed on a text run holding the line.
    let node = h
        .query_all(by().predicate(|n| n.is_focused()))
        .next()
        .expect("focused");
    let sel = node
        .accesskit_node()
        .data()
        .text_selection()
        .expect("the caret is exposed");
    assert_eq!(sel.focus.character_index, 3);
    let runs: Vec<String> = node
        .children()
        .filter(|c| c.accesskit_node().role() == Role::TextRun)
        .filter_map(|c| c.accesskit_node().value())
        .collect();
    assert_eq!(runs, vec!["Hello world".to_string()]);

    // Typing is echoed: the value follows in the same frame.
    type_text(&mut h, "XY");
    let f = focused(&h).expect("focused");
    assert_eq!(f.value, "HelXYlo world");

    // In maths the value is the spoken formula.
    let at = src.find("x^2").unwrap() + 1;
    h.state_mut()
        .doc_mut()
        .unwrap()
        .editor
        .move_caret(at + 2, false);
    h.run_steps(2);
    let f = focused(&h).expect("focused");
    assert!(
        f.value.starts_with("x squared plus 5"),
        "maths value: {:?}",
        f.value
    );
}

// ---- R5 / R7 / R8 ---------------------------------------------------------

#[test]
fn r5_r7_r8_maths_nodes_say_their_answer_and_state() {
    let src = "$$ R := 35.31194307 $$\n\n$$ R = ? $$\n\n$$ R = 35.3 $$\n\n$$ R = 35.4 $$\n\n$$ R = 35.31194307 $$\n\n$$ 3 \\mathrm{m} + 2 \\mathrm{s} = ? $$\n";
    let mut h = app_with(src, 0);
    wait(&mut h);
    let all = labels(&h);
    let has = |want: &str| all.iter().any(|l| l.contains(want));
    assert!(has("R equals 35.31194307"), "answer: {all:?}");
    // 35.31194307 rounds to the written 35.3, so that check holds; 35.4 fails.
    assert!(
        has("R equals 35.3, check holds, 35.31194307 rounds to 35.3"),
        "check: {all:?}"
    );
    assert!(
        all.iter()
            .any(|l| l.starts_with("R equals 35.4, check fails") && l.contains("35.31194307")),
        "failed check: {all:?}"
    );
    assert!(has("R equals 35.31194307, check holds"), "check: {all:?}");
    assert!(
        all.iter().any(|l| l.contains("equals, error:")),
        "error: {all:?}"
    );
}

#[test]
fn r8_a_warning_the_page_marks_is_spoken() {
    // `sinn(2)` reads as a product of letters it probably did not mean: the
    // page marks it with a warning and gives no answer.
    let src = "$$ sinn(2) = ? $$\n";
    let mut h = app_with(src, 0);
    wait(&mut h);
    let pc = h
        .state()
        .doc()
        .and_then(|d| d.page_cache())
        .cloned()
        .unwrap();
    let warned = pc
        .problems
        .iter()
        .any(|p| p.level == mpp_ide::diagnostics::Level::Warning);
    let all = labels(&h);
    if warned {
        assert!(
            all.iter().any(|l| l.contains("warning:")),
            "a marked warning is spoken: {all:?}"
        );
    } else {
        // Nothing marked: nothing extra is said.
        assert!(!all.iter().any(|l| l.contains("warning:")), "{all:?}");
    }
}

// ---- R6 -------------------------------------------------------------------

#[test]
fn r6_answers_errors_and_the_mode_are_announced_but_not_typing() {
    // Every name is defined: an undefined one would be answered "x is not
    // defined" (R8), not with a value.
    let src = "Words.\n\n$$ a := 3 $$\n\n$$ x := 4 $$\n\n$$ 3 \\mathrm{m} + 2 \\mathrm{m} = ? $$\n\n$$ x $$\n";
    let mut h = app_with(src, 2);
    wait(&mut h);
    // Two live regions exist.
    assert!(h
        .query_all(by().predicate(|n| n.live() == Live::Polite && n.role() == Role::Status))
        .next()
        .is_some());
    assert!(h
        .query_all(by().predicate(|n| n.live() == Live::Assertive && n.role() == Role::Alert))
        .next()
        .is_some());
    // Into maths: the mode is announced.
    let x = src.rfind("$$ x").unwrap() + 3;
    h.state_mut().doc_mut().unwrap().editor.move_caret(x, false);
    key(&mut h, Modifiers::NONE, Key::ArrowRight);
    h.step();
    assert!(polite(&h).contains("In maths"), "mode: {:?}", polite(&h));
    // Typing an answer line: nothing is said while typing...
    let before = polite(&h);
    type_text(&mut h, " + a =");
    assert_eq!(polite(&h), before, "no chatter while typing");
    // ...and the answer when the run finishes.
    wait(&mut h);
    assert!(
        polite(&h).contains("x plus a equals"),
        "answer announced: {:?}",
        polite(&h)
    );
    assert!(live(&h, Live::Polite).contains("x plus a equals"));
    // A new error is said at once: metres plus seconds.
    let t = h.state().doc().unwrap().editor.text().to_string();
    let m = t.find("mathrm{m} = ?").unwrap() + 7;
    h.state_mut().doc_mut().unwrap().editor.select(m..m + 1);
    h.step();
    type_text(&mut h, "s");
    assert!(
        h.state()
            .doc()
            .unwrap()
            .editor
            .text()
            .contains("mathrm{s} = ?"),
        "{:?}",
        h.state().doc().unwrap().editor.text()
    );
    wait(&mut h);
    assert!(
        h.state().access.assertive().contains("error"),
        "error announced: {:?} / polite {:?}",
        h.state().access.assertive(),
        polite(&h)
    );
}

// ---- R9 -------------------------------------------------------------------

#[test]
fn r9_a_one_time_hint_is_live_status_text() {
    let mut app = IdeApp::empty();
    app.new_file();
    let mut h = harness(app);
    h.run_steps(3);
    type_text(&mut h, "x := 5");
    key(&mut h, Modifiers::NONE, Key::Enter);
    h.step();
    let p = polite(&h);
    assert!(p.contains("Hint:"), "hint announced: {p:?}");
    assert!(live(&h, Live::Polite).contains("Hint:"));
}

// ---- R10 ------------------------------------------------------------------

#[test]
fn r10_the_insert_menu_is_a_labelled_search_and_listbox() {
    let mut h = app_with("$$ x $$\n", 4);
    key(&mut h, Modifiers::COMMAND, Key::Space);
    h.run_steps(2);
    let search = h.get_by_label("Search maths to insert");
    assert_eq!(search.accesskit_node().role(), Role::TextInput);
    assert!(h
        .query_all(by().predicate(|n| n.role() == Role::ListBox))
        .next()
        .is_some());
    let options: Vec<(String, bool)> = h
        .query_all(by().predicate(|n| n.role() == Role::ListBoxOption))
        .map(|n| {
            let a = n.accesskit_node();
            (
                a.label().unwrap_or_default(),
                a.is_selected().unwrap_or(false),
            )
        })
        .collect();
    assert!(options.len() > 5, "{options:?}");
    assert!(options.iter().all(|(l, _)| !l.is_empty()), "{options:?}");
    assert_eq!(options.iter().filter(|(_, s)| *s).count(), 1, "{options:?}");
    let first = options.iter().find(|(_, s)| *s).unwrap().0.clone();
    // The search box points the screen reader at the highlighted item.
    let active = |h: &H| {
        let s = h.get_by_label("Search maths to insert");
        let id = s
            .accesskit_node()
            .data()
            .active_descendant()
            .expect("active item");
        h.query_all(by().predicate(move |n| n.locate().0 == id))
            .next()
            .and_then(|n| n.accesskit_node().label())
    };
    assert_eq!(active(&h).as_deref(), Some(first.as_str()));
    key(&mut h, Modifiers::NONE, Key::ArrowDown);
    h.step();
    let now = active(&h).expect("active");
    assert_ne!(now, first, "Down moves the active item");
    key(&mut h, Modifiers::NONE, Key::Escape);
}

// ---- R12 ------------------------------------------------------------------

fn in_editor(h: &H) -> bool {
    focused(h).is_some_and(|f| f.role == Role::MultilineTextInput)
}

#[test]
fn r12_panel_shortcuts_take_the_keyboard_and_esc_gives_it_back() {
    let mut h = app_with("Words.\n\n$$ 3 \\mathrm{m} + 2 \\mathrm{s} = ? $$\n", 0);
    wait(&mut h);
    assert!(in_editor(&h));
    key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::M);
    h.run_steps(3);
    assert!(!in_editor(&h), "Problems has the keyboard");
    assert!(focused(&h).is_some(), "something in Problems is focused");
    key(&mut h, Modifiers::NONE, Key::Escape);
    h.run_steps(2);
    assert!(in_editor(&h), "Esc gives the keyboard back to the editor");

    key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::E);
    h.run_steps(3);
    assert!(!in_editor(&h), "Explorer has the keyboard");
    key(&mut h, Modifiers::NONE, Key::Escape);
    h.run_steps(2);
    assert!(in_editor(&h), "Esc from the Explorer");

    key(&mut h, Modifiers::NONE, Key::F1);
    h.run_steps(3);
    let f = focused(&h).expect("focused");
    assert_eq!(f.label, "Search the cheat sheet");
}

#[test]
fn r12_f6_cycles_the_regions_and_comes_back() {
    let mut h = app_with("Words.\n", 0);
    key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::E);
    h.run_steps(2);
    key(&mut h, Modifiers::NONE, Key::Escape);
    h.run_steps(2);
    assert!(in_editor(&h));
    let mut visited = Vec::new();
    for _ in 0..4 {
        key(&mut h, Modifiers::NONE, Key::F6);
        h.run_steps(2);
        visited.push(polite(&h));
        if in_editor(&h) {
            break;
        }
        assert!(focused(&h).is_some(), "F6 always lands somewhere");
    }
    assert!(in_editor(&h), "F6 comes back to the editor: {visited:?}");
    assert!(
        visited.iter().any(|v| v.contains("Side panel")),
        "{visited:?}"
    );
    assert!(
        visited.iter().any(|v| v.contains("Status bar")),
        "{visited:?}"
    );
    // Shift+F6 goes the other way: to the status bar first.
    key(&mut h, Modifiers::SHIFT, Key::F6);
    h.run_steps(2);
    assert!(polite(&h).contains("Status bar"), "{:?}", polite(&h));
}

// ---- R13 ------------------------------------------------------------------

#[test]
fn r13_the_cheat_sheet_search_box_is_labelled() {
    let mut h = app_with("Words.\n", 0);
    key(&mut h, Modifiers::NONE, Key::F1);
    h.run_steps(3);
    let n = h.get_by_label("Search the cheat sheet");
    assert_eq!(n.accesskit_node().role(), Role::TextInput);
}

// ---- R14 ------------------------------------------------------------------

fn status_place(h: &H) -> String {
    h.query_all(by().predicate(|n| n.role() == Role::Label && n.live() == Live::Polite))
        .filter_map(|n| n.accesskit_node().label())
        .next()
        .unwrap_or_default()
}

#[test]
fn r14_the_path_is_live_and_current_in_the_frame_of_the_key() {
    let src = "Some words.\n\n$$ y := \\frac{1}{x + 2} $$\n";
    let start = src.find("y :=").unwrap();
    let mut h = app_with(src, start);
    let mut places = Vec::new();
    for _ in 0..14 {
        key(&mut h, Modifiers::NONE, Key::ArrowRight);
        let d = h.state().doc().unwrap();
        let want = mpp_ide::access::live::spoken_place(&mpp_ide::page::caret_path::status_text(d));
        let got = status_place(&h);
        assert_eq!(got, want, "the live path is one key behind");
        places.push(got);
    }
    assert!(places.iter().any(|p| p.contains("numerator")), "{places:?}");
    assert!(
        places.iter().any(|p| p.contains("denominator")),
        "{places:?}"
    );
}

#[test]
fn r14_where_am_i_says_the_mode_path_file_and_line() {
    let src = "Some words.\n\n$$ y := \\frac{1}{x + 2} $$\n";
    let den = src.find("x + 2").unwrap() + 1;
    let mut h = app_with(src, den);
    key(&mut h, Modifiers::COMMAND | Modifiers::ALT, Key::W);
    h.step();
    assert_eq!(
        polite(&h),
        "Maths, fraction, denominator. Untitled-1, line 3"
    );
    // The file is still open (Ctrl+W would have closed it).
    assert!(h.state().doc().is_some());
    assert!(live(&h, Live::Polite).contains("Untitled-1, line 3"));
}

#[test]
fn r14_the_path_names_the_box_the_next_character_goes_into() {
    // After `1/2 ` the caret is still inside the denominator's braces (the
    // space is in the box): the path must say so, not "Maths".
    let src = "$$ q := \\frac{1}{2 } $$\n";
    let at = src.find("2 }").unwrap() + 2;
    let h = app_with(src, at);
    let d = h.state().doc().unwrap();
    assert_eq!(
        mpp_ide::page::caret_path::status_text(d),
        "Maths \u{203a} fraction \u{203a} denominator"
    );
}

// ---- R16 ------------------------------------------------------------------

#[test]
fn r16_the_focus_ring_has_3_to_1_everywhere_in_both_modes() {
    for mode in [Mode::Light, Mode::Dark] {
        let pairs = mode.palette().line_pairs();
        let ring: Vec<_> = pairs
            .iter()
            .filter(|(n, _, _)| n.starts_with("focus ring"))
            .collect();
        assert!(ring.len() >= 6, "{mode:?}: ring pairs {}", ring.len());
        for (n, fg, bg) in ring {
            let r = contrast(*fg, *bg);
            assert!(r >= 3.0, "{mode:?}: {n} is {r:.2}, needs 3");
        }
    }
}

#[test]
fn r16_a_widget_reached_by_the_keyboard_gets_the_ring() {
    let mut h = app_with("Words.\n", 0);
    key(&mut h, Modifiers::NONE, Key::F6);
    h.run_steps(2);
    let f = h
        .ctx
        .memory(|m| m.focused())
        .expect("a widget has the keyboard");
    let ring = mode_ring(&h);
    let img = {
        let _ = f;
        h.render().expect("render")
    };
    // Some pixel of the ring colour is on screen.
    let found = img.pixels().any(|p| {
        let [r, g, b, _] = p.0;
        (i32::from(r) - i32::from(ring.0)).abs() < 8
            && (i32::from(g) - i32::from(ring.1)).abs() < 8
            && (i32::from(b) - i32::from(ring.2)).abs() < 8
    });
    assert!(found, "the focus ring is drawn");
}

fn mode_ring(h: &H) -> (u8, u8, u8) {
    let c = h.state().mode.palette().focus_ring;
    let c = theme::c32(c);
    (c.r(), c.g(), c.b())
}

// ---- R17 ------------------------------------------------------------------

#[test]
fn r17_labels_are_clean() {
    let src = "Words.\n\n$$ 3 \\mathrm{m} + 2 \\mathrm{s} = ? $$\n\n$$ x^2 $$\n";
    let mut h = app_with(src, 0);
    type_text(&mut h, "A");
    wait(&mut h);
    let all = labels(&h);
    assert!(
        all.iter()
            .all(|l| l == l.trim_end() || l.ends_with('\u{a0}')),
        "no trailing spaces: {:?}",
        all.iter()
            .filter(|l| *l != l.trim_end())
            .collect::<Vec<_>>()
    );
    assert!(
        all.iter()
            .any(|l| l.starts_with("Untitled-1, unsaved") && l.contains("error")),
        "tab label: {all:?}"
    );
    let defs = h.get_by_label("0 definitions");
    assert_eq!(defs.accesskit_node().role(), Role::Button);
    // The ∑ marker is a button that switches the line to prose.
    let markers: Vec<_> = h
        .query_all_by_label(mpp_ide::access::MARKER_LABEL)
        .map(|n| n.accesskit_node().role())
        .collect();
    assert!(!markers.is_empty(), "∑ markers are buttons");
    assert!(markers.iter().all(|r| *r == Role::Button));
    let before = h.state().doc().unwrap().editor.text().to_string();
    h.query_all_by_label(mpp_ide::access::MARKER_LABEL)
        .next()
        .unwrap()
        .click_accesskit();
    h.run_steps(3);
    let after = h.state().doc().unwrap().editor.text().to_string();
    assert_ne!(before, after, "the marker's button switched a line");
    assert!(
        !after.contains("$$ 3"),
        "the first maths line is prose: {after:?}"
    );
}

#[test]
fn r17_the_insert_button_says_insert() {
    let mut h = app_with("$$ x $$\n", 4);
    h.run_steps(2);
    // The menu bar's Insert button (its spaces only make room for the arrow).
    let b = h.get_by_label("Insert");
    assert_eq!(b.accesskit_node().role(), Role::Button);
    assert!(
        !labels(&h).iter().any(|l| l.starts_with("Insert ")),
        "no trailing spaces on Insert"
    );
}

// ---- R19 ------------------------------------------------------------------

#[test]
fn r19_one_spoken_style_for_fractions_in_the_tree() {
    let src = "$$ \\frac{1}{n^2} $$\n\n$$ a/b $$\n\n$$ \\frac{a + b}{2} $$\n";
    let h = app_with(src, 0);
    let all = labels(&h);
    for want in [
        "1 over n squared",
        "a over b",
        "the quantity a plus b end quantity over 2",
    ] {
        assert!(all.iter().any(|l| l == want), "{want:?} in {all:?}");
    }
    assert!(!all.iter().any(|l| l.contains("the fraction")), "{all:?}");
}

// ---- Fix loop 2 -----------------------------------------------------------

/// Types `s` and checks it landed in the file (the editor had the keyboard).
fn types_into_file(h: &mut H, s: &str) -> bool {
    type_text(h, s);
    h.state().doc().unwrap().editor.text().contains(s)
}

#[test]
fn rn1_esc_on_a_prose_line_keeps_the_keyboard_in_the_editor() {
    let mut h = app_with("Hello there.\n", 6);
    assert!(in_editor(&h));
    key(&mut h, Modifiers::NONE, Key::Escape);
    h.step();
    assert!(
        in_editor(&h),
        "Esc in prose: focus {:?}",
        focused(&h).map(|f| f.label)
    );
    key(&mut h, Modifiers::NONE, Key::Escape);
    h.step();
    assert!(in_editor(&h), "Esc again");
    // F10, then Esc: still the editor.
    key(&mut h, Modifiers::NONE, Key::F10);
    h.step();
    key(&mut h, Modifiers::NONE, Key::Escape);
    h.step();
    assert!(
        in_editor(&h),
        "Esc after F10: focus {:?}",
        focused(&h).map(|f| f.label)
    );
    assert!(types_into_file(&mut h, "K"));
}

#[test]
fn rn2_up_on_the_first_line_and_down_on_the_last_stay_in_the_editor() {
    let mut h = app_with("First line.\n\nLast line.", 3);
    for _ in 0..4 {
        key(&mut h, Modifiers::NONE, Key::ArrowUp);
    }
    h.step();
    assert!(
        in_editor(&h),
        "Up at the top: {:?}",
        focused(&h).map(|f| f.label)
    );
    assert_eq!(h.state().doc().unwrap().editor.caret(), 0);
    key(&mut h, Modifiers::COMMAND, Key::End);
    for _ in 0..3 {
        key(&mut h, Modifiers::NONE, Key::ArrowDown);
    }
    h.step();
    assert!(
        in_editor(&h),
        "Down at the bottom: {:?}",
        focused(&h).map(|f| f.label)
    );
    assert!(types_into_file(&mut h, "D"));
}

#[test]
fn r19_inline_maths_says_its_answer_and_the_line_reads_spoken_maths() {
    let src = "$$ v := 20 $$\n\nHalf of $v$ is $v/2 =$ metres per second.\n\nThe angle $\\theta$ is small.\n";
    let mut h = app_with(src, 0);
    wait(&mut h);
    let all = labels(&h);
    assert!(
        all.iter().any(|l| l == "v over 2 equals 10"),
        "inline answer: {all:?}"
    );
    let at = src.find("Half").unwrap() + 2;
    h.state_mut()
        .doc_mut()
        .unwrap()
        .editor
        .move_caret(at, false);
    h.run_steps(2);
    let f = focused(&h).expect("focused");
    assert_eq!(
        f.value,
        "Half of v is v over 2 equals 10 metres per second."
    );
    let at = src.find("The angle").unwrap() + 2;
    h.state_mut()
        .doc_mut()
        .unwrap()
        .editor
        .move_caret(at, false);
    h.run_steps(2);
    let f = focused(&h).expect("focused");
    assert_eq!(f.value, "The angle theta is small.");
}

#[test]
fn r12_problems_items_are_named_buttons_and_enter_jumps_back_to_the_editor() {
    let src = "Words.\n\n$$ v := 20 $$\n\n$$ w + 1 = ? $$\n";
    let mut h = app_with(src, 0);
    wait(&mut h);
    key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::M);
    h.run_steps(3);
    let items: Vec<(Role, String)> = h
        .query_all(by().predicate(|n| {
            n.label()
                .is_some_and(|l| l.starts_with("Error, line") || l.starts_with("Warning, line"))
        }))
        .map(|n| {
            let a = n.accesskit_node();
            (a.role(), a.label().unwrap_or_default())
        })
        .collect();
    assert!(
        items
            .iter()
            .any(|(r, l)| *r == Role::Button && l.starts_with("Warning, line 5: w is not defined")),
        "problem items: {items:?}"
    );
    // No unnamed focusable rows left in the panel.
    let f = focused(&h).expect("something in Problems has the keyboard");
    assert!(
        !f.label.is_empty() || !f.value.is_empty(),
        "the focused row is named: {:?}",
        f.role
    );
    // Where am I says the panel and the row.
    key(&mut h, Modifiers::COMMAND | Modifiers::ALT, Key::W);
    h.step();
    assert!(polite(&h).starts_with("Problems"), "{:?}", polite(&h));
    // Tab to the error row, then Enter: the caret goes there and the editor
    // has the keyboard again.
    for _ in 0..6 {
        if focused(&h).is_some_and(|f| f.label.starts_with("Warning, line 5")) {
            break;
        }
        key(&mut h, Modifiers::NONE, Key::Tab);
        h.step();
    }
    assert!(
        focused(&h).is_some_and(|f| f.label.starts_with("Warning, line 5")),
        "Tab reaches the error: {:?}",
        focused(&h).map(|f| f.label)
    );
    key(&mut h, Modifiers::NONE, Key::Enter);
    h.run_steps(3);
    assert!(in_editor(&h), "Enter gives the keyboard back to the editor");
    let d = h.state().doc().unwrap();
    let line = d.editor.text()[..d.editor.caret()].matches('\n').count() + 1;
    assert_eq!(line, 5, "the caret is on the problem's line");
    assert!(polite(&h).contains("line 5"), "{:?}", polite(&h));
}

#[test]
fn p7_p8_coder_hints_are_in_problems_and_said_once() {
    let mut h = app_with("$$ R := 35 $$\n\n", 15);
    wait(&mut h);
    type_text(&mut h, "print(R)");
    wait(&mut h);
    h.run_steps(3);
    // Not while the line is still being typed.
    assert!(!polite(&h).contains("Hint: To see"), "{:?}", polite(&h));
    key(&mut h, Modifiers::NONE, Key::Enter);
    wait(&mut h);
    h.run_steps(3);
    let said = polite(&h);
    assert!(
        said.contains("Hint: To see the value of R, type R = on a line of its own"),
        "the hint is said: {said:?}"
    );
    // The status bar counts it, and the Problems panel lists it.
    let all = labels(&h);
    assert!(
        all.iter().any(|l| l == "No problems, 1 hint"),
        "status: {all:?}"
    );
    key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::M);
    h.run_steps(3);
    assert!(
        labels(&h)
            .iter()
            .any(|l| l.starts_with("Hint, line 3: To see the value of R")),
        "{:?}",
        labels(&h)
    );
    // Said once: the next run says what is new, not the hint again.
    key(&mut h, Modifiers::NONE, Key::Escape);
    h.run_steps(2);
    key(&mut h, Modifiers::COMMAND, Key::Home);
    for _ in 0..4 {
        key(&mut h, Modifiers::NONE, Key::ArrowRight);
    }
    type_text(&mut h, "0");
    wait(&mut h);
    h.run_steps(3);
    assert!(!polite(&h).contains("Hint: To see"), "{:?}", polite(&h));
}

#[test]
fn rn3_rn4_answers_are_spoken_whole_once_and_not_as_code() {
    let src = "$$ g := 9.81 $$\n\n$$ v := 20 $$\n\n$$ R := 35.31194307 $$\n\n$$ w + 1 = ? $$\n\n$$ R = 40 $$\n\n$$ \\theta := \\pi/6 $$\n\n$$ q := \\frac{v^2 \\sin(2\\theta)}{g} $$\n\n$$ \\frac{1}{2} + \\frac{1}{3} = ? $$\n\n$$ q + R + g + v $$\n";
    let mut h = app_with(src, 0);
    wait(&mut h);
    let all = labels(&h);
    let find = |start: &str| {
        all.iter()
            .find(|l| l.starts_with(start))
            .cloned()
            .unwrap_or_else(|| panic!("no label starts with {start:?}: {all:?}"))
    };
    // R-N3: not cut off, and the same thing is said once.
    assert!(
        !all.iter().any(|l| l.contains('\u{2026}')),
        "cut off: {all:?}"
    );
    let w = find("w plus 1 equals");
    assert_eq!(w.matches("not defined").count(), 1, "{w:?}");
    let check = find("R equals 40");
    assert_eq!(check.matches("right side 40").count(), 1, "{check:?}");
    assert!(!check.contains(".,") && !check.contains(".."), "{check:?}");
    // R-N4: exact answers in words, with the decimal after them.
    let q = find("q is defined as");
    assert!(!q.contains('*') && !q.contains("sqrt("), "code: {q:?}");
    assert!(q.contains("square root of 3"), "{q:?}");
    assert!(q.contains("about 35.31"), "{q:?}");
    let sum = find("1 over 2 plus 1 over 3 equals");
    assert!(sum.contains("equals 5 over 6"), "{sum:?}");
}

#[test]
fn r14_the_path_is_current_while_typing_a_latex_command() {
    let mut h = app_with("", 0);
    let mut lagged = Vec::new();
    for c in "$$ s := \\frac{1}{n} $$".chars() {
        h.event(Event::Text(c.to_string()));
        h.step();
        let desc = |h: &H| {
            h.query_all(by().predicate(|n| n.is_focused()))
                .next()
                .and_then(|n| n.accesskit_node().description())
                .unwrap_or_default()
        };
        let now = (status_place(&h), desc(&h));
        h.run_steps(3);
        let settled = (status_place(&h), desc(&h));
        // A bare `\frac` with no braces yet is a command being typed, not a
        // fraction the caret is in.
        if h.state()
            .doc()
            .unwrap()
            .editor
            .text()
            .ends_with(r"\frac $$")
        {
            assert_eq!(now.0, "Maths", r"after \frac");
        }
        if now != settled {
            lagged.push((c, now, settled));
        }
    }
    assert!(lagged.is_empty(), "the path lagged: {lagged:?}");
}

#[test]
fn r17_the_marker_button_sits_in_the_page_beside_its_maths() {
    let h = app_with("Words.\n\n$$ x^2 $$\n", 0);
    let root = h.kittest_state().root();
    let top: Vec<String> = root.children().filter_map(|c| c.label()).collect();
    assert!(
        !top.iter().any(|l| l == mpp_ide::access::MARKER_LABEL),
        "the marker hangs off the window root: {top:?}"
    );
    let m = h.get_by_label(mpp_ide::access::MARKER_LABEL);
    assert_eq!(m.accesskit_node().role(), Role::Button);
}

// ---- Fix loop 3 (R-F1 … R14) ----------------------------------------------

#[test]
fn rf1_the_problems_shortcut_goes_back_to_an_open_panel_before_it_closes_it() {
    let src = "First line.\n\n$$ v := 20 $$\n\n$$ w + v = ? $$\n\n$$ v = 40 $$\n\nLast line.";
    let mut h = app_with(src, src.len());
    wait(&mut h);
    key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::M);
    h.run_steps(3);
    assert!(!in_editor(&h), "Problems has the keyboard");
    for _ in 0..6 {
        if focused(&h).is_some_and(|f| f.label.starts_with("Warning, line 5")) {
            break;
        }
        key(&mut h, Modifiers::NONE, Key::Tab);
    }
    key(&mut h, Modifiers::NONE, Key::Enter);
    h.run_steps(3);
    assert!(in_editor(&h), "the jump gives the keyboard to the editor");
    let before = h.state().doc().unwrap().editor.text().to_string();

    // The panel is still open; the shortcut takes the keyboard back to it.
    key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::M);
    h.run_steps(3);
    assert!(h.state().problems.open, "the panel stays open");
    assert!(!in_editor(&h), "Problems has the keyboard again");
    let said = polite(&h);
    assert!(said.starts_with("Problems panel, "), "{said:?}");
    assert!(said.contains("item"), "{said:?}");
    // Keys meant for the panel do not edit the file.
    key(&mut h, Modifiers::NONE, Key::Tab);
    key(&mut h, Modifiers::NONE, Key::Tab);
    key(&mut h, Modifiers::NONE, Key::Enter);
    h.run_steps(3);
    assert_eq!(h.state().doc().unwrap().editor.text(), before);

    // From inside the panel, the shortcut closes it and says so.
    if in_editor(&h) {
        // The Enter above chose a problem; go back to the panel first.
        key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::M);
        h.run_steps(3);
        assert!(!in_editor(&h));
    }
    key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::M);
    h.run_steps(3);
    assert!(!h.state().problems.open, "the panel closes from inside it");
    assert!(in_editor(&h), "the editor has the keyboard");
    assert_eq!(polite(&h), "Problems closed, editor");
}

#[test]
fn rf2_inline_answers_are_announced_after_a_run_like_display_answers() {
    let src = "$$ v := 2 $$\n\nHalf of $v$ is $v/2 =$ metres per second.\n";
    let mut h = app_with(src, 9);
    wait(&mut h);
    h.run_steps(3);
    type_text(&mut h, "0");
    wait(&mut h);
    h.run_steps(3);
    let said = polite(&h);
    assert!(said.contains("v over 2 equals 10"), "{said:?}");
}

#[test]
fn rf3_the_spoken_level_matches_the_problems_panel() {
    let src = "Words.\n\n$$ v := 20 $$\n\n$$ w + v = ? $$\n";
    let mut h = app_with(src, 0);
    wait(&mut h);
    let all = labels(&h);
    let line = all
        .iter()
        .find(|l| l.starts_with("w plus v equals"))
        .cloned()
        .unwrap_or_else(|| panic!("{all:?}"));
    assert!(line.contains("warning: w is not defined"), "{line:?}");
    assert!(!line.contains("error"), "{line:?}");
    // Announced as Problems lists it: politely, as a warning.
    let src2 = "Words.

$$ v := 20 $$

$$ v = ? $$
";
    let mut h = app_with(src2, src2.rfind("v = ?").unwrap());
    wait(&mut h);
    h.run_steps(3);
    type_text(&mut h, "w + ");
    wait(&mut h);
    h.run_steps(3);
    assert!(
        !h.state().access.assertive().contains("w is not defined"),
        "{:?}",
        h.state().access.assertive()
    );
    assert!(
        polite(&h).contains("warning: w is not defined"),
        "{:?}",
        polite(&h)
    );
    key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::M);
    h.run_steps(3);
    assert!(labels(&h)
        .iter()
        .any(|l| l.starts_with("Warning, line 5: w is not defined")));
}

/// Focusable nodes a screen reader can land on that say nothing.
fn unnamed_focusable(h: &H) -> Vec<String> {
    h.query_all(by().predicate(|n| {
        n.data().supports_action(egui::accesskit::Action::Focus)
            && n.label().is_none_or(|l| l.trim().is_empty())
            && n.value().is_none_or(|v| v.trim().is_empty())
    }))
    .map(|n| {
        let a = n.accesskit_node();
        format!("{:?} {:?}", a.role(), a.bounding_box())
    })
    .collect()
}

#[test]
fn rf4_no_focusable_node_is_unnamed() {
    let src = "Words.\n\n$$ v := 20 $$\n\n$$ w + v = ? $$\n";
    let mut h = app_with(src, 0);
    wait(&mut h);
    let none = unnamed_focusable(&h);
    assert!(none.is_empty(), "unnamed: {none:?}");
    key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::M);
    h.run_steps(3);
    let none = unnamed_focusable(&h);
    assert!(none.is_empty(), "unnamed with Problems open: {none:?}");
    // After a jump the problem card is open beside the line.
    for _ in 0..2 {
        key(&mut h, Modifiers::NONE, Key::Tab);
    }
    key(&mut h, Modifiers::NONE, Key::Enter);
    h.run_steps(3);
    let none = unnamed_focusable(&h);
    assert!(none.is_empty(), "unnamed after a jump: {none:?}");
    // Tab through the panel: every stop says something.
    key(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::M);
    h.run_steps(3);
    for i in 0..8 {
        let f = focused(&h).expect("something has the keyboard");
        assert!(
            !f.label.trim().is_empty() || !f.value.trim().is_empty(),
            "Tab {i} lands on an unnamed {:?}",
            f.role
        );
        key(&mut h, Modifiers::NONE, Key::Tab);
    }
}

#[test]
fn tn6_where_am_i_is_shown_on_screen_too() {
    let mut h = app_with("Words.\n\n$$ x^2 $$\n", 2);
    wait(&mut h);
    assert!(h.state().access.toast().is_none());
    key(&mut h, Modifiers::COMMAND | Modifiers::ALT, Key::W);
    h.step();
    let said = polite(&h);
    assert!(said.contains("Untitled-1, line 1"), "{said:?}");
    assert_eq!(h.state().access.toast(), Some(said.as_str()));
    h.run_steps(2);
    let drawn = h
        .query_all(by().predicate(|n| n.role() == Role::Label))
        .any(|n| n.accesskit_node().value().is_some_and(|v| v == said));
    assert!(drawn, "the words are drawn on screen");
}

#[test]
fn r14_a_command_being_typed_is_not_called_an_error() {
    let mut h = app_with("", 0);
    for c in r"$$ s := \frac".chars() {
        h.event(Event::Text(c.to_string()));
        h.step();
    }
    wait(&mut h);
    h.run_steps(3);
    let text = h.state().doc().unwrap().editor.text().to_string();
    assert!(text.contains(r"\frac"), "{text:?}");
    let f = focused(&h).expect("the editor has the keyboard");
    assert!(!f.value.contains("error"), "line value: {:?}", f.value);
    assert!(!f.value.contains("Expected"), "line value: {:?}", f.value);
    assert!(
        !h.state().access.assertive().contains("Expected"),
        "{:?}",
        h.state().access.assertive()
    );
    let errors = h.state_mut().doc_mut().unwrap().problems();
    assert!(
        !errors.iter().any(|p| p.message.contains("numerator")),
        "flagged: {errors:?}"
    );
    // Once the caret leaves the command, it is a problem again.
    key(&mut h, Modifiers::NONE, Key::Home);
    h.run_steps(3);
    let errors = h.state_mut().doc_mut().unwrap().problems();
    assert!(
        errors
            .iter()
            .any(|p| p.message.to_lowercase().contains("numerator")),
        "{errors:?}"
    );
}
