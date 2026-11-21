//! Checks (`\overset{?}{=}`): a symbolic proof first, then a numeric spot check.

use std::collections::HashMap;

use mpp_core::hp::Rational as BigRational;
use mpp_core::number::format_f64;
use mpp_core::{Expr, ExprNode};
use num_bigint::BigInt;
use num_complex::Complex64;

use crate::holders::contains_holder;
use crate::numeric::{format_complex, DIGITS};

/// Points tried in the numeric spot check.
const ATTEMPTS: usize = 48;
/// Points that must agree before the check counts as passed.
const WANTED: usize = 12;

/// The outcome of a check.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// The difference of the two sides simplifies to zero.
    Proved,
    /// The two sides agree at every random point tried.
    Holds { points: usize },
    /// The two sides differ at this point.
    Fails {
        /// Values of the free names, as decimal text.
        at: Vec<(String, String)>,
        lhs: String,
        rhs: String,
    },
    /// Two closed expressions agree to this many significant digits in high-precision
    /// arithmetic, but no proof was found (shown with ≈, never ✓).
    Agrees { digits: usize },
    /// Neither a proof nor a numeric check was possible.
    Unknown(String),
}

impl Verdict {
    /// `Some(true)` for ✓, `Some(false)` for ✗, `None` when undecided.
    pub fn passed(&self) -> Option<bool> {
        match self {
            Verdict::Proved | Verdict::Holds { .. } | Verdict::Agrees { .. } => Some(true),
            Verdict::Fails { .. } => Some(false),
            Verdict::Unknown(_) => None,
        }
    }

    /// ✓, ✗, ≈ (agrees numerically, not proved) or ?.
    pub fn mark(&self) -> &'static str {
        if matches!(self, Verdict::Agrees { .. }) {
            return "≈";
        }
        match self.passed() {
            Some(true) => "✓",
            Some(false) => "✗",
            None => "?",
        }
    }

    /// A one-line description.
    pub fn describe(&self) -> String {
        match self {
            Verdict::Proved => "✓ (proved symbolically)".into(),
            Verdict::Holds { points } => format!("✓ (agrees at {points} random points)"),
            Verdict::Fails { at, lhs, rhs } => {
                let point: Vec<String> = at.iter().map(|(n, v)| format!("{n} = {v}")).collect();
                if point.is_empty() {
                    format!("✗ (left side {lhs}, right side {rhs})")
                } else {
                    format!(
                        "✗ (at {}: left side {lhs}, right side {rhs})",
                        point.join(", ")
                    )
                }
            }
            Verdict::Agrees { digits } => {
                format!("≈ (checked numerically to {digits} digits, not proved)")
            }
            Verdict::Unknown(why) => format!("? ({why})"),
        }
    }
}

/// A small deterministic generator, so checks give the same answer every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn is_comparable(e: &Expr) -> bool {
    !matches!(
        e.node(),
        ExprNode::Relation(..)
            | ExprNode::Logic(..)
            | ExprNode::Bool(_)
            | ExprNode::Matrix { .. }
            | ExprNode::Set(_)
            | ExprNode::Interval { .. }
            | ExprNode::Tuple(_)
    )
}

/// Checks whether `lhs = rhs` holds.
pub fn check(lhs: &Expr, rhs: &Expr) -> Verdict {
    // A floating-point value (a numeric fallback) is known to about 15 digits, so two
    // sides that meet only through floats are never "proved" equal.
    let floats = lhs.has_float() || rhs.has_float();
    if lhs == rhs && !floats {
        return Verdict::Proved;
    }
    if crate::units_wire::contains_units(lhs) || crate::units_wire::contains_units(rhs) {
        return check_units(lhs, rhs);
    }
    match (lhs.node(), rhs.node()) {
        (ExprNode::Bool(a), ExprNode::Bool(b)) => {
            return if a == b {
                Verdict::Proved
            } else {
                fails_whole(lhs, rhs)
            };
        }
        (ExprNode::Bool(want), _) if is_proposition(rhs) => return check_truth(rhs, *want),
        (_, ExprNode::Bool(want)) if is_proposition(lhs) => return check_truth(lhs, *want),
        (ExprNode::Tuple(a), ExprNode::Tuple(b)) => return check_entries(lhs, rhs, a, b),
        (
            ExprNode::Matrix {
                rows: ra,
                cols: ca,
                data: a,
            },
            ExprNode::Matrix {
                rows: rb,
                cols: cb,
                data: b,
            },
        ) => {
            if (ra, ca) != (rb, cb) {
                return fails_whole(lhs, rhs);
            }
            return check_entries(lhs, rhs, a, b);
        }
        (ExprNode::Set(a), ExprNode::Set(b)) => return check_sets(lhs, rhs, a, b),
        _ => {}
    }
    if !is_comparable(lhs) || !is_comparable(rhs) {
        return Verdict::Unknown("only numbers and expressions can be compared".into());
    }
    let diff = (lhs - rhs).simplify();
    if !floats && (diff.is_zero() || diff.expand().is_ok_and(|d| d.is_zero())) {
        return Verdict::Proved;
    }
    // Trigonometric identities: sin²x + cos²x = 1, double angles, tan² + 1 = sec² ….
    if mpp_core::trig::identity_zero(&diff) {
        return Verdict::Proved;
    }
    if contains_holder(lhs) || contains_holder(rhs) {
        return Verdict::Unknown(
            "a side holds a calculus expression that is not evaluated yet".into(),
        );
    }
    // A difference that is an exact nonzero number can never vanish: x and x + 10⁻¹²,
    // or 2¹⁰⁰ + 1 and 2¹⁰⁰, are different however close they look.
    if diff
        .as_number()
        .is_some_and(|n| n.is_exact() && !n.is_zero())
    {
        return exact_difference(lhs, rhs);
    }
    if lhs.free_symbols().is_empty() && rhs.free_symbols().is_empty() {
        if let Some(v) = closed_check(lhs, rhs) {
            return v;
        }
    }
    spot_check(lhs, rhs)
}

