//! Sequences defined by recurrences, such as `a(n) = a(n−1) + a(n−2)` with `a(0) = 0`,
//! `a(1) = 1`: exact terms with memoisation, and closed forms for linear recurrences with
//! constant coefficients (characteristic roots, so Fibonacci gives Binet's formula).

use std::fmt;
use std::sync::Mutex;

use num_rational::BigRational;
use num_traits::{One, Zero};

use crate::calc::diff::diff;
use crate::error::{MathError, MathResult};
use crate::expand::expand;
use crate::expr::{Expr, ExprNode};
use crate::poly::Poly;
use crate::simplify::{add, mul, pow, simplify};
use crate::steps::Steps;

/// Most terms computed for one sequence.
pub const MAX_TERMS: i64 = 100_000;
/// Largest recurrence order.
pub const MAX_ORDER: usize = 32;
/// Largest size (in nodes) of one exact term; symbolic terms past this are refused.
const MAX_TERM_SIZE: usize = 20_000;

fn placeholder(j: usize) -> String {
    format!("\u{1}{j}")
}

/// A sequence `name(index)` given by initial terms and a rule for the next one.
pub struct Recurrence {
    name: String,
    index: String,
    start: i64,
    initial: Vec<Expr>,
    rule: Expr,
    template: Expr,
    order: usize,
    cache: Mutex<Vec<Expr>>,
}

impl Clone for Recurrence {
    fn clone(&self) -> Self {
        let cache = self.cache.lock().map(|c| c.clone()).unwrap_or_default();
        Recurrence {
            name: self.name.clone(),
            index: self.index.clone(),
            start: self.start,
            initial: self.initial.clone(),
            rule: self.rule.clone(),
            template: self.template.clone(),
            order: self.order,
            cache: Mutex::new(cache),
        }
    }
}

impl fmt::Debug for Recurrence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

impl fmt::Display for Recurrence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (name, n) = (&self.name, &self.index);
        write!(f, "{name}({n}) = {}", self.rule)?;
        for (i, v) in self.initial.iter().enumerate() {
            write!(f, ", {name}({}) = {v}", self.start + i as i64)?;
        }
        Ok(())
    }
}

/// Replaces `name(index − j)` with placeholders; returns the template and the order.
fn compile(rule: &Expr, name: &str, index: &str) -> MathResult<(Expr, usize)> {
    let mut order = 0usize;
    let mut err = None;
    let template = replace_calls(rule, name, index, &mut order, &mut err);
    match err {
        Some(e) => Err(e),
        None => Ok((template, order)),
    }
}

fn replace_calls(
    e: &Expr,
    name: &str,
    index: &str,
    order: &mut usize,
    err: &mut Option<MathError>,
) -> Expr {
    if let ExprNode::Apply(f, args) = e.node() {
        if &**f == name {
            let bad = |why: &str| MathError::Domain {
                function: format!("the recurrence for {name}"),
                detail: why.into(),
            };
            let [arg] = args.as_slice() else {
                *err = Some(bad("the sequence takes one index"));
                return e.clone();
            };
            let back = simplify(&add(vec![
                Expr::sym(index),
                mul(vec![Expr::int(-1), arg.clone()]),
            ]));
            match back
                .as_number()
                .and_then(|v| v.as_i64())
                .and_then(|j| usize::try_from(j).ok())
            {
                Some(j) if (1..=MAX_ORDER).contains(&j) => {
                    *order = (*order).max(j);
                    return Expr::sym(&placeholder(j));
                }
                _ => {
                    *err = Some(bad(&format!(
                        "{name}({arg}) is not an earlier term {name}({index} − j) with 1 ≤ j ≤ {MAX_ORDER}"
                    )));
                    return e.clone();
                }
            }
        }
    }
    let node = e.map_children(&mut |c| replace_calls(c, name, index, order, err));
    crate::simplify::canonical(node)
}

