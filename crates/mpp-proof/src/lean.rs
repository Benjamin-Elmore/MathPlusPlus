//! The Lean bridge: exports a document's theorems and proof skeletons to Lean 4 +
//! Mathlib, runs `lake build` when Lake is installed, and gives the alignment data the
//! IDE shows next to each statement.
//!
//! What the export promises, and no more:
//! - A statement the translator understands becomes a Lean statement over typed
//!   variables (`n : ℕ`, `x : ℝ`); anything else becomes `(sorry : Prop)` with the `.mpp`
//!   text as its doc comment. Either way the alignment view needs a person to confirm
//!   that the Lean statement says what the `.mpp` statement says.
//! - Each step becomes a `have` line. Only a step the checker proved symbolically (✓)
//!   whose claim translates gets a tactic (`first | ring | norm_num | simp | omega`);
//!   every other step gets `sorry`, and so does the end of every proof (the bridge does
//!   not reconstruct the argument that joins the steps).
//! - After `lake build`, a step whose `have` compiled with its tactic is ⊢. Nothing
//!   else changes status.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use mpp_syntax::document::proof::proof_steps;
use mpp_syntax::document::{BlockKind, Document, InlineKind, Theorem, TheoremKind};
use mpp_syntax::math::{
    BigOpKind, BinOp, MathAst, MulStyle, Node, NodeKind, RelOp, Statement, UnaryOp,
};

use crate::model::{DocumentCheck, StepStatus};
use crate::notation::{self, text};

/// Settings for the generated Lake project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeanConfig {
    /// The `lean-toolchain` line.
    pub toolchain: String,
    /// The Mathlib git revision (`master` follows the newest toolchain).
    pub mathlib_rev: String,
    /// How long `lake build` may run.
    pub timeout: Duration,
}

impl Default for LeanConfig {
    fn default() -> Self {
        LeanConfig {
            toolchain: "leanprover/lean4:stable".into(),
            mathlib_rev: "master".into(),
            timeout: Duration::from_secs(1800),
        }
    }
}

/// The alignment data for one statement: what the IDE shows side by side.
#[derive(Debug, Clone, PartialEq)]
pub struct Alignment {
    /// The `.mpp` statement as written.
    pub mpp: String,
    /// The Lean statement (`(n : ℕ) (h₁ : n ≥ 1) : ∑ k ∈ Finset.Icc 1 n, k = …`), or
    /// `None` when the translator could not state it.
    pub lean: Option<String>,
    /// The Lean statement typeset as LaTeX, with the types and any changed meaning
    /// (ℕ division, ℕ subtraction) made visible.
    pub lean_latex: Option<String>,
    /// Points the reader must check before confirming.
    pub warnings: Vec<String>,
    /// Set only by a person in the IDE. The export always leaves it false.
    pub confirmed: bool,
}

/// One exported step.
#[derive(Debug, Clone, PartialEq)]
pub struct LeanStep {
    pub number: Option<u32>,
    /// The checker's status when exported.
    pub status: StepStatus,
    /// True when the step's `have` ends in `sorry`.
    pub sorry: bool,
    /// The lines of the `.lean` file the step occupies (1-based, inclusive).
    pub lines: (usize, usize),
}

/// One exported theorem.
#[derive(Debug, Clone, PartialEq)]
pub struct LeanTheorem {
    /// The result's name in the document (`Lemma 2`).
    pub name: String,
    /// The Lean declaration name.
    pub lean_name: String,
    pub alignment: Alignment,
    /// The steps of the first proof, in order.
    pub steps: Vec<LeanStep>,
    /// True when the statement itself is `(sorry : Prop)`.
    pub statement_sorry: bool,
}

/// A whole export: the Lake project's files and what each theorem became.
#[derive(Debug, Clone, PartialEq)]
pub struct LeanExport {
    /// The Lean module name (`MppProof.NavierStokes`).
    pub module: String,
    /// The project's files: path relative to the project folder, and content.
    pub files: Vec<(PathBuf, String)>,
    pub theorems: Vec<LeanTheorem>,
}

impl LeanExport {
    /// The main `.lean` file's text.
    pub fn source(&self) -> &str {
        self.files
            .iter()
            .find(|(p, _)| p.extension().is_some_and(|e| e == "lean") && p.components().count() > 1)
            .map_or("", |(_, s)| s.as_str())
    }

