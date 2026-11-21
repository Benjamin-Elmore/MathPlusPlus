//! What the checker reports: a status per step, a summary per theorem.
//!
//! Everything here is preview data. The IDE draws it in the equation view (marks next to
//! steps, squiggles, a summary line under the theorem); none of it is ever written into
//! the `.mpp` file.

use mpp_syntax::document::TheoremKind;
use mpp_syntax::Span;

/// The status of one proof step, from strongest to weakest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StepStatus {
    /// ✓ The CAS proved every claim in the step symbolically.
    Symbolic,
    /// ⊢ Lean compiled the step (only after a `lake build`).
    Lean,
    /// ≈ Every claim agrees at random points (C6), or on a random concrete instance.
    Numeric,
    /// ◇ A logical step whose form is valid (assumption, construction, contradiction,
    /// induction, a reference, or prose with nothing to compute).
    Structure,
    /// 📖 Taken from a cited, established result.
    Cited,
    /// ⚠ Not justified here: a written gap, a claim the checker could not verify, a
    /// missing reason, a deleted citation, a circular reference.
    Gap,
    /// ⚠ Rests on a claim made elsewhere that nobody here has verified.
    External,
    /// ✗ Wrong: the checker found a counterexample.
    Wrong,
}

impl StepStatus {
    /// The mark drawn next to the step.
    pub fn mark(self) -> &'static str {
        match self {
            StepStatus::Symbolic => "✓",
            StepStatus::Lean => "⊢",
            StepStatus::Numeric => "≈",
            StepStatus::Structure => "◇",
            StepStatus::Cited => "📖",
            StepStatus::Gap | StepStatus::External => "⚠",
            StepStatus::Wrong => "✗",
        }
    }

    /// A short name for the status.
    pub fn name(self) -> &'static str {
        match self {
            StepStatus::Symbolic => "checked symbolically",
            StepStatus::Lean => "checked by Lean",
            StepStatus::Numeric => "checked numerically",
            StepStatus::Structure => "structure only",
            StepStatus::Cited => "cited",
            StepStatus::Gap => "gap",
            StepStatus::External => "external, unverified",
            StepStatus::Wrong => "wrong",
        }
    }

    /// How much the status establishes (higher is stronger).
    pub fn strength(self) -> u8 {
        match self {
            StepStatus::Symbolic | StepStatus::Lean => 6,
            StepStatus::Numeric => 5,
            StepStatus::Structure => 4,
            StepStatus::Cited => 3,
            StepStatus::Gap | StepStatus::External => 1,
            StepStatus::Wrong => 0,
        }
    }

    /// True for ✓, ⊢ and ≈: the step was checked by computation.
    pub fn is_computed(self) -> bool {
        matches!(
            self,
            StepStatus::Symbolic | StepStatus::Lean | StepStatus::Numeric
        )
    }

    /// The squiggle the IDE draws under the step (N9 kinds), if any.
    pub fn squiggle(self) -> Option<Squiggle> {
        match self {
            StepStatus::Wrong => Some(Squiggle::Error),
            StepStatus::Gap | StepStatus::External => Some(Squiggle::Warning),
            StepStatus::Cited | StepStatus::Structure => Some(Squiggle::Info),
            _ => None,
        }
    }
}

/// Underline kinds for the IDE (the N9 error styles).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Squiggle {
    Error,
    Warning,
    Info,
}

/// The outcome of checking one claim (one relation) in a step.
#[derive(Debug, Clone, PartialEq)]
pub enum ClaimOutcome {
    /// The CAS proved it.
    Proved,
    /// Agrees at random points or on a random instance; the text says how.
    Numeric(String),
    /// A counterexample: the point and the two sides.
    Failed(String),
    /// Could not be decided; the text says why.
    Unverified(String),
    /// Written in notation the CAS does not compute (an ellipsis); read as structure.
    Informal(String),
}

/// One relation in a step and what became of it.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaimCheck {
    /// The relation as written (the LaTeX source).
    pub source: String,
    pub span: Span,
    pub outcome: ClaimOutcome,
}

/// The check of one step.
#[derive(Debug, Clone, PartialEq)]
pub struct StepCheck {
    /// The step's number as written.
    pub number: Option<u32>,
    pub span: Span,
    pub status: StepStatus,
    /// One sentence for the hover: why the step got its status.
    pub detail: String,
    /// The counterexample of a ✗ step.
    pub counterexample: Option<String>,
    /// The relations checked in the step.
    pub claims: Vec<ClaimCheck>,
    /// The citation keys of a cited or external step.
    pub keys: Vec<String>,
    /// The results (`Lemma 2`) the step rests on, by their display names.
    pub uses: Vec<String>,
    /// True for a prose step under a computational reason (`by algebra`) with no
    /// relation to compute: structure only, and it can never close a gap.
    pub prose: bool,
}

