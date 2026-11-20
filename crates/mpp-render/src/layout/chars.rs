//! Mapping letters to the Unicode maths alphabets.

use mpp_syntax::symbols::{styled, Font};

/// The maths italic form of a letter: Latin letters and lower-case Greek
/// (upper-case Greek stays upright, as in TeX). Anything else is unchanged.
pub fn italic(c: char) -> char {
    let code = c as u32;
    let mapped = match c {
        'h' => Some(0x210E),
        'a'..='z' => Some(0x1D44E + code - 'a' as u32),
        'A'..='Z' => Some(0x1D434 + code - 'A' as u32),
        'α'..='ω' => Some(0x1D6FC + code - 'α' as u32),
        'ϵ' => Some(0x1D716),
        'ϑ' => Some(0x1D717),
        'ϰ' => Some(0x1D718),
        'ϕ' => Some(0x1D719),
        'ϱ' => Some(0x1D71A),
        'ϖ' => Some(0x1D71B),
        'ı' => Some(0x1D6A4),
        'ȷ' => Some(0x1D6A5),
        _ => None,
    };
    mapped.and_then(char::from_u32).unwrap_or(c)
}

/// The letter in a font, falling back to the plain letter.
pub fn in_font(font: Font, c: char) -> char {
    match font {
        Font::Roman => c,
        Font::Bold => bold(c),
        _ => styled(font, c).unwrap_or(c),
    }
}

/// Bold upright Latin, digits and Greek.
fn bold(c: char) -> char {
    if let Some(s) = styled(Font::Bold, c) {
        return s;
    }
    let code = c as u32;
    let mapped = match c {
        'Α'..='Ω' => Some(0x1D6A8 + code - 'Α' as u32),
        'α'..='ω' => Some(0x1D6C2 + code - 'α' as u32),
        '∇' => Some(0x1D6C1),
        '∂' => Some(0x1D6DB),
        _ => None,
    };
    mapped.and_then(char::from_u32).unwrap_or(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_map_to_maths_italic() {
        assert_eq!(italic('x'), '𝑥');
        assert_eq!(italic('h'), 'ℎ');
        assert_eq!(italic('A'), '𝐴');
        assert_eq!(italic('θ'), '𝜃');
        assert_eq!(italic('Γ'), 'Γ');
        assert_eq!(italic('2'), '2');
        assert_eq!(in_font(Font::Blackboard, 'R'), 'ℝ');
        assert_eq!(in_font(Font::Bold, 'α'), '𝛂');
    }
}
