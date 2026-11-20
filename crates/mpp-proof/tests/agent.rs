//! The proof agent client against a scripted fake server: the tool loop, the spend cap,
//! the rules the client enforces, and the API key's handling. No network, no key.

use std::path::PathBuf;
use std::time::Duration;

use mpp_proof::agent::{
    self, AgentConfig, AgentError, ApiKey, KeyStore, MemoryStore, Mode, RunStop, Scripted,
    ScriptedTransport, Task,
};
use serde_json::json;

const KEY: &str = "sk-ant-test-0123456789abcdef";

const GAP_DOC: &str = "**Theorem 1.** For every real $m$, $(2m+1)^2 = 4m^2 + 4m + 1$.\n\n\
**Proof.**\n1. It holds — *(gap: not shown yet)*.\n∎\n";

fn store() -> MemoryStore {
    let s = MemoryStore::default();
    let _ = s.set(&ApiKey::new(KEY));
    s
}

fn config() -> AgentConfig {
    AgentConfig {
        poll_interval: Duration::ZERO,
        max_polls: 20,
        spend_cap_usd: 2.0,
        ..AgentConfig::default()
    }
}

fn task(source: &str, target: &str) -> Task {
    Task {
        path: PathBuf::from("doc.mpp"),
        source: source.into(),
        target: target.into(),
        mode: Mode::Prove,
    }
}

