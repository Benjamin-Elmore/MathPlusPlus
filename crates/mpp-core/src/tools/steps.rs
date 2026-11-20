//! The step tree: the CAS's flat step log arranged the way a textbook lays out working.
//!
//! Each rule the CAS applies records a [`Step`] (rule, before, after, note). A rule that
//! splits a problem into smaller ones leaves unevaluated pieces in its result — the sum
//! rule turns `d/dx(f + g)` into `d/dx f + d/dx g` — and the steps that work those pieces
//! out follow it. [`StepTree::from_steps`] puts each such step under the step that
//! produced its piece, so the IDE can fold a technique (substitution, integration by
//! parts, L'Hôpital's rule) into one line and expand it on click. Every node carries the
//! maths before and after as expressions and LaTeX, and a short sentence saying what was
//! done.

use std::fmt::Write as _;

use crate::expr::{Expr, ExprNode};
use crate::steps::{Step, Steps};

/// One step of working, with the steps that worked out its pieces below it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepNode {
    /// The rule's name, such as "chain rule" or "integration by parts".
    pub rule: String,
    /// A short sentence saying what the step does.
    pub sentence: String,
    /// The maths before the step.
    pub before: Expr,
    /// The maths after the step.
    pub after: Expr,
    /// `before` as LaTeX.
    pub before_latex: String,
    /// `after` as LaTeX.
    pub after_latex: String,
    /// Extra detail from the rule (`u = x^2, du = 2x dx`, a decomposition …).
    pub note: Option<String>,
    /// The sub-steps (folded by default in the preview).
    pub children: Vec<StepNode>,
}

impl StepNode {
    /// A node for `step` without children.
    pub fn new(step: &Step) -> StepNode {
        StepNode {
            rule: step.rule.clone(),
            sentence: sentence(&step.rule, &step.before, step.note.as_deref()),
            before_latex: step.before.to_latex(),
            after_latex: step.after.to_latex(),
            before: step.before.clone(),
            after: step.after.clone(),
            note: step.note.clone(),
            children: Vec::new(),
        }
    }

    /// Number of steps in this node and below it.
    pub fn count(&self) -> usize {
        1 + self.children.iter().map(StepNode::count).sum::<usize>()
    }

    /// What this step finally gives: its own result, or the result of the step that
    /// closes it (substituting back, solving for the integral).
    pub fn result(&self) -> &Expr {
        match self.children.last() {
            Some(last) if CLOSERS.contains(&last.rule.as_str()) => last.result(),
            _ => &self.after,
        }
    }

    /// The step as one line of text: `rule: before = after (note)`.
    pub fn line(&self) -> String {
        let mut out = format!("{}: {} = {}", self.rule, self.before, self.after);
        if let Some(n) = &self.note {
            let _ = write!(out, " ({n})");
        }
        out
    }

    /// The step as one line of LaTeX: `before = after`.
    pub fn latex_line(&self) -> String {
        format!("{} = {}", self.before_latex, self.after_latex)
    }
}

/// The working of one answer as a forest of [`StepNode`]s.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StepTree {
    /// The top-level steps in order.
    pub roots: Vec<StepNode>,
}

impl StepTree {
    /// Arranges a flat step log into a tree (see the module documentation).
    pub fn from_steps(steps: &Steps) -> StepTree {
        let list: Vec<&Step> = steps.iter().collect();
        let mut i = 0;
        let mut roots = Vec::new();
        while i < list.len() {
            roots.push(build(&list, &mut i));
        }
        StepTree { roots }
    }

    /// Like [`StepTree::from_steps`], and makes sure the last top-level step ends at
    /// `answer`: when the working stops at pieces that still need combining, a final
    /// "combine and simplify" step from `question` to `answer` is added.
    pub fn for_answer(steps: &Steps, question: &Expr, answer: &Expr) -> StepTree {
        let mut tree = StepTree::from_steps(steps);
        let done = tree.roots.last().is_some_and(|last| {
            let r = last.result();
            same(r, answer) || ends_in(r, answer) || same(r, &without_order_term(answer))
        });
        if !done && !same(question, answer) {
            let step = Step {
                rule: if tree.roots.is_empty() {
                    "simplify".into()
                } else {
                    "combine the results and simplify".into()
                },
                before: question.clone(),
                after: answer.clone(),
                note: None,
            };
            tree.roots.push(StepNode::new(&step));
        }
        tree
    }

    /// Total number of steps.
    pub fn len(&self) -> usize {
        self.roots.iter().map(StepNode::count).sum()
    }

