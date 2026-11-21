//! Random variables: `X \sim \mathrm{Uniform}[0, 1]`, `\mathcal N(\mu, \sigma^2)`,
//! Bernoulli, binomial, Poisson and exponential distributions as values.
//!
//! - **Exact answers from the distribution** ([`expectation`], [`variance`],
//!   [`probability`]): `E[g]` for a polynomial `g` in independent random variables is
//!   worked out from the exact raw moments of each distribution (linearity, and
//!   `E[XY] = E[X] E[Y]` for different variables, which are independent). Discrete
//!   variables with a finite support small enough to list also take any `g`. `P(event)`
//!   for a condition on one random variable (`X ≤ a`, `a < X ≤ b`, `X = k`, `2X + 1 > 3`)
//!   comes from the exact distribution function. Anything else is an error, never an
//!   approximation passed off as exact.
//! - **Empirical answers from samples** ([`monte_carlo_sum`]): a sum over the members
//!   of i.i.d. families (`\sum_{j=1}^N \mathbf 1[X_j^2 + Y_j^2 \le 1]`) draws the samples
//!   from the seeded generator ([`super::rng::Rng`], behind `%seed`). Each family has its
//!   own stream, derived from the document seed and the family's name, so `X_7` is the
//!   same number in every line of a document and on every computer.

use std::fmt;

use num_bigint::BigInt;
use num_traits::{One, Signed, ToPrimitive, Zero};

use super::rng::Rng;
use crate::domains::distributions::{binom_cdf, binom_pmf, poisson_cdf, poisson_pmf};
use crate::domains::fieldkit::NumericFn;
use crate::domains::util::{compare, numeric, rational, sign};
use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode, LogicOp, RelOp};
use crate::functions::Func;

/// Highest moment worked out exactly.
const MAX_MOMENT: usize = 64;
/// Largest finite support summed term by term.
const MAX_SUPPORT: u64 = 2_000;
/// Most random draws (counting every Bernoulli trial and Poisson piece) for one sum.
const MAX_DRAWS: u64 = 200_000_000;

/// A probability distribution with its parameters (exact expressions where written so).
#[derive(Debug, Clone, PartialEq)]
pub enum Dist {
    /// Uniform on `[a, b]`.
    Uniform { a: Expr, b: Expr },
    /// Normal with mean μ and variance σ² (the textbook `𝒩(μ, σ²)`).
    Normal { mean: Expr, var: Expr },
    /// 1 with probability p, else 0.
    Bernoulli { p: Expr },
    /// Successes in n independent trials with probability p.
    Binomial { n: Expr, p: Expr },
    /// Poisson with mean λ.
    Poisson { rate: Expr },
    /// Exponential with rate λ (mean 1/λ).
    Exponential { rate: Expr },
}

/// The distribution names understood after `∼`, with their parameter counts.
const NAMES: &[(&str, &str, usize)] = &[
    ("Uniform", "Uniform", 2),
    ("Unif", "Uniform", 2),
    ("U", "Uniform", 2),
    ("Normal", "Normal", 2),
    ("Gaussian", "Normal", 2),
    ("N", "Normal", 2),
    ("Bernoulli", "Bernoulli", 1),
    ("Bern", "Bernoulli", 1),
    ("Ber", "Bernoulli", 1),
    ("Binomial", "Binomial", 2),
    ("Bin", "Binomial", 2),
    ("Binom", "Binomial", 2),
    ("Poisson", "Poisson", 1),
    ("Pois", "Poisson", 1),
    ("Po", "Poisson", 1),
    ("Exponential", "Exponential", 1),
    ("Exp", "Exponential", 1),
    ("Expo", "Exponential", 1),
];

/// The canonical name and parameter count of a distribution name as written
/// (`Unif` → `Uniform`), or `None` when it is not one.
pub fn canonical_name(written: &str) -> Option<(&'static str, usize)> {
    NAMES
        .iter()
        .find(|(w, _, _)| *w == written)
        .map(|(_, c, n)| (*c, *n))
}

fn bad(name: &str, detail: impl Into<String>) -> MathError {
    MathError::Domain {
        function: name.into(),
        detail: detail.into(),
    }
}

/// Known to be ≥ 0 (`Some(false)` when known negative, `None` when not known).
fn nonnegative(e: &Expr) -> Option<bool> {
    sign(e).map(|s| s != std::cmp::Ordering::Less)
}

fn positive(e: &Expr) -> Option<bool> {
    sign(e).map(|s| s == std::cmp::Ordering::Greater)
}

impl Dist {
    /// The distribution `name` (canonical or as written) with `params`, checking every
    /// parameter whose value is known.
    pub fn new(name: &str, params: Vec<Expr>) -> MathResult<Dist> {
        let (canon, count) =
            canonical_name(name).ok_or_else(|| MathError::UnknownFunction(name.into()))?;
        if params.len() != count {
            return Err(MathError::WrongArity {
                function: canon.into(),
                expected: count.to_string(),
                got: params.len(),
            });
        }
        let params: Vec<Expr> = params.iter().map(Expr::simplify).collect();
        let d = match canon {
            "Uniform" => Dist::Uniform {
                a: params[0].clone(),
                b: params[1].clone(),
            },
            "Normal" => Dist::Normal {
                mean: params[0].clone(),
                var: params[1].clone(),
            },
            "Bernoulli" => Dist::Bernoulli {
                p: params[0].clone(),
            },
            "Binomial" => Dist::Binomial {
                n: params[0].clone(),
                p: params[1].clone(),
            },
            "Poisson" => Dist::Poisson {
                rate: params[0].clone(),
            },
            _ => Dist::Exponential {
                rate: params[0].clone(),
            },
        };
        d.validate()?;
        Ok(d)
    }

    fn validate(&self) -> MathResult<()> {
        let prob = |p: &Expr| -> MathResult<()> {
            if nonnegative(p) == Some(false)
                || nonnegative(&(Expr::one() - p.clone())) == Some(false)
            {
                return Err(bad(
                    self.name(),
                    format!("the probability p must be between 0 and 1, not {p}"),
                ));
            }
            Ok(())
        };
        match self {
            Dist::Uniform { a, b } => {
                if positive(&(b - a)) == Some(false) {
                    return Err(bad(
                        "Uniform",
                        format!("the interval [{a}, {b}] must have a < b"),
                    ));
                }
            }
            Dist::Normal { var, .. } => {
                if positive(var) == Some(false) {
                    return Err(bad(
                        "Normal",
                        format!("the variance σ² must be positive, not {var}"),
                    ));
                }
            }
            Dist::Bernoulli { p } => prob(p)?,
            Dist::Binomial { n, p } => {
                prob(p)?;
                if n.is_constant() {
                    let ok = n
                        .as_number()
                        .and_then(|x| x.as_integer())
                        .is_some_and(|k| !k.is_negative());
                    if !ok {
                        return Err(bad(
                            "Binomial",
                            format!("the number of trials n must be a whole number ≥ 0, not {n}"),
                        ));
                    }
                }
            }
            Dist::Poisson { rate } | Dist::Exponential { rate } => {
                if positive(rate) == Some(false) {
                    return Err(bad(
                        self.name(),
                        format!("the rate λ must be positive, not {rate}"),
                    ));
                }
            }
        }
        Ok(())
    }

