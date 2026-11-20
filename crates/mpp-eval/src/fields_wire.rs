//! The functions across the fields in documents: textbook names and `%seed`.
//!
//! The operators of number theory, special functions, analysis, discrete maths,
//! probability and information already answer through the domain registry when
//! called by their operator names (`\operatorname{divcount}(360)`, see
//! [`crate::domains_wire`]). This module adds:
//! - **textbook names** ([`textbook`]): `\tau(n)`, `\sigma(n)`, `\sigma_k(n)`, `\mu(n)`,
//!   `\varphi(n)`, `\pi(x)`, `\zeta(s)`, `\zeta(s, a)`, `L(s, \chi)`, `B(a, b)`, `W(x)`,
//!   `W_0(x)`, `W_{-1}(x)`, `J_n(x)`, `P_n(x)`, `T_n(x)`, `U_n(x)`, `K(k)`, `E(k)`,
//!   `\delta'(x)`, `\Phi(z)` and `v_p(x)` lower to their operators when they are
//!   applied to arguments (`H(x)` and `\delta(x)` stay the step and delta calls the
//!   Laplace tables read, so `H(0)` keeps its value open). A bare name stays a
//!   constant or a variable, and a name the document binds itself (a variable, a function, a parameter or an
//!   index) keeps its own meaning, so `\tau := 3` makes `\tau(2)` a product again.
//!   A constant brought in from the standard library (`\varphi` from
//!   `std/constants.mpp`) still means the function when applied, as in a textbook.
//! - **reproducible randomness** ([`seed_calls`], [`evaluate`]): `%seed 42` fixes the
//!   seed for the rest of the document. A sampling operator written without its seed
//!   (`\operatorname{runif}(5)`, `\operatorname{montecarlo}(f, box, N)`,
//!   `\operatorname{brownian}(n, T)`) gets one derived from the document seed, the
//!   block and its place in the block, so two calls differ but every run of the same
//!   file gives the same numbers on every computer. Without `%seed` the seed is 0,
//!   and the line says so.

use mpp_core::{Expr, ExprNode, MathResult};
use mpp_syntax::math::{Constant, Node, NodeKind};

use crate::holders::Ctx;
use crate::lower::LowerScope;
use crate::names::node_key;

/// Environment key holding the document seed set by `%seed n`.
pub const SEED_KEY: &str = "seed:";

/// Marker wrapping a derived seed: `document seed(value, is_default)`.
const SEED_MARK: &str = "document seed";

/// Sampling operators: (name, argument counts without the seed, seed position:
/// `true` = first, `false` = last).
const SAMPLING: &[(&str, &[usize], bool)] = &[
    ("runif", &[1, 3], true),
    ("rnorm", &[1, 3], true),
    ("brownian", &[2], true),
    ("montecarlo", &[3], false),
];

/// A textbook name resolved to a registry operator: the arguments become
/// `lead ++ written ++ trail`.
#[derive(Debug, Clone, PartialEq)]
pub struct Textbook {
    pub op: &'static str,
    pub lead: Vec<Expr>,
    pub trail: Vec<Expr>,
}

impl Textbook {
    fn new(op: &'static str) -> Self {
        Textbook {
            op,
            lead: Vec::new(),
            trail: Vec::new(),
        }
    }

    /// The operator applied to the written arguments.
    pub fn apply(self, args: Vec<Expr>) -> Expr {
        let mut all = self.lead;
        all.extend(args);
        all.extend(self.trail);
        Expr::apply(self.op, all)
    }
}

/// True when `key` is free to take its textbook meaning here: not bound by the
/// statement, not a sequence, and either unbound or a standard-library constant.
fn is_free(scope: &dyn LowerScope, locals: &[String], key: &str) -> bool {
    if locals.iter().any(|l| l == key) || scope.is_sequence(key) {
        return false;
    }
    match scope.binding(key) {
        None => true,
        Some(b) => {
            b.params().is_none()
                && b.origin()
                    .file
                    .as_deref()
                    .is_some_and(|f| f.starts_with("std/"))
        }
    }
}