impl Recurrence {
    /// `name(index) = rule` for `index ≥ start + initial.len()`, where `rule` refers to
    /// earlier terms as `name(index − j)` (user-function applications), and `initial` holds
    /// `name(start), name(start + 1), …`.
    pub fn new(
        name: &str,
        index: &str,
        start: i64,
        initial: Vec<Expr>,
        rule: &Expr,
    ) -> MathResult<Recurrence> {
        let rule = simplify(rule);
        let (template, order) = compile(&rule, name, index)?;
        if initial.len() < order {
            return Err(MathError::Domain {
                function: format!("the recurrence for {name}"),
                detail: format!(
                    "a rule of order {order} needs {order} initial terms, but {} were given",
                    initial.len()
                ),
            });
        }
        Ok(Recurrence {
            name: name.into(),
            index: index.into(),
            start,
            initial: initial.iter().map(simplify).collect(),
            rule,
            template,
            order,
            cache: Mutex::new(Vec::new()),
        })
    }

    /// The same as [`Recurrence::new`] with the rule written for `name(index + shift)`,
    /// as in `a(n+1) = 2a(n) + 1`.
    pub fn with_shift(
        name: &str,
        index: &str,
        start: i64,
        initial: Vec<Expr>,
        rule: &Expr,
        shift: i64,
    ) -> MathResult<Recurrence> {
        let back = add(vec![Expr::sym(index), Expr::int(-shift)]);
        Recurrence::new(name, index, start, initial, &rule.subs(index, &back))
    }

    /// The order (how many earlier terms the rule uses).
    pub fn order(&self) -> usize {
        self.order
    }

    /// The first index.
    pub fn start(&self) -> i64 {
        self.start
    }

    /// The initial terms.
    pub fn initial(&self) -> &[Expr] {
        &self.initial
    }

    /// The rule as given.
    pub fn rule(&self) -> &Expr {
        &self.rule
    }

    /// The term `name(n)`, exactly.
    pub fn nth(&self, n: i64) -> MathResult<Expr> {
        if n < self.start {
            return Err(MathError::Domain {
                function: self.name.clone(),
                detail: format!("the sequence starts at {}", self.start),
            });
        }
        let idx = n - self.start;
        if idx >= MAX_TERMS {
            return Err(MathError::TooLarge(format!("term {n} of {}", self.name)));
        }
        let idx = usize::try_from(idx).map_err(|_| MathError::TooLarge(format!("term {n}")))?;
        let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        while cache.len() <= idx {
            let m = cache.len();
            let value = if let Some(v) = self.initial.get(m) {
                v.clone()
            } else {
                let at = self.start + m as i64;
                let mut pairs: Vec<(String, Expr)> = (1..=self.order)
                    .map(|j| (placeholder(j), cache[m - j].clone()))
                    .collect();
                pairs.push((self.index.clone(), Expr::int(at)));
                let refs: Vec<(&str, Expr)> =
                    pairs.iter().map(|(s, v)| (s.as_str(), v.clone())).collect();
                let v = simplify(&self.template.subs_all(&refs));
                if v.is_undefined() {
                    return Err(MathError::Undefined(format!("{}({at})", self.name)));
                }
                if v.size() > MAX_TERM_SIZE {
                    return Err(MathError::TooLarge(format!("the term {}({at})", self.name)));
                }
                v
            };
            cache.push(value);
        }
        Ok(cache[idx].clone())
    }

    /// The terms `name(start) … name(start + count − 1)`.
    pub fn terms(&self, count: usize) -> MathResult<Vec<Expr>> {
        let last = self.start + i64::try_from(count).unwrap_or(i64::MAX) - 1;
        if count == 0 {
            return Ok(Vec::new());
        }
        self.nth(last)?;
        (self.start..=last).map(|n| self.nth(n)).collect()
    }

