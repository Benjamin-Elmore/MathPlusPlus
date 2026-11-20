//! Printing: readable plain maths (`Display`) and LaTeX (`to_latex`).

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};

use crate::expr::{BigOpKind, Constant, Expr, ExprNode, LimitDir, LogicOp, Name, RelOp};
use crate::functions::Func;
use crate::number::{format_f64, Number};
use crate::simplify::{split_coeff, split_pow, with_coeff};

/// Binding strength, lowest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Prec {
    Rel,
    Or,
    And,
    Add,
    Neg,
    Mul,
    Pow,
    Atom,
}

/// Degrees of a term in each symbol it is a power of.
fn degrees(term: &Expr) -> (BigRational, BTreeMap<Name, BigRational>) {
    let (_, rest) = split_coeff(term);
    let factors = match rest.node() {
        ExprNode::Mul(fs) => fs.clone(),
        _ => vec![rest],
    };
    let mut total = BigRational::zero();
    let mut map = BTreeMap::new();
    for f in factors {
        let (b, x) = split_pow(&f);
        if let (ExprNode::Sym(name), Some(r)) =
            (b.node(), x.as_number().and_then(Number::as_rational))
        {
            total += r;
            *map.entry(name.clone()).or_insert_with(BigRational::zero) += r;
        }
    }
    (total, map)
}

fn term_order(a: &Expr, b: &Expr) -> Ordering {
    let (an, bn) = (a.is_number(), b.is_number());
    if an != bn {
        return an.cmp(&bn);
    }
    let (ta, ma) = degrees(a);
    let (tb, mb) = degrees(b);
    tb.cmp(&ta)
        .then_with(|| {
            let names: std::collections::BTreeSet<&Name> = ma.keys().chain(mb.keys()).collect();
            let zero = BigRational::zero();
            for n in names {
                let da = ma.get(n).unwrap_or(&zero);
                let db = mb.get(n).unwrap_or(&zero);
                match db.cmp(da) {
                    Ordering::Equal => {}
                    other => return other,
                }
            }
            Ordering::Equal
        })
        .then_with(|| split_coeff(a).1.cmp(&split_coeff(b).1))
        .then_with(|| a.cmp(b))
}

/// Terms of a sum in textbook order: descending degree, constants last.
pub fn display_order(terms: &[Expr]) -> Vec<Expr> {
    let mut v = terms.to_vec();
    v.sort_by(term_order);
    v
}

/// Order of the factors of a product for display: constants such as π first.
fn factor_order(a: &Expr, b: &Expr) -> Ordering {
    let rank = |e: &Expr| match split_pow(e).0.node() {
        ExprNode::Const(_) => 0,
        ExprNode::Sym(_) => 1,
        ExprNode::Pow(..) | ExprNode::Func(..) | ExprNode::Apply(..) => 2,
        ExprNode::Add(_) => 3,
        _ => 2,
    };
    rank(a)
        .cmp(&rank(b))
        .then_with(|| split_pow(a).0.cmp(&split_pow(b).0))
        .then_with(|| a.cmp(b))
}

fn is_negative_term(t: &Expr) -> bool {
    split_coeff(t).0.is_negative()
}

fn negate_term(t: &Expr) -> Expr {
    let (c, rest) = split_coeff(t);
    with_coeff(c.neg(), rest)
}

/// Splits a product into its coefficient, numerator factors and denominator factors.
fn fraction_parts(e: &Expr) -> (Number, Vec<Expr>, Vec<Expr>) {
    let (c, rest) = split_coeff(e);
    let factors = match rest.node() {
        ExprNode::Mul(fs) => fs.clone(),
        _ if rest.is_one() => Vec::new(),
        _ => vec![rest],
    };
    let mut num = Vec::new();
    let mut den = Vec::new();
    for f in factors {
        let (b, x) = split_pow(&f);
        match x.as_number() {
            Some(n) if n.is_negative() && matches!(f.node(), ExprNode::Pow(..)) => {
                let pos = n.neg();
                den.push(if pos.is_one() {
                    b
                } else {
                    Expr::new(ExprNode::Pow(b, Expr::num(pos)))
                });
            }
            _ => num.push(f),
        }
    }
    num.sort_by(factor_order);
    den.sort_by(factor_order);
    (c, num, den)
}

/// Shared shape of a product: sign, numeric numerator/denominator and the factors.
struct Product {
    negative: bool,
    num_coeff: Option<Number>,
    den_coeff: Option<BigInt>,
    num: Vec<Expr>,
    den: Vec<Expr>,
}

