//! Precedence levels: relation, logic, additive, multiplicative, postfix, scripts.

use super::{Ctx, Parser};
use crate::lexer::{Delim, Op, Tok};
use crate::math::ast::{
    Accent, BinOp, Bracket, Constant, DerivNotation, Derivative, FracStyle, MulStyle, Name, Node,
    NodeKind, RelOp, UnaryOp, UnitText,
};
use crate::span::Span;
use crate::symbols::{Font, SymbolKind};

/// Operators at the multiplicative level.
enum MulOp {
    Bin(BinOp),
    Frac(FracStyle),
}

impl Parser<'_> {
    /// A relation chain (`a = b < c`), stopping before `= ?` so a query can see it.
    pub(super) fn parse_relation(&mut self) -> Node {
        let first = self.parse_expr();
        let first = self.maybe_convert(first);
        self.relation_tail(first)
    }

    /// Like [`Parser::parse_relation`], but a line may start with the relation
    /// (`&= x + 1` on an aligned line).
    pub(super) fn parse_line_relation(&mut self) -> Node {
        let first = if self.at(|t| matches!(t, Tok::Rel(_))) {
            Node::new(NodeKind::Continuation, Span::point(self.cur_span().start))
        } else {
            let e = self.parse_expr();
            self.maybe_convert(e)
        };
        self.relation_tail(first)
    }

    fn relation_tail(&mut self, first: Node) -> Node {
        let mut rest = Vec::new();
        while let Some(Tok::Rel(op)) = self.peek() {
            let op = *op;
            let query = matches!(op, RelOp::Eq | RelOp::Approx)
                && matches!(self.peek_n(1), Some(Tok::Question));
            if query {
                break;
            }
            self.bump();
            let rhs = self.parse_expr();
            let rhs = self.maybe_convert(rhs);
            rest.push((op, rhs));
        }
        if rest.is_empty() {
            return first;
        }
        let end = rest.last().map_or(first.span, |(_, n)| n.span);
        let span = first.span.join(end);
        Node::new(
            NodeKind::Relation {
                first: Box::new(first),
                rest,
            },
            span,
        )
    }

    /// An expression without relations: the logic level and below.
    pub(super) fn parse_expr(&mut self) -> Node {
        let mut lhs = self.parse_and();
        let mut count = 0;
        while self.at(|t| matches!(t, Tok::Op(Op::Or))) && self.chain_ok(count) {
            count += 1;
            self.bump();
            let rhs = self.parse_and();
            lhs = binary(BinOp::Or, lhs, rhs);
        }
        lhs
    }

    fn parse_and(&mut self) -> Node {
        let mut lhs = self.parse_not();
        let mut count = 0;
        while self.at(|t| matches!(t, Tok::Op(Op::And))) && self.chain_ok(count) {
            count += 1;
            self.bump();
            let rhs = self.parse_not();
            lhs = binary(BinOp::And, lhs, rhs);
        }
        lhs
    }

    fn parse_not(&mut self) -> Node {
        let Some(tok) = self.eat(|t| matches!(t, Tok::Op(Op::Not))) else {
            return self.parse_additive();
        };
        if !self.enter() {
            return self.too_deep();
        }
        let operand = self.parse_not();
        self.leave();
        unary(UnaryOp::Not, tok.span, operand)
    }

    /// `[sign] term { (+ | − | ± | ∓ | ∪ | ∖) term }`.
    pub(super) fn parse_additive(&mut self) -> Node {
        let mut lhs = self.parse_signed_term();
        for count in 0.. {
            let op = match self.peek() {
                Some(Tok::Op(Op::Plus)) => BinOp::Add,
                Some(Tok::Op(Op::Minus)) => BinOp::Sub,
                Some(Tok::Op(Op::PlusMinus)) => BinOp::PlusMinus,
                Some(Tok::Op(Op::MinusPlus)) => BinOp::MinusPlus,
                Some(Tok::Op(Op::Cup)) => BinOp::Union,
                Some(Tok::Op(Op::SetMinus)) => BinOp::SetMinus,
                _ => break,
            };
            if !self.chain_ok(count) {
                break;
            }
            self.bump();
            let rhs = self.parse_signed_term();
            lhs = binary(op, lhs, rhs);
        }
        lhs
    }

    /// A term with an optional leading sign.
    pub(super) fn parse_signed_term(&mut self) -> Node {
        let Some(op) = self.peek().and_then(sign_op) else {
            return self.parse_term();
        };
        let tok = self.bump();
        if !self.enter() {
            return self.too_deep();
        }
        let operand = self.parse_signed_term();
        self.leave();
        unary(op, tok.map_or(operand.span, |t| t.span), operand)
    }

    fn parse_signed_factor(&mut self) -> Node {
        let Some(op) = self.peek().and_then(sign_op) else {
            return self.parse_factor();
        };
        let tok = self.bump();
        if !self.enter() {
            return self.too_deep();
        }
        let operand = self.parse_signed_factor();
        self.leave();
        unary(op, tok.map_or(operand.span, |t| t.span), operand)
    }

    /// `factor { [mul_op] factor }`; no operator means an implicit product.
    pub(super) fn parse_term(&mut self) -> Node {
        let mut lhs = self.parse_factor();
        for count in 0.. {
            if !self.chain_ok(count) {
                break;
            }
            if let Some(Tok::Op(op)) = self.peek() {
                let op = *op;
                let Some(mul) = mul_op(op) else { break };
                if self.ctx.func_arg {
                    break;
                }
                if self.ctx.in_integral && op == Op::Cdot && self.differential_at(1) {
                    break;
                }
                self.bump();
                let rhs = self.parse_signed_factor();
                lhs = match mul {
                    MulOp::Bin(b) => binary(b, lhs, rhs),
                    MulOp::Frac(style) => frac(lhs, rhs, style),
                };
                continue;
            }
            if self.can_start_factor() {
                let rhs = self.parse_factor();
                lhs = binary(BinOp::Mul(MulStyle::Implicit), lhs, rhs);
                continue;
            }
            break;
        }
        lhs
    }

    /// A big operator or a postfix expression.
    pub(super) fn parse_factor(&mut self) -> Node {
        if !self.enter() {
            return self.too_deep();
        }
        let node = self.parse_postfix();
        self.leave();
        node
    }

    /// An atom followed by `^`, `_`, primes, `!`, Unicode scripts, accents and call brackets.
    pub(super) fn parse_postfix(&mut self) -> Node {
        let mut node = self.parse_atom();
        if matches!(node.kind, NodeKind::Number(_)) {
            node = self.with_unit(node);
        }
        let mut count = 0;
        while let Some(tok) = self.peek().cloned() {
            if !self.chain_ok(count) {
                break;
            }
            count += 1;
            let span = self.cur_span();
            node = match tok {
                Tok::Caret => {
                    self.bump();
                    if let Some(count) = self.script_prime() {
                        let end = self.prev_end();
                        prime(node, count, end)
                    } else {
                        let exp = self.parse_script(true, "the exponent after `^`");
                        pow(node, exp)
                    }
                }
                Tok::Superscript(text) => {
                    self.bump();
                    let exp = self.fragment(&text, span);
                    pow(node, exp)
                }
                Tok::Underscore => {
                    self.bump();
                    let sub = self.parse_sub_script();
                    attach_sub(node, sub)
                }
                Tok::Subscript(text) => {
                    self.bump();
                    let sub = self.fragment(&text, span);
                    attach_sub(node, sub)
                }
                Tok::Prime(count) => {
                    self.bump();
                    prime(node, count, span.end)
                }
                Tok::Bang => {
                    self.bump();
                    factorial(node, span.end)
                }
                Tok::Combining(accent) => {
                    self.bump();
                    let full = node.span.join(span);
                    make_accent(accent, node, full)
                }
                Tok::Open {
                    delim: Delim::Paren,
                    ..
                } if callable(&node) => self.call_node(node),
                // `\mathbf 1[x^2 + y^2 \le 1]`: the indicator of a condition.
                Tok::Open {
                    delim: Delim::Square,
                    ..
                } if is_indicator(&node) => self.call_node(node),
                _ => break,
            };
        }
        if is_power_of_ten(&node) {
            // `1.5 \times 10^{3}\,\mathrm{m}`: the unit belongs to the whole number.
            node = self.with_unit(node);
        }
        node
    }

    /// `name(args…)`: the call whose arguments start at the bracket ahead.
    fn call_node(&mut self, callee: Node) -> Node {
        let (args, sized, end) = self.call_args();
        let span = callee.span.join(end);
        Node::new(
            NodeKind::Call {
                callee: Box::new(callee),
                args,
                sized,
            },
            span,
        )
    }

    /// A number followed by a unit becomes a quantity (`9.81 m/s^2`, `30^\circ`).
    fn with_unit(&mut self, value: Node) -> Node {
        // The end of the last token read (a braced exponent's `}` included).
        let start = self.prev_end().max(value.span.end);
        let Some(text) = start.checked_sub(self.base).and_then(|r| self.src.get(r..)) else {
            return value;
        };
        let Some(found) = crate::units::after_number(text) else {
            return value;
        };
        // In an integrand, `1 \, dA` ends with the differential, not deci-amperes.
        if self.ctx.in_integral && found.plain && found.text.starts_with('d') {
            return value;
        }
        let end = start + found.len;
        if !self.consume_to(end) {
            return value;
        }
        let unit = UnitText {
            text: found.text,
            plain: found.plain,
            span: Span::new(start + found.lead, end),
        };
        let span = value.span.join(unit.span);
        Node::new(
            NodeKind::Quantity {
                value: Box::new(value),
                unit,
            },
            span,
        )
    }

    /// Consumes the tokens up to the byte offset `end`, which must fall between tokens.
    fn consume_to(&mut self, end: usize) -> bool {
        let mut k = self.pos;
        let mut last = None;
        while k < self.stop {
            let Some(t) = self.toks.get(k) else { break };
            if t.span.start >= end {
                break;
            }
            if t.span.end > end {
                return false;
            }
            last = Some(t.span.end);
            k += 1;
        }
        if last != Some(end) {
            return false;
        }
        self.pos = k;
        true
    }

    /// True when the next token is a bare `in` followed by a unit that reads as a
    /// conversion target (see [`Parser::maybe_convert`]).
    pub(super) fn at_unit_conversion(&self) -> bool {
        let Some(tok) = self.toks.get(self.pos).filter(|_| self.pos < self.stop) else {
            return false;
        };
        if !matches!(tok.kind, Tok::Rel(RelOp::In)) || self.text(tok.span) != "in" {
            return false;
        }
        tok.span
            .end
            .checked_sub(self.base)
            .and_then(|r| self.src.get(r..))
            .and_then(crate::units::conversion_target)
            .is_some_and(|f| !f.plain || crate::units::is_compound(&f.text))
    }

    /// `expr \operatorname{in} unit`, `expr in unit` or `expr \text{ in } unit`: a
    /// conversion. A bare `in` that is not followed by a unit stays the relation `∈`.
    pub(super) fn maybe_convert(&mut self, expr: Node) -> Node {
        let Some(tok) = self.toks.get(self.pos).filter(|_| self.pos < self.stop) else {
            return expr;
        };
        let tok_span = tok.span;
        let explicit = match &tok.kind {
            Tok::OperatorName(n) if n == "in" => true,
            Tok::Text(t) if t.trim() == "in" => true,
            Tok::Rel(RelOp::In) if self.text(tok_span) == "in" => false,
            _ => return expr,
        };
        let Some(text) = tok_span
            .end
            .checked_sub(self.base)
            .and_then(|r| self.src.get(r..))
        else {
            return expr;
        };
        let Some(found) = crate::units::conversion_target(text) else {
            if explicit {
                self.bump();
                // Read what was written there, so it is reported once.
                let what = if self.can_start_factor() {
                    self.parse_postfix().span
                } else {
                    Span::point(tok_span.end)
                };
                self.error(
                    what,
                    "expected a unit after `in`".to_string(),
                    &["a unit such as `\\mathrm{km/h}`"],
                );
            }
            return expr;
        };
        // `x in S` is membership: after a bare `in`, a one-letter plain unit is not enough.
        if !explicit && found.plain && !crate::units::is_compound(&found.text) {
            return expr;
        }
        let end = tok_span.end + found.len;
        let saved = self.pos;
        self.bump();
        if !self.consume_to(end) {
            self.pos = saved;
            return expr;
        }
        let unit = UnitText {
            text: found.text,
            plain: found.plain,
            span: Span::new(tok_span.end + found.lead, end),
        };
        let span = expr.span.join(unit.span);
        Node::new(
            NodeKind::Convert {
                expr: Box::new(expr),
                unit,
            },
            span,
        )
    }

    /// `^\prime` or `^{\prime\prime}`: returns the prime count and consumes it.
    fn script_prime(&mut self) -> Option<u8> {
        match (self.peek(), self.peek_n(1), self.peek_n(2)) {
            (Some(Tok::Prime(n)), _, _) => {
                let n = *n;
                self.bump();
                Some(n)
            }
            (Some(Tok::LBrace), Some(Tok::Prime(n)), Some(Tok::RBrace)) => {
                let n = *n;
                self.bump();
                self.bump();
                self.bump();
                Some(n)
            }
            _ => None,
        }
    }

    /// The argument of `^` or `_`: a braced group, a signed script, a bracket, or one atom.
    pub(super) fn parse_script(&mut self, sup: bool, what: &str) -> Node {
        if !self.enter() {
            return self.too_deep();
        }
        let node = self.script_inner(sup, what);
        self.leave();
        node
    }

    fn script_inner(&mut self, sup: bool, what: &str) -> Node {
        match self.peek() {
            Some(Tok::LBrace) => self.brace_group(what),
            Some(Tok::Op(Op::Minus | Op::Plus)) => {
                let Some(tok) = self.bump() else {
                    return self.missing(what, &[]);
                };
                let op = if matches!(tok.kind, Tok::Op(Op::Minus)) {
                    UnaryOp::Neg
                } else {
                    UnaryOp::Plus
                };
                let operand = self.parse_script(sup, what);
                unary(op, tok.span, operand)
            }
            Some(Tok::Open {
                delim: Delim::Paren,
                sized: false,
            }) => {
                let group = self.parse_atom();
                ungroup_paren(group)
            }
            Some(t) if starts_operand(t) => {
                let atom = self.parse_atom();
                if sup && self.eat(|t| matches!(t, Tok::Caret)).is_some() {
                    let exp = self.parse_script(true, "the exponent after `^`");
                    pow(atom, exp)
                } else {
                    atom
                }
            }
            _ => self.missing(what, &["`{`", "a number", "a name"]),
        }
    }

    /// A subscript: like a script, but `{max}` (two or more letters) is a label name.
    pub(super) fn parse_sub_script(&mut self) -> Node {
        if let Some(label) = self.word_label() {
            return label;
        }
        let mut sub = self.parse_script(false, "the subscript after `_`");
        // In a subscript, `i` and `e` are indices, not the constants.
        rebind(&mut sub, 'i');
        rebind(&mut sub, 'e');
        sub
    }

    /// `{max}` or plain `max` after `_`: an upright label rather than a product.
    fn word_label(&mut self) -> Option<Node> {
        let is_label = |s: &str| s.len() >= 2 && s.chars().all(|c| c.is_ascii_alphabetic());
        match self.peek()? {
            Tok::LBrace => {
                let open = self.pos;
                let close = self.matching_close(open);
                let close_tok = self.toks.get(close).filter(|_| close < self.stop)?;
                if !matches!(close_tok.kind, Tok::RBrace) {
                    return None;
                }
                let open_span = self.toks.get(open)?.span;
                let inner = Span::new(open_span.end, close_tok.span.start);
                let text = self.text(inner).trim();
                if !is_label(text) {
                    return None;
                }
                let span = open_span.join(close_tok.span);
                let name = text.to_string();
                self.pos = close + 1;
                Some(Node::new(NodeKind::Name(Name::simple(name)), span))
            }
            Tok::Sym(s) if s.kind == SymbolKind::Function => {
                let span = self.cur_span();
                let text = self.text(span);
                if !is_label(text) {
                    return None;
                }
                let name = text.to_string();
                self.bump();
                Some(Node::new(NodeKind::Name(Name::simple(name)), span))
            }
            _ => None,
        }
    }

    /// True when the next token can begin another factor of an implicit product.
    pub(super) fn can_start_factor(&self) -> bool {
        let Some(t) = self.peek() else {
            return false;
        };
        if self.ctx.in_integral && self.differential_at(0) {
            return false;
        }
        match t {
            Tok::Pipe => !self.ctx.bars,
            Tok::DoublePipe => !self.ctx.norms,
            Tok::Sym(s) if self.ctx.func_arg && s.kind == SymbolKind::Function => false,
            Tok::OperatorName(_) if self.ctx.func_arg => false,
            // `v \operatorname{in} \mathrm{km/h}`: the conversion ends the product.
            Tok::OperatorName(n) if n == "in" => false,
            Tok::Text(t) if t.trim() == "in" => false,
            // `P \text{ for all } t \ge 0`: the quantifier ends the statement.
            Tok::Text(t) if is_for_all(t) => false,
            t => starts_operand(t),
        }
    }

    /// True when token `n` ahead is `d` (or `\mathrm{d}`) followed by a variable.
    pub(super) fn differential_at(&self, n: usize) -> bool {
        is_d(self.peek_n(n), false) && self.peek_n(n + 1).is_some_and(starts_variable)
    }

    /// Fresh context with the integral flag set (keeping the bar flags).
    pub(super) fn integral_ctx(&self) -> Ctx {
        Ctx {
            in_integral: true,
            func_arg: false,
            ..self.ctx
        }
    }
}

