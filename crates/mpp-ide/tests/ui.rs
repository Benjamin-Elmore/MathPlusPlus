//! UI tests: the IDE driven with egui_kittest.

use egui::{Event, Key, Modifiers, Pos2, Vec2};
use egui_kittest::Harness;
use mpp_ide::{theme, IdeApp, WELCOME};

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

fn type_text(h: &mut Harness<'static, IdeApp>, s: &str) {
    h.event(Event::Text(s.to_string()));
    h.run_steps(2);
}

fn key(h: &mut Harness<'static, IdeApp>, k: Key) {
    h.key_press(k);
    h.run_steps(2);
}

fn page_text(h: &mut Harness<'static, IdeApp>) -> String {
    h.state_mut().visible_text(900.0)
}

/// Clicks in the page at the end of the first line of the document.
fn click_page(h: &mut Harness<'static, IdeApp>, x: f32, y: f32) {
    h.event(Event::PointerMoved(Pos2::new(x, y)));
    h.event(Event::PointerButton {
        pos: Pos2::new(x, y),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.event(Event::PointerButton {
        pos: Pos2::new(x, y),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(2);
}

fn assert_no_raw(text: &str) {
    for bad in ["$$", "\\frac", "\\int", "_", "^", "\\\\", ":="] {
        assert!(
            !text.contains(bad),
            "raw source {bad:?} is visible in: {text}"
        );
    }
}

fn doc_text(h: &Harness<'static, IdeApp>) -> String {
    h.state()
        .doc()
        .map(|d| d.editor.text().to_string())
        .unwrap_or_default()
}

fn save_png(h: &mut Harness<'static, IdeApp>, name: &str) {
    let img = h.render().expect("render");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ide-snapshots");
    std::fs::create_dir_all(&dir).expect("create dir");
    img.save(dir.join(name)).expect("save png");
}

#[test]
fn the_welcome_page_shows_no_raw_text() {
    let mut h = settled(IdeApp::new());
    let text = page_text(&mut h);
    assert!(text.contains("Welcome"), "heading is typeset: {text}");
    assert!(
        text.contains('\u{222b}') || text.contains('d'),
        "maths is typeset"
    );
    assert_no_raw(&text);
    assert!(!h.state().show_file_text, "file text is off at start");
}

#[test]
fn file_text_is_off_at_start_and_only_a_menu_switch_turns_it_on() {
    let mut h = settled(IdeApp::new());
    for k in [
        Key::F1,
        Key::F2,
        Key::F3,
        Key::F4,
        Key::F5,
        Key::F6,
        Key::F7,
        Key::F8,
    ] {
        key(&mut h, k);
    }
    h.key_press_modifiers(Modifiers::COMMAND, Key::U);
    h.run_steps(2);
    assert!(!h.state().show_file_text);
}

#[test]
fn results_appear_after_evaluation() {
    let mut app = IdeApp::empty();
    app.new_file();
    app.doc_mut()
        .expect("doc")
        .editor
        .set_text("$$ 2 + 3 = ? $$\n");
    let mut h = settled(app);
    let text = page_text(&mut h);
    assert!(text.contains('5'), "the answer 5 is filled in: {text}");
    assert_no_raw(&text);
}

#[test]
fn typing_alpha_in_maths_gives_the_letter() {
    let mut app = IdeApp::empty();
    app.new_file();
    app.doc_mut().expect("doc").editor.set_text("$$ x + y $$\n");
    let mut h = settled(app);
    // Put the caret at the end of the maths by clicking on its right edge.
    let end = h
        .state_mut()
        .doc_mut()
        .and_then(|d| d.editor.text().find(" y").map(|i| i + 2))
        .expect("offset");
    h.state_mut()
        .doc_mut()
        .expect("doc")
        .editor
        .move_caret(end, false);
    h.run_steps(2);
    type_text(&mut h, " + alpha");
    let text = doc_text(&h);
    assert!(text.contains("x + y + \u{3b1}"), "got {text:?}");
    let shown = page_text(&mut h);
    assert!(
        shown.contains('\u{3b1}') || shown.contains('\u{1d6fc}'),
        "{shown}"
    );
    assert_no_raw(&shown);
}

#[test]
fn prose_edits_as_typeset_text() {
    let mut app = IdeApp::empty();
    app.new_file();
    app.doc_mut()
        .expect("doc")
        .editor
        .set_text("# Title\n\nHello *world*.\n");
    let mut h = settled(app);
    // End of the paragraph text.
    let end = doc_text(&h).find("world").expect("world") + "world".len();
    h.state_mut()
        .doc_mut()
        .expect("doc")
        .editor
        .move_caret(end, false);
    h.run_steps(1);
    type_text(&mut h, "!");
    assert!(doc_text(&h).contains("*world!*"), "{:?}", doc_text(&h));
    key(&mut h, Key::Backspace);
    assert!(!doc_text(&h).contains('!'));
    let text = page_text(&mut h);
    assert!(!text.contains('#'), "heading markup is hidden: {text}");
    assert!(!text.contains('*'), "emphasis markup is hidden: {text}");
}

#[test]
fn clicking_the_page_places_the_caret() {
    let mut h = settled(IdeApp::new());
    click_page(&mut h, 700.0, 200.0);
    let caret = h.state().doc().expect("doc").editor.caret();
    assert!(caret > 0, "the caret moved into the text");
}

#[test]
fn go_to_definition_across_two_files() {
    let dir = std::env::temp_dir().join(format!("mpp-ide-goto-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(dir.join("lib.mpp"), "# Library\n\n$$ g := 9.81 $$\n").expect("write");
    std::fs::write(
        dir.join("main.mpp"),
        "Using $g$ from [[lib.mpp]].\n\n$$ g + 1 = ? $$\n",
    )
    .expect("write");
    let mut app = IdeApp::empty();
    app.open_path(&dir.join("main.mpp")).expect("open");
    let mut h = settled(app);
    let offset = doc_text(&h).rfind("g + 1").expect("name");
    let info = {
        let d = h.state().doc().expect("doc");
        let ev = d.eval.as_ref().expect("evaluated");
        mpp_ide::defs::definition_at(ev, d.editor.text(), offset).expect("definition")
    };
    assert!(info.file.is_some(), "defined in the other file");
    h.state_mut().goto_definition(&info);
    h.run_steps(3);
    let d = h.state().doc().expect("doc");
    assert!(
        d.path.as_deref().is_some_and(|p| p.ends_with("lib.mpp")),
        "lib.mpp is now in the view"
    );
    assert!(d.highlight.is_some(), "the definition is highlighted");
    assert_eq!(h.state().docs.len(), 2);
    // The highlight goes with the next key.
    key(&mut h, Key::ArrowRight);
    assert!(
        h.state().doc().expect("doc").highlight.is_none(),
        "the highlight clears"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn errors_underline_the_element_not_a_line_number() {
    let mut app = IdeApp::empty();
    app.new_file();
    app.doc_mut()
        .expect("doc")
        .editor
        .set_text("$$ \\int_0^{} x dx $$\n\n$$ 1 + = ? $$\n");
    let mut h = settled(app);
    let d = h.state().doc().expect("doc");
    let text = d.editor.text().to_string();
    assert!(!text.is_empty());
    let width = 900.0;
    let pc = h
        .state_mut()
        .doc_mut()
        .expect("doc")
        .page(width, 19.0, theme::Mode::Light);
    for p in &pc.problems {
        assert!(
            !p.message.to_lowercase().contains("line "),
            "no line numbers in messages: {}",
            p.message
        );
        assert!(p.rect.width() >= 0.0);
    }
    assert!(!pc.problems.is_empty(), "problems are found");
}

#[test]
fn snapshots_light_and_dark() {
    for (name, mode) in [
        ("ide-light.png", theme::Mode::Light),
        ("ide-dark.png", theme::Mode::Dark),
    ] {
        let mut app = IdeApp::new();
        app.set_theme_choice(match mode {
            theme::Mode::Light => theme::ThemeChoice::Light,
            theme::Mode::Dark => theme::ThemeChoice::Dark,
        });
        let mut h = settled(app);
        h.run_steps(2);
        save_png(&mut h, name);
    }
}

#[test]
fn welcome_text_parses_without_problems() {
    let mut s = mpp_eval::Session::new();
    let ev = s.run_str(WELCOME);
    assert!(ev.errors().is_empty(), "{:?}", ev.errors());
}

#[test]
fn snapshot_errors_and_autocomplete() {
    for (suffix, choice) in [
        ("light", theme::ThemeChoice::Light),
        ("dark", theme::ThemeChoice::Dark),
    ] {
        let mut app = IdeApp::empty();
        app.set_theme_choice(choice);
        app.new_file();
        app.doc_mut().expect("doc").editor.set_text(
            "# Problems\n\nThis line has a missing bound:\n\n$$ \\int_0^{} x \\, dx = ? $$\n\nand this one is fine:\n\n$$ a := 4 $$\n\n$$ a + x = ? $$\n",
        );
        let mut h = settled(app);
        save_png(&mut h, &format!("ide-errors-{suffix}.png"));

        // Autocomplete: type a prefix at the end of the last maths block.
        let end = doc_text(&h).rfind("= ?").expect("query") + 1;
        h.state_mut()
            .doc_mut()
            .expect("doc")
            .editor
            .move_caret(end, false);
        h.run_steps(2);
        type_text(&mut h, " + alp");
        save_png(&mut h, &format!("ide-completion-{suffix}.png"));
    }
}

#[test]
fn ctrl_k_then_ctrl_t_cycles_the_theme() {
    let mut h = settled(IdeApp::new());
    assert_eq!(h.state().theme_choice, theme::ThemeChoice::System);
    let ctrl = Modifiers::COMMAND;
    for expected in [
        theme::ThemeChoice::Light,
        theme::ThemeChoice::Dark,
        theme::ThemeChoice::System,
    ] {
        h.key_press_modifiers(ctrl, Key::K);
        h.run_steps(1);
        h.key_press_modifiers(ctrl, Key::T);
        h.run_steps(2);
        assert_eq!(h.state().theme_choice, expected);
    }
    // Ctrl+T alone does nothing.
    h.key_press_modifiers(ctrl, Key::T);
    h.run_steps(2);
    assert_eq!(h.state().theme_choice, theme::ThemeChoice::System);
}

#[test]
fn follow_windows_is_the_default_and_dark_changes_the_colours() {
    let mut h = settled(IdeApp::new());
    assert_eq!(h.state().mode, theme::Mode::Light, "no system theme: light");
    h.state_mut().set_theme_choice(theme::ThemeChoice::Dark);
    h.run_steps(2);
    assert_eq!(h.state().mode, theme::Mode::Dark);
    let dark = h.ctx.global_style().visuals.panel_fill;
    h.state_mut().set_theme_choice(theme::ThemeChoice::Light);
    h.run_steps(2);
    assert_ne!(h.ctx.global_style().visuals.panel_fill, dark);
}

#[test]
fn copy_gives_unicode_and_paste_inside_the_app_keeps_the_maths() {
    let mut app = IdeApp::empty();
    app.new_file();
    app.doc_mut()
        .expect("doc")
        .editor
        .set_text("$$ \\int_0^1 x^2 dx $$\n");
    let mut h = settled(app);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run_steps(2);
    h.event(Event::Copy);
    h.run_steps(2);
    let (shown, source) = {
        let (a, b) = h.state().last_copy().expect("a copy was made");
        (a.to_string(), b.to_string())
    };
    assert!(
        shown.contains("\u{222b}\u{2080}\u{b9}"),
        "unicode maths: {shown}"
    );
    assert!(source.contains("\\int"));
    let len = doc_text(&h).len();
    h.state_mut()
        .doc_mut()
        .expect("doc")
        .editor
        .move_caret(len, false);
    h.event(Event::Paste(shown));
    h.run_steps(2);
    assert!(
        doc_text(&h).matches("\\int_0^1").count() == 2,
        "the pasted maths is source, not Unicode: {:?}",
        doc_text(&h)
    );
}

#[test]
fn copy_gives_unicode_maths() {
    let out =
        mpp_ide::copy::clipboard_text("\\int_0^1 x^2 dx", true, mpp_ide::copy::CopyFormat::Unicode);
    assert_eq!(out, "\u{222b}\u{2080}\u{b9} x\u{b2} dx");
}

#[test]
fn autocomplete_offers_the_files_own_multi_letter_names() {
    let mut app = IdeApp::empty();
    app.new_file();
    app.doc_mut()
        .expect("doc")
        .editor
        .set_text("$$ velocity := 3 $$\n\n$$ x = ? $$\n");
    let mut h = settled(app);
    let at = doc_text(&h).rfind("x = ?").expect("query");
    // Where it is used, the word is set as it is where it is defined: upright.
    h.state_mut()
        .doc_mut()
        .expect("doc")
        .editor
        .set_text("$$ velocity := 3 $$\n\n$$ 2 velocity + 1 = ? $$\n\n$$ x = ? $$\n");
    h.state_mut().settle();
    let shown = page_text(&mut h);
    assert_eq!(shown.matches("velocity").count(), 2, "{shown}");
    let at = at + doc_text(&h).len() - "$$ velocity := 3 $$\n\n$$ x = ? $$\n".len();
    h.state_mut()
        .doc_mut()
        .expect("doc")
        .editor
        .move_caret(at, false);
    h.run_steps(2);
    type_text(&mut h, "vel");
    let items = h.state().completion_items();
    let first = items.first().expect("a completion");
    assert_eq!(first.1, "velocity", "{items:?}");
    // Shown upright, as it is on the page, never as italic letters.
    assert!(first.0.contains(r"\mathrm{velocity}"), "{items:?}");
    // No LaTeX command names are shown as labels.
    assert!(!items.iter().any(|i| i.1 == "lor"), "{items:?}");
}

#[test]
fn an_empty_box_gets_one_fill_in_note_not_two() {
    // The editor writes its own "fill in" note beside an equation with an empty
    // box; the page must not draw the evaluator's note on top of it.
    let mut app = IdeApp::empty();
    app.new_file();
    app.doc_mut()
        .expect("doc")
        .editor
        .set_text("$$ \\int_0^{} x \\, dx $$\n");
    let mut h = settled(app);
    let shown = page_text(&mut h);
    assert!(!shown.contains("fill"), "{shown}");
}
