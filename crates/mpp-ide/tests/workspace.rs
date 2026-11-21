//! UI tests for the explorer, tabs, navigation, search and outline.

use std::path::{Path, PathBuf};

use egui::{Event, Key, Modifiers, Vec2};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use mpp_ide::settings::SideView;
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
    let d = std::env::temp_dir().join(format!("mpp-wsui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("physics")).expect("mkdir");
    std::fs::create_dir_all(d.join("calculus")).expect("mkdir");
    std::fs::write(
        d.join("physics/mechanics.mpp"),
        "# Mechanics\n\nLet $m = 2$.\n\n$$ E = m v^2 $$\n\n## Motion\n\nFigure: y = x^2 for x in [0, 1]. A parabola.\n",
    )
    .expect("w");
    std::fs::write(d.join("physics/waves.mpp"), "# Waves\n\n$$ f = 3 $$\n").expect("w");
    std::fs::write(
        d.join("calculus/area.mpp"),
        "# Area\n\n$$ A = \\int_0^1 x \\, dx $$\n",
    )
    .expect("w");
    std::fs::write(d.join("main.mpp"), "# Main\n\n![[physics/mechanics]]\n").expect("w");
    std::fs::write(d.join("notes.txt"), "plain").expect("w");
    d
}

fn app_in(d: &Path) -> IdeApp {
    let mut app = IdeApp::empty();
    app.open_folder(d, None);
    app
}

fn run(h: &mut Harness<'static, IdeApp>) {
    h.run_steps(3);
}

fn press(h: &mut Harness<'static, IdeApp>, mods: Modifiers, key: Key) {
    h.key_press_modifiers(mods, key);
    run(h);
}

fn active_title(h: &Harness<'static, IdeApp>) -> String {
    h.state().doc().map(|d| d.title()).unwrap_or_default()
}

/// GPU rendering is one at a time: parallel renders crash the software adapter.
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

#[test]
fn clicking_a_folder_expands_it_and_a_file_opens_a_preview_tab() {
    let d = project("click");
    let mut h = harness(app_in(&d));
    run(&mut h);
    h.get_by_label("physics").click();
    run(&mut h);
    assert!(h.state().ws.explorer.expanded.contains(&d.join("physics")));
    h.get_by_label("mechanics.mpp").click();
    run(&mut h);
    assert_eq!(active_title(&h), "mechanics");
    let doc = h.state().doc().expect("doc");
    assert!(h.state().is_preview(doc), "a single click opens a preview");
    h.get_by_label("waves.mpp").click();
    run(&mut h);
    assert_eq!(h.state().docs.len(), 1, "the preview is replaced");
    assert_eq!(active_title(&h), "waves");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn editing_a_preview_keeps_it_and_the_next_preview_opens_beside_it() {
    let d = project("preview");
    let mut app = app_in(&d);
    app.open_file(&d.join("physics/waves.mpp"), true)
        .expect("open");
    let mut h = harness(app);
    run(&mut h);
    h.event(Event::Text("x".into()));
    run(&mut h);
    assert!(h.state().docs[0].is_dirty());
    h.state_mut()
        .open_file(&d.join("main.mpp"), true)
        .expect("open");
    run(&mut h);
    assert_eq!(h.state().docs.len(), 2, "the edited preview stays");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn quick_open_finds_a_file_by_part_of_its_name() {
    let d = project("quick");
    let mut h = harness(app_in(&d));
    run(&mut h);
    press(&mut h, Modifiers::COMMAND, Key::P);
    assert!(h.state().ws.quick.open);
    h.event(Event::Text("mech".into()));
    run(&mut h);
    h.get_by_label("go to mechanics");
    save_png(&mut h, "ide-a-quick-open.png");
    press(&mut h, Modifiers::NONE, Key::Enter);
    assert!(!h.state().ws.quick.open);
    assert_eq!(active_title(&h), "mechanics");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn alt_arrows_go_back_and_forward_between_files() {
    let d = project("nav");
    let mut app = app_in(&d);
    app.open_file(&d.join("main.mpp"), false).expect("a");
    app.open_file(&d.join("physics/waves.mpp"), false)
        .expect("b");
    let mut h = harness(app);
    run(&mut h);
    assert_eq!(active_title(&h), "waves");
    press(&mut h, Modifiers::ALT, Key::ArrowLeft);
    assert_eq!(active_title(&h), "main");
    press(&mut h, Modifiers::ALT, Key::ArrowRight);
    assert_eq!(active_title(&h), "waves");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn ctrl_b_hides_and_shows_the_side_bar() {
    let d = project("side");
    let mut h = harness(app_in(&d));
    run(&mut h);
    assert!(h.state().ws.side_visible);
    press(&mut h, Modifiers::COMMAND, Key::B);
    assert!(!h.state().ws.side_visible);
    press(&mut h, Modifiers::COMMAND, Key::B);
    assert!(h.state().ws.side_visible);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn ctrl_tab_cycles_and_a_tab_can_be_dragged_to_reorder() {
    let d = project("tabs");
    let mut app = app_in(&d);
    for f in ["main.mpp", "physics/waves.mpp", "calculus/area.mpp"] {
        app.open_file(&d.join(f), false).expect("open");
    }
    let mut h = harness(app);
    run(&mut h);
    assert_eq!(active_title(&h), "area");
    press(&mut h, Modifiers::CTRL, Key::Tab);
    assert_eq!(active_title(&h), "main");
    press(&mut h, Modifiers::CTRL.plus(Modifiers::SHIFT), Key::Tab);
    assert_eq!(active_title(&h), "area");
    h.state_mut().move_tab(0, 2);
    let titles: Vec<String> = h.state().docs.iter().map(|d| d.title()).collect();
    assert_eq!(titles, ["waves", "area", "main"]);
    assert_eq!(active_title(&h), "area", "the active tab follows the move");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn closing_a_tab_with_edits_asks_first() {
    let d = project("close");
    let mut app = app_in(&d);
    app.open_file(&d.join("main.mpp"), false).expect("open");
    let mut h = harness(app);
    run(&mut h);
    h.event(Event::Text("z".into()));
    run(&mut h);
    h.state_mut().request_close(0);
    run(&mut h);
    assert_eq!(h.state().docs.len(), 1, "still open until answered");
    h.get_by_label("Don\u{2019}t save").click();
    run(&mut h);
    assert!(h.state().docs.is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn search_finds_by_symbol_and_results_are_typeset_not_raw() {
    let d = project("search");
    let mut app = app_in(&d);
    app.ws.explorer.refresh();
    app.run_search("\u{222b}");
    let hits = app.ws.search.hits.clone();
    assert_eq!(hits.len(), 1);
    assert!(hits[0].file.ends_with("area.mpp"));
    let dl = mpp_ide::typeset::layout(&hits[0].latex, 16.0, mpp_render::Theme::LIGHT);
    let text = dl.text();
    assert!(
        text.contains('\u{222b}'),
        "shown as a typeset integral: {text}"
    );
    assert!(!text.contains('\\'), "no raw commands: {text}");
    app.show_side(SideView::Search);
    let mut h = harness(app);
    run(&mut h);
    save_png(&mut h, "ide-a-search.png");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn find_all_uses_lists_every_file_that_uses_a_name() {
    let d = project("uses");
    let mut app = app_in(&d);
    std::fs::write(d.join("physics/waves.mpp"), "$$ g = m + 1 $$\n").expect("w");
    app.ws.explorer.refresh();
    app.open_file(&d.join("physics/mechanics.mpp"), false)
        .expect("open");
    let text = app.doc().expect("doc").editor.text().to_string();
    let at = text.find("E = m").expect("pos") + 4;
    app.find_uses_at(at);
    assert_eq!(app.ws.search.uses_of.as_deref(), Some("m"));
    let files: Vec<String> = app
        .ws
        .search
        .hits
        .iter()
        .map(|h| {
            h.file
                .file_name()
                .expect("n")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert!(files.contains(&"mechanics.mpp".to_string()));
    assert!(files.contains(&"waves.mpp".to_string()));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_outline_lists_headings_definitions_and_figures_typeset() {
    let d = project("outline");
    let mut app = app_in(&d);
    app.open_file(&d.join("physics/mechanics.mpp"), false)
        .expect("open");
    app.settle();
    let items = app.outline_items();
    use mpp_ide::explorer::outline::OutlineKind::*;
    let kinds: Vec<_> = items.iter().map(|i| i.kind).collect();
    assert!(kinds.contains(&Heading(1)) && kinds.contains(&Heading(2)));
    assert!(kinds.contains(&Figure));
    assert!(kinds.contains(&Definition), "definitions listed: {kinds:?}");
    for i in &items {
        let dl = mpp_ide::typeset::layout(&i.latex, 16.0, mpp_render::Theme::LIGHT);
        assert!(!dl.text().contains('\\'), "raw text in {:?}", i.latex);
    }
    app.show_side(SideView::Outline);
    let mut h = harness(app);
    run(&mut h);
    save_png(&mut h, "ide-a-outline.png");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_tree_follows_files_changed_outside_the_app() {
    let d = project("live");
    let ctx = egui::Context::default();
    let mut app = IdeApp::empty();
    app.open_folder(&d, Some(&ctx));
    let before = app.ws.explorer.rows.len();
    std::fs::write(d.join("fresh.mpp"), "x").expect("w");
    let mut seen = false;
    for _ in 0..60 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        app.ws.explorer.poll();
        if app.ws.explorer.rows.len() > before {
            seen = true;
            break;
        }
    }
    assert!(seen, "the new file should appear");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_last_folder_and_tabs_come_back_on_start_up() {
    let d = project("session");
    let data = std::env::temp_dir().join(format!("mpp-wsui-data-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let mut app = IdeApp::empty();
    app.restore_session(&data, None);
    app.open_folder(&d, None);
    app.open_file(&d.join("main.mpp"), false).expect("a");
    app.open_file(&d.join("physics/waves.mpp"), false)
        .expect("b");
    app.persist_settings();
    let mut again = IdeApp::empty();
    let first = again.restore_session(&data, None);
    assert!(!first, "settings exist now");
    assert_eq!(again.project_root(), Some(d.as_path()));
    let titles: Vec<String> = again.docs.iter().map(|d| d.title()).collect();
    assert_eq!(titles, ["main", "waves"]);
    assert_eq!(again.doc().expect("doc").title(), "waves");
    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn a_file_dropped_on_the_page_adds_an_import_with_a_relative_path() {
    let d = project("drop");
    let mut app = app_in(&d);
    app.open_file(&d.join("calculus/area.mpp"), false)
        .expect("open");
    app.add_import(&d.join("physics/waves.mpp"));
    let text = app.doc().expect("doc").editor.text().to_string();
    assert!(text.starts_with("![[../physics/waves]]"), "{text}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn explorer_tabs_and_breadcrumbs_screenshot() {
    let d = project("shot");
    let mut app = app_in(&d);
    app.ws.explorer.expanded.insert(d.join("physics"));
    app.ws.explorer.refresh();
    app.open_file(&d.join("main.mpp"), false).expect("a");
    app.open_file(&d.join("physics/mechanics.mpp"), false)
        .expect("b");
    app.open_file(&d.join("physics/waves.mpp"), true)
        .expect("c");
    app.settle();
    let mut h = harness(app);
    run(&mut h);
    h.state_mut().settle();
    run(&mut h);
    save_png(&mut h, "ide-a-explorer-tabs.png");
    let _ = std::fs::remove_dir_all(&d);
}

/// After F12 jumps to a definition in another file, Alt+← from the keyboard comes
/// back and Alt+→ goes there again.
#[test]
fn alt_arrows_come_back_from_a_definition() {
    let d = std::env::temp_dir().join(format!("mpp-wsui-f12nav-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("mkdir");
    std::fs::write(d.join("lib.mpp"), "# Library\n\n$$ g := 9.81 $$\n").expect("w");
    std::fs::write(
        d.join("main.mpp"),
        "Using $g$ from [[lib.mpp]].\n\n$$ g + 1 = ? $$\n",
    )
    .expect("w");
    let mut app = app_in(&d);
    app.open_file(&d.join("main.mpp"), false).expect("main");
    let mut h = harness(app);
    run(&mut h);
    for i in 0..h.state().docs.len() {
        h.state_mut().docs[i].settle();
    }
    run(&mut h);
    let off = h
        .state()
        .doc()
        .and_then(|d| d.editor.text().rfind("g + 1"))
        .expect("g");
    h.state_mut()
        .doc_mut()
        .expect("doc")
        .editor
        .move_caret(off, false);
    run(&mut h);
    press(&mut h, Modifiers::NONE, Key::F12);
    assert_eq!(active_title(&h), "lib");
    press(&mut h, Modifiers::ALT, Key::ArrowLeft);
    assert_eq!(active_title(&h), "main");
    press(&mut h, Modifiers::ALT, Key::ArrowRight);
    assert_eq!(active_title(&h), "lib");
    let _ = std::fs::remove_dir_all(&d);
}
