//! Function analysis for curve sketching: domain and range, intercepts, critical points,
//! local and absolute extrema, where the function increases and decreases, concavity and
//! inflection points, asymptotes, tangent and normal lines, the linearisation, the mean
//! value and intermediate value theorems, and a full summary table ([`Analysis::summary`]).
//!
//! Everything is built from the CAS's derivatives, limits and equation solver. Points come
//! out exact when the solver finds them exactly; otherwise they are numerical and a note
//! says so (`≈`). Questions the tools cannot settle (a periodic function on all of ℝ, a
//! limit the CAS cannot take) are errors that say what to give, never a guess.
//!
//! The method is the textbook one. The places where `f` may stop being defined (zeros of
//! denominators, of logarithm arguments, of the cosine under a tangent …) split the line
//! into intervals; `f` is defined on each interval or on none of it, which one sample
//! point settles. A sign chart of `f′` (or `f″`) works the same way with the zeros of
//! `f′` added, giving increasing and decreasing intervals, the first-derivative test and
//! concavity.

mod points;

use std::cmp::Ordering;

use crate::calc::{diff, limit};
use crate::domains::sets::{Piece, RealSet};
use crate::domains::util;
use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode, LimitDir};
use crate::simplify::simplify;
use crate::steps::Steps;
use crate::table;

use points::{
    cmp_values, eval, inside, is_exact, num, open, point, push_unique, range_of, same_value,
    sample_between, slope_at, sort_values, trouble, value_at, within, zeros, Zeros,
};

/// The kind of a critical point by the first-derivative test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    LocalMax,
    LocalMin,
    Neither,
}

/// A critical point `(x, f(x))` with its kind.
#[derive(Clone, Debug, PartialEq)]
pub struct Critical {
    pub x: Expr,
    pub y: Expr,
    pub kind: Kind,
    /// True when `f′(x) = 0`; false when `f′` is undefined there (a corner or cusp).
    pub stationary: bool,
}

/// The sign of `f′` or `f″` on one open interval.
#[derive(Clone, Debug, PartialEq)]
pub struct Signed {
    pub piece: Piece,
    pub sign: Ordering,
}

/// What the analysis found about the largest or smallest value.
#[derive(Clone, Debug, PartialEq)]
pub struct Absolute {
    /// The value, when it is attained.
    pub value: Option<Expr>,
    /// Where it is attained.
    pub at: Vec<Expr>,
}

/// One function of one variable under analysis, optionally restricted to an interval.
pub struct Analysis {
    f: Expr,
    var: String,
    within: Option<Piece>,
    /// Remarks for the reader (numerical fallbacks, assumptions).
    pub notes: Vec<String>,
    /// The working, when it is being recorded.
    pub steps: Option<Steps>,
    d1: Option<Expr>,
    d2: Option<Expr>,
    /// Denominators of the function as it was written, before simplification cancelled
    /// them (`x − 1` in `(x² − 1)/(x − 1)`): `f` is undefined where they vanish.
    excluded: Vec<Expr>,
}

/// Attained `(x, f(x))` values and the limits at open ends of an interval.
type Candidates = (Vec<(Expr, Expr)>, Vec<Expr>);

/// One part of the summary table.
type Section = fn(&mut Analysis) -> MathResult<Expr>;

/// Picks the variable of `f`: `hint` when given, the only free symbol, or `x`.
pub fn variable_of(f: &Expr, hint: Option<&str>) -> MathResult<String> {
    if let Some(h) = hint {
        return Ok(h.to_string());
    }
    let free: Vec<String> = f.free_symbols().iter().map(|s| s.to_string()).collect();
    match free.as_slice() {
        [v] => Ok(v.clone()),
        [] => Ok("x".into()),
        many if many.iter().any(|v| v == "x") => Ok("x".into()),
        many => Err(MathError::Undefined(format!(
            "the variable of {f} (it could be any of {}; write the function as f(x) := …)",
            many.join(", ")
        ))),
    }
}

/// An interval value (`[0, 4]`, `(0, ∞)`) as a piece.
pub fn interval_of(e: &Expr) -> MathResult<Piece> {
    match e.node() {
        ExprNode::Interval {
            lo,
            hi,
            lo_closed,
            hi_closed,
        } => {
            let (Some(a), Some(b)) = (num(lo), num(hi)) else {
                return Err(MathError::Domain {
                    function: "the interval".into(),
                    detail: format!("its ends must be numbers, not {e}"),
                });
            };
            if a > b {
                return Err(MathError::Domain {
                    function: "the interval".into(),
                    detail: format!("{lo} is larger than {hi}"),
                });
            }
            Ok(Piece {
                lo: lo.clone(),
                hi: hi.clone(),
                lo_closed: *lo_closed && !lo.is_infinite(),
                hi_closed: *hi_closed && !hi.is_infinite(),
            })
        }
        _ => Err(MathError::Domain {
            function: "the helper".into(),
            detail: format!("expected an interval such as [0, 4], not {e}"),
        }),
    }
}

fn set(items: Vec<Expr>) -> Expr {
    Expr::new(ExprNode::Set(items))
}

fn pair(x: Expr, y: Expr) -> Expr {
    Expr::new(ExprNode::Tuple(vec![x, y]))
}

fn whole_line() -> Piece {
    open(Expr::neg_infinity(), Expr::infinity())
}