fn product_shape(e: &Expr) -> Product {
    let (c, num, den) = fraction_parts(e);
    let mut negative = false;
    let (num_coeff, den_coeff) = match &c {
        Number::Rational(r) => {
            negative = r.is_negative();
            let p = r.numer().abs();
            let q = r.denom().clone();
            (
                (!p.is_one()).then(|| Number::from_bigint(p)),
                (!q.is_one()).then_some(q),
            )
        }
        Number::Float(x) => {
            negative = *x < 0.0;
            let a = x.abs();
            ((a != 1.0).then(|| Number::float(a)), None)
        }
        other => (Some(other.clone()), None),
    };
    Product {
        negative,
        num_coeff,
        den_coeff,
        num,
        den,
    }
}

fn paren(s: String, prec: Prec, min: Prec) -> String {
    if prec < min {
        format!("({s})")
    } else {
        s
    }
}

fn rel_symbol(op: RelOp) -> &'static str {
    match op {
        RelOp::Eq => "=",
        RelOp::Ne => "≠",
        RelOp::Lt => "<",
        RelOp::Le => "≤",
        RelOp::Gt => ">",
        RelOp::Ge => "≥",
        RelOp::Approx => "≈",
    }
}

fn plain_number(n: &Number) -> (String, Prec) {
    let s = n.to_string();
    let prec = match n {
        Number::Rational(r) if r.is_negative() => Prec::Neg,
        Number::Rational(r) if !r.is_integer() => Prec::Mul,
        Number::Float(x) if *x < 0.0 => Prec::Neg,
        Number::Complex(re, _) if !re.is_zero() => Prec::Add,
        Number::ComplexFloat(re, _) if *re != 0.0 => Prec::Add,
        Number::Complex(..) | Number::ComplexFloat(..) => Prec::Mul,
        _ => Prec::Atom,
    };
    (s, prec)
}

