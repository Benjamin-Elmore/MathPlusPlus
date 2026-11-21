//! Maths parses kept between document parses.
//!
//! The editor parses the whole document on every keystroke (and the evaluator parses
//! it again), but an edit changes one formula: every other `$…$` and `$$…$$` has the
//! same text as before, only at a new offset. [`parse_math`](super::parse_math) is a
//! function of the text and the offset alone, so a formula seen before is taken from
//! here and its spans moved to the new offset instead of being lexed and parsed again.
//!
//! Each thread keeps its own entries (the editor and the evaluator run on different
//! threads); entries not used for a while are dropped.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::math::ast::{Clause, DefHead, Line, MathAst, Node, NodeKind, Range, Statement};
use crate::math::visit::children_mut;
use crate::span::{Diagnostic, Span};

/// Most bytes kept per thread (stored text plus the estimated size of its parse); past
/// it the entries unused for longest are dropped.
const MAX_BYTES: usize = 32 * 1024 * 1024;
/// A parse takes about this many bytes per byte of formula text (measured: a 1.8 KB
/// formula keeps about 105 KB).
const PARSE_FACTOR: usize = 64;

fn cost(text_len: usize) -> usize {
    text_len * (PARSE_FACTOR + 1) + 256
}
/// Longest formula text kept, in bytes (long ones are rare and costly to keep).
const MAX_TEXT: usize = 16 * 1024;

struct Entry {
    base: usize,
    ast: MathAst,
    diags: Vec<Diagnostic>,
    used: u64,
    cost: usize,
}

#[derive(Default)]
struct Memo {
    entries: HashMap<String, Entry>,
    tick: u64,
    bytes: usize,
}

thread_local! {
    static MEMO: RefCell<Memo> = RefCell::new(Memo::default());
}

/// The parse of `src` at `base`, if the same text was parsed before on this thread.
pub(super) fn lookup(src: &str, base: usize) -> Option<(MathAst, Vec<Diagnostic>)> {
    if src.len() > MAX_TEXT {
        return None;
    }
    MEMO.with(|m| {
        let mut m = m.borrow_mut();
        m.tick += 1;
        let tick = m.tick;
        let e = m.entries.get_mut(src)?;
        e.used = tick;
        let mut ast = e.ast.clone();
        let mut diags = e.diags.clone();
        let from = e.base;
        drop(m);
        if from != base {
            let mv = |s: Span| {
                Span::new(
                    (s.start + base).saturating_sub(from),
                    (s.end + base).saturating_sub(from),
                )
            };
            shift_ast(&mut ast, &mv);
            for d in &mut diags {
                d.span = mv(d.span);
            }
        }
        Some((ast, diags))
    })
}

/// Keeps the parse of `src` at `base`. Only shallow parses are kept (they were made
/// on the caller's stack, so copying and moving them never needs more stack).
pub(super) fn store(src: &str, base: usize, ast: &MathAst, diags: &[Diagnostic]) {
    if src.len() > MAX_TEXT {
        return;
    }
    MEMO.with(|m| {
        let mut m = m.borrow_mut();
        m.tick += 1;
        let tick = m.tick;
        let c = cost(src.len());
        if let Some(old) = m.entries.remove(src) {
            m.bytes -= old.cost;
        }
        while m.bytes + c > MAX_BYTES && !m.entries.is_empty() {
            // Keep the newer half.
            let mut ages: Vec<u64> = m.entries.values().map(|e| e.used).collect();
            ages.sort_unstable();
            let cut = ages[ages.len() / 2];
            m.entries.retain(|_, e| e.used > cut);
            m.bytes = m.entries.values().map(|e| e.cost).sum();
        }
        m.bytes += c;
        m.entries.insert(
            src.to_string(),
            Entry {
                base,
                ast: ast.clone(),
                diags: diags.to_vec(),
                used: tick,
                cost: c,
            },
        );
    });
}

fn shift_ast(ast: &mut MathAst, mv: &impl Fn(Span) -> Span) {
    let MathAst {
        lines,
        label,
        aligned: _,
        span,
    } = ast;
    *span = mv(*span);
    if let Some(l) = label {
        l.span = mv(l.span);
    }
    for line in lines {
        shift_line(line, mv);
    }
}

