//! Units: products of powers of named units, with exact scale factors.

use std::fmt;

use num_rational::Rational64;
use num_traits::{One, Signed, Zero};

use crate::error::{MathError, MathResult};
use crate::expr::Expr;
use crate::units::dimension::{Dimension, BASE_SYMBOLS};
use crate::units::table::{self, Atom, NAMED_DERIVED};

/// Most distinct factors one unit may have.
pub const MAX_FACTORS: usize = 32;

/// A unit such as `km/h` or `kg·m/s²`: named units with rational exponents, in the
/// order they were written. The empty product is the dimensionless unit `1`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Unit {
    factors: Vec<(Atom, Rational64)>,
}

fn int(n: i64) -> Rational64 {
    Rational64::from_integer(n)
}

impl Unit {
    /// The dimensionless unit.
    pub fn one() -> Unit {
        Unit::default()
    }

    /// One named unit, such as `m` or `km` (see [`table::lookup`]).
    pub fn named(symbol: &str) -> Option<Unit> {
        table::lookup(symbol).map(Unit::from_atom)
    }

    /// A unit made of one atom.
    pub fn from_atom(atom: Atom) -> Unit {
        Unit {
            factors: vec![(atom, int(1))],
        }
    }

    /// The factors and their exponents.
    pub fn factors(&self) -> &[(Atom, Rational64)] {
        &self.factors
    }

    /// True for the unit `1`.
    pub fn is_one(&self) -> bool {
        self.factors.is_empty()
    }

    /// The dimension.
    pub fn dimension(&self) -> Dimension {
        self.factors
            .iter()
            .try_fold(Dimension::none(), |acc, (a, e)| acc.mul(&a.dim.pow(*e)?))
            .unwrap_or_default()
    }

    /// True when the dimension is 1 (angles included).
    pub fn is_dimensionless(&self) -> bool {
        self.dimension().is_none()
    }

    /// The exact factor to the coherent SI unit (`km/h` → 5/18).
    pub fn scale(&self) -> Expr {
        crate::simplify::mul(
            self.factors
                .iter()
                .map(|(a, e)| a.scale.pow(rat(*e)))
                .collect(),
        )
    }

    /// The offset of an affine unit (°C, °F) standing alone; `None` otherwise (in a
    /// product such as J/(kg·°C) the degree is a temperature difference).
    pub fn offset(&self) -> Option<&Expr> {
        match self.factors.as_slice() {
            [(a, e)] if e.is_one() => a.offset.as_ref(),
            _ => None,
        }
    }

    /// True when only angle units occur (rad, °).
    pub fn is_angle(&self) -> bool {
        !self.factors.is_empty() && self.factors.iter().all(|(a, _)| a.angle)
    }

    fn push(&mut self, atom: &Atom, e: Rational64) -> MathResult<()> {
        if let Some(slot) = self
            .factors
            .iter_mut()
            .find(|(a, _)| a.symbol == atom.symbol)
        {
            slot.1 += e;
        } else {
            self.factors.push((atom.clone(), e));
        }
        self.factors.retain(|(_, e)| !e.is_zero());
        if self.factors.len() > MAX_FACTORS {
            return Err(MathError::TooLarge("this unit".into()));
        }
        Ok(())
    }

    /// `self · other`.
    pub fn mul(&self, other: &Unit) -> MathResult<Unit> {
        let mut out = self.clone();
        for (a, e) in &other.factors {
            out.push(a, *e)?;
        }
        out.dimension_checked()?;
        Ok(out)
    }

    /// `self / other`.
    pub fn div(&self, other: &Unit) -> MathResult<Unit> {
        self.mul(&other.powr(int(-1))?)
    }

    /// `self ^ k`.
    pub fn powr(&self, k: Rational64) -> MathResult<Unit> {
        let mut factors = Vec::with_capacity(self.factors.len());
        for (a, e) in &self.factors {
            let p = e
                .numer()
                .checked_mul(*k.numer())
                .zip(e.denom().checked_mul(*k.denom()))
                .filter(|(_, d)| *d != 0)
                .map(|(n, d)| Rational64::new(n, d))
                .ok_or_else(|| MathError::TooLarge("this unit exponent".into()))?;
            if !p.is_zero() {
                factors.push((a.clone(), p));
            }
        }
        let u = Unit { factors };
        u.dimension_checked()?;
        Ok(u)
    }

    fn dimension_checked(&self) -> MathResult<Dimension> {
        self.factors
            .iter()
            .try_fold(Dimension::none(), |acc, (a, e)| acc.mul(&a.dim.pow(*e)?))
    }

    /// The coherent SI unit of the same dimension, written in base units.
    pub fn si_base(&self) -> Unit {
        base_unit(&self.dimension())
    }