/// The `d` of a differential or derivative: `d`, `\mathrm{d}`, or `∂` when `partial`.
pub(super) fn is_d(t: Option<&Tok>, partial: bool) -> bool {
    match t {
        Some(Tok::Letter('d')) => !partial,
        Some(Tok::Styled {
            font: Font::Roman,
            text,
        }) => !partial && text == "d",
        Some(Tok::Sym(s)) => partial && s.name == "partial",
        _ => false,
    }
}

/// Tokens that can start a variable name (in a differential or a derivative).
pub(super) fn starts_variable(t: &Tok) -> bool {
    match t {
        Tok::Letter(_) => true,
        Tok::Styled { font, text } => !(*font == Font::Roman && text == "d"),
        Tok::Sym(s) => s.kind == SymbolKind::Letter,
        _ => false,
    }
}

/// Tokens the atom rule accepts.
pub(super) fn starts_operand(t: &Tok) -> bool {
    matches!(
        t,
        Tok::Number(_)
            | Tok::Letter(_)
            | Tok::Sym(_)
            | Tok::Word(_)
            | Tok::Styled { .. }
            | Tok::OperatorName(_)
            | Tok::Text(_)
            | Tok::Command(_)
            | Tok::Begin(_)
            | Tok::Open { .. }
            | Tok::Pipe
            | Tok::DoublePipe
            | Tok::LBrace
            | Tok::Radical(_)
    )
}