/// Significant digits a closed check is worked out to.
const CHECK_DIGITS: usize = 30;
/// Digits an approximate (floating-point) input is trusted to.
const FLOAT_DIGITS: usize = 12;

/// The difference `lhs − rhs` as written (not simplified), for numeric evaluation.
fn raw_difference(lhs: &Expr, rhs: &Expr) -> Expr {
    use mpp_core::Number;
    let minus = Expr::new(ExprNode::Mul(vec![
        Expr::num(Number::from(-1)),
        rhs.clone(),
    ]));
    Expr::new(ExprNode::Add(vec![lhs.clone(), minus]))
}

/// Text of a high-precision value with `sig` digits.
fn decimal_shown(d: &mpp_core::hp::Decimal, sig: usize) -> String {
    mpp_core::number::format_decimal_digits(d, sig)
}

/// The sign of `a − b` for closed expressions when high precision shows it is not zero
/// (a gap below a floating-point input's resolution decides nothing).
fn closed_sign(a: &Expr, b: &Expr) -> Option<std::cmp::Ordering> {
    use mpp_core::hp::{approx_closed, Approx};
    if !(a.free_symbols().is_empty() && b.free_symbols().is_empty()) {
        return None;
    }
    let Approx::Value { value, inexact } = approx_closed(&raw_difference(a, b), 15)? else {
        return None;
    };
    if inexact {
        let size = |e: &Expr| match approx_closed(e, 3) {
            Some(Approx::Value { value, .. }) => Some(value.k),
            _ => None,
        };
        let top = size(a).into_iter().chain(size(b)).max()?;
        if top - value.k >= FLOAT_DIGITS as i64 {
            return None;
        }
    }
    Some(if value.neg {
        std::cmp::Ordering::Less
    } else {
        std::cmp::Ordering::Greater
    })
}

/// The two sides compared in high precision at one point: `Some(true)` when they agree
/// to every digit tried (or, with floating-point inputs, to the digits those carry),
/// `Some(false)` when they differ, `None` when the evaluator cannot handle a side here.
fn point_check(lhs: &Expr, rhs: &Expr, vars: &HashMap<String, BigRational>) -> Option<bool> {
    use mpp_core::hp::{approx, Approx};
    match approx(&raw_difference(lhs, rhs), vars, 20)? {
        Approx::Zero { .. } => Some(true),
        Approx::Value { value, inexact } => {
            if !inexact {
                return Some(false);
            }
            let size = |e: &Expr| match approx(e, vars, 3) {
                Some(Approx::Value { value, .. }) => Some(value.k),
                _ => None,
            };
            let top = size(lhs).into_iter().chain(size(rhs)).max()?;
            Some(top - value.k >= FLOAT_DIGITS as i64)
        }
    }
}

/// Both sides as decimals, with enough digits to show where they differ.
fn sides_shown(
    lhs: &Expr,
    rhs: &Expr,
    vars: &HashMap<String, BigRational>,
) -> Option<(String, String)> {
    use mpp_core::hp::{approx, Approx};
    let diff = approx(&raw_difference(lhs, rhs), vars, 3)?;
    let side = |e: &Expr, sig: usize| match approx(e, vars, sig) {
        // Integers are shown whole while they are short enough to read.
        _ if e
            .as_number()
            .and_then(mpp_core::Number::as_integer)
            .is_some_and(|n| n.to_string().len() <= 40) =>
        {
            Some(e.to_string())
        }
        Some(Approx::Value { value, .. }) => Some(decimal_shown(&value, sig)),
        Some(Approx::Zero { .. }) => Some("0".to_string()),
        None => None,
    };
    let big = |e: &Expr| match approx(e, vars, 3) {
        Some(Approx::Value { value, .. }) => Some(value.k),
        _ => None,
    };
    let top = big(lhs).into_iter().chain(big(rhs)).max()?;
    let sig = match diff {
        Approx::Value { value, .. } => usize::try_from(top - value.k + 3).unwrap_or(0),
        Approx::Zero { .. } => 0,
    }
    .clamp(DIGITS, 400);
    Some((side(lhs, sig)?, side(rhs, sig)?))
}

/// `✗` for two sides whose difference is an exact nonzero number.
fn exact_difference(lhs: &Expr, rhs: &Expr) -> Verdict {
    let mut names: Vec<String> = lhs
        .free_symbols()
        .union(&rhs.free_symbols())
        .map(|s| s.to_string())
        .collect();
    names.sort();
    // At one sample point the two values show the gap.
    let point: HashMap<String, BigRational> = names
        .iter()
        .enumerate()
        .map(|(i, n)| {
            (
                n.clone(),
                BigRational::new(BigInt::from(i as i64 + 1), BigInt::from(2)),
            )
        })
        .collect();
    let at = names
        .iter()
        .map(|n| (n.clone(), point[n].to_string()))
        .collect();
    match sides_shown(lhs, rhs, &point) {
        Some((l, r)) => Verdict::Fails { at, lhs: l, rhs: r },
        None => fails_whole(lhs, rhs),
    }
}

