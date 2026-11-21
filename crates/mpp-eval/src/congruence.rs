//! Congruences: `a \overset{?}{\equiv} b \pmod n` is checked by exact integer
//! arithmetic, and `a \equiv b \pmod n` is read as a relation (true or false when the
//! sides are numbers, a statement about integers otherwise) rather than as
//! `b · mod(n)`.
//!
//! The names in a congruence stand for integers. A check passes outright when
//! `a − b` is a multiple of n for every integer value of the names (every coefficient
//! of the expanded difference is), fails at the first integer point where it is not a
//! multiple, and is otherwise reported as checked at integer points only, never as
//! proved.

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{Signed, Zero};

use mpp_core::{Expr, ExprNode};

use crate::check::Verdict;

/// Name of the core's remainder operator.
const MOD: &str = "mod";
/// Integer points tried for a congruence that is not settled coefficient by coefficient.
const POINTS: usize = 40;

/// `(a, b, n)` when both sides are remainders modulo the same n (`a mod n`, `b mod n`),
/// which is how the parser reads `a \overset{?}{\equiv} b \pmod n`.
pub fn as_congruence(l: &Expr, r: &Expr) -> Option<(Expr, Expr, Expr)> {
    let side = |e: &Expr| -> Option<(Expr, Expr)> {
        match e.node() {
            ExprNode::Apply(name, args) if &**name == MOD && args.len() == 2 => {
                Some((args[0].clone(), args[1].clone()))
            }
            _ => None,
        }
    };
    let (a, n1) = side(l)?;
    let (b, n2) = side(r)?;
    (n1 == n2).then_some((a, b, n1))
}

/// The congruence `a ≡ b (mod n)` as a relation the evaluator can decide:
/// `a mod n = b mod n`.
pub fn relation(a: Expr, b: Expr, n: Expr) -> Expr {
    Expr::relation(
        mpp_core::RelOp::Eq,
        Expr::apply(MOD, vec![a, n.clone()]),
        Expr::apply(MOD, vec![b, n]),
    )
}

fn integer(e: &Expr) -> Option<BigInt> {
    e.simplify().as_number()?.as_integer()
}

/// `x mod n` in `0 … n−1`.
fn residue(x: &BigInt, n: &BigInt) -> BigInt {
    x.mod_floor(n)
}

/// True when every term of the expanded `d` is an integer multiple of `n` times a
/// product of names raised to whole-number powers (so `d` is a multiple of n at every
/// integer point).
fn multiple_everywhere(d: &Expr, n: &BigInt) -> bool {
    let Ok(expanded) = d.expand() else {
        return false;
    };
    let terms = match expanded.node() {
        ExprNode::Add(v) => v.clone(),
        _ => vec![expanded.clone()],
    };
    terms.iter().all(|t| {
        let factors = match t.node() {
            ExprNode::Mul(v) => v.clone(),
            _ => vec![t.clone()],
        };
        let mut coeff = BigInt::from(1);
        for f in &factors {
            if let Some(q) = f.as_number().and_then(|x| x.as_rational()) {
                if !q.is_integer() {
                    return false;
                }
                coeff *= q.to_integer();
                continue;
            }
            let ok = match f.node() {
                ExprNode::Sym(_) => true,
                ExprNode::Pow(b, x) => {
                    matches!(b.node(), ExprNode::Sym(_))
                        && integer(x).is_some_and(|k| k.is_positive())
                }
                _ => false,
            };
            if !ok {
                return false;
            }
        }
        (coeff % n).is_zero()
    })
}

/// A small deterministic sequence of integers in −12 … 12 (an LCG), so a run of the
/// same file always tries the same points.
fn points(count: usize, names: usize) -> Vec<Vec<i64>> {
    let mut state: u64 = 0x2545_F491_4F6C_DD1D;
    (0..count)
        .map(|_| {
            (0..names)
                .map(|_| {
                    state = state
                        .wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(1_442_695_040_888_963_407);
                    ((state >> 33) % 25) as i64 - 12
                })
                .collect()
        })
        .collect()
}