fn plain(e: &Expr) -> (String, Prec) {
    match e.node() {
        ExprNode::Num(n) => plain_number(n),
        ExprNode::Sym(s) => (s.to_string(), Prec::Atom),
        ExprNode::Const(c) => (
            match c {
                Constant::Pi => "π",
                Constant::E => "e",
                Constant::I => "i",
                Constant::Infinity => "∞",
                Constant::Undefined => "undefined",
            }
            .into(),
            Prec::Atom,
        ),
        ExprNode::Add(terms) => {
            let mut out = String::new();
            for (k, t) in display_order(terms).iter().enumerate() {
                if k == 0 {
                    let (s, p) = plain(t);
                    out.push_str(&paren(s, p, Prec::Neg));
                } else if is_negative_term(t) {
                    let (s, p) = plain(&negate_term(t));
                    out.push_str(" - ");
                    out.push_str(&paren(s, p, Prec::Mul));
                } else {
                    let (s, p) = plain(t);
                    out.push_str(" + ");
                    out.push_str(&paren(s, p, Prec::Neg));
                }
            }
            (out, Prec::Add)
        }
        ExprNode::Mul(_) => plain_product(e),
        ExprNode::Pow(b, x) => {
            if x.as_number().is_some_and(Number::is_negative) {
                return plain_product(e);
            }
            if x.as_number() == Some(&Number::Rational(BigRational::new(1.into(), 2.into()))) {
                return (format!("sqrt({})", plain(b).0), Prec::Atom);
            }
            let (bs, bp) = plain(b);
            let (xs, xp) = plain(x);
            let base = paren(bs, bp, Prec::Atom);
            let exp = if xp == Prec::Atom {
                xs
            } else {
                format!("({xs})")
            };
            (format!("{base}^{exp}"), Prec::Pow)
        }
        ExprNode::Func(f, args) => plain_func(*f, args),
        ExprNode::Apply(name, args) => (format!("{name}({})", plain_list(args)), Prec::Atom),
        ExprNode::Relation(op, a, b) => {
            let (sa, pa) = plain(a);
            let (sb, pb) = plain(b);
            (
                format!(
                    "{} {} {}",
                    paren(sa, pa, Prec::Or),
                    rel_symbol(*op),
                    paren(sb, pb, Prec::Or)
                ),
                Prec::Rel,
            )
        }
        ExprNode::Bool(b) => (b.to_string(), Prec::Atom),
        ExprNode::Logic(op, args) => match op {
            LogicOp::Not => {
                let (s, p) = args.first().map(plain).unwrap_or_default_atom();
                (format!("¬{}", paren(s, p, Prec::Atom)), Prec::Neg)
            }
            LogicOp::And | LogicOp::Or => {
                let (sym, prec) = if *op == LogicOp::And {
                    (" ∧ ", Prec::And)
                } else {
                    (" ∨ ", Prec::Or)
                };
                let parts: Vec<String> = args
                    .iter()
                    .map(|a| {
                        let (s, p) = plain(a);
                        paren(
                            s,
                            p,
                            if prec == Prec::And {
                                Prec::Add
                            } else {
                                Prec::And
                            },
                        )
                    })
                    .collect();
                (parts.join(sym), prec)
            }
        },
        ExprNode::Derivative { expr, vars } => {
            let total: u32 = vars.iter().map(|(_, k)| k).sum();
            let head = if vars.len() == 1 && total == 1 {
                format!("d/d{}", vars[0].0)
            } else if vars.len() == 1 {
                format!("d^{total}/d{}^{total}", vars[0].0)
            } else {
                let dens: String = vars
                    .iter()
                    .map(|(v, k)| {
                        if *k == 1 {
                            format!("∂{v}")
                        } else {
                            format!("∂{v}^{k}")
                        }
                    })
                    .collect();
                format!("∂^{total}/{dens}")
            };
            (format!("{head}({})", plain(expr).0), Prec::Atom)
        }
        ExprNode::Integral { expr, var, bounds } => {
            let (s, p) = plain(expr);
            let body = paren(s, p, Prec::Mul);
            let head = match bounds {
                Some((a, b)) => format!("∫_{}^{}", script(a), script(b)),
                None => "∫".into(),
            };
            (format!("{head} {body} d{var}"), Prec::Add)
        }
        ExprNode::BigOp {
            kind,
            expr,
            var,
            lower,
            upper,
        } => {
            let sym = if *kind == BigOpKind::Sum {
                "∑"
            } else {
                "∏"
            };
            let (s, p) = plain(expr);
            (
                format!(
                    "{sym}_({var}={})^{} {}",
                    plain(lower).0,
                    script(upper),
                    paren(s, p, Prec::Pow)
                ),
                Prec::Add,
            )
        }
        ExprNode::Limit {
            expr,
            var,
            point,
            dir,
        } => {
            let side = match dir {
                LimitDir::Both => "",
                LimitDir::Left => "^-",
                LimitDir::Right => "^+",
            };
            let (s, p) = plain(expr);
            (
                format!(
                    "lim_({var}→{}{side}) {}",
                    plain(point).0,
                    paren(s, p, Prec::Pow)
                ),
                Prec::Add,
            )
        }
        ExprNode::Matrix { cols, data, .. } => {
            let rows: Vec<String> = data
                .chunks((*cols).max(1))
                .map(|r| format!("[{}]", plain_list(r)))
                .collect();
            (format!("[{}]", rows.join(", ")), Prec::Atom)
        }
        ExprNode::Piecewise { cases, otherwise } => {
            let mut parts: Vec<String> = cases
                .iter()
                .map(|(v, c)| format!("{} if {}", plain(v).0, plain(c).0))
                .collect();
            if let Some(o) = otherwise {
                parts.push(format!("{} otherwise", plain(o).0));
            }
            (format!("{{ {} }}", parts.join("; ")), Prec::Atom)
        }
        ExprNode::Set(v) => {
            if v.is_empty() {
                ("∅".into(), Prec::Atom)
            } else {
                (format!("{{{}}}", plain_list(v)), Prec::Atom)
            }
        }
        ExprNode::Interval {
            lo,
            hi,
            lo_closed,
            hi_closed,
        } => (
            format!(
                "{}{}, {}{}",
                if *lo_closed { "[" } else { "(" },
                plain(lo).0,
                plain(hi).0,
                if *hi_closed { "]" } else { ")" }
            ),
            Prec::Atom,
        ),
        ExprNode::Tuple(v) => (format!("({})", plain_list(v)), Prec::Atom),
    }
}

trait OrAtom {
    fn unwrap_or_default_atom(self) -> (String, Prec);
}

impl OrAtom for Option<(String, Prec)> {
    fn unwrap_or_default_atom(self) -> (String, Prec) {
        self.unwrap_or_else(|| (String::new(), Prec::Atom))
    }
}

fn script(e: &Expr) -> String {
    let (s, p) = plain(e);
    if p == Prec::Atom {
        s
    } else {
        format!("({s})")
    }
}

fn plain_list(v: &[Expr]) -> String {
    v.iter().map(|a| plain(a).0).collect::<Vec<_>>().join(", ")
}

