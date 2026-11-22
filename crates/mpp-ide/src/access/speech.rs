//! The spoken form of a formula, read from the maths syntax tree: `x^2 + 5`
//! becomes "x squared plus 5". Anything the speaker has no words for is read
//! out as its source text, so nothing is ever left silent.

use mpp_syntax::math::{
    parse_math, BigOpKind, BinOp, Bracket, Constant, DefHead, Fence, IntKind, MathAst, MulStyle,
    Name, Node, NodeKind, QueryMode, RelOp, Statement, UnaryOp,
};

/// The spoken form of the maths in `src`, which starts at byte `base` of the
/// file (spans in the tree index the file). Lines are joined with full stops.
pub fn speak_math(src: &str, base: usize) -> String {
    let (ast, _) = parse_math(src, base);
    speak_ast(&ast, src, base)
}

/// The spoken form of a parsed block.
pub fn speak_ast(ast: &MathAst, src: &str, base: usize) -> String {
    let s = Speaker { src, base };
    let lines: Vec<String> = ast
        .lines
        .iter()
        .map(|l| s.statement(&l.statement))
        .filter(|t| !t.is_empty())
        .collect();
    lines.join(". ")
}

struct Speaker<'a> {
    src: &'a str,
    base: usize,
}

/// Greek letters and a few symbols, by their name.
fn letter_name(c: char) -> Option<&'static str> {
    Some(match c {
        'α' => "alpha",
        'β' => "beta",
        'γ' => "gamma",
        'δ' => "delta",
        'ε' | 'ϵ' => "epsilon",
        'ζ' => "zeta",
        'η' => "eta",
        'θ' | 'ϑ' => "theta",
        'ι' => "iota",
        'κ' => "kappa",
        'λ' => "lambda",
        'μ' => "mu",
        'ν' => "nu",
        'ξ' => "xi",
        'π' => "pi",
        'ρ' => "rho",
        'σ' => "sigma",
        'τ' => "tau",
        'υ' => "upsilon",
        'φ' | 'ϕ' => "phi",
        'χ' => "chi",
        'ψ' => "psi",
        'ω' => "omega",
        'Γ' => "capital gamma",
        'Δ' => "capital delta",
        'Θ' => "capital theta",
        'Λ' => "capital lambda",
        'Ξ' => "capital xi",
        'Π' => "capital pi",
        'Σ' => "capital sigma",
        'Φ' => "capital phi",
        'Ψ' => "capital psi",
        'Ω' => "capital omega",
        _ => return None,
    })
}

fn relation_words(r: RelOp) -> String {
    match r {
        RelOp::Eq => "equals".into(),
        RelOp::Ne => "is not equal to".into(),
        RelOp::Lt => "is less than".into(),
        RelOp::Gt => "is greater than".into(),
        RelOp::Le => "is less than or equal to".into(),
        RelOp::Ge => "is greater than or equal to".into(),
        RelOp::Approx => "is approximately".into(),
        RelOp::Equiv => "is equivalent to".into(),
        RelOp::In => "is in".into(),
        RelOp::NotIn => "is not in".into(),
        RelOp::Subset => "is a subset of".into(),
        RelOp::SubsetEq => "is a subset of or equal to".into(),
        RelOp::Implies => "implies".into(),
        RelOp::Iff => "if and only if".into(),
        RelOp::To => "goes to".into(),
        other => other.symbol().to_string(),
    }
}

fn function_words(name: &str) -> String {
    match name {
        "sin" => "sine",
        "cos" => "cosine",
        "tan" => "tangent",
        "sec" => "secant",
        "csc" => "cosecant",
        "cot" => "cotangent",
        "sinh" => "hyperbolic sine",
        "cosh" => "hyperbolic cosine",
        "tanh" => "hyperbolic tangent",
        "arcsin" => "arc sine",
        "arccos" => "arc cosine",
        "arctan" => "arc tangent",
        "ln" => "natural log",
        "log" => "log",
        "exp" => "e to the",
        other => other,
    }
    .to_string()
}

