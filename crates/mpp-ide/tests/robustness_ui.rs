//! Performance and robustness of the IDE on awkward folders and files: a slow file
//! next to the open one, a byte-order mark, a very long line, two windows sharing the
//! recovery folder.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use mpp_ide::IdeApp;

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mpp-robust-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("scratch folder");
    d
}

#[test]
fn a_slow_file_in_the_folder_never_holds_up_the_open_file() {
    let d = scratch("sibling");
    // Factoring this number takes about ten seconds; the folder index used to evaluate
    // every sibling file before the open file's results were shown.
    std::fs::write(
        d.join("slow.mpp"),
        "$$ \\theta := 2 $$\n\n$$ \\operatorname{factor}(2^{256} + 1) = ? $$\n",
    )
    .expect("write");
    let open = d.join("open.mpp");
    std::fs::write(&open, "$$ 1 + 1 = ? $$\n").expect("write");
    let mut app = IdeApp::empty();
    app.open_file(&open, false).expect("open");
    let start = Instant::now();
    let doc = app.doc_mut().expect("a document");
    doc.settle();
    let took = start.elapsed();
    assert!(took < Duration::from_secs(5), "settled in {took:?}");
    let ev = doc.eval.as_ref().expect("evaluated");
    assert_eq!(
        ev.queries().next().and_then(|(_, l)| l.plain.clone()),
        Some("2".to_string())
    );
    // The sibling's names are still indexed, read without evaluating it.
    let bg = doc.background().expect("background");
    assert_eq!(bg.index.files_defining("θ"), ["slow.mpp"]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_second_window_never_takes_the_first_windows_recovery_copies() {
    use mpp_ide::settings::AutosaveMode;
    let d = scratch("two-windows");
    let data = d.join("appdata");
    let f = d.join("m.mpp");
    std::fs::write(&f, "# M\n").expect("write");
    let offered = |app: &IdeApp| app.ws.autosave.dialog.as_ref().map_or(0, |x| x.files.len());
    // Window A edits and keeps a recovery copy.
    let mut a = IdeApp::empty();
    a.start_up(&data, None);
    a.open_file(&f, false).expect("open");
    a.set_autosave_mode(AutosaveMode::Off);
    {
        let doc = a.doc_mut().expect("doc");
        doc.editor.move_caret(4, false);
        doc.editor.insert_text("\n$$ precious := 1 $$\n");
        doc.touched();
    }
    a.write_recovery();
    // Window B starts while A runs: A's live edits are not a crash.
    let mut b = IdeApp::empty();
    b.start_up(&data, None);
    assert_eq!(offered(&b), 0);
    // B closes cleanly; that must not delete A's copy.
    b.clear_recovery_on_exit();
    drop(b);
    a.write_recovery();
    // A crashes (no clean exit): the next start offers its copy.
    drop(a);
    let mut c = IdeApp::empty();
    c.start_up(&data, None);
    assert_eq!(offered(&c), 1);
    let _ = std::fs::remove_dir_all(&d);
}

/// About 3000 lines: the long worksheet ten times over, with a shared definition.
fn long_document() -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let src = std::fs::read_to_string(root.join("examples/long-worksheet.mpp")).expect("read");
    let mut out = String::from("# Big worksheet\n\nLet $K := 2$.\n\n");
    for r in 0..10 {
        out.push_str(&src.replace("# ", &format!("## Part {r} ")));
        out.push_str(&format!("\n$$ K + {r} = ? $$\n\n"));
    }
    out
}

fn harness(app: IdeApp) -> egui_kittest::Harness<'static, IdeApp> {
    egui_kittest::Harness::builder()
        .with_size(egui::Vec2::new(1180.0, 1500.0))
        .with_max_steps(40)
        .with_context_setup(mpp_ide::theme::install_fonts)
        .build_ui_state(|ui, app: &mut IdeApp| app.ui(ui), app)
}