    /// The canonical name (`Uniform`, `Normal` …).
    pub fn name(&self) -> &'static str {
        match self {
            Dist::Uniform { .. } => "Uniform",
            Dist::Normal { .. } => "Normal",
            Dist::Bernoulli { .. } => "Bernoulli",
            Dist::Binomial { .. } => "Binomial",
            Dist::Poisson { .. } => "Poisson",
            Dist::Exponential { .. } => "Exponential",
        }
    }

    /// The parameters in the order they are written.
    pub fn params(&self) -> Vec<Expr> {
        match self {
            Dist::Uniform { a, b } => vec![a.clone(), b.clone()],
            Dist::Normal { mean, var } => vec![mean.clone(), var.clone()],
            Dist::Bernoulli { p } => vec![p.clone()],
            Dist::Binomial { n, p } => vec![n.clone(), p.clone()],
            Dist::Poisson { rate } | Dist::Exponential { rate } => vec![rate.clone()],
        }
    }

    /// The same distribution with every parameter passed through `f`.
    pub fn map_params(&self, f: &mut impl FnMut(&Expr) -> MathResult<Expr>) -> MathResult<Dist> {
        let params = self
            .params()
            .iter()
            .map(&mut *f)
            .collect::<MathResult<Vec<_>>>()?;
        Dist::new(self.name(), params)
    }

    /// True for the distributions on whole numbers.
    pub fn is_discrete(&self) -> bool {
        matches!(
            self,
            Dist::Bernoulli { .. } | Dist::Binomial { .. } | Dist::Poisson { .. }
        )
    }

    /// The mean, exactly.
    pub fn mean(&self) -> MathResult<Expr> {
        self.raw_moment(1)
    }

    /// The variance, exactly.
    pub fn variance(&self) -> MathResult<Expr> {
        Ok(match self {
            Dist::Uniform { a, b } => ((b - a).pow(2) / Expr::int(12)).simplify(),
            Dist::Normal { var, .. } => var.clone(),
            Dist::Bernoulli { p } => (p * &(Expr::one() - p.clone())).simplify(),
            Dist::Binomial { n, p } => (n * &(p * &(Expr::one() - p.clone()))).simplify(),
            Dist::Poisson { rate } => rate.clone(),
            Dist::Exponential { rate } => (Expr::one() / rate.pow(2)).simplify(),
        })
    }

    /// `E[X^k]`, exactly.
    pub fn raw_moment(&self, k: usize) -> MathResult<Expr> {
        if k > MAX_MOMENT {
            return Err(MathError::TooLarge(format!(
                "the moment E[X^{k}] (at most {MAX_MOMENT})"
            )));
        }
        if k == 0 {
            return Ok(Expr::one());
        }
        let ki = k as i64;
        let m = match self {
            Dist::Uniform { a, b } => {
                (b.pow(ki + 1) - a.pow(ki + 1)) / (Expr::int(ki + 1) * (b - a))
            }
            Dist::Normal { mean, var } => {
                // m_k = μ m_{k−1} + (k − 1) σ² m_{k−2}.
                let mut prev = Expr::one();
                let mut cur = mean.clone();
                for j in 2..=k {
                    let next = (mean * &cur + Expr::int(j as i64 - 1) * var.clone() * prev.clone())
                        .simplify();
                    prev = cur;
                    cur = next;
                }
                cur
            }
            Dist::Bernoulli { p } => p.clone(),
            Dist::Binomial { n, p } => {
                // Σ_j S(k, j) n(n−1)…(n−j+1) p^j.
                let s = stirling2_row(k);
                let mut terms = Vec::new();
                let mut falling = Expr::one();
                for (j, sj) in s.iter().enumerate().skip(1) {
                    falling = falling * (n - &Expr::int(j as i64 - 1));
                    if !sj.is_zero() {
                        terms.push(Expr::bigint(sj.clone()) * falling.clone() * p.pow(j as i64));
                    }
                }
                crate::simplify::add(terms)
            }
            Dist::Poisson { rate } => {
                let s = stirling2_row(k);
                let terms = s
                    .iter()
                    .enumerate()
                    .skip(1)
                    .filter(|(_, sj)| !sj.is_zero())
                    .map(|(j, sj)| Expr::bigint(sj.clone()) * rate.pow(j as i64))
                    .collect();
                crate::simplify::add(terms)
            }
            Dist::Exponential { rate } => {
                let fact: BigInt = (1..=ki).map(BigInt::from).product();
                Expr::bigint(fact) / rate.pow(ki)
            }
        };
        Ok(crate::domains::util::normalize(&m))
    }

    /// The finite support `0 … n` of a discrete distribution, when it is small enough
    /// to list.
    fn finite_support(&self) -> Option<u64> {
        match self {
            Dist::Bernoulli { .. } => Some(1),
            Dist::Binomial { n, .. } => n
                .as_number()
                .and_then(|x| x.as_integer())
                .and_then(|k| k.to_u64())
                .filter(|&k| k <= MAX_SUPPORT),
            _ => None,
        }
    }

    /// `P(X = k)` for a whole number k (discrete distributions).
    fn pmf(&self, k: &BigInt) -> MathResult<Expr> {
        let ke = Expr::bigint(k.clone());
        match self {
            Dist::Bernoulli { p } => Ok(if k.is_zero() {
                (Expr::one() - p.clone()).simplify()
            } else if k.is_one() {
                p.clone()
            } else {
                Expr::zero()
            }),
            Dist::Binomial { n, p } => {
                if !n.is_constant() || !p.is_constant() {
                    return Err(symbolic_params(self));
                }
                binom_pmf(n, p, &ke)
            }
            Dist::Poisson { rate } => {
                if !rate.is_constant() {
                    return Err(symbolic_params(self));
                }
                poisson_pmf(rate, &ke)
            }
            _ => Ok(Expr::zero()),
        }
    }

    /// `P(X ≤ x)` for a constant x.
    pub fn cdf(&self, x: &Expr) -> MathResult<Expr> {
        let undecided = || {
            MathError::Inconclusive(format!(
                "P(X ≤ {x}) for X ∼ {self}: where {x} lies is not known"
            ))
        };
        match self {
            Dist::Uniform { a, b } => {
                if compare(x, a).ok_or_else(undecided)? != std::cmp::Ordering::Greater {
                    return Ok(Expr::zero());
                }
                if compare(x, b).ok_or_else(undecided)? != std::cmp::Ordering::Less {
                    return Ok(Expr::one());
                }
                Ok(((x - a) / (b - a)).simplify())
            }
            Dist::Exponential { rate } => {
                if compare(x, &Expr::zero()).ok_or_else(undecided)? != std::cmp::Ordering::Greater {
                    return Ok(Expr::zero());
                }
                if x.is_infinite() {
                    return Ok(Expr::one());
                }
                Ok((Expr::one() - Expr::e().pow(-(rate * x))).simplify())
            }
            Dist::Normal { mean, var } => {
                if x.is_infinite() {
                    return Ok(
                        if compare(x, &Expr::zero()) == Some(std::cmp::Ordering::Less) {
                            Expr::zero()
                        } else {
                            Expr::one()
                        },
                    );
                }
                let z = (x - mean) / (Expr::int(2) * var.clone()).pow(Expr::rational(1, 2));
                let erf = crate::functions::func(Func::Erf, vec![z.simplify()]);
                Ok((Expr::rational(1, 2) * (Expr::one() + erf)).simplify())
            }
            Dist::Bernoulli { .. } | Dist::Binomial { .. } | Dist::Poisson { .. } => {
                let k = floor_of(x).ok_or_else(undecided)?;
                match k {
                    Bound::NegInf => Ok(Expr::zero()),
                    Bound::PosInf => Ok(Expr::one()),
                    Bound::At(k) => self.discrete_cdf(&k),
                }
            }
        }
    }

    fn discrete_cdf(&self, k: &BigInt) -> MathResult<Expr> {
        if k.is_negative() {
            return Ok(Expr::zero());
        }
        let ke = Expr::bigint(k.clone());
        match self {
            Dist::Bernoulli { p } => Ok(if k.is_zero() {
                (Expr::one() - p.clone()).simplify()
            } else {
                Expr::one()
            }),
            Dist::Binomial { n, p } => {
                if !n.is_constant() || !p.is_constant() {
                    return Err(symbolic_params(self));
                }
                binom_cdf(n, p, &ke)
            }
            Dist::Poisson { rate } => {
                if !rate.is_constant() {
                    return Err(symbolic_params(self));
                }
                poisson_cdf(rate, &ke)
            }
            _ => Err(MathError::Unsupported(
                "a discrete distribution function".into(),
            )),
        }
    }

    /// `P(X < x)` for a constant x.
    fn cdf_below(&self, x: &Expr) -> MathResult<Expr> {
        if !self.is_discrete() {
            return self.cdf(x);
        }
        let undecided = || {
            MathError::Inconclusive(format!(
                "P(X < {x}) for X ∼ {self}: where {x} lies is not known"
            ))
        };
        match ceil_of(x).ok_or_else(undecided)? {
            Bound::NegInf => Ok(Expr::zero()),
            Bound::PosInf => Ok(Expr::one()),
            Bound::At(k) => self.discrete_cdf(&(k - 1)),
        }
    }

    /// A sampler for numeric parameters.
    pub fn sampler(&self) -> MathResult<Sampler> {
        let num = |e: &Expr, what: &str| -> MathResult<f64> {
            numeric(e).ok_or_else(|| {
                MathError::NotANumber(format!(
                    "{what} of {self} has no number, so it cannot be sampled"
                ))
            })
        };
        Ok(match self {
            Dist::Uniform { a, b } => Sampler::Uniform(num(a, "a")?, num(b, "b")?),
            Dist::Normal { mean, var } => {
                Sampler::Normal(num(mean, "the mean")?, num(var, "the variance")?.sqrt())
            }
            Dist::Bernoulli { p } => Sampler::Bernoulli(num(p, "p")?),
            Dist::Binomial { n, p } => {
                let n = n
                    .as_number()
                    .and_then(|x| x.as_integer())
                    .and_then(|k| k.to_u64())
                    .filter(|&k| k <= 1_000_000)
                    .ok_or_else(|| {
                        MathError::TooLarge(format!("sampling {self} (n at most 10⁶)"))
                    })?;
                Sampler::Binomial(n, num(p, "p")?)
            }
            Dist::Poisson { rate } => {
                let l = num(rate, "λ")?;
                if l > 1e6 {
                    return Err(MathError::TooLarge(format!(
                        "sampling {self} (λ at most 10⁶)"
                    )));
                }
                Sampler::Poisson(l)
            }
            Dist::Exponential { rate } => Sampler::Exponential(num(rate, "λ")?),
        })
    }

    /// The distribution in LaTeX (`\mathrm{Uniform}[0, 1]`, `\mathcal{N}(0, 1)`).
    pub fn latex(&self) -> String {
        let p: Vec<String> = self.params().iter().map(Expr::to_latex).collect();
        match self {
            Dist::Uniform { .. } => format!("\\mathrm{{Uniform}}[{}, {}]", p[0], p[1]),
            Dist::Normal { .. } => format!("\\mathcal{{N}}\\left({}, {}\\right)", p[0], p[1]),
            _ => format!("\\mathrm{{{}}}\\left({}\\right)", self.name(), p.join(", ")),
        }
    }
}

