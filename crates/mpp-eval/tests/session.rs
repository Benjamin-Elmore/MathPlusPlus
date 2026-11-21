//! Running whole documents through a session.

use mpp_core::Expr;
use mpp_eval::{
    BlockResult, DefinitionKind, Evaluated, LineKind, LineResult, NoImports, Session, Value,
};
use std::sync::Arc;

fn run(src: &str) -> Evaluated {
    let mut s = Session::new();
    s.run_str(src).clone()
}

/// The query and check results, in order.
fn answers(ev: &Evaluated) -> Vec<&LineResult> {
    ev.queries().map(|(_, l)| l).collect()
}

fn exact(line: &LineResult) -> String {
    line.plain.clone().unwrap_or_else(|| "<none>".into())
}

fn approx(line: &LineResult) -> String {
    line.approx.clone().unwrap_or_else(|| "<none>".into())
}

fn messages(ev: &Evaluated) -> Vec<String> {
    ev.all_diagnostics()
        .iter()
        .map(|d| d.message.clone())
        .collect()
}

fn no_errors(ev: &Evaluated) {
    let errors: Vec<String> = ev.errors().iter().map(|d| d.to_string()).collect();
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
}

#[test]
fn variables_and_exact_queries() {
    let ev =
        run("$$ a := 3 $$\n\n$$ b := a^2 + 1 $$\n\n$$ b = ? $$\n\n$$ \\frac{a}{6} + x = ? $$\n");
    no_errors(&ev);
    let a = answers(&ev);
    assert_eq!(exact(a[0]), "10");
    // Free symbols are fine in exact mode.
    assert_eq!(exact(a[1]), "x + 1/2");
}

#[test]
fn numeric_queries() {
    let ev = run("$$ \\sqrt{2} \\approx ? $$\n\n$$ \\sqrt{-4} \\approx ? $$\n\n$$ \\pi = ? $$\n");
    no_errors(&ev);
    let a = answers(&ev);
    assert_eq!(approx(a[0]), "1.414213562");
    assert_eq!(approx(a[1]), "2*i");
    assert_eq!(exact(a[2]), "π");
    assert_eq!(approx(a[2]), "3.141592654");
}

#[test]
fn unbound_symbol_in_a_numeric_query_is_an_error_naming_it() {
    let ev = run("$$ q + 1 \\approx ? $$\n");
    let errors = ev.errors();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains("`q`"), "{}", errors[0].message);
    // The error points at the name.
    assert_eq!(&ev.source()[errors[0].span.start..errors[0].span.end], "q");
}

#[test]
fn constants_cannot_be_redefined() {
    let src = "$$ g \\equiv 9.81 $$\n\n$$ g := 10 $$\n\n$$ g = ? $$\n";
    let ev = run(src);
    let errors = ev.errors();
    assert_eq!(errors.len(), 1);
    assert!(
        errors[0].message.contains("defined on line 1"),
        "{}",
        errors[0].message
    );
    assert_eq!(&src[errors[0].span.start..errors[0].span.end], "g");
    // Shown as typed: g ≡ 9.81.
    assert_eq!(exact(answers(&ev)[0]), "9.81");
}

#[test]
fn user_functions_and_arity() {
    let ev = run("$$ f(x, y) := x^2 + y $$\n\n$$ f(3, 1) = ? $$\n\n$$ f(2) = ? $$\n");
    let a = answers(&ev);
    assert_eq!(exact(a[0]), "10");
    let errors = ev.errors();
    assert_eq!(errors.len(), 1);
    assert!(
        errors[0].message.contains("takes 2 arguments, but got 1"),
        "{}",
        errors[0].message
    );
}

#[test]
fn unknown_function_with_several_arguments() {
    let ev = run("$$ h(1, 2) = ? $$\n");
    assert!(messages(&ev)
        .iter()
        .any(|m| m.contains("`h` is not a known function")));
}

