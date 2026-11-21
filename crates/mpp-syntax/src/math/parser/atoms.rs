//! Atoms: numbers, names, brackets, fences, fractions, roots, environments,
//! accents, known functions and named operators.

use super::expr::{is_sign, make_accent, name_node, starts_operand, unary, ungroup_paren};
use super::{closes, is_close, is_terminator, Ctx, Parser};
use crate::lexer::{Delim, Op, Tok};
use crate::math::ast::{
    Accent, BinOp, Bracket, CallStyle, CaseCondition, CaseRow, Constant, Dots, Fence, FenceStyle,
    FracStyle, FuncApp, MatrixEnv, MulStyle, Name, NamedOp, Node, NodeKind, RelOp, SetStyle,
    SqrtStyle, UnaryOp,
};
use crate::span::Span;
use crate::symbols::{lookup_name, Font, Symbol, SymbolKind};

const EXPR: &[&str] = &["a number", "a name", "`(`"];

impl Parser<'_> {
    /// One atom (grammar `atom`), including big operators and derivatives.
    pub(super) fn parse_atom(&mut self) -> Node {
        if !self.enter() {
            return self.too_deep();
        }
        let node = self.atom_inner();
        self.leave();
        node
    }

    fn atom_inner(&mut self) -> Node {
        if let Some(node) = self.try_calculus() {
            return node;
        }
        let Some(tok) = self.peek().cloned() else {
            return self.missing("an expression", EXPR);
        };
        let span = self.cur_span();
        match tok {
            Tok::Number(text) => {
                self.bump();
                let (text, end) = self.digit_groups(text, span);
                if let Some(node) = self.digit_expansion(&text, span.join(end)) {
                    return node;
                }
                Node::new(NodeKind::Number(text), span.join(end))
            }
            Tok::Letter(c) => {
                self.bump();
                letter_node(c, span)
            }
            Tok::Sym(sym) => {
                self.bump();
                self.symbol_atom(sym, span)
            }
            Tok::Word(word) => {
                self.bump();
                self.word_atom(word, span)
            }
            Tok::Styled { font, text } => {
                if font == Font::Calligraphic && matches!(text.as_str(), "L" | "F") {
                    if let Some(node) = self.transform(&text, span) {
                        return node;
                    }
                }
                self.bump();
                let name = Name {
                    font: Some(font),
                    ..Name::simple(text)
                };
                Node::new(NodeKind::Name(name), span)
            }
            Tok::OperatorName(name) => {
                self.bump();
                match lookup_name(&name).filter(|s| s.kind == SymbolKind::Function) {
                    Some(sym) => self.func_app(sym, span),
                    None => self.named_op(name, span),
                }
            }
            Tok::Text(text) => {
                self.bump();
                Node::new(NodeKind::Text(text), span)
            }
            Tok::Command(name) => {
                self.bump();
                self.command_atom(&name, span)
            }
            Tok::Begin(env) => {
                self.bump();
                self.environment(&env, span)
            }
            Tok::Open { delim, sized } => {
                self.bump();
                self.bracketed(delim, sized, span)
            }
            Tok::LBrace => self.tex_group(),
            Tok::Pipe => {
                self.bump();
                self.bar_fence(Fence::Abs, span)
            }
            Tok::DoublePipe => {
                self.bump();
                self.bar_fence(Fence::Norm, span)
            }
            Tok::Radical(n) => {
                self.bump();
                self.radical(n, span)
            }
            Tok::Unknown(_) => {
                // The lexer has already reported it.
                self.bump();
                Node::error(span)
            }
            t if is_terminator(&t) || matches!(t, Tok::Op(_)) => {
                self.missing("an expression", EXPR)
            }
            _ => {
                let node = self.missing("an expression", EXPR);
                self.bump();
                node
            }
        }
    }

    /// `\mathcal{L}\{f\}`, `\mathcal{L}^{-1}\{F\}`, `\mathcal{L}_{t \to s}\left\{ f \right\}` and
    /// the same with `\mathcal{F}`: the Laplace and Fourier transforms, as the operators
    /// `laplace`, `invlaplace`, `fourier` and `invfourier` (the subscript names the
    /// variables). `None`, with nothing consumed, when no `\{` follows: then the letter is
    /// an ordinary name.
    fn transform(&mut self, letter: &str, start: Span) -> Option<Node> {
        // Look ahead without consuming: scripts are `_` or `^` and one group or token.
        let mut k = 1;
        let mut scripts = 0;
        while scripts < 2 && matches!(self.peek_n(k), Some(Tok::Underscore | Tok::Caret)) {
            k += 1;
            match self.peek_n(k) {
                Some(Tok::LBrace) => {
                    let mut depth = 0usize;
                    loop {
                        match self.peek_n(k) {
                            Some(Tok::LBrace) => depth += 1,
                            Some(Tok::RBrace) => {
                                depth = depth.saturating_sub(1);
                                if depth == 0 {
                                    break;
                                }
                            }
                            None => return None,
                            _ => {}
                        }
                        k += 1;
                    }
                    k += 1;
                }
                Some(_) => k += 1,
                None => return None,
            }
            scripts += 1;
        }
        if !matches!(
            self.peek_n(k),
            Some(Tok::Open {
                delim: Delim::Brace,
                ..
            })
        ) {
            return None;
        }
        self.bump();
        let mut sub = None;
        let mut inverse = false;
        loop {
            match self.peek() {
                Some(Tok::Underscore) => {
                    self.bump();
                    sub = Some(self.tex_arg("the variables of the transform"));
                }
                Some(Tok::Caret) => {
                    self.bump();
                    let sup = self.tex_arg("`-1` for the inverse transform");
                    let text: String = self
                        .text(sup.span)
                        .chars()
                        .filter(|c| !c.is_whitespace())
                        .collect();
                    if matches!(text.as_str(), "-1" | "{-1}") {
                        inverse = true;
                    } else {
                        self.error(
                            sup.span,
                            "a transform's superscript can only be `-1` (the inverse)",
                            &["`-1`"],
                        );
                    }
                }
                _ => break,
            }
        }
        let open = self.cur_span();
        let (delim, sized) = match self.peek() {
            Some(Tok::Open { delim, sized }) => (*delim, *sized),
            _ => return None,
        };
        self.bump();
        let inner = self.bracketed(delim, sized, open);
        let (inner_kind, inner_span) = inner.into_parts();
        let arg = match inner_kind {
            NodeKind::Set { mut elems, .. } if elems.len() == 1 => elems.remove(0),
            kind => Node::new(kind, inner_span),
        };
        let name = match (letter, inverse) {
            ("L", false) => "laplace",
            ("L", true) => "invlaplace",
            (_, false) => "fourier",
            (_, true) => "invfourier",
        };
        let span = start.join(self.prev_span());
        Some(Node::new(
            NodeKind::NamedOp(Box::new(NamedOp {
                name: name.to_string(),
                sub,
                sup: None,
                args: vec![arg],
                bracketed: true,
            })),
            span,
        ))
    }

    fn symbol_atom(&mut self, sym: &'static Symbol, span: Span) -> Node {
        let kind = match sym.kind {
            SymbolKind::Letter if sym.name == "pi" => NodeKind::Constant(Constant::Pi),
            // `\Delta x`, `\Delta t`, `ΔV`: a change in a quantity is one name.
            SymbolKind::Letter if sym.name == "Delta" => {
                if let Some(Tok::Letter(c)) = self.peek().cloned() {
                    self.bump();
                    let span = span.join(self.prev_span());
                    return Node::new(NodeKind::Name(Name::simple(format!("Δ{c}"))), span);
                }
                NodeKind::Name(Name::simple(sym.unicode))
            }
            SymbolKind::Letter => NodeKind::Name(Name::simple(sym.unicode)),
            SymbolKind::Function if sym.name == "sqrt" => {
                let command = self.text(span).starts_with('\\');
                return self.sqrt(span, command);
            }
            SymbolKind::Function => return self.func_app(sym, span),
            SymbolKind::Accent => {
                let accent = Accent::from_name(sym.name).unwrap_or(Accent::Hat);
                return self.accent(accent, span);
            }
            SymbolKind::Calculus => match sym.name {
                "infty" => NodeKind::Constant(Constant::Infinity),
                "ell" | "hbar" | "aleph" | "Re" | "Im" => NodeKind::Name(Name::simple(sym.unicode)),
                _ => NodeKind::Symbol(sym.unicode),
            },
            SymbolKind::Set if matches!(sym.name, "emptyset" | "varnothing") => NodeKind::Set {
                elems: Vec::new(),
                style: SetStyle::EmptySymbol,
            },
            SymbolKind::Misc => match sym.name {
                "ldots" | "dots" => NodeKind::Dots(Dots::Low),
                "cdots" => NodeKind::Dots(Dots::Centre),
                "vdots" => NodeKind::Dots(Dots::Vertical),
                "ddots" => NodeKind::Dots(Dots::Diagonal),
                _ => NodeKind::Symbol(sym.unicode),
            },
            _ => NodeKind::Symbol(sym.unicode),
        };
        Node::new(kind, span)
    }

    /// `0.d_1 d_2 d_3 \dots`: a decimal expansion with symbolic digits, written right
    /// after a whole number and its point. It is read as one text node (prose
    /// describing a number, with nothing to compute), so it is not a syntax error.
    fn digit_expansion(&mut self, whole: &str, number: Span) -> Option<Node> {
        if !whole.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let point = self.toks.get(self.pos).filter(|_| self.pos < self.stop)?;
        if !matches!(point.kind, Tok::Period) || point.span.start != number.end {
            return None;
        }
        let digit = self
            .toks
            .get(self.pos + 1)
            .filter(|_| self.pos + 1 < self.stop)?;
        let letter = match &digit.kind {
            Tok::Letter(_) => true,
            Tok::Sym(s) => s.kind == SymbolKind::Letter,
            _ => false,
        };
        if !letter || digit.span.start != point.span.end {
            return None;
        }
        self.bump();
        let mut end = number.end;
        // The digits: names (with subscripts) and dots, side by side.
        loop {
            let next = self.peek();
            let more = match next {
                Some(Tok::Letter(_)) => true,
                Some(Tok::Sym(s)) => {
                    s.kind == SymbolKind::Letter
                        || (s.kind == SymbolKind::Misc
                            && matches!(s.name, "dots" | "ldots" | "cdots"))
                }
                _ => false,
            };
            if !more {
                break;
            }
            let part = self.parse_postfix();
            end = part.span.end;
        }
        let span = Span::new(number.start, end);
        let text = self.text(span).to_string();
        Some(Node::new(NodeKind::Text(text), span))
    }

    /// `10\,000` and `1\,234\,567.5`: groups of three digits joined by a thin space
    /// are one number, as in printed tables. Returns the digits and the last span.
    fn digit_groups(&mut self, mut text: String, mut end: Span) -> (String, Span) {
        if text.contains('.') {
            return (text, end);
        }
        while let Some(Tok::Number(next)) = self.peek().cloned() {
            let at = self.cur_span();
            let between = self
                .src
                .get(end.end.saturating_sub(self.base)..at.start.saturating_sub(self.base))
                .unwrap_or("");
            let thin = matches!(between, "\\," | "\\ " | "{,}" | "\u{2009}" | "\u{202F}");
            let group = next.split('.').next().unwrap_or("");
            if !thin || group.len() != 3 || !group.chars().all(|c| c.is_ascii_digit()) {
                break;
            }
            self.bump();
            text.push_str(&next);
            end = at;
            if next.contains('.') {
                break;
            }
        }
        (text, end)
    }

    /// A known function after its name: scripts, then bracketed or bare arguments.
    pub(super) fn func_app(&mut self, sym: &'static Symbol, start: Span) -> Node {
        let mut sub = None;
        let mut power = None;
        loop {
            let span = self.cur_span();
            match self.peek().cloned() {
                Some(Tok::Underscore) => {
                    self.bump();
                    sub = Some(self.parse_sub_script());
                }
                Some(Tok::Caret) => {
                    self.bump();
                    let what = format!("the power after `{}`", self.text(start));
                    power = Some(self.parse_script(true, &what));
                }
                Some(Tok::Superscript(text)) => {
                    self.bump();
                    power = Some(self.fragment(&text, span));
                }
                Some(Tok::Subscript(text)) => {
                    self.bump();
                    sub = Some(self.fragment(&text, span));
                }
                _ => break,
            }
        }
        let inverse = power.as_ref().is_some_and(is_minus_one);
        if inverse {
            power = None;
        }
        // `\max_{t \in [0, 50]} (x - 1)^2 + …`: over a range, max and min take the whole
        // term after them, as a sum does.
        let over_range = matches!(sym.name, "max" | "min")
            && sub.as_ref().is_some_and(|s| {
                matches!(&s.ungrouped().kind, NodeKind::Relation { rest, .. }
                    if rest.len() == 1 && rest[0].0 == RelOp::In)
            });
        let (args, bracketed, end) = if over_range && !inverse {
            let body = self.op_body(&format!("the term after `{}`", self.text(start)));
            let end = body.span;
            (vec![body], false, end)
        } else if self.at(|t| {
            matches!(
                t,
                Tok::Open {
                    delim: Delim::Paren,
                    ..
                }
            )
        }) {
            let (args, _, end) = self.call_args();
            (args, true, end)
        } else if self.at(|t| starts_operand(t) || is_sign(t)) {
            let arg = self.func_arg();
            let end = arg.span;
            (vec![arg], false, end)
        } else if sub.is_none() && power.is_none() && !inverse && self.at_name_end() {
            // `$\sin$` on its own (prose about the function): the function's name.
            let name = Name {
                font: Some(Font::Roman),
                ..Name::simple(sym.name)
            };
            return Node::new(NodeKind::Name(name), start);
        } else {
            let what = format!("the argument of `{}`", self.text(start));
            let missing = self.missing(&what, &["an expression", "`(`"]);
            let end = missing.span;
            (vec![missing], false, end)
        };
        let span = start.join(end);
        Node::new(
            NodeKind::FuncApp(Box::new(FuncApp {
                func: sym.name,
                sub,
                power,
                inverse,
                args,
                bracketed,
            })),
            span,
        )
    }

    /// True at the end of the line or before a `,`: nothing an argument could start with.
    fn at_name_end(&self) -> bool {
        self.peek()
            .is_none_or(|t| matches!(t, Tok::NewLine | Tok::Comma))
    }

    /// A bracket-less function argument: the product to the right, up to the next
    /// function name, explicit operator, `+` or `−`.
    fn func_arg(&mut self) -> Node {
        let ctx = Ctx {
            func_arg: true,
            ..self.ctx
        };
        self.with_ctx(ctx, |p| {
            if let Some(tok) = p.eat(is_sign) {
                let op = match tok.kind {
                    Tok::Op(Op::Minus) => UnaryOp::Neg,
                    Tok::Op(Op::PlusMinus) => UnaryOp::PlusMinus,
                    Tok::Op(Op::MinusPlus) => UnaryOp::MinusPlus,
                    _ => UnaryOp::Plus,
                };
                let operand = p.func_product();
                return unary(op, tok.span, operand);
            }
            p.func_product()
        })
    }

    fn func_product(&mut self) -> Node {
        let mut node = self.parse_factor();
        let mut count = 0;
        while self.can_start_factor() && self.chain_ok(count) {
            count += 1;
            let rhs = self.parse_factor();
            node = super::expr::binary(BinOp::Mul(MulStyle::Implicit), node, rhs);
        }
        node
    }

    /// `( a, b, … )` after a name: returns the arguments, whether the brackets
    /// were sized, and the span of the closing bracket.
    pub(super) fn call_args(&mut self) -> (Vec<Node>, bool, Span) {
        let open = self.cur_span();
        let sized = matches!(
            self.bump().map(|t| t.kind),
            Some(Tok::Open { sized: true, .. })
        );
        let args = self.nested(|p| p.comma_list(is_close));
        let end = if self.at(is_close) {
            self.cur_span()
        } else {
            let span = self.cur_span();
            self.error(
                span,
                "expected `)` to close the argument list",
                &["`)`", "`,`"],
            );
            return (args, sized, open.join(Span::point(span.start)));
        };
        self.bump();
        (args, sized, end)
    }

    /// Relation chains separated by `,` or `;`, stopping before `end`.
    fn comma_list(&mut self, end: fn(&Tok) -> bool) -> Vec<Node> {
        let mut items = Vec::new();
        if self.at(end) {
            return items;
        }
        loop {
            items.push(self.parse_relation());
            if self
                .eat(|t| matches!(t, Tok::Comma | Tok::Semicolon))
                .is_none()
            {
                break;
            }
        }
        items
    }

    fn named_op(&mut self, name: String, start: Span) -> Node {
        let mut sub = None;
        let mut sup = None;
        loop {
            let span = self.cur_span();
            match self.peek().cloned() {
                Some(Tok::Underscore) => {
                    self.bump();
                    sub = Some(self.parse_script(false, "the subscript after `_`"));
                }
                Some(Tok::Caret) => {
                    self.bump();
                    sup = Some(self.parse_script(true, "the superscript after `^`"));
                }
                Some(Tok::Superscript(text)) => {
                    self.bump();
                    sup = Some(self.fragment(&text, span));
                }
                Some(Tok::Subscript(text)) => {
                    self.bump();
                    sub = Some(self.fragment(&text, span));
                }
                _ => break,
            }
        }
        let paren = self.at(|t| {
            matches!(
                t,
                Tok::Open {
                    delim: Delim::Paren,
                    ..
                }
            )
        });
        let (args, bracketed, end) = if paren {
            let (args, _, end) = self.call_args();
            (args, true, end)
        } else if self.can_start_factor() || self.at(is_sign) {
            let arg = self.parse_signed_term();
            let end = arg.span;
            (vec![arg], false, end)
        } else {
            (Vec::new(), false, self.prev_span())
        };
        let span = start.join(end);
        if sub.is_none() && sup.is_none() && args.is_empty() {
            let name = Name {
                font: Some(Font::Roman),
                ..Name::simple(name)
            };
            return Node::new(NodeKind::Name(name), span);
        }
        Node::new(
            NodeKind::NamedOp(Box::new(NamedOp {
                name,
                sub,
                sup,
                args,
                bracketed,
            })),
            span,
        )
    }

    fn command_atom(&mut self, name: &str, span: Span) -> Node {
        let style = match name {
            "frac" | "cfrac" => Some(FracStyle::Frac),
            "tfrac" => Some(FracStyle::TFrac),
            "dfrac" => Some(FracStyle::DFrac),
            _ => None,
        };
        if let Some(style) = style {
            return self.frac(style, span);
        }
        match name {
            "binom" | "dbinom" | "tbinom" => {
                let n = self.tex_arg("the top of `\\binom`");
                let k = self.tex_arg("the bottom of `\\binom`");
                let span = span.join(k.span);
                Node::new(
                    NodeKind::Binom {
                        n: Box::new(n),
                        k: Box::new(k),
                        style: CallStyle::Command,
                    },
                    span,
                )
            }
            "abs" | "norm" => {
                let fence = if name == "abs" {
                    Fence::Abs
                } else {
                    Fence::Norm
                };
                let inner = self.tex_arg(&format!("the argument of `\\{name}`"));
                fenced(
                    fence,
                    inner,
                    FenceStyle::Command,
                    span.join(self.prev_span()),
                )
            }
            "pmod" => {
                let arg = self.tex_arg("the modulus after `\\pmod`");
                let span = span.join(arg.span);
                Node::new(
                    NodeKind::NamedOp(Box::new(NamedOp {
                        name: "mod".to_string(),
                        sub: None,
                        sup: None,
                        args: vec![arg],
                        bracketed: true,
                    })),
                    span,
                )
            }
            "underline" | "boxed" | "cancel" | "underbrace" | "overbrace" | "mathring" => {
                self.tex_arg(&format!("the argument of `\\{name}`"))
            }
            "overset" | "underset" | "stackrel" => {
                let _decoration = self.tex_arg(&format!("the first argument of `\\{name}`"));
                self.tex_arg(&format!("the second argument of `\\{name}`"))
            }
            "mathbb" | "mathcal" | "mathscr" | "mathbf" | "boldsymbol" | "bm" | "mathfrak"
            | "mathrm" | "mathit" | "mathsf" => {
                let font = match name {
                    "mathbb" => Font::Blackboard,
                    "mathcal" | "mathscr" => Font::Calligraphic,
                    "mathbf" | "boldsymbol" | "bm" => Font::Bold,
                    "mathfrak" => Font::Fraktur,
                    _ => Font::Roman,
                };
                let inner = self.tex_arg(&format!("the argument of `\\{name}`"));
                apply_font(font, inner, span.join(self.prev_span()))
            }
            _ => {
                self.error(
                    span,
                    format!("unknown command `\\{name}`"),
                    &["a known LaTeX maths command"],
                );
                Node::error(span)
            }
        }
    }

    /// `\frac{a}{b}` (or a derivative written as a fraction).
    fn frac(&mut self, style: FracStyle, start: Span) -> Node {
        if let Some(d) = self.try_derivative_frac(start) {
            return d;
        }
        let cmd = self.text(start).to_string();
        let num = self.tex_arg(&format!("the numerator of `{cmd}`"));
        let den = self.tex_arg(&format!("the denominator of `{cmd}`"));
        let span = start.join(den.span).join(self.prev_span());
        Node::new(
            NodeKind::Frac {
                num: Box::new(num),
                den: Box::new(den),
                style,
            },
            span,
        )
    }

    /// A TeX argument: `{…}`, one digit of a digit run (`\frac12`), or one atom.
    pub(super) fn tex_arg(&mut self, what: &str) -> Node {
        match self.peek() {
            Some(Tok::LBrace) => self.brace_group(what),
            Some(Tok::Number(text))
                if text.len() > 1 && text.chars().all(|c| c.is_ascii_digit()) =>
            {
                self.split_digit()
            }
            Some(t) if starts_operand(t) => self.parse_atom(),
            _ => self.missing(what, &["`{`"]),
        }
    }

    /// Takes the first digit of the next number token, leaving the rest.
    fn split_digit(&mut self) -> Node {
        let Some(tok) = self.toks.get_mut(self.pos) else {
            return Node::error(Span::point(self.prev_end()));
        };
        let Tok::Number(text) = &mut tok.kind else {
            return Node::error(tok.span);
        };
        let first: String = text.chars().take(1).collect();
        let rest: String = text.chars().skip(1).collect();
        let span = tok.span;
        let head = Span::new(span.start, span.start + 1);
        *text = rest;
        tok.span = Span::new(span.start + 1, span.end);
        Node::new(NodeKind::Number(first), head)
    }

    /// `{ … }` used as an argument: returns what is inside (a tuple if it has commas).
    pub(super) fn brace_group(&mut self, what: &str) -> Node {
        let open = self.cur_span();
        self.bump();
        if self.at(|t| matches!(t, Tok::RBrace)) {
            let node = self.missing(what, EXPR);
            self.bump();
            return node;
        }
        let mut items = self.nested(|p| p.comma_list(|t| matches!(t, Tok::RBrace)));
        self.expect_rbrace(open);
        let span = open.join(self.prev_span());
        if items.len() == 1 {
            if let Some(item) = items.pop() {
                return item;
            }
        }
        Node::new(
            NodeKind::Tuple {
                elems: items,
                bracket: Bracket::TexGroup,
                sized: false,
            },
            span,
        )
    }

    /// `{ … }` as an atom: a group, or a set in the plain form (`{1, 2, 3}`).
    fn tex_group(&mut self) -> Node {
        let open = self.cur_span();
        self.bump();
        if self.at(|t| matches!(t, Tok::RBrace)) {
            let node = self.missing("an expression inside `{}`", EXPR);
            self.bump();
            return node;
        }
        let mut items = self.nested(|p| p.comma_list(|t| matches!(t, Tok::RBrace)));
        self.expect_rbrace(open);
        let span = open.join(self.prev_span());
        if items.len() == 1 {
            if let Some(inner) = items.pop() {
                return Node::new(
                    NodeKind::Group {
                        inner: Box::new(inner),
                        bracket: Bracket::TexGroup,
                        sized: false,
                    },
                    span,
                );
            }
        }
        Node::new(
            NodeKind::Set {
                elems: items,
                style: SetStyle::Braces,
            },
            span,
        )
    }

    /// Expects `}`; otherwise reports it and skips to the matching `}` on this line.
    fn expect_rbrace(&mut self, open: Span) {
        if self.eat(|t| matches!(t, Tok::RBrace)).is_some() {
            return;
        }
        let span = self.cur_span();
        let open_text = self.text(open).to_string();
        self.error(
            span,
            format!("expected `}}` to close the `{open_text}`"),
            &["`}`"],
        );
        let mut depth = 0usize;
        while let Some(t) = self.peek() {
            match t {
                Tok::LBrace => depth += 1,
                Tok::RBrace if depth == 0 => {
                    self.bump();
                    return;
                }
                Tok::RBrace => depth -= 1,
                Tok::NewLine | Tok::End(_) if depth == 0 => return,
                _ => {}
            }
            self.bump();
        }
    }

    /// After an opening bracket: a group, tuple, interval, set or fence.
    fn bracketed(&mut self, delim: Delim, sized: bool, open: Span) -> Node {
        let style = if sized {
            FenceStyle::Sized
        } else {
            FenceStyle::Delimiters
        };
        let fence = match delim {
            Delim::Floor => Some(Fence::Floor),
            Delim::Ceil => Some(Fence::Ceil),
            Delim::Bar => Some(Fence::Abs),
            Delim::DoubleBar => Some(Fence::Norm),
            _ => None,
        };
        if let Some(fence) = fence {
            let inner = self.nested(|p| p.parse_relation());
            let closed = match delim {
                Delim::Bar => self.eat(|t| matches!(t, Tok::Pipe) || closes(t, Delim::Bar)),
                Delim::DoubleBar => {
                    self.eat(|t| matches!(t, Tok::DoublePipe) || closes(t, Delim::DoubleBar))
                }
                _ => self.eat(|t| closes(t, delim)),
            };
            if closed.is_none() {
                self.unclosed(open, fence_closer(fence));
            }
            return fenced(fence, inner, style, open.join(self.prev_span()));
        }
        let elems = self.nested(|p| p.comma_list(is_close));
        let close = match self.peek() {
            Some(Tok::Close { delim: d, .. }) => {
                let d = *d;
                self.bump();
                d
            }
            _ => {
                self.unclosed(open, delim_closer(delim));
                delim
            }
        };
        let span = open.join(self.prev_span());
        self.bracket_node(delim, close, sized, elems, span)
    }

    fn bracket_node(
        &mut self,
        open: Delim,
        close: Delim,
        sized: bool,
        mut elems: Vec<Node>,
        span: Span,
    ) -> Node {
        let bracket = match open {
            Delim::None if close == Delim::Bar => Bracket::EvalBar,
            Delim::Square => Bracket::Square,
            Delim::Brace => Bracket::Brace,
            Delim::Angle => Bracket::Angle,
            Delim::None => Bracket::None,
            _ => Bracket::Paren,
        };
        if open == Delim::Brace {
            if let [only] = elems.as_slice() {
                if let Some(builder) = set_builder(only, span) {
                    return builder;
                }
            }
            return Node::new(
                NodeKind::Set {
                    elems,
                    style: SetStyle::Escaped,
                },
                span,
            );
        }
        let round_or_square = |d: Delim| matches!(d, Delim::Paren | Delim::Square);
        if elems.len() == 2 && round_or_square(open) && round_or_square(close) {
            if open == Delim::Square || close == Delim::Square {
                let hi = elems.pop();
                let lo = elems.pop();
                if let (Some(lo), Some(hi)) = (lo, hi) {
                    return Node::new(
                        NodeKind::Interval {
                            lo: Box::new(lo),
                            hi: Box::new(hi),
                            lo_closed: open == Delim::Square,
                            hi_closed: close == Delim::Square,
                            sized,
                        },
                        span,
                    );
                }
                return Node::error(span);
            }
        } else if open != close && open != Delim::None && close != Delim::None {
            let at = Span::point(span.end);
            self.error(
                at,
                format!(
                    "this bracket was opened with `{}` but closed with `{}`",
                    delim_opener(open),
                    delim_closer(close)
                ),
                &[delim_closer(open)],
            );
        }
        if elems.len() == 1 {
            if let Some(inner) = elems.pop() {
                return Node::new(
                    NodeKind::Group {
                        inner: Box::new(inner),
                        bracket,
                        sized,
                    },
                    span,
                );
            }
        }
        if elems.is_empty() {
            self.error(span, "empty brackets", &["an expression"]);
        }
        Node::new(
            NodeKind::Tuple {
                elems,
                bracket,
                sized,
            },
            span,
        )
    }

    fn unclosed(&mut self, open: Span, closer: &str) {
        let span = self.cur_span();
        let open_text = self.text(open).to_string();
        let expected = format!("`{closer}`");
        self.error(
            Span::point(span.start),
            format!("expected `{closer}` to close the `{open_text}`"),
            &[expected.as_str()],
        );
    }

    /// `| … |` or `‖ … ‖` written with plain bars.
    fn bar_fence(&mut self, fence: Fence, open: Span) -> Node {
        let ctx = Ctx {
            bars: fence == Fence::Abs,
            norms: fence == Fence::Norm,
            ..Ctx::default()
        };
        let inner = self.with_ctx(ctx, |p| p.parse_relation());
        let closed = match fence {
            Fence::Abs => self.eat(|t| matches!(t, Tok::Pipe) || closes(t, Delim::Bar)),
            _ => self.eat(|t| matches!(t, Tok::DoublePipe) || closes(t, Delim::DoubleBar)),
        };
        if closed.is_none() {
            self.unclosed(open, fence_closer(fence));
        }
        fenced(
            fence,
            inner,
            FenceStyle::Delimiters,
            open.join(self.prev_span()),
        )
    }

    /// `\sqrt[n]{x}`, `\sqrt x`, `sqrt(x)`.
    fn sqrt(&mut self, start: Span, command: bool) -> Node {
        let index = if self.at(|t| {
            matches!(
                t,
                Tok::Open {
                    delim: Delim::Square,
                    sized: false
                }
            )
        }) {
            let open = self.cur_span();
            self.bump();
            let idx = self.nested(|p| p.parse_relation());
            if self.eat(|t| closes(t, Delim::Square)).is_none() {
                self.unclosed(open, "]");
            }
            Some(Box::new(idx))
        } else {
            None
        };
        let radicand = match self.peek() {
            Some(Tok::LBrace) => self.brace_group("the expression under the root"),
            Some(t) if starts_operand(t) => {
                let atom = self.parse_atom();
                ungroup_paren(atom)
            }
            _ => self.missing("the expression under the root", &["`{`", "`(`"]),
        };
        let span = start.join(radicand.span).join(self.prev_span());
        let style = if command {
            SqrtStyle::Command
        } else {
            SqrtStyle::Word
        };
        Node::new(
            NodeKind::Sqrt {
                index,
                radicand: Box::new(radicand),
                style,
            },
            span,
        )
    }

    /// `√x`, `∛x`, `∜x`.
    fn radical(&mut self, n: u8, start: Span) -> Node {
        let radicand = if self.at(starts_operand) {
            let atom = self.parse_atom();
            ungroup_paren(atom)
        } else {
            self.missing("the expression under the root", EXPR)
        };
        let index = (n > 2).then(|| Box::new(Node::new(NodeKind::Number(n.to_string()), start)));
        let span = start.join(radicand.span);
        Node::new(
            NodeKind::Sqrt {
                index,
                radicand: Box::new(radicand),
                style: SqrtStyle::Radical,
            },
            span,
        )
    }

    /// Plain-form keywords: `root(n, x)`, `binom(n, k)`, `floor(x)`, `abs(x)` …
    fn word_atom(&mut self, word: &'static str, start: Span) -> Node {
        if word == "otherwise" {
            return Node::new(NodeKind::Text(word.to_string()), start);
        }
        let paren = self.at(|t| {
            matches!(
                t,
                Tok::Open {
                    delim: Delim::Paren,
                    ..
                }
            )
        });
        if !paren {
            return self.missing(&format!("`(` after `{word}`"), &["`(`"]);
        }
        let (mut args, _, end) = self.call_args();
        let span = start.join(end);
        let want = if matches!(word, "root" | "binom") {
            2
        } else {
            1
        };
        if args.len() != want {
            let shape = match word {
                "root" => "`root(n, x)`".to_string(),
                "binom" => "`binom(n, k)`".to_string(),
                w => format!("`{w}(x)`"),
            };
            self.error(
                span,
                format!("`{word}` takes {want} argument(s), as in {shape}"),
                &[shape.as_str()],
            );
            return Node::error(span);
        }
        let second = if want == 2 { args.pop() } else { None };
        let Some(first) = args.pop() else {
            return Node::error(span);
        };
        let kind = match (word, second) {
            ("root", Some(x)) => NodeKind::Sqrt {
                index: Some(Box::new(first)),
                radicand: Box::new(x),
                style: SqrtStyle::Word,
            },
            ("binom", Some(k)) => NodeKind::Binom {
                n: Box::new(first),
                k: Box::new(k),
                style: CallStyle::Word,
            },
            _ => {
                let fence = match word {
                    "floor" => Fence::Floor,
                    "ceil" => Fence::Ceil,
                    "norm" => Fence::Norm,
                    _ => Fence::Abs,
                };
                return fenced(fence, first, FenceStyle::Word, span);
            }
        };
        Node::new(kind, span)
    }

    /// `\hat{x}`, `\vec v`, plain `hat(x)`.
    fn accent(&mut self, accent: Accent, start: Span) -> Node {
        let arg = match self.peek() {
            Some(Tok::LBrace) => self.brace_group("the letter under the accent"),
            Some(t) if starts_operand(t) => {
                let atom = self.parse_atom();
                ungroup_paren(atom)
            }
            _ => self.missing("the letter under the accent", &["`{`", "a name"]),
        };
        let span = start.join(arg.span).join(self.prev_span());
        make_accent(accent, arg, span)
    }

    /// After `\begin{env}`: a matrix, cases, or an unknown environment.
    fn environment(&mut self, env: &str, start: Span) -> Node {
        if let Some(kind) = MatrixEnv::from_env(env) {
            return self.matrix(kind, env, start);
        }
        if env == "array" {
            if self.at(|t| matches!(t, Tok::LBrace)) {
                let close = self.matching_close(self.pos);
                self.pos = (close + 1).min(self.stop);
            }
            return self.matrix(MatrixEnv::Matrix, env, start);
        }
        if matches!(env, "cases" | "dcases" | "rcases") {
            return self.cases(env, start);
        }
        self.error(
            start,
            format!("unknown environment `{env}`"),
            &["`pmatrix`", "`bmatrix`", "`vmatrix`", "`cases`"],
        );
        while let Some(t) = self.bump() {
            if matches!(&t.kind, Tok::End(e) if e == env) {
                break;
            }
        }
        Node::error(start.join(self.prev_span()))
    }

    fn matrix(&mut self, kind: MatrixEnv, env: &str, start: Span) -> Node {
        let rows = self.nested(|p| {
            let mut rows = Vec::new();
            let mut row = Vec::new();
            loop {
                if p.end_env(env) {
                    break;
                }
                let before = p.pos;
                row.push(p.parse_relation());
                match p.peek() {
                    Some(Tok::Amp) => {
                        p.bump();
                    }
                    Some(Tok::NewLine) => {
                        p.bump();
                        rows.push(std::mem::take(&mut row));
                    }
                    Some(Tok::End(_)) | None => {}
                    Some(_) => {
                        let span = p.cur_span();
                        let found = p.text(span).to_string();
                        p.error(
                            span,
                            format!("unexpected `{found}` in a matrix"),
                            &["`&`", "`\\\\`", "`\\end`"],
                        );
                        p.bump();
                    }
                }
                if p.pos == before {
                    p.bump();
                }
            }
            if !row.is_empty() {
                rows.push(row);
            }
            rows
        });
        Node::new(
            NodeKind::Matrix { env: kind, rows },
            start.join(self.prev_span()),
        )
    }

    /// At `\end{env}` (consumed) or the end of input (reported): true.
    fn end_env(&mut self, env: &str) -> bool {
        match self.peek() {
            None => {
                let span = self.cur_span();
                self.error(span, format!("expected `\\end{{{env}}}`"), &["`\\end`"]);
                true
            }
            Some(Tok::End(found)) => {
                let found = found.clone();
                let span = self.cur_span();
                self.bump();
                if found != env {
                    self.error(
                        span,
                        format!("expected `\\end{{{env}}}` but found `\\end{{{found}}}`"),
                        &["`\\end`"],
                    );
                }
                true
            }
            _ => false,
        }
    }

    fn cases(&mut self, env: &str, start: Span) -> Node {
        let rows = self.nested(|p| {
            let mut rows = Vec::new();
            loop {
                if p.end_env(env) {
                    break;
                }
                let before = p.pos;
                let value = p.parse_relation();
                p.eat(|t| matches!(t, Tok::Comma));
                let condition = if p.eat(|t| matches!(t, Tok::Amp)).is_some() {
                    p.case_condition()
                } else {
                    CaseCondition::Missing
                };
                rows.push(CaseRow { value, condition });
                match p.peek() {
                    Some(Tok::NewLine) => {
                        p.bump();
                    }
                    Some(Tok::End(_)) | None => {}
                    Some(_) => {
                        let span = p.cur_span();
                        let found = p.text(span).to_string();
                        p.error(
                            span,
                            format!("unexpected `{found}` in cases"),
                            &["`\\\\`", "`\\end{cases}`"],
                        );
                        p.bump();
                    }
                }
                if p.pos == before {
                    p.bump();
                }
            }
            rows
        });
        Node::new(NodeKind::Cases { rows }, start.join(self.prev_span()))
    }

    fn case_condition(&mut self) -> CaseCondition {
        match self.peek() {
            Some(Tok::Text(t)) => {
                let lower = t.trim().trim_end_matches([',', '.']).to_ascii_lowercase();
                if matches!(lower.as_str(), "otherwise" | "else") {
                    self.bump();
                    return CaseCondition::Otherwise;
                }
                let lead = lower.split_whitespace().next().unwrap_or("");
                if matches!(lead, "if" | "for" | "when" | "where") && lower == lead {
                    self.bump();
                }
            }
            Some(Tok::Word("otherwise")) => {
                self.bump();
                return CaseCondition::Otherwise;
            }
            Some(Tok::Letter('i')) if matches!(self.peek_n(1), Some(Tok::Letter('f'))) => {
                self.bump();
                self.bump();
            }
            _ => {}
        }
        if self.at(|t| matches!(t, Tok::NewLine | Tok::End(_))) || self.at_end() {
            return CaseCondition::Missing;
        }
        CaseCondition::When(self.parse_relation())
    }
}