impl Speaker<'_> {
    /// The source text of a node, the fallback for what has no words.
    fn source(&self, n: &Node) -> String {
        let (a, b) = (
            n.span.start.saturating_sub(self.base),
            n.span.end.saturating_sub(self.base),
        );
        self.src
            .get(a..b)
            .map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "))
            .unwrap_or_default()
    }

    fn statement(&self, st: &Statement) -> String {
        match st {
            Statement::Definition {
                head, body, query, ..
            } => {
                let head = match head {
                    DefHead::Name(n) | DefHead::Invalid(n) => self.node(n),
                    DefHead::Function { name, params } => {
                        format!("{} of {}", self.node(name), self.list(params))
                    }
                    DefHead::Recurrence { name, index, shift } => format!(
                        "{} sub {} plus {}",
                        self.node(name),
                        self.node(index),
                        self.node(shift)
                    ),
                    DefHead::Initial { name, index } => {
                        format!("{} sub {}", self.node(name), self.node(index))
                    }
                };
                let mut out = format!("{head} is defined as {}", self.node(body));
                match query {
                    Some(QueryMode::Exact) => out.push_str(", equals"),
                    Some(QueryMode::Approx) => out.push_str(", is approximately"),
                    None => {}
                }
                out
            }
            Statement::Query { expr, mode, .. } => {
                let verb = match mode {
                    QueryMode::Exact => "equals",
                    QueryMode::Approx => "is approximately",
                };
                format!("{} {verb}", self.node(expr))
            }
            Statement::Check { lhs, rhs } => {
                format!("{} equals {}", self.node(lhs), self.node(rhs))
            }
            Statement::Relation(n) => self.node(n),
            Statement::NameList(names) => self.list(names),
            Statement::Range(r) => format!("{} in {}", self.list(&r.names), self.node(&r.set)),
            Statement::Empty => String::new(),
        }
    }

    fn list(&self, items: &[Node]) -> String {
        items
            .iter()
            .map(|n| self.node(n))
            .collect::<Vec<_>>()
            .join(" and ")
    }

    fn name(&self, n: &Name) -> String {
        let mut chars = n.base.chars();
        let base = match (chars.next(), chars.next()) {
            (Some(c), None) => letter_name(c).map_or_else(|| n.base.clone(), str::to_string),
            _ => n.base.clone(),
        };
        match &n.sub {
            Some(s) => format!("{base} sub {}", self.node(s)),
            None => base,
        }
    }

    /// True for a node that reads as one word, so it needs no "end" marker.
    fn simple(n: &Node) -> bool {
        matches!(
            n.kind,
            NodeKind::Number(_) | NodeKind::Name(_) | NodeKind::Constant(_)
        )
    }

    /// `n` with "the quantity … end quantity" around it when it is longer than a word.
    fn tight(&self, n: &Node) -> String {
        let t = self.node(n);
        if Self::simple(n) {
            t
        } else {
            format!("the quantity {t} end quantity")
        }
    }

    fn node(&self, n: &Node) -> String {
        match &n.kind {
            NodeKind::Number(t) => t.clone(),
            NodeKind::Name(name) => self.name(name),
            NodeKind::Constant(c) => match c {
                Constant::Pi => "pi",
                Constant::E => "e",
                Constant::I => "i",
                Constant::Infinity => "infinity",
            }
            .to_string(),
            NodeKind::Text(t) => t.clone(),
            NodeKind::Placeholder { label } => format!("empty box, {label}"),
            NodeKind::Error | NodeKind::Continuation => String::new(),
            NodeKind::Binary { op, lhs, rhs } => {
                let word = match op {
                    BinOp::Add => " plus ",
                    BinOp::Sub => " minus ",
                    BinOp::PlusMinus => " plus or minus ",
                    BinOp::MinusPlus => " minus or plus ",
                    BinOp::Mul(MulStyle::Implicit) => " ",
                    BinOp::Mul(_) | BinOp::Times => " times ",
                    BinOp::And => " and ",
                    BinOp::Or => " or ",
                    BinOp::Union => " union ",
                    BinOp::Intersect => " intersect ",
                    BinOp::SetMinus => " minus ",
                    BinOp::Mod => " mod ",
                    _ => " ",
                };
                format!("{}{word}{}", self.node(lhs), self.node(rhs))
            }
            NodeKind::Unary { op, operand } => {
                let word = match op {
                    UnaryOp::Neg => "minus",
                    UnaryOp::Plus => "plus",
                    UnaryOp::PlusMinus => "plus or minus",
                    UnaryOp::MinusPlus => "minus or plus",
                    UnaryOp::Not => "not",
                };
                format!("{word} {}", self.node(operand))
            }
            NodeKind::Relation { first, rest } => {
                let mut out = self.node(first);
                for (op, n) in rest {
                    out.push(' ');
                    out.push_str(&relation_words(*op));
                    out.push(' ');
                    out.push_str(&self.node(n));
                }
                out
            }
            NodeKind::Frac { num, den, .. } => {
                if Self::simple(num) && Self::simple(den) {
                    format!("{} over {}", self.node(num), self.node(den))
                } else {
                    format!(
                        "the fraction {} over {} end fraction",
                        self.node(num),
                        self.node(den)
                    )
                }
            }
            NodeKind::Pow { base, exp } => {
                let b = self.tight(base);
                match &exp.kind {
                    NodeKind::Number(t) if t == "2" => format!("{b} squared"),
                    NodeKind::Number(t) if t == "3" => format!("{b} cubed"),
                    _ if Self::simple(exp) => format!("{b} to the power {}", self.node(exp)),
                    _ => format!("{b} to the power {} end power", self.node(exp)),
                }
            }
            NodeKind::Subscript { base, sub } => {
                format!("{} sub {}", self.node(base), self.node(sub))
            }
            NodeKind::Prime { base, count } => {
                let primes = if *count >= 2 { "double prime" } else { "prime" };
                format!("{} {primes}", self.node(base))
            }
            NodeKind::Factorial { operand, count } => {
                let w = if *count >= 2 {
                    "double factorial"
                } else {
                    "factorial"
                };
                format!("{} {w}", self.tight(operand))
            }
            NodeKind::Fenced { fence, inner, .. } => {
                let (open, close) = match fence {
                    Fence::Abs => ("the absolute value of", "end absolute value"),
                    Fence::Norm => ("the norm of", "end norm"),
                    Fence::Floor => ("the floor of", "end floor"),
                    Fence::Ceil => ("the ceiling of", "end ceiling"),
                };
                format!("{open} {} {close}", self.node(inner))
            }
            NodeKind::Sqrt {
                index, radicand, ..
            } => match index {
                None => format!("the square root of {} end root", self.node(radicand)),
                Some(i) => format!(
                    "the root of index {} of {} end root",
                    self.node(i),
                    self.node(radicand)
                ),
            },
            NodeKind::Group { inner, bracket, .. } => {
                if matches!(bracket, Bracket::TexGroup) {
                    self.node(inner)
                } else {
                    format!("open bracket {} close bracket", self.node(inner))
                }
            }
            NodeKind::Call { callee, args, .. } => {
                format!("{} of {}", self.node(callee), self.list(args))
            }
            NodeKind::FuncApp(f) => {
                let mut name = function_words(f.func);
                if f.inverse {
                    name = format!("inverse {name}");
                }
                if let Some(p) = &f.power {
                    name = format!("{name} to the power {}", self.node(p));
                }
                if f.args.is_empty() {
                    name
                } else {
                    format!("{name} of {}", self.list(&f.args))
                }
            }
            NodeKind::BigOp(b) => {
                let what = match b.op {
                    BigOpKind::Sum => "the sum",
                    BigOpKind::Prod => "the product",
                    BigOpKind::Coprod => "the coproduct",
                };
                let mut out = what.to_string();
                if let Some(i) = &b.index {
                    out.push_str(&format!(" over {}", self.node(i)));
                }
                if let Some(f) = &b.from {
                    out.push_str(&format!(" from {}", self.node(f)));
                }
                if let Some(t) = &b.to {
                    out.push_str(&format!(" to {}", self.node(t)));
                }
                format!("{out} of {}", self.node(&b.body))
            }
            NodeKind::Integral(i) => {
                let what = match i.kind {
                    IntKind::Single => "the integral",
                    IntKind::Double => "the double integral",
                    IntKind::Triple => "the triple integral",
                    IntKind::Contour => "the contour integral",
                    IntKind::Surface => "the surface integral",
                };
                let mut out = what.to_string();
                if let (Some(lo), Some(hi)) = (&i.lower, &i.upper) {
                    out.push_str(&format!(" from {} to {}", self.node(lo), self.node(hi)));
                }
                out.push_str(&format!(" of {}", self.node(&i.body)));
                for d in &i.diffs {
                    out.push_str(&format!(" d {}", self.node(d)));
                }
                out
            }
            NodeKind::Limit(l) => format!(
                "the limit as {} approaches {} of {}",
                self.node(&l.var),
                self.node(&l.target),
                self.node(&l.body)
            ),
            NodeKind::Matrix { rows, .. } => {
                let cols = rows.first().map_or(0, Vec::len);
                let mut out = format!("matrix with {} rows and {cols} columns", rows.len());
                for (i, r) in rows.iter().enumerate() {
                    out.push_str(&format!(", row {}: {}", i + 1, self.list(r)));
                }
                out
            }
            NodeKind::Tuple { elems, .. } => format!("the list {}", self.list(elems)),
            NodeKind::Set { elems, .. } => format!("the set {}", self.list(elems)),
            NodeKind::Interval { lo, hi, .. } => {
                format!("the interval from {} to {}", self.node(lo), self.node(hi))
            }
            NodeKind::Quantity { value, unit } => {
                format!("{} {}", self.node(value), unit.text)
            }
            _ => self.source(n),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(src: &str) -> String {
        speak_math(src, 0)
    }

    #[test]
    fn simple_formulas_read_naturally() {
        assert_eq!(said("x^2 + 5"), "x squared plus 5");
        assert_eq!(said("x := 5"), "x is defined as 5");
        assert_eq!(
            said(r"\frac{1}{2} + \frac{1}{3} = ?"),
            "1 over 2 plus 1 over 3 equals"
        );
        assert_eq!(
            said(r"\sqrt{2} \approx ?"),
            "the square root of 2 end root is approximately"
        );
    }

    #[test]
    fn functions_powers_and_greek() {
        assert_eq!(
            said(r"\sin(x)^3"),
            "the quantity sine of x end quantity cubed"
        );
        assert_eq!(said(r"\alpha + \beta"), "alpha plus beta");
        assert_eq!(said("a^n"), "a to the power n");
    }

    #[test]
    fn integrals_and_sums() {
        let t = said(r"\int_0^1 x^2 \, dx = ?");
        assert!(
            t.starts_with("the integral from 0 to 1 of x squared"),
            "{t}"
        );
        let t = said(r"\sum_{i=1}^{10} i");
        assert!(t.starts_with("the sum"), "{t}");
    }

    #[test]
    fn nothing_is_silent() {
        for src in [
            "1.5 + 2 =",
            r"\begin{pmatrix} 1 & 2 \\ 3 & 4 \end{pmatrix}",
            "|x|",
        ] {
            assert!(!said(src).is_empty(), "{src}");
        }
    }
}
