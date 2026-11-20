//! The IDE wired to the engine's later features, driven through the real app
//! with egui_kittest: figures and sliders, grey automatic answers and their
//! spinner, Ctrl+Enter, folded steps, right-click number formats, the Steps and
//! Standard library tiles. Screenshots go to `target/ide-snapshots/`.

use std::time::Duration;

use egui::{Event, Key, Modifiers, PointerButton, Pos2, Vec2};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use mpp_ide::layout::tiles::Pane;
use mpp_ide::{theme, IdeApp};
use mpp_render::HitKind;

const SIZE: Vec2 = Vec2::new(1180.0, 900.0);

fn harness(app: IdeApp) -> Harness<'static, IdeApp> {
    Harness::builder()
        .with_size(SIZE)
        .with_max_steps(30)
        .with_context_setup(theme::install_fonts)
        .build_ui_state(|ui, app: &mut IdeApp| app.ui(ui), app)
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

/// Evaluates, waits for every pending automatic answer, and draws.
fn settle(h: &mut Harness<'static, IdeApp>) {
    h.run_steps(2);
    h.state_mut().apply_eval_settings();
    h.state_mut().settle();
    for _ in 0..400 {
        let pending = h.state().doc().is_some_and(|d| d.pending());
        if !pending {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        if let Some(d) = h.state_mut().doc_mut() {
            d.pump();
        }
    }
    h.run_steps(4);
}

fn settled(text: &str, choice: theme::ThemeChoice) -> Harness<'static, IdeApp> {
    let mut h = harness(app_with(text, choice));
    settle(&mut h);
    h
}

fn text_of(h: &Harness<'static, IdeApp>) -> String {
    h.state()
        .doc()
        .map(|d| d.editor.text().to_string())
        .unwrap_or_default()
}

/// Everything typeset on the page (the scan for raw markup reads this).
fn shown(h: &mut Harness<'static, IdeApp>) -> String {
    h.state_mut().visible_text(1100.0)
}

fn assert_no_markup(s: &str) {
    for bad in ["$", "\\", "_", "^"] {
        assert!(!s.contains(bad), "raw {bad:?} visible in {s:?}");
    }
}

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

/// The screen rectangle of the first result box on the page.
fn result_rect(h: &mut Harness<'static, IdeApp>) -> Option<egui::Rect> {
    let origin = h.state().diag.origin;
    let d = h.state_mut().doc_mut()?;
    let pc = d.page_cache()?.clone();
    pc.page.blocks.iter().find_map(|b| {
        b.dl.hits
            .iter()
            .find(|x| x.kind == HitKind::Result)
            .map(|x| {
                egui::Rect::from_min_size(
                    Pos2::new(origin.x + x.rect.x, origin.y + b.top + x.rect.y),
                    Vec2::new(x.rect.w.max(4.0), x.rect.h.max(4.0)),
                )
            })
    })
}

const WORKSHEET: &str = "# Graphs and answers\n\nThe parabola opens wider as $a$ shrinks.\n\n$$ a := 1, \\; a \\in [0, 3] $$\n\nFigure: $a x^2$, $\\sin x$, $x \\in [-3, 3]$.\n\nAn integral nobody asked about still gets a grey answer:\n\n$$ \\int_0^1 x^2 \\, dx $$\n\nThe working of a derivative, unfolded:\n\n$$ \\frac{d}{dx} x^2 \\sin(3x) = ?! $$\n\n$$ \\sin^2 x + \\cos^2 x \\overset{?}{=} 1 $$\n";

#[test]
fn a_figure_renders_and_its_slider_redraws_it() {
    let mut h = settled(WORKSHEET, theme::ThemeChoice::Light);
    let d = h.state_mut().doc_mut().expect("doc");
    assert_eq!(d.figures.list.len(), 1);
    let st = &mut d.figures.list[0];
    let sliders = st.sliders();
    assert_eq!(sliders.len(), 1, "the slider for a");
    assert_eq!(sliders[0].name, "a");
    let pt = mpp_ide::figure::plot_theme(mpp_ide::theme::Mode::Light);
    let before = st.scene(600.0, 400.0, &pt).expect("a scene");
    assert!(before.counts().0 >= 2, "both curves are drawn");
    st.set_value("a", 2.5);
    let after = st.scene(600.0, 400.0, &pt).expect("a scene");
    assert_ne!(before, after, "moving the slider redraws the curve");
    // The page leaves a row for the slider under the graph.
    let pc = d.page_cache().expect("page").clone();
    let fig = pc
        .page
        .blocks
        .iter()
        .find(|b| matches!(b.tag, mpp_ide::page::BlockTag::Figure(_)))
        .expect("figure block");
    assert_eq!(pc.doc_rows(fig.id), 1);
    h.run_steps(3);
    let s = shown(&mut h);
    assert_no_markup(&s);
}

