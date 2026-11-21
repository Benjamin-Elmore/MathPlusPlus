//! Big operators: sums, products, integrals, limits, derivatives and ∇.

use super::expr::{attach_sub, bound_letter, is_d, is_sign, name_node, rebind, starts_variable};
use super::{Ctx, Parser};
use crate::lexer::{Delim, Op, Tok};
use crate::math::ast::{
    BigOp, BigOpKind, BinOp, Bracket, DerivNotation, DerivVar, Derivative, IntKind, Integral,
    Limit, MulStyle, NablaOp, Name, Node, NodeKind, RelOp, Side, UnaryOp,
};
use crate::span::Span;

impl Parser<'_> {
    /// A big operator or derivative starting at the next token, if there is one.
    pub(super) fn try_calculus(&mut self) -> Option<Node> {
        match self.peek()? {
            Tok::Sym(s) => match s.name {
                "sum" => Some(self.big_op(BigOpKind::Sum)),
                "prod" => Some(self.big_op(BigOpKind::Prod)),
                "coprod" => Some(self.big_op(BigOpKind::Coprod)),
                "int" => Some(self.integral(IntKind::Single)),
                "iint" => Some(self.integral(IntKind::Double)),
                "iiint" => Some(self.integral(IntKind::Triple)),
                "oint" => Some(self.integral(IntKind::Contour)),
                "oiint" => Some(self.integral(IntKind::Surface)),
                "lim" => Some(self.limit()),
                "nabla" => Some(self.nabla()),
                "partial" => Some(self.partial()),
                _ => None,
            },
            Tok::Letter('d') | Tok::Styled { .. } => self.try_slash_derivative(false),
            Tok::Letter('D') if matches!(self.peek_n(1), Some(Tok::Underscore)) => self.try_d_sub(),
            _ => None,
        }
    }

    /// `_lower` and `^upper` in either order (LaTeX, plain or Unicode scripts).
    fn limits(&mut self, lower_what: &str, upper_what: &str) -> (Option<Node>, Option<Node>) {
        let mut lower = None;
        let mut upper = None;
        loop {
            let span = self.cur_span();
            match self.peek().cloned() {
                Some(Tok::Underscore) if lower.is_none() => {
                    self.bump();
                    lower = Some(self.parse_script(false, lower_what));
                }
                Some(Tok::Caret) if upper.is_none() => {
                    self.bump();
                    upper = Some(self.parse_script(true, upper_what));
                }
                Some(Tok::Subscript(text)) if lower.is_none() => {
                    self.bump();
                    lower = Some(self.fragment(&text, span));
                }
                Some(Tok::Superscript(text)) if upper.is_none() => {
                    self.bump();
                    upper = Some(self.fragment(&text, span));
                }
                _ => break,
            }
        }
        (lower, upper)
    }

    /// The operand of a big operator: the multiplicative term to its right.
    pub(super) fn op_body(&mut self, what: &str) -> Node {
        if self.at(is_sign) {
            return self.parse_signed_term();
        }
        if self.can_start_factor() {
            return self.parse_term();
        }
        self.missing(what, &["an expression"])
    }

    /// `\sum_{n=1}^{\infty} term`, `\prod`, `\coprod`.
    fn big_op(&mut self, op: BigOpKind) -> Node {
        let start = self.cur_span();
        let sign = self.text(start).to_string();
        self.bump();
        let (lower, to) = self.limits("the index after `_`", "the upper limit after `^`");
        let mut body = self.op_body(&format!("the term after `{sign}`"));
        let (mut index, mut from, mut over) = (None, None, None);
        if let Some(lower) = lower {
            match &lower.ungrouped().kind {
                NodeKind::Relation { first, rest }
                    if rest.len() == 1 && rest[0].0 == RelOp::Eq && first.is_name() =>
                {
                    index = Some(name_node(first));
                    from = Some(rest[0].1.clone());
                }
                _ if lower.is_name() => index = Some(name_node(&lower)),
                _ => over = Some(lower),
            }
        }
        if let Some(letter) = index.as_ref().and_then(bound_letter) {
            rebind(&mut body, letter);
        }
        let span = start.join(body.span);
        Node::new(
            NodeKind::BigOp(Box::new(BigOp {
                op,
                index,
                from,
                to,
                over,
                body,
            })),
            span,
        )
    }

    /// `\int_a^b body \, dx` and the multiple and closed integrals.
    fn integral(&mut self, kind: IntKind) -> Node {
        let start = self.cur_span();
        self.bump();
        // A sign on its own (`Exchange $\frac{d}{dt}$ and $\int$`) names the operator in
        // prose: it is the symbol, not an integral with its parts missing.
        if self.at_end() {
            let symbol = match kind {
                IntKind::Single => "∫",
                IntKind::Double => "∬",
                IntKind::Triple => "∭",
                IntKind::Contour => "∮",
                IntKind::Surface => "∯",
            };
            return Node::new(NodeKind::Symbol(symbol), start);
        }
        let (lower, upper) = self.limits("the lower bound after `_`", "the upper bound after `^`");
        let ctx = self.integral_ctx();
        if matches!(kind, IntKind::Single | IntKind::Contour) {
            if let Some((body, diffs)) = self.with_ctx(ctx, |p| p.differential_form()) {
                let end = diffs.last().map_or(body.span, |d| d.span);
                return Node::new(
                    NodeKind::Integral(Box::new(Integral {
                        kind,
                        lower,
                        upper,
                        body,
                        diffs,
                        dot: true,
                        diff_in_numerator: false,
                        form: true,
                    })),
                    start.join(end),
                );
            }
        }
        let (body, diffs, dot) = self.with_ctx(ctx, |p| {
            if p.differential_at(0) {
                // `\int dx \, f(x)`: differentials first.
                let diffs = p.differentials(kind);
                let plain = Ctx {
                    in_integral: false,
                    ..p.ctx
                };
                let body = p.with_ctx(plain, |p| {
                    if p.can_start_factor() {
                        p.parse_term()
                    } else {
                        let at = Span::point(p.prev_end());
                        Node::new(NodeKind::Number("1".to_string()), at)
                    }
                });
                return (body, diffs, false);
            }
            let body = if p.can_start_factor() || p.at(is_sign) {
                p.parse_additive()
            } else {
                p.missing("the integrand", &["an expression"])
            };
            let dot = p.at(|t| matches!(t, Tok::Op(Op::Cdot))) && p.differential_at(1);
            if dot {
                p.bump();
            }
            (body, p.differentials(kind), dot)
        });
        let (body, diffs, diff_in_numerator) = if diffs.is_empty() {
            match numerator_differential(body) {
                Ok((body, var)) => (body, vec![var], true),
                Err(body) => (body, diffs, false),
            }
        } else {
            (body, diffs, false)
        };
        if diffs.is_empty() {
            let at = Span::point(self.cur_span().start.max(body.span.end));
            self.error(
                at,
                "expected a differential such as `dx` to close the integral",
                &["`dx`", "`dA`", "`dV`"],
            );
        }
        let end = diffs.last().map_or(body.span, |d| d.span);
        Node::new(
            NodeKind::Integral(Box::new(Integral {
                kind,
                lower,
                upper,
                body,
                diffs,
                dot,
                diff_in_numerator,
                form: false,
            })),
            start.join(end),
        )
    }

    /// `(P\,dx + Q\,dy)` after a line-integral sign: a bracketed sum whose terms each end
    /// in a differential of a different variable. Returns the coefficients as a tuple
    /// and the variables, or `None` (having read nothing) when the bracket is not one.
    fn differential_form(&mut self) -> Option<(Node, Vec<Node>)> {
        let open = self.cur_span();
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
        self.bump();
        let mut coefs = Vec::new();
        let mut diffs: Vec<Node> = Vec::new();
        loop {
            let neg = match self.peek() {
                Some(Tok::Op(Op::Minus)) => Some(self.cur_span()),
                _ => None,
            };
            if neg.is_some() || self.at(|t| matches!(t, Tok::Op(Op::Plus))) {
                self.bump();
            }
            // `dx` alone has the coefficient 1.
            let coef = if self.differential_at(0) {
                Node::new(
                    NodeKind::Number("1".to_string()),
                    Span::point(self.cur_span().start),
                )
            } else if self.can_start_factor() {
                self.parse_term()
            } else {
                self.restore(cp);
                return None;
            };
            let d = self.differentials(IntKind::Single);
            let Some(var) = d.into_iter().next() else {
                self.restore(cp);
                return None;
            };
            let same = |a: &Node, b: &Node| {
                crate::math::sexpr::node(a, crate::math::sexpr::Mode::Semantic)
                    == crate::math::sexpr::node(b, crate::math::sexpr::Mode::Semantic)
            };
            if diffs.iter().any(|v| same(v, &var)) {
                self.restore(cp);
                return None;
            }
            coefs.push(match neg {
                Some(at) => super::expr::unary(UnaryOp::Neg, at, coef),
                None => coef,
            });
            diffs.push(var);
            match self.peek() {
                Some(Tok::Op(Op::Plus | Op::Minus)) => continue,
                Some(Tok::Close { .. }) => {
                    let end = self.cur_span();
                    self.bump();
                    let body = Node::new(
                        NodeKind::Tuple {
                            elems: coefs,
                            bracket: Bracket::Paren,
                            sized: false,
                        },
                        open.join(end),
                    );
                    return Some((body, diffs));
                }
                _ => {
                    self.restore(cp);
                    return None;
                }
            }
        }
    }

    /// `d x`, `dx dy`, `\mathrm{d} x`, up to as many as the integral sign takes.
    fn differentials(&mut self, kind: IntKind) -> Vec<Node> {
        let mut diffs = Vec::new();
        while diffs.len() < kind.arity() && self.differential_at(0) {
            let d = self.cur_span();
            self.bump();
            match self.deriv_var() {
                Some(var) => {
                    let span = d.join(var.span);
                    diffs.push(Node::new(var.into_parts().0, span));
                }
                None => break,
            }
        }
        diffs
    }

    /// A variable in a differential or a derivative denominator: a name with an
    /// optional subscript, or a bracket for Jacobians (`\partial(u, v)`).
    fn deriv_var(&mut self) -> Option<Node> {
        match self.peek()? {
            t if starts_variable(t) => {
                let mut node = self.parse_atom();
                let span = self.cur_span();
                match self.peek().cloned() {
                    Some(Tok::Underscore) => {
                        self.bump();
                        let sub = self.parse_sub_script();
                        node = attach_sub(node, sub);
                    }
                    Some(Tok::Subscript(text)) => {
                        self.bump();
                        let sub = self.fragment(&text, span);
                        node = attach_sub(node, sub);
                    }
                    _ => {}
                }
                Some(name_node(&node))
            }
            Tok::Open { .. } => Some(self.parse_atom()),
            _ => None,
        }
    }

    /// `^n`, `^{n}` or `ⁿ` after a `d`, `∂` or a derivative variable.
    fn deriv_power(&mut self) -> Option<Node> {
        let span = self.cur_span();
        match self.peek().cloned()? {
            Tok::Caret => {
                self.bump();
                Some(self.parse_script(true, "the order after `^`"))
            }
            Tok::Superscript(text) => {
                self.bump();
                Some(self.fragment(&text, span))
            }
            _ => None,
        }
    }

    /// `\lim_{x \to a} body`, `lim_(x->a^+) body`.
    fn limit(&mut self) -> Node {
        let start = self.cur_span();
        self.bump();
        let mut side = None;
        let (var, target) = if self.eat(|t| matches!(t, Tok::Underscore)).is_some() {
            if self.at(|t| matches!(t, Tok::LBrace | Tok::Open { .. })) {
                self.limit_bracket(&mut side)
            } else {
                let script = self.parse_script(false, "`x \\to a` after `\\lim_`");
                self.split_approach(script)
            }
        } else {
            let at = self.cur_span();
            self.error(
                at,
                "expected `_{x \\to a}` after `\\lim`",
                &["`_{x \\to a}`", "`_(x->a)`"],
            );
            (Node::error(at), Node::error(at))
        };
        let mut body = self.op_body("the expression after `\\lim`");
        if let Some(letter) = bound_letter(&var) {
            rebind(&mut body, letter);
        }
        let span = start.join(body.span);
        Node::new(
            NodeKind::Limit(Box::new(Limit {
                var,
                target,
                side,
                body,
            })),
            span,
        )
    }

    /// The bracketed part of `\lim_{x \to 0^+}`; reads the one-sided mark first.
    fn limit_bracket(&mut self, side: &mut Option<Side>) -> (Node, Node) {
        let open = self.pos;
        let close = self.matching_close(open);
        let kind_at = |i: usize| -> Option<&Tok> {
            if i > open && i < close {
                self.toks.get(i).map(|t| &t.kind)
            } else {
                None
            }
        };
        let sign = |t: Option<&Tok>| match t {
            Some(Tok::Op(Op::Plus)) => Some(Side::Right),
            Some(Tok::Op(Op::Minus)) => Some(Side::Left),
            _ => None,
        };
        let mut cut = close;
        let back = |n: usize| close.checked_sub(n);
        if let (Some(a), Some(b)) = (back(2), back(1)) {
            if matches!(kind_at(a), Some(Tok::Caret)) && sign(kind_at(b)).is_some() {
                *side = sign(kind_at(b));
                cut = a;
            }
        }
        if side.is_none() {
            if let (Some(a), Some(b), Some(c), Some(d)) = (back(4), back(3), back(2), back(1)) {
                if matches!(kind_at(a), Some(Tok::Caret))
                    && matches!(kind_at(b), Some(Tok::LBrace))
                    && sign(kind_at(c)).is_some()
                    && matches!(kind_at(d), Some(Tok::RBrace))
                {
                    *side = sign(kind_at(c));
                    cut = a;
                }
            }
        }
        if side.is_none() {
            if let Some(b) = back(1) {
                match kind_at(b) {
                    Some(Tok::Superscript(s)) if s == "+" || s == "-" => {
                        *side = Some(if s == "+" { Side::Right } else { Side::Left });
                        cut = b;
                    }
                    Some(Tok::Op(Op::Plus | Op::Minus))
                        if back(2).is_some_and(|a| {
                            !matches!(kind_at(a), Some(Tok::Op(_) | Tok::Rel(_)) | None)
                        }) =>
                    {
                        *side = sign(kind_at(b));
                        cut = b;
                    }
                    _ => {}
                }
            }
        }
        let open_span = self.cur_span();
        self.bump();
        let old_stop = self.stop;
        self.stop = cut.min(old_stop);
        let (var, target) = self.nested(|p| {
            let var = if p.at(starts_variable) {
                let v = p.parse_postfix();
                name_node(&v)
            } else {
                p.missing("the limit variable", &["a name such as `x`"])
            };
            let target = if p
                .eat(|t| matches!(t, Tok::Rel(RelOp::To | RelOp::MapsTo)))
                .is_some()
            {
                p.parse_expr()
            } else {
                p.missing("`\\to` in the limit", &["`\\to`", "`->`", "`→`"])
            };
            p.expect_end("the end of the limit");
            (var, target)
        });
        self.stop = old_stop;
        self.pos = cut;
        // Skip the one-sided mark, then the closing bracket.
        while self.pos < close.min(self.stop) {
            self.pos += 1;
        }
        if self
            .eat(|t| matches!(t, Tok::RBrace | Tok::Close { .. }))
            .is_none()
        {
            let at = self.cur_span();
            let text = self.text(open_span).to_string();
            self.error(
                Span::point(at.start),
                format!("expected the bracket that closes `{text}`"),
                &["`}`", "`)`"],
            );
        }
        (var, target)
    }

    /// `x \to a` written as a single script atom.
    fn split_approach(&mut self, script: Node) -> (Node, Node) {
        if let NodeKind::Relation { first, rest } = &script.kind {
            if rest.len() == 1 && matches!(rest[0].0, RelOp::To | RelOp::MapsTo) {
                return (name_node(first), rest[0].1.clone());
            }
        }
        self.error(
            script.span,
            "expected `x \\to a` under the limit",
            &["`x \\to a`"],
        );
        (Node::error(script.span), Node::error(script.span))
    }

    /// `\nabla f`, `\nabla \cdot F`, `\nabla \times F`, `\nabla^2 f`.
    fn nabla(&mut self) -> Node {
        let start = self.cur_span();
        self.bump();
        let laplacian = match (self.peek(), self.peek_n(1)) {
            (Some(Tok::Caret), Some(Tok::Number(n))) if n == "2" => {
                self.bump();
                self.bump();
                true
            }
            (Some(Tok::Superscript(n)), _) if n == "2" => {
                self.bump();
                true
            }
            _ => false,
        };
        let op = if laplacian {
            NablaOp::Laplacian
        } else if self.eat(|t| matches!(t, Tok::Op(Op::Cdot))).is_some() {
            NablaOp::Div
        } else if self.eat(|t| matches!(t, Tok::Op(Op::Times))).is_some() {
            NablaOp::Curl
        } else {
            NablaOp::Grad
        };
        let body = if self.can_start_factor() {
            Some(Box::new(self.parse_factor()))
        } else {
            if op != NablaOp::Grad {
                let node = self.missing("the field after `\\nabla`", &["an expression"]);
                return Node::new(
                    NodeKind::Nabla {
                        op,
                        body: Some(Box::new(node)),
                    },
                    start.join(self.prev_span()),
                );
            }
            None
        };
        let end = body.as_ref().map_or(self.prev_span(), |b| b.span);
        Node::new(NodeKind::Nabla { op, body }, start.join(end))
    }

    /// `∂f/∂x`, `\partial_x f`, `\partial_{xy} f`, or a bare `\partial f`.
    fn partial(&mut self) -> Node {
        if let Some(d) = self.try_slash_derivative(true) {
            return d;
        }
        let start = self.cur_span();
        self.bump();
        let span = self.cur_span();
        let sub = match self.peek().cloned() {
            Some(Tok::Underscore) => {
                self.bump();
                Some(self.parse_sub_script())
            }
            Some(Tok::Subscript(text)) => {
                self.bump();
                Some(self.fragment(&text, span))
            }
            _ => None,
        };
        let Some(sub) = sub else {
            let of = self.can_start_factor().then(|| self.parse_postfix());
            let end = of.as_ref().map_or(start, |n| n.span);
            return deriv(
                Derivative {
                    partial: true,
                    order: None,
                    vars: Vec::new(),
                    of,
                    notation: DerivNotation::Bare,
                },
                start.join(end),
            );
        };
        let order = self.deriv_power();
        let vars = split_vars(sub);
        let of = self.can_start_factor().then(|| self.parse_term());
        let end = of.as_ref().map_or(self.prev_span(), |n| n.span);
        deriv(
            Derivative {
                partial: true,
                order,
                vars,
                of,
                notation: DerivNotation::PartialSub,
            },
            start.join(end),
        )
    }

    /// `D_x f`; backtracks when it is just the name `D_x`.
    fn try_d_sub(&mut self) -> Option<Node> {
        let cp = self.checkpoint();
        let start = self.cur_span();
        self.bump();
        self.bump();
        let sub = self.parse_sub_script();
        let is_var = sub
            .as_name()
            .is_some_and(|n| n.base.chars().all(char::is_alphabetic));
        if !is_var || !self.can_start_factor() {
            self.restore(cp);
            return None;
        }
        let of = self.parse_term();
        let span = start.join(of.span);
        Some(deriv(
            Derivative {
                partial: false,
                order: None,
                vars: vec![DerivVar {
                    var: name_node(&sub),
                    power: None,
                }],
                of: Some(of),
                notation: DerivNotation::DSub,
            },
            span,
        ))
    }

    /// `d/dx f`, `dy/dx`, `d^2y/dx^2`, `∂f/∂x`; backtracks on anything else.
    fn try_slash_derivative(&mut self, partial: bool) -> Option<Node> {
        if !is_d(self.peek(), partial) {
            return None;
        }
        let cp = self.checkpoint();
        let result = self.slash_derivative(partial);
        if result.is_none() {
            self.restore(cp);
        }
        result
    }

    fn slash_derivative(&mut self, partial: bool) -> Option<Node> {
        let start = self.cur_span();
        self.bump();
        let order = self.deriv_power();
        let of = if self.at(|t| matches!(t, Tok::Op(Op::Slash))) {
            None
        } else {
            Some(self.deriv_var()?)
        };
        self.eat(|t| matches!(t, Tok::Op(Op::Slash)))?;
        let vars = self.deriv_denominator(partial);
        if vars.is_empty() {
            return None;
        }
        let whole = of.is_some();
        let of = match of {
            Some(of) => Some(of),
            None => self.can_start_factor().then(|| self.parse_term()),
        };
        let end = of
            .as_ref()
            .filter(|_| !whole)
            .map_or(self.prev_span(), |n| n.span);
        let notation = if whole {
            DerivNotation::Whole { frac: false }
        } else {
            DerivNotation::Operator { frac: false }
        };
        Some(deriv(
            Derivative {
                partial,
                order,
                vars,
                of,
                notation,
            },
            start.join(end),
        ))
    }

    /// `d x^2 d y …` in a derivative denominator.
    fn deriv_denominator(&mut self, partial: bool) -> Vec<DerivVar> {
        let mut vars = Vec::new();
        while is_d(self.peek(), partial) {
            let next_ok = self
                .peek_n(1)
                .is_some_and(|t| starts_variable(t) || matches!(t, Tok::Open { .. }));
            if !next_ok {
                break;
            }
            self.bump();
            let Some(var) = self.deriv_var() else {
                break;
            };
            let power = self.deriv_power();
            vars.push(DerivVar { var, power });
        }
        vars
    }

    /// `\frac{d}{dx}`, `\frac{dy}{dx}`, `\frac{\partial^2 f}{\partial x \partial y}`.
    /// Called just after `\frac`; backtracks when the fraction is not a derivative.
    pub(super) fn try_derivative_frac(&mut self, start: Span) -> Option<Node> {
        if !matches!(self.peek(), Some(Tok::LBrace)) {
            return None;
        }
        let partial = match self.peek_n(1) {
            t if is_d(t, true) => true,
            t if is_d(t, false) => false,
            _ => return None,
        };
        let cp = self.checkpoint();
        let result = self.derivative_frac(partial, start);
        if result.is_none() {
            self.restore(cp);
        }
        result
    }

    fn derivative_frac(&mut self, partial: bool, start: Span) -> Option<Node> {
        self.bump();
        self.bump();
        let order = self.deriv_power();
        let of = if self.at(|t| matches!(t, Tok::RBrace)) {
            None
        } else {
            Some(self.nested(|p| p.parse_relation()))
        };
        self.eat(|t| matches!(t, Tok::RBrace))?;
        self.eat(|t| matches!(t, Tok::LBrace))?;
        let vars = self.nested(|p| p.deriv_denominator(partial));
        if vars.is_empty() {
            return None;
        }
        self.eat(|t| matches!(t, Tok::RBrace))?;
        let frac_end = self.prev_span();
        let whole = of.is_some();
        let (of, end) = match of {
            Some(of) => (Some(of), frac_end),
            None => {
                let body = self.can_start_factor().then(|| self.parse_term());
                let end = body.as_ref().map_or(frac_end, |b| b.span);
                (body, end)
            }
        };
        let notation = if whole {
            DerivNotation::Whole { frac: true }
        } else {
            DerivNotation::Operator { frac: true }
        };
        Some(deriv(
            Derivative {
                partial,
                order,
                vars,
                of,
                notation,
            },
            start.join(end),
        ))
    }
}

