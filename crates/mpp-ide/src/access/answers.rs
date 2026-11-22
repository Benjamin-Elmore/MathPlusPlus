//! What a screen reader hears after a formula: its answer and its state, read
//! from what the page shows (the answer slots of `page/answers.rs`, the `= ?`
//! results and the page's problems). Nothing is worked out here: a line with
//! nothing on screen says nothing more.

use mpp_eval::LineKind;
use mpp_syntax::document::{Block, BlockKind, Document};
use mpp_syntax::math::MathAst;
use mpp_syntax::Span;

use crate::diagnostics::Level;
use crate::doc::{Doc, PageCache};
use crate::page::answers::{slots, Content};
use crate::page::{ExtraRefs, PageInput};

/// How one line's words follow its formula: a bare value after "equals", or a
/// clause after a comma.
#[derive(Debug, Clone, PartialEq)]
pub enum Said {
    /// "R equals" + " 35.31194307".
    Value(String),
    /// "R equals 35.3" + ", check fails, …".
    Clause(String),
}

impl Said {
    /// `spoken` with these words after it.
    pub fn after(&self, spoken: &str) -> String {
        match self {
            Said::Value(v) if spoken.is_empty() => v.clone(),
            Said::Value(v) => format!("{spoken} {v}"),
            Said::Clause(c) if spoken.is_empty() => c.clone(),
            Said::Clause(c) => format!("{spoken}, {c}"),
        }
    }

    /// True for an error (announced at once, not politely).
    pub fn is_error(&self) -> bool {
        matches!(self, Said::Clause(c) if c.starts_with("error"))
    }
}

/// A value as words: "≈ 1.41" becomes "approximately 1.41".
fn value_words(v: &str) -> String {
    let v = v.trim();
    let v = v.strip_prefix('=').map_or(v, str::trim_start);
    match v.strip_prefix('\u{2248}') {
        Some(rest) => format!("approximately {}", rest.trim_start()),
        None => v.to_string(),
    }
}

/// A check's reason without its mark: "✗ (left side 35.3, right side 35)"
/// becomes "left side 35.3, right side 35" (the words before it say the verdict).
fn check_words(why: &str) -> String {
    let t = why
        .trim()
        .trim_start_matches(['\u{2713}', '\u{2717}', '\u{2248}', '?'])
        .trim();
    let t = t
        .strip_prefix('(')
        .and_then(|t| t.strip_suffix(')'))
        .unwrap_or(t);
    t.trim().to_string()
}

fn slot_words(c: &Content) -> Option<Said> {
    Some(match c {
        Content::Empty => return None,
        Content::Answer { copy, .. } => Said::Value(value_words(copy)),
        Content::Preview { copy, .. } => Said::Clause(format!("equals {}", value_words(copy))),
        Content::Defined { value } => match value {
            Some((_, copy)) => Said::Clause(format!("defined, equals {}", value_words(copy))),
            None => Said::Clause("defined".into()),
        },
        Content::Check { holds, why, left } => {
            let head = match holds {
                Some(true) => "check holds",
                Some(false) => "check fails",
                None => "check not decided",
            };
            let mut why = check_words(why);
            // Say what the screen shows beside the ✗: the left side's real value.
            if let (Some(false), Some((_, value))) = (holds, left) {
                let value = value_words(value);
                if !why.contains(&value) {
                    why = if why.is_empty() {
                        format!("value {value}")
                    } else {
                        format!("{why}, value {value}")
                    };
                }
            }
            if why.is_empty() {
                Said::Clause(head.into())
            } else {
                Said::Clause(format!("{head}, {why}"))
            }
        }
        Content::Error { short, .. } => Said::Clause(format!("error: {short}")),
    })
}

/// The top-level maths block `id` of `doc`, with its formula.
fn display_block(doc: &Document, start: usize) -> Option<(&Block, &MathAst)> {
    doc.blocks.iter().find_map(|b| match &b.kind {
        BlockKind::DisplayMath(m) if b.span.start == start => Some((b, &m.ast)),
        _ => None,
    })
}

