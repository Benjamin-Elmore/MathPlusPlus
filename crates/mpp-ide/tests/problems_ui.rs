//! Live problems in the equation view, driven through the real app with
//! egui_kittest: one snapshot per kind of problem, the hover card, quick fixes
//! (`Ctrl+.`), F8 / Shift+F8, and the Problems panel (`Ctrl+Shift+M`).
//! Screenshots go to `target/ide-snapshots/`.

use egui::{Event, Key, Modifiers, Pos2, Vec2};
use egui_kittest::Harness;
use mpp_ide::diagnostics::{Kind, Level, Problem};
use mpp_ide::problems::Jump;
use mpp_ide::{theme, IdeApp};

const SIZE: Vec2 = Vec2::new(1180.0, 820.0);

fn harness(app: IdeApp) -> Harness<'static, IdeApp> {
    Harness::builder()
        .with_size(SIZE)
        .with_max_steps(30)
        .with_context_setup(theme::install_fonts)
        .build_ui_state(|ui, app: &mut IdeApp| app.ui(ui), app)
}

fn settle(h: &mut Harness<'static, IdeApp>) {
    h.run_steps(2);
    h.state_mut().settle();
    h.run_steps(3);
}

fn app_with(text: &str, choice: theme::ThemeChoice) -> IdeApp {
    let mut app = IdeApp::empty();
    app.set_theme_choice(choice);
    app.new_file();
    if let Some(d) = app.doc_mut() {
        d.editor.set_text(text);
        d.editor.move_caret(0, false);
    }
    app
}

fn settled(text: &str, choice: theme::ThemeChoice) -> Harness<'static, IdeApp> {
    let mut h = harness(app_with(text, choice));
    settle(&mut h);
    h
}

/// The problems on the page as the view drew them on the last frame.
fn problems(h: &mut Harness<'static, IdeApp>) -> Vec<Problem> {
    h.state_mut()
        .doc_mut()
        .map(|d| d.problems())
        .unwrap_or_default()
}

fn doc_text(h: &Harness<'static, IdeApp>) -> String {
    h.state()
        .doc()
        .map(|d| d.editor.text().to_string())
        .unwrap_or_default()
}

fn caret(h: &Harness<'static, IdeApp>) -> usize {
    h.state().doc().map(|d| d.editor.caret()).unwrap_or(0)
}

/// Rendering is done one test at a time (the GPU device is shared).
static RENDER: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn save_png(h: &mut Harness<'static, IdeApp>, name: &str) {
    let _one = RENDER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let img = h.render().expect("render");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ide-snapshots");
    std::fs::create_dir_all(&dir).expect("create dir");
    img.save(dir.join(name)).expect("save png");
}

fn key(h: &mut Harness<'static, IdeApp>, k: Key) {
    h.key_press(k);
    h.run_steps(2);
}

fn key_mods(h: &mut Harness<'static, IdeApp>, m: Modifiers, k: Key) {
    h.key_press_modifiers(m, k);
    h.run_steps(3);
}

fn move_caret(h: &mut Harness<'static, IdeApp>, at: usize) {
    if let Some(d) = h.state_mut().doc_mut() {
        d.editor.move_caret(at, false);
    }
    h.run_steps(2);
}

/// Moves the pointer over a problem's typeset element.
fn hover(h: &mut Harness<'static, IdeApp>, p: &Problem) {
    let origin = h.state().diag.origin;
    let c = p.rect.center() + origin.to_vec2();
    h.event(Event::PointerMoved(Pos2::new(c.x, c.y)));
    h.run_steps(3);
}

fn assert_plain(p: &Problem) {
    for bad in ["\\", "$$", "`", "line ", "column"] {
        assert!(!p.message.contains(bad), "{bad:?} in {:?}", p.message);
    }
}

fn find(ps: &[Problem], f: impl Fn(&Kind) -> bool) -> &Problem {
    ps.iter()
        .find(|p| f(&p.kind))
        .unwrap_or_else(|| panic!("problem not found in {ps:#?}"))
}