    /// True when there are no steps.
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    /// Every step depth first, with its depth (0 for a top-level step).
    pub fn walk(&self) -> Vec<(usize, &StepNode)> {
        fn go<'a>(n: &'a StepNode, depth: usize, out: &mut Vec<(usize, &'a StepNode)>) {
            out.push((depth, n));
            for c in &n.children {
                go(c, depth + 1, out);
            }
        }
        let mut out = Vec::new();
        for r in &self.roots {
            go(r, 0, &mut out);
        }
        out
    }

    /// The result of the last top-level step.
    pub fn final_result(&self) -> Option<&Expr> {
        self.roots.last().map(StepNode::result)
    }

    /// The rules used, depth first.
    pub fn rules(&self) -> Vec<&str> {
        self.walk()
            .into_iter()
            .map(|(_, n)| n.rule.as_str())
            .collect()
    }

    /// Indented plain text, one step per line with its sentence.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for (depth, n) in self.walk() {
            let _ = writeln!(out, "{}{} {}", "  ".repeat(depth), n.sentence, n.line());
        }
        out
    }
}

/// Builds the node for `list[*i]` and the steps that work out its pieces.
fn build(list: &[&Step], i: &mut usize) -> StepNode {
    let step = list[*i];
    *i += 1;
    let mut node = StepNode::new(step);
    let mut open = pieces(&step.after, &step.before);
    while *i < list.len() {
        let next = list[*i];
        if let Some(k) = open.iter().position(|p| same(p, &next.before)) {
            open.remove(k);
            node.children.push(build(list, i));
        } else if closes(step, next) {
            node.children.push(build(list, i));
        } else {
            break;
        }
    }
    node
}

/// Rules that finish a technique started by their parent step.
const CLOSERS: &[&str] = &["substitute back", "solve for the integral"];

/// True when `next` finishes the technique `opener` started: substituting back after a
/// substitution, or solving for the integral after integrating by parts twice.
fn closes(opener: &Step, next: &Step) -> bool {
    match next.rule.as_str() {
        "substitute back" => {
            opener.rule.contains("substitution")
                && match (&opener.note, &next.note) {
                    (Some(a), Some(b)) => a.starts_with(b.as_str()),
                    _ => false,
                }
        }
        "solve for the integral" => match next.before.node() {
            ExprNode::Relation(_, lhs, _) => same(lhs, &opener.before),
            _ => false,
        },
        _ => false,
    }
}

