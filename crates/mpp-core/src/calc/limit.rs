//! Limits: direct substitution, cancellation, leading terms, dominant growth, rewriting of
//! indeterminate forms and L'Hôpital's rule, each answer confirmed numerically.

use std::cmp::Ordering;
use std::sync::Arc;

use num_rational::BigRational;
use num_traits::{ToPrimitive, Zero};

use crate::calc::diff::diff;
use crate::error::{MathError, MathResult};
use crate::expand::expand;
use crate::expr::{Constant, Expr, ExprNode, LimitDir};
use crate::functions::{func, Func};
use crate::number::Number;
use crate::poly::Poly;
use crate::simplify::{add, mul, pow, simplify, split_coeff};
use crate::steps::Steps;

/// Most applications of L'Hôpital's rule in one limit.
const MAX_LHOPITAL: u32 = 12;
/// Largest combined size (in nodes) of the derivatives L'Hôpital's rule may produce.
const MAX_LHOPITAL_SIZE: usize = 600;
/// Recursion guard for the limit engine.
const MAX_DEPTH: u32 = 400;

/// The limit of `expr` as `var → target` from the side `dir` (ignored at ±∞).
///
/// Returns a finite value, `∞` or `−∞`. A limit that does not exist (sides differ, or the
/// function oscillates) is a [`MathError::LimitDoesNotExist`]; an exact answer that its
/// numeric check contradicts is a [`MathError::Inconclusive`] rather than a wrong answer.
pub fn limit(
    expr: &Expr,
    var: &str,
    target: &Expr,
    dir: LimitDir,
    steps: Option<&mut Steps>,
) -> MathResult<Expr> {
    let mut log = steps;
    let e = simplify(expr);
    let target = simplify(target);
    if !e.contains_sym(var) {
        return Ok(e);
    }
    if target.is_undefined() || target.contains_sym(var) {
        return Err(MathError::Undefined("this limit point".into()));
    }
    if target.is_infinite() {
        let positive = matches!(target.node(), ExprNode::Const(Constant::Infinity));
        let e = if positive {
            e
        } else {
            let flipped = e.subs(var, &-Expr::sym(var));
            Steps::record(
                &mut log,
                "substitute x = −t",
                &holder(&e, var, &target, LimitDir::Both),
                &holder(&flipped, var, &Expr::infinity(), LimitDir::Both),
                Some(format!("{var} → −∞ becomes {var} → ∞")),
            );
            flipped
        };
        let ctx = Ctx::new(var, Point::PosInf, Side::Plus, log);
        return ctx.run(&e);
    }
    let value = target.eval_f64(&crate::eval::EmptyEnv).ok();
    let point = Point::Finite(target.clone(), value);
    let sides: Vec<Side> = match dir {
        LimitDir::Both => vec![Side::Minus, Side::Plus],
        LimitDir::Left => vec![Side::Minus],
        LimitDir::Right => vec![Side::Plus],
    };
    // Direct substitution, trusted only when the function agrees numerically.
    if let Some(v) = substitute(&e, var, &target) {
        let ok = sides
            .iter()
            .all(|s| numeric_check(&e, var, &point, *s, &v).is_ok());
        if ok {
            Steps::record(
                &mut log,
                "direct substitution",
                &holder(&e, var, &target, dir),
                &v,
                None,
            );
            return Ok(v);
        }
    }
    if sides.len() == 1 {
        let ctx = Ctx::new(var, point, sides[0], log);
        return ctx.run(&e);
    }
    // Two-sided: both one-sided limits must exist and agree.
    let logging = log.is_some();
    let mut left_steps = Steps::new();
    let mut right_steps = Steps::new();
    let left = Ctx::new(
        var,
        point.clone(),
        Side::Minus,
        logging.then_some(&mut left_steps),
    )
    .run(&e);
    let right = Ctx::new(var, point, Side::Plus, logging.then_some(&mut right_steps)).run(&e);
    match (left, right) {
        (Ok(a), Ok(b)) if a == b => {
            if let Some(steps) = log {
                steps.0.extend(right_steps.0);
                steps.push(
                    "both one-sided limits agree",
                    holder(&e, var, &target, LimitDir::Both),
                    a.clone(),
                    None,
                );
            }
            Ok(a)
        }
        (Ok(a), Ok(b)) => Err(MathError::LimitDoesNotExist(format!(
            "from the left it is {a}, from the right it is {b}"
        ))),
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

fn holder(e: &Expr, var: &str, point: &Expr, dir: LimitDir) -> Expr {
    Expr::new(ExprNode::Limit {
        expr: e.clone(),
        var: Arc::from(var),
        point: point.clone(),
        dir,
    })
}

fn contains_const(e: &Expr, c: Constant) -> bool {
    matches!(e.node(), ExprNode::Const(k) if *k == c)
        || e.children().iter().any(|x| contains_const(x, c))
}

/// `e` with `var = target`, if that gives a plain value.
fn substitute(e: &Expr, var: &str, target: &Expr) -> Option<Expr> {
    let v = e.subs(var, target);
    if v.contains_sym(var) || contains_const(&v, Constant::Undefined) {
        return None;
    }
    if contains_const(&v, Constant::Infinity) && !v.is_infinite() {
        return None;
    }
    Some(v)
}

#[derive(Clone, Debug)]
enum Point {
    /// A finite point and its numeric value (when it has one).
    Finite(Expr, Option<f64>),
    PosInf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Minus,
    Plus,
}

/// The value a sub-expression tends to.
#[derive(Clone, Debug)]
enum Lv {
    Fin(Expr),
    PosInf,
    NegInf,
    /// Stays bounded but has no limit (such as `sin x` as `x → ∞`).
    Bounded,
}

fn lv_of(e: Expr) -> Lv {
    let e = simplify(&e);
    if matches!(e.node(), ExprNode::Const(Constant::Infinity)) {
        Lv::PosInf
    } else if e.is_infinite() {
        Lv::NegInf
    } else {
        Lv::Fin(e)
    }
}

fn lv_expr(v: &Lv) -> Expr {
    match v {
        Lv::Fin(e) => e.clone(),
        Lv::PosInf => Expr::infinity(),
        Lv::NegInf => Expr::neg_infinity(),
        Lv::Bounded => Expr::undefined(),
    }
}

fn numeric(e: &Expr) -> Option<f64> {
    e.eval_f64(&crate::eval::EmptyEnv).ok()
}

fn is_zero_value(e: &Expr) -> bool {
    e.is_zero() || (e.is_constant() && numeric(e).is_some_and(|v| v.abs() < 1e-14))
}

fn sign_of(e: &Expr) -> Option<Ordering> {
    numeric(e).and_then(|v| v.partial_cmp(&0.0))
}

fn not_exist(why: &str) -> MathError {
    MathError::LimitDoesNotExist(why.into())
}

fn unsupported() -> MathError {
    MathError::Unsupported("this limit".into())
}

/// Sample points approaching the limit point, farthest first.
fn sample_points(point: &Point, side: Side, scales: &[f64]) -> Vec<f64> {
    match point {
        Point::PosInf => scales.iter().map(|s| 1.0 / s).collect(),
        Point::Finite(_, Some(a)) => {
            let unit = a.abs().max(1.0);
            let dir = if side == Side::Plus { 1.0 } else { -1.0 };
            scales.iter().map(|h| a + dir * h * unit).collect()
        }
        Point::Finite(_, None) => Vec::new(),
    }
}

fn eval_at(e: &Expr, var: &str, x: f64) -> Option<f64> {
    e.eval_f64(&[(var, x)]).ok().filter(|v| v.is_finite())
}

/// Checks an exact answer against values of `e` near the limit point.
fn numeric_check(e: &Expr, var: &str, point: &Point, side: Side, result: &Expr) -> MathResult<()> {
    if e.free_symbols().iter().any(|s| &**s != var) {
        return Ok(());
    }
    let xs = sample_points(point, side, &[1e-2, 1e-3, 1e-4, 1e-5]);
    let values: Vec<f64> = xs.iter().filter_map(|x| eval_at(e, var, *x)).collect();
    if values.len() < 2 {
        return Ok(());
    }
    let fail = || MathError::Inconclusive(format!("the limit {result}"));
    let (first, last) = (values[0], values[values.len() - 1]);
    if matches!(result.node(), ExprNode::Const(Constant::Infinity)) {
        return if last > 0.0 && last > first {
            Ok(())
        } else {
            Err(fail())
        };
    }
    if result.is_infinite() {
        return if last < 0.0 && last < first {
            Ok(())
        } else {
            Err(fail())
        };
    }
    let Some(r) = numeric(result) else {
        return Ok(());
    };
    let tolerance = 0.05 * r.abs().max(1.0);
    let best = values
        .iter()
        .map(|v| (v - r).abs())
        .fold(f64::INFINITY, f64::min);
    if best <= tolerance || slow_approach_at_infinity(e, var, point, r, tolerance) {
        Ok(())
    } else {
        Err(fail())
    }
}

/// True when `e` creeps towards `r` as `var → ∞` too slowly for the usual samples to show
/// it (logarithmic convergence such as `−1/ln x → 0`): farther samples must close in on
/// `r` steadily and end within `tolerance` of it.
fn slow_approach_at_infinity(e: &Expr, var: &str, point: &Point, r: f64, tolerance: f64) -> bool {
    if !matches!(point, Point::PosInf) {
        return false;
    }
    let xs = sample_points(point, Side::Plus, &[1e-5, 1e-8, 1e-11, 1e-14, 1e-17]);
    let gaps: Option<Vec<f64>> = xs
        .iter()
        .map(|x| eval_at(e, var, *x).map(|v| (v - r).abs()))
        .collect();
    let Some(gaps) = gaps else {
        return false;
    };
    let closing = gaps.windows(2).all(|w| w[1] < w[0]);
    closing && gaps.last().is_some_and(|g| *g <= tolerance)
}

/// Leading behaviour `c · x^p` as `x → ∞`, when it can be read off.
fn leading(e: &Expr, var: &str) -> Option<(Expr, BigRational)> {
    if !e.contains_sym(var) {
        return Some((e.clone(), BigRational::zero()));
    }
    match e.node() {
        ExprNode::Sym(_) => Some((Expr::one(), BigRational::from_integer(1.into()))),
        ExprNode::Pow(b, x) => {
            let r = x.as_number()?.as_rational()?.clone();
            let (c, p) = leading(b, var)?;
            if !r.is_integer() && sign_of(&c) != Some(Ordering::Greater) {
                return None;
            }
            Some((pow(c, Expr::ratio(r.clone())), p * r))
        }
        ExprNode::Mul(fs) => {
            let mut c = Expr::one();
            let mut p = BigRational::zero();
            for f in fs {
                let (cf, pf) = leading(f, var)?;
                c = mul(vec![c, cf]);
                p += pf;
            }
            Some((c, p))
        }
        ExprNode::Add(ts) => {
            let leads = ts
                .iter()
                .map(|t| leading(t, var))
                .collect::<Option<Vec<_>>>()?;
            let top = leads.iter().map(|(_, p)| p.clone()).max()?;
            let c = add(leads
                .into_iter()
                .filter(|(_, p)| *p == top)
                .map(|(c, _)| c)
                .collect());
            if is_zero_value(&c) {
                None
            } else {
                Some((c, top))
            }
        }
        _ => None,
    }
}

/// Growth class as `x → ∞`: (exponential order, exponential rate, power, log power).
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
struct Growth(f64, f64, f64, f64);

impl Growth {
    fn scale(self, r: f64) -> Growth {
        Growth(self.0 * r.signum(), self.1 * r, self.2 * r, self.3 * r)
    }
}

fn growth(e: &Expr, var: &str) -> Option<Growth> {
    if !e.contains_sym(var) {
        return Some(Growth(0.0, 0.0, 0.0, 0.0));
    }
    match e.node() {
        ExprNode::Sym(_) => Some(Growth(0.0, 0.0, 1.0, 0.0)),
        ExprNode::Pow(b, x) if matches!(b.node(), ExprNode::Const(Constant::E)) => {
            let (c, p) = leading(x, var)?;
            let c = numeric(&c)?;
            let p = p.to_f64()?;
            if p <= 0.0 {
                return Some(Growth(0.0, 0.0, 0.0, 0.0));
            }
            Some(Growth(p * c.signum(), c, 0.0, 0.0))
        }
        ExprNode::Pow(b, x) => {
            let r = x.as_number()?.as_rational()?.to_f64()?;
            Some(growth(b, var)?.scale(r))
        }
        ExprNode::Func(Func::Ln, args) => {
            let g = growth(args.first()?, var)?;
            if g.0 > 0.0 {
                Some(Growth(0.0, 0.0, g.0, 0.0))
            } else if g.2 > 0.0 || g.3 > 0.0 {
                Some(Growth(0.0, 0.0, 0.0, 1.0))
            } else {
                None
            }
        }
        ExprNode::Mul(fs) => {
            let mut acc = Growth(0.0, 0.0, 0.0, 0.0);
            for f in fs {
                let g = growth(f, var)?;
                let exp = if acc.0 == 0.0 {
                    (g.0, g.1)
                } else if g.0 == 0.0 || g.0 == acc.0 {
                    (acc.0, acc.1 + g.1)
                } else if g.0.abs() > acc.0.abs() {
                    (g.0, g.1)
                } else {
                    (acc.0, acc.1)
                };
                acc = Growth(exp.0, exp.1, acc.2 + g.2, acc.3 + g.3);
            }
            Some(acc)
        }
        ExprNode::Add(ts) => {
            let gs = ts
                .iter()
                .map(|t| growth(t, var))
                .collect::<Option<Vec<_>>>()?;
            gs.into_iter()
                .max_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal))
        }
        _ => None,
    }
}

