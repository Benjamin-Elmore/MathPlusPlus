//! Proofs in the IDE, driven through the real app with egui_kittest: status marks
//! beside proof steps and the checker's summary under each theorem (never written
//! into the file), Prove… with no API key (a typeset message, Start disabled), a
//! full agent run against a scripted server ending in a diff that is accepted, the
//! Lean alignment view and the outline's statuses. Screenshots go to
//! `target/ide-snapshots/`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{Event, Modifiers, Pos2, Vec2};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use mpp_ide::diagnostics::{Kind, Level};
use mpp_ide::explorer::outline::OutlineKind;
use mpp_ide::proof::ui::NO_KEY;
use mpp_ide::{theme, IdeApp};
use mpp_proof::agent::{ApiKey, KeyStore, MemoryStore, Scripted, ScriptedTransport};
use mpp_proof::StepStatus;
use serde_json::json;

const SIZE: Vec2 = Vec2::new(1280.0, 900.0);

fn proofs_example() -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/proofs.mpp");
    std::fs::read_to_string(p).expect("examples/proofs.mpp")
}

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
    h.state_mut().settle();
    run(h);
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mpp-proof-ui-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    dir
}

fn open(text: &str, tag: &str) -> (Harness<'static, IdeApp>, PathBuf) {
    let dir = temp_dir(tag);
    let file = dir.join("proofs.mpp");
    std::fs::write(&file, text).expect("write");
    let mut app = IdeApp::empty();
    app.set_theme_choice(theme::ThemeChoice::Light);
    app.open_file(&file, false).expect("open");
    let mut h = harness(app);
    settle(&mut h);
    run(&mut h);
    (h, file)
}

fn doc_text(h: &Harness<'static, IdeApp>) -> String {
    h.state()
        .doc()
        .map(|d| d.editor.text().to_string())
        .unwrap_or_default()
}

fn save_png(h: &mut Harness<'static, IdeApp>, name: &str) {
    static RENDER: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _gpu = RENDER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let img = h.render().expect("render");
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ide-snapshots");
    std::fs::create_dir_all(&dir).expect("dir");
    img.save(dir.join(name)).expect("png");
}

fn hover(h: &mut Harness<'static, IdeApp>, p: Pos2) {
    h.event(Event::PointerMoved(p));
    run(h);
}

