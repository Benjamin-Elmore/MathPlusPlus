//! Checking one algebraic claim against what the proof knows so far.
//!
//! The order of attempts, strongest first:
//! 1. the claim on its own (with the definitions it uses): a CAS proof gives ✓, agreement
//!    at random points gives ≈;
//! 2. the claim as a multiple of an earlier equation (`4r^2 = 2q^2, so q^2 = 2r^2`);
//! 3. the claim after eliminating names with earlier equations (`p = 2r`, `√2 = p/q`);
//! 4. integer sample points, for sums with symbolic bounds, congruences and divisibility,
//!    with earlier conditions (`gcd(a, p) = 1`, `0 < k < p`) as filters.
//!
//! A counterexample makes the claim ✗ only when nothing the checker could not use bears
//! on it: no earlier equation was eliminated into it or left unused, and every name in it
//! was introduced for all values ("for every real $x$", "for an integer $m$"). Otherwise
//! the claim is left unverified, never wrong.

use std::collections::BTreeSet;

use mpp_core::Expr;
use mpp_eval::Verdict;

use crate::cas::{self, Def, LineOut};
use crate::model::ClaimOutcome;
use crate::notation::ClaimOp;

/// An algebraic claim, written as LaTeX ready for the CAS.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Claim {
    pub lhs: String,
    pub op: ClaimOp,
    pub rhs: String,
    pub modulus: Option<String>,
    /// The names it uses, as written.
    pub names: BTreeSet<String>,
}

/// An earlier equation the proof has established or assumed.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Equation {
    pub lhs: String,
    pub rhs: String,
    pub names: BTreeSet<String>,
    /// True when one side is a lone name (`p = 2m + 1`): that name is defined by it.
    pub defines: Option<String>,
}

/// An earlier condition (`a ≥ 0`, `\gcd(a, p) = 1`), used to filter sample points.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Condition {
    pub lhs: String,
    pub op: ClaimOp,
    pub rhs: String,
    pub names: BTreeSet<String>,
}

/// What the proof knows when a claim is checked.
#[derive(Debug, Clone, Default)]
pub(crate) struct Knowledge {
    /// `:=` definitions in force, in order: name, body, names the body uses.
    pub defs: Vec<(Def, BTreeSet<String>)>,
    pub equations: Vec<Equation>,
    pub conditions: Vec<Condition>,
    /// Names declared prime ("Let $p$ be a prime").
    pub primes: BTreeSet<String>,
    /// Names introduced for all values ("for every real $x$").
    pub universal: BTreeSet<String>,
    /// Definitions whose body sums over an indexed family (`A := \sum a_i^2`).
    pub indexed: BTreeSet<String>,
    /// Names known to be real ("for every real $x$", "For real numbers $a_1, \dots,
    /// a_n$"); a family is recorded by its members as written (`a_1`, `a_n`).
    pub reals: BTreeSet<String>,
    /// Indexed families written out as `a_1, \dots, a_n`: base, first and last index.
    pub families: Vec<(String, String, String)>,
    /// Established inequalities `small ≤ big` (both LaTeX), with the names they use:
    /// from checked steps and hypotheses, chains combined (`0 \le S = Q` gives `0 ≤ Q`).
    pub le_facts: Vec<(String, String, BTreeSet<String>)>,
}

/// Points tried by the sampler, and how many must agree.
const SAMPLE_TRIES: usize = 40;
const SAMPLE_WANTED: usize = 8;

impl Knowledge {
    /// The definitions a set of names needs (transitively), in their written order.
    pub(crate) fn defs_for(&self, names: &BTreeSet<String>) -> Vec<Def> {
        let mut need: BTreeSet<String> = names.clone();
        loop {
            let before = need.len();
            for (d, uses) in &self.defs {
                if need.contains(&d.name) {
                    need.extend(uses.iter().cloned());
                }
            }
            if need.len() == before {
                break;
            }
        }
        self.defs
            .iter()
            .filter(|(d, _)| need.contains(&d.name))
            .map(|(d, _)| d.clone())
            .collect()
    }

