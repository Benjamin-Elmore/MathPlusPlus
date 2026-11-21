//! The unit table: SI base and derived units, SI prefixes and common non-SI units.
//! Every scale is exact (a rational, or a rational multiple of π for degrees).

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Zero};

use crate::expr::Expr;
use crate::units::dimension::{Dimension, BASE_COUNT};

/// One named unit (with its prefix applied, if any).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Atom {
    /// The symbol as printed (`km`, `°C`, `Ω`).
    pub symbol: String,
    /// The LaTeX form inside `\mathrm{}` (`k\Omega`, `^\circ C`).
    pub latex: String,
    /// The factor to the coherent SI unit of the same dimension.
    pub scale: Expr,
    /// The offset of an affine temperature scale (°C, °F), in kelvin.
    pub offset: Option<Expr>,
    /// The dimension.
    pub dim: Dimension,
    /// True for angle units (rad, °), which are dimensionless but kept in results.
    pub angle: bool,
}

/// How a table entry is written.
struct Def {
    symbol: &'static str,
    latex: &'static str,
    dim: [i64; BASE_COUNT],
    scale: &'static str,
    offset: Option<&'static str>,
    prefixable: bool,
    angle: bool,
}

const fn def(
    symbol: &'static str,
    dim: [i64; BASE_COUNT],
    scale: &'static str,
    prefixable: bool,
) -> Def {
    Def {
        symbol,
        latex: symbol,
        dim,
        scale,
        offset: None,
        prefixable,
        angle: false,
    }
}

const NONE: [i64; BASE_COUNT] = [0; BASE_COUNT];
const LENGTH: [i64; BASE_COUNT] = [1, 0, 0, 0, 0, 0, 0];
const MASS: [i64; BASE_COUNT] = [0, 1, 0, 0, 0, 0, 0];
const TIME: [i64; BASE_COUNT] = [0, 0, 1, 0, 0, 0, 0];
const TEMP: [i64; BASE_COUNT] = [0, 0, 0, 0, 1, 0, 0];
const FORCE: [i64; BASE_COUNT] = [1, 1, -2, 0, 0, 0, 0];
const ENERGY: [i64; BASE_COUNT] = [2, 1, -2, 0, 0, 0, 0];
const PRESSURE: [i64; BASE_COUNT] = [-1, 1, -2, 0, 0, 0, 0];
const SPEED: [i64; BASE_COUNT] = [1, 0, -1, 0, 0, 0, 0];

