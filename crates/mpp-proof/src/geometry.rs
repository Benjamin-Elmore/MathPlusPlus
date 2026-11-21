//! Coordinate geometry for proof steps: curves, chords, lines and triangle areas.
//!
//! The checker cannot see figures, so a geometric claim is checked only when the text
//! gives what it needs in coordinates:
//!
//! - **A curve** written as `y = f(x)` after a word such as "parabola" or "curve"
//!   ("Take the parabola $y = x^2$").
//! - **A chord** of that curve between two abscissae ("the chord between $x = a$ and
//!   $x = b$"). A claim that "the chord is $y = L(x)$" is proved when `L` is a line
//!   (degree at most 1 in `x`) and both endpoints `(a, f(a))`, `(b, f(b))` lie on it:
//!   two distinct points determine a line.
//! - **A triangle** with its vertices given as points `(p, q)` or as abscissae on the
//!   curve ("vertices on the parabola at $a$, $m$ and $b$"), and an area claim after the
//!   word "area" ("has area $T = \tfrac{h^3}{8}$"). The determinant formula gives the
//!   signed area exactly; the claim is proved when the value is that signed area (or its
//!   negative) and is shown not to be negative from the conditions known (`a < b`), and is
//!   ≈ when that sign is only checked at sample points.
//! - **An area count read off a construction** ("Counting area two ways: …" after a step
//!   proved by construction) cannot be checked without the figure. It is structure (◇)
//!   with that said in the detail, never ✓.
//!
//! Every check is a CAS identity; nothing here is read off a picture.

use std::collections::BTreeSet;

use mpp_core::{Expr, ExprNode};
use mpp_syntax::document::{Inline, InlineKind};
use mpp_syntax::math::{Node, NodeKind, Statement};
use mpp_syntax::Span;

use crate::algebra::{self, Claim, Knowledge};
use crate::cas::{self, Def, LineOut};
use crate::model::ClaimOutcome;
use crate::notation::{self, ClaimOp};

/// Words that name a curve given as `y = f(x)`.
const CURVE_WORDS: &[&str] = &["parabola", "curve", "graph", "cubic", "hyperbola"];

/// A curve `y = f(x)`, as LaTeX.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Curve {
    pub x: String,
    pub y: String,
    pub f: String,
}

/// A chord of the latest curve between `x = from` and `x = to`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Chord {
    pub x: String,
    pub from: String,
    pub to: String,
}

/// What geometry the text has set up so far.
#[derive(Debug, Clone, Default)]
pub(crate) struct Scene {
    pub curves: Vec<Curve>,
    pub chords: Vec<Chord>,
}

/// The lowercase words of `t`.
fn words(t: &str) -> impl Iterator<Item = &str> {
    t.split(|c: char| !c.is_alphabetic())
        .filter(|w| !w.is_empty())
}

/// The single relation in some maths, when that is all it holds.
fn lone_relation(ast: &mpp_syntax::math::MathAst) -> Option<&Node> {
    match ast.lines.as_slice() {
        [line] => match &line.statement {
            Statement::Relation(n) if matches!(n.ungrouped().kind, NodeKind::Relation { .. }) => {
                Some(n)
            }
            _ => None,
        },
        _ => None,
    }
}

/// The single expression in some maths (not a relation), when that is all it holds.
fn lone_expression(ast: &mpp_syntax::math::MathAst) -> Option<&Node> {
    match ast.lines.as_slice() {
        [line] => match &line.statement {
            Statement::Relation(n) if !matches!(n.ungrouped().kind, NodeKind::Relation { .. }) => {
                Some(n)
            }
            _ => None,
        },
        _ => None,
    }
}

/// `name = value` with a lone name on the left: the name and the value's LaTeX.
fn name_equals(node: &Node, src: &str) -> Option<(String, String, Span)> {
    let links = notation::links(node, src);
    let [l] = links.as_slice() else {
        return None;
    };
    if l.op != ClaimOp::Eq || !matches!(l.lhs.ungrouped().kind, NodeKind::Name(_)) {
        return None;
    }
    Some((
        notation::node_text(&l.lhs, src).trim().to_string(),
        notation::side_text(&l.rhs, src),
        l.span,
    ))
}

