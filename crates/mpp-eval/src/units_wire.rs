//! Units in documents: quantities flow through evaluation as ordinary expressions with
//! unit factors, and are checked and shown with [`mpp_core::units`].
//!
//! A quantity `9.81\,\mathrm{m/s^2}` lowers to `9.81 · ⟨m/s^2⟩`, where `⟨m/s^2⟩` is a
//! unit factor: an `Apply` node named `unit:m/s^2` with no arguments. The simplifier
//! treats unit factors as constants, so products, powers, substitution, derivatives and
//! integrals carry them along unchanged (`d/dt` of `3 ⟨m/s⟩ t` is `3 ⟨m/s⟩`, and a
//! function evaluated at `2 ⟨s⟩` picks up the seconds). Unit factors never collide with
//! user names: `unit:m` can't be written as a name.
//!
//! When a value is shown, [`fold`] reads it back as a [`Quantity`], which is where
//! dimensions are checked ("can't add m and s", "sin needs a dimensionless argument")
//! and units simplified (`kg·m/s²` → `N`). Free names have no known dimension: a sum
//! that mixes them with quantities of another kind is left as written rather than
//! reported, since the name may well carry the missing unit.
//!
//! Conversions (`v \operatorname{in} \mathrm{km/h}`) are holders evaluated after
//! resolution; their result keeps the requested unit exactly. A definite integral whose
//! bounds carry a unit is integrated over the plain values with the unit factored out
//! (`∫_{a u}^{b u} f(t) dt = u ∫_a^b f(τ u) dτ`), and bounds of different kinds are an
//! error.

use mpp_core::units::{parse_unit, Quantity, Unit};
use mpp_core::{Expr, ExprNode, Func, MathError, MathResult, RelOp};

use crate::holders::{evaluate_holder, Ctx};

/// Name prefix of a unit factor.
pub const UNIT_PREFIX: &str = "unit:";
/// `unit convert(expr, ⟨target⟩)`: the `in` operator before evaluation.
pub const CONVERT: &str = "unit convert";
/// `unit as(value, ⟨unit⟩)`: a value expressed in exactly this unit (a conversion result).
pub const AS: &str = "unit as";

/// The unit factor for `u` (`1` for the dimensionless unit).
pub fn factor(u: &Unit) -> Expr {
    if u.is_one() {
        return Expr::one();
    }
    Expr::apply(&format!("{UNIT_PREFIX}{}", u.text()), Vec::new())
}

/// The unit of a unit factor.
pub fn unit_of_factor(e: &Expr) -> Option<Unit> {
    match e.node() {
        ExprNode::Apply(name, args) if args.is_empty() => {
            parse_unit(name.strip_prefix(UNIT_PREFIX)?).ok()
        }
        _ => None,
    }
}

/// True when `e` holds a unit factor or a conversion anywhere.
pub fn contains_units(e: &Expr) -> bool {
    match e.node() {
        ExprNode::Apply(name, _)
            if name.starts_with(UNIT_PREFIX) || &**name == CONVERT || &**name == AS =>
        {
            true
        }
        _ => e.children().into_iter().any(contains_units),
    }
}

/// `value unit` as an expression. A zero is kept with its unit (`0 °C` is not `0 K`,
/// and `0 s` is a time), where a product would collapse to a bare `0`.
pub fn quantity_expr(value: Expr, unit_text: &str) -> MathResult<Expr> {
    let u = parse_unit(unit_text)?;
    if u.is_one() {
        return Ok(value);
    }
    if value.simplify().is_zero() {
        return Ok(Expr::apply(AS, vec![Expr::zero(), factor(&u)]));
    }
    Ok(value * factor(&u))
}

/// `expr in unit` before evaluation.
pub fn convert_expr(expr: Expr, unit_text: &str) -> MathResult<Expr> {
    let u = parse_unit(unit_text)?;
    Ok(Expr::apply(CONVERT, vec![expr, factor(&u)]))
}

// ---- reading values back as quantities ------------------------------------------

