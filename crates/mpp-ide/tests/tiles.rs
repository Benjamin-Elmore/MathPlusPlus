//! UI tests for the tile manager: split, dock, maximise, restore, focus
//! moves, saved layouts and the variables tile.

use std::path::{Path, PathBuf};

use egui::{Event, Key, Modifiers, Vec2};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use mpp_ide::layout::tiles::{self, Dir, Pane, Preset};
use mpp_ide::{theme, IdeApp};

const SIZE: Vec2 = Vec2::new(1280.0, 800.0);

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

fn double_click(h: &mut Harness<'static, IdeApp>, pos: egui::Pos2) {
    h.event(Event::PointerMoved(pos));
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            h.step();
        }
    }
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

fn project(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mpp-tiles-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("mkdir");
    std::fs::write(
        d.join("alpha.mpp"),
        "# Energy\n\nLet $m := 3$.\n\n$$ E = m v^2 $$\n\nFigure: y = x^2 for x in [0, 1]. A parabola.\n",
    )
    .expect("w");
    std::fs::write(d.join("beta.mpp"), "# Second\n\n$$ f = 2 $$\n").expect("w");
    d
}

fn open_two(d: &Path) -> IdeApp {
    let mut app = IdeApp::empty();
    app.open_folder(d, None);
    app.open_file(&d.join("alpha.mpp"), false).expect("a");
    app.open_file(&d.join("beta.mpp"), false).expect("b");
    app
}

fn equation_views(app: &mut IdeApp, doc: u64) -> usize {
    app.with_tree(|t| {
        t.tiles
            .iter()
            .filter(|(_, x)| {
                matches!(x, egui_tiles::Tile::Pane(Pane::Equation { doc: d, .. }) if *d == doc)
            })
            .count()
    })
    .unwrap_or(0)
}

