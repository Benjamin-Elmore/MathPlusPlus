//! The proof windows: the proof agent panel, Settings → Proof agent (the API key),
//! and the Lean alignment view.
//!
//! The agent is off until a key is stored; the key lives in Windows Credential
//! Manager and is never shown again once saved. A run always has a spend cap and a
//! live cost counter, and ends with a diff the user accepts or rejects: the file is
//! only changed on Accept. Text is typeset wherever it can be; raw Lean appears only
//! inside the alignment view, which the user opens on purpose.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

use egui::{Context, RichText, Ui};
use mpp_proof::agent::{
    self, AgentConfig, AgentError, ApiKey, CostMeter, KeyStore, MemoryStore, Mode, Prices,
    RunReport, RunStop, StopFlag, Task, Transport,
};
use mpp_proof::lean::{self, LeanConfig, LeanExport};
use mpp_render::Theme;

use super::{prose_latex, Tone};
use crate::theme::sans;
use crate::typeset;

/// Where the API key is kept (the real window uses Windows Credential Manager;
/// tests and new apps start with an empty store in memory).
pub type Keys = Arc<dyn KeyStore + Send + Sync>;

/// The message the agent panel shows while no key is stored.
pub const NO_KEY: &str =
    "The proof agent is off until an API key is stored. Open Settings \u{2192} Proof agent to add one.";

/// Typesets a sentence of words (and `$…$` maths) and labels it for screen readers.
fn typeset_words(ui: &mut Ui, text: &str, size: f32, theme: Theme) -> egui::Response {
    let latex = prose_latex(text);
    let resp = typeset::show(ui, &latex, size, theme);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
    resp
}

