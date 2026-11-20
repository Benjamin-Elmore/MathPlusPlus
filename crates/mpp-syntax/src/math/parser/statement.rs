//! Statement lines: definitions, queries, checks, relations, ranges and clauses.

use super::expr::{self, name_node};
use super::Parser;
use crate::lexer::Tok;
use crate::math::ast::{
    BinOp, Clause, DefHead, DefKind, Label, Line, MathAst, Name, Node, NodeKind, QueryMode, Range,
    RelOp, Statement,
};
use crate::span::Span;

/// Environments that only align lines; the parser looks through them.
const ALIGN_ENVS: &[&str] = &[
    "aligned",
    "align",
    "align*",
    "alignat",
    "alignat*",
    "gather",
    "gather*",
    "gathered",
    "split",
    "equation",
    "equation*",
    "multline",
    "multline*",
    "eqnarray",
    "eqnarray*",
];

/// Per-line facts pulled out of the token stream before parsing.
#[derive(Default)]
struct LineMeta {
    noeval: bool,
    align: Vec<usize>,
}

impl Parser<'_> {
    /// Parses every statement line of the span.
    pub(super) fn parse_ast(&mut self) -> MathAst {
        let (metas, label, aligned) = self.preprocess();
        let mut lines = Vec::new();
        let mut metas = metas.into_iter();
        loop {
            let mut line = self.parse_line();
            let meta = metas.next().unwrap_or_default();
            line.noeval = meta.noeval;
            line.align = meta.align;
            lines.push(line);
            if self.eat(|t| matches!(t, Tok::NewLine)).is_none() {
                break;
            }
        }
        // A trailing `\\` leaves an empty last line; drop it.
        if lines.len() > 1
            && lines
                .last()
                .is_some_and(|l| matches!(l.statement, Statement::Empty) && !l.noeval)
        {
            lines.pop();
        }
        MathAst {
            lines,
            label,
            aligned,
            span: Span::new(self.base, self.base + self.src.len()),
        }
    }

    /// Removes `\label`, `%noeval`, alignment environments and top-level `&`,
    /// recording them per line.
    fn preprocess(&mut self) -> (Vec<LineMeta>, Option<Label>, bool) {
        let mut metas = vec![LineMeta::default()];
        let mut label: Option<Label> = None;
        let mut aligned = false;
        let mut env_depth = 0usize;
        let mut kept = Vec::with_capacity(self.toks.len());
        for tok in std::mem::take(&mut self.toks) {
            let line = metas.len() - 1;
            match &tok.kind {
                Tok::Begin(env) | Tok::End(env) if ALIGN_ENVS.contains(&env.as_str()) => {
                    aligned = true;
                }
                Tok::Begin(_) => {
                    env_depth += 1;
                    kept.push(tok);
                }
                Tok::End(_) => {
                    env_depth = env_depth.saturating_sub(1);
                    kept.push(tok);
                }
                Tok::Label(name) => {
                    if label.is_some() {
                        self.error(tok.span, "a maths block can have only one `\\label`", &[]);
                    } else {
                        label = Some(Label {
                            name: name.clone(),
                            span: tok.span,
                        });
                    }
                }
                Tok::NoEval => {
                    if let Some(m) = metas.get_mut(line) {
                        m.noeval = true;
                    }
                }
                Tok::Amp if env_depth == 0 => {
                    if let Some(m) = metas.get_mut(line) {
                        m.align.push(tok.span.start);
                    }
                }
                Tok::NewLine if env_depth == 0 => {
                    metas.push(LineMeta::default());
                    kept.push(tok);
                }
                _ => kept.push(tok),
            }
        }
        self.toks = kept;
        self.stop = self.toks.len();
        (metas, label, aligned)
    }

    fn at_line_end(&self) -> bool {
        self.peek().is_none_or(|t| matches!(t, Tok::NewLine))
    }

    /// One statement line with its clauses, recovering at `,` or the line end.
    fn parse_line(&mut self) -> Line {
        let start = self.cur_span().start;
        if self.at_line_end() {
            return Line {
                statement: Statement::Empty,
                clauses: Vec::new(),
                noeval: false,
                align: Vec::new(),
                span: Span::point(start),
            };
        }
        let mut statement = self.parse_statement();
        let mut clauses = Vec::new();
        loop {
            if self.at_line_end() {
                break;
            }
            if self.eat(|t| matches!(t, Tok::Comma)).is_some() {
                if self.at_line_end() {
                    break;
                }
                clauses.push(self.parse_clause());
                continue;
            }
            if self.at(|t| matches!(t, Tok::Period))
                && self.peek_n(1).is_none_or(|t| matches!(t, Tok::NewLine))
            {
                self.bump();
                continue;
            }
            self.skip_unexpected();
        }
        if let Some(names) = name_list(&statement, &clauses) {
            statement = Statement::NameList(names);
            clauses.clear();
        }
        let end = self.prev_end().max(start);
        Line {
            statement,
            clauses,
            noeval: false,
            align: Vec::new(),
            span: Span::new(start, end),
        }
    }

    /// Reports the next token as unexpected and skips to the next `,` or line end.
    fn skip_unexpected(&mut self) {
        let first = self.cur_span();
        let found = self.text(first).to_string();
        let mut last = first;
        while let Some(t) = self.peek() {
            if matches!(t, Tok::Comma | Tok::NewLine) {
                break;
            }
            last = self.cur_span();
            self.bump();
        }
        let message = if found.is_empty() {
            "unexpected input".to_string()
        } else {
            format!("unexpected `{found}`")
        };
        self.error(
            first.join(last),
            message,
            &["an operator", "`,`", "`\\\\`", "the end of the maths"],
        );
    }

    /// The core of a line: definition, query, check, range or relation chain.
    fn parse_statement(&mut self) -> Statement {
        if let Some(range) = self.try_range() {
            return Statement::Range(range);
        }
        let lhs = self.parse_line_relation();
        match self.peek() {
            Some(Tok::Assign) => {
                self.bump();
                let mut body = self.parse_body("the value after `:=`");
                let head = self.def_head(lhs, ":=");
                rebind_params(&head, &mut body);
                Statement::Definition {
                    head,
                    kind: DefKind::Assign,
                    body,
                }
            }
            Some(Tok::CheckEq) => {
                self.bump();
                let rhs = self.parse_body("the right side of the check");
                Statement::Check { lhs, rhs }
            }
            Some(Tok::Rel(op @ (RelOp::Eq | RelOp::Approx)))
                if matches!(self.peek_n(1), Some(Tok::Question)) =>
            {
                let mode = if *op == RelOp::Eq {
                    QueryMode::Exact
                } else {
                    QueryMode::Approx
                };
                self.bump();
                self.bump();
                let steps = self.eat(|t| matches!(t, Tok::Bang)).is_some();
                Statement::Query {
                    expr: lhs,
                    mode,
                    steps,
                }
            }
            _ => self.relation_or_const(lhs),
        }
    }

    /// The right side of `:=` or a check, or an error node if it is missing.
    fn parse_body(&mut self, what: &str) -> Node {
        if self.at_line_end() || self.at(|t| matches!(t, Tok::Comma)) {
            return self.missing(what, &["an expression"]);
        }
        self.parse_relation()
    }

    /// `g \equiv 9.81` with a name on the left is a constant definition.
    fn relation_or_const(&mut self, lhs: Node) -> Statement {
        let NodeKind::Relation { first, rest } = lhs.kind else {
            return Statement::Relation(lhs);
        };
        if rest.len() == 1
            && rest[0].0 == RelOp::Equiv
            && head_shape(&first)
            && !mentions_mod(&rest[0].1)
        {
            let mut rest = rest;
            let (_, mut body) = rest.remove(0);
            let head = self.def_head(*first, "≡");
            rebind_params(&head, &mut body);
            return Statement::Definition {
                head,
                kind: DefKind::Const,
                body,
            };
        }
        Statement::Relation(Node::new(NodeKind::Relation { first, rest }, lhs.span))
    }

    /// Classifies the left side of a definition.
    fn def_head(&mut self, node: Node, op: &str) -> DefHead {
        let inner = node.ungrouped().clone();
        if let NodeKind::Call { callee, args, .. } = &inner.kind {
            if callee.is_name() && args.iter().all(Node::is_name) {
                return DefHead::Function {
                    name: name_node(callee),
                    params: args.iter().map(name_node).collect(),
                };
            }
            if let Some(bad) = args.iter().find(|a| !a.is_name()) {
                self.error(
                    bad.span,
                    "expected a parameter name in a function definition",
                    &["a name such as `x`"],
                );
                return DefHead::Invalid(node);
            }
        }
        let Some(name) = inner.as_name() else {
            self.error(
                node.span,
                format!("expected a name or a function head such as `f(x)` before `{op}`"),
                &["a name", "`f(x)`", "`a_{n+1}`"],
            );
            return DefHead::Invalid(node);
        };
        let Some(sub) = name.sub.as_deref() else {
            return DefHead::Name(name_node(&inner));
        };
        let bare = Node::new(
            NodeKind::Name(Name {
                sub: None,
                ..name.clone()
            }),
            inner.span,
        );
        let sub_inner = sub.ungrouped();
        if let NodeKind::Binary {
            op: op @ (BinOp::Add | BinOp::Sub),
            lhs,
            rhs,
        } = &sub_inner.kind
        {
            if lhs.is_name() && matches!(rhs.kind, NodeKind::Number(_)) {
                let shift = if *op == BinOp::Sub {
                    Node::new(
                        NodeKind::Unary {
                            op: crate::math::ast::UnaryOp::Neg,
                            operand: rhs.clone(),
                        },
                        rhs.span,
                    )
                } else {
                    (**rhs).clone()
                };
                return DefHead::Recurrence {
                    name: bare,
                    index: name_node(lhs),
                    shift,
                };
            }
        }
        if let NodeKind::Number(text) = &sub_inner.kind {
            if text.chars().all(|c| c.is_ascii_digit()) {
                return DefHead::Initial {
                    name: bare,
                    index: sub_inner.clone(),
                };
            }
        }
        DefHead::Name(name_node(&inner))
    }

    /// A clause after a comma.
    pub(super) fn parse_clause(&mut self) -> Clause {
        if let Some(range) = self.try_range() {
            return Clause::Range(range);
        }
        let node = self.parse_relation();
        if let NodeKind::Relation { first, rest } = &node.kind {
            if rest.len() == 1 && rest[0].0 == RelOp::Eq && first.is_name() {
                return Clause::Where {
                    name: name_node(first),
                    value: rest[0].1.clone(),
                    span: node.span,
                };
            }
        }
        Clause::Condition(node)
    }

    /// `x, y \in S`: names separated by commas, then `\in`. Backtracks on anything else.
    fn try_range(&mut self) -> Option<Range> {
        let cp = self.checkpoint();
        let start = self.cur_span().start;
        let mut names = Vec::new();
        loop {
            if !self.at(can_start_name) {
                break;
            }
            let node = self.parse_postfix();
            if !node.is_name() {
                break;
            }
            names.push(name_node(&node));
            if self.eat(|t| matches!(t, Tok::Comma)).is_some() {
                continue;
            }
            if self.eat(|t| matches!(t, Tok::Rel(RelOp::In))).is_some() {
                let set = expr::as_interval(self.parse_expr());
                let span = Span::new(start, set.span.end);
                return Some(Range { names, set, span });
            }
            break;
        }
        self.restore(cp);
        None
    }

    /// Comma-separated names (for `Using … from`).
    pub(super) fn parse_names(&mut self) -> Vec<Node> {
        let mut names = Vec::new();
        loop {
            let node = self.parse_postfix();
            if !name_like(&node) && !node.is_error() {
                self.error(
                    node.span,
                    "expected a name",
                    &["a name such as `f`", "`f(x)`"],
                );
            }
            names.push(node);
            if self.eat(|t| matches!(t, Tok::Comma)).is_none() {
                break;
            }
        }
        names
    }
}

