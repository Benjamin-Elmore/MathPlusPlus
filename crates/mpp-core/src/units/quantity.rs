//! Quantities: a value with a unit, and arithmetic that checks dimensions.

use std::cmp::Ordering;
use std::fmt;

use num_rational::Rational64;
use num_traits::ToPrimitive;

use crate::domains::util;
use crate::error::{MathError, MathResult};
use crate::expr::Expr;
use crate::number::Number;
use crate::units::unit::Unit;

/// A value with a unit, such as `9.81 m/s^2`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quantity {
    /// The number (or expression) in front of the unit.
    pub value: Expr,
    /// The unit.
    pub unit: Unit,
}

/// How a unit is named in messages.
pub fn unit_name(u: &Unit) -> String {
    if u.is_one() {
        "a plain number".into()
    } else {
        u.text()
    }
}

fn mismatch(verb: &str, a: &Unit, b: &Unit) -> MathError {
    MathError::Dimension(format!(
        "can't {verb} {} and {}",
        unit_name(a),
        unit_name(b)
    ))
}

impl Quantity {
    /// `value unit`.
    pub fn new(value: Expr, unit: Unit) -> Quantity {
        Quantity { value, unit }
    }

    /// A plain number.
    pub fn number(value: Expr) -> Quantity {
        Quantity::new(value, Unit::one())
    }

    /// Parses the unit from text: `Quantity::parse(Expr::int(3), "km/h")`.
    pub fn parse(value: Expr, unit: &str) -> MathResult<Quantity> {
        Ok(Quantity::new(value, crate::units::parse_unit(unit)?))
    }

    /// True when the dimensions agree.
    pub fn same_dimension(&self, other: &Quantity) -> bool {
        self.unit.dimension() == other.unit.dimension()
    }

    /// `other`'s value expressed as a difference in `self`'s unit.
    fn value_in_my_unit(&self, other: &Quantity) -> Expr {
        if other.unit == self.unit {
            return other.value.clone();
        }
        (&other.value * &(other.unit.scale() / self.unit.scale())).simplify()
    }

    fn checked_sum(&self, other: &Quantity, verb: &str, sign: i64) -> MathResult<Quantity> {
        if !self.same_dimension(other) {
            return Err(mismatch(verb, &self.unit, &other.unit));
        }
        let rhs = self.value_in_my_unit(other);
        Ok(Quantity::new(
            (&self.value + &(Expr::int(sign) * rhs)).simplify(),
            self.unit.clone(),
        ))
    }

    /// `self + other`, in `self`'s unit. An affine temperature on the left takes the
    /// right side as a difference (20 °C + 5 K = 25 °C).
    pub fn add(&self, other: &Quantity) -> MathResult<Quantity> {
        self.checked_sum(other, "add", 1)
    }

    /// `self − other`, in `self`'s unit.
    pub fn sub(&self, other: &Quantity) -> MathResult<Quantity> {
        self.checked_sum(other, "subtract", -1)
    }

    /// `−self`.
    pub fn neg(&self) -> Quantity {
        Quantity::new((-&self.value).simplify(), self.unit.clone())
    }

    fn finish(value: Expr, unit: Unit) -> Quantity {
        let (u, k) = unit.simplified();
        Quantity::new((value * k).simplify(), u)
    }

    /// `self · other`, with the unit simplified (`kg · m/s²` → `N`).
    pub fn mul(&self, other: &Quantity) -> MathResult<Quantity> {
        let unit = self.unit.mul(&other.unit)?;
        Ok(Quantity::finish(&self.value * &other.value, unit))
    }

    /// `self / other`.
    pub fn div(&self, other: &Quantity) -> MathResult<Quantity> {
        if util::is_zero(&other.value) {
            return Err(MathError::DivisionByZero);
        }
        let unit = self.unit.div(&other.unit)?;
        Ok(Quantity::finish(&self.value / &other.value, unit))
    }

    /// `self ^ exp`; a quantity with units needs a rational exponent.
    pub fn pow(&self, exp: &Quantity) -> MathResult<Quantity> {
        let e = exp.require_dimensionless("an exponent")?;
        if self.unit.is_one() {
            return Ok(Quantity::number(self.value.pow(e)));
        }
        let k = e
            .as_number()
            .and_then(Number::as_rational)
            .and_then(|q| Some(Rational64::new(q.numer().to_i64()?, q.denom().to_i64()?)))
            .ok_or_else(|| {
                MathError::Dimension(format!(
                    "a quantity in {} can only be raised to a fixed rational power",
                    self.unit
                ))
            })?;
        let unit = self.unit.powr(k)?;
        Ok(Quantity::finish(self.value.pow(e), unit))
    }

    /// The value as an absolute SI value (offsets applied).
    fn si_value(&self) -> Expr {
        let v = &self.value * &self.unit.scale();
        match self.unit.offset() {
            Some(o) => (v + o.clone()).simplify(),
            None => v.simplify(),
        }
    }

