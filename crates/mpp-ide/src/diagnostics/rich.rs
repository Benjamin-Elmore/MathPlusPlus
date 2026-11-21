//! Problem messages as words with typeset maths in them.
//!
//! The parser and the evaluator write their messages with source fragments in
//! backticks (`` `\frac` ``, `` `}` ``, `` `θ` ``). Those fragments are raw text,
//! which the equation view never shows, so every message is turned into a
//! [`Rich`] line: plain English words, and maths that is typeset like the page.

use egui::{Pos2, Rect, Response, Sense, Ui, Vec2};
use mpp_render::{FontId, Theme};

use crate::paint::{c32, paint_display_list, paint_text};

/// One piece of a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seg {
    /// Words, set in Latin Modern Roman.
    Text(String),
    /// Maths (LaTeX or Unicode), typeset inline.
    Math(String),
    /// Words in italics (file names, box labels).
    Italic(String),
}

/// A message: words and maths, never source text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rich(pub Vec<Seg>);

impl Rich {
    /// A message of plain words.
    pub fn text(s: impl Into<String>) -> Rich {
        Rich(vec![Seg::Text(s.into())])
    }

    /// Adds words.
    pub fn t(mut self, s: impl Into<String>) -> Rich {
        let s = s.into();
        if s.is_empty() {
            return self;
        }
        if let Some(Seg::Text(last)) = self.0.last_mut() {
            last.push_str(&s);
        } else {
            self.0.push(Seg::Text(s));
        }
        self
    }

    /// Adds maths.
    pub fn m(mut self, latex: impl Into<String>) -> Rich {
        self.0.push(Seg::Math(latex.into()));
        self
    }

    /// Adds words in italics.
    pub fn i(mut self, s: impl Into<String>) -> Rich {
        self.0.push(Seg::Italic(s.into()));
        self
    }

    /// Adds another message.
    pub fn and(mut self, other: Rich) -> Rich {
        for s in other.0 {
            self = match s {
                Seg::Text(t) => self.t(t),
                other => {
                    self.0.push(other);
                    self
                }
            };
        }
        self
    }

    /// True when there is nothing to show.
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|s| match s {
            Seg::Text(t) | Seg::Math(t) | Seg::Italic(t) => t.trim().is_empty(),
        })
    }

    /// The message as Unicode text (maths converted to Unicode symbols), for
    /// tests, screen readers and the status bar. Never contains LaTeX.
    pub fn plain(&self) -> String {
        let mut out = String::new();
        for s in &self.0 {
            match s {
                Seg::Text(t) | Seg::Italic(t) => out.push_str(t),
                Seg::Math(m) => out.push_str(&crate::copy::maths_to_unicode(m)),
            }
        }
        out
    }
}

/// Words for a source fragment that is not maths a reader would recognise.
fn fragment_words(code: &str) -> Option<&'static str> {
    Some(match code {
        "$$" => "maths block",
        "$" => "inline maths",
        "[[" => "link",
        "]]" => "end of the link",
        "{" => "group",
        "}" => "end of the group",
        "\\\\" => "new row",
        "&" => "column break",
        "\\frac" | "\\dfrac" | "\\tfrac" => "fraction",
        "\\sqrt" => "root",
        "\\int" | "\\iint" | "\\iiint" | "\\oint" => "integral",
        "\\sum" => "sum",
        "\\prod" => "product",
        "\\lim" => "limit",
        "\\left" => "opening bracket",
        "\\right" => "closing bracket",
        "\\end" => "end of the environment",
        "\\begin" => "environment",
        "\\label" => "label",
        "\\operatorname" => "function name",
        "\\text" | "\\mathrm" => "text",
        "\\overset" => "symbol over a sign",
        "_" => "subscript",
        "^" => "superscript",
        ". " => "full stop",
        _ => return None,
    })
}

