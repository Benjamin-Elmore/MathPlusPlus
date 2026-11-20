//! A compact text outline of a [`Document`], for snapshot tests and debugging.
//!
//! Maths is shown in its semantic S-expression form.

use std::fmt::Write;

use super::ast::*;
use crate::math::sexpr::{self, Mode};
use crate::math::{Clause, MathAst, Node};

/// Prints the blocks of `doc` (nested ones indented) and then its diagnostics.
pub fn dump(doc: &Document) -> String {
    let mut out = String::new();
    blocks(&mut out, &doc.blocks, 0);
    for d in &doc.diagnostics {
        let _ = writeln!(out, "{d}");
    }
    out
}

fn pad(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn blocks(out: &mut String, list: &[Block], depth: usize) {
    for b in list {
        block(out, b, depth);
    }
}

fn math(m: &MathAst) -> String {
    sexpr::ast(m, Mode::Semantic).replace('\n', " | ")
}

fn node(n: &Node) -> String {
    sexpr::node(n, Mode::Semantic)
}

fn nodes(list: &[Node]) -> String {
    list.iter().map(node).collect::<Vec<_>>().join(", ")
}

fn clause(c: &Clause) -> String {
    sexpr::clause(c, Mode::Semantic)
}

fn block(out: &mut String, b: &Block, depth: usize) {
    pad(out, depth);
    let _ = write!(out, "[{}..{}] ", b.span.start, b.span.end);
    match &b.kind {
        BlockKind::Heading { level, content } => {
            let _ = writeln!(out, "Heading({level}) {}", inlines(content));
        }
        BlockKind::Paragraph(c) => {
            let _ = writeln!(out, "Paragraph {}", inlines(c));
        }
        BlockKind::List(l) => {
            let _ = writeln!(out, "List");
            list(out, l, depth + 1);
        }
        BlockKind::Table(t) => {
            let _ = writeln!(out, "Table {:?}", t.aligns);
            pad(out, depth + 1);
            let _ = writeln!(out, "head: {}", cells(&t.header));
            for r in &t.rows {
                pad(out, depth + 1);
                let _ = writeln!(out, "row: {}", cells(r));
            }
        }
        BlockKind::Quote(ps) => {
            let _ = writeln!(out, "Quote");
            for p in ps {
                pad(out, depth + 1);
                let _ = writeln!(out, "{}", inlines(p));
            }
        }
        BlockKind::Verbatim(v) => {
            let _ = writeln!(
                out,
                "Verbatim info={:?} closed={} text={:?}",
                v.info, v.closed, v.text
            );
        }
        BlockKind::Rule => {
            let _ = writeln!(out, "Rule");
        }
        BlockKind::DisplayMath(m) => {
            let _ = writeln!(
                out,
                "DisplayMath number={:?} label={:?} noeval={} closed={} raw={}..{}",
                m.number, m.label, m.noeval, m.closed, m.raw.start, m.raw.end
            );
            pad(out, depth + 1);
            let _ = writeln!(out, "{}", math(&m.ast));
        }
        BlockKind::Figure(f) => {
            let _ = writeln!(out, "Figure");
            for item in &f.items {
                pad(out, depth + 1);
                match &item.kind {
                    FigureItemKind::Expr(m) => {
                        let _ = writeln!(out, "expr {}", math(m));
                    }
                    FigureItemKind::Range(r) => {
                        let _ = writeln!(out, "range {} in {}", nodes(&r.names), node(&r.set));
                    }
                    FigureItemKind::Keyword { keyword, arg } => {
                        let _ = writeln!(
                            out,
                            "keyword {:?}{}",
                            keyword.as_str(),
                            arg.as_ref()
                                .map(|c| format!(" {}", clause(c)))
                                .unwrap_or_default()
                        );
                    }
                }
            }
            if !f.caption.is_empty() {
                pad(out, depth + 1);
                let _ = writeln!(out, "caption: {}", inlines(&f.caption));
            }
        }
        BlockKind::EmbedImport { path, .. } => {
            let _ = writeln!(out, "EmbedImport {path:?}");
        }
        BlockKind::Using(u) => {
            let _ = writeln!(
                out,
                "Using names=[{}] aliases={} path={:?}",
                nodes(&u.names),
                u.aliases
                    .as_ref()
                    .map_or("-".to_string(), |a| format!("[{}]", nodes(a))),
                u.path
            );
        }
        BlockKind::Helper(h) => {
            let _ = writeln!(
                out,
                "Helper {:?} {:?} target={} on={} at={}",
                h.verb,
                h.helper,
                node(&h.target),
                h.interval.as_ref().map_or("-".to_string(), node),
                h.at.as_ref().map_or("-".to_string(), clause)
            );
        }
        BlockKind::Analyse(a) => {
            let _ = writeln!(
                out,
                "Analyse target={} on={}",
                node(&a.target),
                a.interval.as_ref().map_or("-".to_string(), node)
            );
        }
        BlockKind::Solve(s) => {
            let _ = writeln!(
                out,
                "Solve equation={} for={}",
                node(&s.equation),
                s.unknowns
                    .as_ref()
                    .map_or("-".to_string(), |u| format!("[{}]", nodes(u)))
            );
        }
        BlockKind::Data(d) => {
            let _ = writeln!(out, "Data name={} path={:?}", node(&d.name), d.path);
        }
        BlockKind::Assume(a) => {
            let _ = writeln!(out, "Assume [{}]", nodes(&a.conditions));
        }
        BlockKind::Let(l) => {
            let _ = writeln!(out, "Let {}", inlines(&l.content));
        }
        BlockKind::FootnoteDef { name, content } => {
            let _ = writeln!(out, "FootnoteDef {name:?} {}", inlines(content));
        }
        BlockKind::Directive(d) => {
            let _ = writeln!(out, "Directive {:?} {:?}", d.kind, d.text);
        }
        BlockKind::Theorem(t) => {
            let _ = writeln!(out, "Theorem {} closed={}", t.kind.as_str(), t.closed);
            blocks(out, &t.body, depth + 1);
        }
    }
}

fn list(out: &mut String, l: &List, depth: usize) {
    for item in &l.items {
        pad(out, depth);
        let marker = match (l.ordered, item.number) {
            (true, Some(n)) => format!("{n}."),
            (true, None) => "#.".to_string(),
            (false, _) => "-".to_string(),
        };
        let _ = writeln!(out, "{marker} {}", inlines(&item.content));
        for child in &item.children {
            list(out, child, depth + 1);
        }
    }
}

fn cells(row: &[Cell]) -> String {
    row.iter()
        .map(|c| inlines(c))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn inlines(list: &[Inline]) -> String {
    list.iter().map(inline).collect::<Vec<_>>().join(" ")
}

fn inline(i: &Inline) -> String {
    match &i.kind {
        InlineKind::Text(t) => format!("{t:?}"),
        InlineKind::SoftBreak => "<br>".to_string(),
        InlineKind::Emphasis(c) => format!("(em {})", inlines(c)),
        InlineKind::Strong(c) => format!("(strong {})", inlines(c)),
        InlineKind::Math(m) => format!("(math {})", math(&m.ast)),
        InlineKind::Code(c) => format!("(code {c:?})"),
        InlineKind::WikiLink { path, alias } => match alias {
            Some(a) => format!("(wiki {path:?} {a:?})"),
            None => format!("(wiki {path:?})"),
        },
        InlineKind::Link { text, url } => format!("(link {url:?} {})", inlines(text)),
        InlineKind::FootnoteRef(n) => format!("(fnref {n:?})"),
        InlineKind::EqRef { label, number } => format!("(eqref {label:?} {number:?})"),
        InlineKind::Escape(c) => format!("(escape {c:?})"),
    }
}