/// Reads curves and chords from prose ("Take the parabola $y = x^2$ and the chord between
/// $x = a$ and $x = b$").
pub(crate) fn read_scene(inlines: &[Inline], src: &str, scene: &mut Scene) {
    let mut sentence = String::new();
    let mut ends: Vec<(String, String)> = Vec::new();
    for i in inlines {
        match &i.kind {
            InlineKind::Text(t) => {
                let t = t.to_lowercase();
                match t.rfind(['.', ';']) {
                    Some(at) => {
                        sentence = t[at + 1..].to_string();
                        ends.clear();
                    }
                    None => sentence.push_str(&t),
                }
            }
            InlineKind::Math(m) => {
                let Some((name, value, _)) =
                    lone_relation(&m.ast).and_then(|n| name_equals(n, src))
                else {
                    continue;
                };
                let last = words(&sentence).last().unwrap_or("");
                if CURVE_WORDS.contains(&last) {
                    let (vn, _) = mpp_syntax::math::parse_expr(&value, 0);
                    let vars = notation::names(&vn, &value);
                    if let [x] = vars.into_iter().collect::<Vec<_>>().as_slice() {
                        scene.curves.push(Curve {
                            x: x.clone(),
                            y: name,
                            f: value,
                        });
                    }
                    continue;
                }
                let on_curve = scene.curves.last().is_some_and(|c| c.x == name);
                if on_curve && words(&sentence).any(|w| w == "chord") {
                    ends.push((name, value));
                    if let [(x, from), (_, to)] = ends.as_slice() {
                        scene.chords.push(Chord {
                            x: x.clone(),
                            from: from.clone(),
                            to: to.clone(),
                        });
                        ends.clear();
                    }
                }
            }
            _ => {}
        }
    }
}

