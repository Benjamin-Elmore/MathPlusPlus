//! S-expression debug printer for snapshot tests.
//!
//! [`Mode::Full`] shows presentation detail (`:frac` against `/`, brackets,
//! implicit products); [`Mode::Semantic`] leaves it out, so the LaTeX, plain
//! and Unicode spellings of the same maths print the same.

use crate::math::ast::{
    BigOp, BigOpKind, BinOp, Bracket, CallStyle, CaseCondition, CaseRow, Clause, Constant, DefHead,
    DefKind, DerivNotation, Derivative, Dots, Fence, FenceStyle, FracStyle, FuncApp, IntKind,
    Integral, Limit, Line, MathAst, MatrixEnv, MulStyle, NablaOp, Name, NamedOp, Node, NodeKind,
    QueryMode, Range, RelOp, SetStyle, Side, SqrtStyle, Statement, UnaryOp,
};
use crate::symbols::Font;

/// How much detail to print.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Full,
    Semantic,
}

/// Prints a whole maths span, one line per statement line.
pub fn ast(ast: &MathAst, mode: Mode) -> String {
    let p = Printer { mode };
    let mut lines: Vec<String> = ast.lines.iter().map(|l| p.line(l)).collect();
    if let Some(label) = &ast.label {
        lines.push(format!("(label {})", label.name));
    }
    if mode == Mode::Full && ast.aligned {
        lines.push(":aligned".to_string());
    }
    lines.join("\n")
}

/// Prints one expression.
pub fn node(node: &Node, mode: Mode) -> String {
    Printer { mode }.node(node)
}

/// Prints one clause.
pub fn clause(clause: &Clause, mode: Mode) -> String {
    Printer { mode }.clause(clause)
}

struct Printer {
    mode: Mode,
}

/// Builds `(head :keys child …)`.
struct List {
    parts: Vec<String>,
}

impl List {
    fn new(head: impl Into<String>) -> Self {
        List {
            parts: vec![head.into()],
        }
    }

    fn push(&mut self, part: impl Into<String>) -> &mut Self {
        self.parts.push(part.into());
        self
    }

    fn finish(&self) -> String {
        format!("({})", self.parts.join(" "))
    }
}

impl Printer {
    fn full(&self) -> bool {
        self.mode == Mode::Full
    }

    /// Adds a presentation keyword in full mode only.
    fn key(&self, list: &mut List, key: &str) {
        if self.full() {
            list.push(key);
        }
    }

    fn line(&self, line: &Line) -> String {
        let stmt = self.statement(&line.statement);
        if line.clauses.is_empty() && !line.noeval {
            return stmt;
        }
        let mut list = List::new("line");
        list.push(stmt);
        for c in &line.clauses {
            list.push(self.clause(c));
        }
        if line.noeval {
            list.push(":noeval");
        }
        list.finish()
    }

    fn statement(&self, s: &Statement) -> String {
        match s {
            Statement::Definition { head, kind, body } => {
                let op = match kind {
                    DefKind::Assign => ":=",
                    DefKind::Const => "≡",
                };
                let mut list = List::new("def");
                list.push(op).push(self.head(head)).push(self.node(body));
                list.finish()
            }
            Statement::Query { expr, mode, steps } => {
                let op = match (mode, steps) {
                    (QueryMode::Exact, false) => "=?",
                    (QueryMode::Exact, true) => "=?!",
                    (QueryMode::Approx, false) => "≈?",
                    (QueryMode::Approx, true) => "≈?!",
                };
                let mut list = List::new("query");
                list.push(op).push(self.node(expr));
                list.finish()
            }
            Statement::Check { lhs, rhs } => {
                let mut list = List::new("check");
                list.push(self.node(lhs)).push(self.node(rhs));
                list.finish()
            }
            Statement::Relation(n) => self.node(n),
            Statement::NameList(names) => self.seq("names", names),
            Statement::Range(r) => self.range(r),
            Statement::Empty => "(empty)".to_string(),
        }
    }

