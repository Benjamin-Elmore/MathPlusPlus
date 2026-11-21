//! UI tests for the IDE tools: undo and history, autosave and recovery, zoom,
//! the tutorial and did-you-mean.

use std::path::{Path, PathBuf};
use std::time::Duration;

use egui::{Event, Key, Modifiers, Vec2};
use egui_kittest::Harness;
use mpp_ide::settings::AutosaveMode;
use mpp_ide::{theme, IdeApp};

const SIZE: Vec2 = Vec2::new(1180.0, 760.0);

fn harness(app: IdeApp) -> Harness<'static, IdeApp> {
    Harness::builder()
        .with_size(SIZE)
        .with_max_steps(40)
        .with_context_setup(theme::install_fonts)
        .build_ui_state(|ui, app: &mut IdeApp| app.ui(ui), app)
}

fn run(h: &mut Harness<'static, IdeApp>) {
    h.run_steps(3);
}

fn press(h: &mut Harness<'static, IdeApp>, mods: Modifiers, key: Key) {
    h.key_press_modifiers(mods, key);
    run(h);
}

static RENDER: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn save_png(h: &mut Harness<'static, IdeApp>, name: &str) {
    let _gpu = RENDER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let img = h.render().expect("render");
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ide-snapshots");
    std::fs::create_dir_all(&dir).expect("dir");
    img.save(dir.join(name)).expect("png");
}

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mpp-tools-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("mkdir");
    d
}

fn text(h: &Harness<'static, IdeApp>) -> String {
    h.state()
        .doc()
        .map(|d| d.editor.text().to_string())
        .unwrap_or_default()
}

