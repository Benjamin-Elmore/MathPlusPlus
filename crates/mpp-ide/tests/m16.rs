//! Final checks: a screenshot of the IDE for every example and for each kind of
//! problem, and the live-edit timing on a 300-line worksheet.
//! Screenshots go to `target/m16-screens/`.

use std::path::{Path, PathBuf};
use std::time::Instant;

use egui::{Event, Vec2};
use egui_kittest::Harness;
use mpp_ide::{theme, IdeApp};

const SIZE: Vec2 = Vec2::new(1180.0, 1500.0);

static RENDER: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn harness(app: IdeApp) -> Harness<'static, IdeApp> {
    Harness::builder()
        .with_size(SIZE)
        .with_max_steps(40)
        .with_context_setup(theme::install_fonts)
        .build_ui_state(|ui, app: &mut IdeApp| app.ui(ui), app)
}

fn settle(h: &mut Harness<'static, IdeApp>) {
    h.run_steps(3);
    h.state_mut().settle();
    h.run_steps(4);
}

fn open(path: &Path) -> Harness<'static, IdeApp> {
    let mut app = IdeApp::empty();
    app.set_theme_choice(theme::ThemeChoice::Light);
    app.open_file(path, false).expect("open");
    let mut h = harness(app);
    settle(&mut h);
    h
}

fn shot(h: &mut Harness<'static, IdeApp>, name: &str) {
    let _one = RENDER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let img = h.render().expect("render");
    let dir = root().join("target/m16-screens");
    std::fs::create_dir_all(&dir).expect("dir");
    img.save(dir.join(name)).expect("png");
}

fn example_files() -> Vec<PathBuf> {
    let mut out = vec![];
    for e in std::fs::read_dir(root().join("examples")).expect("dir") {
        let p = e.expect("entry").path();
        if p.extension().is_some_and(|x| x == "mpp") {
            out.push(p);
        } else if p.join("main.mpp").exists() {
            out.push(p.join("main.mpp"));
        }
    }
    out.sort();
    out
}