    /// The names a set of names reaches through the definitions.
    pub(crate) fn closure(&self, names: &BTreeSet<String>) -> BTreeSet<String> {
        let mut all = names.clone();
        for d in self.defs_for(names) {
            if let Some((_, uses)) = self.defs.iter().find(|(x, _)| x.name == d.name) {
                all.extend(uses.iter().cloned());
            }
        }
        all
    }
}

/// Checks `claim` against `k`.
pub(crate) fn check_claim(claim: &Claim, k: &Knowledge) -> ClaimOutcome {
    let defs = k.defs_for(&claim.names);
    // 1. On its own.
    let first = direct(claim, &defs, k);
    match &first {
        Attempt::Proved => return ClaimOutcome::Proved,
        Attempt::Holds(how) => return ClaimOutcome::Numeric(how.clone()),
        _ => {}
    }
    let reach = k.closure(&claim.names);
    let relevant: Vec<&Equation> = k
        .equations
        .iter()
        .filter(|e| !e.names.is_disjoint(&reach))
        .collect();
    // 2. A multiple of an earlier equation.
    if claim.op == ClaimOp::Eq {
        for e in relevant.iter().rev() {
            if scaling(claim, e, k) {
                return ClaimOutcome::Proved;
            }
        }
    }
    // 3. Eliminate names with earlier equations.
    let mut eliminated = BTreeSet::new();
    let mut all_used = true;
    let mut second = None;
    if !relevant.is_empty() {
        let elim = eliminate(claim, &relevant, k);
        eliminated = elim.names;
        all_used = elim.all_used;
        if !elim.defs.is_empty() {
            let mut all = defs.clone();
            all.extend(elim.defs);
            let a = direct(claim, &all, k);
            match &a {
                Attempt::Proved => return ClaimOutcome::Proved,
                Attempt::Holds(how) => {
                    return ClaimOutcome::Numeric(format!("{how}, using the earlier equations"))
                }
                _ => {}
            }
            second = Some((a, all));
        }
    }
    let (last, last_defs) = match second {
        Some((a, d)) => (a, d),
        None => (first, defs),
    };
    // 4. Integer points, when the CAS could not decide.
    let last = match last {
        Attempt::Unknown(why) if claim.op == ClaimOp::Eq => {
            match sample(claim, &last_defs, k, Domain::Integers) {
                Attempt::Unknown(_) => Attempt::Unknown(why),
                other => other,
            }
        }
        // The CAS tries every real value, but a name declared prime takes only primes:
        // `0^p = 0` fails at p = 0, which is no prime. When the counterexample gives a
        // prime name a value that is not a prime, the claim is tried again at primes.
        Attempt::Fails(at) if claim.op == ClaimOp::Eq && outside_primes(&at, k) => {
            match sample(claim, &last_defs, k, Domain::Integers) {
                Attempt::Unknown(_) => Attempt::Fails(at),
                other => other,
            }
        }
        other => other,
    };
    match last {
        Attempt::Proved => ClaimOutcome::Proved,
        Attempt::Holds(how) => ClaimOutcome::Numeric(how),
        Attempt::Fails(at) => {
            let unused = !all_used;
            let touched = claim.names.iter().any(|n| eliminated.contains(n));
            let free: BTreeSet<&String> = reach
                .iter()
                .filter(|n| !k.defs.iter().any(|(d, _)| &d.name == *n))
                .collect();
            let universal = free.iter().all(|n| k.universal.contains(*n));
            if !unused && !touched && universal {
                ClaimOutcome::Failed(at)
            } else {
                ClaimOutcome::Unverified(format!(
                    "the sides differ ({at}), but the claim may rest on facts the checker cannot use"
                ))
            }
        }
        Attempt::Unknown(why) => ClaimOutcome::Unverified(why),
    }
}

/// True when a counterexample (`at p = 0, k = 1: left side …`) gives a name declared
/// prime a value that is not a prime, so it lies outside the values the name takes.
fn outside_primes(at: &str, k: &Knowledge) -> bool {
    let Some(point) = at
        .strip_prefix("(at ")
        .or_else(|| at.strip_prefix("at "))
        .and_then(|rest| rest.split_once(':'))
        .map(|(p, _)| p)
    else {
        return false;
    };
    let is_prime = |v: &str| {
        v.trim()
            .parse::<u64>()
            .is_ok_and(|n| n >= 2 && (2..).take_while(|d| d * d <= n).all(|d| n % d != 0))
    };
    point.split(", ").any(|pair| {
        pair.split_once(" = ")
            .is_some_and(|(n, v)| k.primes.contains(n.trim()) && !is_prime(v))
    })
}

