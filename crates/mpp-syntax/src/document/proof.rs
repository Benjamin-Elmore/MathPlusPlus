//! The step syntax of a proof: a numbered list where each item is a claim and a reason.
//!
//! ```text
//! **Proof.**
//! 1. Suppose $p = 2m + 1$ for an integer $m$ — by assumption.
//! 2. $p^2 = 4m^2 + 4m + 1$ — by algebra.
//! 3. $q$ has a prime factor — *(cited: every integer above 1 has one [@euclid])*.
//! 4. The tail vanishes — by the divergence theorem *(gap: decay at infinity)*.
//! 5. So $p$ is even — by contradiction.
//! ∎
//! ```
//!
//! [`proof_steps`] reads a `Proof` block into [`ProofStep`]s. It only reads syntax: what
//! each step claims and which reason it gives. Whether the reason holds is decided by
//! the checker in `mpp-proof`. Nothing here panics on any input.

use super::ast::*;
use super::inline_plain_text;
use crate::span::Span;

/// A proof read as steps.
#[derive(Debug, Clone, PartialEq)]
pub struct ProofSteps {
    pub steps: Vec<ProofStep>,
    /// True when the proof body holds prose outside its numbered list (or no list at
    /// all): that text is shown but cannot be checked step by step.
    pub has_prose: bool,
}

/// One numbered step.
#[derive(Debug, Clone, PartialEq)]
pub struct ProofStep {
    /// The number written in front of the item (`3.`).
    pub number: Option<u32>,
    /// The whole list item.
    pub span: Span,
    /// The claim: the item's inlines before the reason dash (any closing `∎` removed).
    pub claim: Vec<Inline>,
    /// `Base case`, `Inductive step`, `Case 2` … when the claim starts with one.
    pub label: Option<StepLabel>,
    /// The reason after the dash, when the step gives one with `by`.
    pub reason: Option<Reason>,
    /// `*(gap: …)*`, `*(cited: …)*` and `*(external: …)*` notes anywhere in the item.
    pub notes: Vec<Note>,
    /// Steps nested under this one (the cases of a case split).
    pub substeps: Vec<ProofStep>,
}

/// A label at the start of a step's claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepLabel {
    BaseCase,
    InductiveStep,
    /// `Case 1:`, `Case $x > 0$:`; the text after the word.
    Case(String),
}

/// The reason of a step: `— by algebra`, `— by Lemma 2`, `— by steps 1–3` ….
#[derive(Debug, Clone, PartialEq)]
pub struct Reason {
    pub kind: ReasonKind,
    /// The text after `by`, as plain text (`the chain rule and (1)`).
    pub text: String,
    /// Steps, results and equations the reason points to.
    pub refs: Vec<StepRef>,
    pub span: Span,
}

/// What kind of justification a reason is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasonKind {
    /// `by assumption`, `by hypothesis`: the step introduces something assumed.
    Assumption,
    /// `by construction`: the step builds an object.
    Construction,
    /// `by definition`.
    Definition,
    /// `by contradiction`: the step closes a proof by contradiction.
    Contradiction,
    /// `by induction`, `by induction on n`.
    Induction,
    /// `by cases`: the step closes a case split.
    Cases,
    /// `by Lemma 2`, `by step 3`, `by \eqref{ns}`: only references.
    Reference,
    /// Any other rule (`by algebra`, `by the chain rule`, `by the binomial theorem
    /// and Lemma 1`): a computation the checker can try to repeat.
    Rule,
}

/// A reference inside a reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepRef {
    /// `step 3`, `steps 1 and 3`, `steps 1–4` (each number listed).
    Step(u32),
    /// `Lemma 2`, `Theorem 1`, `Corollary 3` (the kind as written, and the number).
    Result { kind: TheoremKind, number: String },
    /// `\eqref{label}`.
    Equation(String),
}

/// A note in brackets and italics.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub kind: NoteKind,
    /// The text after the colon, without the citation.
    pub text: String,
    /// The `[@key]` citations inside the note.
    pub keys: Vec<String>,
    pub span: Span,
}

