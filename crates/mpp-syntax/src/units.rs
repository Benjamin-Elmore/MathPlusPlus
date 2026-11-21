//! Recognising units in maths text (grammar §2.x "Units").
//!
//! A letter is read as a unit only (1) straight after a number (`9.81 m/s^2`,
//! `3\,\mathrm{m}`, `30^\circ`, `25\,{}^\circ\mathrm{C}`), (2) as the target of a
//! conversion (`v \operatorname{in} \mathrm{km/h}`), or (3) inside `\mathrm{}` written
//! by the units picker. Anywhere else `m` is the variable `m`.
//!
//! This module only decides *where* a unit is written; the unit itself is checked and
//! interpreted by the core (`mpp_core::units::parse_unit`). The symbol list below must
//! agree with the core's unit table; a test in `mpp-eval` compares the two.

/// Unit symbols and whether an SI prefix may go in front.
const SYMBOLS: &[(&str, bool)] = &[
    ("m", true),
    ("g", true),
    ("s", true),
    ("A", true),
    ("K", true),
    ("mol", true),
    ("cd", true),
    ("N", true),
    ("J", true),
    ("W", true),
    ("Pa", true),
    ("Hz", true),
    ("C", true),
    ("V", true),
    ("Ω", true),
    ("T", true),
    ("F", true),
    ("Wb", true),
    ("H", true),
    ("S", true),
    ("rad", true),
    ("°", false),
    ("L", true),
    ("min", false),
    ("h", false),
    ("day", false),
    ("yr", false),
    ("t", false),
    ("eV", true),
    ("cal", true),
    ("Wh", true),
    ("bar", true),
    ("atm", false),
    ("psi", false),
    ("ft", false),
    ("in", false),
    ("yd", false),
    ("mi", false),
    ("mph", false),
    ("lb", false),
    ("lbf", false),
    ("°C", false),
    ("°F", false),
];

/// Other spellings of table units.
const ALIASES: &[&str] = &[
    "ohm", "Ohm", "\u{2126}", "deg", "degC", "degF", "℃", "℉", "l", "hr", "sec", "lbs",
];

/// SI prefixes.
const PREFIXES: &[&str] = &[
    "Q", "R", "Y", "Z", "E", "P", "T", "G", "M", "k", "h", "da", "d", "c", "m", "µ", "μ", "u", "n",
    "p", "f", "a", "z", "y", "r", "q",
];

/// Units that are not read in plain text after a number, because the word means
/// something else there (`2 in` is not inches, `\sec`, `\min` are functions).
const NOT_PLAIN: &[&str] = &["in", "sec", "hr", "deg", "l"];

/// True when `name` is a unit symbol, with or without an SI prefix.
pub fn is_unit_name(name: &str) -> bool {
    if ALIASES.contains(&name) || SYMBOLS.iter().any(|(s, _)| *s == name) {
        return true;
    }
    PREFIXES.iter().any(|p| {
        name.strip_prefix(p)
            .is_some_and(|rest| SYMBOLS.iter().any(|(s, pre)| *pre && *s == rest))
    })
}

/// A unit found in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Bytes consumed from the start of the scanned text (separators included).
    pub len: usize,
    /// Bytes of separator before the unit itself.
    pub lead: usize,
    /// The unit as the core reads it (`m/s^2`, `\mathrm{km/h}`, `°C`).
    pub text: String,
    /// True for a unit in plain letters (`m/s`), false for `\mathrm{}` and degree signs.
    pub plain: bool,
}

/// Separators allowed between a number and its unit.
const SEPARATORS: &[&str] = &["\\,", "\\;", "\\:", "\\ ", "\\!", "~", " ", "\t"];

fn skip_separators(text: &str) -> usize {
    let mut k = 0;
    'outer: loop {
        for sep in SEPARATORS {
            if text[k..].starts_with(sep) {
                k += sep.len();
                continue 'outer;
            }
        }
        return k;
    }
}

