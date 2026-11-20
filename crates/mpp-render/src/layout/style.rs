//! TeX's four styles, cramping, atom classes and the inter-atom spacing table.

use crate::font::MathConstants;

/// The size level of a style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Display,
    Text,
    Script,
    ScriptScript,
}

/// A TeX style: a size level plus whether it is cramped (superscripts set lower).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Style {
    pub level: Level,
    pub cramped: bool,
}

impl Style {
    pub const DISPLAY: Style = Style {
        level: Level::Display,
        cramped: false,
    };
    pub const TEXT: Style = Style {
        level: Level::Text,
        cramped: false,
    };

    fn at(self, level: Level) -> Style {
        Style { level, ..self }
    }

    /// The same style, cramped.
    pub fn cramp(self) -> Style {
        Style {
            cramped: true,
            ..self
        }
    }

    /// Display style?
    pub fn is_display(self) -> bool {
        self.level == Level::Display
    }

    /// Script or scriptscript?
    pub fn is_script(self) -> bool {
        self.level >= Level::Script
    }

    /// Style of a superscript.
    pub fn sup(self) -> Style {
        match self.level {
            Level::Display | Level::Text => self.at(Level::Script),
            Level::Script | Level::ScriptScript => self.at(Level::ScriptScript),
        }
    }

    /// Style of a subscript (always cramped).
    pub fn sub(self) -> Style {
        self.sup().cramp()
    }

    /// Style of a fraction numerator.
    pub fn num(self) -> Style {
        match self.level {
            Level::Display => self.at(Level::Text),
            Level::Text => self.at(Level::Script),
            _ => self.at(Level::ScriptScript),
        }
    }

    /// Style of a fraction denominator (cramped).
    pub fn den(self) -> Style {
        self.num().cramp()
    }

    /// Size relative to the base font size.
    pub fn scale(self, c: &MathConstants) -> f32 {
        match self.level {
            Level::Display | Level::Text => 1.0,
            Level::Script => c.script_scale,
            Level::ScriptScript => c.script_script_scale,
        }
    }

    /// `ssty` level for glyph alternates: 0 for text sizes, 1 or 2 in scripts.
    pub fn ssty(self) -> u8 {
        match self.level {
            Level::Display | Level::Text => 0,
            Level::Script => 1,
            Level::ScriptScript => 2,
        }
    }
}

/// TeX atom classes, which decide the spacing between neighbours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Class {
    Ord,
    Op,
    Bin,
    Rel,
    Open,
    Close,
    Punct,
    Inner,
}

impl Class {
    fn index(self) -> usize {
        match self {
            Class::Ord => 0,
            Class::Op => 1,
            Class::Bin => 2,
            Class::Rel => 3,
            Class::Open => 4,
            Class::Close => 5,
            Class::Punct => 6,
            Class::Inner => 7,
        }
    }
}

// 0: none, 1: thin, 2: medium, 3: thick; negative: only in display and text styles.
const TABLE: [[i8; 8]; 8] = [
    //Ord Op  Bin Rel Opn Cls Pct Inn
    [0, 1, -2, -3, 0, 0, 0, -1],     // Ord
    [1, 1, 0, -3, 0, 0, 0, -1],      // Op
    [-2, -2, 0, 0, -2, 0, 0, -2],    // Bin
    [-3, -3, 0, 0, -3, 0, 0, -3],    // Rel
    [0, 0, 0, 0, 0, 0, 0, 0],        // Open
    [0, 1, -2, -3, 0, 0, 0, -1],     // Close
    [-1, -1, 0, -1, -1, -1, -1, -1], // Punct
    [-1, 1, -2, -3, -1, 0, -1, -1],  // Inner
];

/// Space in mu (1/18 em) between two atoms.
pub fn space_mu(left: Class, right: Class, style: Style) -> f32 {
    let v = TABLE[left.index()][right.index()];
    if v < 0 && style.is_script() {
        return 0.0;
    }
    match v.abs() {
        1 => 3.0,
        2 => 4.0,
        3 => 5.0,
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styles_step_down() {
        assert_eq!(Style::DISPLAY.sup().level, Level::Script);
        assert_eq!(Style::DISPLAY.num().level, Level::Text);
        assert!(Style::TEXT.sub().cramped);
        assert_eq!(Style::TEXT.sup().sup().sup().level, Level::ScriptScript);
    }

    #[test]
    fn spacing_follows_tex() {
        assert_eq!(space_mu(Class::Ord, Class::Rel, Style::TEXT), 5.0);
        assert_eq!(space_mu(Class::Ord, Class::Bin, Style::TEXT), 4.0);
        assert_eq!(space_mu(Class::Ord, Class::Bin, Style::TEXT.sup()), 0.0);
        assert_eq!(space_mu(Class::Op, Class::Ord, Style::TEXT.sup()), 3.0);
        assert_eq!(space_mu(Class::Open, Class::Ord, Style::TEXT), 0.0);
    }
}