fn plain_product(e: &Expr) -> (String, Prec) {
    let p = product_shape(e);
    let mut num: Vec<String> = Vec::new();
    if let Some(c) = &p.num_coeff {
        let (s, pr) = plain_number(c);
        num.push(paren(s, pr, Prec::Mul));
    }
    for f in &p.num {
        let (s, pr) = plain(f);
        num.push(paren(s, pr, Prec::Mul));
    }
    let mut den: Vec<String> = Vec::new();
    if let Some(q) = &p.den_coeff {
        den.push(q.to_string());
    }
    // A lone denominator that is itself a product (in a tree built for the steps)
    // needs brackets: `a/(2*x)`, not `a/2*x`.
    let lone = p.den.len() == 1 && p.den_coeff.is_none();
    for f in &p.den {
        let (s, pr) = plain(f);
        den.push(paren(s, pr, if lone { Prec::Pow } else { Prec::Mul }));
    }
    let mut out = String::new();
    if p.negative {
        out.push('-');
    }
    if num.is_empty() {
        out.push('1');
    } else {
        out.push_str(&num.join("*"));
    }
    if !den.is_empty() {
        out.push('/');
        if den.len() == 1 {
            out.push_str(&den[0]);
        } else {
            out.push('(');
            out.push_str(&den.join("*"));
            out.push(')');
        }
    }
    (
        out,
        if p.negative {
            Prec::Neg
        } else if den.is_empty() && num.len() == 1 {
            plain(&p.num.first().cloned().unwrap_or_else(Expr::one))
                .1
                .min(Prec::Atom)
        } else {
            Prec::Mul
        },
    )
}

fn plain_func(f: Func, args: &[Expr]) -> (String, Prec) {
    match f {
        Func::Abs => (format!("|{}|", plain_list(args)), Prec::Atom),
        Func::Factorial => {
            let (s, p) = args.first().map(plain).unwrap_or_default_atom();
            (format!("{}!", paren(s, p, Prec::Atom)), Prec::Pow)
        }
        Func::Log => {
            let x = args.first().map(|a| plain(a).0).unwrap_or_default();
            let b = args.get(1).map(script).unwrap_or_default();
            (format!("log_{b}({x})"), Prec::Atom)
        }
        Func::Gamma => (format!("Γ({})", plain_list(args)), Prec::Atom),
        _ => (format!("{}({})", f.name(), plain_list(args)), Prec::Atom),
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&plain(self).0)
    }
}

// ---------------------------------------------------------------- LaTeX

const GREEK: [&str; 40] = [
    "alpha",
    "beta",
    "gamma",
    "delta",
    "epsilon",
    "varepsilon",
    "zeta",
    "eta",
    "theta",
    "vartheta",
    "iota",
    "kappa",
    "lambda",
    "mu",
    "nu",
    "xi",
    "rho",
    "varrho",
    "sigma",
    "tau",
    "upsilon",
    "phi",
    "varphi",
    "chi",
    "psi",
    "omega",
    "Gamma",
    "Delta",
    "Theta",
    "Lambda",
    "Xi",
    "Pi",
    "Sigma",
    "Upsilon",
    "Phi",
    "Psi",
    "Omega",
    "ell",
    "hbar",
    "nabla",
];

const UNICODE_GREEK: [(char, &str); 24] = [
    ('α', "alpha"),
    ('β', "beta"),
    ('γ', "gamma"),
    ('δ', "delta"),
    ('ε', "epsilon"),
    ('ζ', "zeta"),
    ('η', "eta"),
    ('θ', "theta"),
    ('κ', "kappa"),
    ('λ', "lambda"),
    ('μ', "mu"),
    ('ν', "nu"),
    ('ξ', "xi"),
    ('ρ', "rho"),
    ('σ', "sigma"),
    ('τ', "tau"),
    ('φ', "phi"),
    ('χ', "chi"),
    ('ψ', "psi"),
    ('ω', "omega"),
    ('Γ', "Gamma"),
    ('Δ', "Delta"),
    ('Σ', "Sigma"),
    ('Ω', "Omega"),
];

fn latex_name(name: &str) -> String {
    let (head, sub) = match name.split_once('_') {
        Some((h, s)) => (h, Some(s)),
        None => (name, None),
    };
    let mut chars = head.chars();
    let head_tex = match (chars.next(), chars.next()) {
        (Some(c), None) => match UNICODE_GREEK.iter().find(|(u, _)| *u == c) {
            Some((_, g)) => format!("\\{g}"),
            None => c.to_string(),
        },
        _ if GREEK.contains(&head) => format!("\\{head}"),
        _ => format!("\\mathrm{{{head}}}"),
    };
    match sub {
        Some(s) => format!("{head_tex}_{{{}}}", latex_name_plain(s)),
        None => head_tex,
    }
}

fn latex_name_plain(s: &str) -> String {
    if s.chars().all(|c| c.is_ascii_alphanumeric()) && !GREEK.contains(&s) {
        s.into()
    } else {
        latex_name(s)
    }
}