/// A check between two closed expressions, decided in high precision: different
/// values are ✗; values equal to every digit tried are ≈ (never ✓, since agreement to
/// many digits is not a proof). `None` when the high-precision evaluator cannot handle
/// a side.
fn closed_check(lhs: &Expr, rhs: &Expr) -> Option<Verdict> {
    use mpp_core::hp::{approx_closed, Approx};
    let vars = HashMap::new();
    let diff = approx_closed(&raw_difference(lhs, rhs), CHECK_DIGITS)?;
    let size = |e: &Expr| match approx_closed(e, 3) {
        Some(Approx::Value { value, .. }) => Some(value.k),
        Some(Approx::Zero { .. }) => Some(i64::MIN / 2),
        None => None,
    };
    let top = size(lhs)?.max(size(rhs)?);
    match diff {
        Approx::Zero { inexact: true, .. } => Some(Verdict::Agrees {
            digits: FLOAT_DIGITS,
        }),
        Approx::Zero { below, .. } => {
            let digits = usize::try_from(top.max(below) - below).unwrap_or(CHECK_DIGITS);
            Some(Verdict::Agrees {
                digits: digits.max(CHECK_DIGITS),
            })
        }
        Approx::Value { value, inexact } => {
            // A floating-point input carries only about 15 good digits, so a gap below
            // that is no evidence either way.
            let gap_digits = top - value.k;
            if inexact && gap_digits >= FLOAT_DIGITS as i64 {
                return Some(Verdict::Agrees {
                    digits: FLOAT_DIGITS,
                });
            }
            let (l, r) = sides_shown(lhs, rhs, &vars)?;
            Some(Verdict::Fails {
                at: Vec::new(),
                lhs: l,
                rhs: r,
            })
        }
    }
}

/// `✗ (left side …, right side …)` for the two whole sides.
fn fails_whole(lhs: &Expr, rhs: &Expr) -> Verdict {
    Verdict::Fails {
        at: Vec::new(),
        lhs: shown(lhs),
        rhs: shown(rhs),
    }
}

/// A side as text, with its units when it has them.
fn shown(e: &Expr) -> String {
    match crate::units_wire::present(e) {
        Ok(Some(p)) => p.unicode,
        _ => e.to_text(),
    }
}

/// Lists and matrices: entry by entry. Every entry must pass; the first that fails is
/// reported with its position.
fn check_entries(lhs: &Expr, rhs: &Expr, a: &[Expr], b: &[Expr]) -> Verdict {
    if a.len() != b.len() {
        return fails_whole(lhs, rhs);
    }
    let mut fewest = None;
    for (k, (x, y)) in a.iter().zip(b).enumerate() {
        match check(x, y) {
            Verdict::Proved => {}
            Verdict::Holds { points } => {
                fewest = Some(fewest.map_or(points, |p: usize| p.min(points)));
            }
            Verdict::Agrees { digits } => return Verdict::Agrees { digits },
            Verdict::Fails { at, lhs, rhs } => {
                return Verdict::Fails {
                    at,
                    lhs: format!("{lhs} in entry {}", k + 1),
                    rhs,
                }
            }
            Verdict::Unknown(why) => return Verdict::Unknown(format!("entry {}: {why}", k + 1)),
        }
    }
    match fewest {
        Some(points) => Verdict::Holds { points },
        None => Verdict::Proved,
    }
}

/// Sets: equal when every element of each side is proved equal to one on the other.
fn check_sets(lhs: &Expr, rhs: &Expr, a: &[Expr], b: &[Expr]) -> Verdict {
    let covered = |from: &[Expr], to: &[Expr]| {
        from.iter()
            .all(|x| to.iter().any(|y| check(x, y) == Verdict::Proved))
    };
    if covered(a, b) && covered(b, a) {
        return Verdict::Proved;
    }
    let numbers = |s: &[Expr]| s.iter().all(|x| x.free_symbols().is_empty());
    if numbers(a) && numbers(b) {
        return fails_whole(lhs, rhs);
    }
    Verdict::Unknown("the elements of the sets could not all be matched".into())
}

/// Values with units: compared as quantities (`100 cm = 1 m`); different dimensions
/// can never be equal.
fn check_units(lhs: &Expr, rhs: &Expr) -> Verdict {
    match crate::units_wire::relation(mpp_core::RelOp::Eq, lhs, rhs) {
        Ok(Some(true)) => Verdict::Proved,
        Ok(Some(false)) | Err(mpp_core::MathError::Dimension(_)) => fails_whole(lhs, rhs),
        Ok(None) | Err(_) => {
            Verdict::Unknown("the two sides have units that could not be compared".into())
        }
    }
}

fn is_proposition(e: &Expr) -> bool {
    matches!(
        e.node(),
        ExprNode::Relation(..) | ExprNode::Logic(..) | ExprNode::Bool(_)
    )
}

/// The truth of a relation (or a combination of them) at a point; `None` when it
/// cannot be decided there (a side cannot be evaluated, or the two sides are too close
/// to tell a strict inequality from an equality).
fn truth_at(e: &Expr, env: &HashMap<String, f64>) -> Option<bool> {
    use mpp_core::{LogicOp, RelOp};
    match e.node() {
        ExprNode::Bool(b) => Some(*b),
        ExprNode::Relation(op, a, b) => {
            let (x, y) = (a.eval_f64(env).ok()?, b.eval_f64(env).ok()?);
            if !(x.is_finite() && y.is_finite()) {
                return None;
            }
            let close = (x - y).abs() <= 1e-9 * x.abs().max(y.abs()).max(1.0);
            Some(match op {
                RelOp::Eq | RelOp::Approx => close,
                RelOp::Ne => !close,
                RelOp::Lt | RelOp::Gt if close => return None,
                RelOp::Lt => x < y,
                RelOp::Gt => x > y,
                RelOp::Le => x < y || close,
                RelOp::Ge => x > y || close,
            })
        }
        ExprNode::Logic(op, items) => {
            let vals: Vec<Option<bool>> = items.iter().map(|i| truth_at(i, env)).collect();
            match op {
                LogicOp::And => {
                    if vals.contains(&Some(false)) {
                        Some(false)
                    } else if vals.iter().all(|v| *v == Some(true)) {
                        Some(true)
                    } else {
                        None
                    }
                }
                LogicOp::Or => {
                    if vals.contains(&Some(true)) {
                        Some(true)
                    } else if vals.iter().all(|v| *v == Some(false)) {
                        Some(false)
                    } else {
                        None
                    }
                }
                LogicOp::Not => vals.first().copied().flatten().map(|v| !v),
            }
        }
        _ => None,
    }
}