/// A value read as a quantity.
#[derive(Debug, Clone, PartialEq)]
pub enum Folded {
    /// A quantity; `certain` is false when free names are involved (their dimension is
    /// not known, so they were taken as plain numbers).
    Known {
        q: Quantity,
        certain: bool,
        /// The unit is kept as it is: it was asked for (a conversion), or it is the one
        /// unit the value was written with (`9.81 m/s^2` stays in m/s², not N/kg).
        exact: bool,
    },
    /// Units mixed with names in a way that can't be read as one quantity.
    Unknown,
}

impl Folded {
    fn plain(e: Expr) -> Folded {
        let certain = e.free_symbols().is_empty();
        Folded::Known {
            q: Quantity::number(e),
            certain,
            exact: false,
        }
    }
}

fn known(q: Quantity, certain: bool) -> Folded {
    Folded::Known {
        q,
        certain,
        exact: false,
    }
}

/// Reads `e` as a quantity, checking dimensions on the way.
pub fn fold(e: &Expr) -> MathResult<Folded> {
    if !contains_units(e) {
        return Ok(Folded::plain(e.clone()));
    }
    match e.node() {
        ExprNode::Apply(name, args) if name.starts_with(UNIT_PREFIX) && args.is_empty() => {
            let u = unit_of_factor(e).ok_or_else(|| {
                MathError::Dimension(format!(
                    "`{}` is not a known unit",
                    &name[UNIT_PREFIX.len()..]
                ))
            })?;
            Ok(Folded::Known {
                q: Quantity::new(Expr::one(), u),
                certain: true,
                exact: true,
            })
        }
        ExprNode::Apply(name, args) if &**name == AS => {
            let [value, unit] = args.as_slice() else {
                return Ok(Folded::Unknown);
            };
            let Some(u) = unit_of_factor(unit) else {
                return Ok(Folded::Unknown);
            };
            Ok(Folded::Known {
                q: Quantity::new(value.clone(), u),
                certain: value.free_symbols().is_empty(),
                exact: true,
            })
        }
        ExprNode::Apply(name, _) if &**name == CONVERT => Ok(Folded::Unknown),
        ExprNode::Add(terms) => fold_sum(terms),
        ExprNode::Mul(factors) => fold_product(factors),
        ExprNode::Pow(base, exp) => fold_power(base, exp),
        ExprNode::Func(f, args) => fold_function(*f, args),
        // A special function (`K(sin 15°)`, `W(x)`): every argument dimensionless.
        ExprNode::Apply(name, args) if !args.is_empty() => {
            let mut plain = Vec::with_capacity(args.len());
            let mut all_certain = true;
            for a in args {
                let Folded::Known { q, certain, .. } = fold(a)? else {
                    return Ok(Folded::Unknown);
                };
                if !q.unit.is_dimensionless() {
                    if !certain {
                        return Ok(Folded::Unknown);
                    }
                    return Err(q.require_dimensionless(name).err().unwrap_or_else(|| {
                        MathError::Dimension(format!("{name} needs dimensionless arguments"))
                    }));
                }
                all_certain &= certain;
                plain.push(q.require_dimensionless(name)?);
            }
            Ok(known(
                Quantity::number(Expr::apply(name, plain)),
                all_certain,
            ))
        }
        _ => Ok(Folded::Unknown),
    }
}

fn fold_sum(terms: &[Expr]) -> MathResult<Folded> {
    let mut acc: Option<(Quantity, bool)> = None;
    let mut as_written = true;
    // A temperature on a scale (°C, °F) goes first: the other terms are differences
    // added to it (20 °C + 5 K = 25 °C), whatever order the sum is stored in.
    let mut ordered: Vec<&Expr> = terms.iter().collect();
    if let Some(i) = terms.iter().position(is_scale_temperature) {
        let t = ordered.remove(i);
        ordered.insert(0, t);
    }
    for t in ordered {
        let Folded::Known { q, certain, exact } = fold(t)? else {
            return Ok(Folded::Unknown);
        };
        as_written &= exact && acc.as_ref().is_none_or(|(a, _)| a.unit == q.unit);
        acc = Some(match acc {
            None => (q, certain),
            Some((a, c)) => {
                if !a.same_dimension(&q) {
                    if c && certain {
                        // Name the term that doesn't fit, as written.
                        return Err(a.add(&q).err().unwrap_or_else(|| {
                            MathError::Dimension("these units don't fit together".into())
                        }));
                    }
                    return Ok(Folded::Unknown);
                }
                (a.add(&q)?, c && certain)
            }
        });
    }
    Ok(match acc {
        Some((q, c)) => Folded::Known {
            q,
            certain: c,
            exact: as_written,
        },
        None => Folded::plain(Expr::zero()),
    })
}

