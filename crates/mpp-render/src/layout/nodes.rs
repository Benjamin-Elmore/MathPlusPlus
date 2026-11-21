//! Typesetting each kind of expression node.

use mpp_syntax::math::display::{operator_display, OpDisplay, Piece};
use mpp_syntax::math::visit;
use mpp_syntax::math::{
    Accent, BigOp, BigOpKind, BinOp, Bracket, CaseCondition, Constant, DerivNotation, Derivative,
    Dots, Fence, FracStyle, FuncApp, IntKind, Integral, Limit, MatrixEnv, MulStyle, NablaOp, Name,
    NamedOp, Node, NodeKind, RelOp, SetStyle, Side, UnaryOp,
};
use mpp_syntax::symbols::Font;

use super::boxes::{LBox, Placed};
use super::chars;
use super::engine::{Atom, Engine, MAX_DEPTH};
use super::style::{Class, Level, Style};
use super::IntegralLimits;
use crate::display::{HitKind, NodePath};
use crate::font::FontId;

/// The path of `child` under `parent`, by its position among the parent's children.
pub(crate) fn child_path(parent: &Node, child: &Node, path: &NodePath) -> NodePath {
    let mut found = None;
    let mut i: u16 = 0;
    visit::children(parent, &mut |c| {
        if found.is_none() && std::ptr::eq(c, child) {
            found = Some(i);
        }
        i = i.saturating_add(1);
    });
    path.child(found.unwrap_or(u16::MAX))
}

/// Functions whose subscript goes underneath in display style.
fn has_limits(func: &str) -> bool {
    matches!(
        func,
        "lim"
            | "liminf"
            | "limsup"
            | "max"
            | "min"
            | "sup"
            | "inf"
            | "det"
            | "gcd"
            | "lcm"
            | "Pr"
            | "argmax"
            | "argmin"
    )
}

/// How a function name is spelled when typeset.
fn func_word(func: &str) -> &str {
    match func {
        "liminf" => "lim inf",
        "limsup" => "lim sup",
        "argmax" => "arg max",
        "argmin" => "arg min",
        other => other,
    }
}