/// The opening of every scripted session: environment, agent, session, first message.
fn opening() -> Vec<Scripted> {
    vec![
        Scripted::ok("POST", "/v1/environments", r#"{"id":"env_1"}"#),
        Scripted::ok("POST", "/v1/agents", r#"{"id":"agent_1"}"#),
        Scripted::ok("POST", "/v1/sessions", r#"{"id":"sesn_1"}"#),
        Scripted::ok("POST", "/v1/sessions/sesn_1/events", "{}"),
    ]
}

fn events(list: serde_json::Value) -> Scripted {
    Scripted::ok(
        "GET",
        "/v1/sessions/sesn_1/events",
        &json!({"data": list, "next_page": null}).to_string(),
    )
}

fn post_result() -> Scripted {
    Scripted::ok("POST", "/v1/sessions/sesn_1/events", "{}")
}

fn tool_use(id: &str, name: &str, input: serde_json::Value) -> serde_json::Value {
    json!({"id": id, "type": "agent.custom_tool_use", "name": name, "input": input})
}

fn usage(id: &str, output: u64) -> serde_json::Value {
    json!({"id": id, "type": "span.model_request_end", "model_usage": {
        "input_tokens": 1000, "output_tokens": output, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0
    }})
}

fn idle(id: &str, why: &str) -> serde_json::Value {
    json!({"id": id, "type": "session.status_idle", "stop_reason": {"type": why}})
}

fn message(id: &str, text: &str) -> serde_json::Value {
    json!({"id": id, "type": "agent.message", "content": [{"type": "text", "text": text}]})
}

#[test]
fn the_feature_is_off_without_a_key_and_needs_a_spend_cap() {
    let empty = MemoryStore::default();
    assert!(!agent::available(&empty));
    let t = ScriptedTransport::new(vec![]);
    let r = agent::run(
        &task(GAP_DOC, "Theorem 1"),
        &config(),
        &empty,
        &t,
        &mut |_| {},
    );
    assert!(matches!(r, Err(AgentError::NoKey)));
    assert!(t.requests().is_empty(), "nothing is sent without a key");
    let mut c = config();
    c.spend_cap_usd = 0.0;
    let r = agent::run(&task(GAP_DOC, "Theorem 1"), &c, &store(), &t, &mut |_| {});
    assert!(matches!(r, Err(AgentError::NoSpendCap)));
    assert!(agent::available(&store()));
}

#[test]
fn the_tool_loop_runs_tools_here_and_ends_with_a_diff() {
    let steps = "1. $(2m+1)^2 = 4m^2 + 4m + 1$ — by algebra.";
    let mut script = opening();
    script.push(events(json!([
        tool_use(
            "sevt_1",
            "evaluate",
            json!({"maths": "(2m+1)^2 \\overset{?}{=} 4m^2+4m+1"})
        ),
        usage("sevt_2", 500),
    ])));
    script.push(post_result());
    script.push(events(json!([
        tool_use("sevt_1", "evaluate", json!({"maths": "x"})),
        usage("sevt_2", 500),
        tool_use(
            "sevt_3",
            "write_steps",
            json!({"result": "Theorem 1", "steps": steps})
        ),
    ])));
    script.push(post_result());
    script.push(events(json!([
        usage("sevt_4", 300),
        message("sevt_5", "Done: the theorem is proved."),
        idle("sevt_6", "end_turn"),
    ])));
    let t = ScriptedTransport::new(script);
    let mut counter = Vec::new();
    let report = agent::run(
        &task(GAP_DOC, "Theorem 1"),
        &config(),
        &store(),
        &t,
        &mut |m| counter.push(m.label()),
    )
    .expect("the run");
    assert_eq!(report.stop, RunStop::Finished);
    assert_eq!(t.remaining(), 0, "every scripted reply was used");
    // The verdict comes from the checker on the draft.
    assert_eq!(report.verdict_text, "proved");
    let p = report.proposal.expect("a proposal");
    assert!(p.changes_anything());
    assert!(p.diff.contains("+1. $(2m+1)^2"), "{}", p.diff);
    assert!(p.diff.contains("-1. It holds"), "{}", p.diff);
    assert_eq!(report.agent_says, "Done: the theorem is proved.");
    // The two tools ran locally.
    let names: Vec<&str> = report.tool_calls.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["evaluate", "write_steps"]);
    // The live counter was updated after each model request.
    assert_eq!(counter.len(), 2);
    // Requests: the tool results are answered by id, the session has a budget.
    let reqs = t.requests();
    let bodies: Vec<String> = reqs.iter().filter_map(|r| r.body.clone()).collect();
    assert!(bodies
        .iter()
        .any(|b| b.contains("\"custom_tool_use_id\":\"sevt_1\"")));
    assert!(bodies
        .iter()
        .any(|b| b.contains("\"max_list_cost\"") && b.contains("\"amount\":\"200\"")));
    assert!(bodies
        .iter()
        .any(|b| b.contains("\"claude-opus-5-5\"") && b.contains("\"effort\":\"high\"")));
    for r in &reqs {
        assert!(r
            .headers
            .iter()
            .any(|(k, v)| k == "anthropic-beta" && v == "managed-agents-2026-04-01"));
    }
}

#[test]
fn the_agent_can_never_certify_its_own_work() {
    // The agent writes a wrong step and says it proved the theorem.
    let steps = "1. For every real $m$: $(2m+1)^2 = 4m^2 + 1$ — by algebra.";
    let mut script = opening();
    script.push(events(json!([tool_use(
        "sevt_1",
        "write_steps",
        json!({"result": "Theorem 1", "steps": steps})
    )])));
    script.push(post_result());
    script.push(events(json!([
        message("sevt_2", "I have proved Theorem 1. ✓"),
        idle("sevt_3", "end_turn"),
    ])));
    let t = ScriptedTransport::new(script);
    let report = agent::run(
        &task(GAP_DOC, "Theorem 1"),
        &config(),
        &store(),
        &t,
        &mut |_| {},
    )
    .expect("the run");
    assert_ne!(report.verdict_text, "proved");
    let r = report.checked.result("Theorem 1").expect("checked");
    assert!(!r.summary.verdict.is_proved(), "{}", r.summary.text);
    // Status marks written by the agent are refused.
    let mut script = opening();
    script.push(events(json!([tool_use(
        "sevt_1",
        "write_steps",
        json!({"result": "Theorem 1", "steps": "1. $1 = 1$ — by algebra. ✓"})
    )])));
    script.push(post_result());
    script.push(events(json!([idle("sevt_2", "end_turn")])));
    let t = ScriptedTransport::new(script);
    let report = agent::run(
        &task(GAP_DOC, "Theorem 1"),
        &config(),
        &store(),
        &t,
        &mut |_| {},
    )
    .expect("the run");
    assert!(report.tool_calls[0].is_error);
    assert!(report.proposal.is_none(), "a refused write changes nothing");
}

#[test]
fn the_spend_cap_stops_the_run() {
    let mut script = opening();
    // 200 000 output tokens at $20 per million is $4, over the $2 cap.
    script.push(events(json!([usage("sevt_1", 200_000)])));
    script.push(Scripted::ok("POST", "/v1/sessions/sesn_1/events", "{}")); // the interrupt
    let t = ScriptedTransport::new(script);
    let mut last = String::new();
    let report = agent::run(
        &task(GAP_DOC, "Theorem 1"),
        &config(),
        &store(),
        &t,
        &mut |m| last = m.label(),
    )
    .expect("the run");
    assert_eq!(report.stop, RunStop::SpendCap);
    assert!(report.cost.usd() >= 2.0);
    assert!(last.starts_with("$4.00 of $2.00"), "{last}");
    let interrupt = t
        .requests()
        .last()
        .and_then(|r| r.body.clone())
        .unwrap_or_default();
    assert!(interrupt.contains("user.interrupt"));
}

#[test]
fn the_server_budget_ends_the_run() {
    let mut script = opening();
    script.push(events(json!([
        {"id": "sevt_1", "type": "session.usage", "list_cost": {"amount": "205", "currency": "USD"}},
        idle("sevt_2", "budget_reached"),
    ])));
    let t = ScriptedTransport::new(script);
    let report = agent::run(
        &task(GAP_DOC, "Theorem 1"),
        &config(),
        &store(),
        &t,
        &mut |_| {},
    )
    .expect("the run");
    assert_eq!(report.stop, RunStop::BudgetReached);
    assert_eq!(report.cost.reported_usd, Some(2.05));
}

#[test]
fn navier_stokes_theorem_6_stays_not_proved() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../work/pipeline-tests/navier-stokes.mpp");
    let source = std::fs::read_to_string(&path).expect("the pipeline file");
    let mut script = opening();
    script.push(events(json!([tool_use(
        "sevt_1",
        "write_steps",
        json!({"result": "Theorem 6", "steps": "1. The blow-up follows from the self-similar profile — by algebra.\n2. So the theorem holds — by Lemma 5."})
    )])));
    script.push(post_result());
    script.push(events(json!([tool_use(
        "sevt_2",
        "write_steps",
        json!({"result": "Lemma 1", "steps": "1. $1 = 1$ — by algebra."})
    )])));
    script.push(post_result());
    script.push(events(json!([
        message("sevt_3", "Theorem 6 is proved."),
        idle("sevt_4", "end_turn"),
    ])));
    let t = ScriptedTransport::new(script);
    let report = agent::run(
        &Task {
            path,
            source,
            target: "Theorem 6".into(),
            mode: Mode::Prove,
        },
        &config(),
        &store(),
        &t,
        &mut |_| {},
    )
    .expect("the run");
    assert!(
        report.tool_calls.iter().all(|c| c.is_error),
        "both writes are refused"
    );
    assert!(report.proposal.is_none());
    let t6 = report.checked.result("Theorem 6").expect("Theorem 6");
    assert!(!t6.summary.verdict.is_proved());
    assert!(
        report.verdict_text.starts_with("not proved here"),
        "{}",
        report.verdict_text
    );
}

#[test]
fn conjectures_are_tested_numerically_first() {
    let mut script = opening();
    script.push(events(json!([tool_use(
        "sevt_1",
        "write_steps",
        json!({"conjecture": "(x+1)^2 = x^2 + 1"})
    )])));
    script.push(post_result());
    script.push(events(json!([tool_use(
        "sevt_2",
        "write_steps",
        json!({"conjecture": "(x+1)^2 = x^2 + 2x + 1"})
    )])));
    script.push(post_result());
    script.push(events(json!([idle("sevt_3", "end_turn")])));
    let t = ScriptedTransport::new(script);
    let mut tk = task(GAP_DOC, "squares");
    tk.mode = Mode::Conjecture;
    let report = agent::run(&tk, &config(), &store(), &t, &mut |_| {}).expect("the run");
    assert!(report.tool_calls[0].is_error);
    assert!(
        report.tool_calls[0].output.contains("false"),
        "{}",
        report.tool_calls[0].output
    );
    assert!(
        !report.tool_calls[1].is_error,
        "{}",
        report.tool_calls[1].output
    );
    let p = report.proposal.expect("a proposal");
    assert!(
        p.proposed
            .contains("**Conjecture 2.** $(x+1)^2 = x^2 + 2x + 1$"),
        "{}",
        p.proposed
    );
    let c = report.checked.result("Conjecture 2").expect("checked");
    assert_eq!(c.summary.text, "conjecture: not proved");
}

#[test]
fn files_outside_the_folder_cannot_be_read() {
    let mut ws =
        agent::tools::Workspace::new(PathBuf::from("doc.mpp"), GAP_DOC.into(), "Theorem 1".into());
    for bad in [
        "../secret.txt",
        "C:\\Windows\\win.ini",
        "/etc/passwd",
        "notes.exe",
    ] {
        let (out, err) = ws.run("read_file", &json!({"path": bad}));
        assert!(err, "{bad} was read: {out}");
    }
}

#[test]
fn the_api_key_never_leaves_the_header() {
    let key = ApiKey::new(KEY);
    assert_eq!(format!("{key:?}"), "ApiKey([redacted])");
    assert_eq!(
        key.scrub(&format!("error near {KEY} here")),
        "error near [API key removed] here"
    );
    let mut script = opening();
    script.push(events(json!([
        message("sevt_1", &format!("my key is {KEY}")),
        idle("sevt_2", "end_turn"),
    ])));
    let t = ScriptedTransport::new(script);
    let report = agent::run(
        &task(GAP_DOC, "Theorem 1"),
        &config(),
        &store(),
        &t,
        &mut |_| {},
    )
    .expect("the run");
    let reqs = t.requests();
    assert_eq!(
        t.keyed_requests(),
        reqs.len(),
        "every request carries the key"
    );
    for r in &reqs {
        assert!(!r.url.contains(KEY));
        assert!(!r.body.clone().unwrap_or_default().contains(KEY));
        assert!(!r.headers.iter().any(|(_, v)| v.contains(KEY)));
        assert!(!format!("{r:?}").contains(KEY));
    }
    assert!(!report.agent_says.contains(KEY));
    // An error that echoes the key is scrubbed.
    let mut script = opening();
    script.truncate(1);
    script[0].status = 401;
    script[0].body =
        format!(r#"{{"error":{{"type":"authentication_error","message":"bad key {KEY}"}}}}"#);
    let t = ScriptedTransport::new(script);
    let err = agent::run(
        &task(GAP_DOC, "Theorem 1"),
        &config(),
        &store(),
        &t,
        &mut |_| {},
    )
    .expect_err("401");
    assert!(!err.to_string().contains(KEY), "{err}");
}

#[test]
fn targets_on_the_command_line() {
    assert_eq!(
        agent::parse_target("work/a.mpp#Theorem 6"),
        Some((PathBuf::from("work/a.mpp"), "Theorem 6".to_string()))
    );
    assert_eq!(
        agent::parse_target("a.mpp#lemma-2"),
        Some((PathBuf::from("a.mpp"), "Lemma 2".to_string()))
    );
    assert_eq!(agent::parse_target("a.mpp"), None);
}

/// The live test: costs money, so it runs only with `MPP_LIVE_AGENT=1` and a key in
/// Windows Credential Manager. It asks for a proof of √2's irrationality with the proof
/// removed, and checks that Navier–Stokes Theorem 6 does not come back proved.
#[test]
fn live_agent_run() {
    if std::env::var("MPP_LIVE_AGENT").ok().as_deref() != Some("1") {
        eprintln!("skipped: the live agent test costs money; set MPP_LIVE_AGENT=1 and store a key to run it");
        return;
    }
    let store = agent::CredentialStore::default();
    if !agent::available(&store) {
        eprintln!("skipped: no API key is stored in Windows Credential Manager");
        return;
    }
    let transport = agent::ReqwestTransport::new().expect("HTTP client");
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../work/pipeline-tests");
    let sqrt2 = std::fs::read_to_string(base.join("historic/sqrt2-irrational.mpp")).expect("file");
    let cut = sqrt2
        .rfind("**Proof.**")
        .map_or(sqrt2.clone(), |at| sqrt2[..at].to_string());
    let mut cfg = AgentConfig::default();
    cfg.spend_cap_usd = 3.0;
    let report = agent::run(
        &Task {
            path: base.join("historic/sqrt2-irrational.mpp"),
            source: cut,
            target: "Theorem 2".into(),
            mode: Mode::Prove,
        },
        &cfg,
        &store,
        &transport,
        &mut |m| eprintln!("cost so far: {}", m.label()),
    )
    .expect("the live run");
    let r = report.checked.result("Theorem 2").expect("Theorem 2");
    assert!(r.summary.verdict.is_proved(), "{}", r.summary.text);
    let ns = std::fs::read_to_string(base.join("navier-stokes.mpp")).expect("file");
    let report = agent::run(
        &Task {
            path: base.join("navier-stokes.mpp"),
            source: ns,
            target: "Theorem 6".into(),
            mode: Mode::Prove,
        },
        &cfg,
        &store,
        &transport,
        &mut |m| eprintln!("cost so far: {}", m.label()),
    )
    .expect("the live run");
    let t6 = report.checked.result("Theorem 6").expect("Theorem 6");
    assert!(!t6.summary.verdict.is_proved(), "{}", t6.summary.text);
}