/// True for a term in °C or °F standing alone (a point on the scale).
fn is_scale_temperature(e: &Expr) -> bool {
    matches!(
        fold(e),
        Ok(Folded::Known { q, .. }) if q.unit.offset().is_some()
    )
}

fn fold_product(factors: &[Expr]) -> MathResult<Folded> {
    let mut acc = Quantity::number(Expr::one());
    let mut all_certain = true;
    // `3 · ⟨km/h⟩`: one written unit times numbers keeps that unit as written.
    let mut with_units = 0;
    let mut as_written = true;
    for f in factors {
        let Folded::Known { q, certain, exact } = fold(f)? else {
            return Ok(Folded::Unknown);
        };
        all_certain &= certain;
        if !q.unit.is_one() {
            with_units += 1;
            as_written &= exact;
            if with_units == 1 && acc.unit.is_one() {
                // Keep the unit exactly as written (no simplification yet).
                acc = Quantity::new((&acc.value * &q.value).simplify(), q.unit.clone());
                continue;
            }
        }
        acc = acc.mul(&q)?;
    }
    Ok(Folded::Known {
        q: acc,
        certain: all_certain,
        exact: with_units == 1 && as_written,
    })
}

fn fold_power(base: &Expr, exp: &Expr) -> MathResult<Folded> {
    let Folded::Known {
        q: e, certain: ec, ..
    } = fold(exp)?
    else {
        return Ok(Folded::Unknown);
    };
    if !e.unit.is_one() && !e.unit.is_dimensionless() {
        if !ec {
            return Ok(Folded::Unknown);
        }
        // `e^{2 m}`: the exponential function, named as the reader wrote it.
        let what = if matches!(base.node(), ExprNode::Const(mpp_core::Constant::E)) {
            "exp"
        } else {
            "an exponent"
        };
        return Err(e.require_dimensionless(what).err().unwrap_or_else(|| {
            MathError::Dimension(format!("{what} needs a dimensionless argument"))
        }));
    }
    let Folded::Known {
        q: b, certain: bc, ..
    } = fold(base)?
    else {
        return Ok(Folded::Unknown);
    };
    if b.unit.is_one() {
        let k = e.require_dimensionless("an exponent")?;
        return Ok(known(Quantity::number(b.value.pow(k)), bc && ec));
    }
    match b.pow(&e) {
        Ok(q) => Ok(known(q, bc && ec)),
        // A symbolic power of a quantity: its unit can't be written down.
        Err(_) if !ec => Ok(Folded::Unknown),
        Err(err) => Err(err),
    }
}

/// The value of a concentration (amount per volume) in mol/L: the chemists' convention
/// for its logarithm (`pH = −log₁₀[H⁺]`, with `[H⁺]` in mol/L). `None` when `q` is
/// not a concentration.
fn concentration_in_mol_per_litre(q: &Quantity) -> Option<Expr> {
    let mol_per_litre = parse_unit("mol/L").ok()?;
    if q.unit.dimension() != mol_per_litre.dimension() {
        return None;
    }
    q.convert_to(&mol_per_litre)
        .ok()
        .map(|c| c.value.simplify())
}

/// What a logarithm of a concentration means, for the hover.
pub const LOG_OF_CONCENTRATION: &str =
    "the logarithm of a concentration is taken of its value in mol/L";

/// True when `e` takes a logarithm of a concentration somewhere.
pub fn has_log_of_concentration(e: &Expr) -> bool {
    match e.node() {
        ExprNode::Func(Func::Ln | Func::Log, args) => {
            matches!(
                args.first().map(fold),
                Some(Ok(Folded::Known { ref q, certain: true, .. }))
                    if concentration_in_mol_per_litre(q).is_some()
            ) || args.iter().any(has_log_of_concentration)
        }
        _ => e.children().into_iter().any(has_log_of_concentration),
    }
}