#[test]
fn undo_groups_typing_restores_the_caret_and_redo_comes_back() {
    let mut app = IdeApp::empty();
    app.new_file();
    app.doc_mut().expect("doc").editor.set_text("Hello ");
    app.doc_mut().expect("doc").editor.move_caret(6, false);
    let mut h = harness(app);
    run(&mut h);
    h.event(Event::Text("abc".into()));
    run(&mut h);
    h.event(Event::Text("def".into()));
    run(&mut h);
    assert_eq!(text(&h), "Hello abcdef");
    press(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(
        text(&h),
        "Hello ",
        "one step undoes the whole run of typing"
    );
    assert_eq!(
        h.state().doc().expect("doc").editor.caret(),
        6,
        "caret restored"
    );
    press(&mut h, Modifiers::COMMAND, Key::Y);
    assert_eq!(text(&h), "Hello abcdef");
}

#[test]
fn a_template_insert_is_one_undo_step() {
    let mut app = IdeApp::empty();
    app.new_file();
    app.doc_mut().expect("doc").editor.set_text("$$ x $$");
    let at = mpp_syntax::Span::new(3, 4);
    app.doc_mut()
        .expect("doc")
        .editor
        .replace_node(at, "\\frac{a}{b}");
    assert_eq!(app.doc().expect("doc").editor.text(), "$$ \\frac{a}{b} $$");
    assert!(app.undo());
    assert_eq!(app.doc().expect("doc").editor.text(), "$$ x $$");
    assert!(app.redo());
    assert_eq!(app.doc().expect("doc").editor.text(), "$$ \\frac{a}{b} $$");
}

#[test]
fn the_history_lists_steps_and_any_can_be_restored() {
    let mut app = IdeApp::empty();
    app.new_file();
    app.doc_mut()
        .expect("doc")
        .editor
        .set_text("# Title\n\n$$ a = 1 $$\n");
    let mut h = harness(app);
    run(&mut h);
    h.state_mut()
        .doc_mut()
        .expect("doc")
        .editor
        .replace_node(mpp_syntax::Span::new(16, 17), "2");
    run(&mut h);
    std::thread::sleep(Duration::from_millis(1300));
    h.state_mut()
        .doc_mut()
        .expect("doc")
        .editor
        .replace_node(mpp_syntax::Span::new(16, 17), "3");
    run(&mut h);
    let entries = h.state().history_entries();
    assert!(entries.len() >= 3, "{entries:?}");
    for (_, _, latex) in &entries {
        let dl = mpp_ide::typeset::layout(latex, 15.0, mpp_render::Theme::LIGHT);
        assert!(!dl.text().contains('\\'), "typeset, not raw: {latex}");
    }
    let (first, _, _) = entries.last().expect("oldest").clone();
    assert!(h.state_mut().restore_history_step(first));
    assert!(text(&h).contains("a = 1"));
}

#[test]
fn autosave_modes_off_delay_and_focus() {
    let d = temp("autosave");
    let f = d.join("a.mpp");
    std::fs::write(&f, "one").expect("w");
    let mut app = IdeApp::empty();
    app.open_file(&f, false).expect("open");
    let edit = |app: &mut IdeApp| {
        let doc = app.doc_mut().expect("doc");
        doc.editor.move_caret(3, false);
        doc.editor.insert_text("!");
        doc.touched();
    };
    app.set_autosave_mode(AutosaveMode::Off);
    edit(&mut app);
    std::thread::sleep(Duration::from_millis(1100));
    app.run_autosave(true);
    assert_eq!(
        std::fs::read_to_string(&f).expect("r"),
        "one",
        "off never saves"
    );
    app.set_autosave_mode(AutosaveMode::Focus);
    app.run_autosave(false);
    assert_eq!(
        std::fs::read_to_string(&f).expect("r"),
        "one",
        "no focus loss yet"
    );
    app.run_autosave(true);
    assert_eq!(
        std::fs::read_to_string(&f).expect("r"),
        "one!",
        "saved on focus loss"
    );
    app.set_autosave_mode(AutosaveMode::Delay);
    edit(&mut app);
    app.run_autosave(false);
    assert_eq!(
        std::fs::read_to_string(&f).expect("r"),
        "one!",
        "not yet: still typing"
    );
    std::thread::sleep(Duration::from_millis(1100));
    app.run_autosave(false);
    assert_eq!(
        std::fs::read_to_string(&f).expect("r"),
        "one!!",
        "saved after a pause"
    );
    assert!(!app.doc().expect("doc").is_dirty());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_crash_leaves_a_copy_that_start_up_offers_to_restore() {
    let d = temp("recover");
    let data = temp("recover-data");
    let f = d.join("m.mpp");
    std::fs::write(&f, "# Mechanics\n\n$$ F = m a $$\n").expect("w");
    let mut app = IdeApp::empty();
    app.start_up(&data, None);
    app.open_file(&f, false).expect("open");
    app.set_autosave_mode(AutosaveMode::Off);
    {
        let doc = app.doc_mut().expect("doc");
        let end = doc.editor.text().len();
        doc.editor.move_caret(end, false);
        doc.editor.insert_text("\n$$ E = m c^2 $$\n");
        doc.touched();
    }
    app.write_recovery();
    // The process "crashes": nothing is saved, nothing is cleaned up.
    drop(app);
    assert_eq!(
        std::fs::read_to_string(&f).expect("r"),
        "# Mechanics\n\n$$ F = m a $$\n",
        "the file on disk is untouched"
    );
    let mut again = IdeApp::empty();
    again.start_up(&data, None);
    let found = again
        .ws
        .autosave
        .dialog
        .as_ref()
        .map(|d| d.files.len())
        .unwrap_or(0);
    assert_eq!(found, 1, "one file to restore");
    let mut h = harness(again);
    run(&mut h);
    save_png(&mut h, "ide-a-recovery.png");
    h.state_mut().restore_recovered();
    assert!(text(&h).contains("E = m c^2"), "the unsaved text is back");
    assert!(
        h.state().doc().expect("doc").is_dirty(),
        "and still unsaved"
    );
    // Discarding removes the copies.
    let mut third = IdeApp::empty();
    third.start_up(&data, None);
    if third.ws.autosave.dialog.is_some() {
        third.discard_recovered();
    }
    let mut fourth = IdeApp::empty();
    fourth.start_up(&data, None);
    assert!(
        fourth.ws.autosave.dialog.is_none(),
        "nothing left to restore"
    );
    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn zoom_keys_scroll_and_reset_persist_between_runs() {
    let data = temp("zoom");
    let mut app = IdeApp::empty();
    app.start_up(&data, None);
    app.new_file();
    let base = app.size;
    let mut h = harness(app);
    run(&mut h);
    press(&mut h, Modifiers::COMMAND, Key::Equals);
    press(&mut h, Modifiers::COMMAND, Key::Equals);
    assert!(h.state().size > base, "Ctrl+= grows the text");
    let after_keys = h.state().size;
    press(&mut h, Modifiers::COMMAND, Key::Minus);
    assert!(h.state().size < after_keys);
    h.event(Event::Zoom(1.25));
    run(&mut h);
    let zoomed = h.state().size;
    assert!(zoomed > after_keys - 1.5, "Ctrl+scroll zooms: {zoomed}");
    h.state_mut().persist_settings();
    let mut again = IdeApp::empty();
    again.start_up(&data, None);
    assert!((again.size - zoomed).abs() < 0.01, "zoom is remembered");
    let mut h2 = harness(again);
    run(&mut h2);
    press(&mut h2, Modifiers::COMMAND | Modifiers::ALT, Key::Num0);
    assert!((h2.state().size - base).abs() < 0.01, "Ctrl+Alt+0 resets");
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn the_tutorial_opens_on_first_launch_and_from_help() {
    let data = temp("tutorial");
    let mut app = IdeApp::empty();
    app.start_up(&data, None);
    assert_eq!(app.doc().expect("lesson").title(), "01 Typing maths");
    let root = app.project_root().map(Path::to_path_buf).expect("project");
    assert!(root.ends_with("tutorial"), "{root:?}");
    app.persist_settings();
    // The second launch is an ordinary one: it does not reopen the lesson list.
    let mut second = IdeApp::empty();
    second.start_up(&data, None);
    second.docs.clear();
    second.persist_settings();
    assert!(!mpp_ide::settings::Settings::is_first_launch(&data));
    // Help -> Tutorial brings it back at any time.
    second.open_tutorial(None);
    assert_eq!(second.doc().expect("lesson").title(), "01 Typing maths");
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn every_lesson_reads_cleanly_and_has_a_try_it_box() {
    let data = temp("lessons");
    let dir = mpp_ide::tutorial::tutorial_dir(Some(&data));
    mpp_ide::tutorial::install(&dir).expect("install");
    let mut app = IdeApp::empty();
    for (name, text) in mpp_ide::tutorial::lessons::FILES {
        app.open_file(&dir.join(name), false).expect("open");
        app.settle();
        let shown = app.visible_text(900.0);
        let errors = app.doc().map(|d| d.error_count()).unwrap_or(0);
        assert_eq!(errors, 0, "{name} has problems");
        assert!(!shown.is_empty(), "{name} shows something");
        if name != "constants.mpp" {
            assert!(text.contains("Try it"), "{name}");
            assert!(
                shown.contains("Tryit"),
                "{name} shows its try-it box: {shown}"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn a_missing_import_suggests_the_file_you_meant() {
    let d = temp("didyou");
    std::fs::write(d.join("mechanics.mpp"), "Let $m := 1$.\n").expect("w");
    std::fs::write(d.join("main.mpp"), "![[mechanis]]\n").expect("w");
    let mut app = IdeApp::empty();
    app.open_folder(&d, None);
    app.open_file(&d.join("main.mpp"), false).expect("open");
    app.open_link_at(5);
    let m = app.message().unwrap_or_default().to_string();
    assert!(m.contains("Did you mean mechanics.mpp"), "{m}");
    let _ = std::fs::remove_dir_all(&d);
}