fn sign_op(t: &Tok) -> Option<UnaryOp> {
    match t {
        Tok::Op(Op::Minus) => Some(UnaryOp::Neg),
        Tok::Op(Op::Plus) => Some(UnaryOp::Plus),
        Tok::Op(Op::PlusMinus) => Some(UnaryOp::PlusMinus),
        Tok::Op(Op::MinusPlus) => Some(UnaryOp::MinusPlus),
        _ => None,
    }
}

/// True when the next token is a sign (`-`, `+`, `±`, `∓`).
pub(super) fn is_sign(t: &Tok) -> bool {
    sign_op(t).is_some()
}

fn mul_op(op: Op) -> Option<MulOp> {
    Some(match op {
        Op::Star => MulOp::Bin(BinOp::Mul(MulStyle::Star)),
        Op::Cdot => MulOp::Bin(BinOp::Mul(MulStyle::Cdot)),
        Op::Times => MulOp::Bin(BinOp::Times),
        Op::Compose => MulOp::Bin(BinOp::Compose),
        Op::Tensor => MulOp::Bin(BinOp::Tensor),
        Op::DirectSum => MulOp::Bin(BinOp::DirectSum),
        Op::Cap => MulOp::Bin(BinOp::Intersect),
        Op::Slash => MulOp::Frac(FracStyle::Slash),
        Op::Obelus => MulOp::Frac(FracStyle::Obelus),
        Op::Mod => MulOp::Bin(BinOp::Mod),
        _ => return None,
    })
}