    /// A simpler unit for a computed result, with the factor the value must be
    /// multiplied by: mixed units of one kind are converted to SI (`km·m` → `m²`,
    /// `m/km` → `1`), and coherent SI products become the best-scoring named unit
    /// (`kg·m/s²` → `N`). Units as written by hand (`km/h`) are kept.
    pub fn simplified(&self) -> (Unit, Expr) {
        let dim = self.dimension();
        if dim.is_none() {
            if self.is_angle() && self.factors.len() == 1 {
                return (self.clone(), Expr::one());
            }
            if self.is_one() {
                return (Unit::one(), Expr::one());
            }
            return (Unit::one(), self.scale());
        }
        // A root of a unit (`√(J/kg)`) whose dimension has whole powers is written in
        // whole powers (`m/s`).
        let fractional = |u: &Unit| u.factors.iter().any(|(_, e)| !e.is_integer());
        if fractional(self) && !fractional(&base_unit(&dim)) {
            let best = best_named(&dim).unwrap_or_else(|| base_unit(&dim));
            return (best, self.scale());
        }
        let mixed = self.factors.iter().enumerate().any(|(i, (a, _))| {
            !a.dim.is_none()
                && self.factors[i + 1..]
                    .iter()
                    .any(|(b, _)| b.dim == a.dim && b.symbol != a.symbol)
        });
        let coherent = self
            .factors
            .iter()
            .all(|(a, _)| a.scale.is_one() && a.offset.is_none() && !a.angle);
        // A product of several units whose dimension has a name of its own is written
        // with that name: kΩ·µF is s, kPa·L is J.
        let named_whole = self.factors.len() > 1
            && (best_named(&dim).is_some() || base_unit(&dim).factors.len() == 1);
        if !(mixed || coherent || named_whole) {
            return (self.clone(), Expr::one());
        }
        let factor = if coherent { Expr::one() } else { self.scale() };
        let best = best_named(&dim).unwrap_or_else(|| base_unit(&dim));
        if coherent && score(self) <= score(&best) {
            return (self.clone(), factor);
        }
        (best, factor)
    }

    /// The unit as plain text (`kg·m/s^2`, `J/(kg·K)`, `1/s`).
    pub fn text(&self) -> String {
        self.render(|a| a.symbol.clone(), "·", |b, e| format!("{b}^{e}"))
    }

    /// The unit as Unicode text, with superscript powers (`kg·m/s²`, `m^(1/2)`).
    pub fn unicode_text(&self) -> String {
        self.render(
            |a| a.symbol.clone(),
            "·",
            |b, e| {
                let sup: Option<String> = e
                    .chars()
                    .map(|c| match c {
                        '0' => Some('⁰'),
                        '1' => Some('¹'),
                        '2' => Some('²'),
                        '3' => Some('³'),
                        '4' => Some('⁴'),
                        '5' => Some('⁵'),
                        '6' => Some('⁶'),
                        '7' => Some('⁷'),
                        '8' => Some('⁸'),
                        '9' => Some('⁹'),
                        '-' => Some('⁻'),
                        _ => None,
                    })
                    .collect();
                match sup {
                    Some(s) => format!("{b}{s}"),
                    None => format!("{b}^{e}"),
                }
            },
        )
    }

    /// The unit as LaTeX, upright: `\mathrm{kg\,m/s^{2}}`.
    pub fn latex(&self) -> String {
        if self.is_one() {
            return String::new();
        }
        format!(
            "\\mathrm{{{}}}",
            self.render(|a| a.latex.clone(), "\\,", |b, e| format!("{b}^{{{e}}}"))
        )
    }

    fn render(
        &self,
        name: impl Fn(&Atom) -> String,
        sep: &str,
        power: impl Fn(&str, &str) -> String,
    ) -> String {
        if self.is_one() {
            return "1".into();
        }
        let part = |a: &Atom, e: Rational64| {
            let b = name(a);
            if e.is_one() {
                b
            } else if e.is_integer() {
                power(&b, &e.numer().to_string())
            } else {
                power(&b, &format!("({e})"))
            }
        };
        let num: Vec<String> = self
            .factors
            .iter()
            .filter(|(_, e)| e.is_positive())
            .map(|(a, e)| part(a, *e))
            .collect();
        let den: Vec<String> = self
            .factors
            .iter()
            .filter(|(_, e)| e.is_negative())
            .map(|(a, e)| part(a, -*e))
            .collect();
        let top = if num.is_empty() {
            "1".to_string()
        } else {
            num.join(sep)
        };
        match den.len() {
            0 => top,
            1 => format!("{top}/{}", den[0]),
            _ => format!("{top}/({})", den.join(sep)),
        }
    }
}

impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.text())
    }
}