    /// The theorem exported for a result.
    pub fn theorem(&self, name: &str) -> Option<&LeanTheorem> {
        self.theorems.iter().find(|t| t.name == name)
    }

    /// Writes the project into `dir` (created if needed).
    pub fn write(&self, dir: &Path) -> io::Result<()> {
        for (rel, content) in &self.files {
            let path = dir.join(rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, content)?;
        }
        Ok(())
    }
}

/// Exports every result of `doc` that has a proof or a statement, with the checker's
/// statuses from `check`. `module` is the file's stem (`navier-stokes`).
pub fn export(
    doc: &Document,
    src: &str,
    check: &DocumentCheck,
    module: &str,
    config: &LeanConfig,
) -> LeanExport {
    let module_name = format!("MppProof.{}", camel(module));
    let mut out = String::new();
    out.push_str("import Mathlib\n\n");
    out.push_str(&format!(
        "/-! Exported by Math++ from `{module}.mpp`. Statements need a person's confirmation in the\n    alignment view; `sorry` marks every step the checker did not prove symbolically. -/\n\n"
    ));
    out.push_str("open Real Nat Finset BigOperators\n\n");
    let mut line = out.lines().count() + 1;
    let mut theorems = Vec::new();
    let mut results = check.results.iter();
    let mut statement_src: BTreeMap<String, String> = BTreeMap::new();
    // Pair each result with its statement block and first proof, in document order.
    let mut blocks: Vec<(&Theorem, Option<&Theorem>)> = Vec::new();
    for b in &doc.blocks {
        if let BlockKind::Theorem(t) = &b.kind {
            if t.kind == TheoremKind::Proof {
                if let Some(last) = blocks.last_mut() {
                    if last.1.is_none() {
                        last.1 = Some(t);
                    }
                }
            } else if matches!(
                t.kind,
                TheoremKind::Theorem
                    | TheoremKind::Lemma
                    | TheoremKind::Corollary
                    | TheoremKind::Proposition
                    | TheoremKind::Conjecture
            ) {
                blocks.push((t, None));
                statement_src.insert(format!("{}", blocks.len()), statement_text(t, src));
            }
        }
    }
    for (idx, (statement, proof)) in blocks.iter().enumerate() {
        let Some(result) = results.next() else {
            break;
        };
        let lean_name = format!("{}_{}", snake(module), snake(&result.name));
        let mpp_text = statement_src
            .get(&format!("{}", idx + 1))
            .cloned()
            .unwrap_or_default();
        let formal = formal_statement(statement, src);
        let mut decl = String::new();
        let _ = writeln!(decl, "/-- {} — {} -/", result.name, doc_comment(&mpp_text));
        let (head, alignment, statement_sorry, binders) = match &formal {
            Some(f) => (
                format!("theorem {lean_name} {} :\n    {} := by", f.binders, f.goal),
                Alignment {
                    mpp: mpp_text.clone(),
                    lean: Some(format!("{} : {}", f.binders, f.goal).trim().to_string()),
                    lean_latex: Some(f.latex.clone()),
                    warnings: f.warnings.clone(),
                    confirmed: false,
                },
                false,
                f.vars.clone(),
            ),
            None => (
                format!("theorem {lean_name} : (sorry : Prop) := by"),
                Alignment {
                    mpp: mpp_text.clone(),
                    lean: None,
                    lean_latex: None,
                    warnings: vec![
                        "Math++ could not state this formally: write the Lean statement and confirm it here".into(),
                    ],
                    confirmed: false,
                },
                true,
                BTreeMap::new(),
            ),
        };
        decl.push_str(&head);
        decl.push('\n');
        line += decl.lines().count();
        let mut steps = Vec::new();
        if let (Some(proof), Some(pc)) = (proof, result.proofs.first()) {
            let parsed = proof_steps(proof, src);
            for (k, s) in parsed.steps.iter().enumerate() {
                let status = pc.steps.get(k).map_or(StepStatus::Gap, |c| c.status);
                let n = s.number.unwrap_or(k as u32 + 1);
                let claim = step_claim(&s.claim, src, &binders);
                let mut block = String::new();
                let _ = writeln!(
                    block,
                    "  -- step {n} ({} {}): {}",
                    status.mark(),
                    status.name(),
                    one_line(&crate::checker::plain(&s.claim, src))
                );
                let sorry = match (&claim, status) {
                    (Some(c), StepStatus::Symbolic) => {
                        let _ = writeln!(
                            block,
                            "  have step_{n} : {c} := by first | ring | norm_num | simp | omega"
                        );
                        false
                    }
                    (Some(c), _) => {
                        let _ = writeln!(block, "  have step_{n} : {c} := sorry");
                        true
                    }
                    (None, _) => {
                        let _ = writeln!(block, "  have step_{n} : (sorry : Prop) := sorry");
                        true
                    }
                };
                let count = block.lines().count();
                steps.push(LeanStep {
                    number: Some(n),
                    status,
                    sorry,
                    lines: (line, line + count - 1),
                });
                line += count;
                decl.push_str(&block);
            }
        }
        decl.push_str("  sorry\n\n");
        line += 2;
        out.push_str(&decl);
        theorems.push(LeanTheorem {
            name: result.name.clone(),
            lean_name,
            alignment,
            steps,
            statement_sorry,
        });
    }
    let lib = "MppProof".to_string();
    let file_rel = PathBuf::from(&lib).join(format!("{}.lean", camel(module)));
    let files = vec![
        (PathBuf::from("lean-toolchain"), format!("{}\n", config.toolchain)),
        (
            PathBuf::from("lakefile.toml"),
            format!(
                "name = \"MppProof\"\ndefaultTargets = [\"MppProof\"]\n\n[[require]]\nname = \"mathlib\"\nscope = \"leanprover-community\"\nrev = \"{}\"\n\n[[lean_lib]]\nname = \"MppProof\"\n",
                config.mathlib_rev
            ),
        ),
        (PathBuf::from("MppProof.lean"), format!("import {module_name}\n")),
        (file_rel, out),
    ];
    LeanExport {
        module: module_name,
        files,
        theorems,
    }
}

