//! The two halves of the IDE working together, driven through the real app with
//! egui_kittest: the Problems tile and error counts on tabs, dark mode over the
//! explorer and the tiles, the Insert menu in a split view, and undoing a
//! template insert from Edit, History. Screenshots go to `target/ide-snapshots/`.

use std::path::{Path, PathBuf};

use egui::{Event, Key, Modifiers, Vec2};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use mpp_ide::layout::tiles::Pane;
use mpp_ide::{theme, IdeApp};

const SIZE: Vec2 = Vec2::new(1280.0, 820.0);

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

fn settle(h: &mut Harness<'static, IdeApp>) {
    run(h);
    for i in 0..h.state().docs.len() {
        h.state_mut().docs[i].settle();
    }
    run(h);
}

fn press(h: &mut Harness<'static, IdeApp>, mods: Modifiers, key: Key) {
    h.key_press_modifiers(mods, key);
    run(h);
}

fn type_text(h: &mut Harness<'static, IdeApp>, s: &str) {
    h.event(Event::Text(s.to_string()));
    h.run_steps(2);
}

fn doc_text(h: &Harness<'static, IdeApp>) -> String {
    h.state()
        .doc()
        .map(|d| d.editor.text().to_string())
        .unwrap_or_default()
}

static RENDER: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A rendered frame: width, height and RGBA bytes.
struct Shot {
    w: u32,
    h: u32,
    px: Vec<u8>,
}

fn render(h: &mut Harness<'static, IdeApp>, name: &str) -> Shot {
    let _gpu = RENDER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let img = h.render().expect("render");
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ide-snapshots");
    std::fs::create_dir_all(&dir).expect("dir");
    img.save(dir.join(name)).expect("png");
    Shot {
        w: img.width(),
        h: img.height(),
        px: img.as_raw().clone(),
    }
}

/// A project: a main file importing a file with two errors, and a second
/// file with one.
fn project(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mpp-merged-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(
        dir.join("mechanics.mpp"),
        "# Mechanics\n\nLet $g := 9.81$.\n\n$$ (g + 1 $$\n\n$$ q \\approx ? $$\n",
    )
    .expect("w");
    std::fs::write(
        dir.join("main.mpp"),
        "# Projectile\n\n![[mechanics.mpp]]\n\nLet $v := 20$.\n\n$$ v \\sin \\theta \\approx ? $$\n\n$$ \\frac{v}{2} = ? $$\n",
    )
    .expect("w");
    std::fs::write(dir.join("notes.mpp"), "# Notes\n\n$$ (y $$\n").expect("w");
    dir
}

fn open_project(dir: &Path, choice: theme::ThemeChoice) -> Harness<'static, IdeApp> {
    let mut app = IdeApp::empty();
    app.set_theme_choice(choice);
    app.open_folder(dir, None);
    app.open_file(&dir.join("notes.mpp"), false).expect("notes");
    app.open_file(&dir.join("main.mpp"), false).expect("main");
    let mut h = harness(app);
    settle(&mut h);
    h
}

/// The mean brightness (0 to 255) of a square of the image.
fn brightness(img: &Shot, x: u32, y: u32) -> f32 {
    let (sx, sy) = (img.w as f32 / SIZE.x, img.h as f32 / SIZE.y);
    let (cx, cy) = ((x as f32 * sx) as u32, (y as f32 * sy) as u32);
    let mut sum = 0.0;
    let mut n = 0.0;
    for dy in 0..6 {
        for dx in 0..6 {
            let i = (((cy + dy).min(img.h - 1) * img.w + (cx + dx).min(img.w - 1)) * 4) as usize;
            let p = &img.px[i..i + 3];
            sum += 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32;
            n += 1.0;
        }
    }
    sum / n
}

