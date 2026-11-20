//! Statement lines: definitions, queries with their results, checks, clauses,
//! several lines stacked or aligned, and equation numbers.

use mpp_syntax::math::{
    Clause, DefHead, DefKind, Line, MathAst, Node, NodeKind, QueryMode, RelOp, Statement,
};

use super::boxes::{LBox, Placed};
use super::engine::{Atom, Engine};
use super::style::{Class, Style};
use super::{parse_result, LineResult, MathStyle, ResultKind, ResultValue};
use crate::display::{HitKind, NodePath};
use crate::font::FontId;

/// The top-level nodes of a line in source order; a node path's second index
/// picks one of these.
pub fn line_roots(line: &Line) -> Vec<&Node> {
    let mut v: Vec<&Node> = vec![];
    match &line.statement {
        Statement::Definition { head, body, .. } => {
            match head {
                DefHead::Name(n) | DefHead::Invalid(n) => v.push(n),
                DefHead::Function { name, params } => {
                    v.push(name);
                    v.extend(params.iter());
                }
                DefHead::Recurrence { name, index, shift } => {
                    v.extend([name, index, shift]);
                }
                DefHead::Initial { name, index } => v.extend([name, index]),
            }
            v.push(body);
        }
        Statement::Query { expr, .. } => v.push(expr),
        Statement::Check { lhs, rhs } => v.extend([lhs, rhs]),
        Statement::Relation(n) => v.push(n),
        Statement::NameList(ns) => v.extend(ns.iter()),
        Statement::Range(r) => {
            v.extend(r.names.iter());
            v.push(&r.set);
        }
        Statement::Empty => {}
    }
    for c in &line.clauses {
        match c {
            Clause::Where { name, value, .. } => v.extend([name, value]),
            Clause::Range(r) => {
                v.extend(r.names.iter());
                v.push(&r.set);
            }
            Clause::Condition(n) => v.push(n),
        }
    }
    v
}

/// The node a [`NodePath`] points at.
pub fn node_at_path<'a>(ast: &'a MathAst, path: &NodePath) -> Option<&'a Node> {
    let mut it = path.0.iter();
    let line = ast.lines.get(usize::from(*it.next()?))?;
    let roots = line_roots(line);
    let mut node = *roots.get(usize::from(*it.next()?))?;
    for &i in it {
        let mut found = None;
        let mut k: u16 = 0;
        mpp_syntax::math::visit::children(node, &mut |c| {
            if k == i && found.is_none() {
                found = Some(c);
            }
            k = k.saturating_add(1);
        });
        node = found?;
    }
    Some(node)
}

/// Lays out a whole maths span.
pub fn layout_ast(e: &mut Engine<'_>, ast: &MathAst) -> LBox {
    let st = base_style(e);
    let mut lines = vec![];
    for (i, line) in ast.lines.iter().enumerate() {
        let result = e.opts.results.iter().find(|r| r.line == i).cloned();
        lines.push(layout_line(e, line, i, result.as_ref(), st));
    }
    // Results for lines past the end (or for an empty span) are ignored.
    let body = stack_lines(e, lines, ast.aligned, st);
    with_number(e, body, ast.lines.len() > 1, st)
}

/// Lays out a single expression.
pub fn layout_single(e: &mut Engine<'_>, node: &Node) -> LBox {
    let st = base_style(e);
    let a = e.atom(node, st, &NodePath(vec![0, 0]), None);
    let b = a.b;
    with_number(e, b, false, st)
}

fn base_style(e: &Engine<'_>) -> Style {
    match e.opts.style {
        MathStyle::Display => Style::DISPLAY,
        MathStyle::Text => Style::TEXT,
    }
}

/// A laid-out line and the x of its alignment point (its first relation).
struct LaidLine {
    b: LBox,
    align: Option<f32>,
}

/// Hands out the paths of a line's top-level nodes in order.
struct Slots {
    line: NodePath,
    next: u16,
}

impl Slots {
    fn next(&mut self) -> NodePath {
        let p = self.line.child(self.next);
        self.next = self.next.saturating_add(1);
        p
    }
}