    /// Compares two quantities of the same dimension (`None` when unknown, for example
    /// with symbolic values).
    pub fn compare(&self, other: &Quantity) -> MathResult<Option<Ordering>> {
        if !self.same_dimension(other) {
            return Err(mismatch("compare", &self.unit, &other.unit));
        }
        Ok(util::compare(&self.si_value(), &other.si_value()))
    }

    /// The same quantity in `target` (`100 km/h in m/s` → `250/9 m/s`); offsets are
    /// applied for °C and °F.
    pub fn convert_to(&self, target: &Unit) -> MathResult<Quantity> {
        if self.unit.dimension() != target.dimension() {
            return Err(MathError::Dimension(format!(
                "can't convert {} to {}",
                unit_name(&self.unit),
                unit_name(target)
            )));
        }
        let mut v = self.si_value();
        if let Some(o) = target.offset() {
            v = v - o.clone();
        }
        Ok(Quantity::new(
            (v / target.scale()).simplify(),
            target.clone(),
        ))
    }

    /// The quantity in coherent SI base units (`km/h` → `m/s`).
    pub fn to_si(&self) -> Quantity {
        let target = self.unit.si_base();
        Quantity::new(self.si_value(), target)
    }

    /// The quantity with its unit tidied (see [`Unit::simplified`]).
    pub fn simplified(&self) -> Quantity {
        Quantity::finish(self.value.clone(), self.unit.clone())
    }

    /// The plain value of a dimensionless quantity, angles in radians; an error naming
    /// `what` otherwise ("sin needs a dimensionless argument, but got m").
    pub fn require_dimensionless(&self, what: &str) -> MathResult<Expr> {
        if !self.unit.is_dimensionless() {
            return Err(MathError::Dimension(format!(
                "{what} needs a dimensionless argument, but got {}",
                self.unit
            )));
        }
        Ok((&self.value * &self.unit.scale()).simplify())
    }

    /// Applies a built-in function by name: roots and powers keep units (`√(m²)` → `m`),
    /// `abs`, `floor`, `ceil` keep the unit, and every other function (sin, exp, ln …)
    /// needs a dimensionless argument.
    pub fn apply_function(&self, name: &str) -> MathResult<Quantity> {
        match name {
            "sqrt" => self.pow(&Quantity::number(Expr::rational(1, 2))),
            "cbrt" => self.pow(&Quantity::number(Expr::rational(1, 3))),
            "abs" | "floor" | "ceil" | "sign" => {
                let v = Expr::call(name, vec![self.value.clone()])?;
                let unit = if name == "sign" {
                    Unit::one()
                } else {
                    self.unit.clone()
                };
                Ok(Quantity::new(v, unit))
            }
            _ => {
                let x = self.require_dimensionless(name)?;
                Ok(Quantity::number(Expr::call(name, vec![x])?))
            }
        }
    }

    /// Unicode text: the value as [`Expr::to_text`] shows it, then the unit with
    /// superscript powers (`3t m/s²`).
    pub fn to_text(&self) -> String {
        let v = self.value.to_text();
        if self.unit.is_one() {
            v
        } else if self.unit.text() == "°" {
            format!("{v}°")
        } else {
            format!("{v} {}", self.unit.unicode_text())
        }
    }

    /// LaTeX: the value, a thin space and the upright unit.
    pub fn latex(&self) -> String {
        if self.unit.is_one() {
            self.value.to_latex()
        } else if self.unit.factors().len() == 1 && self.unit.text() == "°" {
            format!("{}^\\circ", self.value.to_latex())
        } else {
            format!("{}\\,{}", self.value.to_latex(), self.unit.latex())
        }
    }
}