fn shift_line(line: &mut Line, mv: &impl Fn(Span) -> Span) {
    let Line {
        statement,
        clauses,
        noeval: _,
        align,
        span,
    } = line;
    *span = mv(*span);
    for a in align.iter_mut() {
        *a = mv(Span::point(*a)).start;
    }
    match statement {
        Statement::Definition {
            head,
            kind: _,
            body,
            query: _,
        } => {
            match head {
                DefHead::Name(n) | DefHead::Invalid(n) => shift_node(n, mv),
                DefHead::Function { name, params } => {
                    shift_node(name, mv);
                    params.iter_mut().for_each(|p| shift_node(p, mv));
                }
                DefHead::Recurrence { name, index, shift } => {
                    shift_node(name, mv);
                    shift_node(index, mv);
                    shift_node(shift, mv);
                }
                DefHead::Initial { name, index } => {
                    shift_node(name, mv);
                    shift_node(index, mv);
                }
            }
            shift_node(body, mv);
        }
        Statement::Query {
            expr,
            mode: _,
            steps: _,
        } => shift_node(expr, mv),
        Statement::Check { lhs, rhs } => {
            shift_node(lhs, mv);
            shift_node(rhs, mv);
        }
        Statement::Relation(n) => shift_node(n, mv),
        Statement::NameList(ns) => ns.iter_mut().for_each(|n| shift_node(n, mv)),
        Statement::Range(r) => shift_range(r, mv),
        Statement::Empty => {}
    }
    for c in clauses {
        match c {
            Clause::Where { name, value, span } => {
                *span = mv(*span);
                shift_node(name, mv);
                shift_node(value, mv);
            }
            Clause::Range(r) => shift_range(r, mv),
            Clause::Condition(n) | Clause::Assumption(n) => shift_node(n, mv),
        }
    }
}

fn shift_range(r: &mut Range, mv: &impl Fn(Span) -> Span) {
    let Range { names, set, span } = r;
    *span = mv(*span);
    names.iter_mut().for_each(|n| shift_node(n, mv));
    shift_node(set, mv);
}

fn shift_node(n: &mut Node, mv: &impl Fn(Span) -> Span) {
    n.span = mv(n.span);
    if let NodeKind::Quantity { unit, .. } | NodeKind::Convert { unit, .. } = &mut n.kind {
        unit.span = mv(unit.span);
    }
    children_mut(n, &mut |c| shift_node(c, mv));
}

#[cfg(test)]
mod tests {
    use crate::math::parse_math;

    fn clear() {
        super::MEMO.with(|m| {
            let mut m = m.borrow_mut();
            m.entries.clear();
            m.bytes = 0;
        });
    }

    fn long_formula(i: usize) -> String {
        let term = r"1 + a_1 \cdot (2 + b^2) + ";
        format!(r"x_{{{i}}} := {} \ x_{{{i}}} = ?", term.repeat(60))
    }

    /// Typing in many different long formulas never takes the memo past its byte cap.
    #[test]
    fn the_memo_stays_under_its_byte_cap() {
        clear();
        for i in 0..1500 {
            parse_math(&long_formula(i), i);
            super::MEMO.with(|m| {
                let m = m.borrow();
                assert!(m.bytes <= super::MAX_BYTES, "{} bytes", m.bytes);
                let sum: usize = m.entries.values().map(|e| e.cost).sum();
                assert_eq!(sum, m.bytes);
            });
        }
        // Recent formulas are still kept.
        let last = long_formula(1499);
        super::MEMO.with(|m| assert!(m.borrow().entries.contains_key(&last)));
    }

    /// A formula taken from the memo at a new offset is exactly a fresh parse there.
    #[test]
    fn a_kept_parse_moved_to_a_new_offset_matches_a_fresh_parse() {
        let cases = [
            r"f(x) := \frac{x^2 + 1}{x}, x > 0 \\ f(2) = ?",
            r"a, b \in [0, 5]",
            r"\int_0^1 e^{-x^2} \, dx \approx ? \label{eq:g}",
            r"v := 9.81\,\mathrm{m/s^2} \\ v \operatorname{in} \mathrm{km/h} = ?",
            r"\begin{aligned} y &= 2x \\ &= ? \end{aligned}",
            r"\frac{1}{ + (",
            r"a_{n+1} := a_n + 1, r = 3",
        ];
        for src in cases {
            let first = parse_math(src, 7);
            let again = parse_math(src, 7);
            assert_eq!(first, again, "{src}");
            let moved = parse_math(src, 1234);
            clear();
            let fresh = parse_math(src, 1234);
            assert_eq!(moved, fresh, "{src}");
            let back = parse_math(src, 3);
            clear();
            assert_eq!(back, parse_math(src, 3), "{src}");
        }
    }
}