/// The unit straight after a number, if there is one: `text` starts just after the
/// number. A plain-letter unit needs a space before it or more than one letter
/// (`3 m`, `9.81m/s^2`, `2kg`), so `2x` and `3t` stay products.
pub fn after_number(text: &str) -> Option<Found> {
    let lead = skip_separators(text);
    let rest = &text[lead..];
    // `5\%`: percent.
    if let Some(r) = rest.strip_prefix(r"\%") {
        return Some(Found {
            len: lead + (rest.len() - r.len()),
            lead,
            text: "%".into(),
            plain: false,
        });
    }
    if let Some((len, unit)) = braceless_roman(rest) {
        return Some(Found {
            len: lead + len,
            lead,
            text: unit,
            plain: false,
        });
    }
    if let Some((len, unit)) = degrees(rest) {
        return Some(Found {
            len: lead + len,
            lead,
            text: unit,
            plain: false,
        });
    }
    if let Some((len, text)) = roman_unit(rest) {
        return Some(Found {
            len: lead + len,
            lead,
            text,
            plain: false,
        });
    }
    let len = plain(rest)?;
    let unit = &rest[..len];
    // Without a space, `5t^2` is a product: only a compound unit (`m/s`) or one whose
    // first symbol has several letters (`2kg`, `10min`) counts.
    if lead == 0 && !is_compound(unit) {
        return None;
    }
    if NOT_PLAIN.contains(&unit) {
        return None;
    }
    Some(Found {
        len: lead + len,
        lead,
        text: unit.to_string(),
        plain: true,
    })
}

/// True when a plain unit can't be mistaken for a one-letter name: it has a `/` or `·`,
/// or its first symbol has two or more letters (`km`, `min`, `m/s`).
pub fn is_compound(unit: &str) -> bool {
    let first_name = unit.chars().take_while(|c| is_name_char(*c)).count();
    first_name >= 2 || unit.contains(['/', '·'])
}

/// The unit after `in` in a conversion (`\mathrm{km/h}`, `km/h`, `{}^\circ\mathrm{F}`).
pub fn conversion_target(text: &str) -> Option<Found> {
    let lead = skip_separators(text);
    let rest = &text[lead..];
    if let Some((len, unit)) = degrees(rest) {
        return Some(Found {
            len: lead + len,
            lead,
            text: unit,
            plain: false,
        });
    }
    if let Some((len, text)) = roman_unit(rest) {
        return Some(Found {
            len: lead + len,
            lead,
            text,
            plain: false,
        });
    }
    let len = plain(rest)?;
    Some(Found {
        len: lead + len,
        lead,
        text: rest[..len].to_string(),
        plain: true,
    })
}

/// `^\circ`, `^{\circ}`, `{}^\circ`, `°`, `℃`, each optionally followed by `C` or `F`
/// (plain, `\mathrm{C}` or `\text{C}`).
fn degrees(text: &str) -> Option<(usize, String)> {
    const MARKS: &[&str] = &[
        "{}^{\\circ}",
        "{}^\\circ",
        "^{\\circ}",
        "^\\circ",
        "°",
        "\\degree",
    ];
    if let Some(r) = text.strip_prefix('℃') {
        return Some((text.len() - r.len(), "°C".into()));
    }
    if let Some(r) = text.strip_prefix('℉') {
        return Some((text.len() - r.len(), "°F".into()));
    }
    let mark = MARKS.iter().find(|m| text.starts_with(**m))?;
    let mut k = mark.len();
    let after = &text[k..];
    let scale = [
        "\\mathrm{C}",
        "\\text{C}",
        "\\mathrm{F}",
        "\\text{F}",
        "C",
        "F",
    ]
    .iter()
    .find(|s| after.starts_with(**s))
    .filter(|s| {
        // A plain `C` must end there (`30^\circ Cx` is not Celsius).
        s.len() > 1 || !after[s.len()..].starts_with(|c: char| c.is_alphanumeric())
    });
    let unit = match scale {
        Some(s) => {
            k += s.len();
            if s.contains('C') {
                "°C"
            } else {
                "°F"
            }
        }
        None => "°",
    };
    Some((k, unit.to_string()))
}

/// `\mathrm K`, `\mathrm m^2`: `\mathrm` takes one letter without braces, as in TeX.
/// The unit comes back braced (`\mathrm{K}`).
fn braceless_roman(text: &str) -> Option<(usize, String)> {
    let after = text.strip_prefix("\\mathrm")?;
    let spaces = after.len() - after.trim_start().len();
    let r = &after[spaces..];
    let letter = r.chars().next().filter(char::is_ascii_alphabetic)?;
    // `\mathrm{` is the braced form; the letter itself must be a unit.
    if !is_unit_text(&letter.to_string(), true) {
        return None;
    }
    let mut k = "\\mathrm".len() + spaces + 1;
    // The letter must end the word (`\mathrm K` but not `\mathrm Km`).
    if text[k..].starts_with(|c: char| c.is_alphanumeric()) {
        return None;
    }
    let letter_end = k;
    k += power(&text[k..]).unwrap_or(0);
    Some((k, format!("\\mathrm{{{letter}}}{}", &text[letter_end..k])))
}