impl fmt::Display for Quantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.unit.is_one() {
            write!(f, "{}", self.value)
        } else if self.unit.text() == "°" {
            write!(f, "{}°", self.value)
        } else {
            write!(f, "{} {}", self.value, self.unit)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn q(v: i64, u: &str) -> Quantity {
        Quantity::parse(Expr::int(v), u).unwrap()
    }

    #[test]
    fn checked_arithmetic() {
        assert_eq!(q(1, "km").add(&q(200, "m")).unwrap().to_string(), "6/5 km");
        assert_eq!(
            q(3, "m").add(&q(2, "s")).unwrap_err().to_string(),
            "can't add m and s"
        );
        assert_eq!(
            q(3, "m")
                .sub(&Quantity::number(Expr::one()))
                .unwrap_err()
                .to_string(),
            "can't subtract m and a plain number"
        );
        let f = q(2, "kg").mul(&q(3, "m/s^2")).unwrap();
        assert_eq!(f.to_string(), "6 N");
        let w = q(10, "J").div(&q(2, "s")).unwrap();
        assert_eq!(w.to_string(), "5 W");
        let a = q(4, "m^2").apply_function("sqrt").unwrap();
        assert_eq!(a.to_string(), "2 m");
        assert_eq!(q(20, "°C").add(&q(5, "K")).unwrap().to_string(), "25 °C");
    }

    #[test]
    fn comparisons() {
        assert_eq!(
            q(1, "km").compare(&q(999, "m")).unwrap(),
            Some(Ordering::Greater)
        );
        assert_eq!(
            q(0, "°C").compare(&q(273, "K")).unwrap(),
            Some(Ordering::Greater)
        );
        assert_eq!(
            q(1, "m").compare(&q(1, "kg")).unwrap_err().to_string(),
            "can't compare m and kg"
        );
    }

    #[test]
    fn conversions() {
        let v = q(100, "km/h")
            .convert_to(&crate::units::parse_unit("m/s").unwrap())
            .unwrap();
        assert_eq!(v.to_string(), "250/9 m/s");
        let t = q(100, "°C")
            .convert_to(&Unit::named("°F").unwrap())
            .unwrap();
        assert_eq!(t.value, Expr::int(212));
        let k = q(-40, "°F")
            .convert_to(&Unit::named("°C").unwrap())
            .unwrap();
        assert_eq!(k.value, Expr::int(-40));
        assert_eq!(
            q(1, "m")
                .convert_to(&Unit::named("s").unwrap())
                .unwrap_err()
                .to_string(),
            "can't convert m to s"
        );
        assert_eq!(q(1, "psi").to_si().unit.text(), "kg/(m·s^2)");
        let lbf = q(1, "lbf").convert_to(&Unit::named("N").unwrap()).unwrap();
        assert!(
            (lbf.value.eval_f64(&crate::eval::EmptyEnv).unwrap() - 4.448_221_615_260_5).abs()
                < 1e-12
        );
    }

    #[test]
    fn functions_need_dimensionless_arguments() {
        let s = q(30, "°").apply_function("sin").unwrap();
        assert_eq!(s.value, Expr::rational(1, 2));
        let r = q(2, "rad").apply_function("cos").unwrap();
        assert!(r.unit.is_one());
        assert_eq!(
            q(2, "m").apply_function("exp").unwrap_err().to_string(),
            "exp needs a dimensionless argument, but got m"
        );
        let ratio = q(3, "m").div(&q(1, "km")).unwrap();
        assert!(ratio.apply_function("ln").is_ok());
    }

    #[test]
    fn latex_is_upright_with_a_thin_space() {
        assert_eq!(q(9, "m/s^2").latex(), "9\\,\\mathrm{m/s^{2}}");
        assert_eq!(q(30, "°").latex(), "30^\\circ");
    }

    const UNITS: [&str; 12] = [
        "m", "km", "ft", "mi", "s", "h", "min", "kg", "lb", "N", "J", "W",
    ];

    proptest! {
        #[test]
        fn conversion_round_trips(v in -1_000_000i64..1_000_000, a in 0usize..12, b in 0usize..12, c in 0usize..12) {
            let ua = Unit::named(UNITS[a]).unwrap();
            let ub = Unit::named(UNITS[b]).unwrap();
            let x = Quantity::new(Expr::int(v), ua.clone());
            match x.convert_to(&ub) {
                Ok(y) => {
                    let back = y.convert_to(&ua).unwrap();
                    prop_assert_eq!(back.value, Expr::int(v));
                    let uc = Unit::named(UNITS[c]).unwrap();
                    if let Ok(z) = y.convert_to(&uc) {
                        prop_assert_eq!(z, x.convert_to(&uc).unwrap());
                    }
                }
                Err(e) => prop_assert!(e.to_string().starts_with("can't convert")),
            }
        }

        #[test]
        fn temperature_round_trips(v in -500i64..5000) {
            for (a, b) in [("°C", "°F"), ("°F", "K"), ("K", "°C")] {
                let x = Quantity::new(Expr::int(v), Unit::named(a).unwrap());
                let y = x.convert_to(&Unit::named(b).unwrap()).unwrap();
                prop_assert_eq!(y.convert_to(&Unit::named(a).unwrap()).unwrap().value, Expr::int(v));
            }
        }

        #[test]
        fn products_have_multiplied_dimensions(a in 0usize..12, b in 0usize..12) {
            let x = Quantity::new(Expr::int(2), Unit::named(UNITS[a]).unwrap());
            let y = Quantity::new(Expr::int(3), Unit::named(UNITS[b]).unwrap());
            let p = x.mul(&y).unwrap();
            let want = x.unit.dimension().mul(&y.unit.dimension()).unwrap();
            prop_assert_eq!(p.unit.dimension(), want);
            // The value is the same physical amount whatever unit the product picked.
            let si = |q: &Quantity| (&q.value * &q.unit.scale()).simplify();
            prop_assert_eq!(si(&p), (si(&x) * si(&y)).simplify());
        }
    }
}