fn click(h: &mut Harness<'static, IdeApp>, p: Pos2) {
    h.event(Event::PointerMoved(p));
    h.event(Event::PointerButton {
        pos: p,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.event(Event::PointerButton {
        pos: p,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    run(h);
}

/// The glyphs on the page (the page has no space glyphs, so neither do these).
fn visible(h: &mut Harness<'static, IdeApp>) -> String {
    h.state_mut().visible_text(900.0)
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect()
}

#[test]
fn proof_steps_show_statuses_and_theorems_their_summaries() {
    let src = proofs_example();
    let (mut h, file) = open(&src, "statuses");
    let pc = h
        .state_mut()
        .doc_mut()
        .map(|d| d.page(900.0, 18.0, theme::Mode::Light))
        .expect("page");
    let statuses: Vec<StepStatus> = pc.proof.steps.iter().map(|s| s.status).collect();
    for want in [
        StepStatus::Symbolic,
        StepStatus::Structure,
        StepStatus::Wrong,
        StepStatus::Cited,
        StepStatus::Gap,
    ] {
        assert!(statuses.contains(&want), "{want:?} in {statuses:?}");
    }
    // N9-style marks: a wrong step is an error, a gap a warning, cited a note.
    let levels: Vec<(Level, StepStatus)> = pc
        .problems
        .iter()
        .filter_map(|p| match p.kind {
            Kind::ProofStep { status } => Some((p.level, status)),
            _ => None,
        })
        .collect();
    assert!(
        levels.contains(&(Level::Error, StepStatus::Wrong)),
        "{levels:?}"
    );
    assert!(
        levels.contains(&(Level::Warning, StepStatus::Gap)),
        "{levels:?}"
    );
    assert!(
        levels.contains(&(Level::Info, StepStatus::Cited)),
        "{levels:?}"
    );
    assert!(
        !levels.iter().any(|(_, s)| *s == StepStatus::Symbolic),
        "checked steps get no squiggle"
    );
    let text = visible(&mut h);
    assert!(text.contains(&squash("Checker: proved")), "{text}");
    assert!(
        text.contains(&squash("Checker: not proved: a step is wrong")),
        "{text}"
    );
    assert!(
        text.contains(&squash("Checker: proved, using 1 cited result")),
        "{text}"
    );
    assert!(text.contains(&squash("Checker: 1 gap")), "{text}");
    // Written numbers and titles are kept in the headings.
    assert!(text.contains(&squash("Lemma 1")), "{text}");
    assert!(text.contains(&squash("odd squares")), "{text}");
    // Preview only: the file is untouched.
    assert_eq!(doc_text(&h), src);
    assert_eq!(std::fs::read_to_string(&file).unwrap_or_default(), src);
    save_png(&mut h, "proof-statuses.png");

    // Scrolled to Theorems 3 to 5, hovering the wrong step's mark shows the
    // counterexample.
    let at = src.find("**Theorem 4**").unwrap_or(0);
    if let Some(d) = h.state_mut().doc_mut() {
        d.scroll_to = Some(mpp_syntax::Span::new(at, at + 4));
    }
    run(&mut h);
    run(&mut h);
    let origin = h.state().diag.origin;
    let shown = h
        .state()
        .doc()
        .and_then(|d| d.page_cache().cloned())
        .expect("the page on screen");
    let marks = mpp_ide::proof::marks::mark_rects(
        &shown.page,
        &shown.proof.steps,
        origin,
        0.0,
        h.state().size,
    );
    let wrong = marks
        .iter()
        .find(|(_, s)| s.status == StepStatus::Wrong)
        .map(|(r, _)| r.center())
        .expect("a mark for the wrong step");
    hover(&mut h, wrong);
    save_png(&mut h, "theorem-summary.png");
}

#[test]
fn prove_without_a_key_explains_and_cannot_start() {
    let (mut h, _) = open(&proofs_example(), "nokey");
    h.state_mut().prove("Theorem 3");
    run(&mut h);
    assert!(
        h.query_by_label_contains("The proof agent is off until an API key is stored.")
            .is_some(),
        "the typeset message: {NO_KEY}"
    );
    let start = h.get_by_label("Start");
    assert!(
        start.accesskit_node().is_disabled(),
        "Start is off without a key"
    );
    save_png(&mut h, "agent-panel-no-key.png");
    // The settings page stores a key; the panel then turns on.
    h.get_by_label("Open Settings \u{2192} Proof agent").click();
    run(&mut h);
    assert!(h.state().proof.settings.open);
    h.state_mut()
        .proof
        .store_key("test-key-123")
        .expect("stored");
    run(&mut h);
    assert!(h
        .query_by_label_contains("The proof agent is off")
        .is_none());
    assert!(!h.get_by_label("Start").accesskit_node().is_disabled());
}

const GAP_DOC: &str = "**Theorem 1.** For every real $m$, $(2m+1)^2 = 4m^2 + 4m + 1$.\n\n**Proof.**\n1. It holds \u{2014} *(gap: not shown yet)*.\n\u{220e}\n";

fn scripted_run() -> ScriptedTransport {
    let steps = "1. $(2m+1)^2 = 4m^2 + 4m + 1$ \u{2014} by algebra.";
    let events = |list: serde_json::Value| {
        Scripted::ok(
            "GET",
            "/v1/sessions/sesn_1/events",
            &json!({"data": list, "next_page": null}).to_string(),
        )
    };
    ScriptedTransport::new(vec![
        Scripted::ok("POST", "/v1/environments", r#"{"id":"env_1"}"#),
        Scripted::ok("POST", "/v1/agents", r#"{"id":"agent_1"}"#),
        Scripted::ok("POST", "/v1/sessions", r#"{"id":"sesn_1"}"#),
        Scripted::ok("POST", "/v1/sessions/sesn_1/events", "{}"),
        events(json!([
            {"id": "sevt_1", "type": "agent.custom_tool_use", "name": "write_steps",
             "input": {"result": "Theorem 1", "steps": steps}},
            {"id": "sevt_2", "type": "span.model_request_end", "model_usage": {
                "input_tokens": 1000, "output_tokens": 500,
                "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0}},
        ])),
        Scripted::ok("POST", "/v1/sessions/sesn_1/events", "{}"),
        events(json!([
            {"id": "sevt_3", "type": "agent.message",
             "content": [{"type": "text", "text": "I replaced the gap with one algebra step."}]},
            {"id": "sevt_4", "type": "session.status_idle", "stop_reason": {"type": "end_turn"}},
        ])),
    ])
}

#[test]
fn an_agent_run_ends_in_a_diff_the_user_accepts() {
    let (mut h, file) = open(GAP_DOC, "agent");
    {
        let app = h.state_mut();
        let keys = MemoryStore::default();
        keys.set(&ApiKey::new("test-key-123")).expect("key");
        app.proof.set_key_store(Arc::new(keys));
        app.proof.transport = Some(Arc::new(scripted_run()));
        app.proof.config.poll_interval = std::time::Duration::ZERO;
        app.proof.config.max_polls = 10;
        app.prove("Theorem 1");
    }
    run(&mut h);
    h.get_by_label("Start").click();
    run(&mut h);
    h.state_mut().proof.wait(std::time::Duration::from_secs(30));
    run(&mut h);
    let report = h.state().proof.agent.report.clone().expect("a report");
    assert_eq!(report.verdict_text, "proved");
    assert!(h.state().proof.agent.cost.contains("of $2.00"));
    assert!(
        h.query_by_label("Checker: proved").is_some(),
        "the checker's verdict, not the agent's"
    );
    assert!(h
        .query_by_label("The agent says: I replaced the gap with one algebra step.")
        .is_some());
    assert_eq!(doc_text(&h), GAP_DOC, "nothing changes before Accept");
    save_png(&mut h, "agent-panel-diff.png");
    h.get_by_label("Accept").click();
    run(&mut h);
    let now = doc_text(&h);
    assert!(now.contains("by algebra") && !now.contains("gap:"), "{now}");
    // Accepting edits the open file; saving is still the user's choice.
    assert_eq!(std::fs::read_to_string(&file).unwrap_or_default(), GAP_DOC);
    assert_eq!(
        h.state().proof.settings.agent_id.as_deref(),
        Some("agent_1"),
        "the agent id is kept for the next run"
    );
}

#[test]
fn the_alignment_view_shows_both_statements_and_a_confirm_box() {
    let (mut h, dir_file) = open(&proofs_example(), "align");
    h.state_mut().align_with_lean("Theorem 2");
    run(&mut h);
    assert!(h.query_by_label("Math++").is_some());
    assert!(h.query_by_label("Lean").is_some());
    assert!(
        h.query_by_label_contains("Finset.Icc").is_some(),
        "the Lean statement of the Gauss sum is shown"
    );
    h.get_by_label("They say the same thing").click();
    run(&mut h);
    assert!(h.state().proof.align.confirmed.contains("Theorem 2"));
    save_png(&mut h, "lean-alignment.png");
    // Export to Lean writes a Lake project (into a folder chosen in a dialog in
    // the app; here directly).
    let out = dir_file.with_file_name("lean-out");
    let msg = h.state_mut().export_lean_to(&out).expect("export");
    assert!(msg.contains("Lean project"), "{msg}");
    assert!(out.join("lakefile.lean").exists() || out.join("lakefile.toml").exists());
    // The confirmation is not written into the file.
    assert_eq!(doc_text(&h), proofs_example());
}

#[test]
fn the_outline_lists_theorems_with_their_status() {
    let (mut h, _) = open(&proofs_example(), "outline");
    let items = h.state_mut().outline_items();
    let theorems: Vec<(String, Option<String>)> = items
        .iter()
        .filter(|i| i.kind == OutlineKind::Theorem)
        .map(|i| (i.latex.clone(), i.status.as_ref().map(|s| s.0.clone())))
        .collect();
    let status_of = |name: &str| {
        theorems
            .iter()
            .find(|(l, _)| l.contains(name))
            .and_then(|(_, s)| s.clone())
    };
    assert_eq!(status_of("Lemma 1").as_deref(), Some("proved"));
    assert_eq!(
        status_of("Theorem 3").as_deref(),
        Some("not proved: a step is wrong")
    );
    assert_eq!(status_of("Theorem 5").as_deref(), Some("1 gap"));
    assert!(theorems.iter().any(|(l, _)| l.contains("odd squares")));
}

#[test]
fn insert_conjecture_opens_the_agent_in_conjecture_mode() {
    let (mut h, _) = open(&proofs_example(), "conjecture");
    h.state_mut().propose_conjectures();
    run(&mut h);
    let p = &h.state().proof.agent;
    assert!(p.open);
    assert_eq!(p.mode, mpp_proof::agent::Mode::Conjecture);
    assert_eq!(p.target, "Proofs, step by step");
    assert!(mpp_ide::insert::find("doc.conjecture").is_some());
}

#[test]
fn right_click_on_a_theorem_offers_prove() {
    let (mut h, _) = open(&proofs_example(), "menu");
    let pc = h
        .state_mut()
        .doc_mut()
        .map(|d| d.page(900.0, 18.0, theme::Mode::Light))
        .expect("page");
    let origin = h.state().diag.origin;
    let at = pc.proof.steps[0].span;
    let rect = pc.page.rect_of_span(at).expect("step on the page");
    let p = origin + rect.center().to_vec2();
    h.event(Event::PointerMoved(p));
    h.event(Event::PointerButton {
        pos: p,
        button: egui::PointerButton::Secondary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.event(Event::PointerButton {
        pos: p,
        button: egui::PointerButton::Secondary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    run(&mut h);
    h.get_by_label("Prove\u{2026}").click();
    run(&mut h);
    assert!(h.state().proof.agent.open);
    assert_eq!(h.state().proof.agent.target, "Lemma 1");
    let _ = click;
}
