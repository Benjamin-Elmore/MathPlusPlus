//! The Managed Agents session loop over raw HTTP.
//!
//! 1. An environment (created once; no network, no built-in tools are used) and an agent
//!    (created once: model, effort, system prompt, the six custom tools, every built-in
//!    tool switched off).
//! 2. A session with a hard budget (`budget.max_list_cost`, in cents).
//! 3. The task as a `user.message`, then the event list is polled: each
//!    `agent.custom_tool_use` runs locally and is answered with `user.custom_tool_result`;
//!    `span.model_request_end` and `session.usage` drive the cost counter;
//!    `session.status_idle` with `end_turn` or `budget_reached`, or
//!    `session.status_terminated`, ends the run.

use std::collections::HashSet;

use serde_json::{json, Value};

use super::cost::{CostMeter, Prices};
use super::key::ApiKey;
use super::tools::{self, Workspace};
use super::transport::{HttpRequest, Transport};
use super::{unified_diff, AgentConfig, AgentError, Proposal, RunReport, RunStop, Task};
use crate::checker::check_source;

const VERSION: &str = "2026-10-21";
const BETA: &str = "managed-agents-2026-04-01";

struct Client<'a> {
    base: String,
    key: &'a ApiKey,
    transport: &'a dyn Transport,
}