fn fold_function(f: Func, args: &[Expr]) -> MathResult<Folded> {
    // `log₁₀ c` of a concentration: the logarithm of its value in mol/L.
    if let (Func::Ln | Func::Log, Some(arg)) = (f, args.first()) {
        if let Folded::Known {
            q, certain: true, ..
        } = fold(arg)?
        {
            if let Some(value) = concentration_in_mol_per_litre(&q) {
                let mut plain = vec![value];
                for base in &args[1..] {
                    let Folded::Known { q, certain, .. } = fold(base)? else {
                        return Ok(Folded::Unknown);
                    };
                    plain.push(q.require_dimensionless("the base of a logarithm")?);
                    if !certain {
                        return Ok(Folded::Unknown);
                    }
                }
                return Ok(known(Quantity::number(Expr::func(f, plain)?), true));
            }
        }
    }
    let [arg] = args else {
        // min, max, gcd … of quantities: every argument in one dimension.
        return fold_many(f, args);
    };
    let Folded::Known { q, certain, .. } = fold(arg)? else {
        return Ok(Folded::Unknown);
    };
    if !certain && !q.unit.is_dimensionless() {
        return Ok(Folded::Unknown);
    }
    Ok(known(q.apply_function(f.name())?, certain))
}

fn fold_many(f: Func, args: &[Expr]) -> MathResult<Folded> {
    if !matches!(f, Func::Min | Func::Max) {
        let mut plain = Vec::with_capacity(args.len());
        for a in args {
            let Folded::Known { q, certain, .. } = fold(a)? else {
                return Ok(Folded::Unknown);
            };
            if !certain {
                return Ok(Folded::Unknown);
            }
            plain.push(q.require_dimensionless(f.name())?);
        }
        return Ok(Folded::plain(Expr::func(f, plain)?));
    }
    let mut first: Option<Quantity> = None;
    let mut values = Vec::with_capacity(args.len());
    for a in args {
        let Folded::Known { q, certain, .. } = fold(a)? else {
            return Ok(Folded::Unknown);
        };
        if !certain {
            return Ok(Folded::Unknown);
        }
        let q = match &first {
            None => {
                first = Some(q.clone());
                q
            }
            Some(u) => {
                if !u.same_dimension(&q) {
                    return Err(MathError::Dimension(format!(
                        "can't compare {} and {}",
                        u.unit, q.unit
                    )));
                }
                q.convert_to(&u.unit)?
            }
        };
        values.push(q.value);
    }
    let unit = first.map(|q| q.unit).unwrap_or_default();
    Ok(known(Quantity::new(Expr::func(f, values)?, unit), true))
}

/// A list of quantities as plain values in the first entry's unit, and that unit;
/// `None` when the list has no units.
pub fn strip_list(e: &Expr) -> MathResult<Option<(Vec<Expr>, Unit)>> {
    let ExprNode::Tuple(items) = e.node() else {
        return Ok(None);
    };
    if !contains_units(e) {
        return Ok(None);
    }
    let mut unit: Option<Unit> = None;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let Folded::Known { q, .. } = fold(item)? else {
            return Err(MathError::Dimension(format!(
                "the units of {} are not known",
                show_raw(item).0
            )));
        };
        let q = match &unit {
            None => {
                unit = Some(q.unit.clone());
                q
            }
            Some(u) => q.convert_to(u)?,
        };
        out.push(q.value);
    }
    Ok(unit.map(|u| (out, u)))
}

// ---- the holder -----------------------------------------------------------------

/// Conversions, and definite integrals whose bounds carry units.
pub fn evaluate(e: &Expr, ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
    match e.node() {
        ExprNode::Apply(name, args) if &**name == CONVERT => Some(convert(args)),
        ExprNode::Integral {
            expr,
            var,
            bounds: Some((lo, hi)),
        } if contains_units(lo) || contains_units(hi) => integral(expr, var, lo, hi, ctx),
        _ => None,
    }
}

/// Asked before the other solvers: an equation to solve that carries units.
pub fn evaluate_solve(e: &Expr, ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
    match e.node() {
        ExprNode::Apply(name, args) if &**name == "operatorname" => solve_with_units(args, ctx),
        _ => None,
    }
}