fn latex_number(n: &Number) -> (String, Prec) {
    match n {
        Number::Rational(r) => {
            if r.is_integer() {
                let s = r.numer().to_string();
                let p = if r.is_negative() {
                    Prec::Neg
                } else {
                    Prec::Atom
                };
                (s, p)
            } else {
                let sign = if r.is_negative() { "-" } else { "" };
                (
                    format!("{sign}\\frac{{{}}}{{{}}}", r.numer().abs(), r.denom()),
                    if r.is_negative() {
                        Prec::Neg
                    } else {
                        Prec::Mul
                    },
                )
            }
        }
        Number::Float(x) => {
            let s = format_f64(*x, 10);
            let s = match s.split_once('e') {
                Some((m, e)) => format!("{m} \\times 10^{{{e}}}"),
                None => s,
            };
            (s, if *x < 0.0 { Prec::Neg } else { Prec::Atom })
        }
        Number::Complex(..) | Number::ComplexFloat(..) => {
            let re = n.re();
            let im = n.im();
            let im_neg = im.is_negative();
            let mag = im.abs_real().unwrap_or(im);
            let im_text = if mag.is_one() {
                "i".to_string()
            } else {
                format!("{} i", latex_number(&mag).0)
            };
            if re.is_zero() {
                (
                    format!("{}{im_text}", if im_neg { "-" } else { "" }),
                    if im_neg { Prec::Neg } else { Prec::Mul },
                )
            } else {
                (
                    format!(
                        "{} {} {im_text}",
                        latex_number(&re).0,
                        if im_neg { "-" } else { "+" }
                    ),
                    Prec::Add,
                )
            }
        }
    }
}

fn lparen(s: String, prec: Prec, min: Prec) -> String {
    if prec < min {
        format!("\\left({s}\\right)")
    } else {
        s
    }
}