/// `10^{3}` or `10^{-6}`: a power of ten that a unit may follow.
fn is_power_of_ten(node: &Node) -> bool {
    let NodeKind::Pow { base, exp } = &node.kind else {
        return false;
    };
    let exp = match &exp.ungrouped().kind {
        NodeKind::Unary { operand, .. } => operand.ungrouped(),
        _ => exp.ungrouped(),
    };
    matches!(&base.kind, NodeKind::Number(t) if t == "10")
        && matches!(&exp.kind, NodeKind::Number(_))
}

/// Names (and primed names) can be called: `f(x)`, `f'(x)`, `v_0(t)`.
/// The bold one `\mathbf 1` (`𝟏`), which before `[…]` is the indicator function.
pub(super) fn is_indicator(node: &Node) -> bool {
    matches!(&node.kind, NodeKind::Name(n)
        if n.sub.is_none() && n.accent.is_none()
            && (n.base == "𝟏" || (n.base == "1" && n.font == Some(crate::symbols::Font::Bold))))
}

fn callable(node: &Node) -> bool {
    match &node.kind {
        NodeKind::Name(_) => true,
        NodeKind::Prime { base, .. } => matches!(base.kind, NodeKind::Name(_)),
        // `\dot x(0)`, `\ddot\theta(t)`: the time derivative of a function, at a point.
        NodeKind::Derivative(d) => {
            matches!(d.notation, DerivNotation::Dot(_))
                && d.of
                    .as_ref()
                    .is_some_and(|of| matches!(of.kind, NodeKind::Name(_)))
        }
        _ => false,
    }
}