/// Wraps words into lines of about `width` characters and typesets each.
fn typeset_paragraph(ui: &mut Ui, text: &str, size: f32, theme: Theme, width: usize) {
    let mut line = String::new();
    let mut lines = vec![];
    for w in text.split_whitespace() {
        // Keep maths spans whole: a word inside `$…$` stays with its line.
        let in_math = line.matches('$').count() % 2 == 1;
        if !in_math && line.chars().count() + w.chars().count() > width && !line.is_empty() {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(w);
        // A sentence that ends past half the width ends its line.
        let in_math = line.matches('$').count() % 2 == 1;
        if !in_math && w.ends_with('.') && line.chars().count() > width / 2 {
            lines.push(std::mem::take(&mut line));
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    for l in lines {
        typeset_words(ui, &l, size, theme);
    }
}

fn tinted(theme: Theme, c: mpp_render::Color) -> Theme {
    Theme { text: c, ..theme }
}

enum Msg {
    Cost(CostMeter),
    Done(Box<Result<RunReport, AgentError>>),
}

struct Running {
    rx: Receiver<Msg>,
    stop: StopFlag,
}

/// The proof agent panel.
pub struct AgentPanel {
    pub open: bool,
    pub mode: Mode,
    /// The result to prove (`Theorem 2`), or the topic of conjectures.
    pub target: String,
    /// The most this run may cost, in dollars.
    pub cap: f64,
    /// The live counter ("$0.42 of $2.00").
    pub cost: String,
    running: Option<Running>,
    pub report: Option<RunReport>,
    pub error: Option<String>,
}

impl Default for AgentPanel {
    fn default() -> Self {
        AgentPanel {
            open: false,
            mode: Mode::Prove,
            target: String::new(),
            cap: 2.0,
            cost: CostMeter::new(Prices::for_model("claude-opus-5-5"), 2.0).label(),
            running: None,
            report: None,
            error: None,
        }
    }
}

impl AgentPanel {
    /// True while a run is going.
    pub fn running(&self) -> bool {
        self.running.is_some()
    }
}

/// Settings → Proof agent.
#[derive(Default)]
pub struct KeySettings {
    pub open: bool,
    /// What is typed in the password box (cleared once saved).
    pub entry: String,
    pub message: Option<String>,
    /// The agent and environment created by the first run (not secrets).
    pub agent_id: Option<String>,
    pub environment_id: Option<String>,
}

/// The Lean alignment view.
#[derive(Default)]
pub struct AlignView {
    pub open: bool,
    /// The result shown.
    pub target: String,
    /// Statements the user confirmed say the same thing (this session only).
    pub confirmed: BTreeSet<String>,
    pub message: Option<String>,
}

/// What a proof window asks the app to do.
#[derive(Debug, Clone, PartialEq)]
pub enum ProofAction {
    /// Replace the file's text with the accepted draft.
    Accept(String),
    /// Export the current file to Lean (pick a folder).
    ExportLean,
    /// Open Settings → Proof agent.
    OpenSettings,
}

/// All proof windows and what they share.
pub struct ProofUi {
    pub keys: Keys,
    /// The HTTP transport (tests use a scripted one; `None` is the real API).
    pub transport: Option<Arc<dyn Transport>>,
    /// Settings every run starts from.
    pub config: AgentConfig,
    pub agent: AgentPanel,
    pub settings: KeySettings,
    pub align: AlignView,
    /// Whether a key is stored, read once and after every change (the store is
    /// not read on every frame).
    key_set: Option<bool>,
}

impl Default for ProofUi {
    fn default() -> Self {
        ProofUi {
            keys: Arc::new(MemoryStore::default()),
            transport: None,
            config: AgentConfig::default(),
            agent: AgentPanel::default(),
            settings: KeySettings::default(),
            align: AlignView::default(),
            key_set: None,
        }
    }
}

impl ProofUi {
    /// Uses `keys` from now on (the real window passes Windows Credential Manager).
    pub fn set_key_store(&mut self, keys: Keys) {
        self.keys = keys;
        self.key_set = None;
    }

    /// True when an API key is stored: the agent can run.
    pub fn available(&mut self) -> bool {
        if self.key_set.is_none() {
            self.key_set = Some(agent::available(self.keys.as_ref()));
        }
        self.key_set == Some(true)
    }

    /// Stores a key (never shown again).
    pub fn store_key(&mut self, key: &str) -> Result<(), String> {
        let key = key.trim();
        if key.is_empty() {
            return Err("Type or paste a key first.".into());
        }
        let r = self.keys.set(&ApiKey::new(key));
        self.key_set = None;
        r
    }

    /// Removes the stored key: the agent is off again.
    pub fn remove_key(&mut self) -> Result<(), String> {
        let r = self.keys.delete();
        self.key_set = None;
        r
    }

    /// Opens the agent panel for a result, or for conjectures.
    pub fn open_agent(&mut self, target: &str, mode: Mode) {
        self.agent.open = true;
        self.agent.mode = mode;
        self.agent.target = target.to_string();
        self.agent.error = None;
        if !self.agent.running() {
            self.agent.report = None;
            self.agent.cost =
                CostMeter::new(Prices::for_model(&self.config.model), self.agent.cap).label();
        }
        self.key_set = None;
    }

    /// Starts a run on `source` (the file at `path`). Does nothing without a key.
    pub fn start(&mut self, path: &Path, source: &str) {
        if self.agent.running() || !self.available() {
            return;
        }
        let task = Task {
            path: path.to_path_buf(),
            source: source.to_string(),
            target: self.agent.target.clone(),
            mode: self.agent.mode,
        };
        let stop = StopFlag::default();
        let config = AgentConfig {
            spend_cap_usd: self.agent.cap,
            agent_id: self.settings.agent_id.clone(),
            environment_id: self.settings.environment_id.clone(),
            stop: stop.clone(),
            ..self.config.clone()
        };
        self.agent.cost =
            CostMeter::new(Prices::for_model(&config.model), config.spend_cap_usd).label();
        self.agent.report = None;
        self.agent.error = None;
        let (tx, rx) = channel();
        let keys = self.keys.clone();
        let transport = self.transport.clone();
        let spawned = std::thread::Builder::new()
            .name("mpp-proof-agent".into())
            .spawn(move || {
                let transport: Arc<dyn Transport> = match transport {
                    Some(t) => t,
                    None => match agent::ReqwestTransport::new() {
                        Ok(t) => Arc::new(t),
                        Err(e) => {
                            let _ = tx.send(Msg::Done(Box::new(Err(AgentError::Transport(e.0)))));
                            return;
                        }
                    },
                };
                let cost_tx = tx.clone();
                let r = agent::run(
                    &task,
                    &config,
                    keys.as_ref(),
                    transport.as_ref(),
                    &mut |m| {
                        let _ = cost_tx.send(Msg::Cost(m.clone()));
                    },
                );
                let _ = tx.send(Msg::Done(Box::new(r)));
            });
        match spawned {
            Ok(_) => self.agent.running = Some(Running { rx, stop }),
            Err(e) => self.agent.error = Some(format!("The agent could not start: {e}")),
        }
    }

    /// Asks the running agent to stop (the session is interrupted).
    pub fn stop(&mut self) {
        if let Some(r) = &self.agent.running {
            r.stop.set();
        }
    }

    /// Picks up cost updates and the end of a run. True while a run is going.
    pub fn poll(&mut self) -> bool {
        let Some(run) = &self.agent.running else {
            return false;
        };
        let mut done = None;
        while let Ok(m) = run.rx.try_recv() {
            match m {
                Msg::Cost(c) => self.agent.cost = c.label(),
                Msg::Done(r) => done = Some(*r),
            }
        }
        match done {
            Some(Ok(report)) => {
                self.agent.cost = report.cost.label();
                if report.agent_id.is_some() {
                    self.settings.agent_id = report.agent_id.clone();
                }
                if report.environment_id.is_some() {
                    self.settings.environment_id = report.environment_id.clone();
                }
                self.agent.report = Some(report);
                self.agent.running = None;
                false
            }
            Some(Err(e)) => {
                self.agent.error = Some(e.to_string());
                self.agent.running = None;
                false
            }
            None => true,
        }
    }

    /// Waits (up to `timeout`) for the running agent to finish (tests).
    pub fn wait(&mut self, timeout: std::time::Duration) {
        let end = std::time::Instant::now() + timeout;
        while self.poll() && std::time::Instant::now() < end {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// Draws every open proof window. `doc` is the current file's text and path.
    pub fn windows(
        &mut self,
        ctx: &Context,
        theme: Theme,
        doc: Option<(&str, Option<&Path>)>,
        check: Option<&mpp_proof::DocumentCheck>,
    ) -> Vec<ProofAction> {
        let mut actions = vec![];
        if self.poll() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        self.agent_window(ctx, theme, doc, &mut actions);
        self.settings_window(ctx, theme);
        self.align_window(ctx, theme, doc, check, &mut actions);
        actions
    }

    fn agent_window(
        &mut self,
        ctx: &Context,
        theme: Theme,
        doc: Option<(&str, Option<&Path>)>,
        actions: &mut Vec<ProofAction>,
    ) {
        if !self.agent.open {
            return;
        }
        let available = self.available();
        let mut open = true;
        let mut decided = None;
        let title = match self.agent.mode {
            Mode::Prove => "Proof agent",
            Mode::Conjecture => "Proof agent: conjectures",
        };
        egui::Window::new(title)
            .id(egui::Id::unique("proof-agent"))
            .open(&mut open)
            .collapsible(false)
            .default_width(560.0)
            .default_pos([360.0, 120.0])
            .show(ctx, |ui| {
                let what = match self.agent.mode {
                    Mode::Prove => format!("Prove {}", self.agent.target),
                    Mode::Conjecture => format!("Propose conjectures about {}", self.agent.target),
                };
                typeset_words(ui, &what, 18.0, theme);
                ui.add_space(6.0);
                if !available {
                    typeset_paragraph(ui, NO_KEY, 16.0, tinted(theme, theme.muted), 64);
                    ui.add_space(4.0);
                    if ui.button("Open Settings \u{2192} Proof agent").clicked() {
                        actions.push(ProofAction::OpenSettings);
                    }
                    ui.add_space(6.0);
                }
                ui.add_enabled_ui(available, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Spend cap").font(sans(14.0)));
                        ui.add_enabled(
                            !self.agent.running(),
                            egui::DragValue::new(&mut self.agent.cap)
                                .range(0.01..=100.0)
                                .speed(0.05)
                                .prefix("$")
                                .max_decimals(2),
                        );
                        ui.separator();
                        ui.label(RichText::new(&self.agent.cost).font(sans(14.0)).strong())
                            .on_hover_text("What this run has cost so far, of its cap");
                    });
                    ui.horizontal(|ui| {
                        let can_start = !self.agent.running() && doc.is_some();
                        if ui
                            .add_enabled(can_start, egui::Button::new("Start"))
                            .clicked()
                        {
                            if let Some((text, path)) = doc {
                                let path = path
                                    .map(Path::to_path_buf)
                                    .unwrap_or_else(|| PathBuf::from("Untitled.mpp"));
                                self.start(&path, text);
                            }
                        }
                        if ui
                            .add_enabled(self.agent.running(), egui::Button::new("Stop"))
                            .clicked()
                        {
                            self.stop();
                        }
                        if self.agent.running() {
                            ui.spinner();
                        }
                    });
                });
                if let Some(e) = &self.agent.error {
                    ui.add_space(4.0);
                    typeset_words(ui, e, 15.0, tinted(theme, theme.error));
                }
                if let Some(report) = &self.agent.report {
                    decided = report_ui(ui, report, theme);
                }
            });
        match decided {
            Some(Decision::Accept(text)) => {
                actions.push(ProofAction::Accept(text));
                self.agent.report = None;
            }
            Some(Decision::Reject) => self.agent.report = None,
            None => {}
        }
        if !open {
            self.stop();
            self.agent.open = false;
        }
    }

    fn settings_window(&mut self, ctx: &Context, theme: Theme) {
        if !self.settings.open {
            return;
        }
        let mut open = true;
        let available = self.available();
        egui::Window::new("Settings \u{2014} Proof agent")
            .id(egui::Id::unique("proof-settings"))
            .open(&mut open)
            .collapsible(false)
            .default_width(460.0)
            .default_pos([400.0, 160.0])
            .show(ctx, |ui| {
                let status = if available {
                    "An API key is stored in Windows Credential Manager."
                } else {
                    "No API key is stored: the proof agent is off."
                };
                typeset_words(ui, status, 16.0, theme);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("API key").font(sans(14.0)));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.settings.entry)
                            .password(true)
                            .hint_text("paste a key")
                            .desired_width(260.0),
                    );
                    if ui.button("Save key").clicked() {
                        let entry = std::mem::take(&mut self.settings.entry);
                        self.settings.message = Some(match self.store_key(&entry) {
                            Ok(()) => "Saved. The key is not shown again.".into(),
                            Err(e) => e,
                        });
                    }
                });
                if available && ui.button("Remove key").clicked() {
                    self.settings.message = Some(match self.remove_key() {
                        Ok(()) => "The key was removed.".into(),
                        Err(e) => e,
                    });
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Default spend cap").font(sans(14.0)));
                    ui.add(
                        egui::DragValue::new(&mut self.agent.cap)
                            .range(0.01..=100.0)
                            .speed(0.05)
                            .prefix("$")
                            .max_decimals(2),
                    );
                });
                ui.separator();
                let ids = [
                    ("Agent", self.settings.agent_id.clone()),
                    ("Environment", self.settings.environment_id.clone()),
                ];
                for (name, id) in ids {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(name).font(sans(14.0)));
                        ui.weak(id.unwrap_or_else(|| "made by the first run".into()));
                    });
                }
                if ui.button("Reset agent and environment").clicked() {
                    self.settings.agent_id = None;
                    self.settings.environment_id = None;
                }
                if let Some(m) = &self.settings.message {
                    ui.separator();
                    typeset_words(ui, m, 15.0, tinted(theme, theme.muted));
                }
            });
        if !open {
            self.settings.open = false;
            self.settings.entry.clear();
        }
    }

    fn align_window(
        &mut self,
        ctx: &Context,
        theme: Theme,
        doc: Option<(&str, Option<&Path>)>,
        check: Option<&mpp_proof::DocumentCheck>,
        actions: &mut Vec<ProofAction>,
    ) {
        if !self.align.open {
            return;
        }
        let Some((src, path)) = doc else {
            self.align.open = false;
            return;
        };
        let export = lean_export(src, path, check);
        let names: Vec<String> = export.theorems.iter().map(|t| t.name.clone()).collect();
        if !names.contains(&self.align.target) {
            if let Some(n) = names.first() {
                self.align.target = n.clone();
            }
        }
        let mut open = true;
        egui::Window::new("Check against Lean")
            .id(egui::Id::unique("lean-align"))
            .open(&mut open)
            .collapsible(false)
            .default_width(760.0)
            .default_pos([260.0, 100.0])
            .show(ctx, |ui| {
                if names.is_empty() {
                    typeset_words(
                        ui,
                        "This file has no theorems to state in Lean.",
                        16.0,
                        theme,
                    );
                    return;
                }
                egui::ComboBox::from_id_salt("lean-align-target")
                    .selected_text(&self.align.target)
                    .show_ui(ui, |ui| {
                        for n in &names {
                            ui.selectable_value(&mut self.align.target, n.clone(), n);
                        }
                    });
                let Some(t) = export.theorem(&self.align.target) else {
                    return;
                };
                ui.add_space(6.0);
                ui.columns(2, |cols| {
                    cols[0].label(RichText::new("Math++").font(sans(14.0)).strong());
                    typeset_paragraph(&mut cols[0], &t.alignment.mpp, 16.0, theme, 44);
                    cols[1].label(RichText::new("Lean").font(sans(14.0)).strong());
                    match &t.alignment.lean_latex {
                        Some(l) => {
                            let r = typeset::show(&mut cols[1], l, 16.0, theme);
                            let raw = t.alignment.lean.clone().unwrap_or_default();
                            r.widget_info(|| {
                                egui::WidgetInfo::labeled(egui::Role::Label, true, &raw)
                            });
                        }
                        None => {
                            typeset_words(
                                &mut cols[1],
                                "Not stated in Lean yet.",
                                16.0,
                                tinted(theme, theme.muted),
                            );
                        }
                    }
                });
                if let Some(raw) = &t.alignment.lean {
                    ui.add_space(6.0);
                    ui.weak("The Lean statement as exported:");
                    let mut shown: &str = raw;
                    ui.add(
                        egui::TextEdit::multiline(&mut shown)
                            .font(egui::TextStyle::Monospace)
                            .desired_rows(2)
                            .desired_width(f32::INFINITY),
                    );
                }
                for w in &t.alignment.warnings {
                    typeset_words(ui, w, 14.0, tinted(theme, theme.warning));
                }
                ui.add_space(6.0);
                let mut same = self.align.confirmed.contains(&t.name);
                let can = t.alignment.lean.is_some();
                if ui
                    .add_enabled(
                        can,
                        egui::Checkbox::new(&mut same, "They say the same thing"),
                    )
                    .changed()
                {
                    if same {
                        self.align.confirmed.insert(t.name.clone());
                    } else {
                        self.align.confirmed.remove(&t.name);
                    }
                }
                ui.horizontal(|ui| {
                    if ui.button("Export to Lean\u{2026}").clicked() {
                        actions.push(ProofAction::ExportLean);
                    }
                    if lean::find_lake().is_none() {
                        ui.weak("Lean is not installed, so nothing is built here.");
                    }
                });
                if let Some(m) = &self.align.message {
                    typeset_words(ui, m, 14.0, tinted(theme, theme.muted));
                }
            });
        if !open {
            self.align.open = false;
        }
    }
}