#[test]
fn recompute_follows_later_definitions_only() {
    let src = "\
$$ m := 2 $$

$$ v := 3 $$

$$ E := \\tfrac12 m v^2 $$

$$ E = ? $$

$$ m := 4 $$

$$ E = ? $$
";
    let ev = run(src);
    no_errors(&ev);
    let a = answers(&ev);
    assert_eq!(exact(a[0]), "9");
    assert_eq!(exact(a[1]), "18");
    // The definition stores the formula, not a snapshot.
    let e = &ev.bindings["E"];
    assert!(e.describe().contains('m'), "{}", e.describe());
}

#[test]
fn shadowing_follows_reading_order() {
    let ev = run("$$ x := 1 $$\n\n$$ x = ? $$\n\n$$ x := 2 $$\n\n$$ x = ? $$\n\n$$ x := x + 1 $$\n\n$$ x = ? $$\n");
    no_errors(&ev);
    let got: Vec<String> = answers(&ev).into_iter().map(exact).collect();
    assert_eq!(got, ["1", "2", "3"]);
}

#[test]
fn self_reference_without_a_value_is_an_error() {
    let ev = run("$$ y := y + 1 $$\n");
    assert!(messages(&ev)
        .iter()
        .any(|m| m.contains("defined in terms of itself")));
}

#[test]
fn circular_definitions_are_reported() {
    let ev = run("$$ a := b $$\n\n$$ b := a $$\n\n$$ a = ? $$\n");
    assert!(
        messages(&ev)
            .iter()
            .any(|m| m.contains("circular") || m.contains("defined in terms of itself")),
        "{:?}",
        messages(&ev)
    );
}

#[test]
fn where_clauses_are_local_to_their_statement() {
    let src = "\
$$ A := \\pi r^2 $$

$$ A = ?, \\quad r = 3 $$

$$ A = ? $$

$$ r^2 + s = ?, \\; r = 2, \\; s = r $$
";
    let ev = run(src);
    no_errors(&ev);
    let a = answers(&ev);
    assert_eq!(exact(a[0]), "9*π");
    assert_eq!(exact(a[1]), "π*r^2");
    assert_eq!(exact(a[2]), "6");
}

#[test]
fn piecewise_functions() {
    let src = "\
$$ f(x) := \\begin{cases} x^2 & x \\ge 0 \\\\ -x & \\text{otherwise} \\end{cases} $$

$$ f(3) = ? $$

$$ f(-2) = ? $$

$$ g(x) := x, \\quad x > 1 \\\\ g(x) := 0, \\quad x \\le 1 $$

$$ g(5) = ? $$

$$ g(0) = ? $$
";
    let ev = run(src);
    no_errors(&ev);
    let got: Vec<String> = answers(&ev).into_iter().map(exact).collect();
    assert_eq!(got, ["9", "2", "5", "0"]);
}

#[test]
fn recursive_function_with_a_base_case() {
    let src = "$$ F(n) := \\begin{cases} 1 & n \\le 1 \\\\ n F(n - 1) & \\text{otherwise} \\end{cases} $$\n\n$$ F(10) = ? $$\n";
    let ev = run(src);
    no_errors(&ev);
    assert_eq!(exact(answers(&ev)[0]), "3628800");
}

#[test]
fn checks() {
    let src = "\
$$ \\sin^2 x + \\cos^2 x \\overset{?}{=} 1 $$

$$ (x + 1)^2 \\overset{?}{=} x^2 + 1 $$

$$ (a+b)^2 \\overset{?}{=} a^2 + 2ab + b^2 $$
";
    let ev = run(src);
    no_errors(&ev);
    let a = answers(&ev);
    let verdicts: Vec<Option<bool>> = a
        .iter()
        .map(|l| l.verdict.as_ref().and_then(|v| v.passed()))
        .collect();
    assert_eq!(verdicts, [Some(true), Some(false), Some(true)]);
    let fail = a[1].plain_answer().unwrap_or_default();
    assert!(fail.starts_with('✗') && fail.contains("x = "), "{fail}");
}