fn has_square_root(e: &Expr) -> bool {
    match e.node() {
        ExprNode::Pow(_, x) => x
            .as_number()
            .and_then(Number::as_rational)
            .is_some_and(|r| r.denom() == &2.into()),
        _ => e.children().iter().any(|c| has_square_root(c)),
    }
}

fn mentions(e: &Expr, pred: &dyn Fn(&Expr) -> bool) -> bool {
    pred(e) || e.children().iter().any(|c| mentions(c, pred))
}

fn is_exp(e: &Expr) -> bool {
    matches!(e.node(), ExprNode::Pow(b, _) if matches!(b.node(), ExprNode::Const(Constant::E)))
}

fn is_ln(e: &Expr) -> bool {
    matches!(e.node(), ExprNode::Func(Func::Ln, _))
}

/// Numerator and denominator factors: negative powers (and `e` to a negative multiple) go below.
fn split_fraction(e: &Expr) -> (Vec<Expr>, Vec<Expr>) {
    let factors = match e.node() {
        ExprNode::Mul(fs) => fs.clone(),
        _ => vec![e.clone()],
    };
    let mut num = Vec::new();
    let mut den = Vec::new();
    for f in factors {
        if let ExprNode::Pow(b, x) = f.node() {
            let negative = if x.is_number() {
                x.as_number().is_some_and(Number::is_negative)
            } else {
                matches!(b.node(), ExprNode::Const(Constant::E)) && split_coeff(x).0.is_negative()
            };
            if negative {
                den.push(pow(b.clone(), mul(vec![Expr::int(-1), x.clone()])));
                continue;
            }
        }
        num.push(f);
    }
    (num, den)
}