/// A single letter: `e` and `i` are the constants, anything else a name.
/// `x \in S : cond` or `x : cond` (also with `\mid`) inside `\{ \}`: the relation chain
/// is split at the colon into the variable, its domain and the condition.
fn set_builder(node: &Node, span: Span) -> Option<Node> {
    let NodeKind::Relation { first, rest } = &node.kind else {
        return None;
    };
    let colon = rest
        .iter()
        .position(|(op, _)| matches!(op, RelOp::Other(":" | "∣")))?;
    let (var, domain) = match colon {
        0 => ((**first).clone(), None),
        1 if rest[0].0 == RelOp::In => ((**first).clone(), Some(Box::new(rest[0].1.clone()))),
        _ => return None,
    };
    if !var.is_name() {
        return None;
    }
    let cond_first = rest[colon].1.clone();
    let cond_rest: Vec<(RelOp, Node)> = rest[colon + 1..].to_vec();
    let condition = if cond_rest.is_empty() {
        cond_first
    } else {
        let end = cond_rest.last().map_or(cond_first.span, |(_, n)| n.span);
        let cspan = cond_first.span.join(end);
        Node::new(
            NodeKind::Relation {
                first: Box::new(cond_first),
                rest: cond_rest,
            },
            cspan,
        )
    };
    Some(Node::new(
        NodeKind::SetBuilder {
            var: Box::new(var),
            domain,
            condition: Box::new(condition),
        },
        span,
    ))
}