pub(super) fn binary(op: BinOp, lhs: Node, rhs: Node) -> Node {
    let span = lhs.span.join(rhs.span);
    Node::new(
        NodeKind::Binary {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        },
        span,
    )
}

pub(super) fn unary(op: UnaryOp, op_span: Span, operand: Node) -> Node {
    let span = op_span.join(operand.span);
    Node::new(
        NodeKind::Unary {
            op,
            operand: Box::new(operand),
        },
        span,
    )
}

pub(super) fn frac(num: Node, den: Node, style: FracStyle) -> Node {
    let span = num.span.join(den.span);
    Node::new(
        NodeKind::Frac {
            num: Box::new(num),
            den: Box::new(den),
            style,
        },
        span,
    )
}

pub(super) fn pow(base: Node, exp: Node) -> Node {
    let span = base.span.join(exp.span);
    Node::new(
        NodeKind::Pow {
            base: Box::new(base),
            exp: Box::new(exp),
        },
        span,
    )
}

fn prime(node: Node, count: u8, end: usize) -> Node {
    let span = Span::new(node.span.start, end.max(node.span.end));
    match node.kind {
        NodeKind::Prime { base, count: c } => Node::new(
            NodeKind::Prime {
                base,
                count: c.saturating_add(count),
            },
            span,
        ),
        kind => Node::new(
            NodeKind::Prime {
                base: Box::new(Node::new(kind, node.span)),
                count,
            },
            span,
        ),
    }
}

