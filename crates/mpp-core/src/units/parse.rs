//! Reading units from text: `m/s^2`, `km/h`, `kg m/s²`, `N·m`, `J/(kg·K)`,
//! `\mathrm{kg\,m/s^2}`, `^\circ C`.
//!
//! Grammar (after LaTeX spacing and wrappers are removed):
//! ```text
//! unit     = product { "/" product }          a / b c  means  a / (b·c)
//! product  = factor { [ "·" | "*" | " " ] factor }
//! factor   = primary [ exponent ]
//! primary  = "(" unit ")" | name | "1"
//! exponent = "^" signed-rational | "^(" signed-rational ")" | superscripts (² ³ ⁻¹ …)
//! ```

use num_rational::Rational64;

use crate::error::{MathError, MathResult};
use crate::units::table;
use crate::units::unit::Unit;

/// Longest unit text accepted.
pub const MAX_LEN: usize = 256;
/// Deepest nesting of parentheses.
const MAX_DEPTH: usize = 32;

fn err(msg: String) -> MathError {
    MathError::Dimension(msg)
}

/// Rewrites LaTeX unit markup into the plain grammar.
fn clean(text: &str) -> MathResult<String> {
    let mut s = text.trim().replace("{}", "");
    for (from, to) in [
        ("^{\\circ}", "°"),
        ("^\\circ", "°"),
        ("\\circ", "°"),
        ("\\degree", "°"),
        ("\\mathrm", ""),
        ("\\textrm", ""),
        ("\\text", ""),
        ("\\operatorname", ""),
        ("\\rm", ""),
        ("\\cdot", "·"),
        ("\\times", "·"),
        ("\\mu", "µ"),
        ("\\Omega", "Ω"),
        ("\\,", " "),
        ("\\;", " "),
        ("\\:", " "),
        ("\\!", ""),
        ("\\ ", " "),
        ("~", " "),
        ("{", "("),
        ("}", ")"),
        ("*", "·"),
        ("⋅", "·"),
        ("•", "·"),
        ("−", "-"),
    ] {
        s = s.replace(from, to);
    }
    if let Some(i) = s.find('\\') {
        let cmd: String = s[i..].chars().take_while(|c| !c.is_whitespace()).collect();
        return Err(err(format!("`{cmd}` can't be used in a unit")));
    }
    // A degree sign belongs to the C or F after it, and `\mu` to the unit after it,
    // however they were spaced or wrapped.
    for (from, to) in [
        ("° C", "°C"),
        ("° F", "°F"),
        ("°(C)", "°C"),
        ("°(F)", "°F"),
        ("µ ", "µ"),
    ] {
        s = s.replace(from, to);
    }
    Ok(s)
}

const SUPERSCRIPTS: [(char, char); 12] = [
    ('⁰', '0'),
    ('¹', '1'),
    ('²', '2'),
    ('³', '3'),
    ('⁴', '4'),
    ('⁵', '5'),
    ('⁶', '6'),
    ('⁷', '7'),
    ('⁸', '8'),
    ('⁹', '9'),
    ('⁻', '-'),
    ('⁺', '+'),
];

fn is_name_char(c: char) -> bool {
    c.is_alphabetic() || matches!(c, 'µ' | 'μ' | 'Ω' | '°' | '℃' | '℉' | '\u{2126}')
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    depth: usize,
    source: String,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn skip_spaces(&mut self) -> bool {
        let start = self.pos;
        while self.peek().is_some_and(char::is_whitespace) {
            self.pos += 1;
        }
        self.pos > start
    }

    fn unexpected(&self) -> MathError {
        match self.peek() {
            Some(c) => err(format!("unexpected `{c}` in the unit `{}`", self.source)),
            None => err(format!("the unit `{}` ends too early", self.source)),
        }
    }

    fn unit(&mut self) -> MathResult<Unit> {
        let mut u = self.product()?;
        loop {
            self.skip_spaces();
            if self.peek() == Some('/') {
                self.pos += 1;
                let d = self.product()?;
                u = u.div(&d)?;
            } else {
                return Ok(u);
            }
        }
    }

    fn product(&mut self) -> MathResult<Unit> {
        self.skip_spaces();
        let mut u = self.factor()?;
        loop {
            let spaced = self.skip_spaces();
            match self.peek() {
                Some('·') => {
                    self.pos += 1;
                    self.skip_spaces();
                    u = u.mul(&self.factor()?)?;
                }
                Some(c) if (spaced || c == '(') && (is_name_char(c) || c == '(') => {
                    u = u.mul(&self.factor()?)?;
                }
                Some(c) if is_name_char(c) => {
                    u = u.mul(&self.factor()?)?;
                }
                _ => return Ok(u),
            }
        }
    }

    fn factor(&mut self) -> MathResult<Unit> {
        let base = self.primary()?;
        match self.exponent()? {
            Some(k) => base.powr(k),
            None => Ok(base),
        }
    }

    fn primary(&mut self) -> MathResult<Unit> {
        match self.peek() {
            Some('(') => {
                self.depth += 1;
                if self.depth > MAX_DEPTH {
                    return Err(MathError::TooLarge("the nesting of this unit".into()));
                }
                self.pos += 1;
                let u = self.unit()?;
                self.skip_spaces();
                if self.peek() != Some(')') {
                    return Err(self.unexpected());
                }
                self.pos += 1;
                self.depth -= 1;
                Ok(u)
            }
            Some('1') => {
                self.pos += 1;
                Ok(Unit::one())
            }
            Some(c) if is_name_char(c) => {
                let start = self.pos;
                // A degree sign joins the letter after it (°C, °F) but nothing more.
                if c == '°' {
                    self.pos += 1;
                    if self.peek().is_some_and(|d| d == 'C' || d == 'F') {
                        self.pos += 1;
                    }
                } else {
                    while self.peek().is_some_and(|d| is_name_char(d) && d != '°') {
                        self.pos += 1;
                    }
                }
                let name: String = self.chars[start..self.pos].iter().collect();
                Unit::named(&name).ok_or_else(|| err(format!("`{name}` is not a known unit")))
            }
            _ => Err(self.unexpected()),
        }
    }

    fn exponent(&mut self) -> MathResult<Option<Rational64>> {
        if self.peek() == Some('^') {
            self.pos += 1;
            let paren = self.peek() == Some('(');
            if paren {
                self.pos += 1;
            }
            let k = self.rational()?;
            if paren {
                if self.peek() != Some(')') {
                    return Err(self.unexpected());
                }
                self.pos += 1;
            }
            return Ok(Some(k));
        }
        let mut text = String::new();
        while let Some(c) = self.peek() {
            match SUPERSCRIPTS.iter().find(|(s, _)| *s == c) {
                Some((_, d)) => {
                    text.push(*d);
                    self.pos += 1;
                }
                None => break,
            }
        }
        if text.is_empty() {
            return Ok(None);
        }
        let n: i64 = text
            .parse()
            .map_err(|_| err(format!("bad exponent in the unit `{}`", self.source)))?;
        Ok(Some(Rational64::from_integer(n)))
    }

    fn integer(&mut self) -> MathResult<i64> {
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
        }
        let digits: String = self.chars[start..self.pos].iter().collect();
        if digits.is_empty() {
            return Err(self.unexpected());
        }
        digits
            .parse()
            .map_err(|_| MathError::TooLarge("this unit exponent".into()))
    }

    fn rational(&mut self) -> MathResult<Rational64> {
        let mut sign = 1;
        match self.peek() {
            Some('-') => {
                sign = -1;
                self.pos += 1;
            }
            Some('+') => self.pos += 1,
            _ => {}
        }
        let n = self.integer()?;
        let d = if self.peek() == Some('/')
            && self
                .chars
                .get(self.pos + 1)
                .is_some_and(char::is_ascii_digit)
        {
            self.pos += 1;
            self.integer()?
        } else {
            1
        };
        if d == 0 {
            return Err(MathError::DivisionByZero);
        }
        Ok(Rational64::new(sign * n, d))
    }
}