/// For each line of the maths block that starts at `start`, the words after its
/// formula (its answer, check, error or warning), or `None`.
pub fn line_words(doc: &Doc, pc: &PageCache, start: usize) -> Vec<Vec<Said>> {
    let src = doc.editor.text();
    let Some((b, ast)) = display_block(&pc.doc, start) else {
        return Vec::new();
    };
    let mut out: Vec<Vec<Said>> = vec![Vec::new(); ast.lines.len()];
    if pc.revision() != doc.editor.revision() {
        return out;
    }
    let refs = ExtraRefs::default();
    let proof = crate::proof::ProofView::default();
    let eval = doc.eval.as_deref();
    let input = PageInput {
        src,
        doc: &pc.doc,
        eval,
        theme: crate::theme::Mode::Light.theme(),
        size: 16.0,
        width: 700.0,
        integral_limits: mpp_render::IntegralLimits::AboveBelow,
        slot_labels: &[],
        show: doc.show(),
        extra_refs: &refs,
        proof: &proof,
    };
    let shown = slots(&input, b, ast);
    for s in &shown {
        if let (Some(w), Some(o)) = (slot_words(&s.content), out.get_mut(s.line)) {
            o.push(w);
        }
    }
    // `= ?` lines show their answer inside the formula (no slot).
    let fresh = eval.filter(|ev| ev.source() == src);
    if let Some(br) = fresh.and_then(|ev| ev.block(b.id)) {
        for (i, line) in ast.lines.iter().enumerate() {
            if shown.iter().any(|s| s.line == i) {
                continue;
            }
            let v = br.values.iter().rev().find(|v| {
                matches!(v.kind, LineKind::Query { .. })
                    && line.span.start <= v.span.start
                    && v.span.end <= line.span.end
            });
            if let Some(a) = v.and_then(mpp_eval::LineResult::answer) {
                out[i].push(Said::Value(value_words(&a)));
            }
        }
    }
    // Errors and warnings the page marks on the line.
    for (i, line) in ast.lines.iter().enumerate() {
        for p in pc.problems.iter().filter(|p| inside(p.span, line.span)) {
            let what = match p.level {
                Level::Error => "error",
                Level::Warning => "warning",
                _ => continue,
            };
            let said = Said::Clause(format!("{what}: {}", p.message.trim()));
            // An error the slot already says is said once.
            if what == "error" && out[i].iter().any(Said::is_error) {
                continue;
            }
            out[i].push(said);
        }
    }
    out
}

fn inside(s: Span, line: Span) -> bool {
    line.start <= s.start && s.end <= line.end
}

/// The whole spoken label of the maths block starting at `start`: each line's
/// formula followed by what the page shows for it.
pub fn block_label(doc: &Doc, pc: &PageCache, start: usize) -> Option<String> {
    let text = doc.editor.text();
    let (_, ast) = display_block(&pc.doc, start)?;
    let spoken = super::speech::speak_lines(ast, text, 0);
    let words = line_words(doc, pc, start);
    let lines: Vec<String> = spoken
        .iter()
        .enumerate()
        .map(|(i, s)| {
            words
                .get(i)
                .into_iter()
                .flatten()
                .fold(s.clone(), |acc, w| w.after(&acc))
        })
        .filter(|l| !l.is_empty())
        .collect();
    (!lines.is_empty()).then(|| lines.join(". "))
}

/// Every answer and error line of the page as it is spoken, for announcing
/// what a finished run changed.
pub fn page_lines(doc: &Doc, pc: &PageCache) -> Vec<(String, bool)> {
    let text = doc.editor.text();
    let mut out = Vec::new();
    for b in &pc.doc.blocks {
        let BlockKind::DisplayMath(m) = &b.kind else {
            continue;
        };
        let spoken = super::speech::speak_lines(&m.ast, text, 0);
        for (i, ws) in line_words(doc, pc, b.span.start).into_iter().enumerate() {
            for w in ws {
                let s = spoken.get(i).cloned().unwrap_or_default();
                out.push((w.after(&s), w.is_error()));
            }
        }
    }
    out
}