impl Analysis {
    /// Analyses `f` in `var`, on `within` when given (otherwise on all of ℝ).
    pub fn new(f: &Expr, var: &str, within: Option<Piece>) -> MathResult<Analysis> {
        let f = simplify(f);
        let others: Vec<String> = f
            .free_symbols()
            .iter()
            .filter(|s| &***s != var)
            .map(|s| s.to_string())
            .collect();
        if !others.is_empty() {
            return Err(MathError::Unsupported(format!(
                "analysing {f} while {} has no value (give it a value first)",
                others.join(", ")
            )));
        }
        let mut probe = Vec::new();
        trouble(&f, var, &mut probe)?;
        Ok(Analysis {
            f,
            var: var.to_string(),
            within,
            notes: Vec::new(),
            steps: None,
            d1: None,
            d2: None,
            excluded: Vec::new(),
        })
    }

    /// The denominators of the function as written. Simplifying `(x² − 1)/(x − 1)` to
    /// `x + 1` loses the point `x = 1` where the original is undefined; with its
    /// denominators the analysis keeps it out of the domain and reports the hole.
    pub fn with_excluded(mut self, denominators: Vec<Expr>) -> Analysis {
        for d in denominators {
            let d = simplify(&d);
            if d.contains_sym(&self.var) && !self.excluded.contains(&d) {
                self.excluded.push(d);
            }
        }
        self.d1 = None;
        self.d2 = None;
        self
    }

    /// Records the working from now on.
    pub fn with_steps(mut self) -> Analysis {
        self.steps = Some(Steps::new());
        self
    }

    /// The function.
    pub fn function(&self) -> &Expr {
        &self.f
    }

    /// The variable.
    pub fn var(&self) -> &str {
        &self.var
    }

    fn x(&self) -> Expr {
        Expr::sym(&self.var)
    }

    fn step(&mut self, rule: &str, before: &Expr, after: &Expr, note: Option<String>) {
        if let Some(s) = self.steps.as_mut() {
            s.push(rule, before.clone(), after.clone(), note);
        }
    }

    fn note(&mut self, text: impl Into<String>) {
        let text = text.into();
        if !self.notes.contains(&text) {
            self.notes.push(text);
        }
    }

    /// `f′`.
    pub fn first_derivative(&mut self) -> MathResult<Expr> {
        if let Some(d) = &self.d1 {
            return Ok(d.clone());
        }
        let d = diff(&self.f, &self.var, 1, None)?;
        let holder = crate::calc::derivative_holder(&self.f, &self.var);
        self.step("differentiate", &holder, &d, None);
        self.d1 = Some(d.clone());
        Ok(d)
    }

    /// `f″`.
    pub fn second_derivative(&mut self) -> MathResult<Expr> {
        if let Some(d) = &self.d2 {
            return Ok(d.clone());
        }
        let d1 = self.first_derivative()?;
        let d = diff(&d1, &self.var, 1, None)?;
        let holder = crate::calc::derivative_holder(&d1, &self.var);
        self.step("differentiate again", &holder, &d, None);
        self.d2 = Some(d.clone());
        Ok(d)
    }

    fn region(&self) -> Piece {
        self.within.clone().unwrap_or_else(whole_line)
    }

    fn bounded(&self) -> bool {
        let r = self.region();
        !r.lo.is_infinite() && !r.hi.is_infinite()
    }

    fn zeros_of(&mut self, g: &Expr) -> MathResult<Zeros> {
        let range = self.within.as_ref().map(range_of);
        let mut notes = Vec::new();
        let z = zeros(g, &self.var, range.as_ref(), &mut notes)?;
        for n in notes {
            self.note(n);
        }
        Ok(z)
    }

    /// The points where `g` may stop being defined, inside the region.
    fn breaks_of(&mut self, g: &Expr) -> MathResult<Zeros> {
        let mut parts = Vec::new();
        trouble(g, &self.var, &mut parts)?;
        // A point where the function as written is undefined breaks every derivative too.
        parts.extend(self.excluded.iter().cloned());
        let mut all = Zeros {
            exact: true,
            ..Zeros::default()
        };
        let mut seen: Vec<Expr> = Vec::new();
        for p in parts {
            if seen.contains(&p) {
                continue;
            }
            seen.push(p.clone());
            let z = self.zeros_of(&p)?;
            all.merge(z);
        }
        Ok(all)
    }

    fn periodic_error(&self, what: &str) -> MathError {
        MathError::Unsupported(format!(
            "{what} of a periodic function on all of ℝ (give an interval, as in \"on $[0, 2\\pi]$\")"
        ))
    }

    fn defined(&self, x: &Expr) -> Option<Expr> {
        let holed = self
            .excluded
            .iter()
            .any(|d| value_at(d, &self.var, x).is_some_and(|v| util::is_zero(&v)));
        if holed {
            return None;
        }
        value_at(&self.f, &self.var, x)
    }