/// One attempt's result.
#[derive(Debug, Clone, PartialEq)]
enum Attempt {
    Proved,
    Holds(String),
    Fails(String),
    Unknown(String),
}

/// The claim with the definitions `defs`, by the operator's own method.
fn direct(claim: &Claim, defs: &[Def], k: &Knowledge) -> Attempt {
    match claim.op {
        ClaimOp::Eq => {
            let line = format!(
                "\\left({}\\right) \\overset{{?}}{{=}} \\left({}\\right)",
                claim.lhs, claim.rhs
            );
            match cas::run_one(defs, line) {
                LineOut::Verdict(Verdict::Proved) => Attempt::Proved,
                LineOut::Verdict(Verdict::Holds { points }) => {
                    Attempt::Holds(format!("agrees at {points} random points"))
                }
                LineOut::Verdict(v @ (Verdict::Fails { .. } | Verdict::Mismatch(_))) => {
                    Attempt::Fails(v.describe().trim_start_matches("✗ ").to_string())
                }
                LineOut::Verdict(Verdict::Agrees { digits }) => {
                    Attempt::Holds(format!("agrees numerically to {digits} digits"))
                }
                LineOut::Verdict(v @ Verdict::Rounds { .. }) => {
                    Attempt::Holds(v.describe().trim_start_matches("✓ ").to_string())
                }
                LineOut::Verdict(Verdict::Unknown(why)) => Attempt::Unknown(why),
                LineOut::Error(e) => Attempt::Unknown(e),
                LineOut::Value(_) => Attempt::Unknown("the claim was not read as a check".into()),
            }
        }
        ClaimOp::Congruent | ClaimOp::Divides => sample(claim, defs, k, Domain::Integers),
        ClaimOp::Lt | ClaimOp::Le | ClaimOp::Gt | ClaimOp::Ge => {
            sample(claim, defs, k, Domain::Reals)
        }
    }
}

/// True when `claim` is `c ×` the equation `e` (difference of the sides) for a nonzero
/// constant `c` that the CAS works out exactly.
fn scaling(claim: &Claim, e: &Equation, k: &Knowledge) -> bool {
    let mut names = claim.names.clone();
    names.extend(e.names.iter().cloned());
    let defs = k.defs_for(&names);
    let ratio = format!(
        "\\frac{{\\left({}\\right) - \\left({}\\right)}}{{\\left({}\\right) - \\left({}\\right)}} = ?",
        claim.lhs, claim.rhs, e.lhs, e.rhs
    );
    let LineOut::Value(v) = cas::run_one(&defs, ratio) else {
        return false;
    };
    // Estimate the constant at a point, then make the CAS prove `claim − c·e = 0`.
    let mut env = std::collections::HashMap::new();
    for (i, s) in v.free_symbols().iter().enumerate() {
        env.insert(
            s.to_string(),
            num_complex::Complex64::new(0.37 + 0.61 * i as f64, 0.0),
        );
    }
    let Ok(z) = v.eval_complex(&env) else {
        return false;
    };
    if z.im.abs() > 1e-9 {
        return false;
    }
    let Some((num, den)) = simple_fraction(z.re) else {
        return false;
    };
    if num == 0 {
        return false;
    }
    let line = format!(
        "\\left({}\\right) - \\left({}\\right) \\overset{{?}}{{=}} \\frac{{{num}}}{{{den}}} \\left(\\left({}\\right) - \\left({}\\right)\\right)",
        claim.lhs, claim.rhs, e.lhs, e.rhs
    );
    matches!(cas::run_one(&defs, line), LineOut::Verdict(Verdict::Proved))
}

/// `x` as `p/q` with `q ≤ 24`, when it is that close to one.
fn simple_fraction(x: f64) -> Option<(i64, i64)> {
    if !x.is_finite() || x.abs() > 1e6 {
        return None;
    }
    (1..=24i64).find_map(|q| {
        let p = (x * q as f64).round();
        ((x * q as f64 - p).abs() < 1e-9 * q as f64).then_some((p as i64, q))
    })
}

