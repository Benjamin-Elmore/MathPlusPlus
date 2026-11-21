//! Search across the project's files, including by symbol (type or paste `∫`
//! or `\int` to find every integral), and find-all-uses of a name. Results
//! carry LaTeX to typeset, never the raw line.

use std::path::{Path, PathBuf};

use mpp_syntax::document::{parse_document, Block, BlockKind, Inline, InlineKind};
use mpp_syntax::symbols::{lookup_char, lookup_latex, lookup_name};
use mpp_syntax::Span;

use super::outline::inlines_latex;

/// The most results one search returns.
pub const MAX_HITS: usize = 300;

/// One result: a block of a file.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub file: PathBuf,
    /// The block's place in the file (opening the hit scrolls here).
    pub span: Span,
    /// What to typeset for it.
    pub latex: String,
}

/// What a query looks for in the source of a block.
struct Needles {
    /// Plain pieces, lower case.
    plain: Vec<String>,
    /// LaTeX commands (`\int`), matched only when not followed by a letter.
    commands: Vec<String>,
}

fn needles(query: &str) -> Needles {
    let q = query.trim().to_lowercase();
    let mut n = Needles {
        plain: vec![],
        commands: vec![],
    };
    if q.is_empty() {
        return n;
    }
    let mut chars = q.chars();
    let single = match (chars.next(), chars.next()) {
        (Some(c), None) => lookup_char(c),
        _ => None,
    };
    let by_name = if q.starts_with('\\') {
        lookup_latex(&q)
    } else {
        None
    };
    for s in [single, by_name].into_iter().flatten() {
        n.plain.push(s.unicode.to_lowercase());
        if s.latex.starts_with('\\') && s.latex.chars().skip(1).all(char::is_alphabetic) {
            n.commands.push(s.latex.to_lowercase());
        }
    }
    if let Some(s) = lookup_name(&q) {
        if s.unicode != q {
            n.plain.push(s.unicode.to_lowercase());
        }
    }
    if n.commands.is_empty() && q.starts_with('\\') && q.chars().skip(1).all(char::is_alphabetic) {
        n.commands.push(q.clone());
    }
    if n.commands.is_empty() {
        n.plain.push(q);
    }
    n
}

fn has_command(hay: &str, cmd: &str) -> bool {
    let mut from = 0;
    while let Some(p) = hay[from..].find(cmd) {
        let end = from + p + cmd.len();
        if !hay[end..].chars().next().is_some_and(char::is_alphabetic) {
            return true;
        }
        from = end;
    }
    false
}

impl Needles {
    fn matches(&self, hay: &str) -> bool {
        let h = hay.to_lowercase();
        self.plain.iter().any(|p| h.contains(p.as_str()))
            || self.commands.iter().any(|c| has_command(&h, c))
    }

    fn is_empty(&self) -> bool {
        self.plain.is_empty() && self.commands.is_empty()
    }
}

/// The LaTeX to typeset for a whole block.
pub fn block_latex(b: &Block, src: &str) -> String {
    let fallback = |s: &str| {
        let clean: String = s
            .chars()
            .filter(|c| !c.is_control())
            .take(90)
            .map(|c| if "\\{}$%&#_^~".contains(c) { ' ' } else { c })
            .collect();
        format!("\\text{{{}}}", clean.trim())
    };
    let latex = match &b.kind {
        BlockKind::Heading { content, .. } | BlockKind::Paragraph(content) => {
            inlines_latex(content, src)
        }
        BlockKind::Let(l) => inlines_latex(&l.content, src),
        BlockKind::DisplayMath(m) => src
            .get(m.raw.start..m.raw.end)
            .map(|s| s.trim().replace('\n', " "))
            .unwrap_or_default(),
        _ => String::new(),
    };
    if latex.is_empty() || latex.len() > 400 {
        fallback(src.get(b.span.start..b.span.end).unwrap_or(""))
    } else {
        latex
    }
}

fn math_sources<'a>(b: &Block, src: &'a str) -> Vec<&'a str> {
    fn from_inlines<'a>(v: &[Inline], src: &'a str, out: &mut Vec<&'a str>) {
        for i in v {
            match &i.kind {
                InlineKind::Math(m) => out.extend(src.get(m.raw.start..m.raw.end)),
                InlineKind::Emphasis(x) | InlineKind::Strong(x) => from_inlines(x, src, out),
                _ => {}
            }
        }
    }
    let mut out = vec![];
    match &b.kind {
        BlockKind::DisplayMath(m) => out.extend(src.get(m.raw.start..m.raw.end)),
        BlockKind::Let(l) => from_inlines(&l.content, src, &mut out),
        BlockKind::Paragraph(c) | BlockKind::Heading { content: c, .. } => {
            from_inlines(c, src, &mut out)
        }
        _ => {}
    }
    out
}

