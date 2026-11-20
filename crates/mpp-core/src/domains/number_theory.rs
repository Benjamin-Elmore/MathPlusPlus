//! Modular arithmetic and primes: `mod`, inverses, fast modular powers, the Chinese
//! remainder theorem, the extended Euclidean algorithm, primality (Miller–Rabin),
//! factorisation (trial division, then Pollard's rho) and Euler's totient.

use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{One, Signed, Zero};

use crate::domains::util::{expect_args, int_arg};
use crate::domains::{Ctx, Domain, Value};
use crate::error::{MathError, MathResult};
use crate::expr::{Expr, ExprNode};

/// Largest number, in bits, that is factored or tested.
pub const MAX_BITS: u64 = 512;
/// Trial division bound before Pollard's rho.
const TRIAL_LIMIT: u32 = 10_000;
/// Iterations Pollard's rho may spend on one factor.
const RHO_LIMIT: u64 = 2_000_000;
/// Miller–Rabin bases; deterministic below 3.3·10²⁴.
const BASES: [u32; 13] = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41];

fn too_big(n: &BigInt) -> MathResult<()> {
    if n.bits() > MAX_BITS {
        Err(MathError::TooLarge(format!("a {}-bit number", n.bits())))
    } else {
        Ok(())
    }
}

fn positive_modulus(op: &str, m: &BigInt) -> MathResult<()> {
    if m.is_positive() {
        Ok(())
    } else {
        Err(MathError::Domain {
            function: op.into(),
            detail: format!("the modulus must be positive, not {m}"),
        })
    }
}

/// `a mod m` in `0 … m−1`.
pub fn modulo(a: &BigInt, m: &BigInt) -> MathResult<BigInt> {
    positive_modulus("mod", m)?;
    Ok(a.mod_floor(m))
}

/// `(g, s, t)` with `g = gcd(a, b) = s·a + t·b` and `g ≥ 0`.
pub fn egcd(a: &BigInt, b: &BigInt) -> (BigInt, BigInt, BigInt) {
    let (mut r0, mut r1) = (a.clone(), b.clone());
    let (mut s0, mut s1) = (BigInt::one(), BigInt::zero());
    let (mut t0, mut t1) = (BigInt::zero(), BigInt::one());
    while !r1.is_zero() {
        let q = r0.div_floor(&r1);
        let r2 = &r0 - &q * &r1;
        r0 = std::mem::replace(&mut r1, r2);
        let s2 = &s0 - &q * &s1;
        s0 = std::mem::replace(&mut s1, s2);
        let t2 = &t0 - &q * &t1;
        t0 = std::mem::replace(&mut t1, t2);
    }
    if r0.is_negative() {
        (-r0, -s0, -t0)
    } else {
        (r0, s0, t0)
    }
}

/// The inverse of `a` modulo `m`.
pub fn modinv(a: &BigInt, m: &BigInt) -> MathResult<BigInt> {
    positive_modulus("modinv", m)?;
    let (g, s, _) = egcd(&a.mod_floor(m), m);
    if !g.is_one() {
        return Err(MathError::Undefined(format!(
            "the inverse of {a} mod {m} (they share the factor {g})"
        )));
    }
    Ok(s.mod_floor(m))
}

/// `b^e mod m`; a negative `e` uses the inverse of `b`.
pub fn powmod(b: &BigInt, e: &BigInt, m: &BigInt) -> MathResult<BigInt> {
    positive_modulus("powmod", m)?;
    if e.is_negative() {
        let inv = modinv(b, m)?;
        return Ok(inv.modpow(&-e, m));
    }
    Ok(b.mod_floor(m).modpow(e, m))
}

/// The Chinese remainder theorem: `x ≡ rᵢ (mod mᵢ)` → `(x, lcm mᵢ)`.
pub fn crt(residues: &[BigInt], moduli: &[BigInt]) -> MathResult<(BigInt, BigInt)> {
    if residues.len() != moduli.len() || residues.is_empty() {
        return Err(MathError::Domain {
            function: "crt".into(),
            detail: "give as many residues as moduli".into(),
        });
    }
    let mut x = BigInt::zero();
    let mut m = BigInt::one();
    for (r, mi) in residues.iter().zip(moduli) {
        positive_modulus("crt", mi)?;
        // x + m·k ≡ r (mod mi)
        let (g, s, _) = egcd(&m, mi);
        let diff = r - &x;
        if !(&diff % &g).is_zero() {
            return Err(MathError::Undefined(format!(
                "a solution (the congruences mod {m} and mod {mi} contradict each other)"
            )));
        }
        let mi_g = mi / &g;
        let k = ((&diff / &g) * s).mod_floor(&mi_g);
        x += &m * k;
        m *= &mi_g;
        x = x.mod_floor(&m);
        too_big(&m)?;
    }
    Ok((x, m))
}

/// Primes below the trial-division bound (sieve of Eratosthenes).
fn small_primes() -> Vec<u32> {
    let n = TRIAL_LIMIT as usize;
    let mut composite = vec![false; n];
    let mut out = Vec::new();
    for i in 2..n {
        if !composite[i] {
            out.push(i as u32);
            let mut j = i * i;
            while j < n {
                composite[j] = true;
                j += i;
            }
        }
    }
    out
}