/// The three notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteKind {
    /// `*(gap: …)*`: not yet justified.
    Gap,
    /// `*(cited: … [@key])*`: an established result, taken from the literature.
    Cited,
    /// `*(external: [@key])*`: a claim made elsewhere that nobody here has verified.
    External,
}

impl NoteKind {
    /// The word before the colon.
    pub fn as_str(self) -> &'static str {
        match self {
            NoteKind::Gap => "gap",
            NoteKind::Cited => "cited",
            NoteKind::External => "external",
        }
    }
}

/// Reads the steps of a proof block (`src` is the whole file, for the maths in reasons
/// and notes). A block that is not a `Proof` has no steps.
pub fn proof_steps(proof: &Theorem, src: &str) -> ProofSteps {
    let mut steps = Vec::new();
    let mut has_prose = false;
    for b in &proof.body {
        match &b.kind {
            BlockKind::List(list) if list.ordered && steps.is_empty() => {
                steps = list.items.iter().map(|i| read_item(i, src)).collect();
            }
            BlockKind::Comment(_) | BlockKind::Directive(_) => {}
            BlockKind::Paragraph(p) if is_only_qed(p) => {}
            _ => has_prose = true,
        }
    }
    ProofSteps { steps, has_prose }
}

fn is_only_qed(p: &[Inline]) -> bool {
    let mut text = String::new();
    for i in p {
        inline_plain_text(i, &mut text);
    }
    let t = text.trim();
    t.is_empty() || t == "∎" || t == "\\qed"
}

fn read_item(item: &ListItem, src: &str) -> ProofStep {
    let content = strip_qed(&item.content);
    let mut notes = Vec::new();
    collect_notes(&content, src, &mut notes);
    let (claim, after) = split_at_dash(&content);
    let reason = after.and_then(|rest| read_reason(rest, src));
    let label = step_label(&claim);
    let substeps = item
        .children
        .iter()
        .flat_map(|l| l.items.iter().map(|i| read_item(i, src)))
        .collect();
    ProofStep {
        number: item.number,
        span: item.span,
        claim,
        label,
        reason,
        notes,
        substeps,
    }
}

/// A proof body without the `∎` / `\qed` typed at its end (in a closing paragraph or
/// the last item of a closing list), for renderers that set the end mark themselves:
/// the mark is automatic, and a typed one is absorbed into it.
pub fn without_closing_qed(body: &[Block]) -> Vec<Block> {
    let mut out = body.to_vec();
    while let Some(last) = out.last_mut() {
        match &mut last.kind {
            BlockKind::Comment(_) | BlockKind::Directive(_) => {
                // Look past trailing comments for the mark.
                let n = out.len();
                if n < 2 {
                    break;
                }
                let mut head = without_closing_qed(&out[..n - 1]);
                head.extend(out[n - 1..].iter().cloned());
                return head;
            }
            BlockKind::Paragraph(p) if is_only_qed(p) => {
                out.pop();
            }
            BlockKind::Paragraph(p) => {
                *p = strip_qed(p);
                break;
            }
            BlockKind::List(list) => {
                strip_list_qed(list);
                break;
            }
            _ => break,
        }
    }
    out
}

fn strip_list_qed(list: &mut List) {
    if let Some(item) = list.items.last_mut() {
        match item.children.last_mut() {
            Some(child) => strip_list_qed(child),
            None => item.content = strip_qed(&item.content),
        }
    }
}

/// The item's inlines without a closing `∎` / `\qed` (and the line break before it).
fn strip_qed(content: &[Inline]) -> Vec<Inline> {
    let mut out: Vec<Inline> = content.to_vec();
    while let Some(last) = out.last_mut() {
        match &mut last.kind {
            InlineKind::SoftBreak => {
                out.pop();
            }
            InlineKind::Text(t) => {
                let trimmed = t.trim_end();
                let without = trimmed
                    .strip_suffix('∎')
                    .or_else(|| trimmed.strip_suffix("\\qed"));
                match without {
                    Some(rest) if rest.trim().is_empty() => {
                        out.pop();
                    }
                    Some(rest) => {
                        *t = rest.to_string();
                        break;
                    }
                    None => break,
                }
            }
            _ => break,
        }
    }
    out
}