/// The result of eliminating names.
struct Elimination {
    defs: Vec<Def>,
    /// The names the new definitions define.
    names: BTreeSet<String>,
    /// True when every equation was used.
    all_used: bool,
}

/// Eliminates names using the equations, newest first: a lone-name side defines that
/// name; otherwise the CAS solves for a name, and only a single solution is used.
fn eliminate(claim: &Claim, equations: &[&Equation], k: &Knowledge) -> Elimination {
    let mut defined: BTreeSet<String> = k.defs.iter().map(|(d, _)| d.name.clone()).collect();
    let mut new_defs: Vec<Def> = Vec::new();
    let mut names = BTreeSet::new();
    let mut used = vec![false; equations.len()];
    for _round in 0..3 {
        let mut progress = false;
        for (idx, e) in equations.iter().enumerate().rev() {
            if used[idx] {
                continue;
            }
            if let Some(d) = &e.defines {
                if !defined.contains(d) {
                    let body = if &e.lhs == d { &e.rhs } else { &e.lhs };
                    new_defs.push(Def::new(d.clone(), body.clone()));
                    defined.insert(d.clone());
                    names.insert(d.clone());
                    used[idx] = true;
                    progress = true;
                    continue;
                }
            }
            let mut candidates: Vec<&String> =
                e.names.iter().filter(|n| !defined.contains(*n)).collect();
            candidates.sort_by_key(|n| !claim.names.contains(*n));
            for v in candidates {
                let mut all: Vec<Def> = k.defs_for(&e.names);
                all.extend(new_defs.iter().cloned());
                let line = format!(
                    "\\operatorname{{solve}}_{{{v}}} \\left({} = {}\\right) = ?",
                    e.lhs, e.rhs
                );
                let LineOut::Value(sol) = cas::run_one(&all, line) else {
                    continue;
                };
                let Some(one) = cas::single_solution(&sol) else {
                    continue;
                };
                let latex = one.to_latex();
                let plain = v.trim_start_matches('\\');
                if latex.is_empty() || one.free_symbols().iter().any(|s| &**s == plain) {
                    continue;
                }
                new_defs.push(Def::new(v.clone(), latex));
                defined.insert(v.clone());
                names.insert(v.clone());
                used[idx] = true;
                progress = true;
                break;
            }
        }
        if !progress {
            break;
        }
    }
    Elimination {
        defs: new_defs,
        names,
        all_used: used.iter().all(|u| *u),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Domain {
    Integers,
    Reals,
}

/// A small deterministic generator.
pub(crate) struct Rng(pub(crate) u64);

impl Rng {
    pub(crate) fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub(crate) fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }
}

/// The primes tried for a name declared prime. Congruences and divisibility are decided
/// in exact integer arithmetic, so the large powers these give are safe.
const PRIMES: [i64; 11] = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31];