/// Miller–Rabin with fixed bases: exact below 3.3·10²⁴, overwhelmingly likely above.
pub fn is_prime(n: &BigInt) -> MathResult<bool> {
    too_big(n)?;
    if n < &BigInt::from(2) {
        return Ok(false);
    }
    for p in BASES {
        let p = BigInt::from(p);
        if n == &p {
            return Ok(true);
        }
        if (n % &p).is_zero() {
            return Ok(false);
        }
    }
    let one = BigInt::one();
    let n1 = n - &one;
    let mut d = n1.clone();
    let mut s = 0u32;
    while d.is_even() {
        d >>= 1;
        s += 1;
    }
    'bases: for a in BASES {
        let mut x = BigInt::from(a).modpow(&d, n);
        if x == one || x == n1 {
            continue;
        }
        for _ in 1..s {
            x = (&x * &x) % n;
            if x == n1 {
                continue 'bases;
            }
        }
        return Ok(false);
    }
    Ok(true)
}

/// The smallest prime above `n`.
pub fn next_prime(n: &BigInt) -> MathResult<BigInt> {
    let mut k = if n < &BigInt::from(2) {
        BigInt::from(2)
    } else {
        n + 1
    };
    for _ in 0..1_000_000 {
        if is_prime(&k)? {
            return Ok(k);
        }
        k += 1;
    }
    Err(MathError::TooLarge("the gap to the next prime".into()))
}

/// Pollard's rho (Brent's cycle detection) for a non-trivial factor of composite `n`.
fn rho(n: &BigInt) -> Option<BigInt> {
    let one = BigInt::one();
    for c in 1u32..20 {
        let c = BigInt::from(c);
        let f = |x: &BigInt| (x * x + &c) % n;
        let (mut x, mut y, mut g) = (BigInt::from(2), BigInt::from(2), one.clone());
        let mut steps = 0u64;
        while g.is_one() {
            x = f(&x);
            y = f(&f(&y));
            g = (&x - &y).abs().gcd(n);
            steps += 1;
            if steps > RHO_LIMIT {
                return None;
            }
        }
        if &g != n {
            return Some(g);
        }
    }
    None
}

/// Prime factorisation `[(p, e)]` of `|n|`, primes ascending.
pub fn factorint(n: &BigInt) -> MathResult<Vec<(BigInt, u32)>> {
    too_big(n)?;
    let mut n = n.abs();
    let mut out: Vec<(BigInt, u32)> = Vec::new();
    if n.is_zero() {
        return Err(MathError::Undefined("the factorisation of 0".into()));
    }
    for p in small_primes() {
        let pb = BigInt::from(p);
        if &pb * &pb > n {
            break;
        }
        let mut e = 0;
        while (&n % &pb).is_zero() {
            n /= &pb;
            e += 1;
        }
        if e > 0 {
            out.push((pb, e));
        }
    }
    let mut stack = vec![n];
    let mut big: Vec<BigInt> = Vec::new();
    while let Some(m) = stack.pop() {
        if m.is_one() {
            continue;
        }
        if is_prime(&m)? {
            big.push(m);
            continue;
        }
        let f = rho(&m).ok_or_else(|| {
            MathError::TooLarge(format!("factoring {m} (no factor found in time)"))
        })?;
        stack.push(&m / &f);
        stack.push(f);
    }
    big.sort();
    for p in big {
        match out.last_mut() {
            Some((q, e)) if *q == p => *e += 1,
            _ => out.push((p, 1)),
        }
    }
    out.sort();
    Ok(out)
}

/// Euler's totient φ(n).
pub fn totient(n: &BigInt) -> MathResult<BigInt> {
    if !n.is_positive() {
        return Err(MathError::Domain {
            function: "totient".into(),
            detail: "φ(n) needs a positive integer".into(),
        });
    }
    let mut phi = n.clone();
    for (p, _) in factorint(n)? {
        phi = phi / &p * (&p - 1);
    }
    Ok(phi)
}

/// The number theory domain.
pub struct NumberTheory;

const OPS: &[&str] = &[
    "mod",
    "modinv",
    "powmod",
    "crt",
    "egcd",
    "isprime",
    "nextprime",
    "factorint",
    "totient",
];

fn list_ints(op: &str, e: &Expr) -> MathResult<Vec<BigInt>> {
    match e.node() {
        ExprNode::Tuple(v) | ExprNode::Set(v) => v.iter().map(|x| int_arg(op, x)).collect(),
        _ => Ok(vec![int_arg(op, e)?]),
    }
}

impl Domain for NumberTheory {
    fn name(&self) -> &'static str {
        "number theory"
    }

    fn operators(&self) -> &'static [&'static str] {
        OPS
    }

    fn apply(&self, op: &str, args: &[Value], _ctx: &mut Ctx<'_>) -> Option<MathResult<Value>> {
        Some(apply(op, args))
    }
}