#[test]
fn the_problems_tile_lists_the_project_and_tabs_show_error_counts() {
    let dir = project("problems");
    let mut h = open_project(&dir, theme::ThemeChoice::Light);
    assert!(!h.state_mut().has_pane(&Pane::Problems));
    press(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::M);
    assert!(
        h.state_mut().has_pane(&Pane::Problems),
        "Ctrl+Shift+M opens the tile"
    );
    assert!(h.state().problems.open);
    run(&mut h);

    // Counts per file: main has its own errors, mechanics (imported, not open)
    // has two, notes has one.
    let counts = h.state_mut().file_counts();
    let of = |name: &str| {
        counts
            .iter()
            .find(|(p, _)| p.ends_with(name))
            .map(|(_, c)| c.errors)
            .unwrap_or(0)
    };
    assert_eq!(of("mechanics.mpp"), 2);
    assert_eq!(of("notes.mpp"), 1);
    let main_errors = of("main.mpp");
    assert!(main_errors >= 1, "{counts:?}");

    // The tabs carry the counts, and the Problems tile lists every file.
    assert!(
        h.query_by_label("notes, 1 error").is_some(),
        "tab with its count"
    );
    let main_tab = if main_errors == 1 {
        "main, 1 error".to_string()
    } else {
        format!("main, {main_errors} errors")
    };
    assert!(h.query_by_label(&main_tab).is_some());
    assert!(h.query_by_label("Problems").is_some(), "the tile's tab");
    let list = h.state_mut().project_problems();
    let titles: Vec<&str> = list.iter().map(|f| f.title.as_str()).collect();
    assert!(
        titles.contains(&"mechanics") && titles.contains(&"notes"),
        "{titles:?}"
    );
    render(&mut h, "merged-problems-tile-light.png");

    // The status bar opens it too (after Ctrl+Shift+M closed it).
    press(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::M);
    assert!(!h.state_mut().has_pane(&Pane::Problems));
    let label = h
        .state()
        .doc()
        .map(|d| format!("{} in this file", d.problem_counts().summary()))
        .unwrap_or_default();
    h.get_by_label(&label).click();
    run(&mut h);
    assert!(
        h.state_mut().has_pane(&Pane::Problems),
        "status bar count opens it"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn dark_mode_covers_the_explorer_tabs_and_tiles() {
    let dir = project("dark");
    let mut h = open_project(&dir, theme::ThemeChoice::Dark);
    press(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::M);
    press(&mut h, Modifiers::COMMAND, Key::Backslash);
    run(&mut h);
    let img = render(&mut h, "merged-dark-split.png");
    // Explorer body, side tab strip, file tab strip, page, Problems tile, status bar.
    for (what, x, y) in [
        ("explorer", 150, 700),
        ("side tabs", 200, 40),
        ("file tabs", 900, 40),
        ("page", 700, 380),
        ("second view", 1150, 380),
        ("problems", 1000, 760),
        ("status bar", 640, 808),
    ] {
        let b = brightness(&img, x, y);
        assert!(b < 70.0, "{what} is light in dark mode ({b:.0})");
    }
    // And the same layout in light mode is light.
    h.state_mut().set_theme_choice(theme::ThemeChoice::Light);
    run(&mut h);
    let img = render(&mut h, "merged-light-split.png");
    for (what, x, y) in [("explorer", 150, 700), ("page", 700, 380)] {
        let b = brightness(&img, x, y);
        assert!(b > 180.0, "{what} is dark in light mode ({b:.0})");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_insert_menu_works_in_a_split_view() {
    let dir = project("insert");
    let mut h = open_project(&dir, theme::ThemeChoice::Light);
    std::fs::write(dir.join("blank.mpp"), "# Split\n\n").expect("w");
    h.state_mut()
        .open_file(&dir.join("blank.mpp"), false)
        .expect("open");
    let end = doc_text(&h).len();
    h.state_mut()
        .doc_mut()
        .expect("doc")
        .editor
        .move_caret(end, false);
    run(&mut h);
    press(&mut h, Modifiers::COMMAND, Key::Backslash);
    let id = h.state().doc().expect("doc").id;
    let views = h
        .state_mut()
        .with_tree(|t| {
            t.tiles
                .iter()
                .filter(|(_, x)| {
                    matches!(x, egui_tiles::Tile::Pane(Pane::Equation { doc, .. }) if *doc == id)
                })
                .count()
        })
        .unwrap_or(0);
    assert_eq!(views, 2, "two views of the file");
    let groups = h
        .state_mut()
        .with_tree(|t| {
            t.tiles
                .iter()
                .filter(|(_, x)| {
                    matches!(
                        x,
                        egui_tiles::Tile::Container(egui_tiles::Container::Tabs(_))
                    )
                })
                .count()
        })
        .unwrap_or(0);
    assert_eq!(
        groups, 3,
        "side bar, the files, and the new view beside them"
    );

    // The toolbar button opens the menu in the focused view; one insert only.
    h.get_by_label("Insert").click();
    run(&mut h);
    assert!(h.state().insert.menu.open);
    type_text(&mut h, "fraction");
    run(&mut h);
    render(&mut h, "merged-insert-split-light.png");
    press(&mut h, Modifiers::NONE, Key::Enter);
    assert!(!h.state().insert.menu.open);
    assert_eq!(
        doc_text(&h).matches("\\frac").count(),
        1,
        "inserted once, not once per view: {}",
        doc_text(&h)
    );
    // Typing fills the boxes; both views show the same typeset page.
    type_text(&mut h, "1");
    press(&mut h, Modifiers::NONE, Key::Tab);
    type_text(&mut h, "2");
    assert!(doc_text(&h).contains("\\frac{1}{2}"), "{}", doc_text(&h));
    let shown = h.state_mut().visible_text(500.0);
    for bad in ["\\", "$", "frac"] {
        assert!(!shown.contains(bad), "{bad:?} visible in {shown}");
    }

    // Dark mode: the menu follows the palette.
    h.state_mut().set_theme_choice(theme::ThemeChoice::Dark);
    h.state_mut().insert.menu.open_at(None);
    run(&mut h);
    type_text(&mut h, "integral");
    run(&mut h);
    render(&mut h, "merged-insert-split-dark.png");
    press(&mut h, Modifiers::NONE, Key::Escape);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn edit_history_undoes_a_template_insert_in_one_step() {
    let mut app = IdeApp::empty();
    app.set_theme_choice(theme::ThemeChoice::Light);
    app.new_file();
    app.doc_mut().expect("doc").editor.set_text("# Sums\n\n");
    app.doc_mut().expect("doc").editor.move_caret(8, false);
    let mut h = harness(app);
    settle(&mut h);
    let before = doc_text(&h);

    // Insert a sum from the menu, then type in its boxes, word by word.
    press(&mut h, Modifiers::COMMAND, Key::Space);
    type_text(&mut h, "sum");
    press(&mut h, Modifiers::NONE, Key::Enter);
    let inserted = doc_text(&h);
    assert!(inserted.contains("\\sum"), "{inserted}");
    for c in ["1", "0"] {
        type_text(&mut h, c);
    }
    let typed = doc_text(&h);
    assert_ne!(typed, inserted);

    // Edit, History lists the insert as one step, typeset.
    let entries = h.state().history_entries();
    assert_eq!(
        entries.len(),
        3,
        "as opened, the insert, the typing: {entries:?}"
    );
    assert_eq!(entries[0].1, "Now", "{entries:?}");
    for (_, _, latex) in &entries {
        let dl = mpp_ide::typeset::layout(latex, 15.0, mpp_render::Theme::LIGHT);
        assert!(!dl.text().contains('\\'), "typeset, not raw: {latex}");
    }
    h.get_by_label("Edit").click();
    run(&mut h);
    h.get_by_label("History").hover();
    run(&mut h);
    render(&mut h, "merged-history-menu-light.png");
    press(&mut h, Modifiers::NONE, Key::Escape);

    // Going back to the step before the insert removes the whole template.
    let (opened, _, _) = entries.last().cloned().expect("oldest");
    assert!(h.state_mut().restore_history_step(opened));
    run(&mut h);
    assert_eq!(doc_text(&h), before);
    // Redo walks forward over the same history: the insert, then the typing.
    press(&mut h, Modifiers::COMMAND, Key::Y);
    assert_eq!(doc_text(&h), inserted);
    press(&mut h, Modifiers::COMMAND, Key::Y);
    assert_eq!(doc_text(&h), typed);
    // And Ctrl+Z undoes the insert as one step after the typing.
    press(&mut h, Modifiers::COMMAND, Key::Z);
    press(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(doc_text(&h), before);
}