/// Tests the claim at sample points (integers or decimals), skipping points that break an
/// earlier condition on the same names.
fn sample(claim: &Claim, defs: &[Def], k: &Knowledge, domain: Domain) -> Attempt {
    let defined: BTreeSet<&String> = defs.iter().map(|d| &d.name).collect();
    let mut free: BTreeSet<String> = claim.names.clone();
    for d in defs {
        if let Some((_, uses)) = k.defs.iter().find(|(x, _)| x.name == d.name) {
            free.extend(uses.iter().cloned());
        }
    }
    if let Some(m) = &claim.modulus {
        if is_name(m) {
            free.insert(m.clone());
        }
    }
    free.retain(|n| !defined.contains(n));
    let conditions: Vec<&Condition> = k
        .conditions
        .iter()
        .filter(|c| {
            // A congruence or divisibility condition has no modulus to test it with here.
            !matches!(c.op, ClaimOp::Congruent | ClaimOp::Divides)
                && !c.names.is_empty()
                && c.names
                    .iter()
                    .all(|n| free.contains(n) || defined.contains(n))
        })
        .collect();
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    let mut agreed = 0usize;
    let mut tried = 0usize;
    let mut last_error = String::from("no sample point could be evaluated");
    let mut exact_points = 0usize;
    while tried < SAMPLE_TRIES && agreed < SAMPLE_WANTED {
        tried += 1;
        let point: Vec<(String, String)> = free
            .iter()
            .map(|n| {
                let v = if k.primes.contains(n) {
                    let i = rng.below(PRIMES.len() as u64) as usize;
                    PRIMES.get(i).copied().unwrap_or(5).to_string()
                } else {
                    match domain {
                        Domain::Integers => (rng.below(8) as i64).to_string(),
                        Domain::Reals => {
                            let u = rng.below(10_000) as f64 / 10_000.0;
                            format!("{:.4}", -3.0 + 6.0 * u)
                        }
                    }
                };
                (n.clone(), v)
            })
            .collect();
        let mut all: Vec<Def> = point
            .iter()
            .map(|(n, v)| Def::new(n.clone(), v.clone()))
            .collect();
        all.extend(defs.iter().cloned());
        let mut lines = vec![
            format!("{} = ?", wrap(&claim.lhs)),
            format!("{} = ?", wrap(&claim.rhs)),
        ];
        if let Some(m) = &claim.modulus {
            lines.push(format!("{} = ?", wrap(m)));
        }
        for c in &conditions {
            lines.push(format!("{} = ?", wrap(&c.lhs)));
            lines.push(format!("{} = ?", wrap(&c.rhs)));
        }
        // A congruence or divisibility in exact arithmetic: the remainder must be 0.
        let exact_line = match (claim.op, &claim.modulus) {
            (ClaimOp::Congruent, Some(m)) => Some(format!(
                "\\left({} - {}\\right) \\bmod {} = ?",
                wrap(&claim.lhs),
                wrap(&claim.rhs),
                wrap(m)
            )),
            (ClaimOp::Divides, _) => Some(format!(
                "{} \\bmod {} = ?",
                wrap(&claim.rhs),
                wrap(&claim.lhs)
            )),
            _ => None,
        };
        if let Some(l) = &exact_line {
            lines.push(l.clone());
        }
        let out = cas::run(&all, &lines);
        let exact: Option<bool> = exact_line.as_ref().and_then(|_| match out.last() {
            Some(LineOut::Value(v)) if v.is_number() => Some(v.is_zero()),
            _ => None,
        });
        let base = if claim.modulus.is_some() { 3 } else { 2 };
        // The point must satisfy every condition that could be evaluated.
        let mut ok = true;
        for (ci, c) in conditions.iter().enumerate() {
            let (Some(a), Some(b)) = (
                out.get(base + 2 * ci).and_then(LineOut::real),
                out.get(base + 2 * ci + 1).and_then(LineOut::real),
            ) else {
                continue;
            };
            if !holds(c.op, a, b, None) {
                ok = false;
            }
        }
        if !ok {
            continue;
        }
        let (Some(l), Some(r)) = (
            out.first().and_then(LineOut::real),
            out.get(1).and_then(LineOut::real),
        ) else {
            if let Some(LineOut::Error(e)) = out.first().or(out.get(1)) {
                last_error = e.clone();
            }
            continue;
        };
        let m = match &claim.modulus {
            Some(_) => match out.get(2).and_then(LineOut::real) {
                Some(m) => Some(m),
                None => continue,
            },
            None => None,
        };
        if matches!(claim.op, ClaimOp::Congruent) && m.is_none() {
            return Attempt::Unknown("a congruence needs `\\pmod{n}` to be checked".into());
        }
        if exact.unwrap_or_else(|| holds(claim.op, l, r, m)) {
            agreed += 1;
            exact_points += usize::from(exact.is_some());
        } else {
            let at: Vec<String> = point.iter().map(|(n, v)| format!("{n} = {v}")).collect();
            let mut msg = format!(
                "at {}: left side {}, right side {}",
                at.join(", "),
                fmt(l),
                fmt(r)
            );
            if let Some(m) = m {
                msg.push_str(&format!(", modulus {}", fmt(m)));
            }
            return Attempt::Fails(msg);
        }
    }
    if agreed >= 3 {
        let what = match domain {
            Domain::Integers => "integer",
            Domain::Reals => "random",
        };
        let how = if exact_points == agreed {
            " (in exact integer arithmetic)"
        } else {
            ""
        };
        Attempt::Holds(format!("agrees at {agreed} {what} points{how}"))
    } else {
        Attempt::Unknown(last_error)
    }
}