fn layout_line(
    e: &mut Engine<'_>,
    line: &Line,
    index: usize,
    result: Option<&LineResult>,
    st: Style,
) -> LaidLine {
    let mut sl = Slots {
        line: NodePath(vec![u16::try_from(index).unwrap_or(u16::MAX)]),
        next: 0,
    };
    let mut atoms: Vec<Atom> = vec![];
    // Index into `atoms` of the relation to align on.
    let mut align_at: Option<usize> = None;
    match &line.statement {
        Statement::Definition { head, kind, body } => {
            match head {
                DefHead::Name(n) | DefHead::Invalid(n) => {
                    atoms.push(e.atom(n, st, &sl.next(), Some("name")));
                }
                DefHead::Function { name, params } => {
                    let f = e.atom(name, st, &sl.next(), Some("name"));
                    let mut inner = vec![];
                    for (i, p) in params.iter().enumerate() {
                        if i > 0 {
                            inner.push(Atom::new(Class::Punct, e.sym(',', st)));
                        }
                        inner.push(e.atom(p, st, &sl.next(), Some("variable")));
                    }
                    let inner = e.group(inner, st);
                    let call = e.fenced(Some('('), inner, Some(')'), st);
                    atoms.push(e.group(vec![f, call], st));
                }
                DefHead::Recurrence { name, index, shift } => {
                    let a = e.atom(name, st, &sl.next(), Some("name"));
                    let ss = st.sub();
                    let i = e.atom(index, ss, &sl.next(), Some("n"));
                    let plus = Atom::new(Class::Bin, e.sym('+', ss));
                    let s = e.atom(shift, ss, &sl.next(), Some("1"));
                    let sub = e.group(vec![i, plus, s], ss);
                    atoms.push(Atom::ord(e.scripts(a.b, Some(sub.b), None, st, false)));
                }
                DefHead::Initial { name, index } => {
                    let a = e.atom(name, st, &sl.next(), Some("name"));
                    let i = e.atom(index, st.sub(), &sl.next(), Some("n"));
                    atoms.push(Atom::ord(e.scripts(a.b, Some(i.b), None, st, false)));
                }
            }
            align_at = Some(atoms.len());
            let sym = match kind {
                DefKind::Assign => "≔",
                DefKind::Const => "≡",
            };
            atoms.push(Atom::new(Class::Rel, e.sym_str(sym, st)));
            atoms.push(e.atom(body, st, &sl.next(), Some("value")));
        }
        Statement::Query { expr, mode, .. } => {
            atoms.push(e.atom(expr, st, &sl.next(), Some("expression")));
            align_at = Some(atoms.len());
            let own = match mode {
                QueryMode::Exact => RelOp::Eq,
                QueryMode::Approx => RelOp::Approx,
            };
            match result {
                Some(r) => {
                    atoms.push(e.rel(r.relation.unwrap_or(own), st));
                    atoms.push(result_atom(e, r, index, line, st));
                }
                None => {
                    atoms.push(e.rel(own, st));
                    atoms.push(Atom::ord(e.sym('?', st)));
                }
            }
        }
        Statement::Check { lhs, rhs } => {
            atoms.push(e.atom(lhs, st, &sl.next(), None));
            align_at = Some(atoms.len());
            let q = e.sym('?', st.sup());
            let eq = e.sym('=', st);
            atoms.push(Atom::new(Class::Rel, e.overset(q, eq, st)));
            atoms.push(e.atom(rhs, st, &sl.next(), None));
            if let Some(r) = result {
                atoms.push(Atom::ord(LBox::kern(e.scale(st))));
                atoms.push(result_atom(e, r, index, line, st));
            }
        }
        Statement::Relation(n) => {
            let p = sl.next();
            match &n.kind {
                NodeKind::Relation { first, rest } => {
                    // Spread the chain into the line so it can align on its first relation.
                    atoms.push(e.child(n, first, st, &p));
                    for (op, rhs) in rest {
                        if align_at.is_none() {
                            align_at = Some(atoms.len());
                        }
                        atoms.push(e.rel(*op, st));
                        atoms.push(e.child(n, rhs, st, &p));
                    }
                }
                _ => atoms.push(e.atom(n, st, &p, None)),
            }
            if let Some(r) = result {
                atoms.push(e.rel(r.relation.unwrap_or(RelOp::Eq), st));
                atoms.push(result_atom(e, r, index, line, st));
            }
        }
        Statement::NameList(ns) => {
            for (i, n) in ns.iter().enumerate() {
                if i > 0 {
                    atoms.push(Atom::new(Class::Punct, e.sym(',', st)));
                }
                atoms.push(e.atom(n, st, &sl.next(), None));
            }
        }
        Statement::Range(r) => {
            for (i, n) in r.names.iter().enumerate() {
                if i > 0 {
                    atoms.push(Atom::new(Class::Punct, e.sym(',', st)));
                }
                atoms.push(e.atom(n, st, &sl.next(), None));
            }
            align_at = Some(atoms.len());
            atoms.push(e.rel(RelOp::In, st));
            atoms.push(e.atom(&r.set, st, &sl.next(), Some("set")));
        }
        Statement::Empty => {}
    }
    for c in &line.clauses {
        atoms.push(Atom::new(Class::Punct, e.sym(',', st)));
        atoms.push(Atom::ord(LBox::kern(e.scale(st))));
        match c {
            Clause::Where { name, value, .. } => {
                atoms.push(e.atom(name, st, &sl.next(), None));
                atoms.push(e.rel(RelOp::Eq, st));
                atoms.push(e.atom(value, st, &sl.next(), None));
            }
            Clause::Range(r) => {
                for (i, n) in r.names.iter().enumerate() {
                    if i > 0 {
                        atoms.push(Atom::new(Class::Punct, e.sym(',', st)));
                    }
                    atoms.push(e.atom(n, st, &sl.next(), None));
                }
                atoms.push(e.rel(RelOp::In, st));
                atoms.push(e.atom(&r.set, st, &sl.next(), None));
            }
            Clause::Condition(n) => atoms.push(e.atom(n, st, &sl.next(), None)),
        }
    }
    let (b, _, _, offsets) = e.hlist_offsets(atoms, st);
    let align = align_at.and_then(|i| offsets.get(i).copied());
    LaidLine { b, align }
}