impl Client<'_> {
    fn request(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value, AgentError> {
        let req = HttpRequest {
            method: method.into(),
            url: format!("{}{}", self.base, path),
            headers: vec![
                ("content-type".into(), "application/json".into()),
                ("anthropic-version".into(), VERSION.into()),
                ("anthropic-beta".into(), BETA.into()),
            ],
            body: body.map(|b| b.to_string()),
        };
        let mut attempt = 0;
        loop {
            attempt += 1;
            let resp = self
                .transport
                .send(&req, self.key)
                .map_err(|e| AgentError::Transport(self.key.scrub(&e.0)))?;
            let retryable = resp.status == 429 || resp.status >= 500;
            if retryable && attempt < 3 {
                let wait = resp.retry_after.unwrap_or(1).min(30);
                std::thread::sleep(std::time::Duration::from_millis(wait * 200));
                continue;
            }
            if resp.status >= 400 {
                let message = serde_json::from_str::<Value>(&resp.body)
                    .ok()
                    .and_then(|v| {
                        v.pointer("/error/message")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                    .unwrap_or_else(|| resp.body.chars().take(300).collect());
                return Err(AgentError::Http {
                    status: resp.status,
                    message: self.key.scrub(&message),
                });
            }
            if resp.body.trim().is_empty() {
                return Ok(Value::Null);
            }
            return serde_json::from_str(&resp.body)
                .map_err(|e| AgentError::Protocol(format!("{e}")));
        }
    }

    fn id_of(v: &Value) -> Result<String, AgentError> {
        v.get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| AgentError::Protocol("the answer has no id".into()))
    }
}

pub(super) fn run(
    task: &Task,
    config: &AgentConfig,
    key: &ApiKey,
    transport: &dyn Transport,
    on_cost: &mut dyn FnMut(&CostMeter),
) -> Result<RunReport, AgentError> {
    let client = Client {
        base: config.base_url.trim_end_matches('/').to_string(),
        key,
        transport,
    };
    let mut meter = CostMeter::new(Prices::for_model(&config.model), config.spend_cap_usd);
    let mut ws = Workspace::new(task.path.clone(), task.source.clone(), task.target.clone());

    let environment_id = match &config.environment_id {
        Some(id) => id.clone(),
        None => Client::id_of(&client.request(
            "POST",
            "/v1/environments",
            Some(json!({
                "name": "mathpp-proof-agent",
                "config": {
                    "type": "cloud",
                    "networking": {"type": "limited", "allow_package_managers": false, "allow_mcp_servers": false}
                }
            })),
        )?)?,
    };
    let agent_id = match &config.agent_id {
        Some(id) => id.clone(),
        None => {
            let mut tool_list = vec![json!({
                "type": "agent_toolset_20260401",
                "default_config": {"enabled": false}
            })];
            tool_list.extend(tools::definitions());
            Client::id_of(&client.request(
                "POST",
                "/v1/agents",
                Some(json!({
                    "name": "Math++ proof agent",
                    "model": {"id": config.model, "effort": config.effort},
                    "system": super::system_prompt(),
                    "tools": tool_list
                })),
            )?)?
        }
    };
    let session = client.request(
        "POST",
        "/v1/sessions",
        Some(json!({
            "agent": agent_id,
            "environment_id": environment_id,
            "title": format!("Math++: {}", task.target),
            "budget": {"type": "limit", "max_list_cost": {"amount": meter.cap_cents(), "currency": "USD"}}
        })),
    )?;
    let session_id = Client::id_of(&session)?;
    let events_path = format!("/v1/sessions/{session_id}/events");
    client.request(
        "POST",
        &events_path,
        Some(json!({"events": [{"type": "user.message", "content": [{"type": "text", "text": super::task_prompt(task)}]}]})),
    )?;

    let mut seen: HashSet<String> = HashSet::new();
    let mut agent_says = String::new();
    let mut stop: Option<RunStop> = None;
    let mut polls = 0usize;
    while stop.is_none() {
        polls += 1;
        if polls > config.max_polls {
            let _ = client.request(
                "POST",
                &events_path,
                Some(json!({"events": [{"type": "user.interrupt"}]})),
            );
            stop = Some(RunStop::TooLong);
            break;
        }
        let events = list_events(&client, &events_path)?;
        for e in events {
            let id = e
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if !id.is_empty() && !seen.insert(id.clone()) {
                continue;
            }
            let kind = e.get("type").and_then(Value::as_str).unwrap_or_default();
            match kind {
                "agent.custom_tool_use" => {
                    let name = e.get("name").and_then(Value::as_str).unwrap_or_default();
                    let input = e.get("input").cloned().unwrap_or(Value::Null);
                    let (text, is_error) = ws.run(name, &input);
                    client.request(
                        "POST",
                        &events_path,
                        Some(json!({"events": [{
                            "type": "user.custom_tool_result",
                            "custom_tool_use_id": id,
                            "content": [{"type": "text", "text": text}],
                            "is_error": is_error
                        }]})),
                    )?;
                }
                "span.model_request_end" => {
                    if let Some(u) = e.get("model_usage") {
                        meter.add(
                            num(u, "input_tokens"),
                            num(u, "output_tokens"),
                            num(u, "cache_read_input_tokens"),
                            num(u, "cache_creation_input_tokens"),
                        );
                        on_cost(&meter);
                    }
                }
                "session.usage" => {
                    if let Some(c) = e.pointer("/list_cost/amount").and_then(amount) {
                        meter.reported_usd = Some(c / 100.0);
                        on_cost(&meter);
                    }
                }
                "agent.message" => {
                    if let Some(parts) = e.get("content").and_then(Value::as_array) {
                        let text: Vec<&str> = parts
                            .iter()
                            .filter_map(|p| p.get("text").and_then(Value::as_str))
                            .collect();
                        agent_says = text.join("\n");
                    }
                }
                "session.error" => {
                    let msg = e
                        .pointer("/error/message")
                        .or_else(|| e.get("message"))
                        .and_then(Value::as_str)
                        .unwrap_or("the session reported an error");
                    let refused = e
                        .pointer("/error/type")
                        .and_then(Value::as_str)
                        .is_some_and(|t| t.contains("refusal"));
                    stop = Some(if refused {
                        RunStop::Refused(msg.to_string())
                    } else {
                        RunStop::Error(msg.to_string())
                    });
                }
                "session.status_idle" => {
                    let why = e
                        .pointer("/stop_reason/type")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    match why {
                        "requires_action" => {}
                        "end_turn" => stop = stop.take().or(Some(RunStop::Finished)),
                        "budget_reached" => stop = Some(RunStop::BudgetReached),
                        "refusal" => {
                            let msg = e
                                .pointer("/stop_reason/explanation")
                                .and_then(Value::as_str)
                                .unwrap_or("the model declined this request");
                            stop = Some(RunStop::Refused(msg.to_string()));
                        }
                        other => {
                            stop = Some(RunStop::Error(format!("the session stopped: {other}")))
                        }
                    }
                }
                "session.status_terminated" => {
                    stop = stop.take().or(Some(RunStop::Finished));
                }
                _ => {}
            }
            if stop.is_some() {
                break;
            }
        }
        if stop.is_none() && config.stop.is_set() {
            let _ = client.request(
                "POST",
                &events_path,
                Some(json!({"events": [{"type": "user.interrupt"}]})),
            );
            stop = Some(RunStop::Stopped);
        }
        if stop.is_none() && meter.over_cap() {
            let _ = client.request(
                "POST",
                &events_path,
                Some(json!({"events": [{"type": "user.interrupt"}]})),
            );
            stop = Some(RunStop::SpendCap);
        }
        if stop.is_none() && !config.poll_interval.is_zero() {
            std::thread::sleep(config.poll_interval);
        }
    }

    let checked = check_source(&ws.draft);
    let verdict_text = checked
        .result(&task.target)
        .map_or_else(String::new, |r| r.summary.text.clone());
    let proposal = (ws.draft != ws.original).then(|| Proposal {
        diff: unified_diff(&ws.original, &ws.draft),
        original: ws.original.clone(),
        proposed: ws.draft.clone(),
    });
    Ok(RunReport {
        stop: stop.unwrap_or(RunStop::Finished),
        cost: meter,
        proposal,
        checked,
        verdict_text,
        agent_says: key.scrub(&agent_says),
        tool_calls: ws.calls.clone(),
        agent_id: Some(agent_id),
        environment_id: Some(environment_id),
        session_id: Some(session_id),
    })
}

/// Every event of the session so far, following `next_page`.
fn list_events(client: &Client<'_>, path: &str) -> Result<Vec<Value>, AgentError> {
    let mut out = Vec::new();
    let mut page: Option<String> = None;
    for _ in 0..100 {
        let p = match &page {
            Some(c) => format!("{path}?page={c}"),
            None => path.to_string(),
        };
        let v = client.request("GET", &p, None)?;
        let items = v
            .get("data")
            .or_else(|| v.get("events"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        out.extend(items);
        match v.get("next_page").and_then(Value::as_str) {
            Some(next) if !next.is_empty() => page = Some(next.to_string()),
            _ => break,
        }
    }
    Ok(out)
}

fn num(v: &Value, field: &str) -> u64 {
    v.get(field).and_then(Value::as_u64).unwrap_or(0)
}

/// A money amount in minor units, written as a string or a number.
fn amount(v: &Value) -> Option<f64> {
    v.as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| v.as_f64())
}