/// A folder with a main file importing a file that has two errors.
fn project(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mpp-problems-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(
        dir.join("mechanics.mpp"),
        "# Mechanics\n\nLet $g := 9.81$.\n\n$$ (g + 1 $$\n\n$$ q \\approx ? $$\n",
    )
    .expect("write");
    std::fs::write(
        dir.join("launch.mpp"),
        "# Launch\n\nLet $\\vartheta := 0.5$ and $v_0 := 20$.\n",
    )
    .expect("write");
    std::fs::write(
        dir.join("main.mpp"),
        "# Projectile\n\n![[mechanics.mpp]]\n\nThe launch angle is used below.\n\n$$ v_0 \\sin \\theta \\approx ? $$\n",
    )
    .expect("write");
    dir
}

fn open_project(
    tag: &str,
    choice: theme::ThemeChoice,
) -> (Harness<'static, IdeApp>, std::path::PathBuf) {
    let dir = project(tag);
    let mut app = IdeApp::empty();
    app.set_theme_choice(choice);
    app.open_path(&dir.join("main.mpp")).expect("open");
    let mut h = harness(app);
    settle(&mut h);
    (h, dir)
}

// ---- one snapshot per kind of problem ------------------------------------

#[test]
fn empty_box_is_underlined_in_red() {
    let src = "# Empty box\n\n$$ \\frac{1}{} = ? $$\n";
    let mut h = settled(src, theme::ThemeChoice::Light);
    let ps = problems(&mut h);
    let p = find(&ps, |k| matches!(k, Kind::EmptyBox { .. }));
    assert_eq!(p.level, Level::Error);
    assert!(p.message.contains("denominator"), "{}", p.message);
    assert_plain(p);
    save_png(&mut h, "problem-empty-box-light.png");
}

#[test]
fn undefined_symbol_suggests_a_close_name() {
    let src = "# Undefined\n\nLet $\\vartheta := 0.3$.\n\n$$ \\sin \\theta \\approx ? $$\n";
    let mut h = settled(src, theme::ThemeChoice::Light);
    let ps = problems(&mut h);
    let p = find(
        &ps,
        |k| matches!(k, Kind::Undefined { name, .. } if name == "θ"),
    );
    assert_eq!(p.level, Level::Error);
    assert!(p.message.starts_with("Undefined: θ."), "{}", p.message);
    assert!(p.message.contains("Did you mean ϑ"), "{}", p.message);
    assert_plain(p);
    save_png(&mut h, "problem-undefined-light.png");
}

#[test]
fn missing_bracket_underlines_the_open_bracket() {
    let src = "# Missing bracket\n\n$$ 2 (x + 1 $$\n";
    let mut h = settled(src, theme::ThemeChoice::Light);
    let ps = problems(&mut h);
    let p = find(&ps, |k| matches!(k, Kind::MissingBracket { .. }));
    assert_eq!(p.level, Level::Error);
    assert_eq!(&src[p.span.start..p.span.end], "(");
    assert_plain(p);
    save_png(&mut h, "problem-missing-bracket-light.png");
}

/// Adding metres to seconds is an error with both units typeset; a conversion to
/// a unit of another kind offers "Convert to" the quantity's own unit.
#[test]
fn unit_mismatch_is_an_error_and_a_bad_conversion_has_a_quick_fix() {
    let src = "# Units\n\n$$ 3 \\mathrm{m} + 2 \\mathrm{s} = ? $$\n";
    let mut h = settled(src, theme::ThemeChoice::Light);
    let ps = problems(&mut h);
    let p = find(&ps, |k| {
        matches!(k, Kind::UnitMismatch { convert: false, .. })
    });
    assert_eq!(p.level, Level::Error);
    assert_plain(p);

    let src = "# Units\n\n$$ 5 \\mathrm{km} \\operatorname{in} \\mathrm{s} = ? $$\n";
    let mut h = settled(src, theme::ThemeChoice::Light);
    let ps = problems(&mut h);
    let p = find(&ps, |k| {
        matches!(k, Kind::UnitMismatch { convert: true, .. })
    })
    .clone();
    let doc = mpp_syntax::document::parse_document(src);
    let fixes = mpp_ide::diagnostics::fixes::quick_fixes(&p, src, &doc);
    let fix = fixes.first().expect("a convert quick fix");
    let mut ed = mpp_ide::editor::Editor::new(src);
    mpp_ide::diagnostics::fixes::apply(&mut ed, fix);
    assert!(
        ed.text().contains("\\operatorname{in} \\mathrm{km}"),
        "{}",
        ed.text()
    );
    save_png(&mut h, "problem-units-light.png");
}