fn letter_node(c: char, span: Span) -> Node {
    let kind = match c {
        'e' => NodeKind::Constant(Constant::E),
        'i' => NodeKind::Constant(Constant::I),
        _ => NodeKind::Name(Name::simple(c.to_string())),
    };
    Node::new(kind, span)
}

fn fenced(fence: Fence, inner: Node, style: FenceStyle, span: Span) -> Node {
    Node::new(
        NodeKind::Fenced {
            fence,
            inner: Box::new(inner),
            style,
        },
        span,
    )
}

/// `\mathbf{F}` on a name sets its font; on anything else wraps it.
fn apply_font(font: Font, inner: Node, span: Span) -> Node {
    if let NodeKind::Name(name) = &inner.ungrouped().kind {
        if name.font.is_none() {
            let mut name = name.clone();
            name.font = Some(font);
            return Node::new(NodeKind::Name(name), span);
        }
    }
    let inner = name_node(&inner);
    Node::new(
        NodeKind::Styled {
            font,
            inner: Box::new(inner),
        },
        span,
    )
}

/// True for `-1` (so `\sin^{-1}` means the inverse).
fn is_minus_one(node: &Node) -> bool {
    match &node.ungrouped().kind {
        NodeKind::Unary {
            op: UnaryOp::Neg,
            operand,
        } => matches!(&operand.ungrouped().kind, NodeKind::Number(t) if t == "1"),
        _ => false,
    }
}

fn fence_closer(fence: Fence) -> &'static str {
    match fence {
        Fence::Abs => "|",
        Fence::Norm => "‖",
        Fence::Floor => "⌋",
        Fence::Ceil => "⌉",
    }
}

fn delim_closer(delim: Delim) -> &'static str {
    match delim {
        Delim::Paren => ")",
        Delim::Square => "]",
        Delim::Brace => "\\}",
        Delim::Angle => "⟩",
        Delim::Floor => "⌋",
        Delim::Ceil => "⌉",
        Delim::Bar => "|",
        Delim::DoubleBar => "‖",
        Delim::None => "\\right.",
    }
}

fn delim_opener(delim: Delim) -> &'static str {
    match delim {
        Delim::Paren => "(",
        Delim::Square => "[",
        Delim::Brace => "\\{",
        Delim::Angle => "⟨",
        Delim::Floor => "⌊",
        Delim::Ceil => "⌈",
        Delim::Bar => "|",
        Delim::DoubleBar => "‖",
        Delim::None => "\\left.",
    }
}