/// `f(i) := …` binds `i` in the body (and likewise `e`).
fn rebind_params(head: &DefHead, body: &mut Node) {
    let params: &[Node] = match head {
        DefHead::Function { params, .. } => params,
        DefHead::Recurrence { index, .. } => std::slice::from_ref(index),
        _ => &[],
    };
    for letter in params.iter().filter_map(expr::bound_letter) {
        expr::rebind(body, letter);
    }
}

/// True when `node` contains `\pmod`, so `a \equiv b \pmod{n}` stays a congruence.
fn mentions_mod(node: &Node) -> bool {
    if let NodeKind::NamedOp(op) = &node.kind {
        if op.name == "mod" {
            return true;
        }
    }
    let mut found = false;
    crate::math::visit::children(node, &mut |child| found = found || mentions_mod(child));
    found
}

/// Tokens that can start a plain name.
fn can_start_name(t: &Tok) -> bool {
    match t {
        Tok::Letter(_) | Tok::Styled { .. } => true,
        Tok::Sym(s) => s.kind == crate::symbols::SymbolKind::Letter && s.name != "pi",
        _ => false,
    }
}

/// A name, or a call whose arguments are all names (`R(\theta)`).
fn name_like(node: &Node) -> bool {
    match &node.kind {
        NodeKind::Call { callee, args, .. } => callee.is_name() && args.iter().all(Node::is_name),
        _ => node.is_name(),
    }
}

/// True when `node` could be a definition head.
fn head_shape(node: &Node) -> bool {
    name_like(node.ungrouped())
}

/// A line of bare names separated by commas is a name list.
fn name_list(statement: &Statement, clauses: &[Clause]) -> Option<Vec<Node>> {
    let Statement::Relation(first) = statement else {
        return None;
    };
    if clauses.is_empty() || !name_like(first) {
        return None;
    }
    let mut names = vec![first.clone()];
    for clause in clauses {
        match clause {
            Clause::Condition(n) if name_like(n) => names.push(n.clone()),
            _ => return None,
        }
    }
    Some(names)
}