#[test]
fn the_default_layout_has_side_panels_and_one_view_per_file() {
    let d = project("default");
    let mut h = harness(open_two(&d));
    run(&mut h);
    h.state_mut().settle();
    run(&mut h);
    let a = h.state().docs[0].id;
    let b = h.state().docs[1].id;
    assert_eq!(equation_views(h.state_mut(), a), 1);
    assert_eq!(equation_views(h.state_mut(), b), 1);
    assert!(h.state_mut().has_pane(&Pane::Explorer));
    assert!(h.state_mut().has_pane(&Pane::Outline));
    save_png(&mut h, "ide-a-layout-default.png");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn ctrl_backslash_splits_the_view_and_ctrl_shift_backslash_splits_down() {
    let d = project("split");
    let mut h = harness(open_two(&d));
    run(&mut h);
    let b = h.state().docs[1].id;
    press(&mut h, Modifiers::COMMAND, Key::Backslash);
    assert_eq!(equation_views(h.state_mut(), b), 2, "two views of one file");
    press(
        &mut h,
        Modifiers::COMMAND.plus(Modifiers::SHIFT),
        Key::Backslash,
    );
    assert_eq!(equation_views(h.state_mut(), b), 3);
    h.state_mut().settle();
    run(&mut h);
    save_png(&mut h, "ide-a-layout-split.png");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_tab_can_be_docked_beside_another_group() {
    let d = project("dock");
    let mut app = open_two(&d);
    let a = app.docs[0].id;
    let moved = app
        .with_tree(|t| {
            let a_tile =
                tiles::find_by(t, |p| matches!(p, Pane::Equation { doc, .. } if *doc == a))
                    .expect("a");
            let group = t.tiles.parent_of(a_tile).expect("group");
            // Drop the tab on the right edge of its own group: a new group beside it.
            let new_group = t.tiles.insert_tab_tile(vec![]);
            tiles::split_beside(t, group, new_group, Dir::Right);
            t.move_tile_to_container(a_tile, new_group, 0, false);
            t.tiles.parent_of(a_tile) == Some(new_group)
        })
        .expect("tree");
    assert!(moved);
    let mut h = harness(app);
    run(&mut h);
    let b = h.state().docs[1].id;
    assert_eq!(equation_views(h.state_mut(), b), 1);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn maximise_and_restore_with_the_key_and_a_double_click() {
    let d = project("max");
    let mut h = harness(open_two(&d));
    run(&mut h);
    // Focus the page, then maximise.
    h.get_by_label("beta").click();
    run(&mut h);
    press(
        &mut h,
        Modifiers::COMMAND.plus(Modifiers::SHIFT),
        Key::Enter,
    );
    assert!(h.state().is_maximised());
    save_png(&mut h, "ide-a-layout-maximised.png");
    h.get_by_label("Restore").click();
    run(&mut h);
    assert!(!h.state().is_maximised(), "restore button");
    let r = h.get_by_label("beta").rect();
    double_click(&mut h, egui::pos2(r.left() + 12.0, r.center().y));
    run(&mut h);
    assert!(h.state().is_maximised(), "double-click on a tab");
    press(
        &mut h,
        Modifiers::COMMAND.plus(Modifiers::SHIFT),
        Key::Enter,
    );
    assert!(!h.state().is_maximised());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn panels_close_and_reopen_from_the_view_menu_api() {
    let d = project("panels");
    let mut app = open_two(&d);
    assert!(app.has_pane(&Pane::Outline));
    app.toggle_pane(Pane::Outline);
    assert!(!app.has_pane(&Pane::Outline));
    app.toggle_pane(Pane::Outline);
    assert!(app.has_pane(&Pane::Outline));
    app.toggle_pane(Pane::Problems);
    assert!(app.has_pane(&Pane::Problems));
    for p in [Pane::Steps, Pane::Library, Pane::Variables] {
        app.toggle_pane(p.clone());
        assert!(app.has_pane(&p) || p == Pane::Variables);
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn ctrl_k_then_an_arrow_moves_focus_between_tiles() {
    let d = project("focus");
    let mut h = harness(open_two(&d));
    run(&mut h);
    press(&mut h, Modifiers::COMMAND, Key::Backslash);
    run(&mut h);
    let right = h.state().ws.tiles.focused;
    assert!(right.is_some());
    press(&mut h, Modifiers::COMMAND, Key::K);
    press(&mut h, Modifiers::NONE, Key::ArrowLeft);
    let left = h.state().ws.tiles.focused;
    assert_ne!(left, right, "focus moved to the left neighbour");
    press(&mut h, Modifiers::COMMAND, Key::K);
    press(&mut h, Modifiers::NONE, Key::ArrowRight);
    assert_eq!(h.state().ws.tiles.focused, right, "and back again");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_layout_is_saved_per_project_and_comes_back() {
    let d = project("persist");
    let data = std::env::temp_dir().join(format!("mpp-tiles-data-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let mut app = IdeApp::empty();
    app.restore_session(&data, None);
    app.open_folder(&d, None);
    app.open_file(&d.join("alpha.mpp"), false).expect("a");
    app.split_focused(Dir::Right);
    let a = app.docs[0].id;
    assert_eq!(equation_views(&mut app, a), 2);
    app.persist_layout();
    app.persist_settings();
    let mut again = IdeApp::empty();
    again.restore_session(&data, None);
    let a2 = again.docs[0].id;
    assert_eq!(equation_views(&mut again, a2), 2, "the split is restored");
    // A different project starts from the default.
    let other = project("persist-other");
    again.open_folder(&other, None);
    again.docs.clear();
    assert!(again.has_pane(&Pane::Explorer));
    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::remove_dir_all(&other);
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn named_layouts_save_switch_and_reset() {
    let d = project("named");
    let mut app = open_two(&d);
    app.apply_preset(Preset::Review);
    assert!(app.has_pane(&Pane::Problems));
    app.save_named_layout("Marking");
    app.apply_preset(Preset::Writing);
    assert!(!app.has_pane(&Pane::Problems));
    app.switch_layout("Marking");
    assert!(app.has_pane(&Pane::Problems), "the saved layout is back");
    app.switch_layout("Graphing");
    assert!(app.has_pane(&Pane::Graph {
        doc: app.docs[1].id,
        figure: 0
    }));
    app.reset_layout();
    assert!(app.has_pane(&Pane::Outline) && !app.has_pane(&Pane::Problems));
    app.save_named_layout("Writing");
    assert!(
        !app.ws.tiles.file.named.contains_key("Writing"),
        "built-in names are taken"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_graph_tile_pins_a_figure_and_the_variables_tile_edits_the_value() {
    let d = project("vars");
    let mut app = IdeApp::empty();
    app.open_folder(&d, None);
    app.open_file(&d.join("alpha.mpp"), false).expect("a");
    app.settle();
    let doc = app.docs[0].id;
    app.open_graph_tile(0, 0);
    assert!(app.has_pane(&Pane::Graph { doc, figure: 0 }));
    app.toggle_pane(Pane::Variables);
    if !app.has_pane(&Pane::Variables) {
        app.toggle_pane(Pane::Variables);
    }
    let mut h = harness(app);
    run(&mut h);
    h.state_mut().settle();
    run(&mut h);
    // Moving the slider changes the number in the document, and the result follows.
    let before = h.state().doc().expect("doc").editor.text().to_string();
    assert!(before.contains("m := 3"), "{before}");
    assert!(h.state_mut().set_variable("m", 5.0));
    let after = h.state().doc().expect("doc").editor.text().to_string();
    assert!(after.contains("m := 5"), "{after}");
    assert!(!h
        .state()
        .doc()
        .expect("doc")
        .editor
        .text()
        .contains("m := 3"));
    h.state_mut().settle();
    run(&mut h);
    assert!(
        h.state_mut().set_variable("m", 7.25),
        "evaluation is fresh again"
    );
    assert!(h
        .state()
        .doc()
        .expect("doc")
        .editor
        .text()
        .contains("m := 7.25"));
    save_png(&mut h, "ide-a-layout-variables.png");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn closing_one_of_two_views_keeps_the_file_open() {
    let d = project("close");
    let mut h = harness(open_two(&d));
    run(&mut h);
    press(&mut h, Modifiers::COMMAND, Key::Backslash);
    let b = h.state().docs[1].id;
    assert_eq!(equation_views(h.state_mut(), b), 2);
    h.state_mut().with_tree(|t| {
        let id = tiles::find_by(
            t,
            |p| matches!(p, Pane::Equation { doc, view } if *doc == b && *view > 0),
        )
        .expect("second view");
        tiles::close_pane(t, id);
    });
    run(&mut h);
    assert_eq!(h.state().docs.len(), 2, "the file stays");
    assert_eq!(equation_views(h.state_mut(), b), 1);
    let _ = std::fs::remove_dir_all(&d);
}