/// The statement of a theorem block as written, for the alignment view.
fn statement_text(t: &Theorem, src: &str) -> String {
    let mut parts = Vec::new();
    for b in &t.body {
        let span = b.span;
        parts.push(text(src, span).trim().to_string());
    }
    parts.join("\n")
}

fn doc_comment(s: &str) -> String {
    one_line(s).replace("-/", "- /")
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn camel(s: &str) -> String {
    let mut out = String::new();
    let mut up = true;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            if up {
                out.extend(c.to_uppercase());
            } else {
                out.push(c);
            }
            up = false;
        } else {
            up = true;
        }
    }
    if out.starts_with(|c: char| c.is_ascii_digit()) || out.is_empty() {
        out.insert(0, 'M');
    }
    out
}

fn snake(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    let t = out.trim_matches('_').to_string();
    if t.starts_with(|c: char| c.is_ascii_digit()) {
        format!("m_{t}")
    } else {
        t
    }
}

// ---- statements --------------------------------------------------------------

/// A statement in Lean: binders, goal, typeset form, and what to check.
struct Formal {
    binders: String,
    goal: String,
    latex: String,
    warnings: Vec<String>,
    vars: BTreeMap<String, LeanType>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeanType {
    Nat,
    Real,
}

impl LeanType {
    fn lean(self) -> &'static str {
        match self {
            LeanType::Nat => "ℕ",
            LeanType::Real => "ℝ",
        }
    }

    fn latex(self) -> &'static str {
        match self {
            LeanType::Nat => "\\mathbb{N}",
            LeanType::Real => "\\mathbb{R}",
        }
    }
}

/// Words a statement may use around its maths and still be stated faithfully from the
/// maths alone: quantifiers, types and connectives. Any other word carries meaning the
/// translator cannot see ("there exist a smooth …"), so the statement is not formalised.
const STATEMENT_WORDS: &[&str] = &[
    "for",
    "every",
    "all",
    "any",
    "each",
    "integer",
    "integers",
    "real",
    "reals",
    "number",
    "numbers",
    "natural",
    "positive",
    "nonnegative",
    "if",
    "then",
    "and",
    "with",
    "where",
    "let",
    "a",
    "an",
    "the",
    "we",
    "have",
    "it",
    "holds",
    "that",
    "is",
    "be",
    "prime",
    "displaystyle",
];