fn latex(e: &Expr) -> (String, Prec) {
    match e.node() {
        ExprNode::Num(n) => latex_number(n),
        ExprNode::Sym(s) => (latex_name(s), Prec::Atom),
        ExprNode::Const(c) => (
            match c {
                Constant::Pi => "\\pi",
                Constant::E => "e",
                Constant::I => "i",
                Constant::Infinity => "\\infty",
                Constant::Undefined => "\\text{undefined}",
            }
            .into(),
            Prec::Atom,
        ),
        ExprNode::Add(terms) => {
            let mut out = String::new();
            for (k, t) in display_order(terms).iter().enumerate() {
                if k == 0 {
                    let (s, p) = latex(t);
                    out.push_str(&lparen(s, p, Prec::Neg));
                } else if is_negative_term(t) {
                    let (s, p) = latex(&negate_term(t));
                    out.push_str(" - ");
                    out.push_str(&lparen(s, p, Prec::Mul));
                } else {
                    let (s, p) = latex(t);
                    out.push_str(" + ");
                    out.push_str(&lparen(s, p, Prec::Neg));
                }
            }
            (out, Prec::Add)
        }
        ExprNode::Mul(_) => latex_product(e),
        ExprNode::Pow(b, x) => {
            if x.as_number().is_some_and(Number::is_negative) {
                return latex_product(e);
            }
            if let Some(r) = x.as_number().and_then(Number::as_rational) {
                if r.numer().is_one() && !r.is_integer() {
                    let inner = latex(b).0;
                    return if r.denom() == &BigInt::from(2) {
                        (format!("\\sqrt{{{inner}}}"), Prec::Atom)
                    } else {
                        (format!("\\sqrt[{}]{{{inner}}}", r.denom()), Prec::Atom)
                    };
                }
            }
            // sin^2(x) style for trig and hyperbolic powers.
            if let (ExprNode::Func(f, args), Some(k)) =
                (b.node(), x.as_number().and_then(Number::as_i64))
            {
                if k > 0 && latex_func_name(*f).is_some_and(|n| n.starts_with('\\')) {
                    if let Some(name) = latex_func_name(*f) {
                        return (
                            format!("{name}^{{{k}}}\\left({}\\right)", latex_list(args)),
                            Prec::Atom,
                        );
                    }
                }
            }
            let (bs, bp) = latex(b);
            let base = lparen(bs, bp, Prec::Atom);
            (format!("{base}^{{{}}}", latex(x).0), Prec::Pow)
        }
        ExprNode::Func(f, args) => latex_func(*f, args),
        ExprNode::Apply(name, args) => (
            format!("{}\\left({}\\right)", latex_name(name), latex_list(args)),
            Prec::Atom,
        ),
        ExprNode::Relation(op, a, b) => {
            let sym = match op {
                RelOp::Eq => "=",
                RelOp::Ne => "\\neq",
                RelOp::Lt => "<",
                RelOp::Le => "\\le",
                RelOp::Gt => ">",
                RelOp::Ge => "\\ge",
                RelOp::Approx => "\\approx",
            };
            (format!("{} {sym} {}", latex(a).0, latex(b).0), Prec::Rel)
        }
        ExprNode::Bool(b) => (format!("\\text{{{b}}}"), Prec::Atom),
        ExprNode::Logic(op, args) => match op {
            LogicOp::Not => {
                let (s, p) = args.first().map(latex).unwrap_or_default_atom();
                (format!("\\neg {}", lparen(s, p, Prec::Atom)), Prec::Neg)
            }
            LogicOp::And | LogicOp::Or => {
                let sym = if *op == LogicOp::And {
                    " \\land "
                } else {
                    " \\lor "
                };
                let parts: Vec<String> = args
                    .iter()
                    .map(|a| {
                        let (s, p) = latex(a);
                        lparen(s, p, Prec::Add)
                    })
                    .collect();
                (
                    parts.join(sym),
                    if *op == LogicOp::And {
                        Prec::And
                    } else {
                        Prec::Or
                    },
                )
            }
        },
        ExprNode::Derivative { expr, vars } => {
            let total: u32 = vars.iter().map(|(_, k)| k).sum();
            let partial = vars.len() > 1;
            let d = if partial { "\\partial" } else { "d" };
            let top = if total == 1 {
                d.to_string()
            } else {
                format!("{d}^{{{total}}}")
            };
            let bottom: Vec<String> = vars
                .iter()
                .map(|(v, k)| {
                    if *k == 1 {
                        format!("{d} {}", latex_name(v))
                    } else {
                        format!("{d} {}^{{{k}}}", latex_name(v))
                    }
                })
                .collect();
            (
                format!(
                    "\\frac{{{top}}}{{{}}}\\left({}\\right)",
                    bottom.join(" \\, "),
                    latex(expr).0
                ),
                Prec::Atom,
            )
        }
        ExprNode::Integral { expr, var, bounds } => {
            let head = match bounds {
                Some((a, b)) => format!("\\int_{{{}}}^{{{}}}", latex(a).0, latex(b).0),
                None => "\\int".into(),
            };
            let (s, p) = latex(expr);
            (
                format!(
                    "{head} {} \\, d{}",
                    lparen(s, p, Prec::Mul),
                    latex_name(var)
                ),
                Prec::Add,
            )
        }
        ExprNode::BigOp {
            kind,
            expr,
            var,
            lower,
            upper,
        } => {
            let sym = if *kind == BigOpKind::Sum {
                "\\sum"
            } else {
                "\\prod"
            };
            let (s, p) = latex(expr);
            (
                format!(
                    "{sym}_{{{}={}}}^{{{}}} {}",
                    latex_name(var),
                    latex(lower).0,
                    latex(upper).0,
                    lparen(s, p, Prec::Pow)
                ),
                Prec::Add,
            )
        }
        ExprNode::Limit {
            expr,
            var,
            point,
            dir,
        } => {
            let side = match dir {
                LimitDir::Both => "",
                LimitDir::Left => "^{-}",
                LimitDir::Right => "^{+}",
            };
            let (s, p) = latex(expr);
            (
                format!(
                    "\\lim_{{{} \\to {}{side}}} {}",
                    latex_name(var),
                    latex(point).0,
                    lparen(s, p, Prec::Pow)
                ),
                Prec::Add,
            )
        }
        ExprNode::Matrix { cols, data, .. } => {
            let rows: Vec<String> = data
                .chunks((*cols).max(1))
                .map(|r| r.iter().map(|x| latex(x).0).collect::<Vec<_>>().join(" & "))
                .collect();
            (
                format!(
                    "\\begin{{pmatrix}} {} \\end{{pmatrix}}",
                    rows.join(" \\\\ ")
                ),
                Prec::Atom,
            )
        }
        ExprNode::Piecewise { cases, otherwise } => {
            let mut rows: Vec<String> = cases
                .iter()
                .map(|(v, c)| format!("{} & {}", latex(v).0, latex(c).0))
                .collect();
            if let Some(o) = otherwise {
                rows.push(format!("{} & \\text{{otherwise}}", latex(o).0));
            }
            (
                format!("\\begin{{cases}} {} \\end{{cases}}", rows.join(" \\\\ ")),
                Prec::Atom,
            )
        }
        ExprNode::Set(v) => {
            if v.is_empty() {
                ("\\emptyset".into(), Prec::Atom)
            } else {
                (
                    format!("\\left\\{{{}\\right\\}}", latex_list(v)),
                    Prec::Atom,
                )
            }
        }
        ExprNode::Interval {
            lo,
            hi,
            lo_closed,
            hi_closed,
        } => (
            format!(
                "\\left{}{}, {}\\right{}",
                if *lo_closed { "[" } else { "(" },
                latex(lo).0,
                latex(hi).0,
                if *hi_closed { "]" } else { ")" }
            ),
            Prec::Atom,
        ),
        ExprNode::Tuple(v) => (format!("\\left({}\\right)", latex_list(v)), Prec::Atom),
    }
}

fn latex_list(v: &[Expr]) -> String {
    v.iter().map(|a| latex(a).0).collect::<Vec<_>>().join(", ")
}