const TABLE: &[Def] = &[
    // SI base units (the kilogram is k + g).
    def("m", LENGTH, "1", true),
    def("g", MASS, "1/1000", true),
    def("s", TIME, "1", true),
    def("A", [0, 0, 0, 1, 0, 0, 0], "1", true),
    def("K", TEMP, "1", true),
    def("mol", [0, 0, 0, 0, 0, 1, 0], "1", true),
    def("cd", [0, 0, 0, 0, 0, 0, 1], "1", true),
    // SI derived units.
    def("N", FORCE, "1", true),
    def("J", ENERGY, "1", true),
    def("W", [2, 1, -3, 0, 0, 0, 0], "1", true),
    def("Pa", PRESSURE, "1", true),
    def("Hz", [0, 0, -1, 0, 0, 0, 0], "1", true),
    def("C", [0, 0, 1, 1, 0, 0, 0], "1", true),
    def("V", [2, 1, -3, -1, 0, 0, 0], "1", true),
    Def {
        symbol: "Ω",
        latex: "\\Omega",
        dim: [2, 1, -3, -2, 0, 0, 0],
        scale: "1",
        offset: None,
        prefixable: true,
        angle: false,
    },
    def("T", [0, 1, -2, -1, 0, 0, 0], "1", true),
    def("F", [-2, -1, 4, 2, 0, 0, 0], "1", true),
    def("Wb", [2, 1, -2, -1, 0, 0, 0], "1", true),
    def("H", [2, 1, -2, -2, 0, 0, 0], "1", true),
    def("S", [-2, -1, 3, 2, 0, 0, 0], "1", true),
    // Angles (dimensionless, kept in results).
    Def {
        symbol: "rad",
        latex: "rad",
        dim: NONE,
        scale: "1",
        offset: None,
        prefixable: true,
        angle: true,
    },
    Def {
        symbol: "°",
        latex: "^\\circ",
        dim: NONE,
        scale: "pi/180",
        offset: None,
        prefixable: false,
        angle: true,
    },
    // Accepted and common non-SI units.
    def("L", [3, 0, 0, 0, 0, 0, 0], "1/1000", true),
    def("min", TIME, "60", false),
    def("h", TIME, "3600", false),
    def("day", TIME, "86400", false),
    // The Julian year, 365.25 days (the year of astronomy and of radiocarbon dating).
    def("yr", TIME, "31557600", false),
    def("t", MASS, "1000", false),
    def("eV", ENERGY, "1.602176634e-19", true),
    def("cal", ENERGY, "4.184", true),
    def("Wh", ENERGY, "3600", true),
    def("bar", PRESSURE, "100000", true),
    def("atm", PRESSURE, "101325", false),
    def("psi", PRESSURE, "44482216152605/6451600000", false),
    def("ft", LENGTH, "0.3048", false),
    def("in", LENGTH, "0.0254", false),
    def("yd", LENGTH, "0.9144", false),
    def("mi", LENGTH, "1609.344", false),
    def("mph", SPEED, "0.44704", false),
    def("lb", MASS, "0.45359237", false),
    def("lbf", FORCE, "4.4482216152605", false),
    Def {
        symbol: "°C",
        latex: "^\\circ C",
        dim: TEMP,
        scale: "1",
        offset: Some("273.15"),
        prefixable: false,
        angle: false,
    },
    Def {
        symbol: "°F",
        latex: "^\\circ F",
        dim: TEMP,
        scale: "5/9",
        offset: Some("45967/180"),
        prefixable: false,
        angle: false,
    },
];

/// Other spellings of table entries.
const ALIASES: &[(&str, &str)] = &[
    ("ohm", "Ω"),
    ("Ohm", "Ω"),
    ("\u{2126}", "Ω"), // OHM SIGN
    ("deg", "°"),
    ("degC", "°C"),
    ("degF", "°F"),
    ("℃", "°C"),
    ("℉", "°F"),
    ("l", "L"),
    ("hr", "h"),
    ("sec", "s"),
    ("lbs", "lb"),
];

/// SI prefixes and their powers of ten.
const PREFIXES: &[(&str, i32, &str)] = &[
    ("Q", 30, "Q"),
    ("R", 27, "R"),
    ("Y", 24, "Y"),
    ("Z", 21, "Z"),
    ("E", 18, "E"),
    ("P", 15, "P"),
    ("T", 12, "T"),
    ("G", 9, "G"),
    ("M", 6, "M"),
    ("k", 3, "k"),
    ("h", 2, "h"),
    ("da", 1, "da"),
    ("d", -1, "d"),
    ("c", -2, "c"),
    ("m", -3, "m"),
    ("µ", -6, "\\mu "),
    ("μ", -6, "\\mu "),
    ("u", -6, "\\mu "),
    ("n", -9, "n"),
    ("p", -12, "p"),
    ("f", -15, "f"),
    ("a", -18, "a"),
    ("z", -21, "z"),
    ("y", -24, "y"),
    ("r", -27, "r"),
    ("q", -30, "q"),
];

/// Parses an exact scale: `p/q`, a decimal with an optional exponent, or `pi/n`.
fn parse_scale(text: &str) -> Expr {
    if let Some(den) = text.strip_prefix("pi/") {
        let d: i64 = den.parse().unwrap_or(1);
        return Expr::pi() / Expr::int(d);
    }
    if let Some((p, q)) = text.split_once('/') {
        let (Ok(p), Ok(q)) = (p.parse::<BigInt>(), q.parse::<BigInt>()) else {
            return Expr::undefined();
        };
        if q.is_zero() {
            return Expr::undefined();
        }
        return Expr::ratio(BigRational::new(p, q));
    }
    let (mantissa, exp) = match text.split_once('e') {
        Some((m, e)) => (m, e.parse::<i32>().unwrap_or(0)),
        None => (text, 0),
    };
    let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits: BigInt = format!("{int}{frac}").parse().unwrap_or_default();
    let shift = exp - frac.len() as i32;
    let ten = BigInt::from(10);
    let q = if shift >= 0 {
        BigRational::from_integer(digits * num_traits::pow(ten, shift.unsigned_abs() as usize))
    } else {
        BigRational::new(digits, num_traits::pow(ten, shift.unsigned_abs() as usize))
    };
    Expr::ratio(q)
}

