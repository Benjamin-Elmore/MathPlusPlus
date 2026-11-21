//! The checker: per-step statuses, the summary rule, and the ways a proof can be fooled.

use mpp_proof::{check_source, StepStatus, Verdict};

fn statuses(src: &str, result: &str) -> Vec<StepStatus> {
    let c = check_source(src);
    let r = c
        .result(result)
        .unwrap_or_else(|| panic!("no result {result}: {}", c.report()));
    r.proofs[0].steps.iter().map(|s| s.status).collect()
}

fn verdict(src: &str, result: &str) -> Verdict {
    let c = check_source(src);
    c.result(result)
        .unwrap_or_else(|| panic!("no result {result}: {}", c.report()))
        .summary
        .verdict
        .clone()
}

#[test]
fn algebra_steps_are_checked_and_a_false_one_is_wrong() {
    let src = "**Theorem 1.** For every integer $m$, things hold.\n\n**Proof.**\n\
               1. For every real $m$: $(2m+1)^2 = 4m^2 + 4m + 1$ — by algebra.\n\
               2. For every real $m$: $(m+1)^2 = m^2 + 1$ — by algebra.\n∎\n";
    let st = statuses(src, "Theorem 1");
    assert_eq!(st, vec![StepStatus::Symbolic, StepStatus::Wrong]);
    let c = check_source(src);
    let step = &c.results[0].proofs[0].steps[1];
    assert!(step.counterexample.is_some());
    assert_eq!(c.results[0].summary.verdict, Verdict::Wrong);
}

#[test]
fn a_claim_that_needs_unknown_facts_is_a_gap_not_wrong() {
    // c is the hypotenuse, which the checker cannot know: unverified, never ✗.
    let src =
        "**Theorem 1.** In a right triangle with legs $a, b$ and hypotenuse $c$, it holds.\n\n\
               **Proof.**\n1. $(a + b)^2 = c^2 + 2ab$ — by algebra.\n∎\n";
    assert_eq!(statuses(src, "Theorem 1"), vec![StepStatus::Gap]);
    assert_eq!(verdict(src, "Theorem 1"), Verdict::Gaps(1));
}

#[test]
fn hypotheses_are_used_by_later_steps() {
    let src = "**Theorem 1.** $\\sqrt 2$ is irrational.\n\n**Proof.**\n\
               1. Suppose $\\sqrt 2 = p / q$ — by assumption.\n\
               2. Then $p^2 = 2 q^2$ — by algebra.\n\
               3. So it fails — by contradiction.\n∎\n";
    assert_eq!(
        statuses(src, "Theorem 1"),
        vec![
            StepStatus::Structure,
            StepStatus::Symbolic,
            StepStatus::Structure
        ]
    );
    assert_eq!(verdict(src, "Theorem 1"), Verdict::Proved);
}

#[test]
fn summary_never_claims_more_than_the_weakest_step() {
    let src = "## References\n- [@k] A book.\n\n\
               **Theorem 1.** It holds.\n\n**Proof.**\n\
               1. $1 + 1 = 2$ — by algebra.\n\
               2. It is known — *(cited: a fact [@k])*.\n∎\n\n\
               **Theorem 2.** It holds.\n\n**Proof.**\n\
               1. $1 + 1 = 2$ — by algebra.\n\
               2. Something — *(gap: not shown)*.\n∎\n\n\
               **Theorem 3.** It holds.\n\n**Proof.**\n\
               1. Somebody says so — *(external: [@k])*.\n\
               2. $1 + 1 = 2$ — by algebra.\n∎\n\n\
               **Theorem 4.** It holds.\n\n**Proof.**\n\
               1. Suppose it fails — by assumption.\n\
               2. Then it holds — by contradiction.\n∎\n";
    assert_eq!(verdict(src, "Theorem 1"), Verdict::ProvedUsingCited(1));
    assert_eq!(verdict(src, "Theorem 2"), Verdict::Gaps(1));
    assert_eq!(verdict(src, "Theorem 3"), Verdict::NotProvedHere(1));
    assert_eq!(verdict(src, "Theorem 4"), Verdict::ProvedStructureOnly);
    let c = check_source(src);
    assert_eq!(
        c.result("Theorem 1").map(|r| r.summary.text.as_str()),
        Some("proved, using 1 cited result")
    );
    assert_eq!(
        c.result("Theorem 3").map(|r| r.summary.text.as_str()),
        Some("not proved here: rests on 1 external, unverified result")
    );
}