/// `\mathrm{…}` pieces with optional powers, joined by `\,`, `\cdot`, `·` or `/`, whose
/// contents are units: `\mathrm{m/s^2}`, `\mathrm{kg}\,\mathrm{m}\,\mathrm{s}^{-2}`.
fn roman(text: &str) -> Option<usize> {
    let mut k = roman_piece(text)?;
    loop {
        let rest = &text[k..];
        let join = ["\\,", "\\cdot", "·", "/", "\\;"]
            .iter()
            .find(|j| rest.trim_start_matches(' ').starts_with(**j));
        let Some(join) = join else { break };
        let spaces = rest.len() - rest.trim_start_matches(' ').len();
        let after = &rest[spaces + join.len()..];
        let inner = after.len() - after.trim_start_matches(' ').len();
        match roman_piece(&after[inner..]) {
            Some(len) => k += spaces + join.len() + inner + len,
            None => break,
        }
    }
    Some(k)
}

/// The micro prefix written as a Greek letter before the upright unit, as siunitx-free
/// papers do (`100\,\mu\mathrm{F}`, `5\,µ\mathrm{s}`): its length.
fn micro_roman(text: &str) -> Option<usize> {
    micro_parts(text).map(|(p, spaces, len)| p + spaces + len)
}

/// Upright units at the start of `text`: their length and the unit text. A micro
/// sign before them goes inside (`\mu\mathrm{F}` reads `\mathrm{\mu F}`).
fn roman_unit(text: &str) -> Option<(usize, String)> {
    if let Some(len) = roman(text) {
        return Some((len, text[..len].to_string()));
    }
    let (p, spaces, len) = micro_parts(text)?;
    let unit = &text[p + spaces..p + spaces + len];
    let open = unit.find('{')? + 1;
    Some((
        micro_roman(text)?,
        format!("{}\\mu {}", &unit[..open], &unit[open..]),
    ))
}

/// The prefix length, the spaces after it and the length of the upright unit.
fn micro_parts(text: &str) -> Option<(usize, usize, usize)> {
    let prefix = ["\\mu", "µ", "μ"].iter().find(|p| text.starts_with(**p))?;
    let after = &text[prefix.len()..];
    // `\mul` is not `\mu`.
    if *prefix == "\\mu" && after.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    let spaces = after.len() - after.trim_start_matches(' ').len();
    let len = roman(&after[spaces..])?;
    Some((prefix.len(), spaces, len))
}