fn atom_from(d: &Def, prefix: Option<(&str, i32, &str)>) -> Atom {
    let mut scale = parse_scale(d.scale);
    let mut symbol = d.symbol.to_string();
    let mut latex = d.latex.to_string();
    if let Some((p, power, p_latex)) = prefix {
        let ten = BigRational::from_integer(BigInt::from(10));
        let factor = if power >= 0 {
            num_traits::pow(ten, power as usize)
        } else {
            BigRational::one() / num_traits::pow(ten, power.unsigned_abs() as usize)
        };
        scale = (scale * Expr::ratio(factor)).simplify();
        symbol = format!("{}{}", if p == "u" || p == "μ" { "µ" } else { p }, d.symbol);
        latex = format!("{p_latex}{}", d.latex);
    }
    Atom {
        symbol,
        latex,
        scale,
        offset: d.offset.map(parse_scale),
        dim: Dimension::from_ints(d.dim),
        angle: d.angle,
    }
}

/// Looks up a unit symbol: an exact name or alias first, then a prefix and a
/// prefixable unit (`km`, `µs`, `kWh`).
pub fn lookup(name: &str) -> Option<Atom> {
    let name = ALIASES
        .iter()
        .find(|(a, _)| *a == name)
        .map_or(name, |(_, c)| c);
    if let Some(d) = TABLE.iter().find(|d| d.symbol == name) {
        return Some(atom_from(d, None));
    }
    for &(p, power, latex) in PREFIXES {
        if let Some(rest) = name.strip_prefix(p) {
            if let Some(d) = TABLE.iter().find(|d| d.symbol == rest && d.prefixable) {
                return Some(atom_from(d, Some((p, power, latex))));
            }
        }
    }
    None
}

/// Every unit symbol in the table (without prefixes), for pickers and docs.
pub fn symbols() -> Vec<&'static str> {
    TABLE.iter().map(|d| d.symbol).collect()
}

/// Named coherent derived units used when simplifying results, in order of preference.
pub const NAMED_DERIVED: &[&str] = &["N", "J", "W", "Pa", "C", "V", "Ω", "T", "F", "Wb", "H", "S"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookups() {
        let km = lookup("km").unwrap();
        assert_eq!(km.scale, Expr::int(1000));
        assert_eq!(km.dim, Dimension::from_ints(LENGTH));
        assert_eq!(lookup("kg").unwrap().scale, Expr::one());
        assert_eq!(lookup("min").unwrap().scale, Expr::int(60));
        assert_eq!(lookup("mm").unwrap().scale, Expr::rational(1, 1000));
        assert_eq!(lookup("us").unwrap().symbol, "µs");
        assert_eq!(lookup("kWh").unwrap().scale, Expr::int(3_600_000));
        assert_eq!(lookup("°").unwrap().scale, Expr::pi() / Expr::int(180));
        assert_eq!(
            lookup("degC").unwrap().offset,
            Some(Expr::rational(27315, 100))
        );
        assert_eq!(
            lookup("cd").unwrap().dim,
            Dimension::from_ints([0, 0, 0, 0, 0, 0, 1])
        );
        assert!(lookup("kmin").is_none());
        assert!(lookup("xyz").is_none());
        assert_eq!(
            lookup("eV").unwrap().scale,
            Expr::ratio(BigRational::new(
                BigInt::from(1_602_176_634),
                num_traits::pow(BigInt::from(10), 28)
            ))
        );
    }

    #[test]
    fn every_entry_has_a_valid_scale() {
        for d in TABLE {
            let a = atom_from(d, None);
            assert!(!a.scale.is_undefined(), "{}", d.symbol);
            assert!(a.offset.iter().all(|o| !o.is_undefined()));
        }
    }
}
