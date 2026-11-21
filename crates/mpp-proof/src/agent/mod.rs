//! The proof agent client: "Prove…" in the IDE and `mpp prove file.mpp#name`.
//!
//! **Design.** A Claude Managed Agents session (beta): Anthropic hosts the agent loop;
//! the agent's tools are custom tools that run here, on the user's PC ([`tools`]). The
//! client talks raw HTTP (Rust has no official SDK) through a [`transport::Transport`],
//! so the loop is tested against a scripted fake server. Model `claude-opus-5-5` at high
//! effort. Managed Agents was chosen over a Messages-API tool loop because it is the
//! default for agents (no loop or context management to host here) and because its
//! session budget is a hard, server-enforced spend cap; a Messages loop can be added as
//! another backend behind the same tools if Ben prefers to host the loop.
//!
//! **Rules the client enforces, whatever the model says:**
//! - only the checker sets statuses: the run's verdict comes from checking the draft, and
//!   the agent's own summary is shown as its words, never as a status;
//! - a conjecture is tested numerically before it is written, and refused on a
//!   counterexample;
//! - external, unverified results stay cited, and gaps can only be closed by steps the
//!   checker verifies or cites ([`tools::enforce_rules`]);
//! - the file is never written: the run ends with a [`Proposal`] (a diff) the user
//!   accepts or rejects;
//! - a spend cap per run is always on (a session budget, plus a local stop), and the
//!   live cost counter is reported after every model request;
//! - the feature is off until an API key is set; the key lives in Windows Credential
//!   Manager ([`key::CredentialStore`]) and never in files, logs or this repository.

pub mod cost;
pub mod key;
mod managed;
pub mod tools;
pub mod transport;

use std::path::PathBuf;
use std::time::Duration;

pub use cost::{CostMeter, Prices};
pub use key::{ApiKey, CredentialStore, KeyStore, MemoryStore};
pub use transport::{
    HttpRequest, HttpResponse, ReqwestTransport, Scripted, ScriptedTransport, Transport,
};

use crate::model::DocumentCheck;

/// Settings for one run.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentConfig {
    pub model: String,
    /// `low` … `max`; proofs run at `high`.
    pub effort: String,
    /// The most one run may cost, in US dollars. Always on; must be above zero.
    pub spend_cap_usd: f64,
    pub base_url: String,
    /// An agent created earlier (ids are not secrets; the IDE keeps them in its
    /// settings). `None` creates one.
    pub agent_id: Option<String>,
    /// An environment created earlier. `None` creates one.
    pub environment_id: Option<String>,
    /// Time between polls of the session's events.
    pub poll_interval: Duration,
    /// Polls before the run gives up.
    pub max_polls: usize,
    /// Set from another thread (the IDE's Stop button) to interrupt the session.
    pub stop: StopFlag,
}

/// A flag another thread sets to stop a run; the session is interrupted at the next poll.
#[derive(Debug, Clone, Default)]
pub struct StopFlag(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl StopFlag {
    /// Asks the run to stop.
    pub fn set(&self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// True once [`StopFlag::set`] was called.
    pub fn is_set(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl PartialEq for StopFlag {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        AgentConfig {
            model: "claude-opus-5-5".into(),
            effort: "high".into(),
            spend_cap_usd: 2.0,
            base_url: "https://api.anthropic.com".into(),
            agent_id: None,
            environment_id: None,
            poll_interval: Duration::from_secs(2),
            max_polls: 1800,
            stop: StopFlag::default(),
        }
    }
}

/// What to do.
#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    /// The `.mpp` file (its folder bounds what the agent may read).
    pub path: PathBuf,
    pub source: String,
    /// The result to prove (`Theorem 6`), or the topic of conjectures to propose.
    pub target: String,
    pub mode: Mode,
}

/// Prove a result, or propose conjectures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Prove,
    Conjecture,
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq)]
pub enum RunStop {
    /// The agent finished its turn.
    Finished,
    /// The local spend cap was reached and the session was interrupted.
    SpendCap,
    /// The server paused the session at its budget.
    BudgetReached,
    /// The model declined (a refusal), with the server's explanation.
    Refused(String),
    /// The session ended with an error.
    Error(String),
    /// The run polled too long without the session finishing.
    TooLong,
    /// The user pressed Stop and the session was interrupted.
    Stopped,
}

/// The proposed change, shown as a diff the user accepts or rejects.
#[derive(Debug, Clone, PartialEq)]
pub struct Proposal {
    pub original: String,
    pub proposed: String,
    /// A unified diff of the two.
    pub diff: String,
}

impl Proposal {
    /// The new file text, for the IDE to write once the user accepts.
    pub fn accept(&self) -> &str {
        &self.proposed
    }

    /// True when the draft differs from the original.
    pub fn changes_anything(&self) -> bool {
        self.original != self.proposed
    }
}

/// The result of a run.
#[derive(Debug, Clone)]
pub struct RunReport {
    pub stop: RunStop,
    pub cost: CostMeter,
    /// The draft as a diff, when the agent changed anything.
    pub proposal: Option<Proposal>,
    /// The checker's report on the draft: the only source of statuses.
    pub checked: DocumentCheck,
    /// The target's summary on the draft, from the checker.
    pub verdict_text: String,
    /// The agent's own closing words, untrusted (shown as "the agent says").
    pub agent_says: String,
    pub tool_calls: Vec<tools::ToolCall>,
    pub agent_id: Option<String>,
    pub environment_id: Option<String>,
    pub session_id: Option<String>,
}

