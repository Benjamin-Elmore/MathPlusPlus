//! Start-up, Open and tabs respond straight away: every change shows in the
//! frames the app itself asks for (as the desktop event loop runs), new files
//! get distinct names, Open files focuses the page, and an opened file is
//! ready to type into.

use std::path::PathBuf;

use egui::{Event, Key, Modifiers, PointerButton, Pos2, Vec2};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use mpp_ide::{theme, IdeApp};

const SIZE: Vec2 = Vec2::new(1180.0, 760.0);

fn harness(app: IdeApp) -> Harness<'static, IdeApp> {
    Harness::builder()
        .with_size(SIZE)
        .with_max_steps(40)
        .with_context_setup(theme::install_fonts)
        .build_ui_state(|ui, app: &mut IdeApp| app.ui(ui), app)
}

fn project(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mpp-resp-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("mkdir");
    std::fs::write(d.join("main.mpp"), "# Main\n\nSome words.\n").expect("w");
    std::fs::write(d.join("waves.mpp"), "# Waves\n\n$$ f := 3 $$\n").expect("w");
    d
}

/// The frame an input causes, then only the frames the app asks for.
fn frames(h: &mut Harness<'static, IdeApp>) {
    h.step();
    idle(h);
}

/// Only the frames the app asks for (no input arrives).
fn idle(h: &mut Harness<'static, IdeApp>) {
    for _ in 0..30 {
        if !h.ctx.has_requested_repaint() {
            break;
        }
        h.step();
    }
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
    idle(h);
}

fn click_label(h: &mut Harness<'static, IdeApp>, label: &str) {
    let pos = h.get_by_label(label).rect().center();
    click_at(h, pos);
}

fn titles(h: &Harness<'static, IdeApp>) -> Vec<String> {
    h.state().docs.iter().map(|d| d.title()).collect()
}

#[test]
fn new_files_are_numbered_and_numbers_are_reused() {
    let mut app = IdeApp::new();
    app.ws.side_visible = true;
    let mut h = harness(app);
    frames(&mut h);
    assert_eq!(titles(&h), ["Untitled-1"], "the welcome page");
    for _ in 0..2 {
        h.key_press_modifiers(Modifiers::COMMAND, Key::N);
        frames(&mut h);
    }
    assert_eq!(titles(&h), ["Untitled-1", "Untitled-2", "Untitled-3"]);
    // Each shows under its own name in Open files, in the same frames.
    for t in titles(&h) {
        assert!(
            h.query_by_label(&format!("open {t}")).is_some(),
            "{t} listed"
        );
    }
    h.state_mut().close_doc(1);
    frames(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::N);
    frames(&mut h);
    assert_eq!(titles(&h), ["Untitled-1", "Untitled-3", "Untitled-2"]);
}

#[test]
fn an_opened_file_has_its_caret_at_the_end_of_the_first_line() {
    let d = project("caret");
    let mut app = IdeApp::empty();
    app.open_folder(&d, None);
    app.open_file(&d.join("main.mpp"), false).expect("open");
    let mut h = harness(app);
    frames(&mut h);
    assert_eq!(h.state().doc().expect("doc").editor.caret(), "# Main".len());
    h.event(Event::Text("s".into()));
    frames(&mut h);
    let text = h.state().doc().expect("doc").editor.text().to_string();
    assert!(
        text.starts_with("# Mains\n"),
        "typing works at once: {text:?}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn clicking_a_file_in_open_files_shows_its_tab_and_gives_it_the_keyboard() {
    let d = project("openfiles");
    let mut app = IdeApp::empty();
    app.open_folder(&d, None);
    app.open_file(&d.join("main.mpp"), false).expect("a");
    app.open_file(&d.join("waves.mpp"), false).expect("b");
    let mut h = harness(app);
    frames(&mut h);
    assert_eq!(h.state().doc().expect("doc").title(), "waves");
    click_label(&mut h, "open main");
    assert_eq!(h.state().doc().expect("doc").title(), "main");
    let main_id = h.state().doc().expect("doc").id;
    let shown = h
        .state()
        .ws
        .tiles
        .tree
        .as_ref()
        .expect("tree")
        .active_tiles()
        .into_iter()
        .any(|t| {
            matches!(
                h.state().ws.tiles.tree.as_ref().and_then(|tr| tr.tiles.get_pane(&t)),
                Some(mpp_ide::layout::Pane::Equation { doc, .. }) if *doc == main_id
            )
        });
    assert!(shown, "main's tab is the one on screen");
    // The keyboard goes to the page: a typed letter lands in main's text.
    h.event(Event::Text("!".into()));
    frames(&mut h);
    let text = h.state().doc().expect("doc").editor.text().to_string();
    assert!(text.starts_with("# Main!"), "typed into main: {text:?}");
    let _ = std::fs::remove_dir_all(&d);
}

/// Open files is drawn after the status bar: the status bar of the file just
/// picked must still appear without another input.
#[test]
fn the_status_bar_follows_a_file_picked_in_open_files_without_another_input() {
    let d = project("status");
    let mut app = IdeApp::empty();
    app.open_folder(&d, None);
    app.open_file(&d.join("waves.mpp"), false).expect("a");
    app.open_file(&d.join("main.mpp"), false).expect("b");
    let mut h = harness(app);
    frames(&mut h);
    for i in 0..h.state().docs.len() {
        h.state_mut().docs[i].settle();
    }
    // Let everything finish: nothing left to redraw for.
    for _ in 0..200 {
        frames(&mut h);
        if !h.ctx.has_requested_repaint() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(h.query_by_label("0 definitions").is_some());
    click_label(&mut h, "open waves");
    assert!(
        h.query_by_label("1 definition").is_some(),
        "the status bar shows the picked file's count"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn open_recent_from_the_menu_shows_the_file_and_closes_the_menu_at_once() {
    let d = project("recent");
    let mut app = IdeApp::empty();
    app.open_folder(&d, None);
    app.recent.touch(&d.join("waves.mpp"));
    app.open_file(&d.join("main.mpp"), false).expect("a");
    let mut h = harness(app);
    frames(&mut h);
    click_label(&mut h, "File");
    let pos = h.get_by_label("Open recent").rect().center();
    h.event(Event::PointerMoved(pos));
    frames(&mut h);
    click_label(&mut h, "waves");
    let doc = h.state().doc().expect("doc");
    assert_eq!(doc.title(), "waves");
    assert!(
        doc.page_cache().is_some(),
        "the opened file's page was drawn without another input"
    );
    assert!(
        h.query_by_label("Open recent").is_none(),
        "the menu closed in the frames that followed"
    );
    assert!(
        h.query_by_label("open waves").is_some(),
        "listed in Open files"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn opening_a_file_from_the_explorer_shows_it_without_another_input() {
    let d = project("explorer");
    let mut app = IdeApp::empty();
    app.open_folder(&d, None);
    let mut h = harness(app);
    frames(&mut h);
    click_label(&mut h, "waves.mpp");
    let doc = h.state().doc().expect("doc");
    assert_eq!(doc.title(), "waves");
    assert!(doc.page_cache().is_some(), "its page was drawn");
    assert!(h.query_by_label("open waves").is_some());
    let _ = std::fs::remove_dir_all(&d);
}