/// A subscript as an argument: an integer, or a name the resolver fills in.
fn sub_arg(sub: &str) -> Option<Expr> {
    if let Ok(n) = sub.parse::<i64>() {
        return Some(Expr::int(n));
    }
    let simple = !sub.is_empty() && sub.chars().all(|c| c.is_alphanumeric() || c == '_');
    simple.then(|| Expr::sym(sub))
}

/// The registry operator a call on `callee` with `nargs` arguments means, when the
/// callee is a textbook name free at this point (see the module notes).
pub fn textbook(
    scope: &dyn LowerScope,
    locals: &[String],
    callee: &Node,
    nargs: usize,
) -> Option<Textbook> {
    let callee = callee.ungrouped();
    match &callee.kind {
        // π(x): the prime-counting function.
        NodeKind::Constant(Constant::Pi) if nargs == 1 => {
            return is_free(scope, locals, "π").then(|| Textbook::new("primepi"));
        }
        // δ'(x), δ''(x): derivatives of the Dirac delta.
        NodeKind::Prime { base, count } if nargs == 1 => {
            let key = node_key(base)?;
            if key != "δ" || !is_free(scope, locals, &key) {
                return None;
            }
            let mut t = Textbook::new("dirac");
            t.trail.push(Expr::int(i64::from(*count)));
            return Some(t);
        }
        _ => {}
    }
    let key = node_key(callee)?;
    if !is_free(scope, locals, &key) {
        return None;
    }
    let (base, sub) = match key.split_once('_') {
        Some((b, s)) => (b, Some(s)),
        None => (key.as_str(), None),
    };
    let t = match (base, sub, nargs) {
        ("τ", None, 1) => Textbook::new("divcount"),
        ("σ", None, 1) => Textbook::new("divsum"),
        ("σ", Some(k), 1) => {
            let mut t = Textbook::new("divsum");
            t.trail.push(sub_arg(k)?);
            t
        }
        ("μ", None, 1) => Textbook::new("mobius"),
        ("φ" | "ϕ", None, 1) => Textbook::new("totient"),
        ("ζ", None, 1) => Textbook::new("zeta"),
        ("ζ", None, 2) => Textbook::new("hurwitzzeta"),
        ("L", None, 2) => Textbook::new("dirichletl"),
        ("B", None, 2) => Textbook::new("beta"),
        ("W", None, 1) => Textbook::new("lambertw"),
        ("W", Some(k), 1) => {
            let mut t = Textbook::new("lambertw");
            t.trail.push(sub_arg(k)?);
            t
        }
        ("J" | "P" | "T" | "U", Some(n), 1) => {
            let op = match base {
                "J" => "besselj",
                "P" => "legendrep",
                "T" => "chebyshevt",
                _ => "chebyshevu",
            };
            let mut t = Textbook::new(op);
            t.lead.push(sub_arg(n)?);
            t
        }
        ("K", None, 1) => Textbook::new("ellipk"),
        ("E", None, 1) => Textbook::new("ellipe"),
        ("Φ", None, 1) => Textbook::new("normalcdf"),
        ("v", Some(p), 1) => {
            let mut t = Textbook::new("padicval");
            t.lead.push(sub_arg(p)?);
            t
        }
        _ => return None,
    };
    Some(t)
}

/// Reads `%seed n` (the text after the `%`): `None` when the line is not a seed
/// directive, `Some(Err)` when it is one but malformed.
pub fn seed_directive(text: &str) -> Option<Result<i64, String>> {
    let rest = text.trim().strip_prefix("seed")?;
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim();
    Some(rest.parse::<i64>().map_err(|_| {
        if rest.is_empty() {
            "`%seed` needs a whole number, such as `%seed 42`".to_string()
        } else {
            format!("`%seed {rest}`: the seed must be a whole number, such as `%seed 42`")
        }
    }))
}