/// Words that make the maths after them a hypothesis (a bound or condition).
const HYPOTHESIS_LEAD: &[&str] = &[
    "every", "all", "any", "each", "integer", "integers", "real", "reals", "numbers", "number",
    "natural", "if", "with", "where", "positive",
];

/// The statement as Lean: exactly one goal relation, hypotheses from the relations after
/// quantifiers or "if", and no prose the maths does not carry. `None` otherwise.
fn formal_statement(t: &Theorem, src: &str) -> Option<Formal> {
    let mut relations: Vec<&Node> = Vec::new();
    let mut goals: Vec<&Node> = Vec::new();
    let mut prose = String::new();
    for b in &t.body {
        match &b.kind {
            BlockKind::Paragraph(p) => {
                let mut last_word = String::new();
                for i in p {
                    match &i.kind {
                        InlineKind::Math(m) => {
                            let mut found = Vec::new();
                            collect_relations(&m.ast, &mut found);
                            let hyp = HYPOTHESIS_LEAD.contains(&last_word.as_str());
                            for n in found {
                                if hyp {
                                    relations.push(n);
                                } else {
                                    goals.push(n);
                                }
                            }
                        }
                        InlineKind::Text(s) => {
                            prose.push_str(s);
                            prose.push(' ');
                            last_word = s
                                .to_lowercase()
                                .split(|c: char| !c.is_alphabetic())
                                .rfind(|w| !w.is_empty())
                                .unwrap_or("")
                                .to_string();
                        }
                        _ => return None,
                    }
                }
            }
            BlockKind::DisplayMath(m) => collect_relations(&m.ast, &mut goals),
            _ => return None,
        }
    }
    let faithful = prose
        .to_lowercase()
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| !w.is_empty())
        .all(|w| STATEMENT_WORDS.contains(&w));
    if !faithful || goals.len() != 1 {
        return None;
    }
    let goal = goals.pop()?;
    if relations
        .iter()
        .chain(std::iter::once(&goal))
        .any(|n| notation::has_indexed_family(n, src) || notation::has_dots(n))
    {
        return None;
    }
    let lower = prose.to_lowercase();
    let integer_context = ["integer", "prime", "natural"]
        .iter()
        .any(|w| lower.contains(w));
    let mut vars: BTreeMap<String, LeanType> = BTreeMap::new();
    let mut tr = Translator {
        src,
        vars: &mut vars,
        integer_context,
        complex: relations
            .iter()
            .chain(std::iter::once(&goal))
            .any(|n| uses_i(n)),
        warnings: BTreeSet::new(),
        bound: BTreeSet::new(),
    };
    // Types first: integer contexts make every free name ℕ.
    let goal_lean = tr.relation(goal)?;
    let mut hyps = Vec::new();
    for h in &relations {
        hyps.push(tr.relation(h)?);
    }
    let warnings: Vec<String> = tr.warnings.iter().cloned().collect();
    let mut binders = String::new();
    let mut latex_binders = Vec::new();
    for (v, ty) in &vars {
        let _ = write!(binders, "({v} : {}) ", ty.lean());
        latex_binders.push(format!("{v} : {}", ty.latex()));
    }
    for (i, (h, hl)) in hyps.iter().enumerate() {
        let _ = write!(binders, "(h{} : {h}) ", i + 1);
        latex_binders.push(hl.clone());
    }
    let latex = if latex_binders.is_empty() {
        goal_lean.1.clone()
    } else {
        format!(
            "\\forall\\, {},\\quad {}",
            latex_binders.join(",\\ "),
            goal_lean.1
        )
    };
    let mut warnings = warnings;
    warnings.push("check that the variable types and hypotheses match the .mpp statement".into());
    Some(Formal {
        binders: binders.trim_end().to_string(),
        goal: goal_lean.0,
        latex,
        warnings,
        vars,
    })
}

fn collect_relations<'a>(ast: &'a MathAst, out: &mut Vec<&'a Node>) {
    for line in &ast.lines {
        if let Statement::Relation(n) = &line.statement {
            if matches!(n.ungrouped().kind, NodeKind::Relation { .. }) {
                out.push(n);
            }
        }
    }
}

