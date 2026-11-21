//! Printing: readable plain maths (`Display`) and LaTeX (`to_latex`).

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};

use crate::expr::{
    BigOpKind, Constant, Expr, ExprNode, LimitDir, LogicOp, Name, RelOp, EULER_GAMMA,
};
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

thread_local! {
    /// True while [`Expr::to_text`] prints: implicit products, superscript powers,
    /// `√`, Unicode subscripts and textbook names of the special functions.
    static UNICODE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn unicode() -> bool {
    UNICODE.with(std::cell::Cell::get)
}

/// Restores the printing mode when dropped, so a panic cannot leave it switched on.
struct UnicodeGuard(bool);

impl Drop for UnicodeGuard {
    fn drop(&mut self) {
        UNICODE.with(|u| u.set(self.0));
    }
}

fn superscript(c: char) -> Option<char> {
    Some(match c {
        '0' => '⁰',
        '1' => '¹',
        '2' => '²',
        '3' => '³',
        '4' => '⁴',
        '5' => '⁵',
        '6' => '⁶',
        '7' => '⁷',
        '8' => '⁸',
        '9' => '⁹',
        '-' => '⁻',
        _ => return None,
    })
}

fn subscript(c: char) -> Option<char> {
    Some(match c {
        '0' => '₀',
        '1' => '₁',
        '2' => '₂',
        '3' => '₃',
        '4' => '₄',
        '5' => '₅',
        '6' => '₆',
        '7' => '₇',
        '8' => '₈',
        '9' => '₉',
        '+' => '₊',
        '-' => '₋',
        'a' => 'ₐ',
        'e' => 'ₑ',
        'o' => 'ₒ',
        'x' => 'ₓ',
        'h' => 'ₕ',
        'k' => 'ₖ',
        'l' => 'ₗ',
        'm' => 'ₘ',
        'n' => 'ₙ',
        'p' => 'ₚ',
        's' => 'ₛ',
        't' => 'ₜ',
        'i' => 'ᵢ',
        'j' => 'ⱼ',
        'r' => 'ᵣ',
        'u' => 'ᵤ',
        'v' => 'ᵥ',
        _ => return None,
    })
}

/// `C_1` as `C₁` when every character of the subscript has a subscript form.
fn unicode_name(name: &str) -> String {
    let Some((base, sub)) = name.split_once('_') else {
        return name.to_string();
    };
    let sub = sub.trim_start_matches('{').trim_end_matches('}');
    match sub.chars().map(subscript).collect::<Option<String>>() {
        Some(s) if !s.is_empty() && !base.is_empty() => format!("{base}{s}"),
        _ => name.to_string(),
    }
}

/// True when a printed factor is one symbol (a letter with optional subscripts and
/// primes), a number, a bracket, a call `f(…)` or a power of one of these, so it can
/// stand next to another without a sign.
fn juxtaposable(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if first == '(' || first == '[' || first == '|' || first == '√' {
        return true;
    }
    if first.is_ascii_digit() {
        return s.chars().all(|c| {
            c.is_ascii_digit() || c == '.' || superscript(c).is_some() || "⁰¹²³⁴⁵⁶⁷⁸⁹⁻".contains(c)
        });
    }
    if !first.is_alphabetic() {
        return false;
    }
    // A call: a word straight before its bracket (`sin(x)`, `W(x)`).
    let word: String = s.chars().take_while(|c| c.is_alphabetic()).collect();
    if s[word.len()..].starts_with('(') {
        return true;
    }
    // One letter, then subscripts, primes, superscripts, a call or a power.
    let rest: String = chars.collect();
    let after_letters =
        rest.trim_start_matches(|c: char| "₀₁₂₃₄₅₆₇₈₉₊₋ₐₑₒₓₕₖₗₘₙₚₛₜᵢⱼᵣᵤᵥ′″⁰¹²³⁴⁵⁶⁷⁸⁹⁻".contains(c));
    after_letters.is_empty() || after_letters.starts_with('(') || after_letters.starts_with('^')
}

/// A word at the start of a factor (`sin(x)`, `ln(x)`): set apart by a space.
fn starts_with_word(s: &str) -> bool {
    let letters = s.chars().take_while(|c| c.is_alphabetic()).count();
    letters >= 2
}

/// Joins printed factors the way they are written by hand: `2xy`, `9π`, `x sin(x)`,
/// with `·` where two would run together (`2·3^x`, `x·kg`).
fn join_factors(parts: &[String]) -> String {
    let mut out = String::new();
    for (k, s) in parts.iter().enumerate() {
        if k > 0 {
            let prev = &parts[k - 1];
            let number_before = prev.starts_with(|c: char| c.is_ascii_digit());
            // `e^x` ends in its exponent, which the next factor would join.
            let open_power = prev.contains('^') && !prev.ends_with(')');
            let sep = if s.starts_with(|c: char| c.is_ascii_digit() || c == '.')
                || !juxtaposable(prev)
                || !juxtaposable(s)
            {
                "·"
            } else if open_power
                || starts_with_word(s)
                || (starts_with_word(prev) && !prev.ends_with(')'))
                || (s.contains('^') && !number_before)
            {
                " "
            } else {
                ""
            };
            out.push_str(sep);
        }
        out.push_str(s);
    }
    out
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

/// `union(I₁, I₂, …)` of intervals and finite sets, printed `I₁ ∪ I₂ ∪ …`.
fn is_set_union(name: &str, args: &[Expr]) -> bool {
    name == "union"
        && args.len() >= 2
        && args
            .iter()
            .all(|a| matches!(a.node(), ExprNode::Interval { .. } | ExprNode::Set(_)))
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

fn hint_prec(h: crate::display::Hint) -> Prec {
    use crate::display::Hint;
    match h {
        Hint::Atom => Prec::Atom,
        Hint::Neg => Prec::Neg,
        Hint::Mul => Prec::Mul,
        Hint::Add => Prec::Add,
    }
}

fn plain_number(n: &Number) -> (String, Prec) {
    if let Some((s, h)) = crate::display::render_active(n, false) {
        return (s, hint_prec(h));
    }
    let mut s = n.to_string();
    if unicode() && matches!(n, Number::Complex(..) | Number::ComplexFloat(..)) {
        // `2i`, not `2*i`.
        s = s.replace("*i", "i");
    }
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
    if let Some(u) = plain_units(e) {
        return u;
    }
    match e.node() {
        ExprNode::Num(n) => plain_number(n),
        ExprNode::Sym(s) if unicode() => (unicode_name(s), Prec::Atom),
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
                if unicode() {
                    let (s, p) = plain(b);
                    let simple = p == Prec::Atom && !s.contains('(');
                    return if simple {
                        (format!("√{s}"), Prec::Atom)
                    } else {
                        (format!("√({s})"), Prec::Atom)
                    };
                }
                return (format!("sqrt({})", plain(b).0), Prec::Atom);
            }
            let (bs, bp) = plain(b);
            let (xs, xp) = plain(x);
            let base = paren(bs, bp, Prec::Atom);
            if unicode()
                && x.as_number()
                    .is_some_and(|n| n.as_rational().is_some_and(|r| r.is_integer()))
            {
                if let Some(sup) = xs.chars().map(superscript).collect::<Option<String>>() {
                    return (format!("{base}{sup}"), Prec::Pow);
                }
            }
            let exp = if xp == Prec::Atom {
                xs
            } else {
                format!("({xs})")
            };
            (format!("{base}^{exp}"), Prec::Pow)
        }
        ExprNode::Func(f, args) => plain_func(*f, args),
        ExprNode::Apply(name, args) if is_set_union(name, args) => (
            args.iter()
                .map(|a| plain(a).0)
                .collect::<Vec<_>>()
                .join(" ∪ "),
            Prec::Add,
        ),
        ExprNode::Apply(..) if crate::family::as_member(e).is_some() => plain_member(e),
        ExprNode::Apply(name, args) if args.is_empty() && &**name == EULER_GAMMA => {
            ("γ".into(), Prec::Atom)
        }
        ExprNode::Apply(name, args) if unicode() => match textbook_apply(name, args) {
            Some(s) => (s, Prec::Atom),
            None => (format!("{name}({})", plain_list(args)), Prec::Atom),
        },
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

/// The textbook spelling of a special function in Unicode text: `W(x)`, `J₁(x)`,
/// `K(k)`, `δ′(x)`, `ψ′(x)`.
fn textbook_apply(name: &str, args: &[Expr]) -> Option<String> {
    let order = |e: &Expr| -> Option<String> {
        let s = plain(e).0;
        s.chars().map(subscript).collect::<Option<String>>()
    };
    let primes = |n: &Expr| -> Option<String> {
        let k = n.as_number()?.as_i64()?;
        match k {
            1 => Some("′".into()),
            2 => Some("″".into()),
            3 => Some("‴".into()),
            _ => None,
        }
    };
    Some(match (name, args) {
        ("lambertw", [x]) => format!("W({})", plain(x).0),
        ("lambertw", [x, k]) => format!("W{}({})", order(k)?, plain(x).0),
        ("besselj", [n, x]) => format!("J{}({})", order(n)?, plain(x).0),
        ("ellipk", [k]) => format!("K({})", plain(k).0),
        ("ellipe", [k]) => format!("E({})", plain(k).0),
        ("delta", [x]) => format!("δ({})", plain(x).0),
        ("delta", [x, j]) => format!("δ{}({})", primes(j)?, plain(x).0),
        ("polygamma", [n, x]) => format!("ψ{}({})", primes(n)?, plain(x).0),
        ("zeta", [s]) => format!("ζ({})", plain(s).0),
        ("hurwitzzeta", [s, a]) => format!("ζ({}, {})", plain(s).0, plain(a).0),
        _ => return None,
    })
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

/// In Unicode text, letters come before exponentials and function calls (`x e^x`,
/// `x sin(x)`), as they are written by hand.
fn hand_rank(e: &Expr) -> u8 {
    match e.node() {
        ExprNode::Sym(_) | ExprNode::Const(_) => 0,
        ExprNode::Pow(b, x) if x.as_number().is_some() && matches!(b.node(), ExprNode::Sym(_)) => 0,
        _ => 1,
    }
}

fn plain_product(e: &Expr) -> (String, Prec) {
    let mut p = product_shape(e);
    if unicode() {
        p.num.sort_by_key(hand_rank);
        p.den.sort_by_key(hand_rank);
    }
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
    let join = |v: &[String]| {
        if unicode() {
            join_factors(v)
        } else {
            v.join("*")
        }
    };
    if num.is_empty() {
        out.push('1');
    } else {
        out.push_str(&join(&num));
    }
    if !den.is_empty() {
        out.push('/');
        if den.len() == 1 {
            out.push_str(&den[0]);
        } else {
            out.push('(');
            out.push_str(&join(&den));
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
        Func::Digamma if unicode() => (format!("ψ({})", plain_list(args)), Prec::Atom),
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
        // `Δx`, the change in x: `\Delta x`, with the x in italics.
        (Some('Δ'), Some(c)) if c.is_ascii_alphabetic() && chars.next().is_none() => {
            format!("\\Delta {c}")
        }
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
    if let Some((s, h)) = crate::display::render_active(n, true) {
        return (s, hint_prec(h));
    }
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
    if let Some(u) = latex_units(e) {
        return u;
    }
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
        ExprNode::Apply(name, args) if is_set_union(name, args) => (
            args.iter()
                .map(|a| latex(a).0)
                .collect::<Vec<_>>()
                .join(" \\cup "),
            Prec::Add,
        ),
        ExprNode::Apply(..) if crate::family::as_member(e).is_some() => latex_member(e),
        ExprNode::Apply(name, args) if args.is_empty() && &**name == EULER_GAMMA => {
            ("\\gamma".into(), Prec::Atom)
        }
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

    /// Unicode text for people to read: `9π`, `2xy`, `x² + 1`, `√2`, `C₁`, `x sin(x)`,
    /// with `·` only where two factors would otherwise run together. The plain form
    /// ([`fmt::Display`], `9*π`) stays the stable ASCII one for tools and tests.
    pub fn to_text(&self) -> String {
        let _restore = UnicodeGuard(UNICODE.with(|u| u.replace(true)));
        plain(self).0
    }
}

// ---- unit factors --------------------------------------------------------------------

/// Name prefix of a unit factor (`unit:m/s^2()`, see `mpp_eval::units_wire`).
pub const UNIT_FACTOR_PREFIX: &str = "unit:";
/// `unit as(value, ⟨unit⟩)`: a value in exactly this unit.
pub const UNIT_AS: &str = "unit as";
/// `unit convert(expr, ⟨unit⟩)`: `expr in unit` before evaluation.
pub const UNIT_CONVERT: &str = "unit convert";

/// The unit of a unit factor `⟨u⟩`, or of a power `⟨u⟩^k` with a rational k.
fn unit_factor(e: &Expr) -> Option<crate::units::Unit> {
    match e.node() {
        ExprNode::Apply(name, args) if args.is_empty() => {
            crate::units::parse_unit(name.strip_prefix(UNIT_FACTOR_PREFIX)?).ok()
        }
        ExprNode::Pow(b, x) => {
            let u = unit_factor(b)?;
            let r = x.as_number()?.as_rational()?;
            let k = num_rational::Rational64::new(
                num_traits::ToPrimitive::to_i64(r.numer())?,
                num_traits::ToPrimitive::to_i64(r.denom())?,
            );
            u.powr(k).ok()
        }
        _ => None,
    }
}

/// A product split into its unit factors (combined into one unit) and the rest, when
/// it has any unit factor.
fn split_units(e: &Expr) -> Option<(Expr, crate::units::Unit)> {
    if let Some(u) = unit_factor(e) {
        return Some((Expr::one(), u));
    }
    let ExprNode::Mul(fs) = e.node() else {
        return None;
    };
    let mut unit = crate::units::Unit::one();
    let mut rest = Vec::with_capacity(fs.len());
    let mut any = false;
    for f in fs {
        match unit_factor(f) {
            Some(u) => {
                unit = unit.mul(&u).ok()?;
                any = true;
            }
            None => rest.push(f.clone()),
        }
    }
    any.then(|| (crate::simplify::mul(rest), unit))
}

/// A unit as text: `m/s^2` (plain) or `m/s²` (Unicode).
fn unit_text(u: &crate::units::Unit) -> String {
    if unicode() {
        u.unicode_text()
    } else {
        u.text()
    }
}

/// `value unit` in plain or Unicode text.
fn plain_with_unit(rest: &Expr, u: &crate::units::Unit) -> (String, Prec) {
    let unit = unit_text(u);
    if u.is_one() {
        return plain(rest);
    }
    if rest.is_one() {
        return (unit, Prec::Atom);
    }
    let (s, p) = plain(rest);
    let value = paren(s, p, Prec::Mul);
    if unit == "°" {
        return (format!("{value}°"), Prec::Mul);
    }
    (format!("{value} {unit}"), Prec::Mul)
}

/// `value\,\mathrm{unit}` in LaTeX.
fn latex_with_unit(rest: &Expr, u: &crate::units::Unit) -> (String, Prec) {
    if u.is_one() {
        return latex(rest);
    }
    let unit = if u.text() == "°" {
        "^\\circ".to_string()
    } else {
        u.latex()
    };
    if rest.is_one() {
        return (unit, Prec::Atom);
    }
    let (s, p) = latex(rest);
    let value = lparen(s, p, Prec::Mul);
    if u.text() == "°" {
        return (format!("{value}{unit}"), Prec::Mul);
    }
    (format!("{value}\\,{unit}"), Prec::Mul)
}

/// The plain or Unicode text of the unit operators (`None` for anything else).
fn plain_units(e: &Expr) -> Option<(String, Prec)> {
    if let Some((rest, u)) = split_units(e) {
        return Some(plain_with_unit(&rest, &u));
    }
    match e.node() {
        ExprNode::Apply(name, args) if &**name == UNIT_AS && args.len() == 2 => {
            let (rest, u) =
                split_units(&args[1]).unwrap_or((args[1].clone(), crate::units::Unit::one()));
            Some(plain_with_unit(
                &crate::simplify::mul(vec![args[0].clone(), rest]),
                &u,
            ))
        }
        ExprNode::Apply(name, args) if &**name == UNIT_CONVERT && args.len() == 2 => {
            let (a, pa) = plain(&args[0]);
            let (b, _) = plain(&args[1]);
            Some((format!("{} in {b}", paren(a, pa, Prec::Mul)), Prec::Rel))
        }
        _ => None,
    }
}

/// The LaTeX of the unit operators (`None` for anything else).
fn latex_units(e: &Expr) -> Option<(String, Prec)> {
    if let Some((rest, u)) = split_units(e) {
        return Some(latex_with_unit(&rest, &u));
    }
    match e.node() {
        ExprNode::Apply(name, args) if &**name == UNIT_AS && args.len() == 2 => {
            let (rest, u) =
                split_units(&args[1]).unwrap_or((args[1].clone(), crate::units::Unit::one()));
            Some(latex_with_unit(
                &crate::simplify::mul(vec![args[0].clone(), rest]),
                &u,
            ))
        }
        ExprNode::Apply(name, args) if &**name == UNIT_CONVERT && args.len() == 2 => {
            let (a, pa) = latex(&args[0]);
            let (b, _) = latex(&args[1]);
            Some((
                format!("{} \\text{{ in }} {b}", lparen(a, pa, Prec::Mul)),
                Prec::Rel,
            ))
        }
        _ => None,
    }
}

/// Text made for reading: a `*` between two factors (`2*x`, `(a+1)*b`, as plain
/// printing writes products) becomes `·`, so no message or answer shows the ASCII
/// product sign.
pub fn readable(text: &str) -> String {
    if !text.contains('*') {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let factor_end = |c: char| c.is_alphanumeric() || matches!(c, ')' | ']' | '}' | '!' | '′');
    let factor_start = |c: char| c.is_alphanumeric() || matches!(c, '(' | '[' | '√' | '|');
    chars
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            let between = c == '*'
                && i > 0
                && chars.get(i + 1).is_some_and(|&n| factor_start(n))
                && factor_end(chars[i - 1]);
            if between {
                '·'
            } else {
                c
            }
        })
        .collect()
}

/// A family member as a subscript: `a_1` (plain), `a₁` (Unicode) or `a_(i + 1)`.
fn plain_member(e: &Expr) -> (String, Prec) {
    let Some((base, index)) = crate::family::as_member(e) else {
        return plain(e);
    };
    let base = if unicode() {
        unicode_name(base)
    } else {
        base.to_string()
    };
    let (s, p) = plain(index);
    if unicode() {
        if let Some(sub) = s.chars().map(subscript).collect::<Option<String>>() {
            if p == Prec::Atom && !sub.is_empty() {
                return (format!("{base}{sub}"), Prec::Atom);
            }
        }
    }
    let sub = if p == Prec::Atom { s } else { format!("({s})") };
    (format!("{base}_{sub}"), Prec::Atom)
}

/// A family member in LaTeX: `a_{1}`.
fn latex_member(e: &Expr) -> (String, Prec) {
    let Some((base, index)) = crate::family::as_member(e) else {
        return latex(e);
    };
    (
        format!("{}_{{{}}}", latex_name(base), latex(index).0),
        Prec::Atom,
    )
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