/// What the user chose for a proposed change.
enum Decision {
    Accept(String),
    Reject,
}

/// The checker's verdict on the draft, the agent's own words, and the diff with
/// Accept and Reject.
fn report_ui(ui: &mut Ui, report: &RunReport, theme: Theme) -> Option<Decision> {
    ui.separator();
    let how = match &report.stop {
        RunStop::Finished => "The agent finished.".to_string(),
        RunStop::SpendCap => "Stopped at the spend cap.".to_string(),
        RunStop::BudgetReached => "The session reached its budget.".to_string(),
        RunStop::Refused(why) => format!("The model declined: {why}"),
        RunStop::Error(e) => format!("The session ended with an error: {e}"),
        RunStop::TooLong => "The run took too long and was stopped.".to_string(),
        RunStop::Stopped => "Stopped.".to_string(),
    };
    typeset_words(ui, &how, 15.0, theme);
    if !report.verdict_text.is_empty() {
        let tone = report
            .checked
            .results
            .iter()
            .find(|r| r.summary.text == report.verdict_text)
            .map_or(Tone::Weak, |r| Tone::of(&r.summary.verdict));
        typeset_words(
            ui,
            &format!("Checker: {}", report.verdict_text),
            16.0,
            tinted(theme, tone.color(&theme)),
        );
    }
    if !report.agent_says.trim().is_empty() {
        ui.add_space(4.0);
        typeset_paragraph(
            ui,
            &format!("The agent says: {}", report.agent_says.trim()),
            15.0,
            tinted(theme, theme.auto_result),
            70,
        );
    }
    if !report.tool_calls.is_empty() {
        ui.collapsing(format!("Tool calls ({})", report.tool_calls.len()), |ui| {
            for c in &report.tool_calls {
                ui.label(RichText::new(&c.name).font(sans(13.0)));
            }
        });
    }
    let Some(p) = report.proposal.as_ref().filter(|p| p.changes_anything()) else {
        typeset_words(ui, "No change to the file was proposed.", 15.0, theme);
        return None;
    };
    ui.add_space(6.0);
    ui.label(RichText::new("Proposed change").font(sans(14.0)).strong());
    egui::ScrollArea::vertical()
        .max_height(260.0)
        .id_salt("proof-diff")
        .show(ui, |ui| {
            for (mark, color, rest) in diff_lines(&p.diff, &theme) {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(mark)
                            .font(sans(15.0))
                            .color(crate::paint::c32(color)),
                    );
                    if rest.trim().is_empty() {
                        ui.weak("(blank line)");
                    } else {
                        typeset_words(ui, rest, 15.0, tinted(theme, color));
                    }
                });
            }
        });
    let mut out = None;
    ui.horizontal(|ui| {
        if ui.button("Accept").clicked() {
            out = Some(Decision::Accept(p.accept().to_string()));
        }
        if ui.button("Reject").clicked() {
            out = Some(Decision::Reject);
        }
    });
    out
}

