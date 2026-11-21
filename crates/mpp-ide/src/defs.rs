//! What names mean: definitions, values and where they were made.

use mpp_eval::{Binding, Evaluated, LineKind, VarKind};
use mpp_syntax::symbols::{lookup_char, lookup_name};
use mpp_syntax::Span;

/// A name in scope, ready to show typeset.
#[derive(Debug, Clone, PartialEq)]
pub struct DefInfo {
    pub name: String,
    /// The definition as LaTeX (`E := \frac12 m v^2`).
    pub definition: String,
    /// The current value as LaTeX, when it is known.
    pub value: Option<String>,
    /// Where it was defined: the file (when imported) and the span in it.
    pub file: Option<String>,
    pub span: Span,
}

/// A name as the page sets it: a symbol by its command, a word such as
/// `velocity` upright, a letter in italic.
pub fn name_latex(name: &str) -> String {
    match lookup_name(name) {
        Some(s) if s.unicode == name => s.latex.to_string(),
        _ => mpp_core::Expr::sym(name).to_latex(),
    }
}

/// The definition and value of one name.
pub fn describe(ev: &Evaluated, name: &str) -> Option<DefInfo> {
    let binding = ev.bindings.get(name)?;
    if mpp_eval::random::is_random_key(name) {
        // `X \sim \mathrm{Uniform}[0, 1]`: shown as the declaration, under X.
        let Binding::Variable { value, .. } = &**binding else {
            return None;
        };
        let (shown, latex) = mpp_eval::random::declaration_latex(name, value)?;
        let origin = binding.origin();
        return Some(DefInfo {
            name: shown,
            definition: latex,
            value: None,
            file: origin
                .path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .or_else(|| origin.file.as_ref().map(|f| f.to_string())),
            span: origin.span,
        });
    }
    // The evaluator's hidden keys are never shown.
    if mpp_eval::is_internal_name(name) {
        return None;
    }
    let origin = binding.origin();
    let n = name_latex(name);
    let (definition, own_value) = match &**binding {
        Binding::Variable { kind, value, .. } => {
            let op = if *kind == VarKind::Constant {
                "\\equiv"
            } else {
                ":="
            };
            // A decimal is shown as it was typed (`g \equiv 9.81`, not 981/100).
            let shown = origin
                .decimal
                .as_deref()
                .map_or_else(|| value.to_latex(), str::to_string);
            (format!("{n} {op} {shown}"), Some(shown))
        }
        Binding::Function { params, body, .. } => (
            format!("{n}({}) := {}", params.join(", "), body.to_latex()),
            None,
        ),
        Binding::Sequence { .. } => (binding.describe(), None),
        Binding::Imported { from, .. } => (format!("{n} \\text{{ from {from}}}"), None),
    };
    // A value computed in this document beats the stored expression.
    let mut value = own_value;
    if origin.file.is_none() {
        let found = ev
            .blocks
            .iter()
            .flat_map(|b| b.values.iter())
            .find(|v| {
                v.span == origin.span
                    && matches!(&v.kind, LineKind::Definition { name: dn, .. } if dn == name)
            })
            .and_then(|v| v.latex.clone());
        if found.is_some() {
            value = found;
        }
    }
    Some(DefInfo {
        name: name.to_string(),
        definition,
        value,
        // The resolved path opens the file; the shown name is the fallback.
        file: origin
            .path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .or_else(|| origin.file.as_ref().map(|f| f.to_string())),
        span: origin.span,
    })
}

/// Every name in scope.
pub fn all(ev: &Evaluated) -> Vec<DefInfo> {
    ev.bindings.keys().filter_map(|n| describe(ev, n)).collect()
}

/// The typeset line of the variables panel: `name = value`, or `f(x) = body`.
pub fn summary_latex(d: &DefInfo) -> String {
    let n = name_latex(&d.name);
    match (&d.value, d.definition.split_once(":=")) {
        (_, Some((head, body))) if head.contains('(') => {
            format!("{} = {}", head.trim(), body.trim())
        }
        (Some(v), _) => format!("{n} = {v}"),
        (None, Some((_, body))) => format!("{n} = {}", body.trim()),
        _ => d.definition.clone(),
    }
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '\''
}

/// Candidate names for the identifier around a source offset, longest first.
pub fn name_candidates(src: &str, offset: usize) -> Vec<String> {
    let offset = offset.min(src.len());
    let mut offset = offset;
    while offset > 0 && !src.is_char_boundary(offset) {
        offset -= 1;
    }
    let mut start = offset;
    for (i, c) in src[..offset].char_indices().rev() {
        if is_ident(c) {
            start = i;
        } else {
            break;
        }
    }
    let mut end = offset;
    for (i, c) in src[offset..].char_indices() {
        if is_ident(c) {
            end = offset + i + c.len_utf8();
        } else {
            break;
        }
    }
    if start == end {
        return vec![];
    }
    let word = src[start..end].to_string();
    let mut out = vec![];
    let mut push = |s: String| {
        if !s.is_empty() && !out.contains(&s) {
            out.push(s);
        }
    };
    push(word.clone());
    let base = word.split('_').next().unwrap_or("").to_string();
    // The letters may be a spelled-out Greek name, or a Greek character.
    for w in [word.clone(), base.clone()] {
        if let Some(s) = lookup_name(&w) {
            push(s.unicode.to_string());
        }
    }
    if let Some(c) = base.chars().next().filter(|_| base.chars().count() == 1) {
        if let Some(s) = lookup_char(c) {
            push(s.name.to_string());
        }
    }
    push(base);
    out
}

/// The definition of the name at a source offset, if it has one.
pub fn definition_at(ev: &Evaluated, src: &str, offset: usize) -> Option<DefInfo> {
    name_candidates(src, offset)
        .into_iter()
        .find_map(|n| describe(ev, &n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mpp_eval::Session;

    #[test]
    fn describes_a_variable() {
        let mut s = Session::new();
        let ev = s.run_str("$$ a := 3 \\\\ b := a + 2 $$\n");
        let d = describe(ev, "b").expect("b is defined");
        assert!(d.definition.contains(":="));
        assert!(d.value.is_some());
    }

    #[test]
    fn a_decimal_is_described_as_typed() {
        let mut s = Session::new();
        let ev = s.run_str("$$ g := 9.81 $$\n");
        let d = describe(ev, "g").expect("g is defined");
        assert_eq!(d.definition, "g := 9.81");
        assert_eq!(d.value.as_deref(), Some("9.81"));
    }

    #[test]
    fn a_word_is_set_upright() {
        assert_eq!(name_latex("velocity"), r"\mathrm{velocity}");
        assert_eq!(name_latex("x"), "x");
    }

    #[test]
    fn finds_the_name_under_an_offset() {
        let src = "x := alpha_1 + 2";
        assert_eq!(name_candidates(src, 8)[0], "alpha_1");
        assert!(name_candidates("  ", 1).is_empty());
    }
}