#[test]
fn typing_in_a_long_document_stays_quick() {
    let d = scratch("latency");
    let path = d.join("big.mpp");
    let src = long_document();
    std::fs::write(&path, &src).expect("write");
    let mut app = IdeApp::empty();
    app.open_file(&path, false).expect("open");
    let mut h = harness(app);
    h.run_steps(3);
    h.state_mut().settle();
    h.run_steps(3);
    let rounds = if cfg!(debug_assertions) { 2 } else { 6 };
    for at in [2, src.len() / 2, src.len()] {
        if let Some(doc) = h.state_mut().doc_mut() {
            let len = doc.editor.text().len();
            doc.editor.move_caret(at.min(len), false);
        }
        h.run_steps(2);
        let mut times = Vec::new();
        for _ in 0..rounds {
            h.event(egui::Event::Text("1".to_string()));
            let start = Instant::now();
            h.run_steps(1);
            h.state_mut().settle();
            h.run_steps(1);
            times.push(start.elapsed());
        }
        times.sort();
        let median = times[times.len() / 2];
        // Keystroke to evaluated and laid out (was 150-243 ms before layouts were
        // reused between edits).
        if !cfg!(debug_assertions) {
            assert!(median < Duration::from_millis(100), "at {at}: {times:?}");
        }
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn reused_layouts_match_a_fresh_layout_after_an_edit_above_them() {
    let src = long_document();
    let mut doc = mpp_ide::Doc::with_text(&src, None);
    let width = 900.0;
    let size = 18.0;
    let mode = mpp_ide::theme::Mode::Light;
    let _ = doc.page(width, size, mode);
    doc.editor.move_caret(2, false);
    doc.editor.insert_text("inserted words ");
    let edited = doc.editor.text().to_string();
    let reused = doc.page(width, size, mode);
    // The same text laid out on a thread whose caches are empty.
    let fresh = std::thread::spawn(move || {
        let mut d = mpp_ide::Doc::with_text(&edited, None);
        let p = d.page(width, size, mode);
        (p.page.stops.clone(), p.page.chars.clone(), p.page.height)
    })
    .join()
    .expect("thread");
    assert_eq!(reused.page.chars, fresh.1);
    assert_eq!(reused.page.stops.len(), fresh.0.len());
    assert!(reused
        .page
        .stops
        .iter()
        .zip(&fresh.0)
        .all(|(a, b)| a.offset == b.offset && (a.x - b.x).abs() < 0.01));
    assert!((reused.page.height - fresh.2).abs() < 0.01);
}

#[test]
fn a_huge_one_line_paragraph_is_laid_out_in_part_and_quickly() {
    // A 10 MB line froze the window for over a minute; the page now lays out the
    // first part of a long run and says how much is not shown.
    let text = "x".repeat(2_000_000);
    let mut doc = mpp_ide::Doc::with_text(&text, None);
    let start = Instant::now();
    let pc = doc.page(900.0, 18.0, mpp_ide::theme::Mode::Light);
    let took = start.elapsed();
    let limit = if cfg!(debug_assertions) { 20 } else { 2 };
    assert!(took < Duration::from_secs(limit), "{took:?}");
    assert!(pc.page.chars.len() < 50_000, "{}", pc.page.chars.len());
    // The shown part wraps inside the column instead of running off the page.
    assert!(pc.page.height > 100.0);
}

#[test]
fn a_keystroke_in_a_rejected_deep_nest_stays_quick() {
    let d = scratch("deep-nest");
    let path = d.join("paren.mpp");
    let mut text = String::from("$$ ");
    text.push_str(&"(".repeat(100_000));
    text.push_str(&")".repeat(100_000));
    text.push_str(" $$\n");
    std::fs::write(&path, &text).expect("write");
    let mut app = IdeApp::empty();
    app.open_file(&path, false).expect("open");
    let mut h = harness(app);
    h.run_steps(2);
    h.state_mut().settle();
    h.run_steps(2);
    h.event(egui::Event::Text("1".to_string()));
    let start = Instant::now();
    h.run_steps(1);
    h.state_mut().settle();
    h.run_steps(1);
    let took = start.elapsed();
    let limit = if cfg!(debug_assertions) { 5_000 } else { 300 };
    assert!(took < Duration::from_millis(limit), "{took:?}");
    let _ = std::fs::remove_dir_all(&d);
}

/// About 190 KB of random maths-like characters, as a fuzzer would write them: a
/// thousand blocks, most with errors.
fn junk_maths(len: usize) -> String {
    let alphabet: Vec<&str> = "$@{}()[]^_&|#*>-=?!:\n abcx0123"
        .split("")
        .filter(|s| !s.is_empty())
        .chain([
            r"\frac",
            r"\int",
            r"\sum",
            r"\begin{cases}",
            r"\end{cases}",
            "$$",
            "[[",
            "]]",
        ])
        .map(|s| if s == "@" { "\\" } else { s })
        .collect();
    let mut seed: u64 = 1;
    let mut out = String::new();
    for _ in 0..len {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        out.push_str(alphabet[(seed >> 33) as usize % alphabet.len()]);
    }
    out
}

#[test]
fn a_keystroke_in_a_long_file_of_junk_maths_stays_quick() {
    // 190 KB of junk with 7000 problems: a keystroke took 0.6-0.9 s, re-laying out
    // every paragraph with a link, placing and classifying every problem again and
    // scanning the whole text for links several times per frame.
    let d = scratch("junk");
    let path = d.join("junk.mpp");
    std::fs::write(&path, junk_maths(100_000)).expect("write");
    let mut app = IdeApp::empty();
    app.open_file(&path, false).expect("open");
    let mut h = harness(app);
    h.run_steps(2);
    h.state_mut().settle();
    h.run_steps(2);
    if let Some(doc) = h.state_mut().doc_mut() {
        let mid = doc.editor.text().len() / 2;
        doc.editor.move_caret(mid, false);
    }
    h.run_steps(2);
    let mut times = Vec::new();
    for _ in 0..3 {
        h.event(egui::Event::Text("1".to_string()));
        let start = Instant::now();
        h.run_steps(1);
        h.state_mut().settle();
        h.run_steps(1);
        times.push(start.elapsed());
    }
    times.sort();
    if !cfg!(debug_assertions) {
        assert!(times[1] < Duration::from_millis(300), "{times:?}");
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn problems_placed_from_the_cache_match_a_fresh_placement() {
    let src = junk_maths(20_000);
    let mut doc = mpp_ide::Doc::with_text(&src, None);
    let (width, size, mode) = (900.0, 18.0, mpp_ide::theme::Mode::Light);
    doc.settle();
    let _ = doc.page(width, size, mode);
    doc.editor.move_caret(src.len() / 3, false);
    doc.editor.insert_text("x + ");
    doc.settle();
    let edited = doc.editor.text().to_string();
    let reused = doc.page(width, size, mode);
    // The same text placed on a thread whose caches are empty.
    let fresh = std::thread::spawn(move || {
        let mut d = mpp_ide::Doc::with_text(&edited, None);
        d.settle();
        let p = d.page(width, size, mode);
        p.problems
            .iter()
            .map(|p| (p.span, p.message.clone(), p.rects.clone(), p.block))
            .collect::<Vec<_>>()
    })
    .join()
    .expect("thread");
    let reused: Vec<_> = reused
        .problems
        .iter()
        .map(|p| (p.span, p.message.clone(), p.rects.clone(), p.block))
        .collect();
    assert!(reused.len() > 100, "{}", reused.len());
    assert_eq!(reused.len(), fresh.len());
    for (a, b) in reused.iter().zip(&fresh) {
        assert_eq!((a.0, &a.1, a.3), (b.0, &b.1, b.3));
        assert_eq!(a.2.len(), b.2.len(), "{a:?} {b:?}");
        for (r, s) in a.2.iter().zip(&b.2) {
            assert!((r.min - s.min).length() < 0.01 && (r.max - s.max).length() < 0.01);
        }
    }
}