/// A relation checked against `true` or `false`: decided exactly for numbers, and for
/// expressions with free names tried at the same random points as an equation (it
/// passes when it has the wanted truth value at every point tried).
fn check_truth(prop: &Expr, want: bool) -> Verdict {
    let mut names: Vec<String> = prop.free_symbols().iter().map(|s| s.to_string()).collect();
    names.sort();
    if names.is_empty() {
        // Exactly, when the difference of the two sides is an exact number.
        if let ExprNode::Relation(op, a, b) = prop.node() {
            let d = (a - b).simplify();
            if let Some(n) = d.as_number().filter(|n| n.is_exact()) {
                if let Some(sign) = n.sign() {
                    use mpp_core::RelOp;
                    use std::cmp::Ordering::{Equal, Greater, Less};
                    let holds = match op {
                        RelOp::Eq | RelOp::Approx => sign == Equal,
                        RelOp::Ne => sign != Equal,
                        RelOp::Lt => sign == Less,
                        RelOp::Le => sign != Greater,
                        RelOp::Gt => sign == Greater,
                        RelOp::Ge => sign != Less,
                    };
                    return truth_verdict(holds, want, Vec::new());
                }
            }
        }
        // Otherwise by the sign of the difference in high precision, when it is clear.
        if let ExprNode::Relation(op, a, b) = prop.node() {
            if let Some(sign) = closed_sign(a, b) {
                use mpp_core::RelOp;
                use std::cmp::Ordering::{Equal, Greater, Less};
                let holds = match op {
                    RelOp::Eq | RelOp::Approx => sign == Equal,
                    RelOp::Ne => sign != Equal,
                    RelOp::Lt => sign == Less,
                    RelOp::Le => sign != Greater,
                    RelOp::Gt => sign == Greater,
                    RelOp::Ge => sign != Less,
                };
                return truth_verdict(holds, want, Vec::new());
            }
        }
        return match truth_at(prop, &HashMap::new()) {
            Some(holds) => truth_verdict(holds, want, Vec::new()),
            None => Verdict::Unknown("the relation could not be decided".into()),
        };
    }
    // Special points first, then (for one variable) the points where the two sides of
    // a relation meet: `x² > 0` fails only at x = 0, which random points never hit.
    // Exact points are decided in exact arithmetic where possible, so that a touch
    // (`x² = 0` at x = 0) counts as equality and not as "too close to tell".
    let mut envs: Vec<TruthPoint> = special_points(&names, &[prop])
        .into_iter()
        .map(|p| {
            use num_traits::ToPrimitive;
            let f = p
                .iter()
                .map(|(n, q)| (n.clone(), q.to_f64().unwrap_or(f64::NAN)))
                .collect();
            (Some(p), f)
        })
        .collect();
    if let [var] = names.as_slice() {
        let mut meets = Vec::new();
        relation_meets(prop, var, &mut meets);
        envs.extend(
            meets
                .into_iter()
                .map(|x| (None, HashMap::from([(var.clone(), x)]))),
        );
    }
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut agreed = 0;
    let mut agreed_special = 0;
    let randoms = (0..ATTEMPTS).map(|a| (false, (None, random_point(&names, &mut rng, a))));
    for (special, (exact, env)) in envs.into_iter().map(|p| (true, p)).chain(randoms) {
        let decided = exact
            .as_ref()
            .and_then(|q| truth_exact(prop, q))
            .or_else(|| truth_at(prop, &env));
        let Some(holds) = decided else {
            continue;
        };
        if holds != want {
            let at = names
                .iter()
                .map(|n| (n.clone(), format_f64(env.get(n).copied().unwrap_or(0.0), 6)))
                .collect();
            return truth_verdict(holds, want, at);
        }
        if special {
            agreed_special += 1;
            continue;
        }
        agreed += 1;
        if agreed >= WANTED {
            break;
        }
    }
    if agreed == 0 {
        Verdict::Unknown("the relation could not be decided at any point".into())
    } else if let Some(name) = piecewise_name(prop) {
        Verdict::Unknown(piecewise_unknown(
            name,
            agreed + agreed_special,
            "the relation holds",
        ))
    } else {
        Verdict::Holds { points: agreed }
    }
}

/// A point a relation is tried at: exact when it is one of the special points.
type TruthPoint = (Option<HashMap<String, BigRational>>, HashMap<String, f64>);