pub(crate) fn wrap(t: &str) -> String {
    format!("\\left({t}\\right)")
}

fn is_name(t: &str) -> bool {
    let t = t.trim();
    !t.is_empty()
        && t.chars()
            .all(|c| c.is_alphabetic() || c == '_' || c == '\\')
}

pub(crate) fn fmt(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        format!("{x:.6}")
    }
}

/// Whether `l op r` holds (with the modulus for a congruence).
pub(crate) fn holds(op: ClaimOp, l: f64, r: f64, modulus: Option<f64>) -> bool {
    let tol = 1e-9 * l.abs().max(r.abs()).max(1.0);
    match op {
        ClaimOp::Eq => (l - r).abs() <= tol,
        ClaimOp::Lt => l < r - tol,
        ClaimOp::Le => l <= r + tol,
        ClaimOp::Gt => l > r + tol,
        ClaimOp::Ge => l >= r - tol,
        ClaimOp::Congruent => match modulus {
            Some(m) if m.round() != 0.0 => {
                let d = (l - r).round();
                (d % m.round()).abs() < 0.5
            }
            _ => false,
        },
        ClaimOp::Divides => {
            let (a, b) = (l.round(), r.round());
            if a == 0.0 {
                b == 0.0
            } else {
                (b % a).abs() < 0.5
            }
        }
    }
}

/// A claim about a discriminant: `lhs op rhs` with `op` one of ≤ and ≥, proved from an
/// earlier `P ≥ 0` for every real `x` where `P` is a quadratic in `x`. A polynomial of
/// degree at most 2 in a real variable that is never negative has `b² − 4ac ≤ 0` (if
/// `a > 0` its minimum `c − b²/4a` is not negative; if `a = 0` then `b = 0`; `a < 0` is
/// impossible), so the claim holds when its two sides differ by a positive constant times
/// that discriminant. The names in the claim and in `P` are taken as they stand: their
/// definitions are not expanded, except definitions that sum nothing over a family.
pub(crate) fn discriminant_rule(
    lhs: &str,
    op: ClaimOp,
    rhs: &str,
    names: &BTreeSet<String>,
    k: &Knowledge,
) -> Option<ClaimOutcome> {
    // The claim as `d ≤ 0`.
    let d = match op {
        ClaimOp::Le => format!("{} - {}", wrap(lhs), wrap(rhs)),
        ClaimOp::Ge => format!("{} - {}", wrap(rhs), wrap(lhs)),
        _ => return None,
    };
    for (small, big, fact_names) in k.le_facts.iter().rev() {
        let mut all = names.clone();
        all.extend(fact_names.iter().cloned());
        // A sum over a family stays one opaque symbol (a reserved one, so that a name
        // such as `C` is not read as a unit).
        let defs: Vec<Def> = k
            .defs_for(&all)
            .into_iter()
            .enumerate()
            .map(|(i, d)| {
                if k.indexed.contains(&d.name) {
                    Def::new(d.name, format!("\\Upsilon_{{{}}}", 900 + i))
                } else {
                    d
                }
            })
            .collect();
        let out = cas::run(
            &defs,
            &[
                format!("{} - {} = ?", wrap(big), wrap(small)),
                format!("{d} = ?"),
            ],
        );
        let (Some(LineOut::Value(p)), Some(LineOut::Value(de))) = (out.first(), out.get(1)) else {
            continue;
        };
        let Ok(p) = mpp_core::expand(p) else {
            continue;
        };
        for v in fact_names {
            if !(k.universal.contains(v) && k.reals.contains(v)) {
                continue;
            }
            let Some(sym) = p
                .free_symbols()
                .iter()
                .find(|s| &***s == v.trim_start_matches('\\'))
                .map(|s| s.to_string())
            else {
                continue;
            };
            let Some(cs) = mpp_core::as_poly(&p, &sym) else {
                continue;
            };
            let [c0, c1, c2] = cs.as_slice() else {
                continue;
            };
            if de.contains_sym(&sym) || cs.iter().any(|c| c.contains_sym(&sym)) {
                continue;
            }
            let disc = c1 * c1 - &Expr::int(4) * &(c2 * c0);
            if positive_multiple(de, &disc) {
                return Some(ClaimOutcome::ProvedBy(format!(
                    "${big} \\ge {small}$ for every real ${v}$ (an earlier step), and a quadratic in ${v}$ that is never negative has discriminant at most 0"
                )));
            }
        }
    }
    None
}

