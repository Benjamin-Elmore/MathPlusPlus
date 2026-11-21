//! Random variables in documents: what `E[X]`, `\operatorname{Var}(X)`, `P(X \le a)`
//! and sums over i.i.d. families mean once `X \sim D` has been declared (see the
//! declaration in `block/random.rs`).
//!
//! - `\mathbb{E}[g]`, `E[g]`, `\operatorname{E}[g]` → [`EXPECTATION`];
//!   `\operatorname{Var}(g)`, `\mathrm{Var}(g)` → [`VARIANCE`]; `P(A)`, `\mathbb{P}(A)`,
//!   `\Pr(A)` → [`PROBABILITY`], whenever the argument uses a declared random variable.
//!   They answer exactly from the distribution ([`mpp_core::domains::probability::random`]),
//!   or report why they cannot; they never fall back to an estimate.
//! - `\sum_{j=1}^{N} f(X_j, Y_j)` over the members of declared families draws seeded
//!   samples: the document seed (`%seed`) and each family's name give the family its
//!   own stream, so a member has the same value wherever it is used.

use std::collections::HashMap;
use std::sync::Mutex;

use mpp_core::domains::probability::random::{monte_carlo_sum, Dist};
use mpp_core::{Expr, ExprNode, MathError, MathResult};

/// Hidden key prefix of a single random variable (`X \sim D`).
pub const RV_KEY: &str = "rv:";
/// Hidden key prefix of an i.i.d. family (`X_j \sim D`).
pub const FAMILY_KEY: &str = "rvs:";

/// `E[g]` (the core operator name; printed `𝔼[g]`).
pub const EXPECTATION: &str = "𝔼";
/// `Var(g)`.
pub const VARIANCE: &str = "Var";
/// `P(A)` (printed `ℙ(A)`).
pub const PROBABILITY: &str = "ℙ";

/// The stored form of a distribution: its canonical name applied to its parameters.
pub fn dist_expr(name: &str, params: Vec<Expr>) -> Expr {
    Expr::apply(name, params)
}

/// The distribution stored by [`dist_expr`], with each parameter passed through
/// `resolve` first.
pub fn dist_of(
    stored: &Expr,
    resolve: &mut impl FnMut(&Expr) -> MathResult<Expr>,
) -> MathResult<Dist> {
    let ExprNode::Apply(name, params) = stored.node() else {
        return Err(MathError::NotANumber(format!(
            "{stored} is not a distribution"
        )));
    };
    let params = params
        .iter()
        .map(&mut *resolve)
        .collect::<MathResult<Vec<_>>>()?;
    Dist::new(name, params)
}

/// The text shown for a declaration in the outline and variables panel
/// (`X \sim \mathrm{Uniform}[0, 1]`), from a hidden key and its stored value.
pub fn declaration_latex(key: &str, stored: &Expr) -> Option<(String, String)> {
    let (name, family) = if let Some(n) = key.strip_prefix(FAMILY_KEY) {
        (n, true)
    } else {
        (key.strip_prefix(RV_KEY)?, false)
    };
    let dist = dist_of(stored, &mut |e| Ok(e.clone())).ok()?;
    let shown = if family {
        format!("{name}_j")
    } else {
        name.to_string()
    };
    let latex = Expr::sym(&shown).to_latex();
    Some((shown, format!("{latex} \\sim {}", dist.latex())))
}

/// True for the hidden keys this module stores.
pub fn is_random_key(key: &str) -> bool {
    key.starts_with(RV_KEY) || key.starts_with(FAMILY_KEY)
}

/// Monte Carlo sums already computed, by everything that decides their value, so a
/// definition used on several lines (or re-run as the document is edited) is sampled
/// once.
static CACHE: Mutex<Option<HashMap<String, f64>>> = Mutex::new(None);
/// Most cached sums.
const CACHE_SIZE: usize = 64;

/// One Monte Carlo sum to compute.
pub struct McSum<'a> {
    pub body: &'a Expr,
    pub var: &'a str,
    pub lo: i64,
    pub hi: i64,
    pub families: &'a [(String, Dist)],
    pub seed: u64,
    pub product: bool,
}

/// A family member inside a resolved expression (`X_j` for the family X).
pub fn member_of(e: &Expr, families: &[(String, Dist)]) -> Option<(String, Expr)> {
    let (name, index) = mpp_core::family::as_member(e)?;
    families
        .iter()
        .any(|(n, _)| n == name)
        .then(|| (name.to_string(), index.clone()))
}

/// The value of a Monte Carlo sum (cached).
pub fn mc_sum(s: &McSum<'_>) -> MathResult<f64> {
    let key = format!(
        "{:?}|{}|{}|{}|{:?}|{}|{}",
        s.body, s.var, s.lo, s.hi, s.families, s.seed, s.product
    );
    if let Ok(guard) = CACHE.lock() {
        if let Some(v) = guard.as_ref().and_then(|m| m.get(&key)) {
            return Ok(*v);
        }
    }
    let families = s.families;
    let members = move |e: &Expr| member_of(e, families);
    let v = monte_carlo_sum(
        s.body, s.var, s.lo, s.hi, families, &members, s.seed, s.product,
    )?;
    if let Ok(mut guard) = CACHE.lock() {
        let map = guard.get_or_insert_with(HashMap::new);
        if map.len() >= CACHE_SIZE {
            map.clear();
        }
        map.insert(key, v);
    }
    Ok(v)
}