/// Why a run could not start or failed outright.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentError {
    /// No API key is set: the feature is off.
    NoKey,
    /// The spend cap is not a positive amount.
    NoSpendCap,
    /// The key store could not be read.
    KeyStore(String),
    /// The server answered with an error status.
    Http { status: u16, message: String },
    /// No answer at all.
    Transport(String),
    /// The server's answer could not be read.
    Protocol(String),
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentError::NoKey => f.write_str("the proof agent is off: set an API key first"),
            AgentError::NoSpendCap => f.write_str("set a spend cap above $0 for the run"),
            AgentError::KeyStore(e) => write!(f, "the API key could not be read: {e}"),
            AgentError::Http { status, message } => {
                write!(f, "the API answered {status}: {message}")
            }
            AgentError::Transport(e) => write!(f, "the API could not be reached: {e}"),
            AgentError::Protocol(e) => write!(f, "the API's answer was not understood: {e}"),
        }
    }
}

impl std::error::Error for AgentError {}

/// True when a key is set (the "Prove…" command is enabled only then).
pub fn available(store: &dyn KeyStore) -> bool {
    matches!(store.get(), Ok(Some(_)))
}

/// Splits `file.mpp#Theorem 6` into the path and the result's name.
pub fn parse_target(arg: &str) -> Option<(PathBuf, String)> {
    let (path, name) = arg.rsplit_once('#')?;
    let name = name.trim().replace(['-', '_'], " ");
    let mut chars = name.chars();
    let first = chars.next()?;
    let name: String = first.to_uppercase().chain(chars).collect();
    (!path.is_empty()).then(|| (PathBuf::from(path), name))
}

/// Runs the agent on `task`. The key comes from `store`; the run stops at the spend cap;
/// `on_cost` is called with the live counter after every model request.
pub fn run(
    task: &Task,
    config: &AgentConfig,
    store: &dyn KeyStore,
    transport: &dyn Transport,
    on_cost: &mut dyn FnMut(&CostMeter),
) -> Result<RunReport, AgentError> {
    let key = store
        .get()
        .map_err(AgentError::KeyStore)?
        .ok_or(AgentError::NoKey)?;
    if !(config.spend_cap_usd.is_finite() && config.spend_cap_usd > 0.0) {
        return Err(AgentError::NoSpendCap);
    }
    managed::run(task, config, &key, transport, on_cost)
}

/// The instructions the agent works under.
pub fn system_prompt() -> &'static str {
    "You write mathematical proofs in Math++ (.mpp), a plain-text maths language whose maths is LaTeX. \
A proof is a numbered list of steps; each step is a claim followed by ` — by <reason>.` \
Reasons: `by algebra` (or another rule such as `by the chain rule`), `by assumption`, `by construction`, \
`by definition`, `by contradiction`, `by induction` (with steps labelled `Base case:` and `Inductive step:`), \
`by cases`, `by Lemma 2`, `by steps 1–3`. Notes: `*(cited: <result> [@key])*` for an established result in the \
file's references, `*(external: [@key])*` for a claim nobody has verified, `*(gap: <what is missing>)*` for anything \
not yet justified.\n\
Rules:\n\
1. Only the Math++ checker decides whether a step holds. Use check_step to see its status; never write status marks \
or claim that something is proved.\n\
2. Write claims as relations in maths ($a = b$, $x \\le y$, $a \\equiv b \\pmod n$) so the checker can test them. \
Prefer small steps the CAS can verify.\n\
3. Before proposing a conjecture, test it numerically with evaluate; write it with write_steps and `conjecture`.\n\
4. Report gaps honestly: if you cannot justify a step, mark it *(gap: …)*. Never hide a gap behind prose.\n\
5. Cite instead of claiming: keep every external result cited as external.\n\
6. Your changes are a draft the user reviews as a diff; write only through write_steps.\n\
When you are done, say in two or three sentences what you changed and which steps remain gaps."
}

/// The first message of a run.
pub fn task_prompt(task: &Task) -> String {
    let what = match task.mode {
        Mode::Prove => format!(
            "Prove {} in the document below, or close as many of its gaps as you honestly can. \
Write the proof with write_steps (result: \"{}\") and check every step with check_step.",
            task.target, task.target
        ),
        Mode::Conjecture => format!(
            "Propose conjectures about {} suggested by the document below. Test each numerically with evaluate first, \
then add it with write_steps (conjecture). Try to prove the ones that hold.",
            task.target
        ),
    };
    format!(
        "{what}\n\nThe document ({}):\n\n{}",
        file_name(&task.path),
        task.source
    )
}

fn file_name(p: &std::path::Path) -> String {
    p.file_name().map_or_else(
        || "document.mpp".to_string(),
        |n| n.to_string_lossy().to_string(),
    )
}

/// A unified diff of two texts, by lines.
pub fn unified_diff(a: &str, b: &str) -> String {
    let x: Vec<&str> = a.lines().collect();
    let y: Vec<&str> = b.lines().collect();
    // Longest common subsequence, by dynamic programming (documents are small).
    let (n, m) = (x.len(), y.len());
    if n.saturating_mul(m) > 25_000_000 {
        return format!(
            "--- original\n+++ draft\n(the files are too large to compare: {n} and {m} lines)\n"
        );
    }
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if x[i] == y[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut out = String::from("--- original\n+++ draft\n");
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && x[i] == y[j] {
            i += 1;
            j += 1;
        } else if j < m && (i == n || lcs[i][j + 1] >= lcs[i + 1][j]) {
            out.push_str(&format!("+{}\n", y[j]));
            j += 1;
        } else if i < n {
            out.push_str(&format!("-{}\n", x[i]));
            i += 1;
        }
    }
    out
}