/// One `\mathrm{unit}` (or `\text{unit}`, `\textrm{unit}`) with an optional power.
fn roman_piece(text: &str) -> Option<usize> {
    let open = ["\\mathrm{", "\\textrm{", "\\text{", "\\mathrm {"]
        .iter()
        .find(|o| text.starts_with(**o))?;
    let body_start = open.len();
    let mut depth = 1usize;
    let mut end = None;
    for (i, c) in text[body_start..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(body_start + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end?;
    if !is_unit_text(&text[body_start..end], true) {
        return None;
    }
    let mut k = end + 1;
    k += power(&text[k..]).unwrap_or(0);
    Some(k)
}

/// Longest plain unit looked for, in characters.
const MAX_PLAIN_UNIT: usize = 64;

/// The longest plain unit at the start of `text` that ends at a word boundary and is
/// not followed by `(` (so `\min(a, b)` and `2 sec(x)` stay functions).
fn plain(text: &str) -> Option<usize> {
    // A unit starts with a letter and is short; without these limits a long run such as
    // `^{2^{2^{…}}}` after a number made every candidate end a full rescan (cubic time).
    if !text
        .chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '°')
    {
        return None;
    }
    let run = text
        .char_indices()
        .take(MAX_PLAIN_UNIT)
        .find(|(_, c)| !is_plain_char(*c))
        .map_or_else(
            || {
                text.char_indices()
                    .nth(MAX_PLAIN_UNIT)
                    .map_or(text.len(), |(i, _)| i)
            },
            |(i, _)| i,
        );
    let mut ends: Vec<usize> = text[..run]
        .char_indices()
        .map(|(i, c)| i + c.len_utf8())
        .collect();
    ends.reverse();
    for end in ends {
        let next = text[end..].chars().next();
        if next.is_some_and(|c| c.is_alphanumeric() || c == '(' || c == '_' || c == '\'') {
            continue;
        }
        if is_unit_text(&text[..end], false) {
            return Some(end);
        }
    }
    None
}

fn is_plain_char(c: char) -> bool {
    c.is_ascii_alphabetic()
        || matches!(
            c,
            'µ' | 'μ' | 'Ω' | '°' | '/' | '·' | '^' | '{' | '}' | '(' | ')' | '-'
        )
        || c.is_ascii_digit()
        || is_superscript(c)
}

fn is_superscript(c: char) -> bool {
    matches!(
        c,
        '⁰' | '¹' | '²' | '³' | '⁴' | '⁵' | '⁶' | '⁷' | '⁸' | '⁹' | '⁻'
    )
}

/// A power after a unit: `^2`, `^{-2}`, `^{1/2}`, `^(1/2)`, `²`, `⁻¹`.
fn power(text: &str) -> Option<usize> {
    let sup: usize = text
        .chars()
        .take_while(|c| is_superscript(*c))
        .map(char::len_utf8)
        .sum();
    if sup > 0 {
        return Some(sup);
    }
    let rest = text.strip_prefix('^')?;
    if let Some(inner) = rest.strip_prefix('{').or_else(|| rest.strip_prefix('(')) {
        let close = inner.find(['}', ')'])?;
        let body = &inner[..close];
        let ok = !body.is_empty()
            && body
                .trim_start_matches('-')
                .split('/')
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
        return ok.then_some(1 + 1 + close + 1);
    }
    let digits = rest
        .strip_prefix('-')
        .map_or(rest, |r| r)
        .chars()
        .take_while(char::is_ascii_digit)
        .count();
    if digits == 0 {
        return None;
    }
    let minus = usize::from(rest.starts_with('-'));
    Some(1 + minus + digits)
}

/// True when `text` is a product and quotient of unit names with powers, such as
/// `m/s^2`, `kg·m/s²`, `J/(kg·K)` or `1/s`. Inside `\mathrm{}` (`roman`), spaces,
/// `\,`, `\cdot`, `*`, `\mu`, `\Omega` and `^\circ` are allowed too.
pub fn is_unit_text(text: &str, roman: bool) -> bool {
    let cleaned;
    let text = if roman {
        cleaned = text
            .replace("\\mu ", "µ")
            .replace("\\mu", "µ")
            .replace("\\Omega", "Ω")
            .replace("{}^\\circ", "°")
            .replace("^\\circ", "°")
            .replace("\\circ", "°")
            .replace("\\cdot", "·")
            .replace("\\,", "·")
            .replace("\\;", "·")
            .replace(['*', ' '], "·");
        cleaned.trim_matches('·')
    } else {
        text
    };
    if text.is_empty() || text.contains('\\') {
        return false;
    }
    let mut p = UnitScan {
        chars: text.chars().collect(),
        pos: 0,
        depth: 0,
    };
    p.quotient() && p.pos == p.chars.len()
}

struct UnitScan {
    chars: Vec<char>,
    pos: usize,
    depth: usize,
}

/// Deepest nesting of brackets in a unit.
const MAX_UNIT_DEPTH: usize = 8;

impl UnitScan {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn quotient(&mut self) -> bool {
        if !self.product() {
            return false;
        }
        while self.peek() == Some('/') {
            self.pos += 1;
            if !self.product() {
                return false;
            }
        }
        true
    }

    fn product(&mut self) -> bool {
        if !self.factor() {
            return false;
        }
        while self.peek() == Some('·') {
            self.pos += 1;
            if !self.factor() {
                return false;
            }
        }
        true
    }

    fn factor(&mut self) -> bool {
        match self.peek() {
            Some('(') => {
                if self.depth >= MAX_UNIT_DEPTH {
                    return false;
                }
                self.pos += 1;
                self.depth += 1;
                let ok = self.quotient() && self.peek() == Some(')');
                self.depth -= 1;
                if !ok {
                    return false;
                }
                self.pos += 1;
            }
            Some('1') => self.pos += 1,
            Some(c) if is_name_char(c) => {
                let start = self.pos;
                while self.peek().is_some_and(is_name_char) {
                    self.pos += 1;
                }
                let name: String = self.chars[start..self.pos].iter().collect();
                if !is_unit_name(&name) {
                    return false;
                }
            }
            _ => return false,
        }
        let rest: String = self.chars[self.pos..].iter().collect();
        if let Some(len) = power(&rest) {
            self.pos += rest[..len].chars().count();
        }
        true
    }
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphabetic() || matches!(c, 'µ' | 'μ' | 'Ω' | '°' | '℃' | '℉' | '\u{2126}')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(text: &str) -> Option<(String, usize)> {
        after_number(text).map(|f| (f.text, f.len))
    }

    #[test]
    fn units_after_a_number() {
        assert_eq!(unit(" m/s^2 + 1"), Some(("m/s^2".into(), 6)));
        assert_eq!(
            unit("\\,\\mathrm{m/s^2}"),
            Some(("\\mathrm{m/s^2}".into(), 16))
        );
        assert_eq!(unit(" km/h"), Some(("km/h".into(), 5)));
        assert_eq!(unit("kg"), Some(("kg".into(), 2)));
        assert_eq!(unit(" m"), Some(("m".into(), 2)));
        assert_eq!(unit("^\\circ"), Some(("°".into(), 6)));
        assert_eq!(unit("\\,{}^\\circ\\mathrm{C}"), Some(("°C".into(), 20)));
        assert_eq!(unit("°F"), Some(("°F".into(), 3)));
        assert_eq!(
            unit("\\,\\mathrm{kg}\\,\\mathrm{m}\\,\\mathrm{s}^{-2}").map(|u| u.0),
            Some("\\mathrm{kg}\\,\\mathrm{m}\\,\\mathrm{s}^{-2}".into())
        );
        assert_eq!(unit(" J/(kg·K)").map(|u| u.0), Some("J/(kg·K)".into()));
        assert_eq!(unit(" m s").map(|u| u.0), Some("m".into()));
        assert_eq!(unit(" min").map(|u| u.0), Some("min".into()));
    }

    #[test]
    fn products_stay_products() {
        assert_eq!(unit("x"), None);
        assert_eq!(unit("m"), None, "a single letter needs a space");
        assert_eq!(unit("t^2"), None);
        assert_eq!(unit(" x"), None);
        assert_eq!(unit(" mx"), None);
        assert_eq!(unit(" dx"), None);
        assert_eq!(unit(" d x"), None);
        assert_eq!(unit(" in"), None);
        assert_eq!(unit(" \\min(a, b)"), None);
        assert_eq!(unit(" min(a, b)"), None);
        assert_eq!(unit(" sec x"), None);
        assert_eq!(unit("\\,\\mathrm{d}x"), None);
        assert_eq!(unit(" m_1"), None);
        assert_eq!(unit(" g'"), None);
    }

    #[test]
    fn conversion_targets() {
        let t = |s: &str| conversion_target(s).map(|f| f.text);
        assert_eq!(t(" \\mathrm{m/s}"), Some("\\mathrm{m/s}".into()));
        assert_eq!(t(" km/h = ?"), Some("km/h".into()));
        assert_eq!(t(" {}^\\circ\\mathrm{F}"), Some("°F".into()));
        assert_eq!(t(" in"), Some("in".into()));
        assert_eq!(t(" x"), None);
    }

    #[test]
    fn unit_text() {
        for ok in [
            "m/s^2",
            "kg·m/s²",
            "J/(kg·K)",
            "1/s",
            "kΩ",
            "µs",
            "m^{-1}",
            "s⁻¹",
        ] {
            assert!(is_unit_text(ok, false), "{ok}");
        }
        for bad in ["", "x", "m/", "m^", "(m", "m//s", "kmin"] {
            assert!(!is_unit_text(bad, false), "{bad}");
        }
        assert!(is_unit_text("kg\\,m/s^2", true));
        assert!(is_unit_text("\\mu m", true));
        assert!(is_unit_text("k\\Omega", true));
        assert!(!is_unit_text("d", true));
        assert!(!is_unit_text("max", true));
    }
}