/// `\operatorname{solve}_t (V_C(t) = 0.99\,V_0)` when the equation carries units: the
/// unknown gets the unit that makes the equation consistent (tried from the units in
/// it), both sides are measured in SI units, and the plain equation is solved; each
/// root comes back with that unit (`4.605 s`).
fn solve_with_units(args: &[Expr], ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
    let [which, sub, _sup, body] = args else {
        return None;
    };
    if which.as_sym() != Some("solve") {
        return None;
    }
    let var = sub.as_sym()?;
    let ExprNode::Tuple(items) = body.node() else {
        return None;
    };
    let [eq] = items.as_slice() else {
        return None;
    };
    let ExprNode::Relation(RelOp::Eq, l, r) = eq.node() else {
        return None;
    };
    if !contains_units(eq) {
        return None;
    }
    let difference = (l.clone() - r.clone()).simplify();
    // The units met in the equation, their reciprocals, and none at all.
    let mut candidates = vec![Unit::one()];
    collect_units(&difference, &mut candidates);
    let x = "#x";
    for unit in candidates {
        let trial = difference.subs(var, &(Expr::sym(x) * factor(&unit)));
        let Ok(Folded::Known { q, .. }) = fold(&trial) else {
            continue;
        };
        // The value of the difference in SI units, as a plain function of x.
        let plain = (q.value.clone() * q.unit.scale()).simplify();
        if plain.free_symbols().iter().any(|s| **s != *x) {
            return None;
        }
        let call = Expr::apply(
            "operatorname",
            vec![
                Expr::sym("solve"),
                Expr::sym(x),
                Expr::new(ExprNode::Tuple(Vec::new())),
                Expr::new(ExprNode::Tuple(vec![Expr::equation(plain, Expr::zero())])),
            ],
        );
        let roots = match evaluate_holder(&call, ctx) {
            Ok(v) => v,
            Err(e) => return Some(Err(e)),
        };
        let with_unit = |v: &Expr| (v.clone() * factor(&unit)).simplify();
        return Some(Ok(match roots.node() {
            ExprNode::Set(vs) => Expr::new(ExprNode::Set(vs.iter().map(with_unit).collect())),
            ExprNode::Tuple(vs) => Expr::new(ExprNode::Tuple(vs.iter().map(with_unit).collect())),
            _ => with_unit(&roots),
        }));
    }
    None
}

/// The units (made coherent) of the parts of `e` that carry one, and their reciprocals.
fn collect_units(e: &Expr, out: &mut Vec<Unit>) {
    if let Ok(Folded::Known { q, .. }) = fold(e) {
        if !q.unit.is_one() {
            let u = q.simplified().unit;
            for cand in [u.clone(), Unit::one().div(&u).unwrap_or_else(|_| u.clone())] {
                if !out.contains(&cand) {
                    out.push(cand);
                }
            }
        }
    }
    for c in e.children() {
        collect_units(c, out);
    }
}

fn convert(args: &[Expr]) -> MathResult<Expr> {
    let [value, target] = args else {
        return Err(MathError::WrongArity {
            function: "in".into(),
            expected: "2".into(),
            got: args.len(),
        });
    };
    let target_unit = unit_of_factor(target)
        .ok_or_else(|| MathError::Dimension("the target of `in` is not a unit".into()))?;
    let q = match fold(value)? {
        Folded::Known { q, .. } => q,
        Folded::Unknown => {
            return Err(MathError::Dimension(format!(
                "the units of {} are not known, so it can't be converted to {target_unit}",
                show_raw(value).0
            )))
        }
    };
    let converted = q.convert_to(&target_unit)?;
    Ok(Expr::apply(AS, vec![converted.value, target.clone()]))
}