struct Ctx<'s> {
    var: String,
    point: Point,
    side: Side,
    log: Option<&'s mut Steps>,
    lhopital: u32,
    depth: u32,
}

impl<'s> Ctx<'s> {
    fn new(var: &str, point: Point, side: Side, log: Option<&'s mut Steps>) -> Ctx<'s> {
        Ctx {
            var: var.into(),
            point,
            side,
            log,
            lhopital: 0,
            depth: 0,
        }
    }

    fn point_expr(&self) -> Expr {
        match &self.point {
            Point::Finite(e, _) => e.clone(),
            Point::PosInf => Expr::infinity(),
        }
    }

    fn dir(&self) -> LimitDir {
        match (&self.point, self.side) {
            (Point::PosInf, _) => LimitDir::Both,
            (_, Side::Plus) => LimitDir::Right,
            (_, Side::Minus) => LimitDir::Left,
        }
    }

    fn holder(&self, e: &Expr) -> Expr {
        holder(e, &self.var, &self.point_expr(), self.dir())
    }

    fn record(&mut self, rule: &str, before: &Expr, after: &Expr, note: Option<String>) {
        let (b, a) = (self.holder(before), self.holder(after));
        Steps::record(&mut self.log, rule, &b, &a, note);
    }

    /// Records that the limit of `before` is the plain value `value`.
    fn record_value(&mut self, rule: &str, before: &Expr, value: &Expr) {
        let b = self.holder(before);
        Steps::record(&mut self.log, rule, &b, value, None);
    }

    fn run(mut self, e: &Expr) -> MathResult<Expr> {
        let result = match self.lim(e)? {
            Lv::Fin(v) => simplify(&v),
            Lv::PosInf => Expr::infinity(),
            Lv::NegInf => Expr::neg_infinity(),
            Lv::Bounded => return Err(not_exist("the function oscillates without settling")),
        };
        if result.is_undefined() {
            return Err(not_exist("the expression has no value there"));
        }
        numeric_check(e, &self.var, &self.point, self.side, &result)?;
        Ok(result)
    }

    /// Sign of `e` just before the limit point, if it is clear.
    fn approach_sign(&self, e: &Expr) -> Option<Ordering> {
        // Moderate points at infinity, so exponentials do not overflow.
        let scales: &[f64] = match self.point {
            Point::PosInf => &[0.05, 0.02, 0.01],
            Point::Finite(..) => &[1e-4, 1e-6, 1e-8],
        };
        let xs = sample_points(&self.point, self.side, scales);
        let signs: Vec<Ordering> = xs
            .iter()
            .filter_map(|x| eval_at(e, &self.var, *x))
            .filter_map(|v| v.partial_cmp(&0.0))
            .collect();
        let first = *signs.first()?;
        (first != Ordering::Equal && signs.iter().all(|s| *s == first)).then_some(first)
    }

    fn infinite(&self, sign: Option<Ordering>) -> MathResult<Lv> {
        match sign {
            Some(Ordering::Greater) => Ok(Lv::PosInf),
            Some(Ordering::Less) => Ok(Lv::NegInf),
            _ => Err(not_exist(
                "the function grows without bound with no fixed sign",
            )),
        }
    }

    fn lim(&mut self, e: &Expr) -> MathResult<Lv> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(unsupported());
        }
        let out = self.lim_inner(e);
        self.depth -= 1;
        out
    }

    fn lim_inner(&mut self, e: &Expr) -> MathResult<Lv> {
        if !e.contains_sym(&self.var) {
            return Ok(lv_of(e.clone()));
        }
        match e.node() {
            ExprNode::Sym(_) => Ok(match &self.point {
                Point::Finite(a, _) => Lv::Fin(a.clone()),
                Point::PosInf => Lv::PosInf,
            }),
            ExprNode::Add(ts) => self.lim_add(e, ts),
            ExprNode::Mul(_) => {
                let (num, den) = split_fraction(e);
                if den.is_empty() {
                    self.lim_product(&num)
                } else {
                    self.lim_quotient(&mul(num), &mul(den))
                }
            }
            ExprNode::Pow(b, x) => {
                let (num, den) = split_fraction(e);
                if !den.is_empty() {
                    return self.lim_quotient(&mul(num), &mul(den));
                }
                self.lim_pow(b, x)
            }
            ExprNode::Func(f, args) => self.lim_func(e, *f, args),
            _ => Err(unsupported()),
        }
    }

    fn lim_add(&mut self, e: &Expr, ts: &[Expr]) -> MathResult<Lv> {
        let mut finite = Vec::new();
        let (mut pos, mut neg, mut bounded) = (false, false, false);
        for t in ts {
            match self.lim(t)? {
                Lv::Fin(v) => finite.push(v),
                Lv::PosInf => pos = true,
                Lv::NegInf => neg = true,
                Lv::Bounded => bounded = true,
            }
        }
        if pos && neg {
            return self.infinity_minus_infinity(e, ts);
        }
        Ok(if pos {
            Lv::PosInf
        } else if neg {
            Lv::NegInf
        } else if bounded {
            Lv::Bounded
        } else {
            lv_of(add(finite))
        })
    }

    fn infinity_minus_infinity(&mut self, e: &Expr, ts: &[Expr]) -> MathResult<Lv> {
        if matches!(self.point, Point::PosInf) {
            if let Some((c, p)) = leading(e, &self.var) {
                let lead = mul(vec![
                    c.clone(),
                    pow(Expr::sym(&self.var), Expr::ratio(p.clone())),
                ]);
                self.record("leading terms", e, &lead, None);
                return self.value_of_leading(&c, &p);
            }
        }
        if ts.len() == 2 && ts.iter().any(has_square_root) {
            let (a, b) = (&ts[0], &ts[1]);
            let top = expand(&add(vec![
                pow(a.clone(), Expr::int(2)),
                mul(vec![Expr::int(-1), pow(b.clone(), Expr::int(2))]),
            ]))?;
            let bottom = add(vec![a.clone(), mul(vec![Expr::int(-1), b.clone()])]);
            let shown = Expr::new(ExprNode::Mul(vec![
                top.clone(),
                Expr::new(ExprNode::Pow(bottom.clone(), Expr::int(-1))),
            ]));
            self.record("multiply by the conjugate", e, &shown, None);
            return self.lim_quotient(&top, &bottom);
        }
        let (n, d) = together(ts);
        if !d.is_one() {
            let shown = Expr::new(ExprNode::Mul(vec![
                n.clone(),
                Expr::new(ExprNode::Pow(d.clone(), Expr::int(-1))),
            ]));
            self.record("combine into one fraction", e, &shown, None);
            return self.lim_quotient(&n, &d);
        }
        Err(MathError::Unsupported("this ∞ − ∞ limit".into()))
    }

    fn value_of_leading(&self, c: &Expr, p: &BigRational) -> MathResult<Lv> {
        match p.cmp(&BigRational::zero()) {
            Ordering::Less => Ok(Lv::Fin(Expr::zero())),
            Ordering::Equal => Ok(lv_of(c.clone())),
            Ordering::Greater => self.infinite(sign_of(c)),
        }
    }

    fn lim_product(&mut self, factors: &[Expr]) -> MathResult<Lv> {
        let mut values = Vec::with_capacity(factors.len());
        for f in factors {
            values.push(self.lim(f)?);
        }
        let zero_idx: Vec<usize> = (0..values.len())
            .filter(|i| matches!(&values[*i], Lv::Fin(v) if is_zero_value(v)))
            .collect();
        let any_inf = values.iter().any(|v| matches!(v, Lv::PosInf | Lv::NegInf));
        let any_bounded = values.iter().any(|v| matches!(v, Lv::Bounded));
        if !zero_idx.is_empty() && any_inf {
            // 0·∞: move one side into a denominator.
            let zeros = mul(zero_idx.iter().map(|i| factors[*i].clone()).collect());
            let others = mul((0..factors.len())
                .filter(|i| !zero_idx.contains(i))
                .map(|i| factors[i].clone())
                .collect());
            let product = mul(factors.to_vec());
            let keep_others_on_top = mentions(&zeros, &is_exp) || mentions(&others, &is_ln);
            let (top, bottom) = if keep_others_on_top {
                (others, pow(zeros, Expr::int(-1)))
            } else {
                (zeros, pow(others, Expr::int(-1)))
            };
            let shown = Expr::new(ExprNode::Mul(vec![
                top.clone(),
                Expr::new(ExprNode::Pow(bottom.clone(), Expr::int(-1))),
            ]));
            self.record("rewrite 0·∞ as a quotient", &product, &shown, None);
            return self.lim_quotient(&top, &bottom);
        }
        if any_bounded {
            if !zero_idx.is_empty() {
                return Ok(Lv::Fin(Expr::zero()));
            }
            if any_inf {
                return Err(not_exist("the function oscillates with growing size"));
            }
            return Ok(Lv::Bounded);
        }
        if any_inf {
            let mut sign = Ordering::Greater;
            for v in &values {
                let s = match v {
                    Lv::PosInf => Ordering::Greater,
                    Lv::NegInf => Ordering::Less,
                    Lv::Fin(x) => sign_of(x).ok_or_else(unsupported)?,
                    Lv::Bounded => Ordering::Greater,
                };
                if s == Ordering::Less {
                    sign = sign.reverse();
                }
            }
            return self.infinite(Some(sign));
        }
        Ok(lv_of(mul(values.iter().map(lv_expr).collect())))
    }

    fn lim_quotient(&mut self, n: &Expr, d: &Expr) -> MathResult<Lv> {
        let ln = self.lim(n)?;
        let ld = self.lim(d)?;
        let inf = |v: &Lv| matches!(v, Lv::PosInf | Lv::NegInf);
        let inf_sign = |v: &Lv| match v {
            Lv::NegInf => Ordering::Less,
            _ => Ordering::Greater,
        };
        match (&ln, &ld) {
            (Lv::Fin(a), Lv::Fin(b)) => {
                let (az, bz) = (is_zero_value(a), is_zero_value(b));
                if !bz {
                    return Ok(lv_of(mul(vec![a.clone(), pow(b.clone(), Expr::int(-1))])));
                }
                if !az {
                    let s = sign_of(a).zip(self.approach_sign(d)).map(|(x, y)| {
                        if x == y {
                            Ordering::Greater
                        } else {
                            Ordering::Less
                        }
                    });
                    return self.infinite(s);
                }
                self.indeterminate(n, d, "0/0")
            }
            (Lv::Fin(_) | Lv::Bounded, _) if inf(&ld) => Ok(Lv::Fin(Expr::zero())),
            (_, Lv::Fin(b)) if inf(&ln) => {
                let bs = if is_zero_value(b) {
                    self.approach_sign(d)
                } else {
                    sign_of(b)
                };
                let s = bs.map(|y| {
                    if inf_sign(&ln) == y {
                        Ordering::Greater
                    } else {
                        Ordering::Less
                    }
                });
                self.infinite(s)
            }
            _ if inf(&ln) && inf(&ld) => self.indeterminate(n, d, "∞/∞"),
            (Lv::Bounded, Lv::Fin(b)) if !is_zero_value(b) => Ok(Lv::Bounded),
            (Lv::Bounded, Lv::Fin(_)) => {
                Err(not_exist("the function oscillates with growing size"))
            }
            _ => Err(unsupported()),
        }
    }

    /// 0/0 or ∞/∞.
    fn indeterminate(&mut self, n: &Expr, d: &Expr, form: &str) -> MathResult<Lv> {
        let var = self.var.clone();
        let quotient = mul(vec![n.clone(), pow(d.clone(), Expr::int(-1))]);
        if matches!(self.point, Point::Finite(..)) {
            if let (Some(pn), Some(pd)) = (Poly::from_expr(n, &var), Poly::from_expr(d, &var)) {
                let g = pn.gcd(&pd);
                if g.degree().is_some_and(|k| k >= 1) {
                    if let (Ok((qn, _)), Ok((qd, _))) = (pn.div_rem(&g), pd.div_rem(&g)) {
                        let (nn, nd) = (qn.to_expr(&var), qd.to_expr(&var));
                        let shown = mul(vec![nn.clone(), pow(nd.clone(), Expr::int(-1))]);
                        self.record(
                            "cancel the common factor",
                            &quotient,
                            &shown,
                            Some(format!("common factor {}", g.to_expr(&var))),
                        );
                        return self.lim_quotient(&nn, &nd);
                    }
                }
            }
        }
        if matches!(self.point, Point::PosInf) {
            if let (Some((cn, pn)), Some((cd, pd))) = (leading(n, &var), leading(d, &var)) {
                let c = mul(vec![cn, pow(cd, Expr::int(-1))]);
                let p = pn - pd;
                let lead = mul(vec![
                    c.clone(),
                    pow(Expr::sym(&var), Expr::ratio(p.clone())),
                ]);
                self.record("leading terms", &quotient, &lead, None);
                return self.value_of_leading(&c, &p);
            }
            if let (Some(gn), Some(gd)) = (growth(n, &var), growth(d, &var)) {
                match gn.partial_cmp(&gd) {
                    Some(Ordering::Less) => {
                        self.record(
                            "dominant growth",
                            &quotient,
                            &Expr::zero(),
                            Some("the denominator grows faster".into()),
                        );
                        return Ok(Lv::Fin(Expr::zero()));
                    }
                    Some(Ordering::Greater) => {
                        let lv = self.infinite(self.approach_sign(&quotient))?;
                        self.record(
                            "dominant growth",
                            &quotient,
                            &lv_expr(&lv),
                            Some("the numerator grows faster".into()),
                        );
                        return Ok(lv);
                    }
                    _ => {}
                }
            }
            if let Some(lv) = self.divide_by_dominant_exp(n, d, &quotient)? {
                return Ok(lv);
            }
        }
        let entry = self.lhopital;
        let mark = self.log.as_ref().map(|l| l.len());
        let result = self.lhopital_step(n, d, &quotient, form);
        match result {
            // L'Hôpital's rule cycled or blew up: drop its steps and read the answer off
            // the series instead.
            Err(MathError::Unsupported(_)) if entry == 0 => {
                if let (Some(log), Some(mark)) = (self.log.as_deref_mut(), mark) {
                    log.0.truncate(mark);
                }
                match self.series_quotient(n, d, &quotient) {
                    Some(lv) => Ok(lv),
                    None => result,
                }
            }
            other => other,
        }
    }

    /// At `+∞`, divides top and bottom by the fastest-growing exponential `e^{λx}`.
    fn divide_by_dominant_exp(
        &mut self,
        n: &Expr,
        d: &Expr,
        quotient: &Expr,
    ) -> MathResult<Option<Lv>> {
        let var = self.var.clone();
        let mut best: Option<(Expr, f64)> = None;
        for side in [n, d] {
            collect_exp_rates(side, &var, &mut best);
        }
        let Some((rate, _)) = best else {
            return Ok(None);
        };
        let damp = pow(
            Expr::e(),
            mul(vec![Expr::int(-1), rate.clone(), Expr::sym(&var)]),
        );
        let n2 = simplify(&expand(&mul(vec![n.clone(), damp.clone()]))?);
        let d2 = simplify(&expand(&mul(vec![d.clone(), damp]))?);
        if &n2 == n && &d2 == d {
            return Ok(None);
        }
        let shown = Expr::new(ExprNode::Mul(vec![
            n2.clone(),
            Expr::new(ExprNode::Pow(d2.clone(), Expr::int(-1))),
        ]));
        self.record(
            "divide by the dominant exponential",
            quotient,
            &shown,
            Some(format!("divide the top and bottom by e^({rate}·{var})")),
        );
        self.lim_quotient(&n2, &d2).map(Some)
    }

    /// The limit of `n/d` from the leading terms of their series.
    fn series_quotient(&mut self, n: &Expr, d: &Expr, quotient: &Expr) -> Option<Lv> {
        let at = self.point_expr();
        let (cn, vn) = crate::calc::series::leading_term(n, &self.var, &at, 30)?;
        let (cd, vd) = crate::calc::series::leading_term(d, &self.var, &at, 30)?;
        let c = simplify(&mul(vec![cn, pow(cd, Expr::int(-1))]));
        let p = vn - vd;
        let base = match &self.point {
            Point::Finite(a, _) => add(vec![
                Expr::sym(&self.var),
                mul(vec![Expr::int(-1), a.clone()]),
            ]),
            Point::PosInf => pow(Expr::sym(&self.var), Expr::int(-1)),
        };
        let lead = mul(vec![c.clone(), pow(base, Expr::int(p))]);
        let lv = match p.cmp(&0) {
            Ordering::Greater => Lv::Fin(Expr::zero()),
            Ordering::Equal => lv_of(c),
            Ordering::Less => {
                let mut s = sign_of(&c)?;
                if self.side == Side::Minus && matches!(self.point, Point::Finite(..)) && p % 2 != 0
                {
                    s = s.reverse();
                }
                self.infinite(Some(s)).ok()?
            }
        };
        self.record(
            "series expansion",
            quotient,
            &lead,
            Some("keep the leading terms of the top and bottom series".into()),
        );
        Some(lv)
    }

    fn lhopital_step(&mut self, n: &Expr, d: &Expr, quotient: &Expr, form: &str) -> MathResult<Lv> {
        let var = self.var.clone();
        if self.lhopital >= MAX_LHOPITAL {
            return Err(MathError::Unsupported(
                "this limit (L'Hôpital's rule did not settle it)".into(),
            ));
        }
        self.lhopital += 1;
        let dn = diff(n, &var, 1, None)?;
        let dd = diff(d, &var, 1, None)?;
        if dd.is_zero() {
            return Err(unsupported());
        }
        if dn.size() + dd.size() > MAX_LHOPITAL_SIZE {
            return Err(MathError::Unsupported(
                "this limit (the derivatives in L'Hôpital's rule grow too large)".into(),
            ));
        }
        let shown = if dd.is_one() {
            dn.clone()
        } else {
            Expr::new(ExprNode::Mul(vec![
                dn.clone(),
                Expr::new(ExprNode::Pow(dd.clone(), Expr::int(-1))),
            ]))
        };
        let note = format!(
            "the limit has the form {form}, so differentiate the top and bottom: d/d{var}({n}) = {dn}, d/d{var}({d}) = {dd}"
        );
        self.record("L'Hôpital's rule", quotient, &shown, Some(note));
        let next = mul(vec![dn, pow(dd, Expr::int(-1))]);
        let value = self.lim(&next)?;
        if let Lv::Fin(v) = &value {
            let v = simplify(v);
            let shown_v = self.holder(&v);
            let last = self
                .log
                .as_ref()
                .and_then(|l| l.0.last())
                .map(|s| s.after.clone());
            if last.is_some_and(|a| a != v && a != shown_v) {
                self.record_value("evaluate the new limit", &next, &v);
            }
        }
        Ok(value)
    }

    fn lim_pow(&mut self, b: &Expr, x: &Expr) -> MathResult<Lv> {
        let var = self.var.clone();
        let (b_var, x_var) = (b.contains_sym(&var), x.contains_sym(&var));
        if !x_var {
            let xv = numeric(x);
            let is_int = x.as_number().is_some_and(Number::is_integer);
            let even = x
                .as_number()
                .and_then(Number::as_integer)
                .is_some_and(|k| num_integer::Integer::is_even(&k));
            return match self.lim(b)? {
                Lv::Fin(l) if !is_zero_value(&l) => Ok(lv_of(pow(l, x.clone()))),
                Lv::Fin(_) => match xv.map(|v| v.partial_cmp(&0.0)) {
                    Some(Some(Ordering::Greater)) => Ok(Lv::Fin(Expr::zero())),
                    Some(Some(Ordering::Less)) => {
                        if even {
                            return Ok(Lv::PosInf);
                        }
                        match self.approach_sign(b) {
                            Some(Ordering::Greater) => Ok(Lv::PosInf),
                            Some(Ordering::Less) if is_int => Ok(Lv::NegInf),
                            _ => Err(not_exist("the power has no real value on this side")),
                        }
                    }
                    _ => Err(unsupported()),
                },
                Lv::PosInf => match xv {
                    Some(v) if v > 0.0 => Ok(Lv::PosInf),
                    Some(v) if v < 0.0 => Ok(Lv::Fin(Expr::zero())),
                    _ => Err(unsupported()),
                },
                Lv::NegInf => match xv {
                    Some(v) if v < 0.0 => Ok(Lv::Fin(Expr::zero())),
                    Some(_) if even => Ok(Lv::PosInf),
                    Some(_) if is_int => Ok(Lv::NegInf),
                    _ => Err(not_exist("the power has no real value here")),
                },
                Lv::Bounded => match xv {
                    Some(v) if v > 0.0 => Ok(Lv::Bounded),
                    _ => Err(unsupported()),
                },
            };
        }
        if !b_var {
            let bv = numeric(b).ok_or_else(unsupported)?;
            return match self.lim(x)? {
                Lv::Fin(g) => Ok(lv_of(pow(b.clone(), g))),
                Lv::PosInf if bv > 1.0 => Ok(Lv::PosInf),
                Lv::PosInf if (0.0..1.0).contains(&bv) => Ok(Lv::Fin(Expr::zero())),
                Lv::NegInf if bv > 1.0 => Ok(Lv::Fin(Expr::zero())),
                Lv::NegInf if bv > 0.0 && bv < 1.0 => Ok(Lv::PosInf),
                _ if bv == 1.0 => Ok(Lv::Fin(Expr::one())),
                Lv::Bounded if bv > 0.0 => Ok(Lv::Bounded),
                _ => Err(unsupported()),
            };
        }
        // f^g = e^(g ln f) for 1^∞, 0^0 and ∞^0.
        let inner = mul(vec![x.clone(), func(Func::Ln, vec![b.clone()])]);
        let e = pow(b.clone(), x.clone());
        let shown = Expr::new(ExprNode::Pow(Expr::e(), inner.clone()));
        self.record("rewrite as an exponential", &e, &shown, None);
        Ok(match self.lim(&inner)? {
            Lv::Fin(l) => lv_of(pow(Expr::e(), l)),
            Lv::PosInf => Lv::PosInf,
            Lv::NegInf => Lv::Fin(Expr::zero()),
            Lv::Bounded => Lv::Bounded,
        })
    }

    fn lim_func(&mut self, e: &Expr, f: Func, args: &[Expr]) -> MathResult<Lv> {
        let as_sin_cos = |u: &Expr| -> Option<Expr> {
            let s = func(Func::Sin, vec![u.clone()]);
            let c = func(Func::Cos, vec![u.clone()]);
            let inv = |x: Expr| pow(x, Expr::int(-1));
            Some(match f {
                Func::Tan => mul(vec![s, inv(c)]),
                Func::Cot => mul(vec![c, inv(s)]),
                Func::Sec => inv(c),
                Func::Csc => inv(s),
                _ => return None,
            })
        };
        if f == Func::Log {
            if let [x, b] = args {
                let rewritten = mul(vec![
                    func(Func::Ln, vec![x.clone()]),
                    pow(func(Func::Ln, vec![b.clone()]), Expr::int(-1)),
                ]);
                return self.lim(&rewritten);
            }
        }
        let [u] = args else {
            let mut vals = Vec::with_capacity(args.len());
            for a in args {
                match self.lim(a)? {
                    Lv::Fin(v) => vals.push(v),
                    _ => return Err(unsupported()),
                }
            }
            return Ok(lv_of(func(f, vals)));
        };
        match self.lim(u)? {
            Lv::Fin(l) => {
                let jump = match f {
                    Func::Floor | Func::Ceil => {
                        numeric(&l).is_some_and(|v| (v - v.round()).abs() < 1e-12)
                    }
                    Func::Sign => is_zero_value(&l),
                    _ => false,
                };
                if jump {
                    return self.one_sided_value(e);
                }
                let v = simplify(&func(f, vec![l]));
                if !contains_const(&v, Constant::Undefined) {
                    if contains_const(&v, Constant::Infinity) && !v.is_infinite() {
                        return Err(unsupported());
                    }
                    return Ok(lv_of(v));
                }
                match as_sin_cos(u) {
                    Some(r) => {
                        self.record("rewrite in sine and cosine", e, &r, None);
                        self.lim(&r)
                    }
                    None => Err(not_exist(&format!("{} has a pole here", f.name()))),
                }
            }
            Lv::Bounded => match f {
                Func::Sin | Func::Cos | Func::Atan | Func::Tanh => Ok(Lv::Bounded),
                _ => Err(unsupported()),
            },
            lv => at_infinity(f, matches!(lv, Lv::PosInf)),
        }
    }

    /// Value of a step function just beside the limit point.
    fn one_sided_value(&self, e: &Expr) -> MathResult<Lv> {
        let xs = sample_points(&self.point, self.side, &[1e-7, 1e-9]);
        let vals: Vec<f64> = xs
            .iter()
            .filter_map(|x| eval_at(e, &self.var, *x))
            .collect();
        match vals.as_slice() {
            [a, b] if a == b && a.fract() == 0.0 && a.abs() < 1e15 => {
                Ok(Lv::Fin(Expr::int(*a as i64)))
            }
            _ => Err(unsupported()),
        }
    }
}