impl Engine<'_> {
    /// Typesets a node as one atom, recording its hit box.
    pub fn atom(&mut self, n: &Node, st: Style, path: &NodePath, hint: Option<&str>) -> Atom {
        if self.depth >= MAX_DEPTH {
            return Atom::ord(self.slot(Some("…"), st, n.span, path));
        }
        self.depth += 1;
        let mut a = self.atom_inner(n, st, path, hint);
        self.depth -= 1;
        if !n.is_slot() {
            if let Some(hit) = self.hit(n.span, path, HitKind::Node) {
                a.b = a.b.with_hit(hit);
            }
        }
        a
    }

    /// Typesets a child of `parent`.
    pub fn child(&mut self, parent: &Node, child: &Node, st: Style, path: &NodePath) -> Atom {
        let p = child_path(parent, child, path);
        self.atom(child, st, &p, None)
    }

    /// Typesets a child of `parent`, labelling it `hint` when it is an empty slot.
    pub fn child_h(
        &mut self,
        parent: &Node,
        child: &Node,
        st: Style,
        path: &NodePath,
        hint: &str,
    ) -> Atom {
        let p = child_path(parent, child, path);
        self.atom(child, st, &p, Some(hint))
    }

    fn atom_inner(&mut self, n: &Node, st: Style, path: &NodePath, hint: Option<&str>) -> Atom {
        use NodeKind as K;
        match &n.kind {
            K::Number(s) => Atom::ord(self.word(s, st)),
            K::Name(name) => Atom::ord(self.name(n, name, st, path, true)),
            K::Constant(c) => Atom::ord(self.constant(*c, st)),
            K::Symbol(s) => Atom::ord(self.sym_str(s, st)),
            K::Dots(d) => {
                let ch = match d {
                    Dots::Low => '…',
                    Dots::Centre => '⋯',
                    Dots::Vertical => '⋮',
                    Dots::Diagonal => '⋱',
                };
                Atom::new(Class::Inner, self.sym(ch, st))
            }
            K::Text(t) => Atom::ord(self.text(t, FontId::Text, st)),
            K::Continuation => Atom::ord(LBox::empty()),
            K::Error => Atom::ord(self.slot(hint, st, n.span, path)),
            K::Placeholder { label } => {
                let label = if label.is_empty() {
                    hint
                } else {
                    Some(label.as_str())
                };
                Atom::ord(self.slot(label, st, n.span, path))
            }
            K::Call { callee, args, .. } => {
                let f = self.child(n, callee, st, path);
                let inner = self.list(n, args, st, path);
                let call = self.fenced(Some('('), inner, Some(')'), st);
                self.group(vec![f, call], st)
            }
            K::FuncApp(app) => self.func_app(n, app, st, path),
            K::Unary { op, operand } => {
                let (ch, class) = match op {
                    UnaryOp::Neg => ('−', Class::Bin),
                    UnaryOp::Plus => ('+', Class::Bin),
                    UnaryOp::PlusMinus => ('±', Class::Bin),
                    UnaryOp::MinusPlus => ('∓', Class::Bin),
                    UnaryOp::Not => ('¬', Class::Ord),
                };
                let o = Atom::new(class, self.sym(ch, st));
                let x = self.child(n, operand, st, path);
                self.group(vec![o, x], st)
            }
            K::Binary { op, lhs, rhs } => {
                let l = self.child(n, lhs, st, path);
                let r = self.child(n, rhs, st, path);
                if *op == BinOp::Mod {
                    let m = Atom::new(Class::Bin, self.roman("mod", st));
                    return self.group(vec![l, m, r], st);
                }
                match bin_symbol(*op) {
                    None => self.group(vec![l, r], st),
                    Some(ch) => {
                        let o = Atom::new(Class::Bin, self.sym(ch, st));
                        self.group(vec![l, o, r], st)
                    }
                }
            }
            K::Frac { num, den, style } => self.frac(n, num, den, *style, st, path),
            K::Pow { base, exp } => self.pow(n, base, exp, st, path),
            K::Subscript { base, sub } => self.subscript(n, base, sub, st, path),
            K::Prime { base, count } => self.prime(n, base, *count, st, path),
            K::Factorial { operand, count } => {
                let x = self.child(n, operand, st, path);
                let bang = if *count >= 2 { "!!" } else { "!" };
                let b = Atom::new(Class::Close, self.sym_str(bang, st));
                self.group(vec![x, b], st)
            }
            K::Fenced { fence, inner, .. } => {
                let (o, c) = match fence {
                    Fence::Abs => ('|', '|'),
                    Fence::Norm => ('‖', '‖'),
                    Fence::Floor => ('⌊', '⌋'),
                    Fence::Ceil => ('⌈', '⌉'),
                };
                let x = self.child(n, inner, st, path);
                self.fenced(Some(o), x, Some(c), st)
            }
            K::Sqrt {
                index, radicand, ..
            } => {
                let r = self.child_h(n, radicand, st.cramp(), path, "radicand");
                let i = index.as_ref().map(|i| {
                    let ss = Style {
                        level: Level::ScriptScript,
                        cramped: true,
                    };
                    self.child_h(n, i, ss, path, "n").b
                });
                Atom::ord(self.radical(r.b, i, st))
            }
            K::Binom { n: top, k, .. } => {
                let a = self.child(n, top, st.num(), path);
                let b = self.child(n, k, st.den(), path);
                let stack = self.fraction(a.b, b.b, st, false);
                self.fenced(Some('('), Atom::ord(stack), Some(')'), st)
            }
            K::Matrix { env, rows } => self.matrix(n, *env, rows, st, path),
            K::Cases { rows } => self.cases(n, rows, st, path),
            K::Set { elems, style } => {
                if *style == SetStyle::EmptySymbol {
                    return Atom::ord(self.sym('∅', st));
                }
                let inner = self.list(n, elems, st, path);
                self.fenced(Some('{'), inner, Some('}'), st)
            }
            K::Interval {
                lo,
                hi,
                lo_closed,
                hi_closed,
                ..
            } => {
                let a = self.child(n, lo, st, path);
                let comma = Atom::new(Class::Punct, self.sym(',', st));
                let b = self.child(n, hi, st, path);
                let inner = self.group(vec![a, comma, b], st);
                let o = if *lo_closed { '[' } else { '(' };
                let c = if *hi_closed { ']' } else { ')' };
                self.fenced(Some(o), inner, Some(c), st)
            }
            K::Tuple { elems, bracket, .. } => {
                let inner = self.list(n, elems, st, path);
                let (o, c) = bracket_chars(*bracket);
                self.fenced(o, inner, c, st)
            }
            K::Group { inner, bracket, .. } => {
                let x = self.child(n, inner, st, path);
                match bracket {
                    Bracket::TexGroup => Atom::ord(x.b),
                    Bracket::None => x,
                    b => {
                        let (o, c) = bracket_chars(*b);
                        self.fenced(o, x, c, st)
                    }
                }
            }
            K::BigOp(op) => self.big_op_node(n, op, st, path),
            K::Integral(int) => self.integral(n, int, st, path),
            K::Limit(lim) => self.limit(n, lim, st, path),
            K::Derivative(d) => self.derivative(n, d, st, path),
            K::Nabla { op, body } => {
                let nabla = Atom::ord(self.sym('∇', st));
                let mut atoms = vec![];
                match op {
                    NablaOp::Grad => atoms.push(nabla),
                    NablaOp::Div => {
                        atoms.push(nabla);
                        atoms.push(Atom::new(Class::Bin, self.sym('⋅', st)));
                    }
                    NablaOp::Curl => {
                        atoms.push(nabla);
                        atoms.push(Atom::new(Class::Bin, self.sym('×', st)));
                    }
                    NablaOp::Laplacian => {
                        let two = self.sym('2', st.sup());
                        atoms.push(Atom::ord(self.scripts(nabla.b, None, Some(two), st, false)));
                    }
                }
                if let Some(b) = body {
                    atoms.push(self.child(n, b, st, path));
                }
                self.group(atoms, st)
            }
            K::NamedOp(op) => self.named_op(n, op, st, path),
            K::Accented { accent, inner } => {
                let x = self.child(n, inner, st.cramp(), path);
                Atom::ord(self.accent(x.b, *accent, st))
            }
            K::Styled { font, inner } => {
                let saved = self.font;
                self.font = Some(*font);
                let x = self.child(n, inner, st, path);
                self.font = saved;
                x
            }
            K::Quantity { value, unit } => {
                let v = self.child(n, value, st, path);
                let thin = Atom::ord(LBox::kern(0.1667 * self.scale(st)));
                let u = Atom::ord(self.unit_box(&unit.text, st));
                self.group(vec![v, thin, u], st)
            }
            K::Convert { expr, unit } => {
                let e = self.child(n, expr, st, path);
                let gap = Atom::ord(LBox::kern(0.3 * self.scale(st)));
                let word = Atom::ord(self.text("in", FontId::Text, st));
                let gap2 = Atom::ord(LBox::kern(0.3 * self.scale(st)));
                let u = Atom::ord(self.unit_box(&unit.text, st));
                self.group(vec![e, gap, word, gap2, u], st)
            }
            K::SetBuilder {
                var,
                domain,
                condition,
            } => {
                let mut atoms = vec![self.child(n, var, st, path)];
                if let Some(d) = domain {
                    atoms.push(self.rel(RelOp::In, st));
                    atoms.push(self.child(n, d, st, path));
                }
                atoms.push(Atom::new(Class::Rel, self.sym(':', st)));
                atoms.push(self.child(n, condition, st, path));
                let inner = self.group(atoms, st);
                self.fenced(Some('{'), inner, Some('}'), st)
            }
            K::Relation { first, rest } => {
                let mut atoms = vec![self.child(n, first, st, path)];
                for (op, rhs) in rest {
                    atoms.push(self.rel(*op, st));
                    atoms.push(self.child(n, rhs, st, path));
                }
                self.group(atoms, st)
            }
        }
    }

    /// An upright roman word (`mod`, units).
    pub fn roman(&mut self, s: &str, st: Style) -> LBox {
        let saved = self.font;
        self.font = Some(Font::Roman);
        let w = self.word(s, st);
        self.font = saved;
        w
    }

    /// A unit typeset upright: `m/s^2`, `\mathrm{km/h}`, `°C`, with exponents raised.
    pub fn unit_box(&mut self, text: &str, st: Style) -> LBox {
        let t = text.trim();
        let t = t
            .strip_prefix("\\mathrm{")
            .and_then(|r| r.strip_suffix('}'))
            .unwrap_or(t)
            .replace("\\circ", "°")
            .replace("\\cdot", "⋅")
            .replace("\\Omega", "Ω")
            .replace("\\mu", "μ");
        let chars: Vec<char> = t.chars().filter(|c| !matches!(c, '{' | '}')).collect();
        let mut parts: Vec<LBox> = vec![];
        let mut i = 0;
        let mut plain = String::new();
        while i < chars.len() {
            if chars[i] == '^' {
                // The exponent attaches to the last character before it.
                let last = plain.pop();
                if !plain.is_empty() {
                    parts.push(self.roman(&plain, st));
                    plain.clear();
                }
                i += 1;
                let mut exp = String::new();
                if i < chars.len() && chars[i] == '-' {
                    exp.push('−');
                    i += 1;
                }
                while i < chars.len() && chars[i].is_ascii_digit() {
                    exp.push(chars[i]);
                    i += 1;
                }
                let sup = self.roman(&exp, st.sup());
                let base = match last {
                    Some(c) => self.roman(&c.to_string(), st),
                    None => parts.pop().unwrap_or_else(LBox::empty),
                };
                parts.push(self.scripts(base, None, Some(sup), st, false));
            } else {
                plain.push(chars[i]);
                i += 1;
            }
        }
        if !plain.is_empty() {
            parts.push(self.roman(&plain, st));
        }
        LBox::hlist(parts)
    }

    /// A relation symbol as an atom.
    pub fn rel(&self, op: RelOp, st: Style) -> Atom {
        Atom::new(Class::Rel, self.sym_str(op.symbol(), st))
    }

    /// Comma-separated children.
    pub fn list(&mut self, parent: &Node, items: &[Node], st: Style, path: &NodePath) -> Atom {
        let mut atoms = vec![];
        for (i, it) in items.iter().enumerate() {
            if i > 0 {
                atoms.push(Atom::new(Class::Punct, self.sym(',', st)));
            }
            atoms.push(self.child(parent, it, st, path));
        }
        self.group(atoms, st)
    }

    fn constant(&self, c: Constant, st: Style) -> LBox {
        match c {
            Constant::Pi => self.letter('π', st, false),
            Constant::E => self.letter('e', st, false),
            Constant::I => self.letter('i', st, false),
            Constant::Infinity => self.sym('∞', st),
        }
    }

    /// A name with its font, accent and subscript. `with_sub` false leaves the
    /// subscript off (the caller attaches it together with a superscript).
    pub fn name(
        &mut self,
        node: &Node,
        name: &Name,
        st: Style,
        path: &NodePath,
        with_sub: bool,
    ) -> LBox {
        let saved = self.font;
        if name.font.is_some() {
            self.font = name.font;
        }
        let mut chars = name.base.chars();
        let base = match (chars.next(), chars.next(), chars.next()) {
            (Some(c), None, _) => self.letter(c, st, false),
            // `\Delta x`, `\delta t`: one name, set as the two letters it is written with.
            (Some(d @ ('Δ' | 'δ')), Some(c), None) if c.is_alphabetic() => LBox::hlist(vec![
                self.letter(d, st, d == 'Δ'),
                self.letter(c, st, false),
            ]),
            _ => self.word(&name.base, st),
        };
        self.font = saved;
        let base = match name.accent {
            Some(a) => self.accent(base, a, st),
            None => base,
        };
        match (&name.sub, with_sub) {
            (Some(sub), true) => {
                let s = self.child(node, sub, st.sub(), path);
                self.scripts(base, Some(s.b), None, st, false)
            }
            _ => base,
        }
    }

    fn func_app(&mut self, n: &Node, app: &FuncApp, st: Style, path: &NodePath) -> Atom {
        let word = self.word(func_word(app.func), st);
        let sup = if app.inverse {
            Some(self.inverse_mark(st.sup()))
        } else {
            app.power
                .as_ref()
                .map(|p| self.child(n, p, st.sup(), path).b)
        };
        let sub = app.sub.as_ref().map(|s| self.child(n, s, st.sub(), path).b);
        let head = if has_limits(app.func) && st.is_display() {
            let with_sup = self.scripts(word, None, sup, st, false);
            self.limits(with_sup, sub, None, st)
        } else {
            self.scripts(word, sub, sup, st, false)
        };
        let mut atoms = vec![Atom::new(Class::Op, head)];
        if app.bracketed {
            let inner = self.list(n, &app.args, st, path);
            atoms.push(self.fenced(Some('('), inner, Some(')'), st));
        } else if !app.args.is_empty() {
            atoms.push(self.list(n, &app.args, st, path));
        }
        self.group(atoms, st)
    }

    /// `−1` as a superscript.
    fn inverse_mark(&self, st: Style) -> LBox {
        LBox::hlist(vec![self.sym('−', st), self.sym('1', st)])
    }

    fn frac(
        &mut self,
        n: &Node,
        num: &Node,
        den: &Node,
        style: FracStyle,
        st: Style,
        path: &NodePath,
    ) -> Atom {
        match style {
            FracStyle::Slash | FracStyle::Obelus => {
                let a = self.child(n, num, st, path);
                let b = self.child(n, den, st, path);
                let o = if style == FracStyle::Slash {
                    Atom::ord(self.sym('/', st))
                } else {
                    Atom::new(Class::Bin, self.sym('÷', st))
                };
                self.group(vec![a, o, b], st)
            }
            _ => {
                let st = match style {
                    FracStyle::DFrac => Style {
                        level: Level::Display,
                        cramped: st.cramped,
                    },
                    FracStyle::TFrac if st.level == Level::Display => Style {
                        level: Level::Text,
                        cramped: st.cramped,
                    },
                    _ => st,
                };
                let a = self.child_h(n, num, st.num(), path, "numerator");
                let b = self.child_h(n, den, st.den(), path, "denominator");
                Atom::new(Class::Inner, self.fraction(a.b, b.b, st, true))
            }
        }
    }

    /// Splits a script base into nucleus and an existing subscript.
    fn pow(&mut self, n: &Node, base: &Node, exp: &Node, st: Style, path: &NodePath) -> Atom {
        let bpath = child_path(n, base, path);
        let sup = self.child_h(n, exp, st.sup(), path, "power");
        self.nucleus_with(base, &bpath, st, None, Some(sup.b))
    }

    fn subscript(&mut self, n: &Node, base: &Node, sub: &Node, st: Style, path: &NodePath) -> Atom {
        let bpath = child_path(n, base, path);
        let s = self.child_h(n, sub, st.sub(), path, "index");
        self.nucleus_with(base, &bpath, st, Some(s.b), None)
    }

    fn prime(&mut self, n: &Node, base: &Node, count: u8, st: Style, path: &NodePath) -> Atom {
        let bpath = child_path(n, base, path);
        let primes = self.primes(count, st.sup());
        self.nucleus_with(base, &bpath, st, None, Some(primes))
    }

    fn primes(&self, count: u8, st: Style) -> LBox {
        let s = match count {
            0 | 1 => "′".to_string(),
            2 => "″".to_string(),
            3 => "‴".to_string(),
            k => "′".repeat(usize::from(k)),
        };
        self.sym_str(&s, st)
    }

    /// Lays out `base` and attaches scripts, merging with scripts the base
    /// already has (`x_n^2`, `f'^2`, `y_1'`).
    fn nucleus_with(
        &mut self,
        base: &Node,
        bpath: &NodePath,
        st: Style,
        sub: Option<LBox>,
        sup: Option<LBox>,
    ) -> Atom {
        if self.depth >= MAX_DEPTH {
            return Atom::ord(self.slot(Some("…"), st, base.span, bpath));
        }
        self.depth += 1;
        let out = self.nucleus_inner(base, bpath, st, sub, sup);
        self.depth -= 1;
        out
    }

    fn nucleus_inner(
        &mut self,
        base: &Node,
        bpath: &NodePath,
        st: Style,
        sub: Option<LBox>,
        sup: Option<LBox>,
    ) -> Atom {
        let hit = self.hit(base.span, bpath, HitKind::Node);
        let wrap = |b: LBox, hit: Option<super::boxes::Hit>| match hit {
            Some(h) => b.with_hit(h),
            None => b,
        };
        match &base.kind {
            NodeKind::Name(name) if name.sub.is_some() && sub.is_none() => {
                let nucleus = self.name(base, name, st, bpath, false);
                let s = name
                    .sub
                    .as_ref()
                    .map(|s| self.child(base, s, st.sub(), bpath).b);
                Atom::ord(wrap(self.scripts(nucleus, s, sup, st, false), hit))
            }
            NodeKind::Subscript { base: b, sub: s } if sub.is_none() => {
                let inner_path = child_path(base, b, bpath);
                let sb = self.child(base, s, st.sub(), bpath).b;
                let a = self.nucleus_with(b, &inner_path, st, Some(sb), sup);
                Atom {
                    b: wrap(a.b, hit),
                    ..a
                }
            }
            NodeKind::Pow { base: b, exp } if sup.is_none() => {
                let inner_path = child_path(base, b, bpath);
                let e = self.child(base, exp, st.sup(), bpath).b;
                let a = self.nucleus_with(b, &inner_path, st, sub, Some(e));
                Atom {
                    b: wrap(a.b, hit),
                    ..a
                }
            }
            NodeKind::Prime { base: b, count } => {
                let inner_path = child_path(base, b, bpath);
                let primes = self.primes(*count, st.sup());
                let sup = match sup {
                    Some(s) => LBox::hlist(vec![primes, s]),
                    None => primes,
                };
                let a = self.nucleus_with(b, &inner_path, st, sub, Some(sup));
                Atom {
                    b: wrap(a.b, hit),
                    ..a
                }
            }
            NodeKind::FuncApp(app) if app.power.is_none() && !app.inverse && sup.is_some() => {
                // `\sin x` with a power written after it: put the power on the name.
                let mut app2 = (**app).clone();
                app2.power = None;
                let word = self.word(func_word(app.func), st);
                let head = self.scripts(word, sub, sup, st, false);
                let mut atoms = vec![Atom::new(Class::Op, head)];
                if app2.bracketed {
                    let inner = self.list(base, &app.args, st, bpath);
                    atoms.push(self.fenced(Some('('), inner, Some(')'), st));
                } else if !app2.args.is_empty() {
                    atoms.push(self.list(base, &app.args, st, bpath));
                }
                let g = self.group(atoms, st);
                Atom {
                    b: wrap(g.b, hit),
                    ..g
                }
            }
            _ => {
                let a = self.atom(base, st, bpath, Some("base"));
                let is_op = a.left == Class::Op && a.right == Class::Op;
                let b = self.scripts(a.b, sub, sup, st, is_op);
                Atom {
                    left: a.left,
                    right: a.right,
                    b,
                }
            }
        }
    }

    fn big_op_node(&mut self, n: &Node, op: &BigOp, st: Style, path: &NodePath) -> Atom {
        let ch = match op.op {
            BigOpKind::Sum => '∑',
            BigOpKind::Prod => '∏',
            BigOpKind::Coprod => '∐',
        };
        let sign = self.big_op(ch, st);
        let ss = st.sub();
        let lower = match (&op.index, &op.from, &op.over) {
            (Some(i), Some(f), _) => {
                let a = self.child_h(n, i, ss, path, "n");
                let eq = self.rel(RelOp::Eq, ss);
                let b = self.child_h(n, f, ss, path, "start");
                Some(self.group(vec![a, eq, b], ss).b)
            }
            (None, Some(f), _) => Some(self.child_h(n, f, ss, path, "start").b),
            (_, None, Some(o)) => Some(self.child_h(n, o, ss, path, "over").b),
            (Some(i), None, None) => Some(self.child_h(n, i, ss, path, "n").b),
            (None, None, None) => None,
        };
        let upper = op
            .to
            .as_ref()
            .map(|t| self.child_h(n, t, st.sup(), path, "end").b);
        let head = if st.is_display() {
            self.limits(sign, lower, upper, st)
        } else {
            self.scripts(sign, lower, upper, st, true)
        };
        let body = self.child_h(n, &op.body, st, path, "term");
        self.group(vec![Atom::new(Class::Op, head), body], st)
    }

    fn integral(&mut self, n: &Node, int: &Integral, st: Style, path: &NodePath) -> Atom {
        let ch = match int.kind {
            IntKind::Single => '∫',
            IntKind::Double => '∬',
            IntKind::Triple => '∭',
            IntKind::Contour => '∮',
            IntKind::Surface => '∯',
        };
        let sign = self.big_op(ch, st);
        let region = int.upper.is_none();
        let lower = int.lower.as_ref().map(|l| {
            let hint = if region { "region" } else { "lower" };
            self.child_h(n, l, st.sub(), path, hint).b
        });
        let upper = int
            .upper
            .as_ref()
            .map(|u| self.child_h(n, u, st.sup(), path, "upper").b);
        let stacked = st.is_display()
            && (self.integral_limits() == IntegralLimits::AboveBelow
                || (region && int.kind != IntKind::Single));
        let head = if stacked {
            self.limits(sign, lower, upper, st)
        } else {
            self.scripts(sign, lower, upper, st, true)
        };
        let mut atoms = vec![Atom::new(Class::Op, head)];
        if let (true, NodeKind::Tuple { elems, .. }) = (int.form, &int.body.kind) {
            // `(P\,dx + Q\,dy)`: each coefficient followed by its differential.
            let bpath = child_path(n, &int.body, path);
            let mut inner = vec![];
            for (i, (c, v)) in elems.iter().zip(&int.diffs).enumerate() {
                if i > 0 {
                    inner.push(Atom::new(Class::Bin, self.sym('+', st)));
                }
                inner.push(self.child(&int.body, c, st, &bpath));
                inner.push(Atom::ord(LBox::kern(3.0 * self.mu(st))));
                let d = self.letter('d', st, false);
                let x = self.child_h(n, v, st, path, "variable");
                inner.push(Atom::ord(LBox::hlist(vec![d, x.b])));
            }
            let body = self.group(inner, st);
            atoms.push(self.fenced(Some('('), body, Some(')'), st));
            return self.group(atoms, st);
        }
        let mut diffs_done = false;
        match (&int.body.kind, int.diff_in_numerator) {
            (NodeKind::Frac { den, style, .. }, true) if *style != FracStyle::Slash => {
                // `\int \frac{dx}{x}`: put the differential back in the numerator.
                let bpath = child_path(n, &int.body, path);
                let num = self.differentials(n, int, st.num(), path);
                let d = self.child(&int.body, den, st.den(), &bpath);
                let mut f = self.fraction(num.b, d.b, st, true);
                if let Some(hit) = self.hit(int.body.span, &bpath, HitKind::Node) {
                    f = f.with_hit(hit);
                }
                atoms.push(Atom::new(Class::Inner, f));
                diffs_done = true;
            }
            _ => atoms.push(self.child_h(n, &int.body, st, path, "integrand")),
        }
        if !diffs_done && !int.diffs.is_empty() {
            if int.dot {
                atoms.push(Atom::new(Class::Bin, self.sym('⋅', st)));
            } else {
                atoms.push(Atom::ord(LBox::kern(3.0 * self.mu(st))));
            }
            atoms.push(self.differentials(n, int, st, path));
        }
        self.group(atoms, st)
    }

    /// `dx dy …` with thin spaces between.
    fn differentials(&mut self, n: &Node, int: &Integral, st: Style, path: &NodePath) -> Atom {
        let mut atoms = vec![];
        for (i, v) in int.diffs.iter().enumerate() {
            if i > 0 {
                atoms.push(Atom::ord(LBox::kern(3.0 * self.mu(st))));
            }
            let d = self.letter('d', st, false);
            let x = self.child_h(n, v, st, path, "variable");
            atoms.push(Atom::ord(LBox::hlist(vec![d, x.b])));
        }
        if atoms.is_empty() {
            atoms.push(Atom::ord(LBox::empty()));
        }
        self.group(atoms, st)
    }

    fn limit(&mut self, n: &Node, lim: &Limit, st: Style, path: &NodePath) -> Atom {
        let word = self.word("lim", st);
        let ss = st.sub();
        let var = self.child_h(n, &lim.var, ss, path, "variable");
        let arrow = self.rel(RelOp::To, ss);
        let mut target = self.child_h(n, &lim.target, ss, path, "value");
        if let Some(side) = lim.side {
            let mark = self.sym(if side == Side::Right { '+' } else { '−' }, ss.sup());
            target.b = self.scripts(target.b, None, Some(mark), ss, false);
        }
        let lower = self.group(vec![var, arrow, target], ss).b;
        let head = if st.is_display() {
            self.limits(word, Some(lower), None, st)
        } else {
            self.scripts(word, Some(lower), None, st, true)
        };
        let body = self.child_h(n, &lim.body, st, path, "expression");
        self.group(vec![Atom::new(Class::Op, head), body], st)
    }

    fn derivative(&mut self, n: &Node, d: &Derivative, st: Style, path: &NodePath) -> Atom {
        let dch = if d.partial { '∂' } else { 'd' };
        let dbox = |e: &Engine<'_>, s: Style| {
            if d.partial {
                e.sym('∂', s)
            } else {
                e.letter('d', s, false)
            }
        };
        let _ = dch;
        match d.notation {
            DerivNotation::Operator { frac } | DerivNotation::Whole { frac } => {
                let whole = matches!(d.notation, DerivNotation::Whole { .. });
                let (nst, dst) = if frac { (st.num(), st.den()) } else { (st, st) };
                // Numerator: d (with order), plus the function for the whole form.
                let mut top = dbox(self, nst);
                if let Some(o) = &d.order {
                    let ob = self.child(n, o, nst.sup(), path).b;
                    top = self.scripts(top, None, Some(ob), nst, false);
                }
                let mut num_atoms = vec![Atom::ord(top)];
                if whole {
                    if let Some(of) = &d.of {
                        num_atoms.push(self.child(n, of, nst, path));
                    }
                }
                let num = self.group(num_atoms, nst);
                let den = self.deriv_vars(n, d, dst, path);
                let mut atoms = if frac {
                    vec![Atom::new(
                        Class::Inner,
                        self.fraction(num.b, den.b, st, true),
                    )]
                } else {
                    vec![num, Atom::ord(self.sym('/', st)), den]
                };
                if !whole {
                    if let Some(of) = &d.of {
                        atoms.push(self.child(n, of, st, path));
                    }
                }
                self.group(atoms, st)
            }
            DerivNotation::PartialSub | DerivNotation::DSub => {
                let head = if d.notation == DerivNotation::DSub {
                    self.letter('D', st, false)
                } else {
                    self.sym('∂', st)
                };
                let mut subs = vec![];
                for v in &d.vars {
                    subs.push(self.child_h(n, &v.var, st.sub(), path, "variable"));
                }
                let sub = (!subs.is_empty()).then(|| self.group(subs, st.sub()).b);
                let head = self.scripts(head, sub, None, st, false);
                let mut atoms = vec![Atom::ord(head)];
                if let Some(of) = &d.of {
                    atoms.push(self.child(n, of, st, path));
                }
                self.group(atoms, st)
            }
            DerivNotation::Dot(k) => {
                let accent = if k >= 2 { Accent::DDot } else { Accent::Dot };
                let target = d.of.as_ref().or(d.vars.first().map(|v| &v.var));
                match target {
                    Some(t) => {
                        let x = self.child(n, t, st.cramp(), path);
                        Atom::ord(self.accent(x.b, accent, st))
                    }
                    None => Atom::ord(self.slot(Some("variable"), st, n.span, path)),
                }
            }
            DerivNotation::Bare => {
                let mut atoms = vec![Atom::ord(self.sym('∂', st))];
                if let Some(of) = &d.of {
                    atoms.push(self.child(n, of, st, path));
                }
                self.group(atoms, st)
            }
        }
    }

    /// The denominator of a derivative: `dx`, `dx^2`, `∂x ∂y`.
    fn deriv_vars(&mut self, n: &Node, d: &Derivative, st: Style, path: &NodePath) -> Atom {
        let mut atoms = vec![];
        for (i, v) in d.vars.iter().enumerate() {
            if i > 0 {
                atoms.push(Atom::ord(LBox::kern(2.0 * self.mu(st))));
            }
            let dd = if d.partial {
                self.sym('∂', st)
            } else {
                self.letter('d', st, false)
            };
            let x = self.child_h(n, &v.var, st, path, "variable");
            let x = match &v.power {
                Some(p) => {
                    let pb = self.child(n, p, st.sup(), path).b;
                    self.scripts(x.b, None, Some(pb), st, false)
                }
                None => x.b,
            };
            atoms.push(Atom::ord(LBox::hlist(vec![dd, x])));
        }
        if atoms.is_empty() {
            let dd = self.letter('d', st, false);
            let slot = self.slot(Some("variable"), st, n.span, path);
            atoms.push(Atom::ord(LBox::hlist(vec![dd, slot])));
        }
        self.group(atoms, st)
    }

    fn named_op(&mut self, n: &Node, op: &NamedOp, st: Style, path: &NodePath) -> Atom {
        match operator_display(&op.name) {
            Some(OpDisplay::Transform { letter, inverse }) => {
                return self.transform(n, op, letter, inverse, st, path)
            }
            Some(OpDisplay::Phrase(pieces, _)) if op.args.len() == 1 => {
                return self.phrase(n, op, pieces, st, path)
            }
            Some(OpDisplay::Words(words) | OpDisplay::Phrase(_, words)) => {
                let word = self.text(words, FontId::Text, st);
                return self.named_op_with(n, op, word, st, path);
            }
            None => {}
        }
        let saved = self.font;
        self.font = Some(Font::Roman);
        let word = self.word(&op.name, st);
        self.font = saved;
        self.named_op_with(n, op, word, st, path)
    }

    /// `ℒ_{t→s}{f}`, `ℒ⁻¹{F}`, `𝓕{f}`, `𝓕⁻¹{F}`.
    fn transform(
        &mut self,
        n: &Node,
        op: &NamedOp,
        letter: char,
        inverse: bool,
        st: Style,
        path: &NodePath,
    ) -> Atom {
        let head = self.sym(chars::in_font(Font::Calligraphic, letter), st);
        let sub = op.sub.as_ref().map(|s| self.child(n, s, st.sub(), path).b);
        let sup = if inverse {
            let ss = st.sup();
            let minus = Atom::ord(self.sym('−', ss));
            let one = Atom::ord(self.sym('1', ss));
            Some(self.group(vec![minus, one], ss).b)
        } else {
            None
        };
        let head = self.scripts(head, sub, sup, st, false);
        let inner = self.list(n, &op.args, st, path);
        let body = self.fenced(Some('{'), inner, Some('}'), st);
        self.group(vec![Atom::ord(head), body], st)
    }

    /// A helper as a sentence: "Critical points of f on [0, 4]".
    fn phrase(
        &mut self,
        n: &Node,
        op: &NamedOp,
        pieces: &[Piece],
        st: Style,
        path: &NodePath,
    ) -> Atom {
        let space = 0.3 * self.scale(st);
        let mut parts: Vec<LBox> = vec![];
        for piece in pieces {
            let (words, node) = match piece {
                Piece::Text(w) => (*w, None),
                Piece::Args => ("", Some(None)),
                Piece::Sub(w) => match &op.sub {
                    Some(s) => (*w, Some(Some(s))),
                    None => continue,
                },
                Piece::Sup(w) => match &op.sup {
                    Some(s) => (*w, Some(Some(s))),
                    None => continue,
                },
            };
            if !words.is_empty() {
                let punct = words.starts_with([',', ':', '?', '.']);
                if !punct && !parts.is_empty() {
                    parts.push(LBox::kern(space));
                }
                parts.push(self.text(words, FontId::Text, st));
            }
            if let Some(node) = node {
                let b = match node {
                    Some(s) => self.child(n, s, st, path).b,
                    None => self.list(n, &op.args, st, path).b,
                };
                if !parts.is_empty() {
                    parts.push(LBox::kern(space));
                }
                parts.push(b);
            }
        }
        Atom::new(Class::Inner, LBox::hlist(parts))
    }

    fn named_op_with(
        &mut self,
        n: &Node,
        op: &NamedOp,
        word: LBox,
        st: Style,
        path: &NodePath,
    ) -> Atom {
        let sub = op.sub.as_ref().map(|s| self.child(n, s, st.sub(), path).b);
        let sup = op.sup.as_ref().map(|s| self.child(n, s, st.sup(), path).b);
        let head = self.scripts(word, sub, sup, st, false);
        let mut atoms = vec![Atom::new(Class::Op, head)];
        if op.bracketed {
            let inner = self.list(n, &op.args, st, path);
            atoms.push(self.fenced(Some('('), inner, Some(')'), st));
        } else if !op.args.is_empty() {
            atoms.push(self.list(n, &op.args, st, path));
        }
        self.group(atoms, st)
    }

    /// Lays out a grid of cells, columns centred (or left-aligned), rows spaced
    /// like a TeX array, centred on the axis.
    pub fn grid(&self, cells: Vec<Vec<LBox>>, left_align: bool, col_gap: f32, st: Style) -> LBox {
        let s = self.scale(st);
        let ncols = cells.iter().map(Vec::len).max().unwrap_or(0);
        let mut widths = vec![0.0f32; ncols];
        for row in &cells {
            for (j, c) in row.iter().enumerate() {
                widths[j] = widths[j].max(c.width);
            }
        }
        let strut_h = 0.84 * s;
        let strut_d = 0.36 * s;
        let mut rows = vec![];
        let mut y = 0.0f32;
        let mut first = true;
        for row in cells {
            let h = row.iter().map(|c| c.height).fold(strut_h, f32::max);
            let d = row.iter().map(|c| c.depth).fold(strut_d, f32::max);
            if !first {
                y += h;
            }
            first = false;
            let mut x = 0.0;
            for (j, c) in row.into_iter().enumerate() {
                let w = widths[j];
                let cx = if left_align {
                    x
                } else {
                    x + (w - c.width) / 2.0
                };
                rows.push(Placed { x: cx, y, b: c });
                x += w + col_gap;
            }
            y += d;
        }
        let total_w: f32 = widths.iter().sum::<f32>() + col_gap * ncols.saturating_sub(1) as f32;
        // y now is the bottom; the first row's baseline is at 0 with height strut_h at least.
        let first_h = rows
            .iter()
            .filter(|p| p.y == 0.0)
            .map(|p| p.b.height)
            .fold(strut_h, f32::max);
        let top = -first_h;
        let bottom = y;
        let height_total = bottom - top;
        // Centre on the axis: middle of the box sits at the axis.
        let mid = top + height_total / 2.0;
        let shift = -self.axis(st) - mid;
        for p in &mut rows {
            p.y += shift;
        }
        LBox::with_children(
            total_w,
            (-(top + shift)).max(0.0),
            (bottom + shift).max(0.0),
            rows,
        )
    }

    fn matrix(
        &mut self,
        n: &Node,
        env: MatrixEnv,
        rows: &[Vec<Node>],
        st: Style,
        path: &NodePath,
    ) -> Atom {
        let cst = if env == MatrixEnv::SmallMatrix {
            st.sup()
        } else {
            Style {
                level: if st.level == Level::Display {
                    Level::Text
                } else {
                    st.level
                },
                cramped: st.cramped,
            }
        };
        let mut cells = vec![];
        for row in rows {
            let mut r = vec![];
            for c in row {
                r.push(self.child_h(n, c, cst, path, "").b);
            }
            cells.push(r);
        }
        let gap = if env == MatrixEnv::SmallMatrix {
            0.5
        } else {
            1.0
        } * self.scale(cst);
        let g = self.grid(cells, false, gap, cst);
        let (o, c) = match env {
            MatrixEnv::Matrix | MatrixEnv::SmallMatrix => (None, None),
            MatrixEnv::PMatrix => (Some('('), Some(')')),
            MatrixEnv::BMatrix => (Some('['), Some(']')),
            MatrixEnv::BraceMatrix => (Some('{'), Some('}')),
            MatrixEnv::VMatrix => (Some('|'), Some('|')),
            MatrixEnv::DoubleVMatrix => (Some('‖'), Some('‖')),
        };
        // A little room inside the brackets, like amsmath's \arraycolsep.
        let pad = if o.is_some() {
            0.2 * self.scale(st)
        } else {
            0.0
        };
        let inner = LBox::hlist(vec![LBox::kern(pad), g, LBox::kern(pad)]);
        self.fenced(o, Atom::ord(inner), c, st)
    }

    fn cases(
        &mut self,
        n: &Node,
        rows: &[mpp_syntax::math::CaseRow],
        st: Style,
        path: &NodePath,
    ) -> Atom {
        let cst = Style {
            level: if st.level == Level::Display {
                Level::Text
            } else {
                st.level
            },
            cramped: st.cramped,
        };
        let mut cells = vec![];
        for row in rows {
            let v = self.child_h(n, &row.value, cst, path, "value").b;
            let cond = match &row.condition {
                CaseCondition::When(c) => self.child_h(n, c, cst, path, "condition").b,
                CaseCondition::Otherwise => self.text("otherwise", FontId::Text, cst),
                CaseCondition::Missing => LBox::empty(),
            };
            cells.push(vec![v, cond]);
        }
        let g = self.grid(cells, true, self.scale(cst), cst);
        let inner = LBox::hlist(vec![LBox::kern(0.1 * self.scale(st)), g]);
        self.fenced(Some('{'), Atom::ord(inner), None, st)
    }
}

fn bin_symbol(op: BinOp) -> Option<char> {
    Some(match op {
        BinOp::Add => '+',
        BinOp::Sub => '−',
        BinOp::PlusMinus => '±',
        BinOp::MinusPlus => '∓',
        BinOp::Mul(MulStyle::Implicit) => return None,
        BinOp::Mul(MulStyle::Star) | BinOp::Mul(MulStyle::Cdot) => '⋅',
        BinOp::Times => '×',
        BinOp::Compose => '∘',
        BinOp::Tensor => '⊗',
        BinOp::DirectSum => '⊕',
        BinOp::Union => '∪',
        BinOp::Intersect => '∩',
        BinOp::SetMinus => '∖',
        BinOp::And => '∧',
        BinOp::Or => '∨',
        BinOp::Mod => return None,
    })
}

fn bracket_chars(b: Bracket) -> (Option<char>, Option<char>) {
    match b {
        Bracket::Paren => (Some('('), Some(')')),
        Bracket::Square => (Some('['), Some(']')),
        Bracket::Brace => (Some('{'), Some('}')),
        Bracket::Angle => (Some('⟨'), Some('⟩')),
        Bracket::TexGroup | Bracket::None => (None, None),
        Bracket::EvalBar => (None, Some('|')),
    }
}