/// One proof of a result.
#[derive(Debug, Clone, PartialEq)]
pub struct ProofCheck {
    pub span: Span,
    /// The title written after `**Proof**`, as plain text (`pairing`).
    pub title: Option<String>,
    pub steps: Vec<StepCheck>,
    /// Problems with the proof as a whole (an undischarged assumption, prose that is not
    /// split into steps); each counts as a gap.
    pub issues: Vec<String>,
    pub summary: Summary,
}

/// The verdict on a theorem, never stronger than its weakest step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Every step checked, by computation or structure, with at least one computed.
    Proved,
    /// As `Proved`, but resting on cited results (the count of distinct citations).
    ProvedUsingCited(usize),
    /// Every step is structure only: nothing was checked by computation.
    ProvedStructureOnly,
    /// Some steps are not justified (the count of gaps).
    Gaps(usize),
    /// Rests on external, unverified results (their count).
    NotProvedHere(usize),
    /// A step is wrong.
    Wrong,
    /// No proof is given (a conjecture, or a theorem with its proof still to write).
    NoProof,
}

impl Verdict {
    /// The summary line shown under the theorem.
    pub fn text(&self) -> String {
        match self {
            Verdict::Proved => "proved".into(),
            Verdict::ProvedUsingCited(1) => "proved, using 1 cited result".into(),
            Verdict::ProvedUsingCited(n) => format!("proved, using {n} cited results"),
            Verdict::ProvedStructureOnly => "proved (structure only)".into(),
            Verdict::Gaps(1) => "1 gap".into(),
            Verdict::Gaps(n) => format!("{n} gaps"),
            Verdict::NotProvedHere(1) => {
                "not proved here: rests on 1 external, unverified result".into()
            }
            Verdict::NotProvedHere(n) => {
                format!("not proved here: rests on {n} external, unverified results")
            }
            Verdict::Wrong => "not proved: a step is wrong".into(),
            Verdict::NoProof => "no proof given".into(),
        }
    }

    /// How much the verdict claims (higher claims more). A checker result that is
    /// stronger than the expected one is an over-claim.
    pub fn strength(&self) -> u8 {
        match self {
            Verdict::Proved => 6,
            Verdict::ProvedUsingCited(_) => 5,
            Verdict::ProvedStructureOnly => 4,
            Verdict::Gaps(_) => 2,
            Verdict::NotProvedHere(_) => 1,
            Verdict::NoProof | Verdict::Wrong => 0,
        }
    }

    /// True for the three "proved" verdicts.
    pub fn is_proved(&self) -> bool {
        matches!(
            self,
            Verdict::Proved | Verdict::ProvedUsingCited(_) | Verdict::ProvedStructureOnly
        )
    }
}

/// The summary of a proof or of a theorem.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub verdict: Verdict,
    /// The line shown under the theorem (the verdict's text, plus where inherited
    /// weaknesses come from).
    pub text: String,
    /// The distinct cited keys the result rests on (its own and its lemmas').
    pub cited: Vec<String>,
    /// The distinct external keys it rests on.
    pub external: Vec<String>,
    /// The number of gaps (its own and its lemmas').
    pub gaps: usize,
    /// True when no step was checked by computation.
    pub structure_only: bool,
}

/// A theorem-style result with its proofs.
#[derive(Debug, Clone, PartialEq)]
pub struct ResultCheck {
    pub kind: TheoremKind,
    /// `Lemma 2`, `Theorem 1`, or `Theorem` when it has no number.
    pub name: String,
    /// The number as written or counted.
    pub number: Option<String>,
    /// The title in brackets, as plain text.
    pub title: Option<String>,
    /// The heading and statement.
    pub span: Span,
    pub proofs: Vec<ProofCheck>,
    /// The other results its proofs rest on (by name).
    pub uses: Vec<String>,
    pub summary: Summary,
}

/// The check of a whole document.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DocumentCheck {
    pub results: Vec<ResultCheck>,
}

impl DocumentCheck {
    /// The result with this name (`Lemma 2`).
    pub fn result(&self, name: &str) -> Option<&ResultCheck> {
        self.results.iter().find(|r| r.name == name)
    }

    /// A plain-text report: one line per result, then one line per step.
    pub fn report(&self) -> String {
        let mut out = String::new();
        for r in &self.results {
            out.push_str(&format!("{}: {}\n", r.name, r.summary.text));
            for (k, p) in self.results_proofs(r).iter().enumerate() {
                if r.proofs.len() > 1 {
                    let t = p.title.clone().unwrap_or_else(|| format!("{}", k + 1));
                    out.push_str(&format!("  proof ({t}): {}\n", p.summary.text));
                }
                for s in &p.steps {
                    let n = s.number.map_or_else(|| "-".to_string(), |n| n.to_string());
                    out.push_str(&format!("  {n}. {} {}\n", s.status.mark(), s.detail));
                }
                for issue in &p.issues {
                    out.push_str(&format!("  ⚠ {issue}\n"));
                }
            }
        }
        out
    }

    fn results_proofs<'a>(&self, r: &'a ResultCheck) -> &'a [ProofCheck] {
        &r.proofs
    }
}