/// A step's claim in Lean, when every name in it is one of the theorem's variables.
fn step_claim(
    claim: &[mpp_syntax::document::Inline],
    src: &str,
    vars: &BTreeMap<String, LeanType>,
) -> Option<String> {
    let mut relations = Vec::new();
    for i in claim {
        if let InlineKind::Math(m) = &i.kind {
            collect_relations(&m.ast, &mut relations);
        }
    }
    let r = relations.last()?;
    let mut local = vars.clone();
    let mut tr = Translator {
        src,
        vars: &mut local,
        integer_context: vars.values().any(|t| *t == LeanType::Nat),
        complex: uses_i(r),
        warnings: BTreeSet::new(),
        bound: BTreeSet::new(),
    };
    let (lean, _) = tr.relation(r)?;
    // A name the theorem does not bind would not compile.
    if local.len() != vars.len() {
        return None;
    }
    Some(lean)
}

/// Translates maths to Lean (with a LaTeX rendering of what Lean will mean).
struct Translator<'a> {
    src: &'a str,
    vars: &'a mut BTreeMap<String, LeanType>,
    integer_context: bool,
    /// The statement uses `i`: functions are the complex ones, and real variables are
    /// cast into ℂ by Lean.
    complex: bool,
    warnings: BTreeSet<String>,
    bound: BTreeSet<String>,
}