/// True when `e = c · base` for a positive rational constant `c` (proved by expanding
/// `e − c · base` to 0).
fn positive_multiple(e: &Expr, base: &Expr) -> bool {
    let mut env = std::collections::HashMap::new();
    let syms = e
        .free_symbols()
        .union(&base.free_symbols())
        .cloned()
        .collect::<Vec<_>>();
    for (i, s) in syms.iter().enumerate() {
        env.insert(
            s.to_string(),
            num_complex::Complex64::new(0.37 + 0.61 * i as f64, 0.0),
        );
    }
    let (Ok(a), Ok(b)) = (e.eval_complex(&env), base.eval_complex(&env)) else {
        return false;
    };
    if b.norm() < 1e-12 || (a / b).im.abs() > 1e-9 {
        return false;
    }
    let Some((num, den)) = simple_fraction((a / b).re) else {
        return false;
    };
    if num <= 0 {
        return false;
    }
    let c = Expr::rational(num, den);
    mpp_core::expand(&(e - &(&c * base))).is_ok_and(|r| r.simplify().is_zero())
}

/// The closed form of `t1 + t2 + ⋯ + tl` read as an arithmetic progression, as LaTeX.
/// `None` when the step `t2 − t1` is not a nonzero number.
pub(crate) fn arithmetic_closed_form(t1: &str, t2: &str, tl: &str) -> Option<String> {
    let out = cas::run_one(&[], format!("{} - {} = ?", wrap(t2), wrap(t1)));
    let LineOut::Value(v) = &out else {
        return None;
    };
    if !v.free_symbols().is_empty() {
        return None;
    }
    let d = out.real()?;
    if d == 0.0 || d.fract() != 0.0 {
        return None;
    }
    Some(format!(
        "\\frac{{{a} + {b}}}{{2}} \\left(\\frac{{{b} - {a}}}{{{d}}} + 1\\right)",
        a = wrap(t1),
        b = wrap(tl),
        d = d as i64
    ))
}