impl fmt::Display for Dist {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let p: Vec<String> = self.params().iter().map(Expr::to_text).collect();
        match self {
            Dist::Uniform { .. } => write!(f, "Uniform[{}, {}]", p[0], p[1]),
            Dist::Normal { .. } => write!(f, "𝒩({}, {})", p[0], p[1]),
            _ => write!(f, "{}({})", self.name(), p.join(", ")),
        }
    }
}

fn symbolic_params(d: &Dist) -> MathError {
    MathError::Inconclusive(format!(
        "probabilities of {d}: its parameters need numbers for this"
    ))
}

/// A whole-number position, or one of the ends of the line.
enum Bound {
    NegInf,
    PosInf,
    At(BigInt),
}

fn int_round(x: &Expr, up: bool) -> Option<Bound> {
    if x.is_infinite() {
        return Some(
            if compare(x, &Expr::zero()) == Some(std::cmp::Ordering::Less) {
                Bound::NegInf
            } else {
                Bound::PosInf
            },
        );
    }
    if let Some(q) = rational(&x.simplify()) {
        return Some(Bound::At(
            if up { q.ceil() } else { q.floor() }.to_integer(),
        ));
    }
    let v = numeric(x)?;
    let r = if up { v.ceil() } else { v.floor() };
    // A value too close to a whole number to round safely in f64 is not decided.
    if (v - v.round()).abs() < 1e-9 || r.abs() > 9.0e15 {
        return None;
    }
    Some(Bound::At(BigInt::from(r as i64)))
}