fn factorial(node: Node, end: usize) -> Node {
    let span = Span::new(node.span.start, end.max(node.span.end));
    match node.kind {
        NodeKind::Factorial { operand, count: 1 } => {
            Node::new(NodeKind::Factorial { operand, count: 2 }, span)
        }
        kind => Node::new(
            NodeKind::Factorial {
                operand: Box::new(Node::new(kind, node.span)),
                count: 1,
            },
            span,
        ),
    }
}

/// Puts a subscript on a name, or wraps anything else in a `Subscript` node.
pub(super) fn attach_sub(node: Node, sub: Node) -> Node {
    let span = node.span.join(sub.span);
    if let Some(mut name) = node.as_name() {
        if name.sub.is_none() && !matches!(node.kind, NodeKind::Group { .. }) {
            name.sub = Some(Box::new(sub));
            return Node::new(NodeKind::Name(name), span);
        }
    }
    Node::new(
        NodeKind::Subscript {
            base: Box::new(node),
            sub: Box::new(sub),
        },
        span,
    )
}

/// Applies an accent; `\dot` and `\ddot` are time derivatives.
pub(super) fn make_accent(accent: Accent, arg: Node, span: Span) -> Node {
    let dots = match accent {
        Accent::Dot => Some(1),
        Accent::DDot => Some(2),
        _ => None,
    };
    if let Some(count) = dots {
        return Node::new(
            NodeKind::Derivative(Box::new(Derivative {
                partial: false,
                order: None,
                vars: Vec::new(),
                of: Some(arg),
                notation: DerivNotation::Dot(count),
            })),
            span,
        );
    }
    // `\hat\pi`: an estimate of π is a name of its own, never the constant.
    if let NodeKind::Constant(Constant::Pi) = &arg.ungrouped().kind {
        let mut name = Name::simple("π");
        name.accent = Some(accent);
        return Node::new(NodeKind::Name(name), span);
    }
    if let NodeKind::Name(name) = &arg.ungrouped().kind {
        if name.accent.is_none() {
            let mut name = name.clone();
            name.accent = Some(accent);
            return Node::new(NodeKind::Name(name), span);
        }
    }
    Node::new(
        NodeKind::Accented {
            accent,
            inner: Box::new(arg),
        },
        span,
    )
}