    /// The domain as a union of intervals (inside the given interval, if any).
    pub fn domain(&mut self) -> MathResult<RealSet> {
        let breaks = self.breaks_of(&self.f.clone())?;
        if breaks.periodic() {
            return Err(self.periodic_error("the domain"));
        }
        let region = self.region();
        let mut cuts: Vec<Expr> = breaks
            .values
            .into_iter()
            .filter(|v| inside(&region, v))
            .collect();
        sort_values(&mut cuts);
        let mut ends = vec![region.lo.clone()];
        ends.extend(cuts.iter().cloned());
        ends.push(region.hi.clone());
        let mut pieces = Vec::new();
        for w in ends.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            let (Some(av), Some(bv)) = (num(a), num(b)) else {
                return Err(MathError::Unsupported(format!(
                    "ordering the points {a} and {b}"
                )));
            };
            if eval(&self.f, &self.var, sample_between(av, bv)).is_some() {
                pieces.push(open(a.clone(), b.clone()));
            }
        }
        for c in &cuts {
            if self.defined(c).is_some() {
                pieces.push(point(c.clone()));
            }
        }
        if region.lo_closed && self.defined(&region.lo).is_some() {
            pieces.push(point(region.lo.clone()));
        }
        if region.hi_closed && self.defined(&region.hi).is_some() {
            pieces.push(point(region.hi.clone()));
        }
        let d = RealSet::from_pieces(pieces)?;
        // Holes: the function as written is undefined there, but the simplified one is
        // not, so the discontinuity is removable.
        for c in &cuts {
            if self.defined(c).is_none() {
                let v = value_at(&self.f, &self.var, c).or_else(|| {
                    self.lim(c, LimitDir::Both)
                        .ok()
                        .filter(|l| !l.is_infinite() && !l.is_undefined() && num(l).is_some())
                });
                if let Some(v) = v {
                    let x = &self.var;
                    self.note(format!(
                        "removable discontinuity at {x} = {c}: f is undefined there but its limit is {v}, so the graph has a hole at ({c}, {v})"
                    ));
                }
            }
        }
        self.step(
            "find the domain",
            &self.f.clone(),
            &d.to_expr(),
            Some(
                "exclude the points where a denominator, logarithm or even root breaks down".into(),
            ),
        );
        Ok(d)
    }

    /// The sign of `g` on the open intervals of the domain between its zeros and breaks.
    /// Neighbouring intervals with the same sign are joined across a point where
    /// `joinable` holds.
    fn chart(
        &mut self,
        g: &Expr,
        joinable: &dyn Fn(&Analysis, &Expr) -> bool,
    ) -> MathResult<Vec<Signed>> {
        let dom = self.domain()?;
        let z = self.zeros_of(g)?;
        let b = self.breaks_of(g)?;
        if z.periodic() || b.periodic() {
            return Err(self.periodic_error("a sign chart"));
        }
        let mut cuts = z.values;
        for v in b.values {
            push_unique(&mut cuts, v);
        }
        sort_values(&mut cuts);
        let mut out: Vec<Signed> = Vec::new();
        for p in dom.pieces() {
            if same_value(&p.lo, &p.hi) {
                continue;
            }
            let mut ends = vec![p.lo.clone()];
            ends.extend(cuts.iter().filter(|c| inside(p, c)).cloned());
            ends.push(p.hi.clone());
            let mut run: Option<Signed> = None;
            for w in ends.windows(2) {
                let (Some(av), Some(bv)) = (num(&w[0]), num(&w[1])) else {
                    continue;
                };
                let s = eval(g, &self.var, sample_between(av, bv));
                let sign = match (z.everywhere, s) {
                    (true, _) => Ordering::Equal,
                    (false, Some(v)) => v.partial_cmp(&0.0).unwrap_or(Ordering::Equal),
                    (false, None) => continue,
                };
                let piece = open(w[0].clone(), w[1].clone());
                match run.as_mut() {
                    Some(r)
                        if r.sign == sign
                            && same_value(&r.piece.hi, &w[0])
                            && joinable(self, &w[0]) =>
                    {
                        r.piece.hi = w[1].clone();
                    }
                    _ => {
                        if let Some(r) = run.take() {
                            out.push(r);
                        }
                        run = Some(Signed { piece, sign });
                    }
                }
            }
            if let Some(r) = run {
                out.push(r);
            }
        }
        Ok(out)
    }

    /// Sign chart of `f′` (intervals joined across points where `f` is defined).
    pub fn first_derivative_chart(&mut self) -> MathResult<Vec<Signed>> {
        let d1 = self.first_derivative()?;
        self.chart(&d1, &|a, p| a.defined(p).is_some())
    }

    /// Sign chart of `f″` (intervals joined across points where `f′` is defined).
    pub fn second_derivative_chart(&mut self) -> MathResult<Vec<Signed>> {
        let d2 = self.second_derivative()?;
        let d1 = self.first_derivative()?;
        let var = self.var.clone();
        self.chart(&d2, &move |a, p| {
            a.defined(p).is_some() && slope_at(&d1, &var, p).is_some()
        })
    }

    /// The critical points: interior points of the domain where `f′ = 0` or `f′` is
    /// undefined, each classified by the first-derivative test.
    pub fn critical_points(&mut self) -> MathResult<Vec<Critical>> {
        let d1 = self.first_derivative()?;
        let dom = self.domain()?;
        let z = self.zeros_of(&d1)?;
        if z.periodic() {
            return Err(self.periodic_error("the critical points"));
        }
        if z.everywhere {
            self.note("f′ is 0 everywhere, so f is constant and every point is critical");
            return Ok(Vec::new());
        }
        let b = self.breaks_of(&d1)?;
        let var = self.var.clone();
        let mut cands: Vec<(Expr, bool)> = z
            .values
            .into_iter()
            .map(|v| {
                let stationary = slope_at(&d1, &var, &v).is_some();
                (v, stationary)
            })
            .collect();
        for v in b.values {
            if slope_at(&d1, &self.var, &v).is_none()
                && !cands.iter().any(|(c, _)| same_value(c, &v))
            {
                cands.push((v, false));
            }
        }
        cands.sort_by(|a, b| {
            num(&a.0)
                .unwrap_or(f64::NAN)
                .total_cmp(&num(&b.0).unwrap_or(f64::NAN))
        });
        let chart = self.chart(&d1, &|_, _| false)?;
        let mut out = Vec::new();
        for (c, stationary) in cands {
            if !dom.pieces().iter().any(|p| inside(p, &c)) {
                continue;
            }
            let Some(y) = self.defined(&c) else {
                continue;
            };
            let left = chart
                .iter()
                .find(|s| same_value(&s.piece.hi, &c))
                .map(|s| s.sign);
            let right = chart
                .iter()
                .find(|s| same_value(&s.piece.lo, &c))
                .map(|s| s.sign);
            let kind = match (left, right) {
                (Some(Ordering::Greater), Some(Ordering::Less)) => Kind::LocalMax,
                (Some(Ordering::Less), Some(Ordering::Greater)) => Kind::LocalMin,
                _ => Kind::Neither,
            };
            out.push(Critical {
                x: c,
                y,
                kind,
                stationary,
            });
        }
        let shown = set(out.iter().map(|c| c.x.clone()).collect());
        self.step(
            "solve f′(x) = 0 and find where f′ is undefined",
            &Expr::equation(d1, Expr::zero()),
            &shown,
            Some("first-derivative test: f′ changes from + to − at a local maximum and from − to + at a local minimum".into()),
        );
        Ok(out)
    }

    /// Open intervals where `f` increases (first) and decreases (second).
    pub fn monotonicity(&mut self) -> MathResult<(RealSet, RealSet)> {
        let chart = self.first_derivative_chart()?;
        let pick = |want: Ordering| -> MathResult<RealSet> {
            RealSet::from_pieces(
                chart
                    .iter()
                    .filter(|s| s.sign == want)
                    .map(|s| s.piece.clone())
                    .collect(),
            )
        };
        Ok((pick(Ordering::Greater)?, pick(Ordering::Less)?))
    }

    /// Open intervals where `f` is concave up (first) and concave down (second).
    pub fn concavity(&mut self) -> MathResult<(RealSet, RealSet)> {
        let chart = self.second_derivative_chart()?;
        let pick = |want: Ordering| -> MathResult<RealSet> {
            RealSet::from_pieces(
                chart
                    .iter()
                    .filter(|s| s.sign == want)
                    .map(|s| s.piece.clone())
                    .collect(),
            )
        };
        Ok((pick(Ordering::Greater)?, pick(Ordering::Less)?))
    }

    /// Points `(x, f(x))` where the concavity changes.
    pub fn inflection_points(&mut self) -> MathResult<Vec<Expr>> {
        let d2 = self.second_derivative()?;
        let chart = self.chart(&d2, &|_, _| false)?;
        let mut out = Vec::new();
        for w in chart.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            if !same_value(&a.piece.hi, &b.piece.lo) {
                continue;
            }
            let changes = matches!(
                (a.sign, b.sign),
                (Ordering::Greater, Ordering::Less) | (Ordering::Less, Ordering::Greater)
            );
            if !changes {
                continue;
            }
            let p = a.piece.hi.clone();
            if let Some(y) = self.defined(&p) {
                out.push(pair(p, y));
            }
        }
        Ok(out)
    }

    /// x-intercepts `(c, 0)` and the y-intercept `(0, f(0))`.
    pub fn intercepts(&mut self) -> MathResult<(Vec<Expr>, Option<Expr>)> {
        let dom = self.domain()?;
        let z = self.zeros_of(&self.f.clone())?;
        if z.periodic() {
            return Err(self.periodic_error("the x-intercepts"));
        }
        let xs: Vec<Expr> = z
            .values
            .into_iter()
            .filter(|v| dom.pieces().iter().any(|p| within(p, v)))
            .filter(|v| self.defined(v).is_some())
            .map(|v| pair(v, Expr::zero()))
            .collect();
        let zero = Expr::zero();
        let y = if dom.pieces().iter().any(|p| within(p, &zero)) {
            self.defined(&zero).map(|v| pair(Expr::zero(), v))
        } else {
            None
        };
        Ok((xs, y))
    }

    /// One-sided limit of `f` at `at` (`dir` Left or Right) or at ±∞.
    fn lim(&mut self, at: &Expr, dir: LimitDir) -> MathResult<Expr> {
        let dir = if at.is_infinite() {
            LimitDir::Both
        } else {
            dir
        };
        limit(&self.f, &self.var, at, dir, None)
    }

    /// Candidates for the extreme values on `piece`: attained values and the limits at
    /// its open ends. `None` for a limit means it does not exist (bounded oscillation).
    fn candidates(&mut self, piece: &Piece, crit: &[Critical]) -> MathResult<Candidates> {
        let mut attained: Vec<(Expr, Expr)> = Vec::new();
        for c in crit {
            if within(piece, &c.x) {
                attained.push((c.x.clone(), c.y.clone()));
            }
        }
        let mut limits = Vec::new();
        for (end, closed, dir) in [
            (&piece.lo, piece.lo_closed, LimitDir::Right),
            (&piece.hi, piece.hi_closed, LimitDir::Left),
        ] {
            if closed {
                if let Some(v) = self.defined(end) {
                    attained.push((end.clone(), v));
                }
                continue;
            }
            match self.lim(end, dir) {
                Ok(v) if v.is_undefined() => {
                    return Err(MathError::Unsupported(format!(
                        "the behaviour of {} near {end}",
                        self.f
                    )))
                }
                Ok(v) => limits.push(v),
                Err(MathError::LimitDoesNotExist(_)) => {
                    return Err(MathError::Unsupported(format!(
                        "deciding the extreme values of {}, which oscillates as {} → {end}",
                        self.f, self.var
                    )))
                }
                Err(e) => return Err(e),
            }
        }
        Ok((attained, limits))
    }

    /// The absolute maximum and minimum on the given interval (or the whole domain), by
    /// comparing the critical values, the values at closed ends and the limits at open
    /// ends (the closed interval method).
    pub fn absolute_extrema(&mut self) -> MathResult<(Absolute, Absolute)> {
        let dom = self.domain()?;
        let crit = self.critical_points()?;
        let mut attained: Vec<(Expr, Expr)> = Vec::new();
        let mut limits: Vec<Expr> = Vec::new();
        for p in dom.pieces() {
            if same_value(&p.lo, &p.hi) {
                if let Some(v) = self.defined(&p.lo) {
                    attained.push((p.lo.clone(), v));
                }
                continue;
            }
            let (a, l) = self.candidates(p, &crit)?;
            attained.extend(a);
            limits.extend(l);
        }
        let pick = |want: Ordering| -> MathResult<Absolute> {
            let mut best: Option<Expr> = None;
            for (_, v) in &attained {
                best = match best {
                    None => Some(v.clone()),
                    Some(b) => match cmp_values(v, &b) {
                        Some(o) if o == want => Some(v.clone()),
                        Some(_) => Some(b),
                        None => {
                            return Err(MathError::Unsupported(format!(
                                "comparing the values {v} and {b}"
                            )))
                        }
                    },
                };
            }
            let Some(best) = best else {
                return Ok(Absolute {
                    value: None,
                    at: Vec::new(),
                });
            };
            for l in &limits {
                match cmp_values(l, &best) {
                    Some(o) if o == want => {
                        return Ok(Absolute {
                            value: None,
                            at: Vec::new(),
                        })
                    }
                    Some(_) => {}
                    None => {
                        return Err(MathError::Unsupported(format!(
                            "comparing the value {best} with the limit {l}"
                        )))
                    }
                }
            }
            let mut at: Vec<Expr> = Vec::new();
            for (x, v) in &attained {
                if same_value(v, &best) {
                    push_unique(&mut at, x.clone());
                }
            }
            sort_values(&mut at);
            Ok(Absolute {
                value: Some(best),
                at,
            })
        };
        let max = pick(Ordering::Greater)?;
        let min = pick(Ordering::Less)?;
        if self.bounded()
            && self
                .within
                .as_ref()
                .is_some_and(|p| p.lo_closed && p.hi_closed)
        {
            self.step(
                "closed interval method",
                &self.f.clone(),
                &set(attained
                    .iter()
                    .map(|(x, v)| pair(x.clone(), v.clone()))
                    .collect()),
                Some("compare f at the critical points and at the endpoints".into()),
            );
        }
        Ok((max, min))
    }

    /// The range: on each interval of the domain `f` is continuous, so its values run
    /// between the smallest and largest of the critical values, closed-end values and
    /// end limits; an end is included when a value there is attained.
    pub fn range(&mut self) -> MathResult<RealSet> {
        let dom = self.domain()?;
        let crit = self.critical_points()?;
        let mut pieces = Vec::new();
        for p in dom.pieces() {
            if same_value(&p.lo, &p.hi) {
                if let Some(v) = self.defined(&p.lo) {
                    pieces.push(point(v));
                }
                continue;
            }
            let (attained, limits) = self.candidates(p, &crit)?;
            let mut all: Vec<(Expr, bool)> = attained.into_iter().map(|(_, v)| (v, true)).collect();
            all.extend(limits.into_iter().map(|v| (v, false)));
            if all.is_empty() {
                return Err(MathError::Unsupported(format!("the range of {}", self.f)));
            }
            let mut lo = all[0].clone();
            let mut hi = all[0].clone();
            for (v, att) in &all[1..] {
                match cmp_values(v, &lo.0) {
                    Some(Ordering::Less) => lo = (v.clone(), *att),
                    Some(Ordering::Equal) => lo.1 |= *att,
                    Some(Ordering::Greater) => {}
                    None => return Err(MathError::Unsupported(format!("the range of {}", self.f))),
                }
                match cmp_values(v, &hi.0) {
                    Some(Ordering::Greater) => hi = (v.clone(), *att),
                    Some(Ordering::Equal) => hi.1 |= *att,
                    Some(Ordering::Less) => {}
                    None => return Err(MathError::Unsupported(format!("the range of {}", self.f))),
                }
            }
            pieces.push(Piece {
                lo_closed: lo.1 && !lo.0.is_infinite(),
                hi_closed: hi.1 && !hi.0.is_infinite(),
                lo: lo.0,
                hi: hi.0,
            });
        }
        let r = RealSet::from_pieces(pieces)?;
        self.step(
            "find the range",
            &self.f.clone(),
            &r.to_expr(),
            Some("f is continuous on each interval of its domain, so its values fill the interval between its least and greatest values and limits there".into()),
        );
        Ok(r)
    }

    /// Vertical (`x = a`), horizontal (`y = L`) and slant (`y = mx + b`) asymptotes.
    pub fn asymptotes(&mut self) -> MathResult<(Vec<Expr>, Vec<Expr>, Vec<Expr>)> {
        let dom = self.domain()?;
        let breaks = self.breaks_of(&self.f.clone())?;
        let x = self.x();
        let mut vertical = Vec::new();
        for p in &breaks.values {
            if self.defined(p).is_some() {
                continue;
            }
            let mut hits = false;
            for (dir, side) in [(LimitDir::Left, -1.0), (LimitDir::Right, 1.0)] {
                let Some(pv) = num(p) else { continue };
                let probe = pv + side * 1e-7 * pv.abs().max(1.0);
                if eval(&self.f, &self.var, probe).is_none() {
                    continue;
                }
                if let Ok(v) = self.lim(p, dir) {
                    hits |= v.is_infinite();
                }
            }
            if hits {
                vertical.push(Expr::equation(x.clone(), p.clone()));
            }
        }
        let mut horizontal = Vec::new();
        let mut slant = Vec::new();
        let ends = [
            (Expr::infinity(), dom.pieces().last().map(|p| p.hi.clone())),
            (
                Expr::neg_infinity(),
                dom.pieces().first().map(|p| p.lo.clone()),
            ),
        ];
        for (end, reach) in ends {
            if !reach.is_some_and(|r| same_value(&r, &end)) {
                continue;
            }
            let v = match self.lim(&end, LimitDir::Both) {
                Ok(v) => v,
                Err(MathError::LimitDoesNotExist(_)) => continue,
                Err(e) => {
                    self.note(format!(
                        "the behaviour as {} → {end} could not be decided ({e})",
                        self.var
                    ));
                    continue;
                }
            };
            if !v.is_infinite() && !v.is_undefined() {
                let eq = Expr::equation(Expr::sym("y"), v);
                if !horizontal.contains(&eq) {
                    horizontal.push(eq);
                }
                continue;
            }
            if let Some(line) = self.slant_at(&end)? {
                if !slant.contains(&line) {
                    slant.push(line);
                }
            }
        }
        Ok((vertical, horizontal, slant))
    }

    fn slant_at(&mut self, end: &Expr) -> MathResult<Option<Expr>> {
        let x = self.x();
        // A rational function: the quotient of the long division when the top's degree
        // is one more than the bottom's.
        let (n, d) = crate::domains::solve::numer_denom(&self.f, &self.var);
        if let (Some(pn), Some(pd)) = (n.as_poly(&self.var), d.as_poly(&self.var)) {
            if pn.len() == pd.len() + 1 && pd.len() >= 2 {
                let (q, _) = crate::poly::poly_div(&n, &d, &self.var)?;
                return Ok(Some(Expr::equation(Expr::sym("y"), q)));
            }
            return Ok(None);
        }
        let ratio = simplify(&(self.f.clone() / x.clone()));
        let Ok(m) = limit(&ratio, &self.var, end, LimitDir::Both, None) else {
            return Ok(None);
        };
        if m.is_infinite() || m.is_undefined() || util::is_zero(&m) {
            return Ok(None);
        }
        let rest = simplify(&(self.f.clone() - m.clone() * x.clone()));
        let Ok(b) = limit(&rest, &self.var, end, LimitDir::Both, None) else {
            return Ok(None);
        };
        if b.is_infinite() || b.is_undefined() {
            return Ok(None);
        }
        Ok(Some(Expr::equation(Expr::sym("y"), simplify(&(m * x + b)))))
    }

    /// `f(a)` and `f′(a)` for the lines at `x = a` (an error when `f` is undefined there).
    fn at(&mut self, a: &Expr) -> MathResult<(Expr, Option<Expr>)> {
        let Some(fa) = self.defined(a) else {
            return Err(MathError::Domain {
                function: self.f.to_string(),
                detail: format!("it is undefined at {} = {a}", self.var),
            });
        };
        let d1 = self.first_derivative()?;
        Ok((fa, slope_at(&d1, &self.var, a)))
    }

    /// The tangent line at `x = a`: `y = f(a) + f′(a)(x − a)`, or `x = a` when the tangent
    /// is vertical.
    pub fn tangent(&mut self, a: &Expr) -> MathResult<Expr> {
        let (fa, slope) = self.at(a)?;
        let x = self.x();
        match slope {
            Some(m) => {
                let line = simplify(&(fa.clone() + m.clone() * (x.clone() - a.clone())));
                let eq = Expr::equation(Expr::sym("y"), line);
                self.step(
                    "tangent line",
                    &self.f.clone(),
                    &eq,
                    Some(format!(
                        "y = f(a) + f′(a)(x − a) with a = {a}, f(a) = {fa}, f′(a) = {m}"
                    )),
                );
                Ok(eq)
            }
            None if self.vertical_tangent(a)? => Ok(Expr::equation(x, a.clone())),
            None => Err(MathError::Undefined(format!(
                "the tangent line at {} = {a} (f has a corner there)",
                self.var
            ))),
        }
    }

    fn vertical_tangent(&mut self, a: &Expr) -> MathResult<bool> {
        let d1 = self.first_derivative()?;
        let mut sides = Vec::new();
        for dir in [LimitDir::Left, LimitDir::Right] {
            if let Ok(v) = limit(&d1, &self.var, a, dir, None) {
                sides.push(v);
            }
        }
        Ok(!sides.is_empty() && sides.iter().all(Expr::is_infinite))
    }

    /// The normal line at `x = a`: perpendicular to the tangent.
    pub fn normal(&mut self, a: &Expr) -> MathResult<Expr> {
        let (fa, slope) = self.at(a)?;
        let x = self.x();
        match slope {
            Some(m) if util::is_zero(&m) => Ok(Expr::equation(x, a.clone())),
            Some(m) => {
                let line = simplify(&(fa - (x - a.clone()) / m));
                Ok(Expr::equation(Expr::sym("y"), line))
            }
            None if self.vertical_tangent(a)? => Ok(Expr::equation(Expr::sym("y"), fa)),
            None => Err(MathError::Undefined(format!(
                "the normal line at {} = {a} (f has a corner there)",
                self.var
            ))),
        }
    }

    /// The linearisation `L(x) = f(a) + f′(a)(x − a)`.
    pub fn linearisation(&mut self, a: &Expr) -> MathResult<Expr> {
        let (fa, slope) = self.at(a)?;
        let Some(m) = slope else {
            return Err(MathError::Undefined(format!(
                "the linearisation at {} = {a} (f is not differentiable there)",
                self.var
            )));
        };
        let x = self.x();
        let l = simplify(&(fa + m * (x.clone() - a.clone())));
        Ok(Expr::equation(Expr::apply("L", vec![x]), l))
    }

    /// True when `[a, b]` lies in the domain (so `f`, an elementary function, is
    /// continuous on it).
    fn continuous_on(&mut self, a: &Expr, b: &Expr) -> MathResult<bool> {
        let saved = self.within.replace(Piece {
            lo: a.clone(),
            hi: b.clone(),
            lo_closed: true,
            hi_closed: true,
        });
        let dom = self.domain();
        self.within = saved;
        let dom = dom?;
        Ok(match dom.pieces() {
            [p] => same_value(&p.lo, a) && same_value(&p.hi, b) && p.lo_closed && p.hi_closed,
            _ => false,
        })
    }

    /// True when `f′` exists at every point of `(a, b)`.
    fn differentiable_on(&mut self, a: &Expr, b: &Expr) -> MathResult<bool> {
        let d1 = self.first_derivative()?;
        let saved = self.within.replace(open(a.clone(), b.clone()));
        let breaks = self.breaks_of(&d1);
        self.within = saved;
        let breaks = breaks?;
        if breaks.periodic() {
            return Err(self.periodic_error("differentiability"));
        }
        Ok(breaks
            .values
            .iter()
            .filter(|p| inside(&open(a.clone(), b.clone()), p))
            .all(|p| slope_at(&d1, &self.var, p).is_some()))
    }

    fn bounds(interval: &Piece) -> MathResult<(Expr, Expr)> {
        if interval.lo.is_infinite() || interval.hi.is_infinite() {
            return Err(MathError::Domain {
                function: "the theorem".into(),
                detail: "it needs a bounded interval [a, b]".into(),
            });
        }
        Ok((interval.lo.clone(), interval.hi.clone()))
    }

    /// The mean value theorem on `[a, b]`: the hypotheses, the average rate of change and
    /// the points `c` in `(a, b)` with `f′(c)` equal to it.
    pub fn mean_value(&mut self, interval: &Piece) -> MathResult<Expr> {
        let (a, b) = Self::bounds(interval)?;
        let cont = self.continuous_on(&a, &b)?;
        let diffable = cont && self.differentiable_on(&a, &b)?;
        let mut rows = vec![
            (
                format!("continuous on [{a}, {b}]"),
                Expr::new(ExprNode::Bool(cont)),
            ),
            (
                format!("differentiable on ({a}, {b})"),
                Expr::new(ExprNode::Bool(diffable)),
            ),
        ];
        if !cont {
            self.note("f is not continuous on the whole interval, so the mean value theorem does not apply");
            return Ok(table::table(rows));
        }
        let (fa, fb) = match (self.defined(&a), self.defined(&b)) {
            (Some(fa), Some(fb)) => (fa, fb),
            _ => return Err(MathError::Undefined("f at the ends of the interval".into())),
        };
        let slope = simplify(&((fb - fa) / (b.clone() - a.clone())));
        rows.push(("average rate of change".into(), slope.clone()));
        let d1 = self.first_derivative()?;
        let saved = self.within.replace(open(a.clone(), b.clone()));
        let z = self.zeros_of(&(d1 - slope));
        self.within = saved;
        let cs: Vec<Expr> = z?
            .values
            .into_iter()
            .filter(|c| inside(&open(a.clone(), b.clone()), c))
            .collect();
        if !diffable {
            self.note("f is not differentiable on the whole open interval, so the mean value theorem does not promise a point c");
        }
        rows.push(("c with f′(c) equal to it".into(), set(cs)));
        Ok(table::table(rows))
    }

    /// The intermediate value theorem on `[a, b]` for the value `target`: whether `f` is
    /// continuous there, whether `target` lies between `f(a)` and `f(b)`, and the points
    /// `c` with `f(c) = target`.
    pub fn intermediate_value(&mut self, interval: &Piece, target: &Expr) -> MathResult<Expr> {
        let (a, b) = Self::bounds(interval)?;
        let cont = self.continuous_on(&a, &b)?;
        let mut rows = vec![(
            format!("continuous on [{a}, {b}]"),
            Expr::new(ExprNode::Bool(cont)),
        )];
        let (Some(fa), Some(fb)) = (self.defined(&a), self.defined(&b)) else {
            return Err(MathError::Undefined("f at the ends of the interval".into()));
        };
        rows.push((format!("f({a})"), fa.clone()));
        rows.push((format!("f({b})"), fb.clone()));
        let between = match (cmp_values(&fa, target), cmp_values(target, &fb)) {
            (Some(x), Some(y)) => x == Ordering::Equal || y == Ordering::Equal || x == y,
            _ => {
                return Err(MathError::Unsupported(format!(
                    "comparing {target} with f({a}) and f({b})"
                )))
            }
        };
        rows.push((
            format!("{target} lies between f({a}) and f({b})"),
            Expr::new(ExprNode::Bool(between)),
        ));
        if cont && between {
            let saved = self.within.replace(interval.clone());
            let z = self.zeros_of(&(self.f.clone() - target.clone()));
            self.within = saved;
            let cs: Vec<Expr> = z?
                .values
                .into_iter()
                .filter(|c| within(interval, c))
                .collect();
            rows.push((format!("c with f(c) = {target}"), set(cs)));
        } else {
            self.note("the hypotheses of the intermediate value theorem do not hold, so it promises nothing here");
        }
        Ok(table::table(rows))
    }

    /// Critical points as a set of `x` values.
    pub fn critical_set(&mut self) -> MathResult<Expr> {
        let c = self.critical_points()?;
        Ok(set(c.into_iter().map(|c| c.x).collect()))
    }

    /// Local maxima and minima as `(x, f(x))` points.
    pub fn local_extrema(&mut self) -> MathResult<(Vec<Expr>, Vec<Expr>)> {
        let crit = self.critical_points()?;
        let of = |k: Kind| -> Vec<Expr> {
            crit.iter()
                .filter(|c| c.kind == k)
                .map(|c| pair(c.x.clone(), c.y.clone()))
                .collect()
        };
        Ok((of(Kind::LocalMax), of(Kind::LocalMin)))
    }

    fn absolute_rows(&mut self) -> MathResult<Vec<(String, Expr)>> {
        let (max, min) = self.absolute_extrema()?;
        let show = |a: &Absolute| -> Expr {
            match &a.value {
                Some(v) => set(a.at.iter().map(|x| pair(x.clone(), v.clone())).collect()),
                None => set(Vec::new()),
            }
        };
        if max.value.is_none() {
            self.note("f has no absolute maximum here: its values approach or exceed every value it attains");
        }
        if min.value.is_none() {
            self.note("f has no absolute minimum here: its values approach or go below every value it attains");
        }
        Ok(vec![
            ("absolute maximum".into(), show(&max)),
            ("absolute minimum".into(), show(&min)),
        ])
    }

    /// Local and absolute extrema as a table.
    pub fn extrema_table(&mut self, local: bool, absolute: bool) -> MathResult<Expr> {
        let mut rows = Vec::new();
        if local {
            let (maxima, minima) = self.local_extrema()?;
            rows.push(("local maxima".to_string(), set(maxima)));
            rows.push(("local minima".to_string(), set(minima)));
        }
        if absolute {
            rows.extend(self.absolute_rows()?);
        }
        Ok(table::table(rows))
    }

    /// Increasing and decreasing intervals as a table.
    pub fn monotonicity_table(&mut self) -> MathResult<Expr> {
        let (up, down) = self.monotonicity()?;
        Ok(table::table(vec![
            ("increasing on".into(), up.to_expr()),
            ("decreasing on".into(), down.to_expr()),
        ]))
    }

    /// Concavity as a table, with the inflection points.
    pub fn concavity_table(&mut self) -> MathResult<Expr> {
        let (up, down) = self.concavity()?;
        let infl = self.inflection_points()?;
        Ok(table::table(vec![
            ("concave up on".into(), up.to_expr()),
            ("concave down on".into(), down.to_expr()),
            ("inflection points".into(), set(infl)),
        ]))
    }

    /// Intercepts as a table.
    pub fn intercepts_table(&mut self) -> MathResult<Expr> {
        let (xs, y) = self.intercepts()?;
        Ok(table::table(vec![
            ("x-intercepts".into(), set(xs)),
            ("y-intercept".into(), set(y.into_iter().collect())),
        ]))
    }

    /// Asymptotes as a table.
    pub fn asymptotes_table(&mut self) -> MathResult<Expr> {
        let (v, h, s) = self.asymptotes()?;
        Ok(table::table(vec![
            ("vertical".into(), set(v)),
            ("horizontal".into(), set(h)),
            ("slant".into(), set(s)),
        ]))
    }

    /// The full curve-sketch summary: every property in one table. A row the tools could
    /// not settle is left out, and a note says why.
    pub fn summary(&mut self) -> MathResult<Expr> {
        let mut rows: Vec<(String, Expr)> = vec![("f(x)".into(), self.f.clone())];
        let dom = self.domain()?;
        rows.push(("domain".into(), dom.to_expr()));
        match self.range() {
            Ok(r) => rows.push(("range".into(), r.to_expr())),
            Err(e) => self.note(format!("the range was not found: {e}")),
        }
        let sections: [(&str, Section); 5] = [
            ("intercepts", Analysis::intercepts_table),
            ("critical points", |a| {
                let c = a.critical_set()?;
                Ok(table::table(vec![("critical points".into(), c)]))
            }),
            ("extrema", |a| a.extrema_table(true, false)),
            ("monotonicity", Analysis::monotonicity_table),
            ("concavity", Analysis::concavity_table),
        ];
        for (what, f) in sections {
            match f(self) {
                Ok(t) => rows.extend(table::rows(&t).unwrap_or_default()),
                Err(e) => self.note(format!("the {what} were not found: {e}")),
            }
        }
        match self.asymptotes_table() {
            Ok(t) => {
                for (label, v) in table::rows(&t).unwrap_or_default() {
                    rows.push((format!("{label} asymptotes"), v));
                }
            }
            Err(e) => self.note(format!("the asymptotes were not found: {e}")),
        }
        if !rows.iter().all(|(_, v)| is_exact(v)) {
            self.note("values with decimals were found numerically (≈)");
        }
        Ok(table::table(rows))
    }
}

#[cfg(test)]
mod tests;