#[test]
fn call_against_product() {
    let src = "\
$$ a(b + 1) = ? $$

$$ f(x) := 2x $$

$$ f(b + 1) = ? $$

$$ a_n(x - c)^2 = ? $$
";
    let ev = run(src);
    no_errors(&ev);
    let a = answers(&ev);
    assert_eq!(exact(a[0]), "a*(b + 1)");
    assert_eq!(exact(a[1]), "2*(b + 1)");
    let expected = Expr::sym("a_n") * (Expr::sym("x") - Expr::sym("c")).pow(2);
    assert_eq!(a[2].value, Some(Value::Expr(expected)));
}

#[test]
fn initial_term_falls_back_to_a_plain_variable() {
    let ev = run("$$ v_0 := 20 $$\n\n$$ 2 v_0 = ? $$\n");
    no_errors(&ev);
    assert_eq!(exact(answers(&ev)[0]), "40");
    assert!(ev.bindings.contains_key("v_0"));
}

#[test]
fn recurrences_iterate() {
    let src = "\
$$ a_1 := 1 $$

$$ a_{n+1} := 2 a_n + 1 $$

$$ a_5 = ? $$

$$ F_1 := 1 \\\\ F_2 := 1 \\\\ F_{n+1} := F_n + F_{n-1} $$

$$ F_{20} = ? $$
";
    let ev = run(src);
    no_errors(&ev);
    let a = answers(&ev);
    assert_eq!(exact(a[0]), "31");
    assert_eq!(exact(a[1]), "6765");
    let kinds: Vec<DefinitionKind> = ev
        .blocks
        .iter()
        .flat_map(|b| b.values.iter())
        .filter_map(|l| match &l.kind {
            LineKind::Definition { kind, .. } => Some(*kind),
            _ => None,
        })
        .collect();
    assert_eq!(kinds[0], DefinitionKind::Initial);
    assert_eq!(kinds[1], DefinitionKind::Recurrence);
}

#[test]
fn let_sentences_define() {
    let ev = run("Let $a = 3$ and $b := 4$.\n\nThe sum is $a + b = ?$ and $c$ is free.\n");
    no_errors(&ev);
    assert_eq!(exact(answers(&ev)[0]), "7");
}

#[test]
fn inline_maths_only_evaluates_explicit_queries() {
    let ev = run("We know $x^2 = 4$ here, and $1 + 1 = ?$.\n");
    no_errors(&ev);
    let block = &ev.blocks[0];
    assert_eq!(block.values.len(), 2);
    assert_eq!(block.values[0].kind, LineKind::Relation);
    assert!(block.values[0].value.is_none());
    assert!(!block.values[0].auto_eval);
    assert_eq!(exact(&block.values[1]), "2");
}

#[test]
fn display_relations_are_recorded_for_auto_evaluation() {
    let ev = run("$$ x^2 + 1 $$\n\n%noeval\n$$ y + 1 $$\n");
    let lines: Vec<&LineResult> = ev.blocks.iter().flat_map(|b| b.values.iter()).collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].auto_eval);
    assert!(!lines[1].auto_eval && lines[1].noeval);
}

#[test]
fn e_and_i() {
    let ev = run(
        "$$ e^{i \\pi} = ? $$\n\n$$ \\sum_{i=1}^{4} i = ? $$\n\n$$ e := 2 $$\n\n$$ e + 1 = ? $$\n",
    );
    no_errors(&ev);
    let got: Vec<String> = answers(&ev).into_iter().map(exact).collect();
    assert_eq!(got, ["-1", "10", "3"]);
}

#[test]
fn steps_are_recorded() {
    let ev = run("$$ a := 2 $$\n\n$$ \\operatorname{expand}((x + a)^2) = ?! $$\n");
    no_errors(&ev);
    let (block, idx) = ev
        .blocks
        .iter()
        .find_map(|b| {
            b.values
                .iter()
                .position(|l| matches!(l.kind, LineKind::Query { steps: true, .. }))
                .map(|i| (b, i))
        })
        .expect("a steps query");
    let steps = block.steps_for(idx).expect("steps");
    assert!(steps.len() >= 2, "{steps:?}");
    assert_eq!(exact(&block.values[idx]), "x^2 + 4*x + 4");
}