fn latex_func_name(f: Func) -> Option<&'static str> {
    Some(match f {
        Func::Sin => "\\sin",
        Func::Cos => "\\cos",
        Func::Tan => "\\tan",
        Func::Sec => "\\sec",
        Func::Csc => "\\csc",
        Func::Cot => "\\cot",
        Func::Asin => "\\arcsin",
        Func::Acos => "\\arccos",
        Func::Atan => "\\arctan",
        Func::Sinh => "\\sinh",
        Func::Cosh => "\\cosh",
        Func::Tanh => "\\tanh",
        Func::Ln => "\\ln",
        Func::Min => "\\min",
        Func::Max => "\\max",
        Func::Gcd => "\\gcd",
        Func::Gamma => "\\Gamma",
        Func::Digamma => "\\psi",
        Func::Atan2 => "\\operatorname{atan2}",
        Func::Asinh => "\\operatorname{arsinh}",
        Func::Acosh => "\\operatorname{arcosh}",
        Func::Atanh => "\\operatorname{artanh}",
        Func::Sign => "\\operatorname{sgn}",
        Func::Erf => "\\operatorname{erf}",
        Func::Si => "\\operatorname{Si}",
        Func::Ci => "\\operatorname{Ci}",
        Func::Lcm => "\\operatorname{lcm}",
        _ => return None,
    })
}

fn latex_func(f: Func, args: &[Expr]) -> (String, Prec) {
    let first = || args.first().map(|a| latex(a).0).unwrap_or_default();
    match f {
        Func::Abs => (format!("\\left|{}\\right|", first()), Prec::Atom),
        Func::Floor => (
            format!("\\left\\lfloor {} \\right\\rfloor", first()),
            Prec::Atom,
        ),
        Func::Ceil => (
            format!("\\left\\lceil {} \\right\\rceil", first()),
            Prec::Atom,
        ),
        Func::Factorial => {
            let (s, p) = args.first().map(latex).unwrap_or_default_atom();
            (format!("{}!", lparen(s, p, Prec::Atom)), Prec::Pow)
        }
        Func::Binomial => (
            format!(
                "\\binom{{{}}}{{{}}}",
                first(),
                args.get(1).map(|a| latex(a).0).unwrap_or_default()
            ),
            Prec::Atom,
        ),
        Func::Log => (
            format!(
                "\\log_{{{}}}\\left({}\\right)",
                args.get(1).map(|a| latex(a).0).unwrap_or_default(),
                first()
            ),
            Prec::Atom,
        ),
        _ => {
            let name = latex_func_name(f).unwrap_or("\\operatorname{f}");
            (
                format!("{name}\\left({}\\right)", latex_list(args)),
                Prec::Atom,
            )
        }
    }
}

fn latex_product(e: &Expr) -> (String, Prec) {
    let p = product_shape(e);
    let join = |parts: Vec<(String, Prec)>| -> String {
        let mut out = String::new();
        for (k, (s, _)) in parts.iter().enumerate() {
            if k > 0 {
                let next_numeric = s.starts_with(|c: char| c.is_ascii_digit());
                out.push_str(if next_numeric { " \\cdot " } else { " " });
            }
            out.push_str(s);
        }
        out
    };
    let mut num: Vec<(String, Prec)> = Vec::new();
    if let Some(c) = &p.num_coeff {
        let (s, pr) = latex_number(c);
        num.push((s, pr));
    }
    for f in &p.num {
        let (s, pr) = latex(f);
        num.push((s, pr));
    }
    let mut den: Vec<(String, Prec)> = Vec::new();
    if let Some(q) = &p.den_coeff {
        den.push((q.to_string(), Prec::Atom));
    }
    for f in &p.den {
        let (s, pr) = latex(f);
        den.push((s, pr));
    }
    // Inside \frac a lone factor needs no brackets.
    let wrap = |parts: Vec<(String, Prec)>, alone_ok: bool| -> Vec<(String, Prec)> {
        let lone = parts.len() == 1 && alone_ok;
        parts
            .into_iter()
            .map(|(s, pr)| {
                if lone {
                    (s, pr)
                } else {
                    (lparen(s, pr, Prec::Mul), pr)
                }
            })
            .collect()
    };
    let num = wrap(num, !den.is_empty());
    let den = wrap(den, true);
    let sign = if p.negative { "-" } else { "" };
    let num_text = if num.is_empty() {
        "1".to_string()
    } else {
        join(num.clone())
    };
    let out = if den.is_empty() {
        format!("{sign}{num_text}")
    } else {
        format!("{sign}\\frac{{{num_text}}}{{{}}}", join(den.clone()))
    };
    let prec = if p.negative {
        Prec::Neg
    } else if den.is_empty() && num.len() == 1 {
        num[0].1.min(Prec::Atom)
    } else {
        Prec::Mul
    };
    (out, prec)
}