#[test]
fn a_deleted_citation_turns_the_step_into_a_gap() {
    let with = "**Theorem 1.** It holds.\n\n**Proof.**\n1. It is known — *(cited: a fact [@k])*.\n∎\n\n## References\n- [@k] A book.\n";
    let without =
        "**Theorem 1.** It holds.\n\n**Proof.**\n1. It is known — *(cited: a fact [@k])*.\n∎\n";
    let unnamed =
        "**Theorem 1.** It holds.\n\n**Proof.**\n1. It is known — *(cited: a fact)*.\n∎\n";
    assert_eq!(statuses(with, "Theorem 1"), vec![StepStatus::Cited]);
    assert_eq!(statuses(without, "Theorem 1"), vec![StepStatus::Gap]);
    assert_eq!(statuses(unnamed, "Theorem 1"), vec![StepStatus::Gap]);
}

#[test]
fn circular_references_are_gaps() {
    let self_ref = "**Theorem 1.** It holds.\n\n**Proof.**\n1. It holds — by Theorem 1.\n∎\n";
    assert_eq!(statuses(self_ref, "Theorem 1"), vec![StepStatus::Gap]);
    let cycle = "**Lemma 1.** A.\n\n**Proof.**\n1. A holds — by Theorem 2.\n∎\n\n\
                 **Theorem 2.** B.\n\n**Proof.**\n1. B holds — by Lemma 1.\n∎\n";
    let c = check_source(cycle);
    for r in &c.results {
        assert!(
            !r.summary.verdict.is_proved(),
            "{} must not be proved: {}",
            r.name,
            r.summary.text
        );
    }
    let forward_step = "**Theorem 1.** It holds.\n\n**Proof.**\n1. It holds — by step 2.\n2. $1 = 1$ — by algebra.\n∎\n";
    assert_eq!(statuses(forward_step, "Theorem 1")[0], StepStatus::Gap);
}

#[test]
fn a_lemma_weakness_is_inherited() {
    let src = "**Lemma 1.** A.\n\n**Proof.**\n1. A holds — *(gap: not shown)*.\n∎\n\n\
               **Theorem 2.** B.\n\n**Proof.**\n1. $1 + 1 = 2$ — by algebra.\n2. B holds — by Lemma 1.\n∎\n";
    assert_eq!(verdict(src, "Lemma 1"), Verdict::Gaps(1));
    let v = verdict(src, "Theorem 2");
    assert_eq!(v, Verdict::Gaps(1));
}

#[test]
fn assumptions_must_be_discharged_and_contradictions_need_one() {
    let open = "**Theorem 1.** It holds.\n\n**Proof.**\n1. Suppose $x = 1$ — by assumption.\n2. $x + 1 = 2$ — by algebra.\n∎\n";
    let c = check_source(open);
    assert_eq!(c.results[0].proofs[0].issues.len(), 1);
    assert_eq!(c.results[0].summary.verdict, Verdict::Gaps(1));
    let nothing = "**Theorem 1.** It holds.\n\n**Proof.**\n1. It holds — by contradiction.\n∎\n";
    assert_eq!(statuses(nothing, "Theorem 1"), vec![StepStatus::Gap]);
}