#[test]
fn import_errors_are_summarised_on_the_import_line() {
    let (mut h, dir) = open_project("p1", theme::ThemeChoice::Light);
    let ps = problems(&mut h);
    let p = find(&ps, |k| matches!(k, Kind::Import { missing: false, .. }));
    assert_eq!(p.level, Level::Error);
    assert_eq!(p.message, "2 errors in mechanics.mpp");
    // One summary per import line, not the evaluator's and the IDE's both.
    let on_line = ps
        .iter()
        .filter(|q| q.span == p.span && q.level == Level::Error)
        .count();
    assert_eq!(on_line, 1, "{ps:#?}");
    let text = doc_text(&h);
    assert!(text[p.span.start..p.span.end].contains("mechanics.mpp"));
    // The undefined θ suggests the file that defines ϑ… and the name itself.
    let u = find(
        &ps,
        |k| matches!(k, Kind::Undefined { name, .. } if name == "θ"),
    );
    assert!(u.message.contains("Did you mean ϑ"), "{}", u.message);
    save_png(&mut h, "problem-import-light.png");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_missing_import_is_an_error_with_a_close_file_name() {
    let dir = project("missing");
    std::fs::write(dir.join("other.mpp"), "![[mechanic.mpp]]\n\n$$ 1 = ? $$\n").expect("w");
    let mut app = IdeApp::empty();
    app.open_path(&dir.join("other.mpp")).expect("open");
    let mut h = harness(app);
    settle(&mut h);
    let ps = problems(&mut h);
    let p = find(&ps, |k| matches!(k, Kind::Import { missing: true, .. }));
    assert_eq!(p.level, Level::Error);
    assert!(
        p.message.contains("Can't find mechanic.mpp"),
        "{}",
        p.message
    );
    // The quick fix changes the file name behind the scenes.
    open_fixes_at(&mut h, p.span.start + 1);
    key(&mut h, Key::Enter);
    assert!(
        doc_text(&h).starts_with("![[mechanics.mpp]]"),
        "{}",
        doc_text(&h)
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn errors_light_and_dark() {
    let src = "# Problems\n\nA ball is thrown at angle $\\vartheta$.\n\nLet $\\vartheta := 0.3$ and $m := 2$.\n\n$$ \\frac{1}{} + \\sin \\theta \\approx ? $$\n\n$$ 2 (x + 1 $$\n\n$$ \\Gamma(x + 1) \\overset{?}{=} x \\Gamma(x) $$\n\n$$ \\int_0^1 e^{-x^2} \\, dx \\approx ? $$\n\n$$ \\operatorname{erf}(x) + \\operatorname{erfc}(x) \\overset{?}{=} 1 $$\n\n**Theorem 1.** Every integer $N > 1$ has a prime factor.\n\n**Proof.**\n1. This is Elements VII.31 — *(cited: every integer greater than 1 has a prime factor [@euclid])*.\n∎\n\n## References\n- [@euclid] Euclid, *Elements*, Book VII Prop. 31.\n";
    // The evaluator now works out every line above, so the note comes from a cited
    // proof step.
    for (suffix, choice) in [
        ("light", theme::ThemeChoice::Light),
        ("dark", theme::ThemeChoice::Dark),
    ] {
        let mut h = settled(src, choice);
        let ps = problems(&mut h);
        let levels: Vec<Level> = ps.iter().map(|p| p.level).collect();
        assert!(levels.contains(&Level::Error), "{ps:?}");
        assert!(levels.contains(&Level::Warning), "{ps:?}");
        assert!(levels.contains(&Level::Info), "{ps:?}");
        for p in &ps {
            assert_plain(p);
        }
        // The status bar count matches.
        let c = h
            .state()
            .doc()
            .map(|d| d.problem_counts())
            .unwrap_or_default();
        assert_eq!(
            c.errors,
            levels.iter().filter(|l| **l == Level::Error).count()
        );
        save_png(&mut h, &format!("problems-errors-{suffix}.png"));
    }
}

// ---- hover card, quick fixes ----------------------------------------------

#[test]
fn hovering_a_problem_shows_its_card() {
    let src = "# Hover\n\nLet $\\vartheta := 0.3$.\n\n$$ \\sin \\theta \\approx ? $$\n";
    for (suffix, choice) in [
        ("light", theme::ThemeChoice::Light),
        ("dark", theme::ThemeChoice::Dark),
    ] {
        let mut h = settled(src, choice);
        let ps = problems(&mut h);
        let p = find(&ps, |k| matches!(k, Kind::Undefined { .. })).clone();
        hover(&mut h, &p);
        save_png(&mut h, &format!("problem-hover-card-{suffix}.png"));
    }
}

/// Puts the caret at `at` and presses Ctrl+.
fn open_fixes_at(h: &mut Harness<'static, IdeApp>, at: usize) {
    move_caret(h, at);
    key_mods(h, Modifiers::COMMAND, Key::Period);
    assert!(
        h.state().diag.menu.is_some(),
        "Ctrl+. opens the quick fixes"
    );
}

#[test]
fn quick_fix_menu_snapshot_and_did_you_mean() {
    let src = "# Quick fix\n\nLet $\\vartheta := 0.3$.\n\n$$ \\sin \\theta \\approx ? $$\n";
    let mut h = settled(src, theme::ThemeChoice::Light);
    let at = src.find("\\theta").expect("θ") + 1;
    open_fixes_at(&mut h, at);
    save_png(&mut h, "problem-quick-fix-menu-light.png");
    let titles: Vec<String> = h
        .state()
        .diag
        .menu
        .as_ref()
        .map(|m| m.fixes.iter().map(|f| f.title.plain()).collect())
        .unwrap_or_default();
    assert_eq!(titles.first().map(String::as_str), Some("Use ϑ instead"));
    assert!(titles.iter().any(|t| t == "Define θ"), "{titles:?}");
    key(&mut h, Key::Enter);
    assert!(h.state().diag.menu.is_none());
    assert!(
        doc_text(&h).contains("$$ \\sin \\vartheta \\approx ? $$"),
        "{}",
        doc_text(&h)
    );
    settle(&mut h);
    let ps = problems(&mut h);
    assert!(
        !ps.iter().any(|p| matches!(p.kind, Kind::Undefined { .. })),
        "{ps:?}"
    );
}

#[test]
fn quick_fix_defines_a_name_with_an_empty_box() {
    let src = "# Define\n\n$$ 2 q \\approx ? $$\n";
    let mut h = settled(src, theme::ThemeChoice::Light);
    open_fixes_at(&mut h, src.find('q').expect("q"));
    // "Define q" is the first fix when no name is close.
    key(&mut h, Key::Enter);
    let text = doc_text(&h);
    assert!(
        text.contains("$$ q := {} $$\n\n$$ 2 q \\approx ? $$"),
        "{text}"
    );
    // The caret waits in the box; typing fills it.
    h.event(Event::Text("5".into()));
    h.run_steps(2);
    settle(&mut h);
    assert!(doc_text(&h).contains("$$ q := {5} $$") || doc_text(&h).contains("$$ q := 5 $$"));
    let ps = problems(&mut h);
    assert!(ps.is_empty(), "{ps:?}");
}

#[test]
fn quick_fix_imports_a_name_from_the_file_that_defines_it() {
    let (mut h, dir) = open_project("p2", theme::ThemeChoice::Light);
    let text = doc_text(&h);
    let at = text.find("v_0").expect("v_0");
    open_fixes_at(&mut h, at + 1);
    let fixes = h
        .state()
        .diag
        .menu
        .as_ref()
        .map(|m| m.fixes.clone())
        .unwrap_or_default();
    let i = fixes
        .iter()
        .position(|f| {
            f.title.plain() == "Import v₀ from launch.mpp"
                || f.title.plain() == "Import v_0 from launch.mpp"
        })
        .unwrap_or_else(|| {
            panic!(
                "{:?}",
                fixes.iter().map(|f| f.title.plain()).collect::<Vec<_>>()
            )
        });
    for _ in 0..i {
        key(&mut h, Key::ArrowDown);
    }
    key(&mut h, Key::Enter);
    let text = doc_text(&h);
    assert!(text.contains("Using $v_0$ from [[launch.mpp]]."), "{text}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn quick_fix_closes_the_bracket() {
    let src = "# Close\n\n$$ 2 (x + 1 $$\n";
    let mut h = settled(src, theme::ThemeChoice::Light);
    open_fixes_at(&mut h, src.find('(').expect("("));
    key(&mut h, Key::Enter);
    assert_eq!(doc_text(&h), "# Close\n\n$$ 2 (x + 1) $$\n");
    settle(&mut h);
    assert!(problems(&mut h).is_empty());
}

#[test]
fn escape_closes_the_quick_fix_menu_without_changes() {
    let src = "$$ 2 (x + 1 $$\n";
    let mut h = settled(src, theme::ThemeChoice::Light);
    open_fixes_at(&mut h, 4);
    key(&mut h, Key::Escape);
    assert!(h.state().diag.menu.is_none());
    assert_eq!(doc_text(&h), src);
}

// ---- F8 -------------------------------------------------------------------

#[test]
fn f8_and_shift_f8_walk_through_the_problems() {
    let src = "# Walk\n\n$$ a \\approx ? $$\n\n$$ 1 + 1 = ? $$\n\n$$ (b $$\n\n$$ c \\approx ? $$\n";
    let mut h = settled(src, theme::ThemeChoice::Light);
    let mut spans: Vec<(usize, usize)> = problems(&mut h)
        .iter()
        .map(|p| (p.span.start, p.span.end))
        .collect();
    spans.sort_unstable();
    spans.dedup();
    assert_eq!(spans.len(), 3, "{spans:?}");
    // The caret lands on the problem, and its card stays open.
    let on = |h: &Harness<'static, IdeApp>, i: usize| {
        let (a, b) = spans[i];
        let c = caret(h);
        assert!(a <= c && c <= b, "caret {c} not in {a}..{b}");
        assert_eq!(h.state().diag.peek.map(|p| p.start), Some(a));
    };
    move_caret(&mut h, 0);
    for i in 0..3 {
        key(&mut h, Key::F8);
        on(&h, i);
    }
    // Wraps around to the first.
    key(&mut h, Key::F8);
    on(&h, 0);
    // Shift+F8 goes back, wrapping to the last.
    key_mods(&mut h, Modifiers::SHIFT, Key::F8);
    on(&h, 2);
    key_mods(&mut h, Modifiers::SHIFT, Key::F8);
    on(&h, 1);
    save_png(&mut h, "problem-f8-peek-light.png");
}

// ---- Problems panel --------------------------------------------------------

#[test]
fn problems_panel_groups_by_file_and_jumps() {
    let (mut h, dir) = open_project("p3", theme::ThemeChoice::Light);
    // A second open file with its own problem.
    let second = dir.join("second.mpp");
    std::fs::write(&second, "# Second\n\n$$ (y $$\n").expect("w");
    h.state_mut().open_path(&second).expect("open");
    settle(&mut h);
    key_mods(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::M);
    assert!(h.state().problems.open, "Ctrl+Shift+M opens the panel");
    let list = h.state_mut().project_problems();
    let titles: Vec<&str> = list.iter().map(|f| f.title.as_str()).collect();
    assert_eq!(titles, ["main", "second", "mechanics"]);
    let mech = &list[2];
    assert_eq!(mech.counts().errors, 2);
    assert_eq!(
        h.state_mut().error_count(&dir.join("mechanics.mpp")),
        2,
        "counts by path for the tree"
    );
    h.run_steps(3);
    save_png(&mut h, "problems-panel-light.png");

    // Clicking an entry of a file that is not open opens it at the problem.
    let span = mech.items[0].span;
    h.state_mut().jump_to(&Jump::File {
        path: dir.join("mechanics.mpp"),
        span,
    });
    h.run_steps(3);
    let d = h.state().doc().expect("doc");
    assert_eq!(d.path.as_deref(), Some(dir.join("mechanics.mpp").as_path()));
    assert_eq!(d.editor.caret(), span.start);
    key_mods(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::M);
    assert!(!h.state().problems.open, "and closes it again");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn problems_panel_dark() {
    let (mut h, dir) = open_project("p4", theme::ThemeChoice::Dark);
    key_mods(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::M);
    h.run_steps(3);
    save_png(&mut h, "problems-panel-dark.png");
    let _ = std::fs::remove_dir_all(dir);
}

/// The status bar counts the file on screen and says so, so it never seems to
/// disagree with the Problems tile, which lists the whole project.
#[test]
fn the_status_bar_says_its_count_is_for_this_file() {
    use egui_kittest::kittest::Queryable;
    let mut h = settled("$$ speed + 1 = ? $$\n", theme::ThemeChoice::Light);
    settle(&mut h);
    assert!(
        h.query_by_label_contains("in this file").is_some(),
        "the status bar names what it counts"
    );
}