/// The truth of a relation at an exact point, from the sign of the difference of its
/// sides in high precision; `None` when that cannot be found.
fn truth_exact(e: &Expr, vars: &HashMap<String, BigRational>) -> Option<bool> {
    use mpp_core::hp::{approx, Approx};
    use mpp_core::{LogicOp, RelOp};
    use std::cmp::Ordering::{Equal, Greater, Less};
    match e.node() {
        ExprNode::Bool(b) => Some(*b),
        ExprNode::Relation(op, a, b) => {
            let sign = match approx(&raw_difference(a, b), vars, 20)? {
                Approx::Zero { inexact: false, .. } => Equal,
                Approx::Value { value, .. } if value.neg => Less,
                Approx::Value { .. } => Greater,
                Approx::Zero { .. } => return None,
            };
            Some(match op {
                RelOp::Eq | RelOp::Approx => sign == Equal,
                RelOp::Ne => sign != Equal,
                RelOp::Lt => sign == Less,
                RelOp::Le => sign != Greater,
                RelOp::Gt => sign == Greater,
                RelOp::Ge => sign != Less,
            })
        }
        ExprNode::Logic(op, items) => {
            let vals: Option<Vec<bool>> = items.iter().map(|i| truth_exact(i, vars)).collect();
            let vals = vals?;
            match op {
                LogicOp::And => Some(vals.iter().all(|v| *v)),
                LogicOp::Or => Some(vals.iter().any(|v| *v)),
                LogicOp::Not => vals.first().map(|v| !v),
            }
        }
        _ => None,
    }
}

/// Where the two sides of each relation in `prop` meet (`a = b`), for one variable.
fn relation_meets(prop: &Expr, var: &str, out: &mut Vec<f64>) {
    match prop.node() {
        ExprNode::Relation(_, a, b) => {
            let d = (a - b).simplify();
            if let Ok(roots) = mpp_core::domains::solve::numeric::roots_in(&d, var, -20.0, 20.0) {
                out.extend(roots.into_iter().take(40));
            }
        }
        ExprNode::Logic(_, items) => {
            for i in items {
                relation_meets(i, var, out);
            }
        }
        _ => {}
    }
}

fn truth_verdict(holds: bool, want: bool, at: Vec<(String, String)>) -> Verdict {
    if holds == want {
        Verdict::Proved
    } else {
        Verdict::Fails {
            at,
            lhs: holds.to_string(),
            rhs: want.to_string(),
        }
    }
}

/// Checks an antiderivative: `∫ f dx` and an answer agree when they differ by a constant.
/// `vars` are the integration variables, `constants` the names of the constants of
/// integration (set to 0). `None` when they do not agree this way either.
pub fn check_up_to_constant(
    lhs: &Expr,
    rhs: &Expr,
    vars: &[String],
    constants: &[String],
) -> Option<Verdict> {
    if vars.is_empty() || !is_comparable(lhs) || !is_comparable(rhs) {
        return None;
    }
    let mut d = (lhs - rhs).simplify();
    for c in constants {
        d = d.subs(c, &Expr::zero());
    }
    let mut numeric = None;
    for v in vars {
        let dd = mpp_core::calc::diff(&d, v, 1, None).ok()?.simplify();
        let proved = dd.is_zero()
            || dd.expand().is_ok_and(|e| e.is_zero())
            || mpp_core::trig::identity_zero(&dd);
        if !proved {
            match spot_check(&dd, &Expr::zero()) {
                Verdict::Holds { points } => numeric = Some(points),
                _ => return None,
            }
        }
    }
    Some(match numeric {
        Some(points) => Verdict::Holds { points },
        None => Verdict::Proved,
    })
}

/// The integration variables of the indefinite integrals in `e`.
pub fn indefinite_variables(e: &Expr) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(e: &Expr, out: &mut Vec<String>) {
        if let ExprNode::Integral {
            var, bounds: None, ..
        } = e.node()
        {
            if !out.iter().any(|v| v == &**var) {
                out.push(var.to_string());
            }
        }
        for c in e.children() {
            walk(c, out);
        }
    }
    walk(e, &mut out);
    out
}

/// The first function in `e` that jumps or has a corner (`⌊x⌋`, `|x|`, `sgn x` …),
/// by the name a reader knows it by. Random points almost never land where such a
/// function changes rule, so agreement at random points proves nothing about it.
pub(crate) fn piecewise_name(e: &Expr) -> Option<&'static str> {
    use mpp_core::Func;
    let own = match e.node() {
        ExprNode::Func(f, _) => match f {
            Func::Floor => Some("⌊ ⌋"),
            Func::Ceil => Some("⌈ ⌉"),
            Func::Abs => Some("| |"),
            Func::Sign => Some("sgn"),
            Func::Min => Some("min"),
            Func::Max => Some("max"),
            _ => None,
        },
        ExprNode::Apply(name, _) => match &**name {
            "frac" => Some("frac"),
            "round" | "nint" => Some("round"),
            "mod" | "bmod" => Some("mod"),
            "heaviside" | "H" | "step" => Some("H"),
            "trunc" => Some("trunc"),
            _ => None,
        },
        ExprNode::Piecewise { .. } => Some("a piecewise definition"),
        _ => None,
    };
    own.or_else(|| e.children().into_iter().find_map(piecewise_name))
}

/// Values every variable is tried at before the random points: 0, the small integers
/// and halves (where `⌊ ⌋`, `sgn` and `| |` change rule) and points far out (where
/// square roots and logarithms of a difference change branch).
const SPECIAL: &[(i64, i64)] = &[
    (0, 1),
    (1, 1),
    (-1, 1),
    (2, 1),
    (-2, 1),
    (1, 2),
    (-1, 2),
    (3, 1),
    (-3, 1),
    (3, 2),
    (-5, 2),
    (1, 3),
    (10, 1),
    (-10, 1),
    (100, 1),
    (-100, 1),
];

fn ratio(p: i64, q: i64) -> BigRational {
    BigRational::new(BigInt::from(p), BigInt::from(q))
}