/// Splits at the last ` — ` (or ` -- `) in the top-level text: the claim before it and
/// the inlines after it.
fn split_at_dash(content: &[Inline]) -> (Vec<Inline>, Option<Vec<Inline>>) {
    for (idx, inline) in content.iter().enumerate().rev() {
        let InlineKind::Text(t) = &inline.kind else {
            continue;
        };
        let found = t
            .rfind('—')
            .map(|p| (p, '—'.len_utf8()))
            .or_else(|| t.rfind(" -- ").map(|p| (p + 1, 2)));
        if let Some((pos, len)) = found {
            let mut claim: Vec<Inline> = content[..idx].to_vec();
            let before = &t[..pos];
            if !before.trim().is_empty() {
                claim.push(Inline {
                    kind: InlineKind::Text(before.trim_end().to_string()),
                    span: Span::new(inline.span.start, inline.span.start + before.len()),
                });
            }
            let mut rest = Vec::new();
            let after = &t[pos + len..];
            if !after.is_empty() {
                let start = (inline.span.start + pos + len).min(inline.span.end);
                rest.push(Inline {
                    kind: InlineKind::Text(after.to_string()),
                    span: Span::new(start, inline.span.end),
                });
            }
            rest.extend(content[idx + 1..].iter().cloned());
            return (claim, Some(rest));
        }
    }
    (content.to_vec(), None)
}

/// Reads `by …` from the inlines after the dash; notes are skipped.
fn read_reason(rest: Vec<Inline>, src: &str) -> Option<Reason> {
    let mut text = String::new();
    let mut refs = Vec::new();
    let mut span: Option<Span> = None;
    for i in &rest {
        if note_of(i, src).is_some() {
            continue;
        }
        span = Some(span.map_or(i.span, |s| s.join(i.span)));
        if let InlineKind::EqRef { label, .. } = &i.kind {
            refs.push(StepRef::Equation(label.clone()));
        }
        text_with_maths(i, src, &mut text);
    }
    let text = text.trim().trim_end_matches('.').trim();
    let body = text
        .strip_prefix("by ")
        .or_else(|| text.strip_prefix("By "))?
        .trim()
        .to_string();
    refs.extend(text_refs(&body));
    let kind = reason_kind(&body, &refs);
    Some(Reason {
        kind,
        text: body,
        refs,
        span: span.unwrap_or_default(),
    })
}

fn reason_kind(body: &str, refs: &[StepRef]) -> ReasonKind {
    let lower = body.to_lowercase();
    let first = lower.split_whitespace().next().unwrap_or("");
    let first = first.trim_matches(|c: char| !c.is_alphanumeric());
    match first {
        "assumption" | "hypothesis" | "supposition" => return ReasonKind::Assumption,
        "construction" => return ReasonKind::Construction,
        "definition" => return ReasonKind::Definition,
        "contradiction" => return ReasonKind::Contradiction,
        "induction" => return ReasonKind::Induction,
        "cases" => return ReasonKind::Cases,
        _ => {}
    }
    let starts_with_ref = [
        "step",
        "steps",
        "lemma",
        "theorem",
        "corollary",
        "proposition",
    ]
    .contains(&first)
        || (body.starts_with('(') && !refs.is_empty());
    if starts_with_ref && !refs.is_empty() {
        ReasonKind::Reference
    } else {
        ReasonKind::Rule
    }
}

