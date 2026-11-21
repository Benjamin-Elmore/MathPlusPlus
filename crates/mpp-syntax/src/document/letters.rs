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

// ---- names spelled out in letters -------------------------------------------

/// The multi-letter names the document defines (`velocity := 3`,
/// `\mathrm{speed} := 2`).
fn spelled_definitions(blocks: &mut [Block]) -> HashSet<String> {
    let mut out = HashSet::new();
    each_math_mut(blocks, &mut |ast| {
        for line in &ast.lines {
            if let Statement::Definition {
                head: DefHead::Name(n),
                ..
            } = &line.statement
            {
                if let NodeKind::Name(Name {
                    base,
                    sub: None,
                    accent: None,
                    ..
                }) = &n.ungrouped().kind
                {
                    if base.chars().count() >= 2 && base.chars().all(|c| c.is_ascii_alphabetic()) {
                        out.insert(base.clone());
                    }
                }
            }
        }
    });
    out
}

/// Every maths span of `blocks`, in document order.
fn each_math_mut(blocks: &mut [Block], f: &mut dyn FnMut(&mut MathAst)) {
    fn inl(items: &mut [Inline], f: &mut dyn FnMut(&mut MathAst)) {
        for i in items {
            match &mut i.kind {
                InlineKind::Math(m) => f(&mut m.ast),
                InlineKind::Emphasis(inner) | InlineKind::Strong(inner) => inl(inner, f),
                InlineKind::Link { text, .. } => inl(text, f),
                _ => {}
            }
        }
    }
    fn lst(l: &mut List, f: &mut dyn FnMut(&mut MathAst)) {
        for item in &mut l.items {
            inl(&mut item.content, f);
            for child in &mut item.children {
                lst(child, f);
            }
        }
    }
    for b in blocks {
        match &mut b.kind {
            BlockKind::DisplayMath(d) => f(&mut d.ast),
            BlockKind::Heading { content, .. } | BlockKind::FootnoteDef { content, .. } => {
                inl(content, f)
            }
            BlockKind::Paragraph(content) => inl(content, f),
            BlockKind::Let(l) => inl(&mut l.content, f),
            BlockKind::Quote(ps) => ps.iter_mut().for_each(|p| inl(p, f)),
            BlockKind::List(l) => lst(l, f),
            BlockKind::Table(t) => t
                .header
                .iter_mut()
                .chain(t.rows.iter_mut().flatten())
                .for_each(|c| inl(c, f)),
            BlockKind::Figure(fig) => {
                for item in &mut fig.items {
                    if let super::FigureItemKind::Expr(ast) = &mut item.kind {
                        f(ast);
                    }
                }
                inl(&mut fig.caption, f);
            }
            BlockKind::Theorem(t) => each_math_mut(&mut t.body, f),
            _ => {}
        }
    }
}

/// Letters side by side that spell a name the document defines (`velocity · 2`
/// after `velocity := 3`) become that one name, so the page sets the word the
/// same way wherever it is written (upright, as where it is defined) and the
/// evaluator reads the name, not a product of letters.
pub(super) fn bind_spelled_names(blocks: &mut [Block]) {
    let words = spelled_definitions(blocks);
    if words.is_empty() {
        return;
    }
    each_math_mut(blocks, &mut |ast| {
        for line in &mut ast.lines {
            let mut roots: Vec<&mut Node> = Vec::new();
            match &mut line.statement {
                Statement::Definition { body, .. } => roots.push(body),
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
                    Clause::Range(_) => {}
                }
            }
            for r in roots {
                spell(r, &words);
            }
        }
    });
}

fn spell(n: &mut Node, words: &HashSet<String>) {
    use crate::math::letters::{implicit_factors, letter_runs};
    let implicit = matches!(
        n.kind,
        NodeKind::Binary {
            op: BinOp::Mul(MulStyle::Implicit),
            ..
        }
    );
    if !implicit {
        visit::children_mut(n, &mut |c| spell(c, words));
        return;
    }
    let factors = implicit_factors(n);
    let runs: Vec<_> = letter_runs(&factors)
        .into_iter()
        .filter(|r| words.contains(&r.word))
        .collect();
    if !runs.is_empty() {
        let mut terms: Vec<Node> = vec![];
        let mut i = 0;
        while i < factors.len() {
            if let Some(r) = runs.iter().find(|r| r.start == i) {
                let name = Node::new(NodeKind::Name(Name::simple(r.word.clone())), r.span);
                let last = factors[r.end - 1];
                let term = match &last.kind {
                    NodeKind::Pow { exp, .. } if r.powered => Node::new(
                        NodeKind::Pow {
                            base: Box::new(name),
                            exp: exp.clone(),
                        },
                        r.span.join(last.span),
                    ),
                    _ => name,
                };
                terms.push(term);
                i = r.end;
            } else {
                terms.push(factors[i].clone());
                i += 1;
            }
        }
        let mut terms = terms.into_iter();
        let Some(mut acc) = terms.next() else {
            return;
        };
        for t in terms {
            let span = acc.span.join(t.span);
            acc = Node::new(
                NodeKind::Binary {
                    op: BinOp::Mul(MulStyle::Implicit),
                    lhs: Box::new(acc),
                    rhs: Box::new(t),
                },
                span,
            );
        }
        *n = acc;
    }
    // The other factors may hold words too (`2(velocity + 1)`).
    for f in implicit_children_mut(n) {
        if !matches!(f.kind, NodeKind::Name(_)) {
            visit::children_mut(f, &mut |c| spell(c, words));
        }
    }
}

/// The factors of an implicit product, mutably.
fn implicit_children_mut(n: &mut Node) -> Vec<&mut Node> {
    fn walk<'a>(n: &'a mut Node, out: &mut Vec<&'a mut Node>) {
        let implicit = matches!(
            n.kind,
            NodeKind::Binary {
                op: BinOp::Mul(MulStyle::Implicit),
                ..
            }
        );
        if !implicit {
            out.push(n);
            return;
        }
        if let NodeKind::Binary { lhs, rhs, .. } = &mut n.kind {
            walk(lhs, out);
            walk(rhs, out);
        }
    }
    let mut out = vec![];
    walk(n, &mut out);
    out
}