/// Where the argument `u` of a piecewise function reaches one of `targets`, when `u`
/// is linear in one variable (`⌊2x + 1⌋` jumps at x = −1/2, 0, 1/2, …).
fn linear_breaks(u: &Expr, targets: &[i64], out: &mut Vec<(String, BigRational)>) {
    let names = u.free_symbols();
    let [x] = names.iter().collect::<Vec<_>>()[..] else {
        return;
    };
    let rat = |e: &Expr| {
        e.simplify()
            .as_number()
            .and_then(|n| n.as_rational().cloned())
    };
    let Some(a) = mpp_core::calc::diff(u, x, 1, None)
        .ok()
        .and_then(|d| rat(&d))
    else {
        return;
    };
    let Some(b) = rat(&u.subs(x, &Expr::zero())) else {
        return;
    };
    if a == BigRational::from_integer(BigInt::from(0)) {
        return;
    }
    for k in targets {
        let v = (BigRational::from_integer(BigInt::from(*k)) - &b) / &a;
        out.push((x.to_string(), v));
    }
}

/// The points where the piecewise functions in `e` change rule, as far as they can be
/// found: integer values of a linear argument of `⌊ ⌋` and its kin, zeros of the
/// argument of `| |`, `sgn` and `H`.
fn break_points(e: &Expr, out: &mut Vec<(String, BigRational)>) {
    use mpp_core::Func;
    let jumps: Vec<i64> = (-3..=3).collect();
    match e.node() {
        ExprNode::Func(Func::Floor | Func::Ceil, args) => {
            for a in args {
                linear_breaks(a, &jumps, out);
            }
        }
        ExprNode::Func(Func::Abs | Func::Sign, args) => {
            for a in args {
                linear_breaks(a, &[0], out);
            }
        }
        ExprNode::Func(Func::Min | Func::Max, args) if args.len() == 2 => {
            linear_breaks(&(&args[0] - &args[1]), &[0], out);
        }
        ExprNode::Apply(name, args)
            if matches!(&**name, "frac" | "round" | "nint" | "trunc") && args.len() == 1 =>
        {
            linear_breaks(&args[0], &jumps, out);
        }
        ExprNode::Apply(name, args)
            if matches!(&**name, "heaviside" | "H" | "step") && args.len() == 1 =>
        {
            linear_breaks(&args[0], &[0], out);
        }
        _ => {}
    }
    for c in e.children() {
        break_points(c, out);
    }
}

/// The exact points a check is tried at before the random ones.
fn special_points(names: &[String], exprs: &[&Expr]) -> Vec<HashMap<String, BigRational>> {
    let mut out: Vec<HashMap<String, BigRational>> = Vec::new();
    if names.is_empty() {
        return out;
    }
    // Every variable at the same value, then mixed so that two variables differ.
    for (p, q) in SPECIAL {
        out.push(names.iter().map(|n| (n.clone(), ratio(*p, *q))).collect());
    }
    if names.len() > 1 {
        for k in 0..SPECIAL.len() {
            out.push(
                names
                    .iter()
                    .enumerate()
                    .map(|(i, n)| {
                        let (p, q) = SPECIAL[(k + 3 * i + 1) % SPECIAL.len()];
                        (n.clone(), ratio(p, q))
                    })
                    .collect(),
            );
        }
    }
    let mut breaks = Vec::new();
    for e in exprs {
        break_points(e, &mut breaks);
    }
    for (var, at) in breaks {
        // The other variables at an ordinary value.
        let mut point: HashMap<String, BigRational> =
            names.iter().map(|n| (n.clone(), ratio(7, 10))).collect();
        point.insert(var, at);
        if !out.contains(&point) {
            out.push(point);
        }
    }
    out
}

/// A random point, alternating between the whole range and positive values so square
/// roots and logarithms get points inside their domain.
fn random_point(names: &[String], rng: &mut Rng, attempt: usize) -> HashMap<String, f64> {
    names
        .iter()
        .map(|n| {
            let u = rng.next();
            let x = if attempt.is_multiple_of(2) {
                -3.0 + 6.0 * u
            } else {
                0.1 + 2.9 * u
            };
            (n.clone(), x)
        })
        .collect()
}

/// The result of trying a check at one point.
enum PointResult {
    Agree,
    Differ(Verdict),
    Skip,
}

