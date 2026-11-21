//! Number-display directives: `%sf 4`, `%dp 2`, `%sci`, `%eng`, `%exact`, `%decimal`,
//! `%mixed`, `%improper`, `%deg`, `%rad`, `%polar`, `%rect`, `%plain`, and the
//! block-only form `%block sf 3`.
//!
//! A `%` line is a display directive only when a keyword follows the `%` straight
//! away (`%sf 4`, not `% sf 4`, which stays a comment).

/// One display option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayOption {
    /// `%exact`: exact values stay exact (the default).
    Exact,
    /// `%decimal`: everything is shown as a decimal.
    Decimal,
    /// `%sf n`: n significant figures.
    Sf(u32),
    /// `%dp n`: n decimal places.
    Dp(u32),
    /// `%sci [n]`: scientific notation from 10^n up and below 10^-n.
    Sci(Option<i32>),
    /// `%eng [n]`: engineering notation (exponents in steps of 3), same thresholds.
    Eng(Option<i32>),
    /// `%plain`: no forced notation, and the default digits again.
    Plain,
    /// `%mixed`: `3 1/2` for `7/2`.
    Mixed,
    /// `%improper`: `7/2` (the default).
    Improper,
    /// `%deg`: angle results in degrees.
    Deg,
    /// `%rad`: angle results in radians (the default).
    Rad,
    /// `%polar`: complex numbers as r∠θ.
    Polar,
    /// `%rect`: complex numbers as a + bi (the default).
    Rect,
}

/// A parsed display directive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayDirective {
    /// Written `%block …`: applies to the next block only.
    pub block_only: bool,
    pub option: DisplayOption,
}

const KEYWORDS: &[&str] = &[
    "sf", "dp", "sci", "eng", "exact", "decimal", "plain", "mixed", "improper", "deg", "rad",
    "polar", "rect", "block",
];

/// Reads the text after a `%`. `None` when it is not a display directive (a
/// comment); `Some(Err(message))` when it names an option but is malformed.
pub fn parse_display_directive(after_percent: &str) -> Option<Result<DisplayDirective, String>> {
    if !after_percent.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    let mut words = after_percent.split_whitespace();
    let first = words.next()?;
    if !KEYWORDS.contains(&first) {
        return None;
    }
    let mut rest: Vec<&str> = words.collect();
    let mut name = first;
    let mut block_only = false;
    if first == "block" {
        block_only = true;
        if rest.is_empty() {
            return Some(Err("`%block` needs an option, such as `%block sf 3`".into()));
        }
        name = rest.remove(0);
        if !KEYWORDS.contains(&name) || name == "block" {
            return Some(Err(format!("`%block {name}` is not a display option")));
        }
    }
    Some(read_option(name, &rest).map(|option| DisplayDirective { block_only, option }))
}

fn number<T: std::str::FromStr + PartialOrd + Copy + std::fmt::Display>(
    name: &str,
    args: &[&str],
    lo: T,
    hi: T,
) -> Result<T, String> {
    match args {
        [one] => match one.parse::<T>() {
            Ok(n) if n >= lo && n <= hi => Ok(n),
            Ok(_) => Err(format!("`%{name}` takes a number from {lo} to {hi}")),
            Err(_) => Err(format!("`%{name}` takes a whole number, not `{one}`")),
        },
        [] => Err(format!("`%{name}` needs a number, such as `%{name} 4`")),
        _ => Err(format!("`%{name}` takes one number")),
    }
}

fn optional_threshold(name: &str, args: &[&str]) -> Result<Option<i32>, String> {
    if args.is_empty() {
        Ok(None)
    } else {
        number(name, args, 0, 300).map(Some)
    }
}

fn none(name: &str, args: &[&str], option: DisplayOption) -> Result<DisplayOption, String> {
    if args.is_empty() {
        Ok(option)
    } else {
        Err(format!("`%{name}` takes no arguments"))
    }
}

fn read_option(name: &str, args: &[&str]) -> Result<DisplayOption, String> {
    match name {
        "sf" => number(name, args, 1, 50).map(DisplayOption::Sf),
        "dp" => number(name, args, 0, 50).map(DisplayOption::Dp),
        "sci" => optional_threshold(name, args).map(DisplayOption::Sci),
        "eng" => optional_threshold(name, args).map(DisplayOption::Eng),
        "exact" => none(name, args, DisplayOption::Exact),
        "decimal" => none(name, args, DisplayOption::Decimal),
        "plain" => none(name, args, DisplayOption::Plain),
        "mixed" => none(name, args, DisplayOption::Mixed),
        "improper" => none(name, args, DisplayOption::Improper),
        "deg" => none(name, args, DisplayOption::Deg),
        "rad" => none(name, args, DisplayOption::Rad),
        "polar" => none(name, args, DisplayOption::Polar),
        "rect" => none(name, args, DisplayOption::Rect),
        _ => Err(format!("`%{name}` is not a display option")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(s: &str) -> DisplayDirective {
        parse_display_directive(s).unwrap().unwrap()
    }

    #[test]
    fn every_directive_parses() {
        assert_eq!(ok("sf 4").option, DisplayOption::Sf(4));
        assert_eq!(ok("dp 2").option, DisplayOption::Dp(2));
        assert_eq!(ok("sci").option, DisplayOption::Sci(None));
        assert_eq!(ok("sci 6").option, DisplayOption::Sci(Some(6)));
        assert_eq!(ok("eng").option, DisplayOption::Eng(None));
        assert_eq!(ok("exact").option, DisplayOption::Exact);
        assert_eq!(ok("decimal").option, DisplayOption::Decimal);
        assert_eq!(ok("mixed").option, DisplayOption::Mixed);
        assert_eq!(ok("improper").option, DisplayOption::Improper);
        assert_eq!(ok("deg").option, DisplayOption::Deg);
        assert_eq!(ok("rad").option, DisplayOption::Rad);
        assert_eq!(ok("polar").option, DisplayOption::Polar);
        assert_eq!(ok("rect").option, DisplayOption::Rect);
        assert_eq!(ok("plain").option, DisplayOption::Plain);
        let b = ok("block sf 3");
        assert!(b.block_only);
        assert_eq!(b.option, DisplayOption::Sf(3));
    }

    #[test]
    fn comments_are_not_directives() {
        assert!(parse_display_directive(" sf 4").is_none());
        assert!(parse_display_directive("a note").is_none());
        assert!(parse_display_directive("noeval").is_none());
        assert!(parse_display_directive("").is_none());
    }

    #[test]
    fn malformed_ones_explain() {
        for bad in [
            "sf",
            "sf x",
            "sf 0",
            "dp -1",
            "exact now",
            "block",
            "block block sf 2",
            "sci x",
        ] {
            assert!(parse_display_directive(bad).unwrap().is_err(), "{bad}");
        }
    }
}