/// Limits of functions whose argument tends to ±∞.
fn at_infinity(f: Func, positive: bool) -> MathResult<Lv> {
    let signed = |pos: bool| if pos { Lv::PosInf } else { Lv::NegInf };
    let half_pi = Expr::rational(if positive { 1 } else { -1 }, 2) * Expr::pi();
    match f {
        Func::Sin | Func::Cos => Ok(Lv::Bounded),
        Func::Tan | Func::Sec | Func::Csc | Func::Cot => {
            Err(not_exist("the function keeps repeating its poles"))
        }
        Func::Atan => Ok(Lv::Fin(half_pi)),
        Func::Tanh => Ok(Lv::Fin(Expr::int(if positive { 1 } else { -1 }))),
        Func::Sign => Ok(Lv::Fin(Expr::int(if positive { 1 } else { -1 }))),
        Func::Sinh | Func::Asinh | Func::Floor | Func::Ceil => Ok(signed(positive)),
        Func::Cosh | Func::Abs => Ok(Lv::PosInf),
        Func::Ln | Func::Acosh | Func::Gamma | Func::Factorial | Func::Digamma if positive => {
            Ok(Lv::PosInf)
        }
        _ => Err(not_exist(&format!(
            "{} has no limit at {}",
            f.name(),
            if positive { "∞" } else { "−∞" }
        ))),
    }
}

