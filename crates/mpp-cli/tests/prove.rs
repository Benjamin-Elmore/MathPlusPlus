//! Proofs on the command line: `mpp check` shows the checker's statuses, `mpp prove`
//! refuses without a key, and a full `mpp prove` run against a scripted server shows
//! the checker's verdict and the diff and writes the file only when told to.

use std::path::PathBuf;
use std::process::Command;

use mpp_cli::{prove, split_target, ProveArgs, ProveIo, ProveOutcome};
use mpp_proof::agent::{AgentConfig, ApiKey, KeyStore, MemoryStore, Scripted, ScriptedTransport};
use serde_json::json;

fn mpp() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_mpp"));
    // Never read or write the real Windows Credential Manager entry.
    c.env("MPP_NO_CREDENTIAL_STORE", "1");
    c
}

fn example(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name)
}

fn text(out: &[u8]) -> String {
    String::from_utf8_lossy(out).to_string()
}

#[test]
fn check_shows_proof_statuses() {
    let out = mpp()
        .arg("check")
        .arg(example("proofs.mpp"))
        .output()
        .unwrap();
    let s = text(&out.stdout);
    assert!(s.contains("Lemma 1: proved"), "{s}");
    assert!(s.contains("Theorem 3: not proved: a step is wrong"), "{s}");
    assert!(s.contains("✗ wrong"), "{s}");
    assert!(s.contains("Theorem 5: 1 gap"), "{s}");
    assert!(s.contains("(x + 1)² = x² + 1"), "maths as Unicode: {s}");
    assert!(!s.contains("$"), "no raw LaTeX: {s}");
}

#[test]
fn prove_is_off_without_a_key() {
    let out = mpp()
        .arg("prove")
        .arg(format!("{}#Theorem-3", example("proofs.mpp").display()))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = text(&out.stderr);
    assert!(err.contains("set an API key first"), "{err}");
    let status = mpp().args(["key", "status"]).output().unwrap();
    assert_eq!(text(&status.stdout).trim(), "not set");
}

#[test]
fn targets_are_split_from_the_file() {
    let (p, t) = split_target("notes/proofs.mpp#theorem-2");
    assert_eq!(p, PathBuf::from("notes/proofs.mpp"));
    assert_eq!(t.as_deref(), Some("Theorem 2"));
    let (p, t) = split_target("proofs.mpp");
    assert_eq!(p, PathBuf::from("proofs.mpp"));
    assert_eq!(t, None);
}

const GAP_DOC: &str = "**Theorem 1.** For every real $m$, $(2m+1)^2 = 4m^2 + 4m + 1$.\n\n**Proof.**\n1. It holds \u{2014} *(gap: not shown yet)*.\n\u{220e}\n";

fn scripted() -> ScriptedTransport {
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
             "content": [{"type": "text", "text": "Done: one algebra step."}]},
            {"id": "sevt_4", "type": "session.status_idle", "stop_reason": {"type": "end_turn"}},
        ])),
    ])
}

fn run_prove(file: &std::path::Path, yes: bool, reply: &str) -> (ProveOutcome, String, String) {
    let keys = MemoryStore::default();
    keys.set(&ApiKey::new("test-key-123")).unwrap();
    let transport = scripted();
    let config = AgentConfig {
        poll_interval: std::time::Duration::ZERO,
        max_polls: 10,
        ..AgentConfig::default()
    };
    let args = ProveArgs {
        path: file,
        target: Some("Theorem 1".into()),
        conjecture: None,
        cap_usd: 1.5,
        yes,
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let reply = reply.to_string();
    let mut answer = move || reply.clone();
    let mut io = ProveIo {
        out: &mut out,
        err: &mut err,
        answer: &mut answer,
    };
    let outcome = prove(&args, &keys, Some(&transport), &config, &mut io);
    (outcome, text(&out), text(&err))
}

#[test]
fn prove_shows_the_checker_verdict_and_writes_only_when_told() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("gap.mpp");
    std::fs::write(&file, GAP_DOC).unwrap();

    let (outcome, out, err) = run_prove(&file, false, "n\n");
    assert_eq!(outcome, ProveOutcome::Proved, "{out}{err}");
    assert!(out.contains("Checker: Theorem 1: proved"), "{out}");
    assert!(
        out.contains("The agent says: Done: one algebra step."),
        "{out}"
    );
    assert!(out.contains("of $1.50"), "the cost against the cap: {out}");
    assert!(
        out.contains("+1. (2m+1)² = 4m² + 4m + 1"),
        "the diff: {out}"
    );
    assert!(out.contains("The file was not changed."), "{out}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), GAP_DOC);
    assert!(!out.contains("test-key-123") && !err.contains("test-key-123"));

    let (outcome, out, _) = run_prove(&file, true, "");
    assert_eq!(outcome, ProveOutcome::Proved);
    assert!(out.contains("Wrote"), "{out}");
    let now = std::fs::read_to_string(&file).unwrap();
    assert!(now.contains("by algebra") && !now.contains("gap:"), "{now}");
}

#[test]
fn lean_writes_a_project() {
    let dir = tempfile::tempdir().unwrap();
    let out_dir = dir.path().join("proofs-lean");
    let out = mpp()
        .arg("lean")
        .arg(example("proofs.mpp"))
        .arg("--out")
        .arg(&out_dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains("Lean project"));
    assert!(out_dir.join("lean-toolchain").exists());
}