#[test]
fn sliders_and_ranges() {
    let ev = run("$$ a := 2, \\; a \\in [0, 5] $$\n\nFigure: $a x^2$, $x \\in [-1, 1]$.\n");
    no_errors(&ev);
    let ranges: Vec<_> = ev.blocks.iter().flat_map(|b| b.ranges.iter()).collect();
    assert_eq!(ranges.len(), 2);
    assert_eq!(ranges[0].names, ["a"]);
    // The figure item has the slider value substituted and x kept symbolic.
    let fig = ev
        .blocks
        .iter()
        .flat_map(|b| b.values.iter())
        .find(|l| l.kind == LineKind::Figure)
        .expect("figure item");
    assert_eq!(exact(fig), "2*x^2");
}

#[test]
fn defines_and_uses() {
    let ev = run("$$ m := 2 $$\n\n$$ E := m c^2 $$\n\n$$ E = ? $$\n");
    let b: Vec<&BlockResult> = ev.blocks.iter().collect();
    assert_eq!(b[0].defines, ["m"]);
    assert_eq!(b[1].defines, ["E"]);
    assert!(b[1].uses.contains(&"m".to_string()));
    assert!(b[2].uses.contains(&"E".to_string()));
    assert_eq!(ev.graph.downstream(b[0].id), vec![b[1].id, b[2].id]);
    assert_eq!(ev.graph.definition_at("m", b[2].id), Some(b[0].id));
}

#[test]
fn incremental_update_recomputes_only_what_changed() {
    let before = "\
$$ m := 2 $$

$$ v := 3 $$

$$ E := \\tfrac12 m v^2 $$

$$ E = ? $$

$$ v = ? $$

Some prose.
";
    let mut s = Session::new();
    let ids: Vec<_> = s.run_str(before).blocks.iter().map(|b| b.id).collect();
    assert_eq!(s.evaluated().recomputed.len(), ids.len());

    // Same text again: nothing runs.
    assert!(s.update(before).recomputed.is_empty());

    // Change m: its block, the formula that uses it and the query on E run again.
    let after = before.replace("m := 2", "m := 4");
    let ev = s.update(&after);
    let new_ids: Vec<_> = ev.blocks.iter().map(|b| b.id).collect();
    assert_eq!(ev.recomputed, vec![new_ids[0], new_ids[2], new_ids[3]]);
    assert_eq!(exact(answers(ev)[0]), "18");

    // Edit prose at the top: blocks move but nothing is recomputed except the new block.
    let moved = format!("# Title\n\n{after}");
    let ev = s.update(&moved);
    assert_eq!(ev.recomputed.len(), 1);
    let q = answers(ev)[0];
    assert_eq!(&ev.source()[q.span.start..q.span.end], "E = ?");
    assert_eq!(exact(q), "18");
}

#[test]
fn incremental_update_on_a_new_definition_upstream() {
    let mut s = Session::new();
    s.run_str("$$ x + 1 = ? $$\n\n$$ y = ? $$\n");
    let ev = s.update("$$ x := 1 $$\n\n$$ x + 1 = ? $$\n\n$$ y = ? $$\n");
    // The new block and the query that used the (previously free) x.
    assert_eq!(ev.recomputed.len(), 2);
    assert_eq!(exact(answers(ev)[0]), "2");
}

#[test]
fn constant_error_moves_with_the_text() {
    let mut s = Session::new();
    s.run_str("$$ g \\equiv 1 $$\n\n$$ g := 2 $$\n");
    let ev = s.update("\n\n$$ g \\equiv 1 $$\n\n$$ g := 2 $$\n");
    assert!(
        ev.errors()[0].message.contains("line 3"),
        "{}",
        ev.errors()[0].message
    );
}