/// Parses a unit written in text or LaTeX.
pub fn parse_unit(text: &str) -> MathResult<Unit> {
    if text.len() > MAX_LEN {
        return Err(MathError::TooLarge("this unit".into()));
    }
    let cleaned = clean(text)?;
    let mut p = Parser {
        chars: cleaned.chars().collect(),
        pos: 0,
        depth: 0,
        source: text.trim().to_string(),
    };
    p.skip_spaces();
    if p.peek().is_none() {
        return Err(err("an empty unit".into()));
    }
    let u = p.unit()?;
    p.skip_spaces();
    if p.peek().is_some() {
        return Err(p.unexpected());
    }
    Ok(u)
}

/// True when `name` is a unit symbol (with or without a prefix).
pub fn is_unit(name: &str) -> bool {
    table::lookup(name).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::Expr;

    fn text(s: &str) -> String {
        parse_unit(s).unwrap().text()
    }

    #[test]
    fn plain_text() {
        assert_eq!(text("m/s^2"), "m/s^2");
        assert_eq!(text("km/h"), "km/h");
        assert_eq!(text("kg m/s^2"), "kg·m/s^2");
        assert_eq!(text("kg*m/s²"), "kg·m/s^2");
        assert_eq!(text("N·m"), "N·m");
        assert_eq!(text("J/(kg·K)"), "J/(kg·K)");
        assert_eq!(text("J/kg K"), "J/(kg·K)");
        assert_eq!(text("m/s/s"), "m/s^2");
        assert_eq!(text("s⁻¹"), "1/s");
        assert_eq!(text("m^(1/2)"), "m^(1/2)");
        assert_eq!(text("1/s"), "1/s");
        assert_eq!(text("°C"), "°C");
        assert_eq!(text("degF"), "°F");
        assert_eq!(text("µs"), "µs");
        assert_eq!(text("kΩ"), "kΩ");
        assert_eq!(text("mph"), "mph");
    }

    #[test]
    fn latex() {
        assert_eq!(text("\\mathrm{kg\\,m/s^2}"), "kg·m/s^2");
        assert_eq!(text("\\mathrm{m}\\,\\mathrm{s}^{-2}"), "m/s^2");
        assert_eq!(text("^\\circ C"), "°C");
        assert_eq!(text("\\mu m"), "µm");
        assert_eq!(text("\\mathrm{N}\\cdot\\mathrm{m}"), "N·m");
        assert_eq!(text("k\\Omega"), "kΩ");
        assert_eq!(text("{}^\\circ\\mathrm{C}"), "°C");
    }

    #[test]
    fn scales_come_through() {
        assert_eq!(parse_unit("km/h").unwrap().scale(), Expr::rational(5, 18));
        assert_eq!(parse_unit("g/cm^3").unwrap().scale(), Expr::int(1000));
    }

    #[test]
    fn errors_read_well() {
        assert_eq!(
            parse_unit("m/furlong").unwrap_err().to_string(),
            "`furlong` is not a known unit"
        );
        assert!(parse_unit("m/(s").is_err());
        assert!(parse_unit("").is_err());
        assert!(parse_unit("\\frac{m}{s}").is_err());
        assert!(parse_unit("m^").is_err());
        assert!(parse_unit(&"m".repeat(300)).is_err());
        assert!(parse_unit(&"(".repeat(100)).is_err());
        assert!(is_unit("km") && !is_unit("x"));
    }
}
