//! Units and unit checking.
//!
//! A [`Dimension`] is a vector of rational exponents of the seven SI base quantities.
//! A [`Unit`] is a product of named units (`km`, `h`, `°C` …) with exact scales, and a
//! [`Quantity`] is a value with a unit whose arithmetic checks dimensions: adding metres
//! to seconds is an error that names both units, `sin`, `exp` and `ln` need
//! dimensionless arguments (radians and degrees are fine), results are simplified to a
//! named derived unit where that reads better (`kg·m/s²` → `N`), and `convert_to` is
//! the `in` operator. [`calculus`] tells the calculus how units pass through
//! derivatives and integrals.

pub mod calculus;
pub mod dimension;
pub mod parse;
pub mod quantity;
pub mod table;
pub mod unit;

pub use dimension::Dimension;
pub use parse::{is_unit, parse_unit};
pub use quantity::Quantity;
pub use unit::Unit;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::Expr;

    /// The worked results in `examples/units.expected.txt`, recomputed through the API.
    #[test]
    fn example_file_matches() {
        let expected = include_str!("../../../../examples/units.expected.txt");
        let q = |v: Expr, u: &str| Quantity::parse(v, u).unwrap();
        let unit = |u: &str| parse_unit(u).unwrap();
        let g = q(Expr::rational(981, 100), "m/s^2");
        let m = q(Expr::int(70), "kg");
        let v = q(Expr::int(100), "km/h");
        let t = q(Expr::int(12), "s");
        // g was typed as the decimal 9.81, so its product is shown as a decimal.
        let decimal = crate::display::NumberFormat {
            style: crate::display::Style::Decimal,
            ..crate::display::NumberFormat::default()
        };
        let mut got: Vec<String> = vec![
            crate::display::with_format(&decimal, || m.mul(&g).unwrap().to_string()),
            v.convert_to(&unit("m/s")).unwrap().to_string(),
            v.mul(&t)
                .unwrap()
                .convert_to(&unit("m"))
                .unwrap()
                .to_string(),
            q(Expr::int(25), "°C")
                .convert_to(&unit("°F"))
                .unwrap()
                .to_string(),
            q(Expr::int(30), "°")
                .apply_function("sin")
                .unwrap()
                .to_string(),
            q(Expr::int(2), "kW")
                .mul(&q(Expr::int(3), "h"))
                .unwrap()
                .convert_to(&unit("kWh"))
                .unwrap()
                .to_string(),
        ];
        got.push(q(Expr::int(3), "m").add(&t).unwrap_err().to_string());
        got.push(
            q(Expr::int(2), "m")
                .apply_function("exp")
                .unwrap_err()
                .to_string(),
        );
        let want: Vec<&str> = expected
            .lines()
            .filter_map(|l| l.split_once('→').map(|(_, r)| r.trim()))
            .collect();
        assert_eq!(got, want);
    }
}
