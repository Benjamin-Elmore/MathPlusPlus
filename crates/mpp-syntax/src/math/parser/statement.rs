//! Statement lines: definitions, queries, checks, relations, ranges and clauses.

use super::expr::{self, name_node};
use super::Parser;
use crate::lexer::{Delim, Tok};
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
#[derive(Default, Clone)]
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
        let mut meta = metas.next().unwrap_or_default();
        loop {
            let mut line = self.parse_line();
            line.noeval = meta.noeval;
            line.align = meta.align.clone();
            lines.push(line);
            if std::mem::take(&mut self.continues) {
                // Several statements on one line, each asking for its answer.
                continue;
            }
            if self.eat(|t| matches!(t, Tok::NewLine)).is_none() {
                break;
            }
            meta = metas.next().unwrap_or_default();
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
        let mut clauses = std::mem::take(&mut self.quantified);
        loop {
            if self.at_line_end() {
                break;
            }
            // `a \approx ? \qquad b \approx ?`: another statement starts here.
            if Self::asks(&statement)
                && clauses.is_empty()
                && !self.at(|t| matches!(t, Tok::Comma | Tok::Period | Tok::Bang))
                && !self.at(|t| {
                    matches!(
                        t,
                        Tok::Open {
                            delim: Delim::Paren,
                            ..
                        }
                    )
                })
            {
                self.continues = true;
                break;
            }
            if let Some(cond) = self.bracketed_assumption(&statement) {
                clauses.push(Clause::Assumption(cond));
                continue;
            }
            if self.eat(|t| matches!(t, Tok::Comma)).is_some() {
                if self.at_line_end() {
                    break;
                }
                // `n_a := c_a V_a, \qquad n_b(V_b) := c_b V_b`: after a definition, a
                // further definition is a statement of its own.
                if matches!(statement, Statement::Definition { .. }) && self.definition_ahead() {
                    self.continues = true;
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

    /// `(x > 0)` after a query or check: a relation in round brackets.
    fn bracketed_assumption(&mut self, statement: &Statement) -> Option<Node> {
        if !matches!(statement, Statement::Query { .. } | Statement::Check { .. }) {
            return None;
        }
        if !self.at(|t| {
            matches!(
                t,
                Tok::Open {
                    delim: Delim::Paren,
                    ..
                }
            )
        }) {
            return None;
        }
        let cp = self.checkpoint();
        let group = self.parse_atom();
        let inner = match &group.kind {
            NodeKind::Group { inner, .. } => (**inner).clone(),
            _ => {
                self.restore(cp);
                return None;
            }
        };
        if matches!(inner.kind, NodeKind::Relation { .. }) {
            Some(inner)
        } else {
            self.restore(cp);
            None
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
        // `\frac{dE}{dt} \le 0 \text{ for all } t \ge 0 \overset{?}{=} \text{true}`: the
        // domain after the quantifier is a condition of the line.
        if self.at(|t| matches!(t, Tok::Text(text) if expr::is_for_all(text))) {
            let quantifier = self.cur_span();
            self.bump();
            if self.at_line_end() || self.at(|t| matches!(t, Tok::CheckEq | Tok::Comma)) {
                self.error(
                    quantifier,
                    "expected the values the statement holds for, such as `t \\ge 0`",
                    &["a condition such as `t \\ge 0`", "`t \\in [0, 1]`"],
                );
            } else if let Some(range) = self.try_range() {
                self.quantified.push(Clause::Range(range));
            } else {
                let domain = self.parse_line_relation();
                self.quantified.push(Clause::Condition(domain));
            }
        }
        match self.peek() {
            Some(Tok::Assign) => {
                self.bump();
                let mut body = if self.at_line_end() || self.at(|t| matches!(t, Tok::Comma)) {
                    self.missing("the value after `:=`", &["an expression"])
                } else {
                    self.parse_line_relation()
                };
                let head = self.def_head(lhs, ":=");
                rebind_params(&head, &mut body);
                let query = self.definition_query();
                Statement::Definition {
                    head,
                    kind: DefKind::Assign,
                    body,
                    query,
                }
            }
            Some(Tok::CheckEq) => {
                self.bump();
                let rhs = self.parse_body("the right side of the check");
                Statement::Check { lhs, rhs }
            }
            Some(Tok::CheckEquiv) => {
                self.bump();
                let rhs = self.parse_body("the right side of the check");
                self.congruence_check(lhs, rhs)
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

    /// `a \overset{?}{\equiv} b \pmod{n}` as the check `a mod n = b mod n`: the
    /// remainders on division by `n` agree exactly when `n` divides `a - b`.
    fn congruence_check(&mut self, lhs: Node, rhs: Node) -> Statement {
        let Some((rhs, modulus)) = split_pmod(&rhs) else {
            self.error(
                rhs.span,
                "a congruence check needs its modulus after the right side",
                &["`\\pmod{n}`"],
            );
            return Statement::Check { lhs, rhs };
        };
        let remainder = |side: Node| {
            let span = side.span;
            Node::new(
                NodeKind::Binary {
                    op: BinOp::Mod,
                    lhs: Box::new(side),
                    rhs: Box::new(modulus.clone()),
                },
                span,
            )
        };
        Statement::Check {
            lhs: remainder(lhs),
            rhs: remainder(rhs),
        }
    }

    /// True when a definition (`head :=` or `head ≡`) starts here; reads nothing.
    fn definition_ahead(&mut self) -> bool {
        let cp = self.checkpoint();
        let _ = self.parse_expr();
        let found = self.at(|t| matches!(t, Tok::Assign));
        self.restore(cp);
        found
    }

    /// `≈ ?` or `= ?` after a definition: show the value just defined.
    fn definition_query(&mut self) -> Option<QueryMode> {
        let mode = match (self.peek(), self.peek_n(1)) {
            (Some(Tok::Rel(RelOp::Eq)), Some(Tok::Question)) => QueryMode::Exact,
            (Some(Tok::Rel(RelOp::Approx)), Some(Tok::Question)) => QueryMode::Approx,
            _ => return None,
        };
        self.bump();
        self.bump();
        Some(mode)
    }

    /// True when the statement just read asked for an answer (`= ?`, `≈ ?`), so what
    /// follows on the line (after `\qquad`) is a statement of its own.
    fn asks(statement: &Statement) -> bool {
        matches!(
            statement,
            Statement::Query { .. } | Statement::Definition { query: Some(_), .. }
        )
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
        let (kind, lhs_span) = lhs.into_parts();
        let NodeKind::Relation { first, rest } = kind else {
            return Statement::Relation(Node::new(kind, lhs_span));
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
            let query = self.definition_query();
            return Statement::Definition {
                head,
                kind: DefKind::Const,
                body,
                query,
            };
        }
        Statement::Relation(Node::new(NodeKind::Relation { first, rest }, lhs_span))
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
        // `velocity := 3`: letters side by side on the left of `:=` name one thing.
        if let Some(word) = crate::math::letters::spelled_word(&inner) {
            return DefHead::Name(Node::new(NodeKind::Name(Name::simple(word)), inner.span));
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
            if self.at(|t| matches!(t, Tok::Rel(RelOp::In))) {
                // `v in km/h` is a conversion, not a range.
                if names.len() == 1 && self.at_unit_conversion() {
                    break;
                }
                self.bump();
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

/// Splits a trailing `\pmod{n}` off `b \pmod{n}` (`a^p + 1 \pmod p` is read as
/// `a^p + (1 \pmod p)`, so the modulus is taken off the rightmost term): the side without
/// it and the modulus.
pub fn split_pmod(node: &Node) -> Option<(Node, Node)> {
    let NodeKind::Binary { op, lhs, rhs } = &node.kind else {
        return None;
    };
    if matches!(op, BinOp::Mul(_)) {
        if let NodeKind::NamedOp(m) = &rhs.kind {
            if m.name == "mod" && m.args.len() == 1 {
                return Some(((**lhs).clone(), m.args.first()?.clone()));
            }
        }
    }
    let (inner, modulus) = split_pmod(rhs)?;
    let span = lhs.span.join(inner.span);
    let side = Node::new(
        NodeKind::Binary {
            op: *op,
            lhs: lhs.clone(),
            rhs: Box::new(inner),
        },
        span,
    );
    Some((side, modulus))
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

#[cfg(test)]
mod tests {
    use crate::math::ast::{BinOp, NodeKind, Statement};
    use crate::math::parse_math;

    #[test]
    fn congruence_check_compares_remainders() {
        let src = r"(a + 1)^p \overset{?}{\equiv} a^p + 1 \pmod p";
        let (ast, diags) = parse_math(src, 0);
        assert!(diags.is_empty(), "{diags:?}");
        let Statement::Check { lhs, rhs } = &ast.lines[0].statement else {
            panic!("expected a check");
        };
        for side in [lhs, rhs] {
            let NodeKind::Binary { op, rhs: m, .. } = &side.kind else {
                panic!("expected a remainder");
            };
            assert_eq!(*op, BinOp::Mod);
            assert_eq!(&src[m.span.start..m.span.end], "p");
        }
        let NodeKind::Binary { lhs: b, .. } = &rhs.kind else {
            unreachable!()
        };
        assert_eq!(&src[b.span.start..b.span.end], "a^p + 1");
    }

    #[test]
    fn congruence_check_needs_a_modulus() {
        let (_, diags) = parse_math(r"2^{10} \overset{?}{\equiv} 1", 0);
        assert!(diags.iter().any(|d| d.message.contains("modulus")));
    }
}