/// `lhs = p + ⋯` where `p` is the polynomial written before the ellipsis: the claim that
/// `p` is the start of the Maclaurin series of `lhs` (every coefficient up to `p`'s
/// degree agrees).
pub(crate) fn check_series(
    lhs: &str,
    poly: &str,
    k: &Knowledge,
    names: &BTreeSet<String>,
) -> ClaimOutcome {
    let defs = k.defs_for(names);
    let out = cas::run(
        &defs,
        &[format!("{} = ?", wrap(lhs)), format!("{} = ?", wrap(poly))],
    );
    let (Some(LineOut::Value(f)), Some(LineOut::Value(p))) = (out.first(), out.get(1)) else {
        return ClaimOutcome::Unverified("the series could not be read".into());
    };
    let vars: Vec<String> = f
        .free_symbols()
        .union(&p.free_symbols())
        .map(|s| s.to_string())
        .collect();
    let [var] = vars.as_slice() else {
        return ClaimOutcome::Unverified("a series in more than one variable".into());
    };
    let Some(coeffs) = mpp_core::as_poly(p, var) else {
        return ClaimOutcome::Unverified(
            "the terms before the ellipsis are not a polynomial".into(),
        );
    };
    let degree = coeffs.len().saturating_sub(1);
    let order = u32::try_from(degree + 1).unwrap_or(1).max(1);
    let diff = f.clone() - p.clone();
    match mpp_core::calc::series(&diff, var, &mpp_core::Expr::zero(), order, None) {
        Ok(s) => {
            if s.coeffs.iter().all(|c| c.simplify().is_zero()) {
                ClaimOutcome::Proved
            } else {
                ClaimOutcome::Failed(format!(
                    "the series of the left side up to {var}^{degree} is {}",
                    (f.clone() - diff.clone() + s.terms.clone()).simplify()
                ))
            }
        }
        Err(e) => ClaimOutcome::Unverified(format!("the series could not be computed: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> BTreeSet<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn claim(lhs: &str, op: ClaimOp, rhs: &str, ns: &[&str]) -> Claim {
        Claim {
            lhs: lhs.into(),
            op,
            rhs: rhs.into(),
            modulus: None,
            names: names(ns),
        }
    }

    #[test]
    fn identity_is_proved() {
        let c = claim("(2m+1)^2", ClaimOp::Eq, "4m^2 + 4m + 1", &["m"]);
        assert_eq!(check_claim(&c, &Knowledge::default()), ClaimOutcome::Proved);
    }

    #[test]
    fn wrong_identity_needs_universal_names() {
        let c = claim("(m+1)^2", ClaimOp::Eq, "m^2 + 1", &["m"]);
        let mut k = Knowledge::default();
        assert!(matches!(check_claim(&c, &k), ClaimOutcome::Unverified(_)));
        k.universal.insert("m".into());
        assert!(matches!(check_claim(&c, &k), ClaimOutcome::Failed(_)));
    }

    #[test]
    fn equations_are_eliminated() {
        let mut k = Knowledge::default();
        k.equations.push(Equation {
            lhs: "\\sqrt{2}".into(),
            rhs: "p / q".into(),
            names: names(&["p", "q"]),
            defines: None,
        });
        k.equations.push(Equation {
            lhs: "p".into(),
            rhs: "2r".into(),
            names: names(&["p", "r"]),
            defines: Some("p".into()),
        });
        let c = claim("4 r^2", ClaimOp::Eq, "2 q^2", &["r", "q"]);
        let out = check_claim(&c, &k);
        assert!(
            matches!(out, ClaimOutcome::Proved | ClaimOutcome::Numeric(_)),
            "{out:?}"
        );
    }

    #[test]
    fn multiples_of_an_equation() {
        let mut k = Knowledge::default();
        k.equations.push(Equation {
            lhs: "(a + b)^2".into(),
            rhs: "c^2 + 4 \\cdot \\tfrac12 a b".into(),
            names: names(&["a", "b", "c"]),
            defines: None,
        });
        let c = claim("a^2 + b^2", ClaimOp::Eq, "c^2", &["a", "b", "c"]);
        assert_eq!(check_claim(&c, &k), ClaimOutcome::Proved);
    }

    #[test]
    fn congruences_by_sampling_primes() {
        let mut k = Knowledge::default();
        k.primes.insert("p".into());
        let mut c = claim("(a+1)^p", ClaimOp::Congruent, "a^p + 1", &["a", "p"]);
        c.modulus = Some("p".into());
        assert!(matches!(check_claim(&c, &k), ClaimOutcome::Numeric(_)));
        // Without knowing p is prime the sample finds p = 4 or 6 and gives up honestly.
        let k = Knowledge::default();
        assert!(!matches!(
            check_claim(&c, &k),
            ClaimOutcome::Numeric(_) | ClaimOutcome::Proved
        ));
    }

    #[test]
    fn conditions_filter_points() {
        let mut k = Knowledge::default();
        k.primes.insert("p".into());
        k.conditions.push(Condition {
            lhs: "\\gcd(a, p)".into(),
            op: ClaimOp::Eq,
            rhs: "1".into(),
            names: names(&["a", "p"]),
        });
        let mut c = claim("a^{p-1}", ClaimOp::Congruent, "1", &["a", "p"]);
        c.modulus = Some("p".into());
        assert!(matches!(check_claim(&c, &k), ClaimOutcome::Numeric(_)));
    }

    #[test]
    fn congruences_are_exact_for_large_powers() {
        // 7^31 is far past f64 precision; the remainder is computed exactly.
        let k = Knowledge::default();
        let mut c = claim("7^{31}", ClaimOp::Congruent, "7", &[]);
        c.modulus = Some("31".into());
        assert_eq!(
            sample(&c, &[], &k, Domain::Integers),
            Attempt::Holds("agrees at 8 integer points (in exact integer arithmetic)".into())
        );
        c.rhs = "8".into();
        assert!(matches!(
            sample(&c, &[], &k, Domain::Integers),
            Attempt::Fails(_)
        ));
        let d = claim("31", ClaimOp::Divides, "7^{31} - 7", &[]);
        assert!(matches!(
            sample(&d, &[], &k, Domain::Integers),
            Attempt::Holds(_)
        ));
    }
}