impl Translator<'_> {
    fn relation(&mut self, n: &Node) -> Option<(String, String)> {
        let NodeKind::Relation { first, rest } = &n.ungrouped().kind else {
            return None;
        };
        let modulus = notation::pmod_of(text(self.src, n.span)).map(|(_, m)| m);
        let mut lean = Vec::new();
        let mut latex = Vec::new();
        let mut prev = self.expr(first)?;
        for (op, next) in rest {
            let rhs = self.expr(next)?;
            let (l_op, x_op) = match op {
                RelOp::Eq => ("=", "="),
                RelOp::Ne => ("≠", "\\ne"),
                RelOp::Lt => ("<", "<"),
                RelOp::Le => ("≤", "\\le"),
                RelOp::Gt => (">", ">"),
                RelOp::Ge => ("≥", "\\ge"),
                RelOp::Other(s) if *s == "∣" => ("∣", "\\mid"),
                RelOp::Equiv => {
                    let m = modulus.clone()?;
                    let mx = self.name_text(&m);
                    lean.push(format!("{} ≡ {} [MOD {}]", prev.0, rhs.0, mx));
                    latex.push(format!("{} \\equiv {} \\pmod{{{}}}", prev.1, rhs.1, mx));
                    prev = rhs;
                    continue;
                }
                _ => return None,
            };
            lean.push(format!("{} {l_op} {}", prev.0, rhs.0));
            latex.push(format!("{} {x_op} {}", prev.1, rhs.1));
            prev = rhs;
        }
        Some((lean.join(" ∧ "), latex.join(",\\ ")))
    }

    fn name_text(&mut self, m: &str) -> String {
        let n = sanitize(m);
        if n.chars().all(|c| c.is_ascii_digit()) {
            return n;
        }
        self.vars.entry(n.clone()).or_insert(LeanType::Nat);
        n
    }

    fn var(&mut self, raw: &str) -> String {
        let n = sanitize(raw);
        if !self.bound.contains(&n) {
            let ty = if self.integer_context {
                LeanType::Nat
            } else {
                LeanType::Real
            };
            self.vars.entry(n.clone()).or_insert(ty);
        }
        n
    }

    /// (Lean, LaTeX).
    fn expr(&mut self, n: &Node) -> Option<(String, String)> {
        let src = self.src;
        Some(match &n.kind {
            NodeKind::Number(t) => (t.trim().to_string(), t.trim().to_string()),
            NodeKind::Name(name) => {
                if name.base.chars().count() > 1
                    && name.sub.is_none()
                    && !name.base.chars().all(char::is_alphabetic)
                {
                    return None;
                }
                let raw = notation::node_text(n, src);
                let v = self.var(raw.trim());
                if self.bound.contains(&v) && !self.integer_context {
                    // A sum's index is a natural number; the summand is real.
                    (format!("({v} : ℝ)"), v)
                } else {
                    (v.clone(), v)
                }
            }
            NodeKind::Constant(c) => match c {
                mpp_syntax::math::Constant::Pi => ("Real.pi".into(), "\\pi".into()),
                mpp_syntax::math::Constant::E => ("Real.exp 1".into(), "e".into()),
                mpp_syntax::math::Constant::I => ("Complex.I".into(), "i".into()),
                mpp_syntax::math::Constant::Infinity => return None,
            },
            NodeKind::Group { inner, .. } => {
                let (l, x) = self.expr(inner)?;
                (format!("({l})"), format!("({x})"))
            }
            NodeKind::Unary {
                op: UnaryOp::Neg,
                operand,
            } => {
                let (l, x) = self.expr(operand)?;
                (format!("(-{l})"), format!("-{x}"))
            }
            // `a^p + 1 \pmod p` reads the modulus into the last term; the relation
            // carries it instead.
            NodeKind::Binary { lhs, rhs, .. } if matches!(&rhs.kind, NodeKind::NamedOp(op) if op.name == "mod" || op.name == "pmod") =>
            {
                return self.expr(lhs);
            }
            NodeKind::Binary { op, lhs, rhs } => {
                let (a, ax) = self.expr(lhs)?;
                let (b, bx) = self.expr(rhs)?;
                match op {
                    BinOp::Add => (format!("{a} + {b}"), format!("{ax} + {bx}")),
                    BinOp::Sub => {
                        if self.integer_context {
                            self.warnings.insert(
                                "subtraction in ℕ stops at 0 (a − b = 0 when b > a)".into(),
                            );
                            (
                                format!("{a} - {b}"),
                                format!("{ax} \\mathbin{{\\dot-}} {bx}"),
                            )
                        } else {
                            (format!("{a} - {b}"), format!("{ax} - {bx}"))
                        }
                    }
                    BinOp::Mul(MulStyle::Implicit | MulStyle::Cdot | MulStyle::Star)
                    | BinOp::Times => (format!("{a} * {b}"), format!("{ax} \\cdot {bx}")),
                    _ => return None,
                }
            }
            NodeKind::Frac { num, den, .. } => {
                let (a, ax) = self.expr(num)?;
                let (b, bx) = self.expr(den)?;
                if self.integer_context {
                    self.warnings
                        .insert("division in ℕ rounds down (7 / 2 = 3)".into());
                    (
                        format!("({a}) / ({b})"),
                        format!("\\left\\lfloor \\frac{{{ax}}}{{{bx}}} \\right\\rfloor"),
                    )
                } else {
                    (format!("({a}) / ({b})"), format!("\\frac{{{ax}}}{{{bx}}}"))
                }
            }
            NodeKind::Pow { base, exp }
                if matches!(base.kind, NodeKind::Constant(mpp_syntax::math::Constant::E)) =>
            {
                let (b, bx) = self.expr(exp)?;
                let f = if self.complex {
                    "Complex.exp"
                } else {
                    "Real.exp"
                };
                (format!("{f} ({b})"), format!("e^{{{bx}}}"))
            }
            NodeKind::Pow { base, exp } => {
                let (a, ax) = self.expr(base)?;
                let (b, bx) = self.expr(exp)?;
                (format!("({a}) ^ ({b})"), format!("{{{ax}}}^{{{bx}}}"))
            }
            NodeKind::Sqrt {
                index: None,
                radicand,
                ..
            } => {
                let (a, ax) = self.expr(radicand)?;
                (format!("Real.sqrt ({a})"), format!("\\sqrt{{{ax}}}"))
            }
            NodeKind::Factorial { operand, count: 1 } => {
                let (a, ax) = self.expr(operand)?;
                (format!("Nat.factorial ({a})"), format!("{ax}!"))
            }
            NodeKind::Binom { n: top, k, .. } => {
                let (a, ax) = self.expr(top)?;
                let (b, bx) = self.expr(k)?;
                (
                    format!("Nat.choose ({a}) ({b})"),
                    format!("\\binom{{{ax}}}{{{bx}}}"),
                )
            }
            NodeKind::FuncApp(app) => {
                let [arg] = app.args.as_slice() else {
                    if app.func == "gcd" {
                        let [a, b] = app.args.as_slice() else {
                            return None;
                        };
                        let (a, ax) = self.expr(a)?;
                        let (b, bx) = self.expr(b)?;
                        return Some((
                            format!("Nat.gcd ({a}) ({b})"),
                            format!("\\gcd({ax}, {bx})"),
                        ));
                    }
                    return None;
                };
                if app.power.is_some() || app.inverse || app.sub.is_some() {
                    return None;
                }
                let (a, ax) = self.expr(arg)?;
                let ns = if self.complex { "Complex" } else { "Real" };
                let f = match app.func {
                    "sin" => format!("{ns}.sin"),
                    "cos" => format!("{ns}.cos"),
                    "tan" => format!("{ns}.tan"),
                    "exp" => format!("{ns}.exp"),
                    "ln" | "log" => format!("{ns}.log"),
                    _ => return None,
                };
                (format!("{f} ({a})"), format!("\\{}({ax})", app.func))
            }
            NodeKind::BigOp(b) if b.op == BigOpKind::Sum => {
                let index = b.index.as_ref()?;
                let i = sanitize(notation::node_text(index, src).trim());
                let from = b.from.as_ref()?;
                let to = b.to.as_ref()?;
                let (lo, lox) = self.expr(from)?;
                self.bound.insert(i.clone());
                let infinite = matches!(
                    to.kind,
                    NodeKind::Constant(mpp_syntax::math::Constant::Infinity)
                );
                let (body, bodyx) = self.expr(&b.body)?;
                if infinite {
                    self.warnings
                        .insert("an infinite sum is Lean's tsum, which is 0 when the series does not converge".into());
                    (
                        format!("(∑' {i} : ℕ, (if {lo} ≤ {i} then {body} else 0))"),
                        format!("\\sum_{{{i}={lox}}}^{{\\infty}} {bodyx}"),
                    )
                } else {
                    let (hi, hix) = self.expr(to)?;
                    (
                        format!("(∑ {i} ∈ Finset.Icc ({lo}) ({hi}), {body})"),
                        format!("\\sum_{{{i}={lox}}}^{{{hix}}} {bodyx}"),
                    )
                }
            }
            NodeKind::Call { callee, args, .. } if args.len() == 1 => {
                // `n(n+1)`: a product.
                let (a, ax) = self.expr(callee)?;
                let (b, bx) = self.expr(args.first()?)?;
                (format!("{a} * ({b})"), format!("{ax}({bx})"))
            }
            _ => return None,
        })
    }
}