fn floor_of(x: &Expr) -> Option<Bound> {
    int_round(x, false)
}

fn ceil_of(x: &Expr) -> Option<Bound> {
    int_round(x, true)
}

/// Row k of the Stirling numbers of the second kind, `S(k, 0) … S(k, k)`.
fn stirling2_row(k: usize) -> Vec<BigInt> {
    let mut row = vec![BigInt::one()];
    for n in 1..=k {
        let mut next = vec![BigInt::zero(); n + 1];
        for (j, slot) in next.iter_mut().enumerate().skip(1) {
            let keep = row.get(j).cloned().unwrap_or_default() * BigInt::from(j);
            let add = row.get(j - 1).cloned().unwrap_or_default();
            *slot = keep + add;
        }
        row = next;
    }
    row
}

/// Draws from a distribution with numeric parameters.
#[derive(Debug, Clone, Copy)]
pub enum Sampler {
    Uniform(f64, f64),
    Normal(f64, f64),
    Bernoulli(f64),
    Binomial(u64, f64),
    Poisson(f64),
    Exponential(f64),
}

/// Largest Poisson mean drawn in one piece (Knuth's product method needs `e^{-λ}`).
const POISSON_PIECE: f64 = 30.0;

impl Sampler {
    /// Random draws one sample costs (for the work limit).
    pub fn cost(&self) -> u64 {
        match self {
            Sampler::Binomial(n, _) => (*n).max(1),
            Sampler::Poisson(l) => (*l as u64).max(1),
            _ => 1,
        }
    }

    /// One sample.
    pub fn sample(&self, rng: &mut Rng) -> f64 {
        match *self {
            Sampler::Uniform(a, b) => rng.uniform_in(a, b),
            Sampler::Normal(m, sd) => m + sd * rng.normal(),
            Sampler::Bernoulli(p) => f64::from(u8::from(rng.uniform() < p)),
            Sampler::Binomial(n, p) => (0..n).filter(|_| rng.uniform() < p).count() as f64,
            Sampler::Poisson(l) => {
                // A sum of independent Poisson pieces is Poisson with the summed mean.
                let pieces = (l / POISSON_PIECE).ceil().max(1.0);
                let part = l / pieces;
                let limit = (-part).exp();
                let mut total = 0u64;
                for _ in 0..pieces as u64 {
                    let mut prod = rng.uniform();
                    while prod > limit {
                        total += 1;
                        prod *= rng.uniform();
                    }
                }
                total as f64
            }
            Sampler::Exponential(l) => -(1.0 - rng.uniform()).ln() / l,
        }
    }
}

/// SplitMix64's finaliser.
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// The seed of family `name`'s stream under document seed `seed` (FNV-1a of the name).
pub fn family_seed(seed: u64, name: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    mix(mix(seed) ^ h)
}

/// What a family member looks like inside an expression: its family's name and index.
pub type MemberOf<'a> = &'a dyn Fn(&Expr) -> Option<(String, Expr)>;

/// `Σ_{j=lo}^{hi} body` (or `Π` when `product`), where `body` holds members of i.i.d.
/// families (`members` recognises them; `families` gives each family's distribution).
/// Member j of a family is the j-th draw of its own stream (from index 0), so the same
/// member has the same value wherever it is used. The body may also use `var` itself;
/// every other name must already have a value.
#[allow(clippy::too_many_arguments)]
pub fn monte_carlo_sum(
    body: &Expr,
    var: &str,
    lo: i64,
    hi: i64,
    families: &[(String, Dist)],
    members: MemberOf<'_>,
    seed: u64,
    product: bool,
) -> MathResult<f64> {
    if lo < 0 {
        return Err(bad(
            "a Monte Carlo sum",
            format!("members of a random family start at index 0, so the sum cannot start at {lo}"),
        ));
    }
    if hi < lo {
        return Ok(if product { 1.0 } else { 0.0 });
    }
    let count = (hi - lo + 1) as u64;
    if hi as u64 + 1 > super::MAX_SAMPLES as u64 {
        return Err(MathError::TooLarge(format!(
            "a Monte Carlo sum up to index {hi} (at most {})",
            super::MAX_SAMPLES - 1
        )));
    }
    // Replace each member X_j by a placeholder name for its family.
    let mut used: Vec<usize> = Vec::new();
    let mut failure: Option<MathError> = None;
    let replaced = replace_members(body, &mut |e| {
        let (name, index) = members(e)?;
        let k = families.iter().position(|(n, _)| *n == name)?;
        if !index.is_sym(var) {
            failure.get_or_insert_with(|| {
                bad(
                    "a Monte Carlo sum",
                    format!(
                        "a member of the random family {name} inside the sum must use the sum's index {var}, not {index}"
                    ),
                )
            });
            return None;
        }
        if !used.contains(&k) {
            used.push(k);
        }
        Some(Expr::sym(&format!("#rv{k}")))
    });
    if let Some(err) = failure {
        return Err(err);
    }
    let mut names = vec![var.to_string()];
    names.extend(used.iter().map(|k| format!("#rv{k}")));
    if let Some(free) = replaced
        .free_symbols()
        .into_iter()
        .find(|s| !names.iter().any(|n| n == &**s))
    {
        return Err(MathError::UnboundVariable(free.to_string()));
    }
    let samplers: Vec<Sampler> = used
        .iter()
        .map(|&k| families[k].1.sampler())
        .collect::<MathResult<_>>()?;
    let cost: u64 = samplers.iter().map(Sampler::cost).sum::<u64>().max(1);
    if cost.saturating_mul(hi as u64 + 1) > MAX_DRAWS {
        return Err(MathError::TooLarge(format!(
            "a Monte Carlo sum needing more than {MAX_DRAWS} random draws"
        )));
    }
    let mut rngs: Vec<Rng> = used
        .iter()
        .map(|&k| Rng::seeded(family_seed(seed, &families[k].0)))
        .collect();
    // Members below the first index are drawn and skipped, so member j is always the
    // j-th draw of its stream.
    for _ in 0..lo {
        for (rng, s) in rngs.iter_mut().zip(&samplers) {
            s.sample(rng);
        }
    }
    let f = NumericFn::new(&replaced, &names);
    let mut point = vec![0.0; names.len()];
    // Kahan summation keeps a million terms accurate.
    let (mut acc, mut comp) = (if product { 1.0 } else { 0.0 }, 0.0f64);
    for i in 0..count {
        point[0] = (lo as u64 + i) as f64;
        for (slot, (rng, s)) in point[1..].iter_mut().zip(rngs.iter_mut().zip(&samplers)) {
            *slot = s.sample(rng);
        }
        let v = f.eval(&point)?;
        if !v.is_finite() {
            return Err(MathError::NotANumber(format!(
                "the term at {var} = {} is not a finite number",
                lo as u64 + i
            )));
        }
        if product {
            acc *= v;
        } else {
            let y = v - comp;
            let t = acc + y;
            comp = (t - acc) - y;
            acc = t;
        }
    }
    Ok(acc)
}