/// The unevaluated pieces (derivatives, integrals, limits, sums) left in `after`, other
/// than `before` itself.
fn pieces(after: &Expr, before: &Expr) -> Vec<Expr> {
    fn go(e: &Expr, out: &mut Vec<Expr>) {
        // `∫ (a + b) dx` is worked out term by term, so each term's integral is a piece
        // too (the sum rule left implicit).
        if let ExprNode::Integral {
            expr,
            var,
            bounds: None,
        } = e.node()
        {
            if let ExprNode::Add(terms) = expr.node() {
                for t in terms {
                    out.push(Expr::new(ExprNode::Integral {
                        expr: t.clone(),
                        var: var.clone(),
                        bounds: None,
                    }));
                }
            }
        }
        match e.node() {
            ExprNode::Derivative { .. }
            | ExprNode::Integral { .. }
            | ExprNode::Limit { .. }
            | ExprNode::BigOp { .. } => out.push(e.clone()),
            _ => {
                for c in e.children() {
                    go(c, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    go(after, &mut out);
    out.retain(|p| !same(p, before));
    out
}

/// Same maths, comparing as printed (a step may show a tree the simplifier would
/// rearrange, and the pieces it leaves are matched against the canonical form).
fn same(a: &Expr, b: &Expr) -> bool {
    a == b || a.to_string() == b.to_string()
}

/// A series answer without its `O(…)` term (the steps stop at the terms).
fn without_order_term(e: &Expr) -> Expr {
    match e.node() {
        ExprNode::Add(terms) => {
            let kept: Vec<Expr> = terms
                .iter()
                .filter(|t| !matches!(t.node(), ExprNode::Apply(name, _) if &**name == "O"))
                .cloned()
                .collect();
            crate::simplify::add(kept)
        }
        _ => e.clone(),
    }
}

/// True when `e` is an equation whose right side is `answer` (`I = …`).
fn ends_in(e: &Expr, answer: &Expr) -> bool {
    matches!(e.node(), ExprNode::Relation(_, _, rhs) if same(rhs, answer))
}

/// What the maths before a step is: a derivative, an integral, a limit or other.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Derivative,
    Integral,
    Limit,
    Sum,
    Other,
}

fn kind_of(e: &Expr) -> Kind {
    match e.node() {
        ExprNode::Derivative { .. } => Kind::Derivative,
        ExprNode::Integral { .. } => Kind::Integral,
        ExprNode::Limit { .. } => Kind::Limit,
        ExprNode::BigOp { .. } => Kind::Sum,
        _ => Kind::Other,
    }
}

/// A short sentence for a step: what the rule does, in words a student would use.
pub fn sentence(rule: &str, before: &Expr, note: Option<&str>) -> String {
    let kind = kind_of(before);
    let lower = rule.to_lowercase();
    let fixed: Option<&str> = match (lower.as_str(), kind) {
        ("power rule", Kind::Integral) => {
            Some("Use the power rule for integrals: ∫ xⁿ dx = xⁿ⁺¹/(n + 1) for n ≠ −1.")
        }
        ("power rule", _) => Some("Use the power rule: d/dx xⁿ = n·xⁿ⁻¹."),
        ("constant rule", Kind::Integral) => {
            Some("The integral of a constant c is c times the variable.")
        }
        ("constant rule", _) => Some("The derivative of a constant is 0."),
        ("sum rule", Kind::Integral) => Some("Integrate term by term (the sum rule)."),
        ("sum rule", _) => Some("Differentiate term by term (the sum rule)."),
        ("constant multiple", Kind::Integral) => {
            Some("Take the constant factor outside the integral.")
        }
        ("constant multiple", _) => Some("Take the constant factor outside the derivative."),
        ("product rule", _) => Some("Use the product rule: (uv)′ = u′v + uv′."),
        ("quotient rule", _) => Some("Use the quotient rule: (u/v)′ = (u′v − uv′)/v²."),
        ("chain rule", _) => Some(
            "Use the chain rule: differentiate the outer function at the inner one, then multiply by the derivative of the inner function.",
        ),
        ("logarithmic differentiation", _) => {
            Some("Write the power as an exponential and differentiate its logarithm.")
        }
        ("substitution", _) => {
            Some("Substitute u for the inner function, writing du in terms of dx.")
        }
        ("substitute back", _) => Some("Put the original variable back in place of u."),
        ("integration by parts", _) => Some("Integrate by parts: ∫ u dv = uv − ∫ v du."),
        ("solve for the integral", _) => Some(
            "The original integral came back, so solve the equation for it.",
        ),
        ("partial fractions", _) => {
            Some("Split the rational function into partial fractions.")
        }
        ("trigonometric substitution", _) => {
            Some("Use a trigonometric substitution to remove the square root.")
        }
        ("expand", _) => Some("Multiply out, then integrate term by term."),
        ("simplify the integrand", _) => Some("Simplify the integrand first."),
        ("add the constant of integration", _) => {
            Some("Add the constant of integration.")
        }
        ("fundamental theorem of calculus", _) => Some(
            "Evaluate the antiderivative at the bounds: ∫ₐᵇ f dx = F(b) − F(a).",
        ),
        ("l'hôpital's rule", _) => Some(
            "The limit is indeterminate, so use L'Hôpital's rule: take the limit of the derivatives of the top and bottom.",
        ),
        ("evaluate the new limit", _) => Some("The new limit can be evaluated directly."),
        ("direct substitution", _) => {
            Some("The function is continuous here, so substitute the point.")
        }
        ("both one-sided limits agree", _) => {
            Some("The left and right limits are equal, so the limit exists.")
        }
        ("leading terms", _) => Some("Only the leading terms matter as the variable grows."),
        ("cancel the common factor", _) => {
            Some("Cancel the factor that makes both the top and bottom zero.")
        }
        ("multiply by the conjugate", _) => Some("Multiply the top and bottom by the conjugate."),
        ("combine into one fraction", _) => Some("Combine everything into one fraction."),
        ("rewrite 0·∞ as a quotient", _) => {
            Some("Rewrite the product 0·∞ as a quotient so L'Hôpital's rule applies.")
        }
        ("rewrite as an exponential", _) => {
            Some("Write the power as e to the power of its logarithm.")
        }
        ("series expansion", _) => {
            Some("Replace the top and bottom by the leading terms of their series.")
        }
        ("combine the series", _) => {
            Some("Combine the known series term by term and keep the terms up to the order asked for.")
        }
        ("simplify", _) => Some("Simplify."),
        ("combine the results and simplify", _) => {
            Some("Put the pieces together and simplify.")
        }
        ("substitute the known values", _)
        | ("substitute the known values and simplify", _) => {
            Some("Put in the values of the names that are already defined.")
        }
        _ => None,
    };
    if let Some(s) = fixed {
        return s.to_string();
    }
    let what = match kind {
        Kind::Derivative => " to the derivative",
        Kind::Integral => " to the integral",
        Kind::Limit => " to the limit",
        Kind::Sum => " to the sum",
        Kind::Other => "",
    };
    if lower.starts_with("derivative of ") {
        return format!("Use the {lower}.");
    }
    if lower.starts_with("integral of ")
        || lower.starts_with("series of ")
        || lower.ends_with(" series")
    {
        return format!("Use the known {lower}.");
    }
    if lower.ends_with(" test") {
        return match note {
            Some(n) => format!("Apply the {lower}: {n}."),
            None => format!("Apply the {lower}."),
        };
    }
    let mut chars = rule.chars();
    match chars.next() {
        Some(c) => format!("Apply {}{}{what}.", c.to_lowercase(), chars.as_str())
            .replace("Apply apply", "Apply"),
        None => "Simplify.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calc::{diff, integrate, limit};
    use crate::expr::LimitDir;
    use crate::functions::{func, Func};

    fn x() -> Expr {
        Expr::sym("x")
    }
    fn sin(e: Expr) -> Expr {
        func(Func::Sin, vec![e])
    }
    fn cos(e: Expr) -> Expr {
        func(Func::Cos, vec![e])
    }

    #[test]
    fn derivative_steps_nest_under_their_rule() {
        // d/dx [e^x sin(x² + 1)]
        let f = Expr::e().pow(x()) * sin(x().pow(2) + Expr::one());
        let mut steps = Steps::new();
        let answer = diff(&f, "x", 1, Some(&mut steps)).unwrap();
        let q = crate::calc::derivative_holder(&f, "x");
        let tree = StepTree::for_answer(&steps, &q, &answer);
        assert_eq!(tree.roots[0].rule, "product rule");
        let child_rules: Vec<&str> = tree.roots[0]
            .children
            .iter()
            .map(|c| c.rule.as_str())
            .collect();
        assert!(child_rules.contains(&"chain rule"), "{child_rules:?}");
        // The chain rule's own pieces sit under it.
        let chain = tree.roots[0]
            .children
            .iter()
            .find(|c| c.rule == "chain rule")
            .unwrap();
        assert_eq!(chain.children[0].rule, "sum rule");
        assert_eq!(tree.final_result(), Some(&answer));
        assert_eq!(tree.len(), steps.len() + 1);
        assert!(tree.roots[0].sentence.contains("(uv)′"));
        assert!(tree.roots[0].before_latex.contains("\\frac{d}{d x}"));
    }

    #[test]
    fn substitution_is_folded_with_its_back_substitution() {
        let f = x() * cos(x().pow(2));
        let mut steps = Steps::new();
        let answer = integrate(&f, "x", Some(&mut steps)).unwrap();
        let q = crate::calc::integrate::holder(&f, "x");
        let tree = StepTree::for_answer(&steps, &q, &answer);
        assert_eq!(tree.roots.len(), 1, "{}", tree.to_text());
        let sub = &tree.roots[0];
        assert_eq!(sub.rule, "substitution");
        assert_eq!(sub.note.as_deref(), Some("u = x^2, du = 2*x dx"));
        assert_eq!(
            sub.children.last().map(|c| c.rule.as_str()),
            Some("substitute back")
        );
        assert_eq!(tree.final_result(), Some(&answer));
    }

    #[test]
    fn lhopital_has_its_form_and_derivatives() {
        let mut steps = Steps::new();
        let v = limit(
            &(sin(x()) / x()),
            "x",
            &Expr::zero(),
            LimitDir::Both,
            Some(&mut steps),
        )
        .unwrap();
        assert_eq!(v, Expr::one());
        let tree = StepTree::from_steps(&steps);
        let rules = tree.rules();
        assert!(rules.contains(&"L'Hôpital's rule"), "{rules:?}");
        let lh = tree
            .walk()
            .into_iter()
            .find(|(_, n)| n.rule == "L'Hôpital's rule")
            .map(|(_, n)| n.clone())
            .unwrap();
        let note = lh.note.unwrap_or_default();
        assert!(note.contains("0/0") && note.contains("cos(x)"), "{note}");
        assert_eq!(tree.final_result(), Some(&Expr::one()));
    }

    #[test]
    fn sentences_cover_unknown_rules() {
        assert_eq!(
            sentence("ratio test", &x(), Some("the ratio tends to 1/2 < 1")),
            "Apply the ratio test: the ratio tends to 1/2 < 1."
        );
        assert_eq!(
            sentence("derivative of sin", &x(), None),
            "Use the derivative of sin."
        );
        assert_eq!(sentence("odd symmetry", &x(), None), "Apply odd symmetry.");
        let empty = StepTree::for_answer(&Steps::new(), &x(), &x());
        assert!(empty.is_empty());
        let simplified = StepTree::for_answer(
            &Steps::new(),
            &Expr::new(ExprNode::Add(vec![x(), x()])),
            &(Expr::int(2) * x()),
        );
        assert_eq!(simplified.roots[0].rule, "simplify");
    }
}