    fn head(&self, head: &DefHead) -> String {
        match head {
            DefHead::Name(n) => self.node(n),
            DefHead::Function { name, params } => {
                let mut list = List::new("fn-head");
                list.push(self.node(name));
                for p in params {
                    list.push(self.node(p));
                }
                list.finish()
            }
            DefHead::Recurrence { name, index, shift } => {
                let mut list = List::new("recur");
                list.push(self.node(name))
                    .push(self.node(index))
                    .push(self.node(shift));
                list.finish()
            }
            DefHead::Initial { name, index } => {
                let mut list = List::new("initial");
                list.push(self.node(name)).push(self.node(index));
                list.finish()
            }
            DefHead::Invalid(n) => format!("(invalid {})", self.node(n)),
        }
    }

    fn clause(&self, c: &Clause) -> String {
        match c {
            Clause::Where { name, value, .. } => {
                format!("(where {} {})", self.node(name), self.node(value))
            }
            Clause::Range(r) => self.range(r),
            Clause::Condition(n) => format!("(if {})", self.node(n)),
        }
    }

    fn range(&self, r: &Range) -> String {
        let names: Vec<String> = r.names.iter().map(|n| self.node(n)).collect();
        format!("(range ({}) {})", names.join(" "), self.node(&r.set))
    }

    fn seq(&self, head: &str, items: &[Node]) -> String {
        let mut list = List::new(head);
        for i in items {
            list.push(self.node(i));
        }
        list.finish()
    }

    fn opt(&self, list: &mut List, key: &str, node: Option<&Node>) {
        if let Some(n) = node {
            list.push(key).push(self.node(n));
        }
    }

