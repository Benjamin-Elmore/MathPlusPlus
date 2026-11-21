//! The parser never panics, always returns a tree, and keeps every span inside
//! the input, whatever it is given.

use mpp_syntax::math::{parse_math, sexpr, visit, Clause, DefHead, MathAst, Node, Statement};
use mpp_syntax::Span;
use proptest::prelude::*;

/// Pieces that exercise every part of the lexer and parser.
const PIECES: &[&str] = &[
    "x",
    "y",
    "2",
    "3.5",
    "1e-3",
    "d",
    "D",
    "e",
    "i",
    "f",
    "(",
    ")",
    "[",
    "]",
    "{",
    "}",
    "\\{",
    "\\}",
    "|",
    "\\|",
    "‖",
    "+",
    "-",
    "*",
    "/",
    "^",
    "_",
    "'",
    "!",
    ",",
    "&",
    "=",
    "<",
    ">",
    "<=",
    "->",
    ":=",
    "?",
    "?!",
    "=?",
    ".",
    ":",
    ";",
    "\\\\",
    "\\frac",
    "\\tfrac12",
    "\\sqrt",
    "\\sum",
    "\\prod",
    "\\int",
    "\\iint",
    "\\oint",
    "\\lim",
    "\\to",
    "\\infty",
    "\\partial",
    "\\nabla",
    "\\cdot",
    "\\times",
    "\\sin",
    "\\cos",
    "\\ln",
    "\\log",
    "\\left(",
    "\\right)",
    "\\left|",
    "\\right|",
    "\\left.",
    "\\begin{pmatrix}",
    "\\end{pmatrix}",
    "\\begin{cases}",
    "\\end{cases}",
    "\\begin{aligned}",
    "\\end{aligned}",
    "\\text{if }",
    "\\mathbf{F}",
    "\\mathbb{R}",
    "\\operatorname{crit}",
    "\\hat",
    "\\dot",
    "\\overset{?}{=}",
    "\\label{a}",
    "%noeval",
    "\\equiv",
    "\\in",
    "\\quad",
    "\\,",
    "α",
    "∫",
    "∑",
    "≤",
    "²",
    "⁻¹",
    "₁",
    "√",
    "∂",
    "∇",
    "ℝ",
    "ẋ",
    "\u{301}",
    "−",
    "…",
    "lim_(",
    "sum_(",
    "int_",
    "sqrt(",
    "root(",
    "floor(",
    "abs(",
    "inf",
    "alpha",
    "\\",
    "#",
    "$",
    "\u{0}",
    " ",
    "\n",
];

fn piece_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(PIECES), 0..40).prop_map(|p| p.concat())
}

/// Checks that a span lies inside `base..base + len`.
fn check_span(span: Span, base: usize, len: usize) {
    assert!(span.start <= span.end, "inverted span {span}");
    assert!(
        span.start >= base && span.end <= base + len,
        "span {span} outside {base}..{}",
        base + len
    );
}

fn check_node(node: &Node, base: usize, len: usize) {
    check_span(node.span, base, len);
    visit::children(node, &mut |child| check_node(child, base, len));
}

fn check_ast(ast: &MathAst, base: usize, len: usize) {
    for line in &ast.lines {
        check_span(line.span, base, len);
        match &line.statement {
            Statement::Definition { head, body, .. } => {
                match head {
                    DefHead::Name(n) | DefHead::Invalid(n) => check_node(n, base, len),
                    DefHead::Function { name, params } => {
                        check_node(name, base, len);
                        params.iter().for_each(|p| check_node(p, base, len));
                    }
                    DefHead::Recurrence { name, index, shift } => {
                        for n in [name, index, shift] {
                            check_node(n, base, len);
                        }
                    }
                    DefHead::Initial { name, index } => {
                        check_node(name, base, len);
                        check_node(index, base, len);
                    }
                }
                check_node(body, base, len);
            }
            Statement::Query { expr, .. } => check_node(expr, base, len),
            Statement::Check { lhs, rhs } => {
                check_node(lhs, base, len);
                check_node(rhs, base, len);
            }
            Statement::Relation(n) => check_node(n, base, len),
            Statement::NameList(ns) => ns.iter().for_each(|n| check_node(n, base, len)),
            Statement::Range(r) => {
                r.names.iter().for_each(|n| check_node(n, base, len));
                check_node(&r.set, base, len);
            }
            Statement::Empty => {}
        }
        for clause in &line.clauses {
            match clause {
                Clause::Where { name, value, .. } => {
                    check_node(name, base, len);
                    check_node(value, base, len);
                }
                Clause::Range(r) => {
                    r.names.iter().for_each(|n| check_node(n, base, len));
                    check_node(&r.set, base, len);
                }
                Clause::Condition(n) | Clause::Assumption(n) => check_node(n, base, len),
            }
        }
    }
}

fn run(src: &str) {
    let base = 17;
    let (ast, diags) = parse_math(src, base);
    assert!(!ast.lines.is_empty());
    check_ast(&ast, base, src.len());
    for d in &diags {
        check_span(d.span, base, src.len());
        assert!(!d.message.is_empty());
    }
    // Printing must not panic either, and parsing is deterministic.
    let printed = sexpr::ast(&ast, sexpr::Mode::Full);
    let (again, _) = parse_math(src, base);
    assert_eq!(printed, sexpr::ast(&again, sexpr::Mode::Full));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn random_pieces_never_panic(src in piece_strategy()) {
        run(&src);
    }

    #[test]
    fn random_text_never_panics(src in "\\PC{0,60}") {
        run(&src);
    }
}

#[test]
fn deep_nesting_is_reported_not_overflowed() {
    let units = [
        "(",
        r"\sqrt{",
        "x^{",
        "-",
        r"\neg ",
        r"\sum ",
        "|",
        r"\frac{",
        "x^",
        r"\sin ",
        "x + ",
        "x ",
        "x!",
        "x_1",
        r"a \land ",
        r"\int ",
        "f(",
    ];
    // Past both limits (256 brackets, 10 000 operators in a row). The debug printer
    // walks the tree recursively, as deep as a 10 000-operator chain, so it runs on
    // a stack like the apps' main threads; parsing itself needs no large stack (see
    // `deep_input.rs`).
    std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(move || {
            for unit in units {
                let src = unit.repeat(12_000);
                let (ast, diags) = parse_math(&src, 0);
                assert!(!diags.is_empty(), "{unit}");
                let _ = sexpr::ast(&ast, sexpr::Mode::Full);
            }
        })
        .expect("spawn")
        .join()
        .expect("no stack overflow");
}

#[test]
fn awkward_inputs() {
    for src in [
        "",
        " ",
        "\\",
        "\\\\",
        "\\\\\\\\",
        "%",
        "%noeval",
        "$",
        "{",
        "}",
        "\\left",
        "\\right",
        "\\begin",
        "\\end",
        "\\begin{",
        "\\text{",
        "x_",
        "^",
        "''",
        "!!",
        "?",
        "=?!",
        "\\lim_{",
        "\\lim_{x \\to 0^",
        "\\frac{d}{d",
        "d/d",
        "∂/∂",
        "\\int d",
        "|||",
        "‖‖",
        "\\overset{?}",
        "a,,b",
        ",,,",
        "&&&",
        "\\\\ \\\\",
        "x \\in",
        "\\in x",
    ] {
        run(src);
    }
}