fn apply(op: &str, args: &[Value]) -> MathResult<Value> {
    let int = |i: usize| int_arg(op, &args[i]);
    let truth = |b: bool| Expr::new(ExprNode::Bool(b));
    match op {
        "mod" => {
            expect_args(op, args, 2)?;
            Ok(Expr::bigint(modulo(&int(0)?, &int(1)?)?))
        }
        "modinv" => {
            expect_args(op, args, 2)?;
            Ok(Expr::bigint(modinv(&int(0)?, &int(1)?)?))
        }
        "powmod" => {
            expect_args(op, args, 3)?;
            Ok(Expr::bigint(powmod(&int(0)?, &int(1)?, &int(2)?)?))
        }
        "crt" => {
            expect_args(op, args, 2)?;
            let (x, m) = crt(&list_ints(op, &args[0])?, &list_ints(op, &args[1])?)?;
            Ok(Expr::new(ExprNode::Tuple(vec![
                Expr::bigint(x),
                Expr::bigint(m),
            ])))
        }
        "egcd" => {
            expect_args(op, args, 2)?;
            let (g, s, t) = egcd(&int(0)?, &int(1)?);
            Ok(Expr::new(ExprNode::Tuple(vec![
                Expr::bigint(g),
                Expr::bigint(s),
                Expr::bigint(t),
            ])))
        }
        "isprime" => {
            expect_args(op, args, 1)?;
            Ok(truth(is_prime(&int(0)?)?))
        }
        "nextprime" => {
            expect_args(op, args, 1)?;
            Ok(Expr::bigint(next_prime(&int(0)?)?))
        }
        "factorint" => {
            expect_args(op, args, 1)?;
            let fs = factorint(&int(0)?)?;
            Ok(Expr::new(ExprNode::Tuple(
                fs.into_iter()
                    .map(|(p, e)| {
                        Expr::new(ExprNode::Tuple(vec![
                            Expr::bigint(p),
                            Expr::int(i64::from(e)),
                        ]))
                    })
                    .collect(),
            )))
        }
        "totient" => {
            expect_args(op, args, 1)?;
            Ok(Expr::bigint(totient(&int(0)?)?))
        }
        _ => Err(MathError::UnknownFunction(op.into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn b(n: i64) -> BigInt {
        BigInt::from(n)
    }

    #[test]
    fn modular_basics() {
        assert_eq!(modulo(&b(-7), &b(3)).unwrap(), b(2));
        assert_eq!(modinv(&b(3), &b(11)).unwrap(), b(4));
        assert!(modinv(&b(6), &b(9)).is_err());
        assert_eq!(powmod(&b(2), &b(10), &b(1000)).unwrap(), b(24));
        assert_eq!(powmod(&b(3), &b(-1), &b(11)).unwrap(), b(4));
        assert_eq!(
            crt(&[b(2), b(3), b(2)], &[b(3), b(5), b(7)]).unwrap(),
            (b(23), b(105))
        );
        assert!(crt(&[b(1), b(2)], &[b(4), b(6)]).is_err());
        assert_eq!(egcd(&b(240), &b(46)).0, b(2));
    }

    #[test]
    fn primes_and_factors() {
        assert!(is_prime(&b(2)).unwrap());
        assert!(is_prime(&b(1_000_000_007)).unwrap());
        assert!(!is_prime(&b(561)).unwrap()); // Carmichael number
        assert!(!is_prime(&b(1)).unwrap());
        assert_eq!(next_prime(&b(13)).unwrap(), b(17));
        assert_eq!(
            factorint(&b(360)).unwrap(),
            vec![(b(2), 3), (b(3), 2), (b(5), 1)]
        );
        let n = b(1_000_000_007) * b(998_244_353);
        assert_eq!(
            factorint(&n).unwrap(),
            vec![(b(998_244_353), 1), (b(1_000_000_007), 1)]
        );
        assert_eq!(totient(&b(36)).unwrap(), b(12));
        assert_eq!(totient(&b(1)).unwrap(), b(1));
    }

    #[test]
    fn domain_operators() {
        let mut ctx = Ctx::new();
        let r = NumberTheory
            .apply("factorint", &[Expr::int(12)], &mut ctx)
            .unwrap()
            .unwrap();
        assert_eq!(r.to_string(), "((2, 2), (3, 1))");
        let r = NumberTheory
            .apply("mod", &[Expr::rational(1, 2), Expr::int(3)], &mut ctx)
            .unwrap();
        assert!(r.is_err());
    }

    proptest! {
        #[test]
        fn egcd_is_bezout(a in -10_000i64..10_000, c in -10_000i64..10_000) {
            let (g, s, t) = egcd(&b(a), &b(c));
            prop_assert_eq!(&s * b(a) + &t * b(c), g.clone());
            prop_assert_eq!(g, b(a).gcd(&b(c)));
        }

        #[test]
        fn factorisation_multiplies_back(n in 1i64..5_000_000) {
            let fs = factorint(&b(n)).unwrap();
            let prod = fs.iter().fold(BigInt::one(), |acc, (p, e)| acc * p.pow(*e));
            prop_assert_eq!(prod, b(n));
            for (p, _) in fs {
                prop_assert!(is_prime(&p).unwrap());
            }
        }
    }
}