fn replace_members(e: &Expr, f: &mut impl FnMut(&Expr) -> Option<Expr>) -> Expr {
    if let Some(r) = f(e) {
        return r;
    }
    if e.children().is_empty() {
        return e.clone();
    }
    Expr::new(e.map_children(&mut |c| replace_members(c, f)))
}

// ---- exact expectations and probabilities --------------------------------------------

/// The random variables in `e` (by name, in `rvs` order).
fn present<'a>(e: &Expr, rvs: &'a [(String, Dist)]) -> Vec<&'a (String, Dist)> {
    rvs.iter().filter(|(n, _)| e.contains_sym(n)).collect()
}

/// `E[e]` where the names in `rvs` are independent random variables.
pub fn expectation(e: &Expr, rvs: &[(String, Dist)]) -> MathResult<Expr> {
    let e = e.simplify();
    if present(&e, rvs).is_empty() {
        return Ok(e);
    }
    // E[1[A]] = P(A).
    if let ExprNode::Piecewise { cases, otherwise } = e.node() {
        if let ([(v, cond)], Some(o)) = (cases.as_slice(), otherwise) {
            if v.is_one() && o.is_zero() {
                return probability(cond, rvs);
            }
        }
    }
    let expanded = crate::expand::expand(&e).unwrap_or(e.clone());
    let terms = match expanded.node() {
        ExprNode::Add(v) => v.clone(),
        _ => vec![expanded.clone()],
    };
    let mut out = Vec::with_capacity(terms.len());
    for t in &terms {
        out.push(term_expectation(t, rvs)?);
    }
    Ok(crate::domains::util::normalize(&crate::simplify::add(out)))
}

/// E of one product term: constants out, each variable's factors to its own moment.
fn term_expectation(t: &Expr, rvs: &[(String, Dist)]) -> MathResult<Expr> {
    let factors = match t.node() {
        ExprNode::Mul(v) => v.clone(),
        _ => vec![t.clone()],
    };
    let mut constant = Vec::new();
    let mut by_var: Vec<(usize, Vec<Expr>)> = Vec::new();
    for f in factors {
        let vars = present(&f, rvs);
        match vars.as_slice() {
            [] => constant.push(f),
            [(name, _)] => {
                let k = rvs.iter().position(|(n, _)| n == name).unwrap_or(0);
                match by_var.iter_mut().find(|(i, _)| *i == k) {
                    Some((_, fs)) => fs.push(f),
                    None => by_var.push((k, vec![f])),
                }
            }
            _ => {
                return Err(MathError::Unsupported(format!(
                "the expectation of {f}, which mixes several random variables inside one function"
            )))
            }
        }
    }
    let mut out = constant;
    for (k, fs) in by_var {
        let (name, dist) = &rvs[k];
        let g = crate::simplify::mul(fs);
        out.push(single_expectation(&g, name, dist)?);
    }
    Ok(crate::simplify::mul(out))
}

/// `E[g(X)]` for one random variable X ∼ `dist`.
fn single_expectation(g: &Expr, x: &str, dist: &Dist) -> MathResult<Expr> {
    if let Some(coeffs) = crate::poly::as_poly(g, x) {
        let mut terms = Vec::with_capacity(coeffs.len());
        for (k, c) in coeffs.iter().enumerate() {
            if !c.is_zero() {
                terms.push(c * &dist.raw_moment(k)?);
            }
        }
        return Ok(crate::simplify::add(terms));
    }
    if let Some(n) = dist.finite_support() {
        let mut terms = Vec::new();
        for k in 0..=n {
            let kb = BigInt::from(k);
            let p = dist.pmf(&kb)?;
            terms.push(g.subs_all(&[(x, Expr::int(k as i64))]) * p);
        }
        return Ok(crate::domains::util::normalize(&crate::simplify::add(
            terms,
        )));
    }
    Err(MathError::Unsupported(format!(
        "the exact expectation of {g} for {x} ∼ {dist} (exact for polynomials in {x}; a Monte Carlo sum over samples estimates the rest)"
    )))
}

/// `Var(e) = E[e²] − E[e]²` for independent random variables `rvs`.
pub fn variance(e: &Expr, rvs: &[(String, Dist)]) -> MathResult<Expr> {
    let e = e.simplify();
    if let Some(name) = e.as_sym() {
        if let Some((_, d)) = rvs.iter().find(|(n, _)| n == name) {
            return d.variance();
        }
    }
    let m1 = expectation(&e, rvs)?;
    let m2 = expectation(&e.pow(2), rvs)?;
    Ok(crate::domains::util::normalize(&(m2 - m1.pow(2))))
}

/// An interval of values for one variable: `(lower, closed)` and `(upper, closed)`.
#[derive(Clone)]
struct Window {
    lo: Option<(Expr, bool)>,
    hi: Option<(Expr, bool)>,
    eq: Option<Expr>,
    ne: Vec<Expr>,
}

/// `P(cond)` where `cond` is a condition on one of the random variables `rvs`.
pub fn probability(cond: &Expr, rvs: &[(String, Dist)]) -> MathResult<Expr> {
    let vars = present(cond, rvs);
    let (name, dist) = match vars.as_slice() {
        [one] => (&one.0, &one.1),
        [] => {
            return Err(MathError::Inconclusive(format!(
                "P({cond}): the condition uses no random variable"
            )))
        }
        _ => {
            return Err(MathError::Unsupported(format!(
                "the exact probability of {cond}, a condition on several random variables (a Monte Carlo sum over samples estimates it)"
            )))
        }
    };
    let mut w = Window {
        lo: None,
        hi: None,
        eq: None,
        ne: Vec::new(),
    };
    collect_window(cond, name, &mut w)?;
    window_probability(&w, dist, cond)
}