#[test]
fn a_bare_integral_gets_a_grey_answer_and_ctrl_enter_keeps_it() {
    let src = "$$ \\int_0^1 x^2 \\, dx $$\n";
    let mut h = settled(src, theme::ThemeChoice::Light);
    let ev = h.state().doc().and_then(|d| d.eval.clone()).expect("eval");
    let auto = ev
        .auto_results()
        .find(|(_, v)| v.auto)
        .map(|(_, v)| v.clone());
    let auto = auto.expect("an automatic answer");
    assert!(
        auto.latex.as_deref().is_some_and(|l| l.contains('3')),
        "{auto:?}"
    );
    assert!(
        result_rect(&mut h).is_some(),
        "the grey answer is on the page"
    );
    assert_eq!(text_of(&h), src, "the file keeps what was typed");
    // Ctrl+Enter on the line writes the answer in.
    let inside = src.find("x^2").expect("x");
    if let Some(d) = h.state_mut().doc_mut() {
        d.editor.move_caret(inside, false);
    }
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    h.run_steps(3);
    let text = text_of(&h);
    assert!(text.contains("= \\frac{1}{3}"), "{text}");
    let path = std::env::temp_dir().join(format!("mpp-keep-{}.mpp", std::process::id()));
    h.state_mut()
        .doc_mut()
        .expect("doc")
        .save_as(&path)
        .expect("save");
    let on_disk = std::fs::read_to_string(&path).expect("read");
    assert!(on_disk.contains("\\frac{1}{3}"));
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_slow_answer_shows_a_spinner_then_the_result() {
    let mut app = app_with(
        "$$ \\int_0^1 x^2 e^x \\, dx $$\n",
        theme::ThemeChoice::Light,
    );
    // No time at all for automatic answers: every one is finished in the background.
    app.eval_settings.budget = Some(Duration::ZERO);
    let mut h = harness(app);
    h.run_steps(2);
    h.state_mut().apply_eval_settings();
    h.state_mut().settle();
    h.run_steps(2);
    let (pending, spinners) = {
        let d = h.state_mut().doc_mut().expect("doc");
        let pending = d.pending();
        let spinners = d.page_cache().map_or(0, |pc| pc.page.spinners.len());
        (pending, spinners)
    };
    if pending {
        assert!(
            spinners > 0,
            "a spinner turns while the answer is worked out"
        );
    }
    settle(&mut h);
    let d = h.state().doc().expect("doc");
    assert!(!d.pending());
    let ev = d.eval.clone().expect("eval");
    assert!(ev.auto_results().any(|(_, v)| v.auto && v.latex.is_some()));
}

#[test]
fn worked_steps_unfold_on_click_and_fill_the_steps_tile() {
    let src = "$$ \\frac{d}{dx} x^2 \\sin(3x) = ?! $$\n";
    let mut app = app_with(src, theme::ThemeChoice::Light);
    app.toggle_pane(Pane::Steps);
    let mut h = harness(app);
    settle(&mut h);
    let origin = h.state().diag.origin;
    let toggle = {
        let d = h.state_mut().doc_mut().expect("doc");
        d.page_cache()
            .and_then(|pc| pc.page.step_toggles.first().copied())
    };
    let toggle = toggle.expect("a fold line for the working");
    let before = h
        .state_mut()
        .doc_mut()
        .expect("doc")
        .page_cache()
        .map_or(0.0, |pc| pc.page.height);
    let c = toggle.rect.center() + origin.to_vec2();
    h.event(Event::PointerMoved(c));
    h.event(Event::PointerButton {
        pos: c,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.event(Event::PointerButton {
        pos: c,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(4);
    let d = h.state_mut().doc_mut().expect("doc");
    assert!(d.show().is_expanded(toggle.line), "the working is unfolded");
    let after = d.page_cache().map_or(0.0, |pc| pc.page.height);
    assert!(after > before, "the steps take room under the line");
    if let Some(d) = h.state_mut().doc_mut() {
        d.editor.move_caret(5, false);
    }
    h.run_steps(3);
    let s = shown(&mut h);
    assert_no_markup(&s);
    save_png(&mut h, "wiring-steps-light.png");
}

#[test]
fn right_click_switches_a_result_to_a_decimal() {
    let src = "$$ \\frac{1}{3} + \\frac{1}{6} = ? $$\n";
    let mut h = settled(src, theme::ThemeChoice::Light);
    let r = result_rect(&mut h).expect("a result");
    let c = r.center();
    h.event(Event::PointerMoved(c));
    h.event(Event::PointerButton {
        pos: c,
        button: PointerButton::Secondary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.event(Event::PointerButton {
        pos: c,
        button: PointerButton::Secondary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(3);
    h.get_by_label("Show as a decimal").click();
    h.run_steps(4);
    let d = h.state().doc().expect("doc");
    let line = d
        .eval
        .as_ref()
        .and_then(|e| e.queries().next())
        .map(|(_, v)| v.span);
    assert!(d.show().format_for(line.expect("line")).is_some());
    assert_eq!(text_of(&h), src, "only the view changed");
    let s = shown(&mut h);
    assert!(s.contains("0.5"), "{s}");
}

#[test]
fn the_library_tile_inserts_and_imports_a_formula() {
    let mut app = app_with("# Library\n\n", theme::ThemeChoice::Light);
    app.toggle_pane(Pane::Library);
    let mut h = harness(app);
    settle(&mut h);
    let (i, e) = mpp_ide::library::entries()
        .iter()
        .enumerate()
        .find(|(_, e)| e.entry.kind == mpp_eval::stdlib::EntryKind::Function && e.key.is_some())
        .expect("a library function");
    if let Some(d) = h.state_mut().doc_mut() {
        let end = d.editor.text().len();
        d.editor.move_caret(end, false);
    }
    h.state_mut()
        .library_action(i, mpp_ide::layout::tools_tiles::LibAction::Import);
    h.state_mut()
        .library_action(i, mpp_ide::layout::tools_tiles::LibAction::Insert);
    h.run_steps(3);
    let text = text_of(&h);
    assert!(text.contains(e.entry.file), "{text}");
    let head = e.entry.name.split('(').next().unwrap_or("");
    assert!(text.matches(head).count() >= 1, "{text}");
    // Autocomplete and did-you-mean know the library's names.
    assert!(mpp_ide::library::names()
        .iter()
        .any(|n| Some(n) == e.key.as_ref()));
    let s = shown(&mut h);
    assert_no_markup(&s);
    save_png(&mut h, "wiring-library-tile-light.png");
}

#[test]
fn screenshots_of_a_worksheet_light_and_dark_and_a_surface_tile() {
    for (name, choice) in [
        ("light", theme::ThemeChoice::Light),
        ("dark", theme::ThemeChoice::Dark),
    ] {
        let mut h = settled(WORKSHEET, choice);
        let line = {
            let d = h.state_mut().doc_mut().expect("doc");
            d.page_cache()
                .and_then(|pc| pc.page.step_toggles.first().map(|t| t.line))
        };
        if let (Some(line), Some(d)) = (line, h.state_mut().doc_mut()) {
            d.show_mut().toggle_steps(line);
        }
        h.run_steps(4);
        let s = shown(&mut h);
        assert_no_markup(&s);
        save_png(&mut h, &format!("wiring-worksheet-{name}.png"));
    }
    let src = "# Surface\n\nFigure: $z = \\sin x \\cos y$, $x \\in [-3, 3]$, $y \\in [-3, 3]$.\n";
    let mut app = app_with(src, theme::ThemeChoice::Light);
    app.settle();
    app.open_graph_tile(0, 0);
    let mut h = harness(app);
    settle(&mut h);
    let d = h.state().doc().expect("doc");
    assert!(d.figures.list.first().is_some_and(|f| f.is_3d()));
    save_png(&mut h, "wiring-surface-tile-light.png");
}