impl Expr {
    /// LaTeX source for this expression.
    pub fn to_latex(&self) -> String {
        latex(self).0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::BigOpKind;

    fn x() -> Expr {
        Expr::sym("x")
    }
    fn n(k: i64) -> Expr {
        Expr::int(k)
    }
    fn call(name: &str, args: Vec<Expr>) -> Expr {
        Expr::call(name, args).unwrap()
    }

    #[test]
    fn plain_text() {
        assert_eq!(
            (x().pow(2) + n(2) * x() + n(1)).to_string(),
            "x^2 + 2*x + 1"
        );
        assert_eq!((n(1) - x()).to_string(), "-x + 1");
        assert_eq!((x() / n(2)).to_string(), "x/2");
        assert_eq!((n(3) / x()).to_string(), "3/x");
        assert_eq!((x() / (x() + n(1))).to_string(), "x/(x + 1)");
        assert_eq!(x().sqrt().recip().to_string(), "1/sqrt(x)");
        assert_eq!((-x()).to_string(), "-x");
        assert_eq!((n(2) * Expr::pi() * Expr::sym("r")).to_string(), "2*π*r");
        assert_eq!(x().pow(Expr::rational(1, 3)).to_string(), "x^(1/3)");
        assert_eq!((x() + n(1)).pow(2).to_string(), "(x + 1)^2");
        assert_eq!(Expr::e().pow(x()).to_string(), "e^x");
        assert_eq!(call("sin", vec![x()]).pow(2).to_string(), "sin(x)^2");
        assert_eq!(call("abs", vec![x()]).to_string(), "|x|");
        assert_eq!(call("factorial", vec![x() + n(1)]).to_string(), "(x + 1)!");
        assert_eq!(call("log", vec![x(), n(2)]).to_string(), "log_2(x)");
        assert_eq!(
            (n(3) * Expr::i() + n(2)).to_string(),
            "2 + 3*i",
            "complex numbers"
        );
        assert_eq!(Expr::neg_infinity().to_string(), "-∞");
        assert_eq!(Expr::equation(x(), n(2)).to_string(), "x = 2");
    }

    #[test]
    fn plain_text_of_holders() {
        let s = Expr::new(ExprNode::BigOp {
            kind: BigOpKind::Sum,
            expr: Expr::sym("n").pow(-2),
            var: "n".into(),
            lower: n(1),
            upper: Expr::infinity(),
        });
        assert_eq!(s.to_string(), "∑_(n=1)^∞ (1/n^2)");
        let i = Expr::new(ExprNode::Integral {
            expr: x().pow(2),
            var: "x".into(),
            bounds: Some((n(0), n(1))),
        });
        assert_eq!(i.to_string(), "∫_0^1 x^2 dx");
        let m = Expr::matrix(vec![vec![n(1), n(2)], vec![n(3), n(4)]]).unwrap();
        assert_eq!(m.to_string(), "[[1, 2], [3, 4]]");
    }

    #[test]
    fn latex_text() {
        assert_eq!((x() / n(2)).to_latex(), "\\frac{x}{2}");
        assert_eq!(x().sqrt().to_latex(), "\\sqrt{x}");
        assert_eq!(x().root(n(3)).to_latex(), "\\sqrt[3]{x}");
        assert_eq!(call("sin", vec![x()]).to_latex(), "\\sin\\left(x\\right)");
        assert_eq!(
            call("sin", vec![x()]).pow(2).to_latex(),
            "\\sin^{2}\\left(x\\right)"
        );
        assert_eq!(
            (x().pow(2) - n(2) * x() + n(1)).to_latex(),
            "x^{2} - 2 x + 1"
        );
        assert_eq!((n(2) * n(3).sqrt()).to_latex(), "2 \\sqrt{3}");
        assert_eq!(Expr::sym("alpha").to_latex(), "\\alpha");
        assert_eq!(Expr::sym("v_0").to_latex(), "v_{0}");
        assert_eq!(Expr::rational(-3, 4).to_latex(), "-\\frac{3}{4}");
        assert_eq!(
            (-(x() + n(1)) / Expr::sym("y")).to_latex(),
            "-\\frac{x + 1}{y}"
        );
        let m = Expr::matrix(vec![vec![n(1), n(0)], vec![n(0), n(1)]]).unwrap();
        assert_eq!(
            m.to_latex(),
            "\\begin{pmatrix} 1 & 0 \\\\ 0 & 1 \\end{pmatrix}"
        );
    }

    #[test]
    fn textbook_order() {
        let y = Expr::sym("y");
        let e = n(1) + x() + x().pow(3) + y.clone() * x() + x().pow(2);
        assert_eq!(e.to_string(), "x^3 + x^2 + x*y + x + 1");
    }
}