/// `step 3`, `steps 1 and 3`, `steps 1–4`, `steps 2-7`, `Lemma 2`, `Theorem 1.2`.
fn text_refs(body: &str) -> Vec<StepRef> {
    let words: Vec<&str> = body
        .split(|c: char| c.is_whitespace() || c == ',' || c == ';' || c == '(' || c == ')')
        .filter(|w| !w.is_empty())
        .collect();
    let mut out = Vec::new();
    let mut k = 0;
    while k < words.len() {
        let w = words[k];
        let lower = w.to_lowercase();
        if lower == "step" || lower == "steps" {
            let mut j = k + 1;
            while j < words.len() {
                let item = words[j];
                if item == "and" || item == "or" {
                    j += 1;
                    continue;
                }
                match number_range(item) {
                    Some((a, b)) => {
                        for n in a..=b.min(a.saturating_add(200)) {
                            out.push(StepRef::Step(n));
                        }
                        j += 1;
                    }
                    None => break,
                }
            }
            k = j;
            continue;
        }
        let kind = TheoremKind::ALL
            .iter()
            .copied()
            .find(|kind| kind.as_str().eq_ignore_ascii_case(w));
        if let (Some(kind), Some(next)) = (kind, words.get(k + 1)) {
            let number = next.trim_end_matches('.');
            if !number.is_empty()
                && number
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '.')
                && number.chars().next().is_some_and(|c| c.is_ascii_digit())
            {
                out.push(StepRef::Result {
                    kind,
                    number: number.to_string(),
                });
                k += 2;
                continue;
            }
        }
        k += 1;
    }
    out
}

/// `3` → (3, 3); `1–4`, `1-4`, `1—4` → (1, 4).
fn number_range(w: &str) -> Option<(u32, u32)> {
    let w = w.trim_end_matches('.');
    if let Ok(n) = w.parse::<u32>() {
        return Some((n, n));
    }
    let (a, b) = w
        .split_once('–')
        .or_else(|| w.split_once('-'))
        .or_else(|| w.split_once('—'))?;
    let (a, b) = (a.parse::<u32>().ok()?, b.parse::<u32>().ok()?);
    (a <= b).then_some((a, b))
}

fn step_label(claim: &[Inline]) -> Option<StepLabel> {
    let InlineKind::Text(t) = &claim.first()?.kind else {
        return None;
    };
    let t = t.trim_start();
    let lower = t.to_lowercase();
    if lower.starts_with("base case") {
        return Some(StepLabel::BaseCase);
    }
    if lower.starts_with("inductive step") || lower.starts_with("induction step") {
        return Some(StepLabel::InductiveStep);
    }
    if let Some(rest) = t.strip_prefix("Case ") {
        let name = rest.split(':').next().unwrap_or("").trim().to_string();
        return Some(StepLabel::Case(name));
    }
    None
}

/// The note an inline is, if it is one: `*(gap: …)*` and friends.
fn note_of(i: &Inline, src: &str) -> Option<Note> {
    let InlineKind::Emphasis(inner) = &i.kind else {
        return None;
    };
    let mut text = String::new();
    let mut keys = Vec::new();
    for x in inner {
        match &x.kind {
            InlineKind::Cite(cites) => keys.extend(cites.iter().map(|c| c.key.clone())),
            _ => text_with_maths(x, src, &mut text),
        }
    }
    let t = text.trim();
    let t = t.strip_prefix('(')?;
    let t = t.strip_suffix(')').unwrap_or(t);
    let (word, rest) = t.split_once(':')?;
    let kind = match word.trim().to_lowercase().as_str() {
        "gap" => NoteKind::Gap,
        "cited" | "cite" => NoteKind::Cited,
        "external" => NoteKind::External,
        _ => return None,
    };
    Some(Note {
        kind,
        text: rest.trim().to_string(),
        keys,
        span: i.span,
    })
}

/// Plain text of an inline, with maths kept as its source between dollars.
fn text_with_maths(i: &Inline, src: &str, out: &mut String) {
    match &i.kind {
        InlineKind::Math(m) => {
            out.push('$');
            out.push_str(src.get(m.raw.start..m.raw.end).unwrap_or("…").trim());
            out.push('$');
        }
        InlineKind::Emphasis(inner) | InlineKind::Strong(inner) => {
            for x in inner {
                text_with_maths(x, src, out);
            }
        }
        _ => inline_plain_text(i, out),
    }
}

