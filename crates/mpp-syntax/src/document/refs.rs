//! Reference lists: `References` blocks, `References from [[refs.mpp]]` and the
//! entries inside them, written `[@key] Author | Title | Year | link`.

use super::ast::Reference;
use crate::span::Span;

/// True for the characters a citation key may use.
pub(crate) fn is_key_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | ':' | '.' | '/')
}

/// What the line after `References` is: its own entries follow, or a file holds them.
pub(crate) enum Head {
    Entries,
    From { path: String, path_span: Span },
}

/// Reads a `References` line (without its first-line indentation). `ts` is where
/// `t` starts in the file.
pub(crate) fn head(t: &str, ts: usize) -> Option<Head> {
    let rest = t.trim_end().strip_prefix("References")?;
    let rest_trim = rest.trim();
    if rest_trim.is_empty() || rest_trim == ":" {
        return Some(Head::Entries);
    }
    let after = rest.trim_start().strip_prefix("from")?;
    let after_ws = after.trim_start();
    let inner = after_ws.strip_prefix("[[")?;
    let close = inner.find("]]")?;
    if !inner[close + 2..].trim().trim_end_matches('.').is_empty() {
        return None;
    }
    let path = inner[..close].trim();
    if path.is_empty() {
        return None;
    }
    let start =
        ts + (t.len() - inner.len()) + (inner[..close].len() - inner[..close].trim_start().len());
    Some(Head::From {
        path: path.to_string(),
        path_span: Span::new(start, start + path.len()),
    })
}

/// Reads one entry line. `at` is where the line's text starts in the file.
pub(crate) fn entry(text: &str, at: usize) -> Result<Reference, String> {
    let t = text.trim();
    let rest = t
        .strip_prefix("[@")
        .ok_or("an entry starts with `[@key]`")?;
    let close = rest.find(']').ok_or("the key needs a closing `]`")?;
    let key = rest[..close].trim();
    if key.is_empty() || !key.chars().all(is_key_char) {
        return Err("a key uses letters, digits and `_ - : . /`".to_string());
    }
    let after = rest[close + 1..].trim_start();
    let after = after.strip_prefix(':').unwrap_or(after);
    let fields: Vec<&str> = after.split('|').map(str::trim).collect();
    let (author, title) = match fields.as_slice() {
        [a, t, ..] if !a.is_empty() && !t.is_empty() => (*a, *t),
        _ => return Err("an entry needs `Author | Title`, then optionally `| Year | link`".into()),
    };
    let is_link = |s: &str| s.starts_with("http://") || s.starts_with("https://");
    let mut year = None;
    let mut link = None;
    for f in fields.iter().skip(2).filter(|f| !f.is_empty()) {
        if is_link(f) {
            link = Some((*f).to_string());
        } else if year.is_none() {
            year = Some((*f).to_string());
        }
    }
    let lead = text.len() - text.trim_start().len();
    Ok(Reference {
        key: key.to_string(),
        author: author.to_string(),
        title: title.to_string(),
        year,
        link,
        span: Span::new(at + lead, at + lead + t.len()),
    })
}

/// Reads every entry in the text of a references file (a `.mpp` file whose lines
/// `[@key] Author | Title | Year | link` are the entries; every other line is
/// ignored). Spans index `src`.
pub fn parse_reference_entries(src: &str) -> Vec<Reference> {
    let mut out = Vec::new();
    let mut pos = 0;
    for line in src.split('\n') {
        let text = line.trim_end_matches('\r');
        if text.trim_start().starts_with("[@") {
            if let Ok(r) = entry(text, pos) {
                out.push(r);
            }
        }
        pos += line.len() + 1;
    }
    out
}

/// The name a citation uses in author-year style: the first author's surname,
/// "A and B" for two authors, "A et al." for more.
pub fn cite_name(author: &str) -> String {
    let people: Vec<&str> = author
        .split(" and ")
        .flat_map(|p| p.split(';'))
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    let surname = |p: &str| -> String {
        match p.split_once(',') {
            Some((last, _)) => last.trim().to_string(),
            None => p.split_whitespace().last().unwrap_or(p).to_string(),
        }
    };
    match people.as_slice() {
        [] => author.to_string(),
        [one] => surname(one),
        [a, b] => format!("{} and {}", surname(a), surname(b)),
        [a, ..] => format!("{} et al.", surname(a)),
    }
}