/// Geometric outcomes for relations in a step's claim, by the span of each relation link.
/// `constructed` is true when an earlier step was proved by construction.
pub(crate) fn step_outcomes(
    claim: &[Inline],
    src: &str,
    scene: &Scene,
    k: &Knowledge,
    constructed: bool,
) -> Vec<(Span, ClaimOutcome)> {
    let mut out = Vec::new();
    let mut sentence = String::new();
    let mut vertices: Vec<Point> = Vec::new();
    let mut after_at = false;
    for i in claim {
        match &i.kind {
            InlineKind::Text(t) => {
                let t = t.to_lowercase();
                match t.rfind(['.', ';']) {
                    Some(at) => {
                        sentence = t[at + 1..].to_string();
                        vertices.clear();
                        after_at = false;
                    }
                    None => sentence.push_str(&t),
                }
                if words(&t).last() == Some("at") {
                    after_at = true;
                }
            }
            InlineKind::Math(m) => {
                let ws: Vec<&str> = words(&sentence).collect();
                let has = |w: &str| ws.contains(&w);
                if has("triangle") && !has("area") {
                    if let Some(p) =
                        lone_expression(&m.ast).and_then(|n| point(n, src, scene, &ws, after_at))
                    {
                        vertices.push(p);
                    }
                    continue;
                }
                let Some(rel) = lone_relation(&m.ast) else {
                    continue;
                };
                if has("triangle") && has("area") {
                    if let (Some((_, value, span)), [a, b, c]) =
                        (name_equals(rel, src), vertices.as_slice())
                    {
                        out.push((span, triangle_area([a, b, c], &value, k)));
                    }
                    continue;
                }
                if has("area")
                    && (has("counting") || ws.windows(2).any(|w| w == ["two", "ways"]))
                    && constructed
                {
                    for l in notation::links(rel, src) {
                        out.push((
                            l.span,
                            ClaimOutcome::Informal(
                                "an area count read off the construction in the steps before; the checker cannot see the figure, so it is taken as structure"
                                    .into(),
                            ),
                        ));
                    }
                    continue;
                }
                if has("chord") || has("line") {
                    if let Some((name, value, span)) = name_equals(rel, src) {
                        if let Some(o) = chord_line(&name, &value, scene, k) {
                            out.push((span, o));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// A vertex written in coordinates: `(p, q)`, or an abscissa on the latest curve when the
/// sentence says the vertices are on it.
#[derive(Debug, Clone)]
struct Point {
    x: String,
    y: String,
}

fn point(node: &Node, src: &str, scene: &Scene, ws: &[&str], after_at: bool) -> Option<Point> {
    if let NodeKind::Tuple { elems, .. } = &node.ungrouped().kind {
        if let [x, y] = elems.as_slice() {
            return Some(Point {
                x: notation::side_text(x, src),
                y: notation::side_text(y, src),
            });
        }
        return None;
    }
    let curve = scene.curves.last()?;
    let on_curve = ws.iter().any(|w| CURVE_WORDS.contains(w)) && ws.contains(&"on");
    if !(on_curve && after_at) {
        return None;
    }
    let x = notation::side_text(node, src);
    Some(Point {
        y: substitute(&curve.f, &curve.x, &x),
        x,
    })
}

/// `f` with every `x` replaced by `(value)`, through the CAS (`None` keeps the text).
fn substitute(f: &str, x: &str, value: &str) -> String {
    let defs = [Def::new(x, value)];
    match cas::run_one(&defs, format!("{} = ?", algebra::wrap(f))) {
        LineOut::Value(v) => v.to_latex(),
        _ => format!("{}|_{{{x} = {value}}}", algebra::wrap(f)),
    }
}

/// The value of some LaTeX after the definitions it needs.
fn value(text: &str, k: &Knowledge) -> Option<Expr> {
    let (node, _) = mpp_syntax::math::parse_expr(text, 0);
    let names = notation::names(&node, text);
    let defs = k.defs_for(&names);
    match cas::run_one(&defs, format!("{} = ?", algebra::wrap(text))) {
        LineOut::Value(v) => Some(v),
        _ => None,
    }
}

fn is_zero(e: &Expr) -> bool {
    mpp_core::expand(e).is_ok_and(|r| r.simplify().is_zero())
}

/// "The chord is $y = L$": both endpoints of the latest chord lie on the line `y = L`.
fn chord_line(name: &str, line: &str, scene: &Scene, k: &Knowledge) -> Option<ClaimOutcome> {
    let curve = scene.curves.last()?;
    let chord = scene.chords.last()?;
    if name != curve.y || chord.x != curve.x {
        return None;
    }
    let x = curve.x.trim_start_matches('\\');
    let l = value(line, k)?;
    let f = value(&curve.f, k)?;
    let (from, to) = (value(&chord.from, k)?, value(&chord.to, k)?);
    let coeffs = mpp_core::as_poly(&mpp_core::expand(&l).ok()?, x)?;
    if coeffs.len() > 2 || coeffs.iter().any(|c| c.contains_sym(x)) {
        return Some(ClaimOutcome::Unverified(format!(
            "${name} = {line}$ is not a line in ${x}$"
        )));
    }
    if is_zero(&(&to - &from)) {
        return Some(ClaimOutcome::Unverified(
            "the chord's two endpoints are the same point".into(),
        ));
    }
    for (e, text) in [(&from, &chord.from), (&to, &chord.to)] {
        if !is_zero(&(&l.subs(x, e) - &f.subs(x, e))) {
            return Some(ClaimOutcome::Unverified(format!(
                "the line does not pass through the chord's endpoint at ${x} = {text}$"
            )));
        }
    }
    Some(ClaimOutcome::ProvedBy(format!(
        "the chord's endpoints at ${x} = {}$ and ${x} = {}$ lie on ${} = {}$ and on this line, and two distinct points determine a line",
        chord.from, chord.to, curve.y, curve.f
    )))
}

/// "The triangle with vertices … has area $T = v$": the determinant formula.
fn triangle_area(points: [&Point; 3], claimed: &str, k: &Knowledge) -> ClaimOutcome {
    let coords: Option<Vec<(Expr, Expr)>> = points
        .iter()
        .map(|p| Some((value(&p.x, k)?, value(&p.y, k)?)))
        .collect();
    let (Some(c), Some(v)) = (coords, value(claimed, k)) else {
        return ClaimOutcome::Unverified("the vertices or the area could not be read".into());
    };
    let [(x1, y1), (x2, y2), (x3, y3)] = c.as_slice() else {
        return ClaimOutcome::Unverified("a triangle needs three vertices".into());
    };
    let det = &(&(x2 - x1) * &(y3 - y1)) - &(&(x3 - x1) * &(y2 - y1));
    let signed = &Expr::rational(1, 2) * &det;
    let matches = is_zero(&(&signed - &v)) || is_zero(&(&signed + &v));
    if !matches {
        return ClaimOutcome::Unverified(format!(
            "the determinant formula gives the signed area ${}$, which is not ±${claimed}$",
            signed.simplify().to_latex()
        ));
    }
    // The area is |signed| = |v|: the claim needs v ≥ 0.
    if let Some(why) = not_negative(&v, k) {
        return ClaimOutcome::ProvedBy(format!(
            "the determinant formula gives the signed area ±${claimed}$, and ${claimed} \\ge 0$ {why}"
        ));
    }
    let (node, _) = mpp_syntax::math::parse_expr(claimed, 0);
    let sign = Claim {
        lhs: claimed.to_string(),
        op: ClaimOp::Ge,
        rhs: "0".into(),
        modulus: None,
        names: notation::names(&node, claimed),
    };
    match algebra::check_claim(&sign, k) {
        ClaimOutcome::Proved | ClaimOutcome::ProvedBy(_) => ClaimOutcome::ProvedBy(format!(
            "the determinant formula gives the signed area ±${claimed}$, which is not negative"
        )),
        ClaimOutcome::Numeric(how) => ClaimOutcome::Numeric(format!(
            "the determinant formula gives the signed area ±${claimed}$ exactly; that it is not negative was checked numerically ({how})"
        )),
        _ => ClaimOutcome::Unverified(format!(
            "the determinant formula gives the signed area ±${claimed}$, but its sign could not be settled"
        )),
    }
}

/// Why `v ≥ 0` follows from the known conditions, when it does: `v` factors into a positive
/// number, even powers, and factors whose sign a condition fixes (`b - a > 0` from
/// `a < b`), with an even number of negative ones.
fn not_negative(v: &Expr, k: &Knowledge) -> Option<String> {
    let factored = mpp_core::factor(v);
    let factors: Vec<Expr> = match factored.node() {
        ExprNode::Mul(fs) => fs.clone(),
        _ => vec![factored.clone()],
    };
    // Known positive differences: `v - u` from `u < v` (or `u ≤ v`, which gives ≥ 0).
    let mut known: Vec<(Expr, String)> = Vec::new();
    for c in &k.conditions {
        let (small, big) = match c.op {
            ClaimOp::Lt | ClaimOp::Le => (&c.lhs, &c.rhs),
            ClaimOp::Gt | ClaimOp::Ge => (&c.rhs, &c.lhs),
            _ => continue,
        };
        if let (Some(s), Some(b)) = (value(small, k), value(big, k)) {
            let op = match c.op {
                ClaimOp::Lt => "<",
                ClaimOp::Le => "\\le",
                ClaimOp::Gt => ">",
                _ => "\\ge",
            };
            known.push((&b - &s, format!("${} {op} {}$", c.lhs, c.rhs)));
        }
    }
    let mut negative = false;
    let mut used: BTreeSet<String> = BTreeSet::new();
    for f in factors {
        let (base, exp) = match f.node() {
            ExprNode::Pow(b, e) => (b.clone(), e.clone()),
            _ => (f.clone(), Expr::one()),
        };
        let even = exp
            .as_number()
            .map(|n| n.to_f64())
            .is_some_and(|e| e.fract() == 0.0 && (e as i64) % 2 == 0);
        if even {
            continue;
        }
        if let Some(n) = base.as_number().map(|n| n.to_f64()) {
            if n < 0.0 {
                negative = !negative;
            }
            continue;
        }
        let sign = known.iter().find_map(|(d, why)| {
            if is_zero(&(&base - d)) {
                Some((false, why))
            } else if is_zero(&(&base + d)) {
                Some((true, why))
            } else {
                None
            }
        });
        match sign {
            Some((neg, why)) => {
                used.insert(why.clone());
                if neg {
                    negative = !negative;
                }
            }
            None => return None,
        }
    }
    if negative {
        return None;
    }
    let why: Vec<String> = used.into_iter().collect();
    Some(if why.is_empty() {
        "(its factors are positive numbers and even powers)".into()
    } else {
        format!("since {}", why.join(" and "))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algebra::Condition;

    fn scene() -> Scene {
        Scene {
            curves: vec![Curve {
                x: "x".into(),
                y: "y".into(),
                f: "x^2".into(),
            }],
            chords: vec![Chord {
                x: "x".into(),
                from: "a".into(),
                to: "b".into(),
            }],
        }
    }

    fn a_below_b() -> Knowledge {
        let mut k = Knowledge::default();
        k.conditions.push(Condition {
            lhs: "a".into(),
            op: ClaimOp::Lt,
            rhs: "b".into(),
            names: ["a", "b"].iter().map(|s| s.to_string()).collect(),
        });
        k.defs.push((
            Def::new("h", "b - a"),
            ["a", "b"].iter().map(|s| s.to_string()).collect(),
        ));
        k
    }

    #[test]
    fn the_chord_of_a_parabola() {
        let k = a_below_b();
        let s = scene();
        assert!(matches!(
            chord_line("y", "(a + b)x - ab", &s, &k),
            Some(ClaimOutcome::ProvedBy(_))
        ));
        assert!(matches!(
            chord_line("y", "(a + b)x + ab", &s, &k),
            Some(ClaimOutcome::Unverified(_))
        ));
        assert!(matches!(
            chord_line("y", "x^2", &s, &k),
            Some(ClaimOutcome::Unverified(_))
        ));
    }

    #[test]
    fn a_triangle_on_the_parabola() {
        let k = a_below_b();
        let s = scene();
        let on = |x: &str| Point {
            x: x.into(),
            y: substitute(&s.curves[0].f, "x", x),
        };
        let (p, q, r) = (on("a"), on("\\tfrac{a+b}{2}"), on("b"));
        assert!(matches!(
            triangle_area([&p, &q, &r], "\\tfrac{h^3}{8}", &k),
            ClaimOutcome::ProvedBy(_)
        ));
        assert!(matches!(
            triangle_area([&p, &q, &r], "\\tfrac{h^3}{6}", &k),
            ClaimOutcome::Unverified(_)
        ));
        // Without `a < b` the sign is not settled by a condition.
        let k2 = Knowledge {
            defs: k.defs.clone(),
            ..Knowledge::default()
        };
        assert!(!matches!(
            triangle_area([&p, &q, &r], "\\tfrac{h^3}{8}", &k2),
            ClaimOutcome::ProvedBy(_)
        ));
    }

    #[test]
    fn a_right_triangle_in_coordinates() {
        let k = Knowledge::default();
        let pt = |x: &str, y: &str| Point {
            x: x.into(),
            y: y.into(),
        };
        let (o, p, q) = (pt("0", "0"), pt("3", "0"), pt("0", "4"));
        assert!(matches!(
            triangle_area([&o, &p, &q], "6", &k),
            ClaimOutcome::ProvedBy(_)
        ));
    }
}