fn uses_i(n: &Node) -> bool {
    let mut found = false;
    notation::walk(n, &mut |x| {
        if matches!(x.kind, NodeKind::Constant(mpp_syntax::math::Constant::I)) {
            found = true;
        }
    });
    found
}

/// A Lean identifier for a name as written (`\theta` → `θ`, `p_1` → `p_1`).
fn sanitize(raw: &str) -> String {
    let mut s = raw.replace(['{', '}', ' '], "");
    for (tex, uni) in [
        ("\\alpha", "α"),
        ("\\beta", "β"),
        ("\\gamma", "γ"),
        ("\\delta", "δ"),
        ("\\theta", "θ"),
        ("\\lambda", "μλ"),
        ("\\mu", "μ"),
        ("\\nu", "ν"),
        ("\\phi", "φ"),
        ("\\omega", "ω"),
    ] {
        s = s.replace(tex, uni);
    }
    // `λ` is a keyword in Lean.
    s = s.replace("μλ", "lam");
    s.retain(|c| c.is_alphanumeric() || c == '_' || c == '\'');
    if s.is_empty() || s.starts_with(|c: char| c.is_ascii_digit()) {
        format!("v{s}")
    } else {
        s
    }
}

// ---- running Lake --------------------------------------------------------------

/// What `lake build` reported.
#[derive(Debug, Clone, PartialEq)]
pub struct LakeOutcome {
    /// True when the build finished (warnings allowed).
    pub success: bool,
    /// Error lines in the main file: (line, message).
    pub errors: Vec<(usize, String)>,
    /// The tail of Lake's output, for the IDE's log.
    pub log: String,
}

/// Why Lake could not be run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LakeError {
    /// `lake` is not on PATH (Lean is not installed).
    NotInstalled,
    /// It ran past the time limit and was stopped.
    TimedOut,
    /// It could not be started.
    Io(String),
}

impl std::fmt::Display for LakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LakeError::NotInstalled => f.write_str("Lean is not installed (no `lake` on PATH)"),
            LakeError::TimedOut => f.write_str("`lake build` ran past its time limit"),
            LakeError::Io(e) => write!(f, "`lake build` could not run: {e}"),
        }
    }
}

impl std::error::Error for LakeError {}