/// `∫_{a u}^{b u} f(t) dt = u ∫_a^b f(τ u) dτ`.
fn integral(
    body: &Expr,
    var: &str,
    lo: &Expr,
    hi: &Expr,
    ctx: &Ctx<'_>,
) -> Option<MathResult<Expr>> {
    let (
        Ok(Folded::Known {
            q: a, certain: ca, ..
        }),
        Ok(Folded::Known {
            q: b, certain: cb, ..
        }),
    ) = (fold(lo), fold(hi))
    else {
        return None;
    };
    if !(ca && cb) {
        return None;
    }
    // A plain `0` fits any unit; `0 s` is in seconds.
    let zero = |q: &Quantity| q.unit.is_one() && mpp_core::domains::util::is_zero(&q.value);
    let unit = if !b.unit.is_one() { &b.unit } else { &a.unit };
    if !a.same_dimension(&b) && !zero(&a) && !zero(&b) {
        return Some(Err(MathError::Dimension(format!(
            "the bounds of this integral don't match: one is in {} and the other in {}",
            a.unit, b.unit
        ))));
    }
    let plain = |q: &Quantity| -> MathResult<Expr> {
        if zero(q) {
            Ok(Expr::zero())
        } else {
            mpp_core::units::calculus::bound_value(q, unit)
        }
    };
    let (lo_v, hi_v) = match (plain(&a), plain(&b)) {
        (Ok(x), Ok(y)) => (x, y),
        (Err(e), _) | (_, Err(e)) => return Some(Err(e)),
    };
    let u = factor(unit);
    let scaled = body.subs(var, &(Expr::sym(var) * u.clone()));
    let inner = Expr::new(ExprNode::Integral {
        expr: scaled,
        var: var.into(),
        bounds: Some((lo_v, hi_v)),
    });
    Some(evaluate_holder(&inner, ctx).map(|v| (v * u).simplify()))
}

// ---- presentation ------------------------------------------------------------------

/// How a value with units is shown.
#[derive(Debug, Clone, PartialEq)]
pub struct Presented {
    pub text: String,
    pub latex: String,
    /// The decimal value with its unit, when the exact value is not a plain fraction.
    pub approx: Option<String>,
}

/// Checks the dimensions of a result; an error names the units that don't fit.
pub fn check(e: &Expr) -> MathResult<()> {
    if contains_units(e) {
        check_inside(e)?;
    }
    Ok(())
}

fn check_inside(e: &Expr) -> MathResult<()> {
    match e.node() {
        ExprNode::Relation(op, a, b) => {
            relation(*op, a, b)?;
            Ok(())
        }
        ExprNode::Tuple(items) | ExprNode::Set(items) | ExprNode::Matrix { data: items, .. } => {
            items.iter().try_for_each(check)
        }
        _ => fold(e).map(|_| ()),
    }
}

/// `a op b` for quantities: decided when both sides are known numbers.
pub(crate) fn relation(op: RelOp, a: &Expr, b: &Expr) -> MathResult<Option<bool>> {
    let (
        Folded::Known {
            q: x, certain: cx, ..
        },
        Folded::Known {
            q: y, certain: cy, ..
        },
    ) = (fold(a)?, fold(b)?)
    else {
        return Ok(None);
    };
    if !(cx && cy) {
        return Ok(None);
    }
    use std::cmp::Ordering::{Equal, Greater, Less};
    let Some(ord) = x.compare(&y)? else {
        return Ok(None);
    };
    Ok(Some(match op {
        RelOp::Eq | RelOp::Approx => ord == Equal,
        RelOp::Ne => ord != Equal,
        RelOp::Lt => ord == Less,
        RelOp::Le => ord != Greater,
        RelOp::Gt => ord == Greater,
        RelOp::Ge => ord != Less,
    }))
}

/// The presentation of a value with units, or `None` for a value without any.
pub fn present(e: &Expr) -> MathResult<Option<Presented>> {
    if !contains_units(e) {
        return Ok(None);
    }
    if let ExprNode::Relation(op, a, b) = e.node() {
        if let Some(truth) = relation(*op, a, b)? {
            let t = truth.to_string();
            return Ok(Some(Presented {
                text: t.clone(),
                latex: format!("\\text{{{t}}}"),
                approx: None,
            }));
        }
    }
    // A list, set or interval of quantities: each entry shown as a quantity.
    if let Some(p) = present_each(e)? {
        return Ok(Some(p));
    }
    let shown = match fold(e)? {
        Folded::Known { q, exact, .. } => {
            let q = if exact { q } else { q.simplified() };
            let approx = approx_of(&q);
            Presented {
                text: q.to_string(),
                latex: q.latex(),
                approx,
            }
        }
        Folded::Unknown => {
            let (text, latex) = show_raw(e);
            Presented {
                text,
                latex,
                approx: None,
            }
        }
    };
    Ok(Some(shown))
}