fn not_one_var(cond: &Expr, name: &str) -> MathError {
    MathError::Unsupported(format!(
        "the exact probability of {cond} (exact for conditions such as {name} ≤ a, a < {name} ≤ b or {name} = k, and linear ones such as 2{name} + 1 > 3)"
    ))
}

fn collect_window(cond: &Expr, x: &str, w: &mut Window) -> MathResult<()> {
    match cond.node() {
        ExprNode::Logic(LogicOp::And, parts) => {
            for p in parts {
                collect_window(p, x, w)?;
            }
            Ok(())
        }
        ExprNode::Relation(op, a, b) => {
            // a op b  ⇔  α x + β op 0 with α constant.
            let diff = (a - b).simplify();
            let coeffs = crate::poly::as_poly(&diff, x).ok_or_else(|| not_one_var(cond, x))?;
            let (beta, alpha) = match coeffs.as_slice() {
                [b0, a1] => (b0.clone(), a1.clone()),
                _ => return Err(not_one_var(cond, x)),
            };
            let s = sign(&alpha).ok_or_else(|| {
                MathError::Inconclusive(format!("P({cond}): the sign of {alpha} is not known"))
            })?;
            let at = (-beta / alpha).simplify();
            let flip = s == std::cmp::Ordering::Less;
            let op = match (op, flip) {
                (RelOp::Lt, true) => RelOp::Gt,
                (RelOp::Le, true) => RelOp::Ge,
                (RelOp::Gt, true) => RelOp::Lt,
                (RelOp::Ge, true) => RelOp::Le,
                (o, _) => *o,
            };
            match op {
                RelOp::Lt | RelOp::Le => tighten(&mut w.hi, at, op == RelOp::Le, true),
                RelOp::Gt | RelOp::Ge => tighten(&mut w.lo, at, op == RelOp::Ge, false),
                RelOp::Eq => {
                    if let Some(prev) = &w.eq {
                        if compare(prev, &at) != Some(std::cmp::Ordering::Equal) {
                            w.eq = Some(Expr::undefined());
                            return Ok(());
                        }
                    }
                    w.eq = Some(at);
                    Ok(())
                }
                RelOp::Ne => {
                    w.ne.push(at);
                    Ok(())
                }
                RelOp::Approx => Err(not_one_var(cond, x)),
            }
        }
        _ => Err(not_one_var(cond, x)),
    }
}

/// Keeps the tighter of two bounds.
fn tighten(slot: &mut Option<(Expr, bool)>, at: Expr, closed: bool, upper: bool) -> MathResult<()> {
    let replace = match slot {
        None => true,
        Some((prev, prev_closed)) => {
            let c = compare(&at, prev).ok_or_else(|| {
                MathError::Inconclusive(format!("which of {at} and {prev} is smaller is not known"))
            })?;
            match c {
                std::cmp::Ordering::Equal => *prev_closed && !closed,
                std::cmp::Ordering::Less => upper,
                std::cmp::Ordering::Greater => !upper,
            }
        }
    };
    if replace {
        *slot = Some((at, closed));
    }
    Ok(())
}

fn window_probability(w: &Window, dist: &Dist, cond: &Expr) -> MathResult<Expr> {
    if let Some(at) = &w.eq {
        if at.is_undefined() {
            return Ok(Expr::zero());
        }
        // X = a inside the other bounds.
        let inside_lo = match &w.lo {
            None => true,
            Some((l, closed)) => match compare(at, l) {
                Some(std::cmp::Ordering::Greater) => true,
                Some(std::cmp::Ordering::Equal) => *closed,
                Some(std::cmp::Ordering::Less) => false,
                None => return Err(undecided(cond)),
            },
        };
        let inside_hi = match &w.hi {
            None => true,
            Some((h, closed)) => match compare(at, h) {
                Some(std::cmp::Ordering::Less) => true,
                Some(std::cmp::Ordering::Equal) => *closed,
                Some(std::cmp::Ordering::Greater) => false,
                None => return Err(undecided(cond)),
            },
        };
        let excluded =
            w.ne.iter()
                .any(|n| compare(n, at) == Some(std::cmp::Ordering::Equal));
        if !inside_lo || !inside_hi || excluded {
            return Ok(Expr::zero());
        }
        return point_mass(dist, at, cond);
    }
    // P(lo ⋯ X ⋯ hi) = P(X ≤ hi or < hi) − P(X < lo or ≤ lo).
    let upper = match &w.hi {
        None => Expr::one(),
        Some((h, true)) => dist.cdf(h)?,
        Some((h, false)) => dist.cdf_below(h)?,
    };
    let lower = match &w.lo {
        None => Expr::zero(),
        Some((l, true)) => dist.cdf_below(l)?,
        Some((l, false)) => dist.cdf(l)?,
    };
    let mut p = (upper - lower).simplify();
    if sign(&p) == Some(std::cmp::Ordering::Less) {
        p = Expr::zero();
    }
    // X ≠ a removes the mass at a (only discrete distributions have any).
    for n in &w.ne {
        let inside = |b: &Option<(Expr, bool)>, upper: bool| match b {
            None => Some(true),
            Some((v, closed)) => compare(n, v).map(|c| match c {
                std::cmp::Ordering::Equal => *closed,
                std::cmp::Ordering::Less => upper,
                std::cmp::Ordering::Greater => !upper,
            }),
        };
        match (inside(&w.lo, false), inside(&w.hi, true)) {
            (Some(true), Some(true)) => p = (p - point_mass(dist, n, cond)?).simplify(),
            (Some(_), Some(_)) => {}
            _ => return Err(undecided(cond)),
        }
    }
    Ok(crate::domains::util::normalize(&p))
}

fn undecided(cond: &Expr) -> MathError {
    MathError::Inconclusive(format!("P({cond}): where the bounds lie is not known"))
}