fn result_atom(e: &mut Engine<'_>, r: &LineResult, index: usize, line: &Line, st: Style) -> Atom {
    let node = match &r.value {
        ResultValue::Node(n) => n.clone(),
        ResultValue::Latex(s) => parse_result(s),
    };
    let saved = (e.color, e.hits);
    e.color = match r.kind {
        ResultKind::Filled => e.opts.theme.result,
        ResultKind::Ghost => e.opts.theme.auto_result,
    };
    e.hits = false;
    let mut a = e.atom(&node, st, &NodePath::default(), None);
    (e.color, e.hits) = saved;
    if let Some(hit) = e.hit(
        mpp_syntax::Span::point(line.span.end),
        &NodePath(vec![u16::try_from(index).unwrap_or(u16::MAX)]),
        HitKind::Result,
    ) {
        a.b = a.b.with_hit(hit);
    }
    a
}

/// Stacks lines: aligned on their relations, or each centred.
fn stack_lines(e: &Engine<'_>, lines: Vec<LaidLine>, aligned: bool, st: Style) -> LBox {
    if lines.len() == 1 {
        if let Some(l) = lines.into_iter().next() {
            return l.b;
        }
        return LBox::empty();
    }
    if lines.is_empty() {
        return LBox::empty();
    }
    let s = e.scale(st);
    let use_align = aligned && lines.iter().all(|l| l.align.is_some());
    let left_of_align = lines
        .iter()
        .map(|l| l.align.unwrap_or(0.0))
        .fold(0.0, f32::max);
    let right_of_align = lines
        .iter()
        .map(|l| l.b.width - l.align.unwrap_or(0.0))
        .fold(0.0, f32::max);
    let width = if use_align {
        left_of_align + right_of_align
    } else {
        lines.iter().map(|l| l.b.width).fold(0.0, f32::max)
    };
    let mut children = vec![];
    let mut y = 0.0f32;
    let mut prev_depth = 0.0f32;
    let first_height = lines.first().map(|l| l.b.height).unwrap_or(0.0);
    for (i, l) in lines.into_iter().enumerate() {
        if i > 0 {
            y += (1.5 * s).max(prev_depth + l.b.height + 0.25 * s);
        }
        prev_depth = l.b.depth;
        let x = if use_align {
            left_of_align - l.align.unwrap_or(0.0)
        } else {
            (width - l.b.width) / 2.0
        };
        children.push(Placed { x, y, b: l.b });
    }
    // Put the baseline on the middle line's axis so the block centres.
    let total = first_height + y + prev_depth;
    let mid = -first_height + total / 2.0;
    let shift = -(mid) - e.axis(st);
    for c in &mut children {
        c.y += shift;
    }
    let height = first_height - shift;
    let depth = y + prev_depth + shift;
    LBox::with_children(width, height.max(0.0), depth.max(0.0), children)
}

/// Adds the equation number and centres the block in the line width.
fn with_number(e: &Engine<'_>, body: LBox, centre_number: bool, st: Style) -> LBox {
    let px = e.opts.font_size.max(1.0);
    let line_w = e
        .opts
        .width
        .filter(|w| w.is_finite() && *w > 0.0)
        .map(|w| w / px);
    let number = e
        .opts
        .equation_number
        .as_ref()
        .filter(|_| st.is_display())
        .map(|n| e.text(&format!("({n})"), FontId::Text, Style::TEXT));
    match (line_w, number) {
        (None, None) => body,
        (None, Some(num)) => {
            let gap = 2.0;
            let y = number_y(&body, &num, centre_number);
            let w = body.width + gap + num.width;
            place2(body, 0.0, num, w - 0.0, y, w)
        }
        (Some(w), None) => {
            let w = w.max(body.width);
            let x = (w - body.width) / 2.0;
            let (h, d) = (body.height, body.depth);
            LBox::with_children(w, h, d, vec![Placed { x, y: 0.0, b: body }])
        }
        (Some(w), Some(num)) => {
            let w = w.max(body.width + 2.0 * (num.width + 1.0));
            let x = (w - body.width) / 2.0;
            let y = number_y(&body, &num, centre_number);
            place2(body, x, num, w, y, w)
        }
    }
}

fn number_y(body: &LBox, num: &LBox, centre: bool) -> f32 {
    if centre {
        (body.depth - body.height) / 2.0 + (num.height - num.depth) / 2.0
    } else {
        0.0
    }
}

/// Puts `body` at `bx` and `num` flush right against `right`.
fn place2(body: LBox, bx: f32, num: LBox, right: f32, ny: f32, width: f32) -> LBox {
    let height = body.height.max(num.height - ny);
    let depth = body.depth.max(num.depth + ny);
    let nx = right - num.width;
    LBox::with_children(
        width,
        height,
        depth,
        vec![
            Placed {
                x: bx,
                y: 0.0,
                b: body,
            },
            Placed {
                x: nx,
                y: ny,
                b: num,
            },
        ],
    )
}