/// Turns a backticked fragment into a message piece.
fn fragment(code: &str) -> Seg {
    if let Some(w) = fragment_words(code) {
        return Seg::Text(w.to_string());
    }
    if let Some(env) = code
        .strip_prefix("\\end{")
        .and_then(|r| r.strip_suffix('}'))
    {
        return Seg::Text(format!("end of the {env}"));
    }
    if code.starts_with('\\') && mpp_syntax::symbols::lookup_latex(code).is_none() {
        // An unknown command: name it in words, without the backslash.
        return Seg::Italic(code.trim_start_matches('\\').to_string());
    }
    if code.trim().is_empty() {
        return Seg::Text("a space".into());
    }
    // A lone bracket is not a formula: set it as a typeset character.
    if code.chars().count() == 1 && "()[]|⟨⟩".contains(code) {
        return Seg::Text(code.to_string());
    }
    Seg::Math(code.to_string())
}

/// Rewrites a parser or evaluator message (with backticked source fragments)
/// as a sentence of words and typeset maths. Line numbers are dropped.
pub fn from_message(msg: &str) -> Rich {
    let msg = strip_line_numbers(msg);
    let mut out = Rich::default();
    let mut rest = msg.as_str();
    while let Some(open) = rest.find('`') {
        out = prose(out, &rest[..open]);
        let after = &rest[open + 1..];
        match after.find('`') {
            // A fragment that is itself a sentence with maths (`Assume $x > 0$.`).
            Some(close) if after[..close].matches('$').count() >= 2 => {
                out = prose(out, &after[..close]);
                rest = &after[close + 1..];
            }
            Some(close) => {
                let code = &after[..close];
                match fragment(code) {
                    Seg::Text(t) => out = out.t(t),
                    other => out.0.push(other),
                }
                rest = &after[close + 1..];
            }
            None => {
                out = prose(out, after);
                rest = "";
            }
        }
    }
    out = prose(out, rest);
    sentence(out)
}

/// Words with inline maths between dollar signs (`Assume $n \neq -1$.`), the way
/// the checker and the assumption rules write their messages.
fn prose(mut out: Rich, text: &str) -> Rich {
    let mut rest = text;
    while let Some(open) = rest.find('$') {
        let after = rest[open..].trim_start_matches('$');
        let Some(close) = after.find('$') else {
            break;
        };
        out = words(out, &rest[..open]);
        let maths = after[..close].trim();
        if !maths.is_empty() && maths != "…" {
            out.0.push(Seg::Math(maths.to_string()));
        }
        rest = after[close..].trim_start_matches('$');
    }
    words(out, rest)
}

/// Plain words, where a word written in text maths (`x^2`, `e^(−iωx)`, `V_C`,
/// `F{f}`) becomes typeset maths.
fn words(mut out: Rich, text: &str) -> Rich {
    for (i, word) in text.split(' ').enumerate() {
        if i > 0 {
            out = out.t(" ");
        }
        let (lead, core, trail) = trim_word(word);
        if core.contains(['^', '_', '{', '}', '\\']) {
            out = out.t(lead);
            // A word with a command in it is LaTeX already.
            let latex = if core.contains('\\') {
                core.to_string()
            } else {
                text_maths_latex(core)
            };
            out.0.push(Seg::Math(latex));
            out = out.t(trail);
        } else {
            out = out.t(word);
        }
    }
    out
}

/// Splits sentence punctuation (and brackets that do not belong to the maths) off a
/// word: `(|aₖ|^(1/k)` → `(`, `|aₖ|^(1/k)`, ``.
fn trim_word(word: &str) -> (&str, &str, &str) {
    let mut core = word;
    let mut lead = 0;
    let mut trail = 0;
    loop {
        let opens = core.matches('(').count();
        let closes = core.matches(')').count();
        if let Some(rest) = core.strip_suffix([',', '.', ';', ':', '?', '!']) {
            core = rest;
            trail += 1;
        } else if closes > opens && core.ends_with(')') {
            core = &core[..core.len() - 1];
            trail += 1;
        } else if opens > closes && core.starts_with('(') {
            core = &core[1..];
            lead += 1;
        } else {
            break;
        }
    }
    (&word[..lead], core, &word[word.len() - trail..])
}