fn collect_notes(content: &[Inline], src: &str, out: &mut Vec<Note>) {
    for i in content {
        if let Some(n) = note_of(i, src) {
            out.push(n);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::parse_document;

    fn steps(src: &str) -> ProofSteps {
        let doc = parse_document(src);
        let mut found = None;
        doc.walk(&mut |b| {
            if let BlockKind::Theorem(t) = &b.kind {
                if t.kind == TheoremKind::Proof && found.is_none() {
                    found = Some(proof_steps(t, src));
                }
            }
        });
        found.unwrap_or(ProofSteps {
            steps: Vec::new(),
            has_prose: true,
        })
    }

    #[test]
    fn reasons_and_notes() {
        let s = steps(
            "**Proof.**\n\
             1. Suppose $p = 2m + 1$ — by assumption.\n\
             2. $p^2 = 4m^2 + 4m + 1$ — by algebra.\n\
             3. $q$ divides $N$ — by steps 1 and 2.\n\
             4. Then $x = 0$ — by Lemma 2 and the binomial theorem.\n\
             5. It vanishes — by the divergence theorem *(gap: decay at infinity)*.\n\
             6. It holds — *(cited: every integer has a prime factor [@euclid])*.\n\
             7. So $p$ is even — by contradiction.\n\
             ∎\n\n\
             References\n[@euclid] Euclid | Elements\n",
        );
        assert!(!s.has_prose);
        assert_eq!(s.steps.len(), 7);
        let kinds: Vec<Option<ReasonKind>> = s
            .steps
            .iter()
            .map(|st| st.reason.as_ref().map(|r| r.kind))
            .collect();
        assert_eq!(
            kinds,
            vec![
                Some(ReasonKind::Assumption),
                Some(ReasonKind::Rule),
                Some(ReasonKind::Reference),
                Some(ReasonKind::Reference),
                Some(ReasonKind::Rule),
                None,
                Some(ReasonKind::Contradiction),
            ]
        );
        assert_eq!(
            s.steps[2].reason.as_ref().map(|r| r.refs.clone()),
            Some(vec![StepRef::Step(1), StepRef::Step(2)])
        );
        assert_eq!(s.steps[4].notes[0].kind, NoteKind::Gap);
        assert_eq!(s.steps[4].notes[0].text, "decay at infinity");
        assert_eq!(s.steps[5].notes[0].kind, NoteKind::Cited);
        assert_eq!(s.steps[5].notes[0].keys, vec!["euclid".to_string()]);
        // The ∎ on the line after the last step is not part of its claim.
        let mut last = String::new();
        for i in &s.steps[6].claim {
            inline_plain_text(i, &mut last);
        }
        assert!(!last.contains('∎'));
    }

    #[test]
    fn step_ranges_and_labels() {
        let s = steps(
            "**Proof.**\n\
             1. Base case: $1 = 1$ — by algebra.\n\
             2. Inductive step: it holds — by steps 1–1.\n\
             3. So it holds — by steps 1-2 and Theorem 1.2.\n\
             ∎\n",
        );
        assert_eq!(s.steps[0].label, Some(StepLabel::BaseCase));
        assert_eq!(s.steps[1].label, Some(StepLabel::InductiveStep));
        let refs = s.steps[2].reason.as_ref().map(|r| r.refs.clone());
        assert_eq!(
            refs,
            Some(vec![
                StepRef::Step(1),
                StepRef::Step(2),
                StepRef::Result {
                    kind: TheoremKind::Theorem,
                    number: "1.2".into()
                }
            ])
        );
    }

    #[test]
    fn prose_proof_has_no_steps() {
        let s = steps("**Proof.** This is obvious. ∎\n");
        assert!(s.steps.is_empty());
        assert!(s.has_prose);
    }
}