#[test]
fn induction_needs_base_case_and_step() {
    let ok = "**Theorem 1.** For every integer $n \\ge 1$, $\\sum_{k=1}^{n} k = \\frac{n(n+1)}{2}$.\n\n**Proof.**\n\
              1. Base case: $\\sum_{k=1}^{1} k = 1$ — by algebra.\n\
              2. Inductive step: if $\\sum_{k=1}^{n} k = \\frac{n(n+1)}{2}$, then $\\sum_{k=1}^{n+1} k = \\frac{(n+1)(n+2)}{2}$ — by algebra.\n\
              3. So it holds for every $n$ — by induction.\n∎\n";
    assert_eq!(verdict(ok, "Theorem 1"), Verdict::Proved);
    let bad = "**Theorem 1.** It holds.\n\n**Proof.**\n1. So it holds — by induction.\n∎\n";
    assert_eq!(statuses(bad, "Theorem 1"), vec![StepStatus::Gap]);
}

#[test]
fn a_step_without_a_reason_is_a_gap_and_prose_proofs_are_not_proved() {
    let src = "**Theorem 1.** It holds.\n\n**Proof.**\n1. $x = x$.\n∎\n";
    assert_eq!(statuses(src, "Theorem 1"), vec![StepStatus::Gap]);
    let prose = "**Theorem 1.** It holds.\n\n**Proof.** Obvious. ∎\n";
    assert_eq!(verdict(prose, "Theorem 1"), Verdict::Gaps(1));
}

#[test]
fn conjectures_and_unproved_theorems() {
    let src = "**Conjecture 1.** Every even number above 2 is a sum of two primes.\n\n**Theorem 2.** Something.\n";
    let c = check_source(src);
    assert_eq!(c.results[0].summary.text, "conjecture: not proved");
    assert_eq!(c.results[1].summary.verdict, Verdict::NoProof);
}

#[test]
fn field_identities_are_checked_on_an_instance() {
    let src = "Let $\\mathbf u(\\mathbf x, t) \\in \\mathbb R^3$ be a velocity field.\n\n\
               $$ \\nabla \\cdot \\mathbf u = 0 \\label{div} $$\n\n\
               **Lemma 1.** For a field as in \\eqref{div}, identities hold.\n\n**Proof.**\n\
               1. $(\\mathbf u \\cdot \\nabla)\\mathbf u = \\nabla\\big(\\tfrac12 |\\mathbf u|^2\\big) - \\mathbf u \\times (\\nabla \\times \\mathbf u)$ — by vector identities.\n\
               2. $\\nabla \\cdot (\\nabla \\times \\mathbf u) = 0$ — by vector identities.\n\
               3. $(\\mathbf u \\cdot \\nabla)\\mathbf u = \\nabla\\big(\\tfrac12 |\\mathbf u|^2\\big)$ — by vector identities.\n∎\n";
    let st = statuses(src, "Lemma 1");
    assert_eq!(st[0], StepStatus::Numeric);
    assert_eq!(st[1], StepStatus::Numeric);
    assert_eq!(st[2], StepStatus::Wrong, "{}", check_source(src).report());
}

#[test]
fn checking_never_changes_the_file() {
    let src = "**Theorem 1.** It holds.\n\n**Proof.**\n1. $1 + 1 = 2$ — by algebra.\n∎\n";
    let copy = src.to_string();
    let _ = check_source(src);
    assert_eq!(src, copy);
}

#[test]
fn congruence_checks_in_steps_stay_congruences() {
    // Read as an equation, `3^{12} = 1` would be ✗; as a congruence mod 13 it holds.
    let src = "**Theorem 1.** It holds.\n\n**Proof.**\n\
               1. $3^{12} \\overset{?}{\\equiv} 1 \\pmod{13}$ — by algebra.\n\
               2. For an integer $m$: $3^{12} \\overset{?}{\\equiv} 2 \\pmod{13}$ — by algebra.\n∎\n";
    let c = check_source(src);
    let steps = &c.results[0].proofs[0].steps;
    assert!(steps[0].status.is_computed(), "{}", c.report());
    assert_eq!(steps[1].status, StepStatus::Wrong, "{}", c.report());
}

