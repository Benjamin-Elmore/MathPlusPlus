//! The proof checker: walks a document's theorems and proofs and gives every step a
//! status and every theorem a summary that never claims more than its weakest step.
//!
//! Only this module sets statuses. The proof agent (see [`crate::agent`]) can write
//! steps, but it reads statuses from here like any other caller.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use mpp_syntax::document::proof::{
    proof_steps, NoteKind, ProofStep, ReasonKind, StepLabel, StepRef,
};
use mpp_syntax::document::{
    inline_plain_text, parse_document, Block, BlockKind, Document, Inline, InlineKind, Reference,
    Theorem, TheoremKind,
};
use mpp_syntax::math::{BinOp, Clause, DefHead, MathAst, Node, NodeKind, Statement};
use mpp_syntax::Span;

use crate::algebra::{self, Claim, Condition, Equation, Knowledge};
use crate::cas::Def;
use crate::fields::FieldScope;
use crate::geometry::{self, Scene};
use crate::indexed;
use crate::model::*;
use crate::notation::{self, ClaimOp, Link};

/// Checks every theorem in `src`.
pub fn check_source(src: &str) -> DocumentCheck {
    let doc = parse_document(src);
    check_document(&doc, src, &[])
}

/// Checks every theorem in a parsed document. `extra_references` are the entries read
/// from a `References from [[file]]` file, if the document has one.
pub fn check_document(doc: &Document, src: &str, extra_references: &[Reference]) -> DocumentCheck {
    let ctx = DocContext::read(doc, src, extra_references);
    let mut results: Vec<ResultCheck> = Vec::new();
    let mut raw: Vec<RawResult> = Vec::new();
    for item in &ctx.items {
        let r = check_result(item, &ctx, src);
        raw.push(r);
    }
    // Summaries through the results each one rests on.
    let names: Vec<String> = raw.iter().map(|r| r.check.name.clone()).collect();
    let mut memo: HashMap<usize, Summary> = HashMap::new();
    for i in 0..raw.len() {
        let mut stack = Vec::new();
        effective(i, &raw, &names, &mut memo, &mut stack);
    }
    for (i, mut r) in raw.into_iter().enumerate() {
        if let Some(s) = memo.remove(&i) {
            r.check.summary = s;
        }
        results.push(r.check);
    }
    DocumentCheck { results }
}

// ---- the document's context --------------------------------------------------

/// A theorem-style result with the blocks that state and prove it.
struct Item<'a> {
    kind: TheoremKind,
    name: String,
    number: Option<String>,
    title: Option<String>,
    span: Span,
    theorem: &'a Theorem,
    proofs: Vec<(&'a Theorem, Span)>,
    /// Index into the document's top-level blocks.
    position: usize,
}