/// Checks `a ≡ b (mod n)` by exact integer arithmetic.
pub fn check(a: &Expr, b: &Expr, n: &Expr) -> Verdict {
    let Some(m) = integer(n).filter(|m| m.is_positive()) else {
        return Verdict::Unknown(format!(
            "the modulus {} must be a whole number of at least 1",
            n.to_text()
        ));
    };
    let shown = |x: &Expr| match integer(x) {
        Some(v) => format!("{} ≡ {}", x.to_text(), residue(&v, &m)),
        None => x.to_text(),
    };
    let d = (a.clone() - b.clone()).simplify();
    if let Some(v) = integer(&d) {
        return if (v % &m).is_zero() {
            Verdict::Proved
        } else {
            Verdict::Fails {
                at: Vec::new(),
                lhs: shown(a),
                rhs: shown(b),
            }
        };
    }
    if d.as_number().is_some() {
        return Verdict::Unknown(format!(
            "{} is not a whole number, so it has no remainder",
            d.to_text()
        ));
    }
    if multiple_everywhere(&d, &m) {
        return Verdict::Proved;
    }
    // Integer points: the first where the difference is not a multiple of n fails.
    let names: Vec<String> = d.free_symbols().iter().map(|s| s.to_string()).collect();
    let mut tried = 0;
    for p in points(POINTS, names.len()) {
        let pairs: Vec<(&str, Expr)> = names
            .iter()
            .map(String::as_str)
            .zip(p.iter().map(|&v| Expr::int(v)))
            .collect();
        let Some(v) = integer(&d.subs_all(&pairs)) else {
            // A negative power or a fraction at this point: not an integer, skipped.
            continue;
        };
        tried += 1;
        if !(v % &m).is_zero() {
            let at = names
                .iter()
                .zip(&p)
                .map(|(n, v)| (Expr::sym(n).to_text(), v.to_string()))
                .collect();
            let la = integer(&a.subs_all(&pairs)).map(|x| residue(&x, &m));
            let lb = integer(&b.subs_all(&pairs)).map(|x| residue(&x, &m));
            return Verdict::Fails {
                at,
                lhs: la.map_or_else(|| a.to_text(), |x| format!("≡ {x}")),
                rhs: lb.map_or_else(|| b.to_text(), |x| format!("≡ {x}")),
            };
        }
    }
    if tried == 0 {
        return Verdict::Unknown(format!(
            "{} is not a whole number at the integer points tried",
            d.to_text()
        ));
    }
    Verdict::Holds { points: tried }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x() -> Expr {
        Expr::sym("x")
    }

    #[test]
    fn numbers_are_exact() {
        let n = Expr::int(13);
        assert_eq!(
            check(&Expr::int(3).pow(12), &Expr::one(), &n),
            Verdict::Proved
        );
        assert!(matches!(
            check(&Expr::int(3).pow(12), &Expr::int(2), &n),
            Verdict::Fails { .. }
        ));
        assert_eq!(check(&Expr::int(-1), &Expr::int(12), &n), Verdict::Proved);
        // 2^1000 is far beyond f64, and exact here.
        assert_eq!(
            check(
                &Expr::int(2).pow(1000),
                &Expr::int(2).pow(1000 % 16),
                &Expr::int(17)
            ),
            Verdict::Proved
        );
        assert!(matches!(
            check(&Expr::one(), &Expr::one(), &Expr::int(0)),
            Verdict::Unknown(_)
        ));
    }

    #[test]
    fn names_are_integers() {
        let n = Expr::int(7);
        // x + 7 − x = 7: a multiple of 7 for every x.
        assert_eq!(check(&(x() + Expr::int(7)), &x(), &n), Verdict::Proved);
        assert_eq!(
            check(&(Expr::int(14) * x().pow(2)), &Expr::zero(), &n),
            Verdict::Proved
        );
        // x⁷ ≡ x (mod 7) holds for every integer (Fermat), checked at integer points.
        assert!(matches!(
            check(&x().pow(7), &x(), &n),
            Verdict::Holds { .. }
        ));
        // x² ≡ x (mod 7) does not: x = 2 gives 4.
        assert!(matches!(
            check(&x().pow(2), &x(), &n),
            Verdict::Fails { .. }
        ));
    }

    #[test]
    fn congruence_sides_are_recognised() {
        let n = Expr::int(5);
        let l = Expr::apply(MOD, vec![Expr::int(7), n.clone()]);
        let r = Expr::apply(MOD, vec![Expr::int(2), n.clone()]);
        assert!(as_congruence(&l, &r).is_some());
        let other = Expr::apply(MOD, vec![Expr::int(2), Expr::int(6)]);
        assert!(as_congruence(&l, &other).is_none());
    }
}