/// Text maths as LaTeX: `^(…)` and `_(…)` group their contents, a run of letters or
/// digits after `^` or `_` is the script, braces are literal braces.
pub(crate) fn text_maths_latex(s: &str) -> String {
    let cs: Vec<char> = s.chars().collect();
    let mut o = String::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        match c {
            '^' | '_' => {
                o.push(c);
                i += 1;
                if cs.get(i) == Some(&'(') {
                    let mut depth = 0;
                    let mut j = i;
                    while j < cs.len() {
                        match cs[j] {
                            '(' => depth += 1,
                            ')' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                        j += 1;
                    }
                    let inner: String = cs[i + 1..j.min(cs.len())].iter().collect();
                    o.push('{');
                    o.push_str(&text_maths_latex(&inner));
                    o.push('}');
                    i = (j + 1).min(cs.len());
                } else {
                    let mut j = i;
                    while j < cs.len() && (cs[j].is_alphanumeric() || cs[j] == '−' && j == i) {
                        j += 1;
                    }
                    let run: String = cs[i..j].iter().collect();
                    o.push('{');
                    o.push_str(&run);
                    o.push('}');
                    i = j;
                }
            }
            '{' => {
                o.push_str(r"\{");
                i += 1;
            }
            '}' => {
                o.push_str(r"\}");
                i += 1;
            }
            c => {
                o.push(c);
                i += 1;
            }
        }
    }
    o
}

/// Starts with a capital letter and ends with a full stop.
pub fn sentence(mut r: Rich) -> Rich {
    if let Some(Seg::Text(t)) = r.0.first_mut() {
        let trimmed = t.trim_start().to_string();
        let mut cs = trimmed.chars();
        if let Some(c) = cs.next() {
            *t = c.to_uppercase().chain(cs).collect();
        }
    }
    match r.0.last_mut() {
        Some(Seg::Text(t)) => {
            let trimmed = t.trim_end().to_string();
            *t = trimmed;
            if !t.ends_with(['.', '?', '!']) {
                t.push('.');
            }
        }
        _ => r = r.t("."),
    }
    r
}

/// Removes "on line 3" / "line 3" from a message: positions are never shown as
/// line numbers.
fn strip_line_numbers(msg: &str) -> String {
    let mut out = String::with_capacity(msg.len());
    let words: Vec<&str> = msg.split(' ').collect();
    let mut i = 0;
    while i < words.len() {
        let w = words[i];
        let next_is_number = words
            .get(i + 1)
            .map(|n| n.trim_end_matches([',', '.', ')']))
            .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        if (w == "line" || w == "lines") && next_is_number {
            let tail = words[i + 1]
                .trim_start_matches(|c: char| c.is_ascii_digit())
                .to_string();
            // "on line 3" becomes "earlier"; a bare "line 3" becomes "elsewhere".
            if out.ends_with("on ") {
                out.truncate(out.len() - 3);
                out.push_str("earlier");
            } else {
                out.push_str("elsewhere");
            }
            out.push_str(&tail);
            out.push(' ');
            i += 2;
            continue;
        }
        out.push_str(w);
        out.push(' ');
        i += 1;
    }
    out.trim_end().to_string()
}

/// One laid-out piece of a message.
enum Atom {
    Word {
        text: String,
        font: FontId,
        width: f32,
    },
    Math(std::sync::Arc<mpp_render::DisplayList>),
}

/// The pieces of a message laid out in lines no wider than `max_width`.
struct Laid {
    /// (x, line, atom)
    items: Vec<(f32, usize, Atom)>,
    lines: Vec<(f32, f32)>,
    width: f32,
}