/// What the document says outside its proofs.
struct DocContext<'a> {
    items: Vec<Item<'a>>,
    /// Known citation keys.
    keys: BTreeSet<String>,
    /// Labelled display equations.
    labels: HashMap<String, &'a MathAst>,
    /// Definitions, conditions, primes and universal names, with the block index they
    /// come from.
    defs: Vec<(usize, Def, BTreeSet<String>)>,
    conditions: Vec<(usize, Condition)>,
    /// Facts read from prose: primes, universal and real names, indexed families.
    marks: Vec<(usize, Mark)>,
    /// Prose outside theorems, for the geometry it sets up (curves, chords).
    prose: Vec<(usize, &'a [Inline])>,
    /// Maths outside theorems that can declare fields, with its block index.
    field_maths: Vec<(usize, &'a Node)>,
    /// Definitions that sum over an indexed family.
    indexed: BTreeSet<String>,
}

impl<'a> DocContext<'a> {
    fn read(doc: &'a Document, src: &'a str, extra: &[Reference]) -> Self {
        let mut ctx = DocContext {
            items: Vec::new(),
            keys: BTreeSet::new(),
            labels: HashMap::new(),
            defs: Vec::new(),
            conditions: Vec::new(),
            marks: Vec::new(),
            prose: Vec::new(),
            field_maths: Vec::new(),
            indexed: BTreeSet::new(),
        };
        for r in doc.reference_entries() {
            ctx.keys.insert(r.key.clone());
        }
        for r in doc.reference_list_entries().iter().chain(extra.iter()) {
            ctx.keys.insert(r.key.clone());
        }
        let mut count = 0usize;
        for (pos, b) in doc.blocks.iter().enumerate() {
            match &b.kind {
                BlockKind::Theorem(t) if t.kind == TheoremKind::Proof => {
                    // A proof belongs to the result before it.
                    if let Some(last) = ctx.items.last_mut() {
                        last.proofs.push((t, b.span));
                    }
                }
                BlockKind::Theorem(t) if is_result_kind(t.kind) => {
                    count += 1;
                    let number = t.number.clone().or_else(|| Some(count.to_string()));
                    let name = match &number {
                        Some(n) => format!("{} {n}", t.kind.as_str()),
                        None => t.kind.as_str().to_string(),
                    };
                    ctx.items.push(Item {
                        kind: t.kind,
                        name,
                        number,
                        title: t.title.as_ref().map(|i| plain(i, src)),
                        span: b.span,
                        theorem: t,
                        proofs: Vec::new(),
                        position: pos,
                    });
                }
                BlockKind::Theorem(_) => {}
                _ => ctx.read_block(pos, b, src),
            }
        }
        ctx
    }

    fn read_block(&mut self, pos: usize, b: &'a Block, src: &'a str) {
        match &b.kind {
            BlockKind::DisplayMath(m) => {
                if let Some(l) = &m.ast.label {
                    self.labels.insert(l.name.clone(), &m.ast);
                }
                self.read_math(pos, &m.ast, src);
            }
            BlockKind::Let(l) => self.read_inlines(pos, &l.content, src),
            BlockKind::Paragraph(p) => self.read_inlines(pos, p, src),
            BlockKind::Assume(a) => {
                for c in &a.conditions {
                    for link in notation::links(c, src) {
                        if let Some(cond) = condition(&link, src) {
                            self.conditions.push((pos, cond));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn read_inlines(&mut self, pos: usize, inlines: &'a [Inline], src: &'a str) {
        for p in prose_marks(inlines, src) {
            self.marks.push((pos, p));
        }
        self.prose.push((pos, inlines));
        // "with $a < b$": an inequality the document assumes from here on.
        for (link, hyp) in prose_relations(inlines, src) {
            let inequality = matches!(
                link.op,
                ClaimOp::Lt | ClaimOp::Le | ClaimOp::Gt | ClaimOp::Ge
            );
            if hyp && inequality {
                if let Some(cond) = condition(&link, src) {
                    self.conditions.push((pos, cond));
                }
            }
        }
        for i in inlines {
            if let InlineKind::Math(m) = &i.kind {
                self.read_math(pos, &m.ast, src);
            }
        }
    }

    fn read_math(&mut self, pos: usize, ast: &'a MathAst, src: &'a str) {
        for line in &ast.lines {
            match &line.statement {
                Statement::Definition { head, body, .. } => {
                    if let Some(d) = definition(head, body, src) {
                        if notation::has_indexed_family(body, src) {
                            self.indexed.insert(d.0.name.clone());
                        }
                        self.defs.push((pos, d.0, d.1));
                    }
                }
                Statement::Relation(n) => self.field_maths.push((pos, n)),
                _ => {}
            }
        }
    }
}

fn is_result_kind(k: TheoremKind) -> bool {
    matches!(
        k,
        TheoremKind::Theorem
            | TheoremKind::Lemma
            | TheoremKind::Corollary
            | TheoremKind::Proposition
            | TheoremKind::Conjecture
    )
}

/// Inlines as plain text, with maths as its source between dollars.
pub(crate) fn plain(inlines: &[Inline], src: &str) -> String {
    let mut s = String::new();
    for i in inlines {
        match &i.kind {
            InlineKind::Math(m) => s.push_str(&format!("${}$", notation::text(src, m.raw).trim())),
            _ => inline_plain_text(i, &mut s),
        }
    }
    s.trim().to_string()
}

/// `name := body` as a scratch definition, with the names the body uses.
fn definition(head: &DefHead, body: &Node, src: &str) -> Option<(Def, BTreeSet<String>)> {
    let head_text = match head {
        DefHead::Name(n) => notation::node_text(n, src).trim().to_string(),
        DefHead::Function { name, params } => {
            let ps: Vec<&str> = params
                .iter()
                .map(|p| notation::text(src, p.span).trim())
                .collect();
            format!(
                "{}({})",
                notation::text(src, name.span).trim(),
                ps.join(", ")
            )
        }
        _ => return None,
    };
    // A body with an ellipsis the CAS cannot read is kept as written, so a claim that
    // uses it is recognised as informal.
    let body_text =
        formal_text(body, src).unwrap_or_else(|| notation::node_text(body, src).to_string());
    Some((Def::new(head_text, body_text), notation::names(body, src)))
}

/// The LaTeX of a node for the CAS: `\sum_{k \ge 1}` rewritten, `\pmod` removed, and an
/// arithmetic ellipsis (`1 + 2 + \dots + n`) replaced by its closed form. `None` when the
/// node holds an ellipsis that is not of that kind.
fn formal_text(node: &Node, src: &str) -> Option<String> {
    if !notation::has_dots(node) {
        return Some(notation::side_text(node, src));
    }
    let (a, b, c) = notation::arithmetic_ellipsis(node)?;
    algebra::arithmetic_closed_form(
        &notation::side_text(a, src),
        &notation::side_text(b, src),
        &notation::side_text(c, src),
    )
}

fn condition(link: &Link, src: &str) -> Option<Condition> {
    let lhs = formal_text(&link.lhs, src)?;
    let rhs = formal_text(&link.rhs, src)?;
    let mut names = notation::names(&link.lhs, src);
    names.extend(notation::names(&link.rhs, src));
    Some(Condition {
        lhs,
        op: link.op,
        rhs,
        names,
    })
}

/// Facts read from prose around maths.
#[derive(Debug, Clone)]
enum Mark {
    /// "Let $p$ be a prime", "a prime $p$".
    Prime(String),
    /// "for every real $x$", "for an integer $m$": a name that stands for any value.
    Universal(String),
    /// A name in a sentence that says it is real ("for every real $x$", "For real
    /// numbers $a_1, \dots, a_n$").
    Real(String),
    /// An indexed family written out as `a_1, \dots, a_n`: base, first and last index.
    Family(String, String, String),
}

/// Records a mark in what a proof knows.
fn apply_mark(k: &mut Knowledge, mark: Mark) {
    match mark {
        Mark::Prime(n) => {
            k.primes.insert(n);
        }
        Mark::Universal(n) => {
            k.universal.insert(n);
        }
        Mark::Real(n) => {
            k.reals.insert(n);
        }
        Mark::Family(base, from, to) => {
            if !k.families.iter().any(|(b, _, _)| *b == base) {
                k.families.push((base, from, to));
            }
        }
    }
}

/// The relations in maths inside prose, each with whether the word before it makes it a
/// hypothesis ("if", "with", "suppose" …).
fn prose_relations(inlines: &[Inline], src: &str) -> Vec<(Link, bool)> {
    let mut out = Vec::new();
    let mut prev_text = String::new();
    for i in inlines {
        match &i.kind {
            InlineKind::Text(t) => prev_text = t.to_lowercase(),
            InlineKind::Math(m) => {
                let last = prev_text
                    .split(|c: char| !c.is_alphabetic())
                    .rfind(|w| !w.is_empty())
                    .unwrap_or("")
                    .to_string();
                let hyp = HYPOTHESIS_WORDS.contains(&last.as_str());
                for line in &m.ast.lines {
                    if let Statement::Relation(n) = &line.statement {
                        out.extend(notation::links(n, src).into_iter().map(|l| (l, hyp)));
                    }
                }
                prev_text.clear();
            }
            _ => {}
        }
    }
    out
}

/// `a_1, \dots, a_n` in a name list: the family's base and its first and last index.
fn family_list(names: &[Node], src: &str) -> Option<(String, String, String)> {
    let subscripted = |n: &Node| -> Option<(String, String)> {
        let NodeKind::Name(name) = &n.ungrouped().kind else {
            return None;
        };
        let sub = name.sub.as_ref()?;
        let whole = notation::node_text(n, src);
        let base = whole.split('_').next()?.trim().to_string();
        let idx = notation::text(src, sub.span)
            .trim()
            .trim_start_matches('{')
            .trim_end_matches('}')
            .trim()
            .to_string();
        Some((base, idx))
    };
    if !names.iter().any(|n| matches!(n.kind, NodeKind::Dots(_))) {
        return None;
    }
    let first = names.first().and_then(subscripted)?;
    let last = names.last().and_then(subscripted)?;
    (first.0 == last.0 && !first.0.is_empty()).then_some((first.0, first.1, last.1))
}

/// True when the sentence says its names are real.
fn says_real(sentence: &str) -> bool {
    sentence
        .split(|c: char| !c.is_alphabetic())
        .any(|w| w == "real" || w == "reals")
}

fn prose_marks(inlines: &[Inline], src: &str) -> Vec<Mark> {
    let mut out = Vec::new();
    // The current sentence's text before each maths span.
    let mut sentence = String::new();
    for (k, i) in inlines.iter().enumerate() {
        let InlineKind::Math(m) = &i.kind else {
            if let InlineKind::Text(t) = &i.kind {
                let t = t.to_lowercase();
                match t.rfind(['.', ';']) {
                    Some(at) => sentence = t[at + 1..].to_string(),
                    None => sentence.push_str(&t),
                }
            }
            continue;
        };
        let real = says_real(&sentence);
        let before = match k.checked_sub(1).and_then(|j| inlines.get(j)) {
            Some(Inline {
                kind: InlineKind::Text(t),
                ..
            }) => t.to_lowercase(),
            _ => String::new(),
        };
        let after = match inlines.get(k + 1) {
            Some(Inline {
                kind: InlineKind::Text(t),
                ..
            }) => t.to_lowercase(),
            _ => String::new(),
        };
        let mut names = BTreeSet::new();
        for line in &m.ast.lines {
            if let Statement::Relation(n) | Statement::Query { expr: n, .. } = &line.statement {
                names.extend(notation::names(n, src));
            }
            if let Statement::NameList(ns) = &line.statement {
                for n in ns {
                    names.extend(notation::names(n, src));
                }
                if let Some((base, from, to)) = family_list(ns, src) {
                    out.push(Mark::Family(base, from, to));
                }
            }
        }
        if real {
            out.extend(names.iter().cloned().map(Mark::Real));
        }
        let single = names.len() == 1;
        let after_t = after.trim_start();
        if single
            && (after_t.starts_with("be a prime")
                || after_t.starts_with("is a prime")
                || after_t.starts_with("is prime")
                || before.trim_end().ends_with("prime"))
        {
            out.extend(names.iter().cloned().map(Mark::Prime));
        }
        if universal_phrase(&before) {
            out.extend(names.into_iter().map(Mark::Universal));
        }
    }
    out
}

/// True when text ends with a universal quantifier ("for every real", "for an integer",
/// "For real numbers").
fn universal_phrase(before: &str) -> bool {
    let words: Vec<&str> = before.split_whitespace().rev().take(4).collect();
    let Some(pos) = words.iter().position(|w| *w == "for") else {
        return false;
    };
    let allowed = [
        "every", "all", "any", "each", "a", "an", "real", "reals", "integer", "integers",
        "natural", "numbers", "number", "positive", "whole",
    ];
    words.iter().take(pos).all(|w| allowed.contains(w)) && !words.contains(&"some")
}

// ---- checking one result ---------------------------------------------------

struct RawResult {
    check: ResultCheck,
    /// Per proof: own summary inputs and the results it rests on (by index).
    proofs: Vec<ProofRaw>,
}

struct ProofRaw {
    own: Summary,
    uses: Vec<usize>,
}

fn check_result(item: &Item<'_>, ctx: &DocContext<'_>, src: &str) -> RawResult {
    let mut proofs = Vec::new();
    let mut raws = Vec::new();
    for (proof, span) in &item.proofs {
        let (p, raw) = check_proof(item, proof, *span, ctx, src);
        proofs.push(p);
        raws.push(raw);
    }
    let mut uses: Vec<String> = Vec::new();
    for raw in &raws {
        for &u in &raw.uses {
            if let Some(it) = ctx.items.get(u) {
                if !uses.contains(&it.name) {
                    uses.push(it.name.clone());
                }
            }
        }
    }
    let summary = if proofs.is_empty() {
        no_proof(item.kind)
    } else {
        best(proofs.iter().map(|p| p.summary.clone()))
    };
    RawResult {
        check: ResultCheck {
            kind: item.kind,
            name: item.name.clone(),
            number: item.number.clone(),
            title: item.title.clone(),
            span: item.span,
            proofs,
            uses,
            summary,
        },
        proofs: raws,
    }
}

fn no_proof(kind: TheoremKind) -> Summary {
    Summary {
        verdict: Verdict::NoProof,
        text: if kind == TheoremKind::Conjecture {
            "conjecture: not proved".into()
        } else {
            "no proof given".into()
        },
        cited: Vec::new(),
        external: Vec::new(),
        gaps: 0,
        structure_only: true,
    }
}

fn best(summaries: impl Iterator<Item = Summary>) -> Summary {
    let mut out: Option<Summary> = None;
    for s in summaries {
        out = Some(match out {
            Some(o) if o.verdict.strength() >= s.verdict.strength() => o,
            _ => s,
        });
    }
    out.unwrap_or_else(|| no_proof(TheoremKind::Theorem))
}

/// State carried through a proof's steps.
struct ProofState<'c> {
    k: Knowledge,
    fields: FieldScope,
    keys: &'c BTreeSet<String>,
    seen_steps: BTreeSet<u32>,
    open_assumptions: Vec<u32>,
    base_case: bool,
    inductive_step: bool,
    uses: Vec<usize>,
    /// Index of the result being proved.
    me: usize,
    /// Curves and chords set up so far.
    scene: Scene,
    /// True once a step was proved by construction.
    constructed: bool,
    /// Outcomes decided for the current step's relations by geometry, by span.
    overrides: HashMap<(usize, usize), ClaimOutcome>,
    /// Each earlier step's status, and the sides of the relations it checked.
    statuses: HashMap<u32, StepStatus>,
    terms: Vec<(u32, String)>,
}

fn check_proof(
    item: &Item<'_>,
    proof: &Theorem,
    span: Span,
    ctx: &DocContext<'_>,
    src: &str,
) -> (ProofCheck, ProofRaw) {
    let me = ctx
        .items
        .iter()
        .position(|i| i.position == item.position)
        .unwrap_or(0);
    let mut k = Knowledge::default();
    for (pos, d, uses) in &ctx.defs {
        if *pos < item.position {
            k.defs.push((d.clone(), uses.clone()));
        }
    }
    for (pos, c) in &ctx.conditions {
        if *pos < item.position {
            k.conditions.push(c.clone());
        }
    }
    for (pos, m) in &ctx.marks {
        if *pos < item.position {
            apply_mark(&mut k, m.clone());
        }
    }
    k.indexed = ctx.indexed.clone();
    let mut fields = FieldScope::new();
    for (pos, n) in &ctx.field_maths {
        if *pos < item.position {
            fields.read(n, src);
        }
    }
    // The statement: definitions, declarations, conditions and quantifiers.
    let mut eqrefs: BTreeSet<String> = BTreeSet::new();
    let mut scene = Scene::default();
    for (pos, p) in &ctx.prose {
        if *pos < item.position {
            geometry::read_scene(p, src, &mut scene);
        }
    }
    for b in &item.theorem.body {
        read_statement_block(b, src, &mut k, &mut fields, &mut eqrefs);
        if let BlockKind::Paragraph(p) = &b.kind {
            geometry::read_scene(p, src, &mut scene);
        }
    }
    if let Some(title) = &item.theorem.title {
        collect_eqrefs(title, &mut eqrefs);
    }
    let steps = proof_steps(proof, src);
    for s in &steps.steps {
        collect_eqrefs(&s.claim, &mut eqrefs);
        if let Some(r) = &s.reason {
            for rf in &r.refs {
                if let StepRef::Equation(l) = rf {
                    eqrefs.insert(l.clone());
                }
            }
        }
    }
    for label in &eqrefs {
        if let Some(ast) = ctx.labels.get(label) {
            for line in &ast.lines {
                if let Statement::Relation(n) = &line.statement {
                    fields.assume(n);
                }
                for c in &line.clauses {
                    if let Clause::Condition(n) = c {
                        fields.assume(n);
                    }
                }
            }
        }
    }
    let mut st = ProofState {
        k,
        fields,
        keys: &ctx.keys,
        seen_steps: BTreeSet::new(),
        open_assumptions: Vec::new(),
        base_case: false,
        inductive_step: false,
        uses: Vec::new(),
        me,
        scene,
        constructed: false,
        overrides: HashMap::new(),
        statuses: HashMap::new(),
        terms: Vec::new(),
    };
    let mut checks = Vec::new();
    for s in &steps.steps {
        let c = check_step(s, &mut st, ctx, src);
        if let Some(n) = s.number {
            st.seen_steps.insert(n);
        }
        checks.push(c);
    }
    let mut issues = Vec::new();
    if steps.steps.is_empty() {
        issues.push(
            "the proof is written as prose, not as numbered steps, so no step could be checked"
                .to_string(),
        );
    }
    // A direct proof of a conditional statement starts from its hypothesis: reaching
    // the conclusion discharges it (conditional proof), so it is not left open.
    let statement: String = item
        .theorem
        .body
        .iter()
        .filter_map(|b| match &b.kind {
            BlockKind::Paragraph(p) => Some(plain(p, src)),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");
    if is_conditional_statement(&statement) {
        let mut still_open = Vec::new();
        for a in std::mem::take(&mut st.open_assumptions) {
            let text = steps
                .steps
                .iter()
                .position(|s| s.number == Some(a))
                .map(|i| plain(&steps.steps[i].claim, src))
                .unwrap_or_default();
            if announces_contradiction(&text) {
                still_open.push(a);
            } else if let Some(i) = steps.steps.iter().position(|s| s.number == Some(a)) {
                if let Some(c) = checks.get_mut(i) {
                    c.detail =
                        "assumed: the theorem's hypothesis, discharged by the conclusion".into();
                }
            }
        }
        st.open_assumptions = still_open;
    }
    for a in &st.open_assumptions {
        issues.push(format!(
            "the assumption in step {a} is never discharged (no contradiction or case split closes it)"
        ));
    }
    let own = summarise(&checks, &issues);
    let title = proof.title.as_ref().map(|t| plain(t, src));
    (
        ProofCheck {
            span,
            title,
            steps: checks,
            issues,
            summary: own.clone(),
        },
        ProofRaw { own, uses: st.uses },
    )
}

/// True when a theorem's statement has a hypothesis a direct proof may assume: "If …
/// then …", "Let …", "Suppose …", "for every …", an implication.
fn is_conditional_statement(text: &str) -> bool {
    let t = format!(" {} ", text.to_lowercase());
    [
        " if ",
        " let ",
        " suppose ",
        " assume ",
        " given ",
        " whenever ",
        " for every ",
        " for all ",
        " for each ",
        " for any ",
        "∀",
        "\\forall",
        "\\implies",
        "\\rightarrow",
        "⇒",
    ]
    .iter()
    .any(|w| t.contains(w))
}

/// True when an assumed step says it is assumed in order to reach a contradiction.
fn announces_contradiction(text: &str) -> bool {
    let t = format!(" {} ", text.to_lowercase());
    [
        "contradiction",
        "contrary",
        " not ",
        "¬",
        "\\neg",
        "\\ne ",
        "\\neq",
        "\\notin",
        "≠",
        "∉",
        "n't",
    ]
    .iter()
    .any(|w| t.contains(w))
}

fn collect_eqrefs(inlines: &[Inline], out: &mut BTreeSet<String>) {
    for i in inlines {
        match &i.kind {
            InlineKind::EqRef { label, .. } => {
                out.insert(label.clone());
            }
            InlineKind::Emphasis(x) | InlineKind::Strong(x) => collect_eqrefs(x, out),
            _ => {}
        }
    }
}

fn read_statement_block(
    b: &Block,
    src: &str,
    k: &mut Knowledge,
    fields: &mut FieldScope,
    eqrefs: &mut BTreeSet<String>,
) {
    let read_ast = |ast: &MathAst, k: &mut Knowledge, fields: &mut FieldScope| {
        for line in &ast.lines {
            match &line.statement {
                Statement::Definition { head, body, .. } => {
                    if let Some((d, uses)) = definition(head, body, src) {
                        if notation::has_indexed_family(body, src) {
                            k.indexed.insert(d.name.clone());
                        }
                        k.defs.push((d, uses));
                    }
                }
                Statement::Relation(n) => fields.read(n, src),
                _ => {}
            }
            for c in &line.clauses {
                match c {
                    Clause::Where { name, value, .. } => {
                        fields.declare(value, src);
                        fields.define(name, value, src);
                    }
                    Clause::Condition(n) => fields.read(n, src),
                    _ => {}
                }
            }
        }
    };
    match &b.kind {
        BlockKind::DisplayMath(m) => read_ast(&m.ast, k, fields),
        BlockKind::Paragraph(p) | BlockKind::Let(mpp_syntax::document::Let { content: p }) => {
            for mark in prose_marks(p, src) {
                apply_mark(k, mark);
            }
            collect_eqrefs(p, eqrefs);
            for i in p {
                if let InlineKind::Math(m) = &i.kind {
                    read_ast(&m.ast, k, fields);
                }
            }
        }
        _ => {}
    }
}

// ---- one step ----------------------------------------------------------------

/// Words that make the maths after them a hypothesis of the step.
const HYPOTHESIS_WORDS: &[&str] = &[
    "if", "suppose", "assume", "let", "given", "put", "set", "take", "fix", "with", "where",
    "when", "whenever",
];

/// The maths of a step, split into what it assumes and what it claims.
#[derive(Default)]
struct StepMaths {
    defs: Vec<(Def, BTreeSet<String>, Node, Node)>,
    hypotheses: Vec<Link>,
    claims: Vec<(Link, String)>,
    /// Each relation chain (`0 \le S = Q`) as its links, and whether it is a hypothesis.
    chains: Vec<(Vec<Link>, bool)>,
}

fn step_maths(claim: &[Inline], src: &str) -> StepMaths {
    let mut out = StepMaths::default();
    let mut prev_text = String::new();
    for i in claim {
        match &i.kind {
            InlineKind::Text(t) => prev_text = t.to_lowercase(),
            InlineKind::Math(m) => {
                let last = prev_text
                    .split(|c: char| !c.is_alphabetic())
                    .rfind(|w| !w.is_empty())
                    .unwrap_or("")
                    .to_string();
                let hyp = HYPOTHESIS_WORDS.contains(&last.as_str());
                for line in &m.ast.lines {
                    match &line.statement {
                        Statement::Definition { head, body, .. } => {
                            if let (
                                Some((d, uses)),
                                DefHead::Name(h) | DefHead::Function { name: h, .. },
                            ) = (definition(head, body, src), head)
                            {
                                out.defs.push((d, uses, h.clone(), body.clone()));
                            }
                        }
                        Statement::Relation(n) => {
                            let links = notation::links(n, src);
                            out.chains.push((links.clone(), hyp));
                            for l in links {
                                let text = notation::text(src, l.span).to_string();
                                if hyp {
                                    out.hypotheses.push(l);
                                } else {
                                    out.claims.push((l, text));
                                }
                            }
                        }
                        Statement::Check { lhs, rhs } => out
                            .claims
                            .push(check_statement_link(lhs, rhs, line.span, src)),
                        _ => {}
                    }
                    for c in &line.clauses {
                        if let Clause::Condition(n) = c {
                            for l in notation::links(n, src) {
                                let text = notation::text(src, l.span).to_string();
                                if hyp {
                                    out.hypotheses.push(l);
                                } else {
                                    out.claims.push((l, text));
                                }
                            }
                        }
                    }
                }
                prev_text.clear();
            }
            _ => {}
        }
    }
    out
}

/// The claim of a check line: `a \overset{?}{=} b` is an equation, and
/// `a \overset{?}{\equiv} b \pmod{n}` (read by the parser as `a mod n = b mod n`) is the
/// congruence it was written as. Returns the link and its source text.
fn check_statement_link(lhs: &Node, rhs: &Node, line: Span, src: &str) -> (Link, String) {
    let remainder = |n: &Node| match &n.kind {
        NodeKind::Binary {
            op: BinOp::Mod,
            lhs,
            rhs,
        } => Some((
            (**lhs).clone(),
            notation::text(src, rhs.span).trim().to_string(),
        )),
        _ => None,
    };
    if let (Some((a, m)), Some((b, m2))) = (remainder(lhs), remainder(rhs)) {
        if m == m2 && !m.is_empty() {
            let span = a.span.join(b.span);
            let text = notation::text(src, Span::new(span.start, line.end.max(span.end)));
            return (
                Link {
                    lhs: a,
                    op: ClaimOp::Congruent,
                    rhs: b,
                    modulus: Some(m),
                    span,
                },
                text.trim().to_string(),
            );
        }
    }
    let span = lhs.span.join(rhs.span);
    (
        Link {
            lhs: lhs.clone(),
            op: ClaimOp::Eq,
            rhs: rhs.clone(),
            modulus: None,
            span,
        },
        notation::text(src, span).to_string(),
    )
}

fn check_step(
    step: &ProofStep,
    st: &mut ProofState<'_>,
    ctx: &DocContext<'_>,
    src: &str,
) -> StepCheck {
    let mut c = StepCheck {
        number: step.number,
        span: step.span,
        status: StepStatus::Structure,
        detail: String::new(),
        counterexample: None,
        claims: Vec::new(),
        keys: Vec::new(),
        uses: Vec::new(),
        prose: false,
    };
    for mark in prose_marks(&step.claim, src) {
        apply_mark(&mut st.k, mark);
    }
    geometry::read_scene(&step.claim, src, &mut st.scene);
    let maths = step_maths(&step.claim, src);
    for (d, uses, head, body) in &maths.defs {
        if notation::has_indexed_family(body, src) {
            st.k.indexed.insert(d.name.clone());
        }
        st.k.defs.push((d.clone(), uses.clone()));
        st.fields.define(head, body, src);
    }
    for h in &maths.hypotheses {
        add_fact(&mut st.k, h, src);
    }
    st.overrides = geometry::step_outcomes(&step.claim, src, &st.scene, &st.k, st.constructed)
        .into_iter()
        .map(|(span, o)| ((span.start, span.end), o))
        .collect();
    let reason_kind = step.reason.as_ref().map(|r| r.kind);
    match step.label {
        Some(StepLabel::BaseCase) => st.base_case = true,
        Some(StepLabel::InductiveStep) => st.inductive_step = true,
        _ => {}
    }

    // References inside the reason: earlier steps, other results.
    let mut bad_ref: Option<String> = None;
    if let Some(r) = &step.reason {
        for rf in &r.refs {
            match rf {
                StepRef::Step(n) => {
                    let earlier = step.number.is_none_or(|me| *n < me);
                    if !earlier || !st.seen_steps.contains(n) {
                        bad_ref = Some(format!(
                            "it refers to step {n}, which does not come before it"
                        ));
                    }
                }
                StepRef::Result { kind, number } => {
                    let found = ctx.items.iter().position(|it| {
                        it.number.as_deref() == Some(number.as_str())
                            && (it.kind == *kind || is_result_kind(*kind))
                    });
                    match found {
                        Some(i) if i == st.me => {
                            bad_ref = Some(format!(
                                "it cites {} {number}, the result being proved (circular)",
                                kind.as_str()
                            ));
                        }
                        Some(i) => {
                            if !st.uses.contains(&i) {
                                st.uses.push(i);
                            }
                            if let Some(it) = ctx.items.get(i) {
                                c.uses.push(it.name.clone());
                            }
                        }
                        None => {
                            bad_ref = Some(format!(
                                "it refers to {} {number}, which is not in this document",
                                kind.as_str()
                            ));
                        }
                    }
                }
                StepRef::Equation(_) => {}
            }
        }
    }

    // Notes win over reasons: a gap is a gap, an external claim is unverified.
    let gap = step.notes.iter().find(|n| n.kind == NoteKind::Gap);
    let external = step.notes.iter().find(|n| n.kind == NoteKind::External);
    let cited = step.notes.iter().find(|n| n.kind == NoteKind::Cited);
    if let Some(g) = gap {
        c.status = StepStatus::Gap;
        c.detail = format!("gap: {}", g.text);
    } else if let Some(e) = external {
        c.status = StepStatus::External;
        c.keys = e.keys.clone();
        c.detail = format!(
            "rests on an external, unverified claim{}",
            keys_text(&e.keys)
        );
    } else if let Some(ct) = cited {
        c.keys = ct.keys.clone();
        let missing: Vec<&String> = ct.keys.iter().filter(|k| !st.keys.contains(*k)).collect();
        if ct.keys.is_empty() {
            c.status = StepStatus::Gap;
            c.detail = "a cited result must name its source with a citation [@key]".into();
        } else if !missing.is_empty() {
            c.status = StepStatus::Gap;
            c.detail = format!(
                "the citation{} has no entry in the references, so the step is not justified",
                keys_text(&missing.iter().map(|s| (*s).clone()).collect::<Vec<_>>())
            );
        } else {
            c.status = StepStatus::Cited;
            c.detail = format!("cited: {}", ct.text);
        }
    } else if let Some(why) = &bad_ref {
        c.status = StepStatus::Gap;
        c.detail = format!("not justified: {why}");
    } else {
        match reason_kind {
            None => {
                c.status = StepStatus::Gap;
                c.detail = "no reason given (write `— by …`, or mark it *(gap: …)*)".into();
            }
            Some(ReasonKind::Assumption) => {
                for (l, _) in &maths.claims {
                    add_fact(&mut st.k, l, src);
                }
                if let Some(n) = step.number {
                    st.open_assumptions.push(n);
                }
                c.detail = "assumed".into();
            }
            Some(ReasonKind::Construction) | Some(ReasonKind::Definition) => {
                // Claims are checked when the CAS can; otherwise the step introduces them.
                let (status, detail) = claims_status(&maths, st, src, &mut c, true);
                c.status = status.unwrap_or(StepStatus::Structure);
                c.detail = detail.unwrap_or_else(|| {
                    if reason_kind == Some(ReasonKind::Construction) {
                        "by construction".into()
                    } else {
                        "by definition".into()
                    }
                });
            }
            Some(ReasonKind::Contradiction) => {
                if st.open_assumptions.is_empty() {
                    c.status = StepStatus::Gap;
                    c.detail =
                        "a contradiction needs an assumption to contradict, and none is open"
                            .into();
                } else {
                    let opened: Vec<String> =
                        st.open_assumptions.iter().map(|n| n.to_string()).collect();
                    c.detail = format!(
                        "contradiction: discharges the assumption in step {}",
                        opened.join(", ")
                    );
                    st.open_assumptions.clear();
                }
            }
            Some(ReasonKind::Induction) => {
                if st.base_case && st.inductive_step {
                    c.detail = "induction: base case and inductive step are both present".into();
                    st.base_case = false;
                    st.inductive_step = false;
                } else if let Some(detail) = inferred_induction(step, st, src) {
                    c.detail = detail;
                } else {
                    c.status = StepStatus::Gap;
                    c.detail = "induction needs a step labelled `Base case:` and one labelled `Inductive step:`".into();
                }
            }
            Some(ReasonKind::Cases) => {
                st.open_assumptions.clear();
                c.detail = "the cases are closed".into();
            }
            Some(ReasonKind::Reference) => {
                let (status, detail) = claims_status(&maths, st, src, &mut c, true);
                c.status = status.unwrap_or(StepStatus::Structure);
                let names = if c.uses.is_empty() {
                    "earlier steps".to_string()
                } else {
                    c.uses.join(", ")
                };
                c.detail = detail.unwrap_or_else(|| format!("follows from {names}"));
            }
            Some(ReasonKind::Rule) => {
                let rule = step
                    .reason
                    .as_ref()
                    .map_or(String::new(), |r| r.text.clone());
                if maths.claims.is_empty() {
                    c.status = StepStatus::Structure;
                    c.detail = format!("by {rule}: a prose step with no relation to compute");
                    c.prose = true;
                } else {
                    let (status, detail) = claims_status(&maths, st, src, &mut c, false);
                    c.status = status.unwrap_or(StepStatus::Structure);
                    c.detail = detail.unwrap_or_else(|| format!("by {rule}"));
                }
            }
        }
    }
    // Inequalities the step establishes or assumes, for later steps (a discriminant).
    for (links, hyp) in &maths.chains {
        let established = *hyp
            || links.iter().all(|l| {
                c.claims.iter().any(|cc| {
                    cc.span == l.span
                        && matches!(
                            cc.outcome,
                            ClaimOutcome::Proved
                                | ClaimOutcome::ProvedBy(_)
                                | ClaimOutcome::Numeric(_)
                        )
                })
            });
        if established && !links.is_empty() {
            record_inequalities(&mut st.k, links, src);
        }
    }
    // The step's own relations are known from here on (claims the CAS proved are
    // identities and add nothing; `claims_status` records the others as it goes).
    if reason_kind != Some(ReasonKind::Assumption) && c.claims.is_empty() {
        for (l, _) in &maths.claims {
            add_fact(&mut st.k, l, src);
        }
    }
    // Nested steps (cases) cap the step's status.
    for sub in &step.substeps {
        let sc = check_step(sub, st, ctx, src);
        if sc.status.strength() < c.status.strength() {
            c.status = sc.status;
            c.detail = format!("a case is weaker: {}", sc.detail);
        }
    }
    st.overrides.clear();
    if reason_kind == Some(ReasonKind::Construction) {
        st.constructed = true;
    }
    if let Some(n) = step.number {
        st.statuses.insert(n, c.status);
        if c.status.is_computed() {
            for (l, _) in &maths.claims {
                for side in [&l.lhs, &l.rhs] {
                    if let Some(t) = formal_text(side, src) {
                        st.terms.push((n, t));
                    }
                }
            }
        }
    }
    c
}

/// An induction written without `Base case:` and `Inductive step:` labels ("Repeating,
/// stage $j$ adds $4^{-j} T$ — by induction on $j$"), read from the steps before it: an
/// expression in the induction variable must equal, at its first two values, quantities
/// that two earlier checked steps established, in order (the base case and the first
/// step). The general step from `j` to `j + 1` is not checked, so the step is structure
/// (◇) at best, and the detail says so.
fn inferred_induction(step: &ProofStep, st: &ProofState<'_>, src: &str) -> Option<String> {
    let reason = step.reason.as_ref()?;
    let mut words = reason.text.split_whitespace();
    words.find(|w| *w == "on")?;
    let var = words.next()?.trim_matches(['$', '.', ',']).to_string();
    if var.is_empty() {
        return None;
    }
    let me = step.number?;
    // Expressions in the claim that use the variable.
    let mut exprs: Vec<String> = Vec::new();
    for i in &step.claim {
        let InlineKind::Math(m) = &i.kind else {
            continue;
        };
        for line in &m.ast.lines {
            if let Statement::Relation(n) = &line.statement {
                if !matches!(n.ungrouped().kind, NodeKind::Relation { .. })
                    && notation::names(n, src).contains(&var)
                {
                    exprs.extend(formal_text(n, src));
                }
            }
        }
    }
    // A step's quantities, when the step was checked by computation.
    let checked: Vec<&(u32, String)> = st
        .terms
        .iter()
        .filter(|(n, t)| {
            *n < me
                && st.statuses.get(n).is_some_and(|s| s.is_computed())
                && t.trim().parse::<f64>().is_err()
        })
        .collect();
    let matching = |e: &str, at: i64| -> Option<(u32, String)> {
        checked.iter().find_map(|(n, t)| {
            let mut names = BTreeSet::new();
            for x in [e, t.as_str()] {
                let (node, _) = mpp_syntax::math::parse_expr(x, 0);
                names.extend(notation::names(&node, x));
            }
            names.remove(&var);
            let mut defs = vec![Def::new(var.clone(), at.to_string())];
            defs.extend(st.k.defs_for(&names));
            let line = format!("\\left({e}\\right) \\overset{{?}}{{=}} \\left({t}\\right)");
            matches!(
                crate::cas::run_one(&defs, line),
                crate::cas::LineOut::Verdict(mpp_eval::Verdict::Proved)
            )
            .then(|| (*n, t.clone()))
        })
    };
    for e in &exprs {
        for first in [0i64, 1] {
            let (Some((b, bt)), Some((s, stx))) = (matching(e, first), matching(e, first + 1))
            else {
                continue;
            };
            if b < s {
                return Some(format!(
                    "induction on ${var}$ read from the steps before: ${var} = {first}$ gives ${bt}$ (step {b}) and ${var} = {}$ gives ${stx}$ (step {s}), both checked; the step from ${var}$ to ${var} + 1$ in general is not checked and is taken as structure",
                    first + 1
                ));
            }
        }
    }
    None
}

/// Records the inequalities a chain gives (`0 \le S = Q` gives `0 ≤ S` and `0 ≤ Q`): every
/// pair of terms joined only by `=` and `≤` (or `<`), or only by `=` and `≥` (or `>`), with
/// at least one inequality between them. A link the checker cannot test breaks the chain.
fn record_inequalities(k: &mut Knowledge, links: &[Link], src: &str) {
    let mut terms: Vec<&Node> = Vec::new();
    let mut ops: Vec<ClaimOp> = Vec::new();
    let mut runs: Vec<(Vec<&Node>, Vec<ClaimOp>)> = Vec::new();
    for l in links {
        let joined = terms.last().is_some_and(|t| t.span == l.lhs.span);
        if !joined && !terms.is_empty() {
            runs.push((std::mem::take(&mut terms), std::mem::take(&mut ops)));
        }
        if terms.is_empty() {
            terms.push(&l.lhs);
        }
        terms.push(&l.rhs);
        ops.push(l.op);
    }
    runs.push((terms, ops));
    for (terms, ops) in runs {
        for i in 0..terms.len() {
            for j in i + 1..terms.len() {
                let between = &ops[i..j];
                let up = between
                    .iter()
                    .all(|o| matches!(o, ClaimOp::Eq | ClaimOp::Le | ClaimOp::Lt));
                let down = between
                    .iter()
                    .all(|o| matches!(o, ClaimOp::Eq | ClaimOp::Ge | ClaimOp::Gt));
                let strict = between.iter().any(|o| *o != ClaimOp::Eq);
                let (small, big) = match (up, down) {
                    (true, false) if strict => (terms[i], terms[j]),
                    (false, true) if strict => (terms[j], terms[i]),
                    _ => continue,
                };
                let (Some(s), Some(b)) = (formal_text(small, src), formal_text(big, src)) else {
                    continue;
                };
                let mut names = notation::names(small, src);
                names.extend(notation::names(big, src));
                k.le_facts.push((s, b, names));
            }
        }
    }
}

fn keys_text(keys: &[String]) -> String {
    if keys.is_empty() {
        String::new()
    } else {
        let k: Vec<String> = keys.iter().map(|k| format!("[@{k}]")).collect();
        format!(" {}", k.join(", "))
    }
}

/// Records a relation as something known: an equation (for elimination) and a condition
/// (for filtering sample points).
fn add_fact(k: &mut Knowledge, l: &Link, src: &str) {
    let Some(cond) = condition(l, src) else {
        return;
    };
    if l.op == ClaimOp::Eq {
        let lone = |n: &Node| -> Option<String> {
            matches!(n.ungrouped().kind, NodeKind::Name(_))
                .then(|| notation::node_text(n, src).trim().to_string())
        };
        let defined: BTreeSet<&String> = k.defs.iter().map(|(d, _)| &d.name).collect();
        let defines = lone(&l.lhs)
            .filter(|n| !defined.contains(n) && !notation::names(&l.rhs, src).contains(n))
            .or_else(|| {
                lone(&l.rhs)
                    .filter(|n| !defined.contains(n) && !notation::names(&l.lhs, src).contains(n))
            });
        k.equations.push(Equation {
            lhs: cond.lhs.clone(),
            rhs: cond.rhs.clone(),
            names: cond.names.clone(),
            defines,
        });
    }
    k.conditions.push(cond);
}

/// Checks the step's claims. Returns the status they give (None when there are none)
/// and a detail sentence. With `lenient`, a claim the CAS cannot settle does not make the
/// step a gap (the reason, not the computation, justifies it), and nothing becomes ✗.
fn claims_status(
    maths: &StepMaths,
    st: &mut ProofState<'_>,
    src: &str,
    c: &mut StepCheck,
    lenient: bool,
) -> (Option<StepStatus>, Option<String>) {
    if maths.claims.is_empty() {
        return (None, None);
    }
    let mut worst: Option<StepStatus> = None;
    let mut detail: Option<String> = None;
    let mut numeric_how: Option<String> = None;
    let mut informal: Option<String> = None;
    let mut proved_by: Vec<String> = Vec::new();
    for (link, text) in &maths.claims {
        let outcome = check_link(link, st, src);
        // An identity the CAS proved adds nothing; a fact proved by a rule (an area, a
        // chord's line, a term of a zero sum) is known from here on.
        if !matches!(outcome, ClaimOutcome::Proved | ClaimOutcome::Numeric(_)) {
            add_fact(&mut st.k, link, src);
        }
        c.claims.push(ClaimCheck {
            source: text.trim().to_string(),
            span: link.span,
            outcome: outcome.clone(),
        });
        let status = match &outcome {
            ClaimOutcome::Proved => StepStatus::Symbolic,
            ClaimOutcome::ProvedBy(rule) => {
                if !proved_by.contains(rule) {
                    proved_by.push(rule.clone());
                }
                StepStatus::Symbolic
            }
            ClaimOutcome::Numeric(how) => {
                numeric_how.get_or_insert_with(|| how.clone());
                StepStatus::Numeric
            }
            ClaimOutcome::Failed(at) if !lenient => {
                c.counterexample = Some(at.clone());
                detail = Some(format!("wrong: ${}$ fails {at}", text.trim()));
                StepStatus::Wrong
            }
            ClaimOutcome::Informal(why) => {
                informal.get_or_insert_with(|| why.clone());
                StepStatus::Structure
            }
            ClaimOutcome::Failed(_) | ClaimOutcome::Unverified(_) if lenient => {
                StepStatus::Structure
            }
            ClaimOutcome::Unverified(why) | ClaimOutcome::Failed(why) => {
                if detail.is_none() {
                    detail = Some(format!("not checked: ${}$: {why}", text.trim()));
                }
                StepStatus::Gap
            }
        };
        worst = Some(match worst {
            Some(w) if w.strength() <= status.strength() => w,
            _ => status,
        });
    }
    let worst = worst.unwrap_or(StepStatus::Structure);
    let detail = detail.or_else(|| match worst {
        StepStatus::Symbolic => Some(if proved_by.is_empty() {
            "checked symbolically by the CAS".into()
        } else {
            format!("checked symbolically: {}", proved_by.join("; "))
        }),
        StepStatus::Numeric => Some(format!(
            "checked numerically: {}",
            numeric_how.clone().unwrap_or_default()
        )),
        StepStatus::Structure => informal.map(|w| {
            let computed = c.claims.iter().any(|cc| {
                matches!(
                    cc.outcome,
                    ClaimOutcome::Proved | ClaimOutcome::ProvedBy(_) | ClaimOutcome::Numeric(_)
                )
            });
            if computed {
                format!("structure only: {w}; the step's other relations are checked")
            } else {
                format!("structure only: {w}")
            }
        }),
        _ => None,
    });
    if worst == StepStatus::Structure && detail.is_none() {
        return (Some(worst), None);
    }
    (Some(worst), detail)
}

/// Checks one relation: on fields when it talks about fields, otherwise with the CAS.
fn check_link(link: &Link, st: &ProofState<'_>, src: &str) -> ClaimOutcome {
    if let Some(o) = st.overrides.get(&(link.span.start, link.span.end)) {
        return o.clone();
    }
    if st.fields.mentions(&link.lhs, src) || st.fields.mentions(&link.rhs, src) {
        if link.op != ClaimOp::Eq {
            return ClaimOutcome::Unverified("only equations between fields are checked".into());
        }
        return st.fields.check(&link.lhs, &link.rhs, src);
    }
    let mut names = notation::names(&link.lhs, src);
    names.extend(notation::names(&link.rhs, src));
    let sides = (formal_text(&link.lhs, src), formal_text(&link.rhs, src));
    // An inequality about a discriminant, from an earlier quadratic that is never negative.
    if let (Some(lhs), Some(rhs)) = &sides {
        if let Some(o) = algebra::discriminant_rule(lhs, link.op, rhs, &names, &st.k) {
            return o;
        }
    }
    // Sums over an indexed family (`\sum a_i b_i`), which the CAS reads as one symbol.
    let reach = st.k.closure(&names);
    if notation::has_indexed_family(&link.lhs, src)
        || notation::has_indexed_family(&link.rhs, src)
        || reach.iter().any(|n| st.k.indexed.contains(n))
        || (!st.k.indexed.is_empty() && names.iter().any(|n| indexed::is_template(n)))
    {
        let (Some(lhs), Some(rhs)) = &sides else {
            return ClaimOutcome::Informal(
                "it is written with an ellipsis (⋯), which is read but not computed".into(),
            );
        };
        if link.modulus.is_some() || matches!(link.op, ClaimOp::Congruent | ClaimOp::Divides) {
            return ClaimOutcome::Unverified(
                "congruences about sums over an indexed family are not checked".into(),
            );
        }
        return indexed::check(
            &indexed::FamilyClaim {
                lhs,
                op: link.op,
                rhs,
                names: &names,
            },
            &st.k,
        );
    }
    let defs = st.k.defs.iter().filter(|(d, _)| reach.contains(&d.name));
    for (d, _) in defs {
        if d.body.contains("\\dots") || d.body.contains("\\cdots") {
            return ClaimOutcome::Informal(format!(
                "it uses ${}$, defined with an ellipsis the CAS cannot compute",
                d.name
            ));
        }
    }
    if link.op == ClaimOp::Eq {
        if let Some(poly) = notation::truncated_series(&link.rhs) {
            return algebra::check_series(
                &notation::side_text(&link.lhs, src),
                &notation::side_text(poly, src),
                &st.k,
                &names,
            );
        }
    }
    let (Some(lhs), Some(rhs)) = (formal_text(&link.lhs, src), formal_text(&link.rhs, src)) else {
        return ClaimOutcome::Informal(
            "it is written with an ellipsis (⋯), which is read but not computed".into(),
        );
    };
    let claim = Claim {
        lhs,
        op: link.op,
        rhs,
        modulus: link.modulus.clone(),
        names,
    };
    algebra::check_claim(&claim, &st.k)
}

// ---- summaries ---------------------------------------------------------------

/// A proof's own summary, from its steps and issues.
fn summarise(steps: &[StepCheck], issues: &[String]) -> Summary {
    let mut cited = BTreeSet::new();
    let mut external = BTreeSet::new();
    let mut unnamed_external = 0usize;
    let mut gaps = issues.len();
    let mut wrong = false;
    let mut computed = false;
    for s in steps {
        match s.status {
            StepStatus::Wrong => wrong = true,
            StepStatus::Gap => gaps += 1,
            StepStatus::External => {
                if s.keys.is_empty() {
                    unnamed_external += 1;
                }
                external.extend(s.keys.iter().cloned());
            }
            StepStatus::Cited => cited.extend(s.keys.iter().cloned()),
            st if st.is_computed() => computed = true,
            _ => {}
        }
    }
    let cited: Vec<String> = cited.into_iter().collect();
    let mut external: Vec<String> = external.into_iter().collect();
    for i in 0..unnamed_external {
        external.push(format!("(unnamed {})", i + 1));
    }
    let verdict = verdict(wrong, external.len(), gaps, cited.len(), !computed);
    Summary {
        text: verdict.text(),
        verdict,
        cited,
        external,
        gaps,
        structure_only: !computed,
    }
}

fn verdict(
    wrong: bool,
    external: usize,
    gaps: usize,
    cited: usize,
    structure_only: bool,
) -> Verdict {
    if wrong {
        Verdict::Wrong
    } else if external > 0 {
        Verdict::NotProvedHere(external)
    } else if gaps > 0 {
        Verdict::Gaps(gaps)
    } else if cited > 0 {
        Verdict::ProvedUsingCited(cited)
    } else if structure_only {
        Verdict::ProvedStructureOnly
    } else {
        Verdict::Proved
    }
}

/// A result's summary through the results it rests on (the strongest of its proofs).
/// A cycle of results resting on each other makes each a gap.
fn effective(
    i: usize,
    raw: &[RawResult],
    names: &[String],
    memo: &mut HashMap<usize, Summary>,
    stack: &mut Vec<usize>,
) -> Summary {
    if let Some(s) = memo.get(&i) {
        return s.clone();
    }
    let Some(r) = raw.get(i) else {
        return no_proof(TheoremKind::Theorem);
    };
    if r.proofs.is_empty() {
        let s = r.check.summary.clone();
        memo.insert(i, s.clone());
        return s;
    }
    stack.push(i);
    let mut candidates = Vec::new();
    for p in &r.proofs {
        let mut cited: BTreeSet<String> = p.own.cited.iter().cloned().collect();
        let mut external: BTreeSet<String> = p.own.external.iter().cloned().collect();
        let mut gaps = p.own.gaps;
        let mut wrong = p.own.verdict == Verdict::Wrong;
        let mut structure_only = p.own.structure_only;
        let mut notes: Vec<String> = Vec::new();
        for &u in &p.uses {
            let name = names.get(u).cloned().unwrap_or_default();
            if stack.contains(&u) {
                gaps += 1;
                notes.push(format!(
                    "circular: rests on {name}, which rests on this result"
                ));
                continue;
            }
            let s = effective(u, raw, names, memo, stack);
            match s.verdict {
                Verdict::Wrong => {
                    gaps += 1;
                    notes.push(format!("rests on {name}, which has a wrong step"));
                }
                Verdict::NoProof => {
                    gaps += 1;
                    notes.push(format!("rests on {name}, which has no proof"));
                }
                _ => {
                    if s.gaps > 0 {
                        notes.push(format!("{} in {name}", plural(s.gaps, "gap")));
                    }
                    if !s.external.is_empty() {
                        notes.push(format!("{name} rests on an external result"));
                    }
                }
            }
            gaps += s.gaps;
            cited.extend(s.cited.iter().cloned());
            external.extend(s.external.iter().cloned());
            structure_only &= s.structure_only;
        }
        if p.own.verdict == Verdict::Wrong {
            wrong = true;
        }
        let v = verdict(wrong, external.len(), gaps, cited.len(), structure_only);
        let mut text = v.text();
        if !notes.is_empty() {
            text.push_str(&format!(" ({})", notes.join("; ")));
        }
        candidates.push(Summary {
            verdict: v,
            text,
            cited: cited.into_iter().collect(),
            external: external.into_iter().collect(),
            gaps,
            structure_only,
        });
    }
    stack.pop();
    let s = best(candidates.into_iter());
    memo.insert(i, s.clone());
    s
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// The result names used by a document's proofs, for tests and tools.
pub fn uses_map(check: &DocumentCheck) -> BTreeMap<String, Vec<String>> {
    check
        .results
        .iter()
        .map(|r| (r.name.clone(), r.uses.clone()))
        .collect()
}

/// Where each result sits in a document: its name, the span of its heading and
/// statement, and the span of its first proof.
pub fn result_spans(doc: &Document, src: &str) -> Vec<(String, Span, Option<Span>)> {
    DocContext::read(doc, src, &[])
        .items
        .iter()
        .map(|i| (i.name.clone(), i.span, i.proofs.first().map(|(_, s)| *s)))
        .collect()
}