fn try_point(
    lhs: &Expr,
    rhs: &Expr,
    names: &[String],
    exact: Option<&HashMap<String, BigRational>>,
    float: &HashMap<String, f64>,
) -> PointResult {
    use num_traits::ToPrimitive;
    // Binomial identities are tried on the usual domain, whole numbers 0 ≤ k ≤ n and
    // real n: at a negative whole number the extended definition breaks symmetry
    // (`C(n, k) = C(n, n − k)` fails at n = k = −1), which no textbook means.
    if (has_binomial(lhs) || has_binomial(rhs)) && {
        let env: HashMap<String, Complex64> = names
            .iter()
            .map(|n| {
                let x = match exact {
                    Some(q) => q[n].to_f64().unwrap_or(f64::NAN),
                    None => float.get(n).copied().unwrap_or(0.0),
                };
                (n.clone(), Complex64::new(x, 0.0))
            })
            .collect();
        binomial_off_domain(lhs, &env) || binomial_off_domain(rhs, &env)
    } {
        return PointResult::Skip;
    }
    let shown_at = || -> Vec<(String, String)> {
        names
            .iter()
            .map(|n| {
                let v = match exact {
                    Some(q) if float.is_empty() => q[n].to_string(),
                    _ => format_f64(float.get(n).copied().unwrap_or(0.0), 6),
                };
                (n.clone(), v)
            })
            .collect()
    };
    // High precision first: a gap far below f64's resolution still shows.
    match exact.and_then(|q| point_check(lhs, rhs, q)) {
        Some(true) => return PointResult::Agree,
        Some(false) => {
            if let Some((l, r)) = exact.and_then(|q| sides_shown(lhs, rhs, q)) {
                return PointResult::Differ(Verdict::Fails {
                    at: shown_at(),
                    lhs: l,
                    rhs: r,
                });
            }
        }
        None => {}
    }
    let env: HashMap<String, Complex64> = names
        .iter()
        .map(|n| {
            let x = match exact {
                Some(q) => q[n].to_f64().unwrap_or(f64::NAN),
                None => float.get(n).copied().unwrap_or(0.0),
            };
            (n.clone(), Complex64::new(x, 0.0))
        })
        .collect();
    let (Ok(l), Ok(r)) = (lhs.eval_complex(&env), rhs.eval_complex(&env)) else {
        return PointResult::Skip;
    };
    if !(l.re.is_finite() && l.im.is_finite() && r.re.is_finite() && r.im.is_finite()) {
        return PointResult::Skip;
    }
    let scale = l.norm().max(r.norm()).max(1.0);
    if names.is_empty() {
        // Two constants the high-precision evaluator cannot handle: agreement to
        // floating-point resolution is ≈ (not a proof), a gap well above it is ✗,
        // and a gap in between decides nothing.
        let gap = (l - r).norm() / l.norm().max(r.norm()).max(f64::MIN_POSITIVE);
        if gap <= 1e-12 {
            return PointResult::Differ(Verdict::Agrees {
                digits: FLOAT_DIGITS,
            });
        }
        if gap <= 1e-8 {
            return PointResult::Differ(Verdict::Unknown(format!(
                "the two sides agree to only about {} digits in floating point, which cannot decide it",
                (-gap.log10()).floor()
            )));
        }
    }
    if (l - r).norm() <= 1e-8 * scale {
        PointResult::Agree
    } else {
        PointResult::Differ(Verdict::Fails {
            at: shown_at(),
            lhs: format_complex(l, DIGITS),
            rhs: format_complex(r, DIGITS),
        })
    }
}

fn has_binomial(e: &Expr) -> bool {
    matches!(e.node(), ExprNode::Func(mpp_core::Func::Binomial, _))
        || e.children().iter().any(|c| has_binomial(c))
}

/// Whether a binomial coefficient in `e` has a negative whole number on top or at the
/// bottom at this point.
fn binomial_off_domain(e: &Expr, env: &HashMap<String, Complex64>) -> bool {
    let negative_whole = |x: &Expr| {
        x.eval_complex(env)
            .is_ok_and(|v| v.im == 0.0 && v.re < 0.0 && v.re.fract() == 0.0)
    };
    if let ExprNode::Func(mpp_core::Func::Binomial, args) = e.node() {
        if args.iter().any(negative_whole) {
            return true;
        }
    }
    e.children().iter().any(|c| binomial_off_domain(c, env))
}

fn spot_check(lhs: &Expr, rhs: &Expr) -> Verdict {
    let mut names: Vec<String> = lhs
        .free_symbols()
        .union(&rhs.free_symbols())
        .map(|s| s.to_string())
        .collect();
    names.sort();
    // Special points first (0, the integers, far-out values, the points where a
    // piecewise function changes rule): a false identity often fails only there.
    let mut agreed_special = 0;
    let none: HashMap<String, f64> = HashMap::new();
    for point in special_points(&names, &[lhs, rhs]) {
        match try_point(lhs, rhs, &names, Some(&point), &none) {
            PointResult::Agree => agreed_special += 1,
            PointResult::Differ(v) => return v,
            PointResult::Skip => {}
        }
    }
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut agreed = 0;
    let attempts = if names.is_empty() { 1 } else { ATTEMPTS };
    for attempt in 0..attempts {
        let float = random_point(&names, &mut rng, attempt);
        let exact: Option<HashMap<String, BigRational>> = float
            .iter()
            .map(|(n, x)| BigRational::from_float(*x).map(|q| (n.clone(), q)))
            .collect();
        match try_point(lhs, rhs, &names, exact.as_ref(), &float) {
            PointResult::Agree => {
                agreed += 1;
                if agreed >= WANTED {
                    break;
                }
            }
            PointResult::Differ(v) => return v,
            PointResult::Skip => {}
        }
    }
    if agreed == 0 {
        return Verdict::Unknown("neither side could be evaluated".into());
    }
    if let Some(name) = piecewise_name(lhs).or_else(|| piecewise_name(rhs)) {
        if prove_by_signs(lhs, rhs) {
            return Verdict::Proved;
        }
        return Verdict::Unknown(piecewise_unknown(
            name,
            agreed + agreed_special,
            "the two sides agree",
        ));
    }
    Verdict::Holds { points: agreed }
}

/// Why a check with a piecewise function that agreed at every point tried is still
/// undecided.
pub(crate) fn piecewise_unknown(name: &str, points: usize, what: &str) -> String {
    format!(
        "{what} at the {points} points tried, including 0 and the integers, \
         but {name} changes rule from point to point, so trying points cannot show it holds for every value"
    )
}