/// Searches every file for `query` (words or a symbol).
pub fn search_files(
    files: &[PathBuf],
    read: &dyn Fn(&Path) -> Option<String>,
    query: &str,
) -> Vec<Hit> {
    let n = needles(query);
    let mut hits = vec![];
    if n.is_empty() {
        return hits;
    }
    for f in files {
        let Some(src) = read(f) else {
            continue;
        };
        let doc = parse_document(&src);
        doc.walk(&mut |b| {
            if hits.len() >= MAX_HITS {
                return;
            }
            if matches!(b.kind, BlockKind::Rule | BlockKind::Directive(_)) {
                return;
            }
            let Some(text) = src.get(b.span.start..b.span.end) else {
                return;
            };
            if n.matches(text) {
                hits.push(Hit {
                    file: f.clone(),
                    span: b.span,
                    latex: block_latex(b, &src),
                });
            }
        });
    }
    hits
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '\''
}

/// True when `name` appears in `hay` as a whole identifier.
pub fn has_word(hay: &str, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut from = 0;
    while let Some(p) = hay[from..].find(name) {
        let s = from + p;
        let e = s + name.len();
        let before = hay[..s].chars().next_back();
        let after = hay[e..].chars().next();
        let backslash = before == Some('\\');
        if !before.is_some_and(is_ident) && !after.is_some_and(is_ident) && !backslash {
            return true;
        }
        from = e;
    }
    false
}

/// Every maths block, in any file, that uses `name`.
pub fn find_uses(
    files: &[PathBuf],
    read: &dyn Fn(&Path) -> Option<String>,
    name: &str,
) -> Vec<Hit> {
    let mut hits = vec![];
    for f in files {
        let Some(src) = read(f) else {
            continue;
        };
        let doc = parse_document(&src);
        doc.walk(&mut |b| {
            if hits.len() < MAX_HITS && math_sources(b, &src).iter().any(|m| has_word(m, name)) {
                hits.push(Hit {
                    file: f.clone(),
                    span: b.span,
                    latex: block_latex(b, &src),
                });
            }
        });
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(files: &[(&str, &str)]) -> (Vec<PathBuf>, impl Fn(&Path) -> Option<String>) {
        let owned: Vec<(PathBuf, String)> = files
            .iter()
            .map(|(a, b)| (PathBuf::from(a), (*b).to_string()))
            .collect();
        let paths = owned.iter().map(|(p, _)| p.clone()).collect();
        (paths, move |p: &Path| {
            owned.iter().find(|(q, _)| q == p).map(|(_, t)| t.clone())
        })
    }

    #[test]
    fn finds_words_and_symbols() {
        let (paths, read) = fake(&[
            ("a.mpp", "# Area\n\n$$ A = \\int_0^1 x \\, dx $$\n"),
            ("b.mpp", "Plain words about forces.\n\n$$ F = m a $$\n"),
        ]);
        assert_eq!(search_files(&paths, &read, "forces").len(), 1);
        let by_char = search_files(&paths, &read, "\u{222b}");
        assert_eq!(by_char.len(), 1);
        assert_eq!(by_char[0].file, PathBuf::from("a.mpp"));
        assert!(by_char[0].latex.contains("int"));
        assert_eq!(search_files(&paths, &read, "\\int").len(), 1);
        assert!(search_files(&paths, &read, "").is_empty());
    }

    #[test]
    fn command_search_does_not_match_longer_commands() {
        assert!(has_command("a \\int b", "\\int"));
        assert!(!has_command("a \\intercal b", "\\int"));
    }

    #[test]
    fn uses_match_whole_names_in_maths_only() {
        let (paths, read) = fake(&[(
            "a.mpp",
            "The mass m is large.\n\n$$ E = m v^2 $$\n\n$$ mm = 2 $$\n",
        )]);
        let u = find_uses(&paths, &read, "m");
        assert_eq!(u.len(), 1, "{u:?}");
    }
}
