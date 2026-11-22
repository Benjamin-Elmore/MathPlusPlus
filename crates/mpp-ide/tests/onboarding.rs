//! U24: the tutorial and first run, and the one-time hints. Driven through the
//! real app (`ui()`, which runs `pump()`), with key events and clicks.

use egui::{Event, Key, Modifiers, Vec2};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use mpp_ide::hints::Hint;
use mpp_ide::tutorial::lessons;
use mpp_ide::{theme, IdeApp};

const SIZE: Vec2 = Vec2::new(1180.0, 820.0);

fn harness(app: IdeApp) -> Harness<'static, IdeApp> {
    Harness::builder()
        .with_size(SIZE)
        .with_max_steps(40)
        .with_context_setup(theme::install_fonts)
        .build_ui_state(|ui, app: &mut IdeApp| app.ui(ui), app)
}

fn fresh() -> Harness<'static, IdeApp> {
    let mut app = IdeApp::empty();
    app.new_file();
    let mut h = harness(app);
    h.run_steps(2);
    h
}

fn type_text(h: &mut Harness<'static, IdeApp>, s: &str) {
    for c in s.chars() {
        h.event(Event::Text(c.to_string()));
        h.step();
    }
}

fn press(h: &mut Harness<'static, IdeApp>, k: Key) {
    h.key_press_modifiers(Modifiers::NONE, k);
    h.step();
}

fn hint(h: &Harness<'static, IdeApp>) -> Option<String> {
    h.state()
        .doc()
        .and_then(|d| d.editor.hint().map(str::to_string))
}

fn seen(h: &Harness<'static, IdeApp>, which: Hint) -> bool {
    h.state().ws.settings.hints.is_seen(which)
}

fn scratch(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("mpp-onb-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("mkdir");
    d
}

#[test]
fn the_first_run_opens_page_one() {
    let d = scratch("first");
    let mut app = IdeApp::empty();
    app.start_up(&d, None);
    let doc = app.doc().expect("a file is open");
    let name = doc
        .path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string());
    assert_eq!(name.as_deref(), Some(lessons::FILES[0].0));
    assert!(lessons::FILES[0].0.starts_with("1 "));
    assert!(doc.editor.text().contains("Prose and maths"));
    // The second run does not open it again.
    let mut again = IdeApp::empty();
    again.start_up(&d, None);
    assert!(again.docs.iter().all(|x| x.path.is_none()
        || !x
            .path
            .as_ref()
            .is_some_and(|p| p.ends_with(lessons::FILES[0].0))
        || again.ws.settings.tutorial_seen));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn every_page_opens_without_errors() {
    let d = scratch("pages");
    let mut app = IdeApp::empty();
    app.start_up(&d, None);
    let dir = mpp_ide::tutorial::tutorial_dir(Some(&d));
    for (name, _) in lessons::FILES {
        app.open_file(&dir.join(name), false).expect("open");
        let doc = app.doc_mut().expect("doc");
        let errors: Vec<String> = doc
            .problems()
            .into_iter()
            .filter(|p| p.level == mpp_ide::diagnostics::Level::Error)
            .map(|p| p.message)
            .collect();
        assert!(errors.is_empty(), "{name}: {errors:?}");
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_tutorial_panel_lists_the_five_pages() {
    let d = scratch("panel");
    let mut app = IdeApp::empty();
    app.start_up(&d, None);
    app.show_side(mpp_ide::settings::SideView::Tutorial);
    let mut h = harness(app);
    h.run_steps(3);
    for (name, _) in lessons::FILES.iter().take(5) {
        let title = name.trim_end_matches(".mpp");
        assert!(h.query_all_by_label(title).next().is_some(), "{title}");
    }
    h.query_all_by_label("2 Typing maths")
        .next()
        .expect("page 2")
        .click();
    h.run_steps(3);
    let doc = h.state().doc().expect("doc");
    assert!(doc.editor.text().starts_with("# 2. Typing maths"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_heading_hint_shows_once_and_resets_from_help() {
    let mut h = fresh();
    type_text(&mut h, "#T");
    assert!(hint(&h).is_some_and(|s| s.contains("Heading 1")));
    assert!(seen(&h, Hint::Heading));
    // The same thing again stays quiet.
    press(&mut h, Key::Enter);
    type_text(&mut h, "#U");
    assert!(hint(&h).is_none(), "a hint shows once");
    // Help -> Hints -> Reset all hints.
    h.get_by_label("Help").click();
    h.run_steps(2);
    h.get_by_label("Hints").click();
    h.run_steps(2);
    h.get_by_label_contains("Heading from #");
    h.get_by_label("Reset all hints").click();
    h.run_steps(3);
    assert!(!seen(&h, Hint::Heading));
    press(&mut h, Key::Enter);
    type_text(&mut h, "#V");
    assert!(hint(&h).is_some_and(|s| s.contains("Heading 1")));
}

#[test]
fn a_number_is_kept_a_number_with_a_hint() {
    let mut h = fresh();
    type_text(&mut h, "1.3");
    assert!(
        hint(&h).is_some_and(|s| s.contains("number")),
        "{:?}",
        hint(&h)
    );
    assert!(seen(&h, Hint::NumberNotList));
}

#[test]
fn starting_maths_and_auto_detect_each_hint_once() {
    let mut h = fresh();
    type_text(&mut h, "=");
    assert!(hint(&h).is_some_and(|s| s.contains("Maths line")));
    assert!(seen(&h, Hint::StartMaths));
    // A second maths line from `=`: no hint.
    let mut h = fresh();
    type_text(&mut h, "x := 5");
    press(&mut h, Key::Enter);
    h.run_steps(2);
    assert!(
        seen(&h, Hint::AutoMaths),
        "{:?}",
        h.state().doc().map(|d| d.editor.text().to_string())
    );
}

#[test]
fn a_conversion_hints_once() {
    let mut h = fresh();
    type_text(&mut h, "$");
    type_text(&mut h, "x**2 ");
    assert!(seen(&h, Hint::Conversion));
    assert!(hint(&h).is_none() || hint(&h).is_some_and(|s| s.contains("Ctrl+Z")));
}

#[test]
fn shown_hints_survive_a_restart() {
    let d = scratch("persist");
    let mut app = IdeApp::empty();
    app.start_up(&d, None);
    app.new_file();
    let mut h = harness(app);
    h.run_steps(2);
    type_text(&mut h, "#T");
    h.run_steps(2);
    let json = h.state().ws.settings.to_json();
    assert!(json.contains("heading"), "{json}");
    let back = mpp_ide::settings::Settings::from_json(&json);
    assert!(back.hints.is_seen(Hint::Heading));
    assert!(!back.hints.is_seen(Hint::Conversion));
    let _ = std::fs::remove_dir_all(&d);
}