#[test]
fn every_example_has_a_screenshot_and_no_raw_source() {
    for path in example_files() {
        let name = if path.file_name().is_some_and(|n| n == "main.mpp") {
            path.parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else {
            path.file_stem().unwrap().to_string_lossy().into_owned()
        };
        let mut h = open(&path);
        let text = h.state_mut().visible_text(900.0);
        assert!(!text.is_empty(), "{name}: nothing on the page");
        for bad in ["$$", "@frac", "@int", "@sqrt"].map(tex) {
            assert!(
                !text.contains(&bad),
                "{name}: raw source {bad:?} on the page"
            );
        }
        shot(&mut h, &format!("{name}.png"));
    }
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mpp-m16-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    dir
}

/// Each kind of problem in place: (file name, the document).
const KINDS: &[(&str, &str)] = &[
    (
        "empty-box",
        "# Empty box\n\nA fraction with nothing under it:\n\n$$ @frac{1}{} + 2 = ? $$\n",
    ),
    (
        "undefined-name",
        "# Undefined name

Let $@vartheta := 0.3$.

$$ @sin @theta @approx ? $$
",
    ),
    (
        "missing-bracket",
        "# Missing bracket\n\nLet $m := 2$.\n\n$$ 2 (m + 1 = ? $$\n",
    ),
    (
        "unit-mismatch",
        "# Unit mismatch\n\nLet $d := 5@,@mathrm{m}$ and $t := 2@,@mathrm{s}$.\n\n$$ d + t = ? $$\n",
    ),
    (
        "numeric-fallback",
        "# Numeric fallback\n\nNo closed form exists, so a decimal is given:\n\n$$ @int_0^1 e^{-x^2} @, dx @approx ? $$\n\n$$ @int_0^1 @sin(x^2) @, dx = ? $$\n",
    ),
    (
        "assumption-warning",
        "# Assumption\n\nNothing is known about $x$:\n\n$$ @sqrt{x^2} = ? $$\n\n$$ @int x^n @, dx = ? $$\n",
    ),
];

#[test]
fn each_problem_kind_is_shown_in_place() {
    for (name, src) in KINDS {
        let dir = scratch(name);
        let file = dir.join("doc.mpp");
        std::fs::write(&file, tex(src)).expect("write");
        let mut h = open(&file);
        let ps = h
            .state_mut()
            .doc_mut()
            .map(|d| d.problems())
            .unwrap_or_default();
        assert!(!ps.is_empty(), "{name}: no problem shown");
        println!(
            "{name}: {:?}",
            ps.iter().map(|p| (&p.message, p.level)).collect::<Vec<_>>()
        );
        shot(&mut h, &format!("error-{name}.png"));
    }
    // An import error: a file that does not exist, and one with errors of its own.
    let dir = scratch("import");
    std::fs::write(
        dir.join("bad.mpp"),
        tex("# Bad\n\n$$ (1 + 2 $$\n\n$$ @frac{1}{} = ? $$\n"),
    )
    .expect("w");
    std::fs::write(
        dir.join("main.mpp"),
        "# Import errors\n\n![[bad.mpp]]\n\n![[missing-file.mpp]]\n\n$$ 1 + 1 = ? $$\n",
    )
    .expect("w");
    let mut h = open(&dir.join("main.mpp"));
    let ps = h
        .state_mut()
        .doc_mut()
        .map(|d| d.problems())
        .unwrap_or_default();
    let missing = ps
        .iter()
        .find(|p| p.message.starts_with("Can"))
        .expect("missing-file problem");
    // The note is drawn after the whole import line, never over its words.
    assert!(missing.rect.width() > 120.0, "{:?}", missing.rect);
    shot(&mut h, "error-import-error.png");
}

#[test]
fn a_live_edit_in_the_long_worksheet_is_fast() {
    let path = root().join("examples/long-worksheet.mpp");
    let src = std::fs::read_to_string(&path).expect("read");
    assert!(src.lines().count() >= 290);
    // The evaluator alone.
    let mid = src.find("Section 9").expect("section") + 20;
    let at = src[mid..].find("$$ ").map(|i| mid + i + 3).expect("math");
    let edited = format!("{}{}{}", &src[..at], "1+", &src[at..]);
    let mut session = mpp_eval::Session::new();
    session.update(&src);
    let mut worst = 0.0f64;
    for i in 0..5 {
        let text = if i % 2 == 0 { &edited } else { &src };
        let t = Instant::now();
        session.update(text);
        worst = worst.max(t.elapsed().as_secs_f64() * 1000.0);
    }
    println!("session update after a one-character change: worst of 5 = {worst:.1} ms");

    // The IDE: type a character in the middle, then lay out and paint the page.
    let mut h = open(&path);
    let pos = at;
    if let Some(d) = h.state_mut().doc_mut() {
        d.editor.move_caret(pos, false);
    }
    h.run_steps(2);
    let mut worst_ui = 0.0f64;
    for _ in 0..5 {
        h.event(Event::Text("1".to_string()));
        let t = Instant::now();
        h.run_steps(1);
        h.state_mut().settle();
        h.run_steps(1);
        worst_ui = worst_ui.max(t.elapsed().as_secs_f64() * 1000.0);
    }
    println!("IDE edit, re-evaluate, re-layout and paint: worst of 5 = {worst_ui:.1} ms");
    if !cfg!(debug_assertions) {
        assert!(worst < 100.0, "session update took {worst:.1} ms");
        assert!(worst_ui < 100.0, "IDE edit took {worst_ui:.1} ms");
    }
}

/// Sources are written with `@` where LaTeX has a backslash.
fn tex(s: &str) -> String {
    s.replace('@', &char::from(92u8).to_string())
}

#[test]
fn figure_readouts_and_legend_labels_keep_their_spaces() {
    let tex = mpp_ide::figure::readout_latex("tangent at x = 1: y = x - 2");
    assert!(tex.starts_with(&tex_of("@text{tangent at }x = 1")), "{tex}");
    assert!(tex.contains(&tex_of("@text{: }y = x - 2")), "{tex}");
    let label = mpp_plot::Label::text("tangent line");
    assert_eq!(label.latex, tex_of("@text{tangent line}"));
    assert_eq!(mpp_plot::Label::text("sin").latex, "sin");
}

fn tex_of(s: &str) -> String {
    tex(s)
}