/// `\frac{dx}{x}` as an integral body: takes the `dx` out, leaving `\frac{1}{x}`.
fn numerator_differential(body: Node) -> Result<(Node, Node), Node> {
    let (kind, body_span) = body.into_parts();
    let NodeKind::Frac { num, den, style } = kind else {
        return Err(Node::new(kind, body_span));
    };
    let split = match &num.kind {
        NodeKind::Binary {
            op: BinOp::Mul(MulStyle::Implicit),
            lhs,
            rhs,
        } if is_d_name(lhs) && rhs.is_name() => Some((lhs.span, name_node(rhs))),
        _ => None,
    };
    match split {
        Some((d_span, var)) => {
            let one = Node::new(NodeKind::Number("1".to_string()), d_span);
            let var_span = d_span.join(var.span);
            let var = Node::new(var.into_parts().0, var_span);
            let frac = Node::new(
                NodeKind::Frac {
                    num: Box::new(one),
                    den,
                    style,
                },
                body_span,
            );
            Ok((frac, var))
        }
        None => Err(Node::new(NodeKind::Frac { num, den, style }, body_span)),
    }
}

/// The letter `d` (or an upright `d`) as a name node.
fn is_d_name(node: &Node) -> bool {
    matches!(&node.kind, NodeKind::Name(n) if n.base == "d" && n.sub.is_none() && n.accent.is_none())
}

fn deriv(d: Derivative, span: Span) -> Node {
    Node::new(NodeKind::Derivative(Box::new(d)), span)
}

/// `\partial_{xy}` means ∂x ∂y; a tuple subscript lists the variables.
fn split_vars(sub: Node) -> Vec<DerivVar> {
    let plain = |v: Node| DerivVar {
        var: v,
        power: None,
    };
    match &sub.kind {
        NodeKind::Name(n)
            if n.base.chars().count() > 1
                && n.sub.is_none()
                && n.font.is_none()
                && n.accent.is_none() =>
        {
            n.base
                .chars()
                .map(|c| {
                    plain(Node::new(
                        NodeKind::Name(Name::simple(c.to_string())),
                        sub.span,
                    ))
                })
                .collect()
        }
        NodeKind::Tuple { elems, .. } => elems.iter().map(|e| plain(name_node(e))).collect(),
        _ => vec![plain(name_node(&sub))],
    }
}
