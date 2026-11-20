//! Turning syntax names into the plain keys the environment uses.
//!
//! A key is the letter as Unicode (`x`, `θ`, `𝐯` for a bold v), a combining mark for
//! an accent (`x̂`), then `_` and the subscript (`v_0`, `x_max`, `a_n`). The core
//! printer splits keys at the first `_` again when it typesets them.

use mpp_syntax::math::{Accent, BinOp, Constant, MulStyle, Name, Node, NodeKind, UnaryOp};
use mpp_syntax::symbols::{styled, Font};

/// The key of a name with all its decorations, or `None` when the subscript is
/// too complicated to be part of a name.
pub fn name_key(name: &Name) -> Option<String> {
    let mut key = base_key(name);
    if let Some(sub) = &name.sub {
        key.push('_');
        key.push_str(&sub_text(sub)?);
    }
    Some(key)
}

/// The key of a name without its subscript (`a` for `a_{n+1}`).
pub fn base_key(name: &Name) -> String {
    let mut key = String::new();
    let mut chars = name.base.chars();
    match (name.font, chars.next(), chars.next()) {
        (Some(font), Some(c), None) if font != Font::Roman => {
            key.push(styled(font, c).unwrap_or(c));
        }
        _ => key.push_str(&name.base),
    }
    if let Some(accent) = name.accent {
        key.push(combining(accent));
    }
    key
}

/// The key when `node` is a plain name (including `e` and `i` used as names).
pub fn node_key(node: &Node) -> Option<String> {
    node.as_name().and_then(|n| name_key(&n))
}

/// The key of `base` with the subscript `sub` (`v` and `0` give `v_0`).
pub fn subscripted(base: &str, sub: &Node) -> Option<String> {
    Some(format!("{base}_{}", sub_text(sub)?))
}

fn combining(accent: Accent) -> char {
    match accent {
        Accent::Hat | Accent::WideHat => '\u{0302}',
        Accent::Bar => '\u{0304}',
        Accent::Overline => '\u{0305}',
        Accent::Vec => '\u{20D7}',
        Accent::Dot => '\u{0307}',
        Accent::DDot => '\u{0308}',
        Accent::Tilde | Accent::WideTilde => '\u{0303}',
    }
}

/// Plain text for a subscript: numbers, names and simple sums of them.
pub fn sub_text(node: &Node) -> Option<String> {
    match &node.kind {
        NodeKind::Number(t) | NodeKind::Text(t) => Some(t.trim().to_string()),
        NodeKind::Name(n) => name_key(n),
        NodeKind::Constant(c) => Some(match c {
            Constant::Pi => "π".to_string(),
            Constant::Infinity => "∞".to_string(),
            Constant::E | Constant::I => c.letter()?.to_string(),
        }),
        NodeKind::Group { inner, .. } => sub_text(inner),
        NodeKind::Unary {
            op: UnaryOp::Neg,
            operand,
        } => Some(format!("-{}", sub_text(operand)?)),
        NodeKind::Binary { op, lhs, rhs } => {
            let sep = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul(MulStyle::Implicit) => "",
                BinOp::Mul(_) | BinOp::Times => "*",
                _ => return None,
            };
            Some(format!("{}{sep}{}", sub_text(lhs)?, sub_text(rhs)?))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mpp_syntax::math::parse_expr;

    fn key(src: &str) -> Option<String> {
        let (node, _) = parse_expr(src, 0);
        node_key(&node)
    }

    #[test]
    fn keys() {
        assert_eq!(key("x").as_deref(), Some("x"));
        assert_eq!(key("v_0").as_deref(), Some("v_0"));
        assert_eq!(key("\\theta").as_deref(), Some("θ"));
        assert_eq!(key("x_{max}").as_deref(), Some("x_max"));
        assert_eq!(key("\\hat x").as_deref(), Some("x\u{0302}"));
        assert_eq!(key("\\mathbf{v}").as_deref(), Some("𝐯"));
        assert_eq!(key("e").as_deref(), Some("e"));
        assert_eq!(key("2"), None);
    }
}