/// A proof by cases for identities with `| |`, `sgn` and `H` whose arguments are
/// products of powers of the variables (`|x y| = |x| |y|`, `sgn(x) |x| = x`): with the
/// sign of every variable fixed (−, 0 or +), each such function is a fixed multiple of
/// its argument, and the identity must then simplify to 0 = 0 in every case. A case
/// where a side is undefined (`x/|x|` at x = 0) is outside the domain.
fn prove_by_signs(lhs: &Expr, rhs: &Expr) -> bool {
    let names: Vec<String> = lhs
        .free_symbols()
        .union(&rhs.free_symbols())
        .map(|s| s.to_string())
        .collect();
    if names.is_empty() || names.len() > 3 {
        return false;
    }
    if !sign_split_ok(lhs) || !sign_split_ok(rhs) {
        return false;
    }
    let cases = 3usize.pow(names.len() as u32);
    for case in 0..cases {
        let mut signs: HashMap<String, i32> = HashMap::new();
        let mut c = case;
        for n in &names {
            signs.insert(n.clone(), (c % 3) as i32 - 1);
            c /= 3;
        }
        let zeros: Vec<(&str, Expr)> = names
            .iter()
            .filter(|n| signs[*n] == 0)
            .map(|n| (n.as_str(), Expr::zero()))
            .collect();
        let fixed = |e: &Expr| -> Option<Expr> {
            let e = if zeros.is_empty() {
                e.clone()
            } else {
                e.subs_all(&zeros)
            };
            replace_by_sign(&e, &signs).map(|e| e.simplify())
        };
        let (Some(l), Some(r)) = (fixed(lhs), fixed(rhs)) else {
            return false;
        };
        if has_undefined(&l) || has_undefined(&r) {
            continue;
        }
        let d = (&l - &r).simplify();
        if !(d.is_zero() || d.expand().is_ok_and(|d| d.is_zero())) {
            return false;
        }
    }
    true
}

fn has_undefined(e: &Expr) -> bool {
    e.is_undefined() || e.children().into_iter().any(has_undefined)
}

/// True when the only piecewise functions in `e` are `| |`, `sgn` and `H` of a product
/// of powers of variables.
fn sign_split_ok(e: &Expr) -> bool {
    use mpp_core::Func;
    if piecewise_name(e).is_none() {
        return true;
    }
    match e.node() {
        ExprNode::Func(Func::Abs | Func::Sign, args) => args.len() == 1 && monomial(&args[0]),
        ExprNode::Apply(name, args) if matches!(&**name, "heaviside" | "H" | "step") => {
            args.len() == 1 && monomial(&args[0])
        }
        ExprNode::Func(Func::Floor | Func::Ceil | Func::Min | Func::Max, _)
        | ExprNode::Piecewise { .. } => false,
        ExprNode::Apply(name, _) if piecewise_name(&Expr::apply(name, vec![])).is_some() => false,
        _ => e.children().into_iter().all(sign_split_ok),
    }
}

/// A number times a product of variables raised to whole powers.
fn monomial(e: &Expr) -> bool {
    match e.node() {
        ExprNode::Num(n) => n.is_exact(),
        ExprNode::Sym(_) => true,
        ExprNode::Pow(b, x) => {
            b.as_sym().is_some()
                && x.as_number()
                    .and_then(mpp_core::Number::as_integer)
                    .is_some()
        }
        ExprNode::Mul(items) => items.iter().all(monomial),
        _ => false,
    }
}

/// The sign of a monomial when each variable has the sign given (`None` if unknown).
fn monomial_sign(e: &Expr, signs: &HashMap<String, i32>) -> Option<i32> {
    use num_traits::Signed;
    match e.node() {
        ExprNode::Num(n) => Some(match n.sign()? {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }),
        ExprNode::Sym(s) => signs.get(&**s).copied(),
        ExprNode::Pow(b, x) => {
            let s = monomial_sign(b, signs)?;
            let k = x.as_number().and_then(mpp_core::Number::as_integer)?;
            if s == 0 {
                return if k.is_positive() { Some(0) } else { None };
            }
            Some(if (&k % 2u32) == BigInt::from(0) { 1 } else { s })
        }
        ExprNode::Mul(items) => items
            .iter()
            .try_fold(1, |acc, i| monomial_sign(i, signs).map(|s| acc * s)),
        _ => None,
    }
}

/// `e` with every `| |`, `sgn` and `H` replaced by its value for the signs given.
fn replace_by_sign(e: &Expr, signs: &HashMap<String, i32>) -> Option<Expr> {
    use mpp_core::Func;
    let arg_sign = |args: &[Expr]| -> Option<(Expr, i32)> {
        let u = args.first()?.simplify();
        let s = monomial_sign(&u, signs)?;
        Some((u, s))
    };
    match e.node() {
        ExprNode::Func(Func::Abs, args) => {
            let (u, s) = arg_sign(args)?;
            Some(Expr::int(i64::from(s)) * u)
        }
        ExprNode::Func(Func::Sign, args) => Some(Expr::int(i64::from(arg_sign(args)?.1))),
        ExprNode::Apply(name, args) if matches!(&**name, "heaviside" | "H" | "step") => {
            match arg_sign(args)?.1 {
                1 => Some(Expr::one()),
                -1 => Some(Expr::zero()),
                // The value at the jump depends on the convention.
                _ => Some(Expr::undefined()),
            }
        }
        _ => {
            let mut failed = false;
            let node = e.map_children(&mut |c| {
                replace_by_sign(c, signs).unwrap_or_else(|| {
                    failed = true;
                    c.clone()
                })
            });
            (!failed).then(|| Expr::new(node))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdicts() {
        let x = Expr::sym("x");
        let sin2 = Expr::call("sin", vec![x.clone()]).unwrap().pow(2);
        let cos2 = Expr::call("cos", vec![x.clone()]).unwrap().pow(2);
        assert_eq!(check(&(sin2 + cos2), &Expr::one()).passed(), Some(true));
        let sq = (x.clone() + Expr::one()).pow(2);
        let wrong = x.pow(2) + Expr::one();
        let v = check(&sq, &wrong);
        assert_eq!(v.passed(), Some(false));
        assert!(v.describe().contains("at x = "));
        assert_eq!(check(&Expr::int(2), &Expr::int(3)).mark(), "✗");
    }
}