/// Entries of a tuple, set or interval shown one by one: `(150 cm, 160 cm)`.
fn present_each(e: &Expr) -> MathResult<Option<Presented>> {
    let one = |x: &Expr| -> MathResult<(String, String)> {
        Ok(match present(x)? {
            Some(p) => (p.text, p.latex),
            None => (x.to_string(), x.to_latex()),
        })
    };
    let (parts, open, close, lopen, lclose): (Vec<&Expr>, &str, &str, &str, &str) = match e.node() {
        ExprNode::Tuple(items) => (items.iter().collect(), "(", ")", "\\left(", "\\right)"),
        ExprNode::Set(items) => (items.iter().collect(), "{", "}", "\\left\\{", "\\right\\}"),
        ExprNode::Interval {
            lo,
            hi,
            lo_closed,
            hi_closed,
        } => (
            vec![lo, hi],
            if *lo_closed { "[" } else { "(" },
            if *hi_closed { "]" } else { ")" },
            if *lo_closed { "\\left[" } else { "\\left(" },
            if *hi_closed { "\\right]" } else { "\\right)" },
        ),
        _ => return Ok(None),
    };
    let mut text = Vec::with_capacity(parts.len());
    let mut latex = Vec::with_capacity(parts.len());
    for p in parts {
        let (t, l) = one(p)?;
        text.push(t);
        latex.push(l);
    }
    Ok(Some(Presented {
        text: format!("{open}{}{close}", text.join(", ")),
        latex: format!("{lopen}{}{lclose}", latex.join(", ")),
        approx: None,
    }))
}

/// `686.7 N` for a quantity whose exact value is not a plain fraction or integer.
fn approx_of(q: &Quantity) -> Option<String> {
    if !q.value.is_constant() || crate::numeric::is_exact_rational(&q.value) {
        return None;
    }
    let digits = crate::numeric::approximate(&q.value).ok()?;
    Some(if q.unit.is_one() {
        digits
    } else if q.unit.text() == "°" {
        format!("{digits}°")
    } else {
        format!("{digits} {}", q.unit)
    })
}

/// The decimal value of a quantity, unit included (`≈ ?` with units).
pub fn approximate(e: &Expr) -> MathResult<Option<String>> {
    // An interval of quantities (a confidence interval): each end.
    if let ExprNode::Interval {
        lo,
        hi,
        lo_closed,
        hi_closed,
    } = e.node()
    {
        let end = |x: &Expr| -> MathResult<Option<String>> {
            if contains_units(x) {
                approximate(x)
            } else {
                Ok(crate::numeric::approximate(x).ok())
            }
        };
        let (Some(a), Some(b)) = (end(lo)?, end(hi)?) else {
            return Ok(None);
        };
        return Ok(Some(format!(
            "{}{a}, {b}{}",
            if *lo_closed { "[" } else { "(" },
            if *hi_closed { "]" } else { ")" }
        )));
    }
    // A set or list of quantities (the roots of an equation with units): each entry.
    if let ExprNode::Set(items) | ExprNode::Tuple(items) = e.node() {
        if !items.iter().any(contains_units) {
            return Ok(None);
        }
        let mut parts = Vec::with_capacity(items.len());
        for item in items {
            let shown = if contains_units(item) {
                approximate(item)?
            } else {
                crate::numeric::approximate(item).ok()
            };
            let Some(s) = shown else {
                return Ok(None);
            };
            parts.push(s);
        }
        let (open, close) = if matches!(e.node(), ExprNode::Set(_)) {
            ("{", "}")
        } else {
            ("(", ")")
        };
        return Ok(Some(format!("{open}{}{close}", parts.join(", "))));
    }
    let Folded::Known { q, exact, .. } = fold(e)? else {
        return Ok(None);
    };
    let q = if exact { q } else { q.simplified() };
    if !q.value.is_constant() {
        return Ok(None);
    }
    let Ok(digits) = crate::numeric::approximate(&q.value) else {
        return Ok(None);
    };
    Ok(Some(if q.unit.is_one() {
        digits
    } else if q.unit.text() == "°" {
        format!("{digits}°")
    } else {
        format!("{digits} {}", q.unit)
    }))
}