#[test]
fn calculus_queries_are_answered() {
    let ev = run(r"$$ \lim_{x\to 0} \frac{\sin x}{x} = ? $$

$$ \sum_{n=1}^{\infty} \frac{1}{n^2} = ? $$

$$ \int_0^1 x^2 \, dx \approx ? $$
");
    no_errors(&ev);
    let a = answers(&ev);
    assert_eq!(exact(a[0]), "1");
    assert_eq!(exact(a[1]), "π^2/6");
    assert_eq!(approx(a[2]), "0.3333333333");
    assert!(ev.all_diagnostics().is_empty());
}

#[test]
fn the_example_document_without_imports() {
    let src = "\
# Projectile motion
Let $g \\equiv 9.81$ be gravity and $v_0 := 20$ the launch speed.
Using $\\theta, R(\\theta)$ from [[launch.mpp]].
![[constants.mpp]]

$$ y(x) := x \\tan\\theta - \\frac{g x^2}{2 v_0^2 \\cos^2\\theta} $$
$$ E := \\tfrac12 m v_0^2 $$

$$ \\frac{d}{dx} y(x) = ? $$
$$ \\int_0^{R} y(x)\\,dx \\approx ? $$
$$ \\sum_{n=1}^{\\infty} \\frac{1}{n^2} = ? $$
$$ \\lim_{x\\to 0} \\frac{\\sin x}{x} = ? $$
$$ E = ? $$

Figure: $y(x)$, $x \\in [0, R]$.
Figure: $z = x^2 - y^2$, $x, y \\in [-2, 2]$.
";
    let ev = Session::new()
        .with_importer(Arc::new(NoImports))
        .run_str(src)
        .clone();
    no_errors(&ev);
    assert!(ev.bindings["g"].is_constant());
    assert!(ev.bindings.contains_key("v_0"));
    assert!(ev.bindings.contains_key("y"));
    let a = answers(&ev);
    assert_eq!(a.len(), 5);
    // The derivative holder has g and v_0 substituted.
    let d = exact(a[0]);
    assert!(d.contains("981") && !d.contains("v_0"), "{d}");
    assert_eq!(exact(a[4]), "200*m");
    let report = ev.report();
    assert!(report.contains("E = ?  = 200m"), "{report}");
    assert!(report.contains("warning"), "{report}");
}

#[test]
fn report_lists_queries_and_problems() {
    let ev = run("$$ 1 + 1 = ? $$\n\n$$ \\sqrt{2} \\approx ? $$\n\n$$ q \\approx ? $$\n");
    let report = ev.report();
    assert!(report.contains("   1: 1 + 1 = ?  = 2"), "{report}");
    assert!(report.contains("≈ 1.414213562"), "{report}");
    assert!(report.contains("error: `q` has no value"), "{report}");
}

#[test]
fn parse_errors_are_attached_to_their_block() {
    let ev = run("$$ x + $$\n\n$$ 2 = ? $$\n");
    assert!(ev.blocks[0].has_errors());
    assert!(!ev.blocks[1].has_errors());
    assert_eq!(exact(answers(&ev)[0]), "2");
}

#[test]
fn run_file_reads_from_disk() {
    let dir = std::env::temp_dir().join(format!("mpp-eval-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("doc.mpp");
    std::fs::write(&path, "$$ 6 \\cdot 7 = ? $$\n").expect("write");
    let mut s = Session::new();
    let ev = s.run_file(&path).expect("run");
    assert_eq!(exact(answers(ev)[0]), "42");
    assert!(s.run_file(dir.join("missing.mpp")).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sessions_run_on_another_thread() {
    let mut s = Session::new();
    let handle = std::thread::spawn(move || {
        s.run_str("$$ 2^{10} = ? $$\n");
        s
    });
    let s = handle.join().expect("thread");
    assert_eq!(exact(answers(s.evaluated())[0]), "1024");
}

#[test]
fn definitions_inside_a_proof_stay_in_the_proof() {
    let src = "$$ a := 2 $$\n\n**Theorem 1.** For every $n$, $n = n$.\n\n**Proof.**\n\n$$ N := 5 $$\n\n$$ a := 7 $$\n\n$$ N + a = ? $$\n\n∎\n\n$$ a = ? $$\n\n$$ N = ? $$\n";
    let ev = run(src);
    let lines = answers(&ev);
    assert_eq!(
        exact(lines[0]),
        "12",
        "inside the proof its names are known"
    );
    assert_eq!(exact(lines[1]), "2", "the proof's `a` does not leak");
    assert_eq!(exact(lines[2]), "N", "N is not defined after the proof");
}