/// The `lake` executable on PATH, if any.
pub fn find_lake() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let names: &[&str] = if cfg!(windows) {
        &["lake.exe", "lake.cmd", "lake"]
    } else {
        &["lake"]
    };
    std::env::split_paths(&path)
        .find_map(|dir| names.iter().map(|n| dir.join(n)).find(|p| p.is_file()))
}

/// Writes the export into `dir` and runs `lake build` there.
pub fn build(
    export: &LeanExport,
    dir: &Path,
    config: &LeanConfig,
) -> Result<LakeOutcome, LakeError> {
    let lake = find_lake().ok_or(LakeError::NotInstalled)?;
    export
        .write(dir)
        .map_err(|e| LakeError::Io(e.to_string()))?;
    let mut child = Command::new(lake)
        .arg("build")
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| LakeError::Io(e.to_string()))?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if start.elapsed() > config.timeout {
                    let _ = child.kill();
                    return Err(LakeError::TimedOut);
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => return Err(LakeError::Io(e.to_string())),
        }
    }
    let out = child
        .wait_with_output()
        .map_err(|e| LakeError::Io(e.to_string()))?;
    let mut log = String::from_utf8_lossy(&out.stdout).to_string();
    log.push_str(&String::from_utf8_lossy(&out.stderr));
    let file = format!("{}.lean", export.module.replace('.', "/"));
    Ok(parse_lake_output(&log, &file, out.status.success()))
}

/// Reads `error:` lines that point into `file` (`MppProof/X.lean:12:4: error: …`).
pub fn parse_lake_output(log: &str, file: &str, success: bool) -> LakeOutcome {
    let file_fwd = file.replace('\\', "/");
    let mut errors = Vec::new();
    for raw in log.lines() {
        let l = raw.replace('\\', "/");
        let Some(at) = l.find(&file_fwd) else {
            continue;
        };
        let rest = &l[at + file_fwd.len()..];
        let mut parts = rest.trim_start_matches(':').splitn(3, ':');
        let (Some(line), Some(_col), Some(msg)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let is_error = msg.trim_start().starts_with("error") || l.trim_start().starts_with("error");
        if let (true, Ok(n)) = (is_error, line.trim().parse::<usize>()) {
            errors.push((n, msg.trim().to_string()));
        }
    }
    let tail: Vec<&str> = log.lines().rev().take(40).collect();
    LakeOutcome {
        success,
        errors,
        log: tail.into_iter().rev().collect::<Vec<_>>().join("\n"),
    }
}

/// Marks ⊢ every step whose tactic compiled: it had no `sorry` and no error on its lines.
/// Returns how many steps changed. This is the only way a step becomes ⊢.
pub fn apply(check: &mut DocumentCheck, export: &LeanExport, outcome: &LakeOutcome) -> usize {
    let mut changed = 0;
    for t in &export.theorems {
        let Some(r) = check.results.iter_mut().find(|r| r.name == t.name) else {
            continue;
        };
        let Some(proof) = r.proofs.first_mut() else {
            continue;
        };
        for (k, s) in t.steps.iter().enumerate() {
            if s.sorry {
                continue;
            }
            let failed = outcome
                .errors
                .iter()
                .any(|(line, _)| *line >= s.lines.0 && *line <= s.lines.1);
            if failed || !outcome.success && outcome.errors.is_empty() {
                continue;
            }
            if let Some(step) = proof.steps.get_mut(k) {
                if step.status != StepStatus::Lean {
                    step.status = StepStatus::Lean;
                    step.detail = format!("checked by Lean: {}", step.detail);
                    changed += 1;
                }
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(camel("navier-stokes"), "NavierStokes");
        assert_eq!(camel("01-pendulum"), "M01Pendulum");
        assert_eq!(snake("Lemma 2"), "lemma_2");
        assert_eq!(sanitize("\\theta"), "θ");
        assert_eq!(sanitize("\\lambda"), "lam");
        assert_eq!(sanitize("p_{1}"), "p_1");
    }

    #[test]
    fn lake_errors_are_read() {
        let log = "info: building\nMppProof/Gauss.lean:12:4: error: unsolved goals\nMppProof/Gauss.lean:20:2: warning: declaration uses 'sorry'\n";
        let o = parse_lake_output(log, "MppProof/Gauss.lean", false);
        assert_eq!(o.errors, vec![(12, "error: unsolved goals".to_string())]);
    }
}
