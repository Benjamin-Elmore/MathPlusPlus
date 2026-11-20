//! The example document from the queue, sentence-form helpers and figure items.

mod common;

use common::full;
use mpp_syntax::math::{
    parse_clause, parse_expr, parse_interval, parse_name_list, sexpr, Clause, DefHead, DefKind,
    NodeKind, QueryMode, Statement,
};

/// The maths of "What a .mpp file looks like" in the run state, in file order.
const QUEUE_EXAMPLE: &[&str] = &[
    r"g \equiv 9.81",
    r"v_0 := 20",
    r"\theta, R(\theta)",
    r"y(x) := x \tan\theta - \frac{g x^2}{2 v_0^2 \cos^2\theta}",
    r"E := \tfrac12 m v_0^2",
    r"\frac{d}{dx} y(x) = ?",
    r"\int_0^{R} y(x)\,dx \approx ?",
    r"\sum_{n=1}^{\infty} \frac{1}{n^2} = ?",
    r"\lim_{x\to 0} \frac{\sin x}{x} = ?",
    r"y(x)",
    r"x \in [0, R]",
    r"z = x^2 - y^2",
    r"x, y \in [-2, 2]",
];

#[test]
fn queue_example() {
    let mut out = String::new();
    for src in QUEUE_EXAMPLE {
        out.push_str(&format!("{src}\n  => {}\n", full(src)));
    }
    insta::assert_snapshot!(out);
}

#[test]
fn queue_example_statement_kinds() {
    let kinds: Vec<&str> = QUEUE_EXAMPLE
        .iter()
        .map(|src| {
            let ast = common::parse_clean(src);
            match &ast.lines[0].statement {
                Statement::Definition {
                    kind: DefKind::Const,
                    ..
                } => "const",
                Statement::Definition {
                    head: DefHead::Function { .. },
                    ..
                } => "function",
                Statement::Definition { .. } => "variable",
                Statement::Query {
                    mode: QueryMode::Exact,
                    ..
                } => "exact",
                Statement::Query { .. } => "approx",
                Statement::NameList(_) => "names",
                Statement::Range(_) => "range",
                Statement::Relation(_) => "relation",
                _ => "other",
            }
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "const", "variable", "names", "function", "variable", "exact", "approx", "exact",
            "exact", "relation", "range", "relation", "range"
        ]
    );
}

#[test]
fn using_name_list() {
    let (names, diags) = parse_name_list(r"\theta, R(\theta)", 6);
    assert!(diags.is_empty());
    assert_eq!(names.len(), 2);
    assert_eq!(names[0].span.start, 6);
    let (_, diags) = parse_name_list(r"a, 2, b", 0);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].message, "expected a name");
}

#[test]
fn helper_intervals() {
    let show = |src: &str| {
        let (node, diags) = parse_interval(src, 0);
        assert!(diags.is_empty(), "{src}: {diags:?}");
        sexpr::node(&node, sexpr::Mode::Semantic)
    };
    assert_eq!(show("[0, 4]"), "(interval [] 0 4)");
    assert_eq!(show(r"(0, \infty)"), "(interval () 0 #inf)");
    assert_eq!(show("[-1, 1)"), "(interval [) (neg 1) 1)");
    let (_, diags) = parse_interval("x + 1", 0);
    assert_eq!(diags.len(), 1);
}

#[test]
fn helper_expressions_and_clauses() {
    let (node, diags) = parse_expr(r"x^3 - 3x", 0);
    assert!(diags.is_empty());
    assert_eq!(
        sexpr::node(&node, sexpr::Mode::Semantic),
        "(- (^ x 3) (* 3 x))"
    );
    let (clause, diags) = parse_clause("x = 1", 0);
    assert!(diags.is_empty());
    assert!(matches!(clause, Clause::Where { .. }));
    let (clause, _) = parse_clause(r"x \in [0, 1]", 0);
    assert!(matches!(clause, Clause::Range(_)));
    let (clause, _) = parse_clause("x > 0", 0);
    assert_eq!(
        sexpr::clause(&clause, sexpr::Mode::Semantic),
        "(if (rel x > 0))"
    );
    let (_, diags) = parse_expr("x )", 0);
    assert_eq!(diags.len(), 1);
}

#[test]
fn figure_items() {
    let mut out = String::new();
    for src in [
        r"y(x)",
        r"x \in [0, R]",
        r"z = x^2 - y^2",
        r"x, y \in [-2, 2]",
        r"r = 1 + \cos\theta",
        r"x^2 + y^2 = 1",
        r"y' = x - y",
        r"a := 2, \; a \in [0, 5]",
    ] {
        out.push_str(&format!("{src}\n  => {}\n", full(src)));
    }
    insta::assert_snapshot!(out);
}

#[test]
fn calls_are_left_for_the_evaluator() {
    let ast = common::parse_clean("f(x) + x(x + 1)");
    let Statement::Relation(node) = &ast.lines[0].statement else {
        panic!("expected an expression");
    };
    let NodeKind::Binary { lhs, rhs, .. } = &node.kind else {
        panic!("expected a sum");
    };
    assert!(matches!(lhs.kind, NodeKind::Call { .. }));
    assert!(matches!(rhs.kind, NodeKind::Call { .. }));
}
