//! Copying maths out of the IDE.
//!
//! Inside Math++ a copy keeps the source, so pasting into another `.mpp` file
//! keeps the maths exactly. For other applications the default is Unicode maths
//! (`∫₀¹ x² dx`); the option gives LaTeX instead. The right-click menu also
//! offers each form once: as LaTeX, as plain text and as code (U18).

use mpp_syntax::symbols::lookup_latex;

use crate::editor::forms::{to_code, to_latex};

/// What the clipboard gets for other applications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CopyFormat {
    /// Plain text with Unicode maths (`∫₀¹ x² dx`).
    #[default]
    Unicode,
    Latex,
    /// As a programmer types it (`int(x**2, x, 0, 1)`).
    Code,
}

/// The clipboard text for a selection of source text. `all_maths` says the
/// selection lies inside a maths span (otherwise `$ … $` pairs are looked for).
pub fn clipboard_text(selection: &str, all_maths: bool, format: CopyFormat) -> String {
    let convert: fn(&str) -> String = match format {
        CopyFormat::Latex => to_latex,
        CopyFormat::Unicode => maths_to_unicode,
        CopyFormat::Code => to_code,
    };
    if all_maths {
        convert(selection)
    } else {
        mixed(selection, convert, format == CopyFormat::Latex)
    }
}

/// The items of the right-click menu for a selection: the format chosen, if any.
pub fn copy_menu(ui: &mut egui::Ui) -> Option<CopyFormat> {
    let mut chosen = None;
    for (label, format) in [
        ("Copy as LaTeX", CopyFormat::Latex),
        ("Copy as plain text", CopyFormat::Unicode),
        ("Copy as code", CopyFormat::Code),
    ] {
        let text = egui::RichText::new(label).font(crate::theme::sans(14.0));
        if ui.button(text).clicked() {
            chosen = Some(format);
        }
    }
    chosen
}

/// Prose with `$ … $` maths: each maths part converted, the prose kept (and the
/// dollar signs too when `keep` is set).
fn mixed(s: &str, convert: fn(&str) -> String, keep: bool) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('$') {
        out.push_str(&rest[..i]);
        let (delim, after) = if rest[i..].starts_with("$$") {
            ("$$", &rest[i + 2..])
        } else {
            ("$", &rest[i + 1..])
        };
        let d = if keep { delim } else { "" };
        match after.find(delim) {
            Some(j) => {
                out.push_str(&format!("{d}{}{d}", convert(&after[..j])));
                rest = &after[j + delim.len()..];
            }
            None => {
                out.push_str(&format!("{d}{}", convert(after)));
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Converts maths source (LaTeX, plain or Unicode) to Unicode text.
pub fn maths_to_unicode(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut p = Conv { c: &chars, i: 0 };
    let s = p.seq(false);
    s.split('\n')
        .map(|l| l.trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

struct Conv<'a> {
    c: &'a [char],
    i: usize,
}

impl Conv<'_> {
    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }

    fn seq(&mut self, in_group: bool) -> String {
        let mut out = String::new();
        while let Some(ch) = self.peek() {
            match ch {
                '}' if in_group => break,
                '}' | '{' | '&' => {
                    self.i += 1;
                    if ch == '{' {
                        out.push_str(&self.seq(true));
                        if self.peek() == Some('}') {
                            self.i += 1;
                        }
                    }
                }
                '\\' => self.command(&mut out),
                '^' | '_' => {
                    self.i += 1;
                    let arg = self.argument();
                    out.push_str(&script(&arg, ch == '^'));
                }
                _ => {
                    out.push(ch);
                    self.i += 1;
                }
            }
        }
        out
    }

    /// One argument: a braced group or a single token.
    fn argument(&mut self) -> String {
        while self.peek() == Some(' ') {
            self.i += 1;
        }
        match self.peek() {
            Some('{') => {
                self.i += 1;
                let s = self.seq(true);
                if self.peek() == Some('}') {
                    self.i += 1;
                }
                s
            }
            Some('\\') => {
                let mut s = String::new();
                self.command(&mut s);
                s
            }
            Some(c) => {
                self.i += 1;
                c.to_string()
            }
            None => String::new(),
        }
    }

    fn command(&mut self, out: &mut String) {
        self.i += 1; // backslash
        let Some(first) = self.peek() else {
            return;
        };
        if !first.is_ascii_alphabetic() {
            self.i += 1;
            match first {
                ',' | ';' | ':' | ' ' => out.push('\u{2009}'),
                '\\' => out.push('\n'),
                '!' => {}
                c => out.push(c),
            }
            return;
        }
        let mut name = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_alphabetic() {
                name.push(c);
                self.i += 1;
            } else {
                break;
            }
        }
        match name.as_str() {
            "frac" | "dfrac" | "tfrac" => {
                let a = self.argument();
                let b = self.argument();
                out.push_str(&format!("{}/{}", wrap(&a), wrap(&b)));
            }
            "sqrt" => {
                let a = self.argument();
                out.push('√');
                out.push_str(&wrap(&a));
            }
            "left" | "right" | "displaystyle" | "textstyle" | "limits" | "nolimits" | "begin"
            | "end" => {
                // `\begin{x}` / `\end{x}` carry a name argument that is not maths.
                if name == "begin" || name == "end" {
                    let _ = self.argument();
                }
            }
            "text" | "mathrm" | "operatorname" | "mathbf" | "mathit" | "mathbb" | "mathcal" => {
                out.push_str(&self.argument());
            }
            "quad" | "qquad" => out.push('\u{2003}'),
            other => match lookup_latex(other) {
                Some(s) => out.push_str(s.unicode),
                None => out.push_str(other),
            },
        }
    }
}

fn wrap(s: &str) -> String {
    let simple = !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '.');
    if simple {
        s.to_string()
    } else {
        format!("({s})")
    }
}