/// The changed lines of a unified diff, removals before the additions that
/// replace them: (sign, colour, text).
fn diff_lines<'a>(diff: &'a str, theme: &Theme) -> Vec<(&'static str, mpp_render::Color, &'a str)> {
    let mut out = vec![];
    let (mut minus, mut plus): (Vec<&str>, Vec<&str>) = (vec![], vec![]);
    let flush = |out: &mut Vec<_>, minus: &mut Vec<&'a str>, plus: &mut Vec<&'a str>| {
        out.extend(minus.drain(..).map(|r| ("\u{2212}", theme.error, r)));
        out.extend(plus.drain(..).map(|r| ("+", theme.series[2], r)));
    };
    for line in diff.lines().skip(2) {
        if let Some(r) = line.strip_prefix('+') {
            plus.push(r);
        } else if let Some(r) = line.strip_prefix('-') {
            minus.push(r);
        } else {
            flush(&mut out, &mut minus, &mut plus);
        }
    }
    flush(&mut out, &mut minus, &mut plus);
    out
}

/// The Lean export of a file (the statements, alignment data and proof skeleton).
pub fn lean_export(
    src: &str,
    path: Option<&Path>,
    check: Option<&mpp_proof::DocumentCheck>,
) -> LeanExport {
    let doc = mpp_syntax::document::parse_document(src);
    let owned;
    let check = match check {
        Some(c) => c,
        None => {
            owned = super::check_text(src, path.and_then(Path::parent));
            &owned
        }
    };
    let module = path.and_then(Path::file_stem).map_or_else(
        || "document".to_string(),
        |s| s.to_string_lossy().to_string(),
    );
    lean::export(&doc, src, check, &module, &LeanConfig::default())
}