/// SplitMix64's finaliser: spreads the bits of `x`.
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// True when `e` holds a sampling call written without its seed.
pub fn needs_seed(e: &Expr) -> bool {
    if let ExprNode::Apply(name, args) = e.node() {
        if SAMPLING
            .iter()
            .any(|(op, counts, _)| *op == &**name && counts.contains(&args.len()))
        {
            return true;
        }
    }
    e.children().into_iter().any(needs_seed)
}

/// Gives every sampling call in `e` that has no seed one derived from the document
/// seed (`None`: no `%seed`, so 0), the block and `place` (the line's offset in its
/// block). The k-th such call in `e` gets a different seed from the others.
pub fn seed_calls(e: &Expr, seed: Option<i64>, block: u64, place: u64) -> Expr {
    let mut k = 0u64;
    let base = seed.unwrap_or(0);
    let default = seed.is_none();
    rewrite(e, &mut |name, args| {
        let (_, counts, first) = SAMPLING.iter().find(|(op, _, _)| *op == name)?;
        if !counts.contains(&args.len()) {
            return None;
        }
        let h = mix(mix(mix(base as u64) ^ block) ^ place.wrapping_mul(31).wrapping_add(k));
        // 53 bits: shown as an ordinary whole number in the notes.
        let derived = (h >> 11) as i64;
        k += 1;
        let mark = Expr::apply(
            SEED_MARK,
            vec![Expr::int(derived), Expr::new(ExprNode::Bool(default))],
        );
        let mut all = args.to_vec();
        if *first {
            all.insert(0, mark);
        } else {
            all.push(mark);
        }
        Some(Expr::apply(name, all))
    })
}

fn rewrite(e: &Expr, f: &mut impl FnMut(&str, &[Expr]) -> Option<Expr>) -> Expr {
    let inner = Expr::new(e.map_children(&mut |c| rewrite(c, f)));
    if let ExprNode::Apply(name, args) = inner.node() {
        if let Some(done) = f(name, args) {
            return done;
        }
    }
    inner
}

/// Answers sampling calls whose seed came from the document (see [`seed_calls`]).
pub fn evaluate(e: &Expr, ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
    let ExprNode::Apply(name, args) = e.node() else {
        return None;
    };
    let mut unwrapped = Vec::with_capacity(args.len());
    let mut marked = false;
    for a in args {
        match a.node() {
            ExprNode::Apply(m, inner) if &**m == SEED_MARK => {
                let [value, default] = inner.as_slice() else {
                    return None;
                };
                if matches!(default.node(), ExprNode::Bool(true)) {
                    ctx.note("no `%seed` line in the document, so the seed is 0");
                }
                unwrapped.push(value.clone());
                marked = true;
            }
            _ => unwrapped.push(a.clone()),
        }
    }
    if !marked {
        return None;
    }
    let call = Expr::apply(name, unwrapped);
    Some(crate::holders::evaluate_holder(&call, ctx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_lines() {
        assert_eq!(seed_directive("seed 42"), Some(Ok(42)));
        assert_eq!(seed_directive("seed -7"), Some(Ok(-7)));
        assert!(matches!(seed_directive("seed"), Some(Err(_))));
        assert!(matches!(seed_directive("seed x"), Some(Err(_))));
        assert_eq!(seed_directive("seeds 4"), None);
        assert_eq!(seed_directive("noeval"), None);
    }

    #[test]
    fn derived_seeds_differ_and_repeat() {
        let call = Expr::apply("runif", vec![Expr::int(3)]);
        let two = Expr::new(ExprNode::Tuple(vec![call.clone(), call.clone()]));
        assert!(needs_seed(&two));
        let a = seed_calls(&two, Some(42), 7, 3);
        let b = seed_calls(&two, Some(42), 7, 3);
        assert_eq!(a, b);
        let ExprNode::Tuple(items) = a.node() else {
            panic!("a tuple");
        };
        assert_ne!(items[0], items[1]);
        assert!(!needs_seed(&a));
        let seeded = Expr::apply("runif", vec![Expr::int(1), Expr::int(3)]);
        assert!(!needs_seed(&seeded));
    }
}
