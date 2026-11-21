//! `20 t`, `2 g`, `5 m`: a number, a plain space and one letter that is also a unit
//! symbol. When the letter is a variable where it is written, it is the variable, so
//! the page sets it in italic and the evaluator multiplies: the letter is a parameter
//! of the function being defined (`u(t) := e^{-20 t}`), a name the document defined
//! before (`g \equiv 9.81\,\mathrm{m/s^2}`, `Let $m = 2\,\mathrm{kg}$`), a plotting
//! range of the same figure, or a letter the same line also uses as a variable
//! (`\int 2 t \, dt`). Otherwise the letter stays a unit, and the evaluator warns that
//! it may not be what was meant. A thin space (`20\,t`) or `\mathrm{t}` always makes
//! the unit.

use std::collections::HashSet;

use crate::math::{
    visit, BinOp, Clause, DefHead, MathAst, MulStyle, Name, Node, NodeKind, Statement,
};

use super::{Block, BlockKind, Inline, InlineKind, List};

/// Rewrites, in document order, every number-space-letter quantity whose letter is a
/// variable where it is written into the product it means.
pub(super) fn bind_unit_letters(blocks: &mut [Block], src: &str) {
    let mut defined = HashSet::new();
    for b in blocks {
        block(b, src, &mut defined);
    }
}

fn block(b: &mut Block, src: &str, defined: &mut HashSet<String>) {
    match &mut b.kind {
        BlockKind::DisplayMath(d) => math(&mut d.ast, src, defined, &[]),
        BlockKind::Heading { content, .. } | BlockKind::FootnoteDef { content, .. } => {
            inlines(content, src, defined, false)
        }
        BlockKind::Paragraph(content) => inlines(content, src, defined, false),
        BlockKind::Let(l) => inlines(&mut l.content, src, defined, true),
        BlockKind::Quote(paragraphs) => {
            for p in paragraphs {
                inlines(p, src, defined, false);
            }
        }
        BlockKind::List(l) => list(l, src, defined),
        BlockKind::Table(t) => {
            for cell in t.header.iter_mut().chain(t.rows.iter_mut().flatten()) {
                inlines(cell, src, defined, false);
            }
        }
        BlockKind::Figure(f) => {
            let ranges: Vec<String> = f
                .items
                .iter()
                .filter_map(|i| match &i.kind {
                    super::FigureItemKind::Range(r) => Some(r.names.iter().filter_map(letter)),
                    _ => None,
                })
                .flatten()
                .collect();
            for item in &mut f.items {
                if let super::FigureItemKind::Expr(ast) = &mut item.kind {
                    math(ast, src, defined, &ranges);
                }
            }
            inlines(&mut f.caption, src, defined, false);
        }
        BlockKind::Theorem(t) => {
            for inner in &mut t.body {
                block(inner, src, defined);
            }
        }
        _ => {}
    }
}

fn list(l: &mut List, src: &str, defined: &mut HashSet<String>) {
    for item in &mut l.items {
        inlines(&mut item.content, src, defined, false);
        for child in &mut item.children {
            list(child, src, defined);
        }
    }
}

/// `defines`: the sentence is a `Let`, so `m = 2\,\mathrm{kg}` in it defines m.
fn inlines(items: &mut [Inline], src: &str, defined: &mut HashSet<String>, defines: bool) {
    for i in items {
        match &mut i.kind {
            InlineKind::Math(m) => {
                math(&mut m.ast, src, defined, &[]);
                if defines {
                    for line in &m.ast.lines {
                        if let Statement::Relation(n) = &line.statement {
                            if let NodeKind::Relation { first, .. } = &n.kind {
                                defined.extend(letter(first));
                            }
                        }
                    }
                }
            }
            InlineKind::Emphasis(inner) | InlineKind::Strong(inner) => {
                inlines(inner, src, defined, defines)
            }
            InlineKind::Link { text, .. } => inlines(text, src, defined, defines),
            _ => {}
        }
    }
}

fn math(ast: &mut MathAst, src: &str, defined: &mut HashSet<String>, extra: &[String]) {
    for line in &mut ast.lines {
        let mut bound: HashSet<String> = defined.clone();
        bound.extend(extra.iter().cloned());
        let mut defines = None;
        let mut roots: Vec<&mut Node> = Vec::new();
        match &mut line.statement {
            Statement::Definition { head, body, .. } => {
                match head {
                    DefHead::Name(n) => defines = letter(n),
                    DefHead::Function { name, params } => {
                        defines = letter(name);
                        bound.extend(params.iter().filter_map(letter));
                    }
                    _ => {}
                }
                roots.push(body);
            }
            Statement::Query { expr, .. } => roots.push(expr),
            Statement::Check { lhs, rhs } => {
                roots.push(lhs);
                roots.push(rhs);
            }
            Statement::Relation(n) => roots.push(n),
            _ => {}
        }
        for c in &mut line.clauses {
            match c {
                Clause::Where { value, .. } => roots.push(value),
                Clause::Condition(n) | Clause::Assumption(n) => roots.push(n),
                Clause::Range(r) => bound.extend(r.names.iter().filter_map(letter)),
            }
        }
        // Letters the line itself uses as variables (`20t`, `dt`).
        for r in &roots {
            used_letters(r, &mut bound);
        }
        for r in roots {
            rewrite(r, src, &bound);
        }
        defined.extend(defines);
    }
}

/// The key of a bare one-letter name (`t`, `g`), the only kind a unit can be mistaken for.
fn letter(n: &Node) -> Option<String> {
    match &n.ungrouped().kind {
        NodeKind::Name(Name {
            base,
            sub: None,
            accent: None,
            ..
        }) if base.chars().count() == 1 && base.chars().all(|c| c.is_ascii_alphabetic()) => {
            Some(base.clone())
        }
        _ => None,
    }
}

fn used_letters(n: &Node, out: &mut HashSet<String>) {
    out.extend(letter(n));
    visit::children(n, &mut |c| used_letters(c, out));
}

fn rewrite(n: &mut Node, src: &str, bound: &HashSet<String>) {
    if let NodeKind::Quantity { value, unit } = &n.kind {
        let gap = src.get(value.span.end..unit.span.start).unwrap_or("\\");
        if unit.plain
            && unit.text.len() == 1
            && unit.text.chars().all(|c| c.is_ascii_alphabetic())
            && bound.contains(&unit.text)
            && !gap.contains('\\')
        {
            let name = Node::new(NodeKind::Name(Name::simple(unit.text.clone())), unit.span);
            let lhs = value.clone();
            *n = Node::new(
                NodeKind::Binary {
                    op: BinOp::Mul(MulStyle::Implicit),
                    lhs,
                    rhs: Box::new(name),
                },
                n.span,
            );
            return;
        }
    }
    visit::children_mut(n, &mut |c| rewrite(c, src, bound));
}