/// Removes plain round brackets used only for grouping (`x^(n+1)`, `sqrt(x)`).
pub(super) fn ungroup_paren(node: Node) -> Node {
    match node.kind {
        NodeKind::Group {
            inner,
            bracket: Bracket::Paren,
            sized: false,
        } => *inner,
        kind => Node::new(kind, node.span),
    }
}

/// A round pair `(a, b)` read where a set is expected becomes the open interval.
pub(crate) fn as_interval(node: Node) -> Node {
    match node.kind {
        NodeKind::Tuple {
            mut elems,
            bracket: Bracket::Paren,
            sized,
        } if elems.len() == 2 => {
            let hi = elems.pop();
            let lo = elems.pop();
            match (lo, hi) {
                (Some(lo), Some(hi)) => Node::new(
                    NodeKind::Interval {
                        lo: Box::new(lo),
                        hi: Box::new(hi),
                        lo_closed: false,
                        hi_closed: false,
                        sized,
                    },
                    node.span,
                ),
                _ => Node::error(node.span),
            }
        }
        kind => Node::new(kind, node.span),
    }
}

/// Turns the constant `e` or `i` into the name `letter` throughout `node`, for
/// scopes that bind it (`\sum_{i=1}`, `f(i) := …`, subscripts).
pub(super) fn rebind(node: &mut Node, letter: char) {
    if let NodeKind::Constant(c) = node.kind {
        if c.letter() == Some(letter) {
            node.kind = NodeKind::Name(Name::simple(letter.to_string()));
        }
        return;
    }
    crate::math::visit::children_mut(node, &mut |child| rebind(child, letter));
}

/// The single letter `e` or `i` when `node` names it.
pub(super) fn bound_letter(node: &Node) -> Option<char> {
    let name = node.as_name()?;
    let mut chars = name.base.chars();
    match (chars.next(), chars.next(), name.sub.is_none()) {
        (Some(c @ ('e' | 'i')), None, true) => Some(c),
        _ => None,
    }
}

/// The node turned into a plain `Name` node (so `e` and `i` become names).
pub(super) fn name_node(node: &Node) -> Node {
    match node.as_name() {
        Some(name) => Node::new(NodeKind::Name(name), node.span),
        None => node.clone(),
    }
}

/// `\text{ for all }` (also "for every", "for each"): a quantifier over the condition
/// that follows.
pub(super) fn is_for_all(text: &str) -> bool {
    let words: Vec<&str> = text.split_whitespace().collect();
    matches!(words.as_slice(), ["for", "all" | "every" | "each"])
}