    /// Each kind prints in its own small function so deep trees use little stack.
    fn node(&self, n: &Node) -> String {
        use NodeKind as K;
        match &n.kind {
            K::Number(t) => t.clone(),
            K::Name(name) => self.name(name),
            K::Constant(c) => constant(*c).to_string(),
            K::Symbol(s) => format!("(sym {s})"),
            K::Dots(d) => self.dots(*d),
            K::Text(t) => format!("(text {t:?})"),
            K::Continuation => "(cont)".to_string(),
            K::Error => "(error)".to_string(),
            K::Placeholder { label } => format!("(slot {label:?})"),
            K::Call {
                callee,
                args,
                sized,
            } => self.call(callee, args, *sized),
            K::FuncApp(app) => self.func_app(app),
            K::Unary { op, operand } => self.unary(*op, operand),
            K::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs),
            K::Frac { num, den, style } => self.frac(num, den, *style),
            K::Pow { base, exp } => self.pair("^", base, exp),
            K::Subscript { base, sub } => self.pair("_", base, sub),
            K::Prime { base, count } => self.prefixed(&format!("prime {count}"), base),
            K::Factorial { operand, count } => {
                self.prefixed(if *count == 2 { "!!" } else { "!" }, operand)
            }
            K::Fenced {
                fence,
                inner,
                style,
            } => self.fenced(*fence, inner, *style),
            K::Sqrt {
                index,
                radicand,
                style,
            } => self.sqrt(index.as_deref(), radicand, *style),
            K::Binom { n, k, style } => self.binom(n, k, *style),
            K::Matrix { env, rows } => self.matrix(*env, rows),
            K::Cases { rows } => self.cases(rows),
            K::Set { elems, style } => self.set(elems, *style),
            K::Interval {
                lo,
                hi,
                lo_closed,
                hi_closed,
                sized,
            } => self.interval(lo, hi, (*lo_closed, *hi_closed), *sized),
            K::Tuple {
                elems,
                bracket,
                sized,
            } => self.tuple(elems, *bracket, *sized),
            K::Group {
                inner,
                bracket,
                sized,
            } => self.group(inner, *bracket, *sized),
            K::BigOp(op) => self.big_op(op),
            K::Integral(int) => self.integral(int),
            K::Limit(lim) => self.limit(lim),
            K::Derivative(d) => self.derivative(d),
            K::Nabla { op, body } => self.nabla(*op, body.as_deref()),
            K::NamedOp(op) => self.named_op(op),
            K::Accented { accent, inner } => self.prefixed(accent.name(), inner),
            K::Styled { font, inner } => self.prefixed(font_name(*font), inner),
            K::Relation { first, rest } => self.relation(first, rest),
        }
    }

    #[inline(never)]
    fn name(&self, name: &Name) -> String {
        let mut s = name.base.clone();
        if let Some(font) = name.font {
            s = format!("{}:{s}", font_name(font));
        }
        if let Some(acc) = name.accent {
            s = format!("({} {s})", acc.name());
        }
        match &name.sub {
            Some(sub) => format!("(_ {s} {})", self.node(sub)),
            None => s,
        }
    }

    #[inline(never)]
    fn dots(&self, d: Dots) -> String {
        if !self.full() {
            return "…".to_string();
        }
        let which = match d {
            Dots::Low => "low",
            Dots::Centre => "centre",
            Dots::Vertical => "vertical",
            Dots::Diagonal => "diagonal",
        };
        format!("(dots {which})")
    }

    #[inline(never)]
    fn pair(&self, head: &str, a: &Node, b: &Node) -> String {
        format!("({head} {} {})", self.node(a), self.node(b))
    }

    #[inline(never)]
    fn prefixed(&self, head: &str, inner: &Node) -> String {
        format!("({head} {})", self.node(inner))
    }

    #[inline(never)]
    fn call(&self, callee: &Node, args: &[Node], sized: bool) -> String {
        let mut list = List::new("call");
        if sized {
            self.key(&mut list, ":sized");
        }
        list.push(self.node(callee));
        for a in args {
            list.push(self.node(a));
        }
        list.finish()
    }

    #[inline(never)]
    fn func_app(&self, app: &FuncApp) -> String {
        let head = if app.inverse {
            format!("{}^-1", app.func)
        } else {
            app.func.to_string()
        };
        let mut list = List::new(head);
        if app.bracketed {
            self.key(&mut list, ":paren");
        }
        self.opt(&mut list, ":sub", app.sub.as_ref());
        for a in &app.args {
            list.push(self.node(a));
        }
        let applied = list.finish();
        match &app.power {
            Some(p) => format!("(^ {applied} {})", self.node(p)),
            None => applied,
        }
    }

    #[inline(never)]
    fn unary(&self, op: UnaryOp, operand: &Node) -> String {
        let head = match op {
            UnaryOp::Neg => "neg",
            UnaryOp::Plus => "pos",
            UnaryOp::PlusMinus => "±",
            UnaryOp::MinusPlus => "∓",
            UnaryOp::Not => "not",
        };
        self.prefixed(head, operand)
    }

    #[inline(never)]
    fn binary(&self, op: BinOp, lhs: &Node, rhs: &Node) -> String {
        let head = match op {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::PlusMinus => "±",
            BinOp::MinusPlus => "∓",
            BinOp::Mul(_) => "*",
            BinOp::Times => "×",
            BinOp::Compose => "∘",
            BinOp::Tensor => "⊗",
            BinOp::DirectSum => "⊕",
            BinOp::Union => "∪",
            BinOp::Intersect => "∩",
            BinOp::SetMinus => "∖",
            BinOp::And => "and",
            BinOp::Or => "or",
        };
        let key = match op {
            BinOp::Mul(MulStyle::Implicit) => Some(":implicit"),
            BinOp::Mul(MulStyle::Star) => Some(":star"),
            BinOp::Mul(MulStyle::Cdot) => Some(":cdot"),
            _ => None,
        };
        match key.filter(|_| self.full()) {
            Some(key) => self.pair(&format!("{head} {key}"), lhs, rhs),
            None => self.pair(head, lhs, rhs),
        }
    }

    #[inline(never)]
    fn frac(&self, num: &Node, den: &Node, style: FracStyle) -> String {
        if !self.full() {
            return self.pair("/", num, den);
        }
        let key = match style {
            FracStyle::Frac => ":frac",
            FracStyle::TFrac => ":tfrac",
            FracStyle::DFrac => ":dfrac",
            FracStyle::Slash => ":slash",
            FracStyle::Obelus => ":obelus",
        };
        self.pair(&format!("/ {key}"), num, den)
    }

    #[inline(never)]
    fn fenced(&self, fence: Fence, inner: &Node, style: FenceStyle) -> String {
        let head = match fence {
            Fence::Abs => "abs",
            Fence::Norm => "norm",
            Fence::Floor => "floor",
            Fence::Ceil => "ceil",
        };
        let key = match style {
            FenceStyle::Delimiters => None,
            FenceStyle::Sized => Some(":sized"),
            FenceStyle::Command => Some(":cmd"),
            FenceStyle::Word => Some(":word"),
        };
        match key.filter(|_| self.full()) {
            Some(k) => self.prefixed(&format!("{head} {k}"), inner),
            None => self.prefixed(head, inner),
        }
    }

    #[inline(never)]
    fn sqrt(&self, index: Option<&Node>, radicand: &Node, style: SqrtStyle) -> String {
        let mut list = List::new(if index.is_some() { "root" } else { "sqrt" });
        let key = match style {
            SqrtStyle::Command => None,
            SqrtStyle::Radical => Some(":radical"),
            SqrtStyle::Word => Some(":word"),
        };
        if let Some(k) = key {
            self.key(&mut list, k);
        }
        if let Some(i) = index {
            list.push(self.node(i));
        }
        list.push(self.node(radicand));
        list.finish()
    }

    #[inline(never)]
    fn binom(&self, n: &Node, k: &Node, style: CallStyle) -> String {
        if style == CallStyle::Word && self.full() {
            return self.pair("binom :word", n, k);
        }
        self.pair("binom", n, k)
    }

    #[inline(never)]
    fn matrix(&self, env: MatrixEnv, rows: &[Vec<Node>]) -> String {
        let mut list = List::new("matrix");
        list.push(format!(":{env:?}").to_lowercase());
        for row in rows {
            list.push(self.seq("row", row));
        }
        list.finish()
    }

    #[inline(never)]
    fn cases(&self, rows: &[CaseRow]) -> String {
        let mut list = List::new("cases");
        for row in rows {
            let cond = match &row.condition {
                CaseCondition::When(c) => self.node(c),
                CaseCondition::Otherwise => "otherwise".to_string(),
                CaseCondition::Missing => "_".to_string(),
            };
            list.push(format!("(case {} {cond})", self.node(&row.value)));
        }
        list.finish()
    }

    #[inline(never)]
    fn set(&self, elems: &[Node], style: SetStyle) -> String {
        let mut list = List::new("set");
        let key = match style {
            SetStyle::Escaped => ":escaped",
            SetStyle::Braces => ":braces",
            SetStyle::EmptySymbol => ":empty",
        };
        self.key(&mut list, key);
        for e in elems {
            list.push(self.node(e));
        }
        list.finish()
    }

    #[inline(never)]
    fn interval(&self, lo: &Node, hi: &Node, closed: (bool, bool), sized: bool) -> String {
        let ends = format!(
            "{}{}",
            if closed.0 { "[" } else { "(" },
            if closed.1 { "]" } else { ")" }
        );
        let mut list = List::new("interval");
        list.push(ends);
        if sized {
            self.key(&mut list, ":sized");
        }
        list.push(self.node(lo)).push(self.node(hi));
        list.finish()
    }

    #[inline(never)]
    fn tuple(&self, elems: &[Node], bracket: Bracket, sized: bool) -> String {
        let mut list = List::new("tuple");
        list.push(bracket_name(bracket));
        if sized {
            self.key(&mut list, ":sized");
        }
        for e in elems {
            list.push(self.node(e));
        }
        list.finish()
    }

    #[inline(never)]
    fn group(&self, inner: &Node, bracket: Bracket, sized: bool) -> String {
        if !self.full() {
            return self.node(inner);
        }
        let head = if sized {
            format!("group {} :sized", bracket_name(bracket))
        } else {
            format!("group {}", bracket_name(bracket))
        };
        self.prefixed(&head, inner)
    }

    #[inline(never)]
    fn big_op(&self, op: &BigOp) -> String {
        let head = match op.op {
            BigOpKind::Sum => "sum",
            BigOpKind::Prod => "prod",
            BigOpKind::Coprod => "coprod",
        };
        let mut list = List::new(head);
        self.opt(&mut list, ":index", op.index.as_ref());
        self.opt(&mut list, ":from", op.from.as_ref());
        self.opt(&mut list, ":to", op.to.as_ref());
        self.opt(&mut list, ":over", op.over.as_ref());
        list.push(self.node(&op.body));
        list.finish()
    }

    #[inline(never)]
    fn integral(&self, int: &Integral) -> String {
        let head = match int.kind {
            IntKind::Single => "int",
            IntKind::Double => "iint",
            IntKind::Triple => "iiint",
            IntKind::Contour => "oint",
            IntKind::Surface => "oiint",
        };
        let mut list = List::new(head);
        if int.diff_in_numerator {
            self.key(&mut list, ":d-in-numerator");
        }
        self.opt(&mut list, ":lower", int.lower.as_ref());
        self.opt(&mut list, ":upper", int.upper.as_ref());
        list.push(self.node(&int.body));
        let d: Vec<String> = int.diffs.iter().map(|d| self.node(d)).collect();
        let head = if int.dot { "·d" } else { "d" };
        list.push(format!("({head} {})", d.join(" ")));
        list.finish()
    }

    #[inline(never)]
    fn limit(&self, lim: &Limit) -> String {
        let mut list = List::new("lim");
        list.push(self.node(&lim.var)).push(self.node(&lim.target));
        match lim.side {
            Some(Side::Left) => {
                list.push(":left");
            }
            Some(Side::Right) => {
                list.push(":right");
            }
            None => {}
        }
        list.push(self.node(&lim.body));
        list.finish()
    }

    #[inline(never)]
    fn derivative(&self, d: &Derivative) -> String {
        let mut list = List::new(if d.partial { "pderiv" } else { "deriv" });
        match d.notation {
            DerivNotation::Dot(n) => {
                list.push(format!(":dot {n}"));
            }
            DerivNotation::Bare => {
                list.push(":bare");
            }
            DerivNotation::Operator { frac } => {
                self.key(&mut list, if frac { ":op-frac" } else { ":op-slash" });
            }
            DerivNotation::Whole { frac } => {
                self.key(&mut list, if frac { ":whole-frac" } else { ":whole-slash" });
            }
            DerivNotation::PartialSub => self.key(&mut list, ":sub"),
            DerivNotation::DSub => self.key(&mut list, ":dsub"),
        }
        self.opt(&mut list, ":order", d.order.as_ref());
        if !d.vars.is_empty() {
            let vars: Vec<String> = d
                .vars
                .iter()
                .map(|v| match &v.power {
                    Some(p) => self.pair("^", &v.var, p),
                    None => self.node(&v.var),
                })
                .collect();
            list.push(format!("(by {})", vars.join(" ")));
        }
        match &d.of {
            Some(of) => list.push(self.node(of)),
            None => list.push("_"),
        };
        list.finish()
    }

    #[inline(never)]
    fn nabla(&self, op: NablaOp, body: Option<&Node>) -> String {
        let head = match op {
            NablaOp::Grad => "grad",
            NablaOp::Div => "div",
            NablaOp::Curl => "curl",
            NablaOp::Laplacian => "laplacian",
        };
        match body {
            Some(b) => self.prefixed(head, b),
            None => format!("({head} _)"),
        }
    }

    #[inline(never)]
    fn named_op(&self, op: &NamedOp) -> String {
        let mut list = List::new("op");
        list.push(op.name.clone());
        if op.bracketed {
            self.key(&mut list, ":paren");
        }
        self.opt(&mut list, ":sub", op.sub.as_ref());
        self.opt(&mut list, ":sup", op.sup.as_ref());
        for a in &op.args {
            list.push(self.node(a));
        }
        list.finish()
    }

    #[inline(never)]
    fn relation(&self, first: &Node, rest: &[(RelOp, Node)]) -> String {
        let mut list = List::new("rel");
        list.push(self.node(first));
        for (op, n) in rest {
            list.push(op.symbol()).push(self.node(n));
        }
        list.finish()
    }
}

fn constant(c: Constant) -> &'static str {
    match c {
        Constant::Pi => "#pi",
        Constant::E => "#e",
        Constant::I => "#i",
        Constant::Infinity => "#inf",
    }
}

fn font_name(font: Font) -> &'static str {
    match font {
        Font::Blackboard => "bb",
        Font::Calligraphic => "cal",
        Font::Bold => "bf",
        Font::Fraktur => "frak",
        Font::Roman => "rm",
    }
}

fn bracket_name(bracket: Bracket) -> &'static str {
    match bracket {
        Bracket::Paren => "paren",
        Bracket::Square => "square",
        Bracket::Brace => "brace",
        Bracket::Angle => "angle",
        Bracket::TexGroup => "texgroup",
        Bracket::None => "none",
    }
}