/// The page input the answer slots are worked out from.
fn page_input<'a>(
    doc: &'a Doc,
    pc: &'a PageCache,
    refs: &'a ExtraRefs,
    proof: &'a crate::proof::ProofView,
) -> PageInput<'a> {
    PageInput {
        src: doc.editor.text(),
        doc: &pc.doc,
        eval: doc.eval.as_deref(),
        theme: crate::theme::Mode::Light.theme(),
        size: 16.0,
        width: 700.0,
        integral_limits: mpp_render::IntegralLimits::AboveBelow,
        slot_labels: &[],
        show: doc.show(),
        extra_refs: refs,
        proof,
    }
}

/// The words after inline maths at `span` (`$v/2 =$`): the answer its chip
/// shows, a `= ?` answer, a check's verdict, or its error or warning.
pub fn inline_words(doc: &Doc, pc: &PageCache, span: Span) -> Vec<Said> {
    let src = doc.editor.text();
    let mut out = Vec::new();
    if pc.revision() != doc.editor.revision() {
        return out;
    }
    let refs = ExtraRefs::default();
    let proof = crate::proof::ProofView::default();
    let input = page_input(doc, pc, &refs, &proof);
    if let Some(ev) = doc.eval.as_deref().filter(|ev| ev.source() == src) {
        pc.doc.walk(&mut |b| {
            if matches!(b.kind, BlockKind::DisplayMath(_)) || !inside(span, b.span) {
                return;
            }
            let Some(br) = ev.block(b.id) else {
                return;
            };
            for v in br.values.iter().filter(|v| inside(v.span, span)) {
                let said = if v.open {
                    crate::page::answers::answer_of(v, &input, v.span)
                        .map(|(latex, copy)| Content::Answer { latex, copy })
                        .or_else(|| crate::page::answers::error_in(br, v.span))
                        .as_ref()
                        .and_then(slot_words)
                } else if matches!(v.kind, LineKind::Query { .. } | LineKind::Check) {
                    v.answer().map(|a| {
                        if v.verdict.is_none() {
                            return Said::Value(value_words(&a));
                        }
                        let head = match a.trim_start().chars().next() {
                            Some('\u{2713}') => "check holds",
                            Some('\u{2717}') => "check fails",
                            _ => "check not decided",
                        };
                        match check_words(&a) {
                            w if w.is_empty() => Said::Clause(head.into()),
                            w => Said::Clause(format!("{head}, {w}")),
                        }
                    })
                } else {
                    None
                };
                if let Some(w) = said {
                    if !out.contains(&w) {
                        out.push(w);
                    }
                }
            }
        });
    }
    for p in pc.problems.iter().filter(|p| inside(p.span, span)) {
        let what = match p.level {
            Level::Error => "error",
            Level::Warning => "warning",
            _ => continue,
        };
        if out
            .iter()
            .any(|w| matches!(w, Said::Clause(c) if c.starts_with(what)))
        {
            continue;
        }
        out.push(Said::Clause(format!("{what}: {}", p.message.trim())));
    }
    out
}

/// The spoken label of inline maths at `span`: its formula and what the page
/// shows after it.
pub fn inline_label(doc: &Doc, pc: &PageCache, span: Span) -> Option<String> {
    let spoken = super::spoken_at(doc.editor.text(), span)?;
    let words = inline_words(doc, pc, span);
    Some(words.iter().fold(spoken, |acc, w| w.after(&acc)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_and_clauses_follow_the_formula() {
        assert_eq!(
            Said::Value("35.31194307".into()).after("R equals"),
            "R equals 35.31194307"
        );
        assert_eq!(
            Said::Clause("check fails".into()).after("R equals 35.3"),
            "R equals 35.3, check fails"
        );
        assert_eq!(value_words("= 2"), "2");
        assert_eq!(value_words("\u{2248} 1.414"), "approximately 1.414");
        assert!(Said::Clause("error: no".into()).is_error());
        assert!(!Said::Clause("warning: w".into()).is_error());
        assert_eq!(
            check_words("\u{2717} (left side 35.31194307, right side 35.3)"),
            "left side 35.31194307, right side 35.3"
        );
    }
}