/// Text and LaTeX of an expression with unit factors written as units.
fn show_raw(e: &Expr) -> (String, String) {
    let mut units: Vec<Unit> = Vec::new();
    let replaced = replace_factors(e, &mut units);
    let mut text = replaced.to_string();
    let mut latex = replaced.to_latex();
    for (i, u) in units.iter().enumerate() {
        let key = placeholder(i);
        text = text.replace(&key, &u.text());
        latex = latex.replace(&format!("\\mathrm{{{key}}}"), &u.latex());
        latex = latex.replace(&key, &u.latex());
    }
    (text, latex)
}

fn placeholder(i: usize) -> String {
    format!("zzunit{i}zz")
}

fn replace_factors(e: &Expr, units: &mut Vec<Unit>) -> Expr {
    if let Some(u) = unit_of_factor(e) {
        let i = match units.iter().position(|x| *x == u) {
            Some(i) => i,
            None => {
                units.push(u);
                units.len() - 1
            }
        };
        return Expr::sym(&placeholder(i));
    }
    match e.node() {
        ExprNode::Apply(name, args) if &**name == AS => match args.as_slice() {
            [v, u] => replace_factors(&(v * u), units),
            _ => e.clone(),
        },
        _ => Expr::new(e.map_children(&mut |c| replace_factors(c, units))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(v: i64, u: &str) -> Expr {
        quantity_expr(Expr::int(v), u).unwrap()
    }

    fn shown(e: &Expr) -> String {
        present(e).unwrap().unwrap().text
    }

    #[test]
    fn products_simplify_and_sums_check() {
        let w = (q(70, "kg") * q(10, "m/s^2")).simplify();
        assert_eq!(shown(&w), "700 N");
        let bad = (q(3, "m") + q(2, "s")).simplify();
        assert_eq!(check(&bad).unwrap_err().to_string(), "can't add m and s");
        let mixed = (q(1, "km") + q(500, "m")).simplify();
        assert!(["3/2 km", "1500 m"].contains(&shown(&mixed).as_str()));
    }

    #[test]
    fn names_of_unknown_dimension_are_not_errors() {
        let e = (Expr::sym("x") + q(3, "m")).simplify();
        assert!(check(&e).is_ok());
        let e = (Expr::sym("t") * q(6, "m/s^2")).simplify();
        assert_eq!(shown(&e), "6*t m/s^2");
    }

    #[test]
    fn functions_need_plain_numbers() {
        let e = Expr::call("sin", vec![q(30, "°")]).unwrap();
        assert_eq!(shown(&e), "1/2");
        let e = Expr::e().pow(q(2, "m"));
        assert_eq!(
            check(&e).unwrap_err().to_string(),
            "exp needs a dimensionless argument, but got m"
        );
        let e = q(9, "m^2").sqrt();
        assert_eq!(shown(&e), "3 m");
    }

    #[test]
    fn conversions_keep_their_unit() {
        let c = convert(&[q(100, "km/h"), factor(&parse_unit("m/s").unwrap())]).unwrap();
        assert_eq!(shown(&c), "250/9 m/s");
        assert_eq!(approximate(&c).unwrap().as_deref(), Some("27.77777778 m/s"));
        let c = convert(&[q(1, "N*m"), factor(&parse_unit("N·m").unwrap())]).unwrap();
        assert_eq!(shown(&c), "1 N·m");
        assert!(convert(&[q(1, "m"), factor(&parse_unit("s").unwrap())]).is_err());
    }

    #[test]
    fn comparisons() {
        let r = Expr::relation(RelOp::Lt, q(3, "m"), q(2, "km"));
        assert_eq!(shown(&r), "true");
        let r = Expr::relation(RelOp::Lt, q(3, "m"), q(2, "kg"));
        assert!(present(&r).is_err());
    }
}