/// The largest positive rate `λ` among the exponentials `e^{λx + …}` inside `e`.
fn collect_exp_rates(e: &Expr, var: &str, best: &mut Option<(Expr, f64)>) {
    if let ExprNode::Pow(b, x) = e.node() {
        if matches!(b.node(), ExprNode::Const(Constant::E)) {
            if let Some((c, p)) = leading(x, var) {
                if p == BigRational::from_integer(1.into()) {
                    if let Some(v) = numeric(&c).filter(|v| *v > 0.0) {
                        if best.as_ref().is_none_or(|(_, b)| v > *b) {
                            *best = Some((c, v));
                        }
                    }
                }
            }
        }
    }
    for c in e.children() {
        collect_exp_rates(c, var, best);
    }
}

/// Writes a sum as one fraction `N/D`.
fn together(ts: &[Expr]) -> (Expr, Expr) {
    let parts: Vec<(Expr, Expr)> = ts
        .iter()
        .map(|t| {
            let (n, d) = split_fraction(t);
            (mul(n), mul(d))
        })
        .collect();
    let den = mul(parts.iter().map(|(_, d)| d.clone()).collect());
    let num = add((0..parts.len())
        .map(|i| {
            let mut v = vec![parts[i].0.clone()];
            v.extend(
                parts
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| *j != i)
                    .map(|(_, (_, d))| d.clone()),
            );
            mul(v)
        })
        .collect());
    (num, den)
}

#[cfg(test)]
mod tests;