fn lay_out(r: &Rich, size: f32, theme: Theme, max_width: f32) -> Laid {
    let space = crate::page::text_width(FontId::Text, " ") * size;
    // Each atom with whether a space comes before it.
    let mut atoms: Vec<(Atom, bool)> = vec![];
    let mut pending_space = false;
    for s in &r.0 {
        match s {
            Seg::Text(t) | Seg::Italic(t) => {
                let font = if matches!(s, Seg::Italic(_)) {
                    FontId::TextItalic
                } else {
                    FontId::Text
                };
                let mut space_before = pending_space || t.starts_with(' ');
                for w in t.split(' ') {
                    if w.is_empty() {
                        space_before = true;
                        continue;
                    }
                    let width = crate::page::text_width(font, w) * size;
                    atoms.push((
                        Atom::Word {
                            text: w.to_string(),
                            font,
                            width,
                        },
                        space_before,
                    ));
                    space_before = true;
                }
                pending_space = t.ends_with(' ');
            }
            Seg::Math(m) => {
                let dl = crate::typeset::layout(m, size, theme);
                atoms.push((Atom::Math(dl), pending_space));
                pending_space = false;
            }
        }
    }
    let mut items = vec![];
    let mut lines: Vec<(f32, f32)> = vec![];
    let (mut x, mut line, mut asc, mut desc) = (0.0f32, 0usize, size * 0.75, size * 0.25);
    let mut width = 0.0f32;
    for (atom, spaced) in atoms {
        let (w, a, d) = match &atom {
            Atom::Word { width, .. } => (*width, size * 0.75, size * 0.25),
            Atom::Math(dl) => (dl.width, dl.height, dl.depth),
        };
        let gap = if spaced && x > 0.0 { space } else { 0.0 };
        if x > 0.0 && spaced && x + gap + w > max_width {
            lines.push((asc, desc));
            line += 1;
            x = 0.0;
            asc = size * 0.75;
            desc = size * 0.25;
        } else {
            x += gap;
        }
        asc = asc.max(a);
        desc = desc.max(d);
        items.push((x, line, atom));
        x += w;
        width = width.max(x);
    }
    lines.push((asc, desc));
    Laid {
        items,
        lines,
        width,
    }
}

/// The size a message takes when wrapped at `max_width`.
pub fn measure(r: &Rich, size: f32, theme: Theme, max_width: f32) -> Vec2 {
    let l = lay_out(r, size, theme, max_width);
    let h: f32 = l.lines.iter().map(|(a, d)| a + d + size * 0.15).sum();
    Vec2::new(l.width, h)
}

/// Paints a message with its top-left corner at `at`; returns the rectangle used.
pub fn paint(
    painter: &egui::Painter,
    r: &Rich,
    at: Pos2,
    size: f32,
    color: egui::Color32,
    theme: Theme,
    max_width: f32,
) -> Rect {
    let l = lay_out(r, size, theme, max_width);
    let mut tops = vec![];
    let mut y = at.y;
    for (a, d) in &l.lines {
        tops.push((y, *a));
        y += a + d + size * 0.15;
    }
    for (x, line, atom) in &l.items {
        let (top, asc) = tops.get(*line).copied().unwrap_or((at.y, size));
        let baseline = top + asc;
        match atom {
            Atom::Word { text, font, .. } => {
                paint_text(
                    painter,
                    text,
                    Pos2::new(at.x + x, baseline),
                    size,
                    *font,
                    color,
                );
            }
            Atom::Math(dl) => {
                paint_display_list(painter, dl, Pos2::new(at.x + x, baseline - dl.height));
            }
        }
    }
    Rect::from_min_max(at, Pos2::new(at.x + l.width, y))
}

/// Shows a message in a UI, wrapped to the available width.
pub fn show(ui: &mut Ui, r: &Rich, size: f32, color: egui::Color32, theme: Theme) -> Response {
    let max = ui.available_width().max(80.0);
    let s = measure(r, size, theme, max);
    let (resp, painter) = ui.allocate_painter(s.max(Vec2::splat(1.0)), Sense::click());
    paint(&painter, r, resp.rect.min, size, color, theme, max);
    resp
}

/// The colour of a message's words in a theme (maths keeps its own ink).
pub fn ink(theme: &Theme) -> egui::Color32 {
    c32(theme.text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backticks_become_words_or_maths() {
        let r = from_message("expected the denominator of `\\frac` after `{`");
        assert_eq!(
            r.plain(),
            "Expected the denominator of fraction after group."
        );
        let r = from_message("`θ` has no value, so there is no number to give");
        assert!(
            matches!(r.0.first(), Some(Seg::Math(m)) if m == "θ"),
            "{r:?}"
        );
        assert_eq!(r.plain(), "θ has no value, so there is no number to give.");
    }

    #[test]
    fn line_numbers_are_never_kept() {
        let r = from_message("`g` is a constant (≡) defined on line 3 and cannot be redefined");
        let p = r.plain();
        assert!(!p.contains('3'), "{p}");
        assert!(p.contains("defined earlier"), "{p}");
    }

    #[test]
    fn unknown_commands_are_named_without_a_backslash() {
        let r = from_message("unknown command `\\foo`");
        assert!(!r.plain().contains('\\'), "{}", r.plain());
    }
}