/// An exponent as an expression.
pub fn rat(e: Rational64) -> Expr {
    Expr::rational(*e.numer(), *e.denom())
}

/// `(number of factors, Σ|exponent|)`: lower reads better.
fn score(u: &Unit) -> (usize, Rational64) {
    (
        u.factors.len(),
        u.factors.iter().map(|(_, e)| e.abs()).sum(),
    )
}

/// The coherent SI unit of `dim` in base units (`kg·m/s^2`).
fn base_unit(dim: &Dimension) -> Unit {
    // Mass first reads naturally: kg·m/s², kg/(m·s²).
    let order = [1usize, 0, 2, 3, 4, 5, 6];
    let mut factors = Vec::new();
    for i in order {
        let e = dim.0[i];
        if !e.is_zero() {
            if let Some(a) = table::lookup(BASE_SYMBOLS[i]) {
                factors.push((a, e));
            }
        }
    }
    Unit { factors }
}

/// The best way to write `dim` with one named derived unit (alone or with base
/// units), when it beats the plain base form.
fn best_named(dim: &Dimension) -> Option<Unit> {
    let base = base_unit(dim);
    let mut best = base.clone();
    for sym in NAMED_DERIVED {
        let Some(atom) = table::lookup(sym) else {
            continue;
        };
        for k in [1i64, -1] {
            let Ok(rest) = dim.div(&atom.dim.pow(int(k)).unwrap_or_default()) else {
                continue;
            };
            let mut cand = Unit {
                factors: vec![(atom.clone(), int(k))],
            };
            for (a, e) in base_unit(&rest).factors {
                cand.factors.push((a, e));
            }
            if score(&cand) < score(&best) {
                best = cand;
            }
        }
    }
    (best != base).then_some(best)
}

impl Unit {
    /// Number of factors (a size measure).
    pub fn len(&self) -> usize {
        self.factors.len()
    }

    /// True for the unit `1`.
    pub fn is_empty(&self) -> bool {
        self.factors.is_empty()
    }
}

impl From<Atom> for Unit {
    fn from(a: Atom) -> Unit {
        Unit::from_atom(a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Unit {
        Unit::named(s).unwrap()
    }

    #[test]
    fn products_and_display() {
        let v = u("m").div(&u("s")).unwrap();
        assert_eq!(v.text(), "m/s");
        let a = v.div(&u("s")).unwrap();
        assert_eq!(a.text(), "m/s^2");
        let f = u("kg").mul(&a).unwrap();
        assert_eq!(f.text(), "kg·m/s^2");
        assert_eq!(f.latex(), "\\mathrm{kg\\,m/s^{2}}");
        let c = u("J").div(&u("kg").mul(&u("K")).unwrap()).unwrap();
        assert_eq!(c.text(), "J/(kg·K)");
        assert_eq!(Unit::one().powr(int(2)).unwrap().text(), "1");
        assert_eq!(Unit::one().div(&u("s")).unwrap().text(), "1/s");
        assert_eq!(u("kΩ").latex(), "\\mathrm{k\\Omega}");
    }

    #[test]
    fn scales() {
        let kmh = u("km").div(&u("h")).unwrap();
        assert_eq!(kmh.scale(), Expr::rational(5, 18));
        assert!(kmh.offset().is_none());
        assert_eq!(u("°C").offset(), Some(&Expr::rational(27315, 100)));
    }

    #[test]
    fn simplification() {
        let f = u("kg")
            .mul(&u("m"))
            .unwrap()
            .div(&u("s").powr(int(2)).unwrap())
            .unwrap();
        let (s, k) = f.simplified();
        assert_eq!((s.text(), k), ("N".to_string(), Expr::one()));
        let p = u("J").div(&u("s")).unwrap();
        assert_eq!(p.simplified().0.text(), "W");
        let v = u("m").div(&u("s")).unwrap();
        assert_eq!(v.simplified().0.text(), "m/s");
        let kmh = u("km").div(&u("h")).unwrap();
        assert_eq!(kmh.simplified().0.text(), "km/h");
        let mixed = u("km").mul(&u("m")).unwrap();
        let (s, k) = mixed.simplified();
        assert_eq!((s.text(), k), ("m^2".to_string(), Expr::int(1000)));
        let ratio = u("m").div(&u("km")).unwrap();
        let (s, k) = ratio.simplified();
        assert_eq!((s.text(), k), ("1".to_string(), Expr::rational(1, 1000)));
        let va = u("V").mul(&u("A")).unwrap();
        assert_eq!(va.simplified().0.text(), "W");
        let pa = u("N").div(&u("m").powr(int(2)).unwrap()).unwrap();
        assert_eq!(pa.simplified().0.text(), "Pa");
        assert_eq!(u("rad").simplified().0.text(), "rad");
    }
}
