//! The app's proof commands: Prove…, Insert → Conjecture, Check against Lean…,
//! Export to Lean and Settings → Proof agent.

use std::path::Path;

use egui::Ui;
use mpp_proof::agent::Mode;
use mpp_syntax::document::parse_document;

use super::ui::ProofAction;
use crate::app::IdeApp;

impl IdeApp {
    /// The result (`Theorem 2`) at the caret, or the first one in the file.
    pub fn result_at_caret(&self) -> Option<String> {
        let d = self.doc()?;
        let doc = d.parsed();
        super::result_at(&doc, d.editor.caret())
            .or_else(|| super::result_names(&doc).into_iter().next())
    }

    /// Prove… on a result: opens the proof agent panel for it.
    pub fn prove(&mut self, target: &str) {
        self.proof.open_agent(target, Mode::Prove);
    }

    /// Insert → Conjecture: the panel in conjecture mode, about the file's topic.
    pub fn propose_conjectures(&mut self) {
        let topic = self
            .doc()
            .map(|d| {
                let doc = parse_document(d.editor.text());
                topic_of(&doc, d.editor.text()).unwrap_or_else(|| d.title())
            })
            .unwrap_or_else(|| "this file".into());
        self.proof.open_agent(&topic, Mode::Conjecture);
    }

    /// Check against Lean…: the alignment view for a result.
    pub fn align_with_lean(&mut self, target: &str) {
        self.proof.align.open = true;
        self.proof.align.target = target.to_string();
        self.proof.align.message = None;
    }

    /// Writes the current file's Lean project into `dir`.
    pub fn export_lean_to(&mut self, dir: &Path) -> Result<String, String> {
        let Some(d) = self.doc() else {
            return Err("No file is open.".into());
        };
        let export = super::ui::lean_export(d.editor.text(), d.path.as_deref(), d.proof.as_deref());
        export
            .write(dir)
            .map_err(|e| format!("Could not write the Lean project: {e}"))?;
        Ok(format!(
            "Wrote a Lean project with {} statement(s) to {}.",
            export.theorems.len(),
            crate::explorer::display_name(dir)
        ))
    }

    fn export_lean_dialog(&mut self) {
        let Some(dir) = rfd::FileDialog::new()
            .set_title("Export to Lean: choose a folder")
            .pick_folder()
        else {
            return;
        };
        let msg = match self.export_lean_to(&dir) {
            Ok(m) | Err(m) => m,
        };
        self.proof.align.message = Some(msg.clone());
        self.message = Some(msg);
    }

    /// The Proof menu.
    pub(crate) fn proof_menu(&mut self, ui: &mut Ui) {
        let target = self.result_at_caret();
        let has = target.is_some();
        if ui
            .add_enabled(has, egui::Button::new("Prove\u{2026}"))
            .clicked()
        {
            if let Some(t) = &target {
                self.prove(t);
            }
            ui.close();
        }
        if ui.button("Propose conjectures\u{2026}").clicked() {
            self.propose_conjectures();
            ui.close();
        }
        ui.separator();
        if ui
            .add_enabled(has, egui::Button::new("Check against Lean\u{2026}"))
            .clicked()
        {
            if let Some(t) = &target {
                self.align_with_lean(t);
            }
            ui.close();
        }
        if ui
            .add_enabled(has, egui::Button::new("Export to Lean\u{2026}"))
            .clicked()
        {
            self.export_lean_dialog();
            ui.close();
        }
        ui.separator();
        if ui.button("Settings \u{2192} Proof agent\u{2026}").clicked() {
            self.proof.settings.open = true;
            ui.close();
        }
    }

    /// Draws the proof windows and carries out what they ask for.
    pub(crate) fn proof_windows(&mut self, ctx: &egui::Context) {
        let theme = self.mode.theme();
        let (text, path, check) = match self.doc() {
            Some(d) => (
                Some(d.editor.text().to_string()),
                d.path.clone(),
                d.proof.clone(),
            ),
            None => (None, None, None),
        };
        let doc = text.as_deref().map(|t| (t, path.as_deref()));
        let actions = self.proof.windows(ctx, theme, doc, check.as_deref());
        for a in actions {
            match a {
                ProofAction::Accept(new) => self.accept_proof_draft(&new),
                ProofAction::ExportLean => self.export_lean_dialog(),
                ProofAction::OpenSettings => self.proof.settings.open = true,
            }
        }
    }

    /// Accept on the agent's diff: the draft becomes the file's text (one undo step).
    pub fn accept_proof_draft(&mut self, new: &str) {
        if let Some(d) = self.doc_mut() {
            let end = d.editor.text().len();
            d.editor.replace_node(mpp_syntax::Span::new(0, end), new);
            d.touched();
        }
    }
}

/// The file's topic for conjectures: its title, else its first heading.
fn topic_of(doc: &mpp_syntax::document::Document, src: &str) -> Option<String> {
    use mpp_syntax::document::{BlockKind, TitleKind};
    doc.blocks.iter().find_map(|b| match &b.kind {
        BlockKind::Title(t) if t.kind == TitleKind::Title => src
            .get(b.span.start..b.span.end)?
            .lines()
            .next()
            .map(|l| l.trim_start_matches("Title:").trim().to_string())
            .filter(|s| !s.is_empty()),
        BlockKind::Heading { content, .. } => {
            let s = src
                .get(b.span.start..b.span.end)
                .unwrap_or_default()
                .trim_start_matches('#')
                .trim()
                .to_string();
            (!content.is_empty() && !s.is_empty()).then_some(s)
        }
        _ => None,
    })
}