fn raise(c: char) -> Option<char> {
    let i = "0123456789+-=()ni".find(c)?;
    "⁰¹²³⁴⁵⁶⁷⁸⁹⁺⁻⁼⁽⁾ⁿⁱ"
        .chars()
        .nth("0123456789+-=()ni"[..i].chars().count())
}

fn lower(c: char) -> Option<char> {
    let src = "0123456789+-=()aeoxhklmnpst";
    let i = src.find(c)?;
    "₀₁₂₃₄₅₆₇₈₉₊₋₌₍₎ₐₑₒₓₕₖₗₘₙₚₛₜ"
        .chars()
        .nth(src[..i].chars().count())
}

fn script(s: &str, sup: bool) -> String {
    let map = |c: char| if sup { raise(c) } else { lower(c) };
    let converted: Option<String> = s.chars().map(map).collect();
    match converted {
        Some(t) if !t.is_empty() => t,
        _ => format!("{}({s})", if sup { '^' } else { '_' }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integral_becomes_unicode_maths() {
        assert_eq!(maths_to_unicode("\\int_0^1 x^2 dx"), "∫₀¹ x² dx");
    }

    #[test]
    fn greek_and_fractions() {
        assert_eq!(maths_to_unicode("\\alpha + \\frac{a}{b+1}"), "α + a/(b+1)");
        assert_eq!(maths_to_unicode("\\sqrt{x}"), "√x");
    }

    #[test]
    fn mixed_text_keeps_prose() {
        assert_eq!(
            clipboard_text(
                r"The area is $\int_0^1 x^2 dx$.",
                false,
                CopyFormat::Unicode
            ),
            "The area is ∫₀¹ x² dx."
        );
    }

    #[test]
    fn latex_option_keeps_the_source() {
        assert_eq!(clipboard_text("$x^2$", false, CopyFormat::Latex), "$x^2$");
        // What the editor wrote in Unicode goes out as commands.
        assert_eq!(
            clipboard_text("$α ≤ 1$", false, CopyFormat::Latex),
            r"$\alpha \leq 1$"
        );
    }

    #[test]
    fn three_ways_to_copy() {
        let src = r"\int_{0}^{1} x^{2} \, dx";
        assert_eq!(clipboard_text(src, true, CopyFormat::Latex), src);
        assert_eq!(
            clipboard_text(src, true, CopyFormat::Unicode),
            "∫₀¹ x² \u{2009} dx"
        );
        assert_eq!(
            clipboard_text(src, true, CopyFormat::Code),
            "int(x**2, x, 0, 1)"
        );
        assert_eq!(
            clipboard_text("Area $x^{2}$ here", false, CopyFormat::Code),
            "Area x**2 here"
        );
    }
}