/// `P(X = a)`: zero for a continuous distribution or a value off the support.
fn point_mass(dist: &Dist, at: &Expr, cond: &Expr) -> MathResult<Expr> {
    if !dist.is_discrete() {
        return Ok(Expr::zero());
    }
    if at.is_infinite() {
        return Ok(Expr::zero());
    }
    match rational(&at.simplify()) {
        Some(q) if q.is_integer() => dist.pmf(&q.to_integer()),
        Some(_) => Ok(Expr::zero()),
        None => {
            let v = numeric(at).ok_or_else(|| undecided(cond))?;
            if (v - v.round()).abs() > 1e-9 {
                Ok(Expr::zero())
            } else {
                Err(undecided(cond))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x() -> Expr {
        Expr::sym("X")
    }

    fn rv(name: &str, d: &str, p: Vec<Expr>) -> (String, Dist) {
        (name.to_string(), Dist::new(d, p).unwrap())
    }

    #[test]
    fn names_and_validation() {
        assert_eq!(canonical_name("Unif"), Some(("Uniform", 2)));
        assert_eq!(canonical_name("Pois"), Some(("Poisson", 1)));
        assert!(canonical_name("Weibull").is_none());
        assert!(Dist::new("Uniform", vec![Expr::int(1), Expr::int(0)]).is_err());
        assert!(Dist::new("Bernoulli", vec![Expr::rational(3, 2)]).is_err());
        assert!(Dist::new("Normal", vec![Expr::int(0), Expr::int(-1)]).is_err());
        assert!(Dist::new("Binomial", vec![Expr::rational(1, 2), Expr::rational(1, 2)]).is_err());
        assert!(Dist::new("Poisson", vec![Expr::int(0)]).is_err());
        assert!(Dist::new("Poisson", vec![Expr::sym("λ")]).is_ok());
        assert!(Dist::new("Uniform", vec![Expr::int(0)]).is_err());
    }

    #[test]
    fn exact_means_and_variances() {
        let u = Dist::new("Uniform", vec![Expr::int(0), Expr::int(1)]).unwrap();
        assert_eq!(u.mean().unwrap(), Expr::rational(1, 2));
        assert_eq!(u.variance().unwrap(), Expr::rational(1, 12));
        let b = Dist::new("Binomial", vec![Expr::int(10), Expr::rational(1, 4)]).unwrap();
        assert_eq!(b.mean().unwrap(), Expr::rational(5, 2));
        assert_eq!(b.variance().unwrap(), Expr::rational(15, 8));
        // E[X²] = Var + mean² for the binomial: 15/8 + 25/4 = 65/8.
        assert_eq!(b.raw_moment(2).unwrap(), Expr::rational(65, 8));
        let p = Dist::new("Poisson", vec![Expr::int(3)]).unwrap();
        assert_eq!(p.raw_moment(2).unwrap(), Expr::int(12));
        let e = Dist::new("Exponential", vec![Expr::int(2)]).unwrap();
        assert_eq!(e.mean().unwrap(), Expr::rational(1, 2));
        assert_eq!(e.raw_moment(3).unwrap(), Expr::rational(3, 4));
        let n = Dist::new("Normal", vec![Expr::int(1), Expr::int(4)]).unwrap();
        // E[X⁴] = μ⁴ + 6μ²σ² + 3σ⁴ = 1 + 24 + 48.
        assert_eq!(n.raw_moment(4).unwrap(), Expr::int(73));
    }

    #[test]
    fn symbolic_parameters_stay_exact() {
        let b = Dist::new("Bernoulli", vec![Expr::sym("p")]).unwrap();
        let rvs = vec![("X".to_string(), b)];
        let v = variance(&x(), &rvs).unwrap();
        let expected = (Expr::sym("p") * (Expr::one() - Expr::sym("p"))).simplify();
        assert!(crate::domains::util::equal(&v, &expected), "{v}");
    }

    #[test]
    fn linearity_and_independence() {
        let rvs = vec![
            rv("X", "Uniform", vec![Expr::int(0), Expr::int(1)]),
            rv("Y", "Poisson", vec![Expr::int(2)]),
        ];
        let y = Expr::sym("Y");
        // E[3X + 2Y − 1] = 3/2 + 4 − 1.
        let e = Expr::int(3) * x() + Expr::int(2) * y.clone() - Expr::one();
        assert_eq!(expectation(&e, &rvs).unwrap(), Expr::rational(9, 2));
        // E[XY] = E[X] E[Y] = 1.
        assert_eq!(expectation(&(x() * y.clone()), &rvs).unwrap(), Expr::one());
        // Var(X + Y) = 1/12 + 2.
        assert_eq!(variance(&(x() + y), &rvs).unwrap(), Expr::rational(25, 12));
        // A function of X that is not a polynomial is refused, not approximated.
        let s = crate::functions::func(Func::Sin, vec![x()]);
        assert!(expectation(&s, &rvs).is_err());
    }

    #[test]
    fn finite_support_takes_any_function() {
        let rvs = vec![rv("X", "Bernoulli", vec![Expr::rational(1, 3)])];
        // E[2^X] = 2/3 + 2/3.
        let e = Expr::int(2).pow(x());
        assert_eq!(expectation(&e, &rvs).unwrap(), Expr::rational(4, 3));
    }

    #[test]
    fn probabilities_from_the_distribution_function() {
        let rvs = vec![rv("X", "Uniform", vec![Expr::int(0), Expr::int(1)])];
        let le = |a: Expr, b: Expr| Expr::relation(RelOp::Le, a, b);
        assert_eq!(
            probability(&le(x(), Expr::rational(1, 2)), &rvs).unwrap(),
            Expr::rational(1, 2)
        );
        assert_eq!(
            probability(&le(x(), Expr::int(3)), &rvs).unwrap(),
            Expr::one()
        );
        // 2X + 1 > 2 ⇔ X > 1/2.
        let c = Expr::relation(RelOp::Gt, Expr::int(2) * x() + Expr::one(), Expr::int(2));
        assert_eq!(probability(&c, &rvs).unwrap(), Expr::rational(1, 2));
        // A continuous variable takes any one value with probability 0.
        let eq = Expr::relation(RelOp::Eq, x(), Expr::rational(1, 3));
        assert_eq!(probability(&eq, &rvs).unwrap(), Expr::zero());
        // E[1[X ≤ 1/4]] = 1/4.
        let ind = Expr::new(ExprNode::Piecewise {
            cases: vec![(Expr::one(), le(x(), Expr::rational(1, 4)))],
            otherwise: Some(Expr::zero()),
        });
        assert_eq!(expectation(&ind, &rvs).unwrap(), Expr::rational(1, 4));
    }

    #[test]
    fn discrete_probabilities_are_exact() {
        let rvs = vec![rv(
            "X",
            "Binomial",
            vec![Expr::int(4), Expr::rational(1, 2)],
        )];
        let rel = |op, b: Expr| Expr::relation(op, x(), b);
        assert_eq!(
            probability(&rel(RelOp::Eq, Expr::int(2)), &rvs).unwrap(),
            Expr::rational(3, 8)
        );
        assert_eq!(
            probability(&rel(RelOp::Le, Expr::int(1)), &rvs).unwrap(),
            Expr::rational(5, 16)
        );
        assert_eq!(
            probability(&rel(RelOp::Lt, Expr::int(1)), &rvs).unwrap(),
            Expr::rational(1, 16)
        );
        assert_eq!(
            probability(&rel(RelOp::Lt, Expr::rational(3, 2)), &rvs).unwrap(),
            Expr::rational(5, 16)
        );
        assert_eq!(
            probability(&rel(RelOp::Ne, Expr::int(2)), &rvs).unwrap(),
            Expr::rational(5, 8)
        );
        assert_eq!(
            probability(&rel(RelOp::Eq, Expr::rational(1, 2)), &rvs).unwrap(),
            Expr::zero()
        );
        // 1 ≤ X < 3.
        let both = Expr::new(ExprNode::Logic(
            LogicOp::And,
            vec![rel(RelOp::Ge, Expr::int(1)), rel(RelOp::Lt, Expr::int(3))],
        ));
        assert_eq!(probability(&both, &rvs).unwrap(), Expr::rational(5, 8));
        // Poisson(2): P(X = 0) = e^{-2}.
        let rvs = vec![rv("X", "Poisson", vec![Expr::int(2)])];
        let p0 = probability(&rel(RelOp::Eq, Expr::zero()), &rvs).unwrap();
        assert!(crate::domains::util::equal(
            &p0,
            &Expr::e().pow(Expr::int(-2))
        ));
        // Exponential(1): P(X > 1) = e^{-1}.
        let rvs = vec![rv("X", "Exponential", vec![Expr::one()])];
        let p = probability(&rel(RelOp::Gt, Expr::one()), &rvs).unwrap();
        assert!(
            crate::domains::util::equal(&p, &Expr::e().pow(Expr::int(-1))),
            "{p}"
        );
        // Normal: P(X ≤ μ) = 1/2 exactly.
        let rvs = vec![rv("X", "Normal", vec![Expr::int(3), Expr::int(4)])];
        assert_eq!(
            probability(&rel(RelOp::Le, Expr::int(3)), &rvs).unwrap(),
            Expr::rational(1, 2)
        );
    }

    #[test]
    fn several_variables_are_not_guessed() {
        let rvs = vec![
            rv("X", "Uniform", vec![Expr::int(0), Expr::int(1)]),
            rv("Y", "Uniform", vec![Expr::int(0), Expr::int(1)]),
        ];
        let c = Expr::relation(RelOp::Le, x().pow(2) + Expr::sym("Y").pow(2), Expr::one());
        assert!(probability(&c, &rvs).is_err());
    }

    fn member(e: &Expr) -> Option<(String, Expr)> {
        match e.node() {
            ExprNode::Apply(n, a) if a.len() == 1 => Some((n.to_string(), a[0].clone())),
            _ => None,
        }
    }

    #[test]
    fn monte_carlo_pi_is_seeded_and_close() {
        let fam = vec![
            rv("X", "Uniform", vec![Expr::int(0), Expr::int(1)]),
            rv("Y", "Uniform", vec![Expr::int(0), Expr::int(1)]),
        ];
        let j = Expr::sym("j");
        let xj = Expr::apply("X", vec![j.clone()]);
        let yj = Expr::apply("Y", vec![j]);
        let body = Expr::new(ExprNode::Piecewise {
            cases: vec![(
                Expr::one(),
                Expr::relation(RelOp::Le, xj.pow(2) + yj.pow(2), Expr::one()),
            )],
            otherwise: Some(Expr::zero()),
        });
        let n = 100_000;
        let a = monte_carlo_sum(&body, "j", 1, n, &fam, &member, 42, false).unwrap();
        let b = monte_carlo_sum(&body, "j", 1, n, &fam, &member, 42, false).unwrap();
        assert_eq!(a, b);
        let pi = 4.0 * a / n as f64;
        let sigma = 4.0
            * (0.25 * std::f64::consts::PI * (1.0 - 0.25 * std::f64::consts::PI) / n as f64).sqrt();
        assert!((pi - std::f64::consts::PI).abs() < 4.0 * sigma, "{pi}");
        let c = monte_carlo_sum(&body, "j", 1, n, &fam, &member, 7, false).unwrap();
        assert_ne!(a, c);
    }

    #[test]
    fn sample_means_match_the_distributions() {
        let j = Expr::sym("j");
        let xj = Expr::apply("X", vec![j]);
        for (d, p, mean) in [
            ("Normal", vec![Expr::int(2), Expr::int(9)], 2.0),
            ("Bernoulli", vec![Expr::rational(3, 10)], 0.3),
            ("Binomial", vec![Expr::int(20), Expr::rational(1, 4)], 5.0),
            ("Poisson", vec![Expr::int(45)], 45.0),
            ("Exponential", vec![Expr::int(4)], 0.25),
        ] {
            let dist = Dist::new(d, p).unwrap();
            let sd = numeric(&dist.variance().unwrap()).unwrap().sqrt();
            let fam = vec![("X".to_string(), dist)];
            let n = 20_000;
            let s = monte_carlo_sum(&xj, "j", 1, n, &fam, &member, 1, false).unwrap();
            let m = s / n as f64;
            assert!((m - mean).abs() < 5.0 * sd / (n as f64).sqrt(), "{d}: {m}");
        }
    }

    #[test]
    fn members_keep_their_values() {
        // X_1 + … + X_10 = (X_1 + … + X_5) + (X_6 + … + X_10).
        let fam = vec![rv("X", "Uniform", vec![Expr::int(0), Expr::int(1)])];
        let xj = Expr::apply("X", vec![Expr::sym("j")]);
        let all = monte_carlo_sum(&xj, "j", 1, 10, &fam, &member, 3, false).unwrap();
        let a = monte_carlo_sum(&xj, "j", 1, 5, &fam, &member, 3, false).unwrap();
        let b = monte_carlo_sum(&xj, "j", 6, 10, &fam, &member, 3, false).unwrap();
        assert!((all - a - b).abs() < 1e-12);
        // Other names must have values; a different index is refused.
        let free = xj.clone() * Expr::sym("c");
        assert!(monte_carlo_sum(&free, "j", 1, 5, &fam, &member, 3, false).is_err());
        let shifted = Expr::apply("X", vec![Expr::sym("j") + Expr::one()]);
        assert!(monte_carlo_sum(&shifted, "j", 1, 5, &fam, &member, 3, false).is_err());
        assert!(monte_carlo_sum(&xj, "j", 1, 1 << 40, &fam, &member, 3, false).is_err());
    }
}