#[test]
fn sums_over_indexed_families() {
    let src = "**Theorem 1.** For real numbers $a_1, \\dots, a_n$ and $b_1, \\dots, b_n$, it holds.\n\n\
               **Proof.**\n\
               1. Let $A := \\sum a_i^2$, $C := \\sum a_i b_i$. For every real $x$: $\\sum_{i=1}^{n} a_i (a_i x + b_i) = A x + C$ — by algebra.\n\
               2. For every real $x$: $\\sum_{i=1}^{n} a_i (a_i x + b_i) = A x + 2 C$ — by algebra.\n\
               3. $\\left(\\sum_{i=1}^{n} a_i b_i\\right)^2 \\le \\left(\\sum_{i=1}^{n} a_i^2\\right)\\left(\\sum_{i=1}^{n} b_i^2\\right)$ — by algebra.\n∎\n";
    let c = check_source(src);
    let steps = &c.results[0].proofs[0].steps;
    // Linearity proves the first for every n; the second is not proved; the third is only
    // evidence (random families), so ≈ and never ✓.
    assert_eq!(steps[0].status, StepStatus::Symbolic, "{}", c.report());
    assert!(!steps[1].status.is_computed(), "{}", c.report());
    assert_eq!(steps[2].status, StepStatus::Numeric, "{}", c.report());
}

#[test]
fn coordinate_geometry_is_checked_and_figures_are_structure() {
    let setup =
        "Take the parabola $y = x^2$ and the chord between $x = a$ and $x = b$, with $a < b$.\n\n\
                 **Theorem 1.** It holds.\n\n**Proof.**\n";
    let good = format!(
        "{setup}1. Let $h := b - a$. The chord is $y = (a + b)x - ab$ — by algebra.\n\
         2. The triangle with vertices on the parabola at $a$, $\\tfrac{{a+b}}{{2}}$ and $b$ has area $T = \\tfrac{{h^3}}{{8}}$ — by the determinant formula.\n∎\n"
    );
    assert_eq!(
        statuses(&good, "Theorem 1"),
        vec![StepStatus::Symbolic, StepStatus::Symbolic]
    );
    let bad = format!(
        "{setup}1. The chord is $y = (a + b)x + ab$ — by algebra.\n\
         2. The triangle with vertices on the parabola at $a$, $\\tfrac{{a+b}}{{2}}$ and $b$ has area $T = \\tfrac{{(b-a)^3}}{{6}}$ — by the determinant formula.\n∎\n"
    );
    let st = statuses(&bad, "Theorem 1");
    assert!(st.iter().all(|s| !s.is_computed()), "{st:?}");
    // An area count needs a construction before it, and is then only structure.
    let count = "**Theorem 1.** It holds.\n\n**Proof.**\n\
                 1. Place four copies of the triangle in a square of side $a + b$ — by construction.\n\
                 2. Counting area two ways: $(a + b)^2 = c^2 + 2ab$, so $a^2 + b^2 = c^2$ — by algebra.\n∎\n";
    assert_eq!(
        statuses(count, "Theorem 1"),
        vec![StepStatus::Structure, StepStatus::Structure]
    );
    let no_figure = "**Theorem 1.** It holds.\n\n**Proof.**\n\
                     1. Counting area two ways: $(a + b)^2 = c^2 + 2ab$ — by algebra.\n∎\n";
    assert_eq!(statuses(no_figure, "Theorem 1"), vec![StepStatus::Gap]);
}

#[test]
fn induction_read_from_earlier_steps_is_structure_at_best() {
    let src = "**Theorem 1.** It holds.\n\n**Proof.**\n\
               1. Let $T := 8$. Stage $0$ has area $T = 8$ — by algebra.\n\
               2. Stage $1$ has area $\\tfrac12 T = 4$ — by algebra.\n\
               3. Repeating, stage $j$ has area $2^{-j} T$ — by induction on $j$.\n∎\n";
    let c = check_source(src);
    let steps = &c.results[0].proofs[0].steps;
    assert_eq!(steps[2].status, StepStatus::Structure, "{}", c.report());
    // A pattern the earlier steps do not give stays a gap.
    let wrong = src.replace("2^{-j} T", "3^{-j} T");
    let c = check_source(&wrong);
    assert_eq!(
        c.results[0].proofs[0].steps[2].status,
        StepStatus::Gap,
        "{}",
        c.report()
    );
}