    /// A closed form in the index for a linear recurrence with constant coefficients
    /// (`a(n) = c₁a(n−1) + … + c_m a(n−m) + d`), from the roots of the characteristic
    /// polynomial. The answer is checked against the computed terms.
    pub fn closed_form(&self, steps: Option<&mut Steps>) -> MathResult<Expr> {
        let mut log = steps;
        let m = self.order;
        let unsupported = || {
            MathError::Unsupported(format!(
                "a closed form for {} (not linear with constant coefficients)",
                self.name
            ))
        };
        if m == 0 {
            return Ok(self.rule.clone());
        }
        if self.initial.len() != m {
            return Err(unsupported());
        }
        let mut cs = Vec::with_capacity(m);
        for j in 1..=m {
            let c = diff(&self.template, &placeholder(j), 1, None)?;
            let r = c.as_number().and_then(|v| v.as_rational()).cloned();
            cs.push(r.ok_or_else(unsupported)?);
        }

        let names: Vec<String> = (1..=m).map(placeholder).collect();
        let mut zero_pairs: Vec<(&str, Expr)> = Vec::new();
        for s in &names {
            zero_pairs.push((s.as_str(), Expr::zero()));
        }
        let d = simplify(&self.template.subs_all(&zero_pairs));
        if d.contains_sym(&self.index) || names.iter().any(|s| d.contains_sym(s)) {
            return Err(unsupported());
        }
        // Linearity: template − Σ cⱼ pⱼ − d must vanish.
        let mut check = vec![self.template.clone(), mul(vec![Expr::int(-1), d.clone()])];
        for (j, c) in cs.iter().enumerate() {
            check.push(mul(vec![Expr::ratio(-c.clone()), Expr::sym(&names[j])]));
        }
        if !expand(&add(check))?.is_zero() {
            return Err(unsupported());
        }
        if cs.last().is_some_and(Zero::is_zero) {
            return Err(unsupported());
        }
        // Characteristic polynomial λ^m − Σ cⱼ λ^{m−j}.
        let mut coeffs = vec![BigRational::zero(); m + 1];
        coeffs[m] = BigRational::one();
        for (j, c) in cs.iter().enumerate() {
            coeffs[m - (j + 1)] = -c.clone();
        }
        let chi = Poly::new(coeffs);
        let lambda = Expr::sym("λ");
        let roots = characteristic_roots(&chi).ok_or_else(unsupported)?;
        Steps::record(
            &mut log,
            "characteristic equation",
            &chi.to_expr("λ"),
            &lambda,
            Some(format!(
                "roots {}",
                roots
                    .iter()
                    .map(|(r, k)| if *k > 1 {
                        format!("{r} (×{k})")
                    } else {
                        r.to_string()
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        );
        // Particular solution for the constant d.
        let s: BigRational = cs.iter().sum();
        let particular = if d.is_zero() {
            Expr::zero()
        } else if s.is_one() {
            return Err(unsupported());
        } else {
            simplify(&mul(vec![
                d.clone(),
                Expr::ratio((BigRational::one() - s).recip()),
            ]))
        };
        let n = Expr::sym(&self.index);
        // Basis solutions n^i · rⁿ, one per root and multiplicity.
        let basis: Vec<(Expr, i64)> = roots
            .iter()
            .flat_map(|(r, mult)| (0..*mult as i64).map(move |i| (r.clone(), i)))
            .collect();
        let basis_at = |(r, i): &(Expr, i64), x: &Expr| {
            mul(vec![
                pow(x.clone(), Expr::int(*i)),
                pow(r.clone(), x.clone()),
            ])
        };
        let mut matrix: Vec<Vec<Expr>> = Vec::with_capacity(m);
        let mut rhs = Vec::with_capacity(m);
        for (row, init) in self.initial.iter().enumerate() {
            let at = Expr::int(self.start + row as i64);
            matrix.push(basis.iter().map(|b| simplify(&basis_at(b, &at))).collect());
            rhs.push(simplify(&add(vec![
                init.clone(),
                mul(vec![Expr::int(-1), particular.clone()]),
            ])));
        }
        let consts = solve(matrix, rhs).ok_or_else(unsupported)?;
        let mut parts = vec![particular];
        for (a, b) in consts.iter().zip(&basis) {
            parts.push(mul(vec![a.clone(), basis_at(b, &n)]));
        }
        let closed = simplify(&add(parts));
        // Check against the computed terms.
        for k in 0..(m as i64 + 6) {
            let at = self.start + k;
            let exact = self.nth(at)?.eval_f64(&crate::eval::EmptyEnv);
            let formula = closed.eval_f64(&[(self.index.as_str(), at as f64)]);
            match (exact, formula) {
                (Ok(a), Ok(b)) if (a - b).abs() <= 1e-8 * a.abs().max(1.0) => {}
                (Err(_), _) => {}
                _ => {
                    return Err(MathError::Inconclusive(format!(
                        "the closed form {closed} for {}",
                        self.name
                    )))
                }
            }
        }
        Steps::record(
            &mut log,
            "solve for the constants",
            &Expr::apply(&self.name, vec![n]),
            &closed,
            Some("the initial terms fix the constants".into()),
        );
        Ok(closed)
    }
}

/// Roots of the characteristic polynomial with multiplicities: rational roots, then the
/// quadratic formula for what is left.
fn characteristic_roots(p: &Poly) -> Option<Vec<(Expr, usize)>> {
    let mut out = Vec::new();
    for (part, mult) in p.square_free() {
        let mult = mult as usize;
        let mut rest = part.clone();
        for r in rest.clone().rational_roots()? {
            let lin = Poly::new(vec![-r.clone(), BigRational::one()]);
            let (q, rem) = rest.div_rem(&lin).ok()?;
            if !rem.is_zero() {
                return None;
            }
            rest = q;
            out.push((Expr::ratio(r), mult));
        }
        match rest.degree() {
            Some(0) | None => {}
            Some(2) => {
                let c = rest.coeffs();
                let (a, b, cc) = (
                    Expr::ratio(c[2].clone()),
                    Expr::ratio(c[1].clone()),
                    Expr::ratio(c[0].clone()),
                );
                let disc = simplify(&add(vec![
                    pow(b.clone(), Expr::int(2)),
                    mul(vec![Expr::int(-4), a.clone(), cc]),
                ]));
                let sq = pow(disc, Expr::rational(1, 2));
                let two_a = mul(vec![Expr::int(2), a]);
                for sign in [1, -1] {
                    let root = simplify(&mul(vec![
                        add(vec![
                            mul(vec![Expr::int(-1), b.clone()]),
                            mul(vec![Expr::int(sign), sq.clone()]),
                        ]),
                        pow(two_a.clone(), Expr::int(-1)),
                    ]));
                    out.push((root, mult));
                }
            }
            _ => return None,
        }
    }
    Some(out)
}

fn is_zero_expr(e: &Expr) -> bool {
    e.is_zero() || expand(e).is_ok_and(|x| x.is_zero())
}

/// Gaussian elimination over expressions.
fn solve(mut a: Vec<Vec<Expr>>, mut b: Vec<Expr>) -> Option<Vec<Expr>> {
    let n = b.len();
    let tidy = |e: Expr| {
        let s = simplify(&e);
        expand(&s).map(|x| simplify(&x)).unwrap_or(s)
    };
    for col in 0..n {
        let pivot = (col..n).find(|&r| !is_zero_expr(&a[r][col]))?;
        a.swap(col, pivot);
        b.swap(col, pivot);
        let inv = pow(a[col][col].clone(), Expr::int(-1));
        for r in 0..n {
            if r == col || is_zero_expr(&a[r][col]) {
                continue;
            }
            let factor = simplify(&mul(vec![a[r][col].clone(), inv.clone()]));
            for c in col..n {
                let v = add(vec![
                    a[r][c].clone(),
                    mul(vec![Expr::int(-1), factor.clone(), a[col][c].clone()]),
                ]);
                a[r][c] = tidy(v);
            }
            let v = add(vec![
                b[r].clone(),
                mul(vec![Expr::int(-1), factor, b[col].clone()]),
            ]);
            b[r] = tidy(v);
        }
    }
    Some(
        (0..n)
            .map(|i| tidy(mul(vec![b[i].clone(), pow(a[i][i].clone(), Expr::int(-1))])))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n() -> Expr {
        Expr::sym("n")
    }
    fn a(arg: Expr) -> Expr {
        Expr::apply("a", vec![arg])
    }
    fn i(v: i64) -> Expr {
        Expr::int(v)
    }

    fn fibonacci() -> Recurrence {
        let rule = a(n() - i(1)) + a(n() - i(2));
        Recurrence::new("a", "n", 0, vec![i(0), i(1)], &rule).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn fibonacci_terms_are_exact() {
        let fib = fibonacci();
        assert_eq!(fib.order(), 2);
        assert_eq!(fib.nth(10), Ok(i(55)));
        assert_eq!(fib.nth(50), Ok(i(12_586_269_025)));
        let big = fib.nth(300).unwrap_or_else(|e| panic!("{e}"));
        assert!(big
            .to_string()
            .starts_with("222232244629420445529739893461909967206666939096499764990979600"));
        assert_eq!(fib.terms(6), Ok(vec![i(0), i(1), i(1), i(2), i(3), i(5)]));
        assert!(fib.nth(-1).is_err());
        assert!(fib.nth(MAX_TERMS + 5).is_err());
    }

    #[test]
    fn binet_formula() {
        let mut steps = Steps::new();
        let closed = fibonacci()
            .closed_form(Some(&mut steps))
            .unwrap_or_else(|e| panic!("{e}"));
        let v = closed.eval_f64(&[("n", 20.0)]).unwrap_or(f64::NAN);
        assert!((v - 6765.0).abs() < 1e-6, "{closed}");
        assert!(closed.to_string().contains("sqrt(5)"), "{closed}");
        let rules: Vec<&str> = steps.iter().map(|s| s.rule.as_str()).collect();
        assert_eq!(
            rules,
            vec!["characteristic equation", "solve for the constants"]
        );
    }

    #[test]
    fn first_order_and_repeated_roots() {
        // a(n+1) = 2a(n) + 1, a(0) = 0  →  2^n − 1
        let r = Recurrence::with_shift("a", "n", 0, vec![i(0)], &(i(2) * a(n()) + i(1)), 1)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r.nth(10), Ok(i(1023)));
        let c = r.closed_form(None).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(c, i(2).pow(n()) - i(1));
        // a(n) = 4a(n−1) − 4a(n−2): repeated root 2
        let rule = i(4) * a(n() - i(1)) - i(4) * a(n() - i(2));
        let r =
            Recurrence::new("a", "n", 0, vec![i(1), i(4)], &rule).unwrap_or_else(|e| panic!("{e}"));
        let c = r.closed_form(None).unwrap_or_else(|e| panic!("{e}"));
        let v = c.eval_f64(&[("n", 6.0)]).unwrap_or(f64::NAN);
        let exact = r
            .nth(6)
            .ok()
            .and_then(|e| e.eval_f64(&crate::eval::EmptyEnv).ok());
        assert_eq!(Some(v), exact);
    }

    #[test]
    fn nonlinear_and_invalid_rules() {
        // a(n) = a(n−1)² + n is fine to evaluate but has no closed form here.
        let rule = a(n() - i(1)).pow(i(2)) + n();
        let r = Recurrence::new("a", "n", 1, vec![i(1)], &rule).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(r.nth(3), Ok(i(12)));
        assert!(r.closed_form(None).is_err());
        assert!(Recurrence::new("a", "n", 0, vec![], &a(n() - i(1))).is_err());
        assert!(Recurrence::new("a", "n", 0, vec![i(1)], &a(n() + i(1))).is_err());
        // Symbolic growth is capped rather than running away.
        let rule = a(n() - i(1)) * (a(n() - i(1)) + Expr::sym("x"));
        let r = Recurrence::new("a", "n", 0, vec![Expr::sym("x")], &rule)
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(r.nth(40).is_err());
        assert!(r.to_string().starts_with("a(n) = "));
    }
}
