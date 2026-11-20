use SymbolKind::{
    Accent, Arrow, Calculus, Delimiter, Function, Letter, Logic, Misc, Operator, Relation, Set,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Letter,
    Operator,
    Relation,
    Set,
    Logic,
    Calculus,
    Arrow,
    Accent,
    Font,
    Delimiter,
    Function,
    Misc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Symbol {
    pub name: &'static str,
    pub unicode: &'static str,
    pub latex: &'static str,
    pub kind: SymbolKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Font {
    Blackboard,
    Calligraphic,
    Bold,
    Fraktur,
    Roman,
}

const fn entry(
    name: &'static str,
    unicode: &'static str,
    latex: &'static str,
    kind: SymbolKind,
) -> Symbol {
    Symbol {
        name,
        unicode,
        latex,
        kind,
    }
}

pub static SYMBOLS: &[Symbol] = &[
    entry("alpha", "α", "\\alpha", Letter),
    entry("beta", "β", "\\beta", Letter),
    entry("gamma", "γ", "\\gamma", Letter),
    entry("delta", "δ", "\\delta", Letter),
    entry("epsilon", "ϵ", "\\epsilon", Letter),
    entry("varepsilon", "ε", "\\varepsilon", Letter),
    entry("zeta", "ζ", "\\zeta", Letter),
    entry("eta", "η", "\\eta", Letter),
    entry("theta", "θ", "\\theta", Letter),
    entry("vartheta", "ϑ", "\\vartheta", Letter),
    entry("iota", "ι", "\\iota", Letter),
    entry("kappa", "κ", "\\kappa", Letter),
    entry("lambda", "λ", "\\lambda", Letter),
    entry("mu", "μ", "\\mu", Letter),
    entry("nu", "ν", "\\nu", Letter),
    entry("xi", "ξ", "\\xi", Letter),
    entry("omicron", "ο", "\\omicron", Letter),
    entry("pi", "π", "\\pi", Letter),
    entry("varpi", "ϖ", "\\varpi", Letter),
    entry("rho", "ρ", "\\rho", Letter),
    entry("varrho", "ϱ", "\\varrho", Letter),
    entry("sigma", "σ", "\\sigma", Letter),
    entry("varsigma", "ς", "\\varsigma", Letter),
    entry("tau", "τ", "\\tau", Letter),
    entry("upsilon", "υ", "\\upsilon", Letter),
    entry("phi", "ϕ", "\\phi", Letter),
    entry("varphi", "φ", "\\varphi", Letter),
    entry("chi", "χ", "\\chi", Letter),
    entry("psi", "ψ", "\\psi", Letter),
    entry("omega", "ω", "\\omega", Letter),
    entry("Gamma", "Γ", "\\Gamma", Letter),
    entry("Delta", "Δ", "\\Delta", Letter),
    entry("Theta", "Θ", "\\Theta", Letter),
    entry("Lambda", "Λ", "\\Lambda", Letter),
    entry("Xi", "Ξ", "\\Xi", Letter),
    entry("Pi", "Π", "\\Pi", Letter),
    entry("Sigma", "Σ", "\\Sigma", Letter),
    entry("Upsilon", "Υ", "\\Upsilon", Letter),
    entry("Phi", "Φ", "\\Phi", Letter),
    entry("Psi", "Ψ", "\\Psi", Letter),
    entry("Omega", "Ω", "\\Omega", Letter),
    entry("Alpha", "Α", "A", Letter),
    entry("Beta", "Β", "B", Letter),
    entry("Epsilon", "Ε", "E", Letter),
    entry("Zeta", "Ζ", "Z", Letter),
    entry("Eta", "Η", "H", Letter),
    entry("Iota", "Ι", "I", Letter),
    entry("Kappa", "Κ", "K", Letter),
    entry("Mu", "Μ", "M", Letter),
    entry("Nu", "Ν", "N", Letter),
    entry("Omicron", "Ο", "O", Letter),
    entry("Rho", "Ρ", "P", Letter),
    entry("Tau", "Τ", "T", Letter),
    entry("Chi", "Χ", "X", Letter),
    entry("pm", "±", "\\pm", Operator),
    entry("mp", "∓", "\\mp", Operator),
    entry("times", "×", "\\times", Operator),
    entry("div", "÷", "\\div", Operator),
    entry("cdot", "·", "\\cdot", Operator),
    entry("circ", "∘", "\\circ", Operator),
    entry("otimes", "⊗", "\\otimes", Operator),
    entry("oplus", "⊕", "\\oplus", Operator),
    entry("ast", "∗", "\\ast", Operator),
    entry("star", "⋆", "\\star", Operator),
    entry("bullet", "•", "\\bullet", Operator),
    entry("leq", "≤", "\\leq", Relation),
    entry("le", "≤", "\\le", Relation),
    entry("geq", "≥", "\\geq", Relation),
    entry("ge", "≥", "\\ge", Relation),
    entry("neq", "≠", "\\neq", Relation),
    entry("ne", "≠", "\\ne", Relation),
    entry("approx", "≈", "\\approx", Relation),
    entry("equiv", "≡", "\\equiv", Relation),
    entry("sim", "∼", "\\sim", Relation),
    entry("simeq", "≃", "\\simeq", Relation),
    entry("cong", "≅", "\\cong", Relation),
    entry("propto", "∝", "\\propto", Relation),
    entry("ll", "≪", "\\ll", Relation),
    entry("gg", "≫", "\\gg", Relation),
    entry("prec", "≺", "\\prec", Relation),
    entry("succ", "≻", "\\succ", Relation),
    entry("perp", "⊥", "\\perp", Relation),
    entry("parallel", "∥", "\\parallel", Relation),
    entry("mid", "∣", "\\mid", Relation),
    entry("in", "∈", "\\in", Set),
    entry("notin", "∉", "\\notin", Set),
    entry("ni", "∋", "\\ni", Set),
    entry("subset", "⊂", "\\subset", Set),
    entry("subseteq", "⊆", "\\subseteq", Set),
    entry("supset", "⊃", "\\supset", Set),
    entry("supseteq", "⊇", "\\supseteq", Set),
    entry("cup", "∪", "\\cup", Set),
    entry("cap", "∩", "\\cap", Set),
    entry("setminus", "∖", "\\setminus", Set),
    entry("emptyset", "∅", "\\emptyset", Set),
    entry("varnothing", "∅", "\\varnothing", Set),
    entry("forall", "∀", "\\forall", Logic),
    entry("exists", "∃", "\\exists", Logic),
    entry("nexists", "∄", "\\nexists", Logic),
    entry("neg", "¬", "\\neg", Logic),
    entry("lnot", "¬", "\\lnot", Logic),
    entry("land", "∧", "\\land", Logic),
    entry("wedge", "∧", "\\wedge", Logic),
    entry("lor", "∨", "\\lor", Logic),
    entry("vee", "∨", "\\vee", Logic),
    entry("implies", "⇒", "\\implies", Logic),
    entry("Rightarrow", "⇒", "\\Rightarrow", Logic),
    entry("iff", "⇔", "\\iff", Logic),
    entry("Leftrightarrow", "⇔", "\\Leftrightarrow", Logic),
    entry("therefore", "∴", "\\therefore", Logic),
    entry("because", "∵", "\\because", Logic),
    entry("partial", "∂", "\\partial", Calculus),
    entry("nabla", "∇", "\\nabla", Calculus),
    entry("int", "∫", "\\int", Calculus),
    entry("iint", "∬", "\\iint", Calculus),
    entry("iiint", "∭", "\\iiint", Calculus),
    entry("oint", "∮", "\\oint", Calculus),
    entry("oiint", "∯", "\\oiint", Calculus),
    entry("sum", "∑", "\\sum", Calculus),
    entry("prod", "∏", "\\prod", Calculus),
    entry("coprod", "∐", "\\coprod", Calculus),
    entry("infty", "∞", "\\infty", Calculus),
    entry("prime", "′", "\\prime", Calculus),
    entry("dprime", "″", "\\prime\\prime", Calculus),
    entry("ell", "ℓ", "\\ell", Calculus),
    entry("hbar", "ℏ", "\\hbar", Calculus),
    entry("Re", "ℜ", "\\Re", Calculus),
    entry("Im", "ℑ", "\\Im", Calculus),
    entry("aleph", "ℵ", "\\aleph", Calculus),
    entry("to", "→", "\\to", Arrow),
    entry("rightarrow", "→", "\\rightarrow", Arrow),
    entry("gets", "←", "\\gets", Arrow),
    entry("leftarrow", "←", "\\leftarrow", Arrow),
    entry("leftrightarrow", "↔", "\\leftrightarrow", Arrow),
    entry("Leftarrow", "⇐", "\\Leftarrow", Arrow),
    entry("mapsto", "↦", "\\mapsto", Arrow),
    entry("uparrow", "↑", "\\uparrow", Arrow),
    entry("downarrow", "↓", "\\downarrow", Arrow),
    entry("longrightarrow", "⟶", "\\longrightarrow", Arrow),
    entry("Longrightarrow", "⟹", "\\Longrightarrow", Arrow),
    entry("longleftarrow", "⟵", "\\longleftarrow", Arrow),
    entry("Longleftarrow", "⟸", "\\Longleftarrow", Arrow),
    entry("longleftrightarrow", "⟷", "\\longleftrightarrow", Arrow),
    entry("Longleftrightarrow", "⟺", "\\Longleftrightarrow", Arrow),
    entry("longmapsto", "⟼", "\\longmapsto", Arrow),
    entry("hat", "\u{0302}", "\\hat", Accent),
    entry("widehat", "\u{0302}", "\\widehat", Accent),
    entry("bar", "\u{0304}", "\\bar", Accent),
    entry("overline", "\u{0305}", "\\overline", Accent),
    entry("vec", "\u{20D7}", "\\vec", Accent),
    entry("dot", "\u{0307}", "\\dot", Accent),
    entry("ddot", "\u{0308}", "\\ddot", Accent),
    entry("tilde", "\u{0303}", "\\tilde", Accent),
    entry("widetilde", "\u{0303}", "\\widetilde", Accent),
    entry("mathbb", "", "\\mathbb", SymbolKind::Font),
    entry("mathcal", "", "\\mathcal", SymbolKind::Font),
    entry("mathbf", "", "\\mathbf", SymbolKind::Font),
    entry("mathfrak", "", "\\mathfrak", SymbolKind::Font),
    entry("mathrm", "", "\\mathrm", SymbolKind::Font),
    entry("langle", "⟨", "\\langle", Delimiter),
    entry("rangle", "⟩", "\\rangle", Delimiter),
    entry("lfloor", "⌊", "\\lfloor", Delimiter),
    entry("rfloor", "⌋", "\\rfloor", Delimiter),
    entry("lceil", "⌈", "\\lceil", Delimiter),
    entry("rceil", "⌉", "\\rceil", Delimiter),
    entry("vert", "|", "\\vert", Delimiter),
    entry("Vert", "‖", "\\Vert", Delimiter),
    entry("lbrace", "{", "\\{", Delimiter),
    entry("rbrace", "}", "\\}", Delimiter),
    entry("ldots", "…", "\\ldots", Misc),
    entry("dots", "…", "\\dots", Misc),
    entry("cdots", "⋯", "\\cdots", Misc),
    entry("vdots", "⋮", "\\vdots", Misc),
    entry("ddots", "⋱", "\\ddots", Misc),
    entry("angle", "∠", "\\angle", Misc),
    entry("triangle", "△", "\\triangle", Misc),
    entry("degree", "°", "\\degree", Misc),
    entry("qed", "∎", "\\qedsymbol", Misc),
    entry("square", "□", "\\square", Misc),
    entry("sin", "sin", "\\sin", Function),
    entry("cos", "cos", "\\cos", Function),
    entry("tan", "tan", "\\tan", Function),
    entry("sec", "sec", "\\sec", Function),
    entry("csc", "csc", "\\csc", Function),
    entry("cot", "cot", "\\cot", Function),
    entry("arcsin", "arcsin", "\\arcsin", Function),
    entry("arccos", "arccos", "\\arccos", Function),
    entry("arctan", "arctan", "\\arctan", Function),
    entry("sinh", "sinh", "\\sinh", Function),
    entry("cosh", "cosh", "\\cosh", Function),
    entry("tanh", "tanh", "\\tanh", Function),
    entry("coth", "coth", "\\coth", Function),
    entry("arsinh", "arsinh", "\\operatorname{arsinh}", Function),
    entry("arcosh", "arcosh", "\\operatorname{arcosh}", Function),
    entry("artanh", "artanh", "\\operatorname{artanh}", Function),
    entry("ln", "ln", "\\ln", Function),
    entry("log", "log", "\\log", Function),
    entry("exp", "exp", "\\exp", Function),
    entry("sqrt", "sqrt", "\\sqrt", Function),
    entry("det", "det", "\\det", Function),
    entry("dim", "dim", "\\dim", Function),
    entry("ker", "ker", "\\ker", Function),
    entry("gcd", "gcd", "\\gcd", Function),
    entry("lcm", "lcm", "\\operatorname{lcm}", Function),
    entry("max", "max", "\\max", Function),
    entry("min", "min", "\\min", Function),
    entry("sup", "sup", "\\sup", Function),
    entry("inf", "inf", "\\inf", Function),
    entry("lim", "lim", "\\lim", Function),
    entry("liminf", "liminf", "\\liminf", Function),
    entry("limsup", "limsup", "\\limsup", Function),
    entry("deg", "deg", "\\deg", Function),
    entry("arg", "arg", "\\arg", Function),
    entry("sgn", "sgn", "\\operatorname{sgn}", Function),
    // Special functions of the calculus toolkits (erf x, the sine and cosine integrals).
    entry("erf", "erf", "\\operatorname{erf}", Function),
    entry("Si", "Si", "\\operatorname{Si}", Function),
    entry("Ci", "Ci", "\\operatorname{Ci}", Function),
];

pub fn lookup_name(name: &str) -> Option<&'static Symbol> {
    let name = name.strip_prefix('\\').unwrap_or(name);
    SYMBOLS.iter().find(|s| s.name == name)
}

pub fn lookup_char(c: char) -> Option<&'static Symbol> {
    let mut buf = [0u8; 4];
    let text: &str = c.encode_utf8(&mut buf);
    SYMBOLS.iter().find(|s| s.unicode == text)
}

pub fn lookup_latex(cmd: &str) -> Option<&'static Symbol> {
    let cmd = cmd.strip_prefix('\\').unwrap_or(cmd);
    SYMBOLS
        .iter()
        .find(|s| s.latex.strip_prefix('\\') == Some(cmd))
}

pub fn styled(font: Font, c: char) -> Option<char> {
    match font {
        Font::Blackboard => {
            blackboard_hole(c).or_else(|| offset(c, 0x1D538, 0x1D552, Some(0x1D7D8)))
        }
        Font::Calligraphic => script_hole(c).or_else(|| offset(c, 0x1D49C, 0x1D4B6, None)),
        Font::Bold => offset(c, 0x1D400, 0x1D41A, Some(0x1D7CE)),
        Font::Fraktur => fraktur_hole(c).or_else(|| offset(c, 0x1D504, 0x1D51E, None)),
        Font::Roman => c.is_ascii_alphanumeric().then_some(c),
    }
}

fn offset(c: char, upper: u32, lower: u32, digits: Option<u32>) -> Option<char> {
    let code = match c {
        'A'..='Z' => upper + (c as u32 - 'A' as u32),
        'a'..='z' => lower + (c as u32 - 'a' as u32),
        '0'..='9' => digits? + (c as u32 - '0' as u32),
        _ => return None,
    };
    char::from_u32(code)
}

fn blackboard_hole(c: char) -> Option<char> {
    match c {
        'C' => Some('ℂ'),
        'H' => Some('ℍ'),
        'N' => Some('ℕ'),
        'P' => Some('ℙ'),
        'Q' => Some('ℚ'),
        'R' => Some('ℝ'),
        'Z' => Some('ℤ'),
        _ => None,
    }
}

fn script_hole(c: char) -> Option<char> {
    match c {
        'B' => Some('ℬ'),
        'E' => Some('ℰ'),
        'F' => Some('ℱ'),
        'H' => Some('ℋ'),
        'I' => Some('ℐ'),
        'L' => Some('ℒ'),
        'M' => Some('ℳ'),
        'R' => Some('ℛ'),
        'e' => Some('ℯ'),
        'g' => Some('ℊ'),
        'o' => Some('ℴ'),
        _ => None,
    }
}

fn fraktur_hole(c: char) -> Option<char> {
    match c {
        'C' => Some('ℭ'),
        'H' => Some('ℌ'),
        'I' => Some('ℑ'),
        'R' => Some('ℜ'),
        'Z' => Some('ℨ'),
        _ => None,
    }
}

const SUPERSCRIPTS: &[(char, char)] = &[
    ('⁰', '0'),
    ('¹', '1'),
    ('²', '2'),
    ('³', '3'),
    ('⁴', '4'),
    ('⁵', '5'),
    ('⁶', '6'),
    ('⁷', '7'),
    ('⁸', '8'),
    ('⁹', '9'),
    ('⁺', '+'),
    ('⁻', '-'),
    ('⁼', '='),
    ('⁽', '('),
    ('⁾', ')'),
    ('ⁱ', 'i'),
    ('ⁿ', 'n'),
];

const SUBSCRIPTS: &[(char, char)] = &[
    ('₀', '0'),
    ('₁', '1'),
    ('₂', '2'),
    ('₃', '3'),
    ('₄', '4'),
    ('₅', '5'),
    ('₆', '6'),
    ('₇', '7'),
    ('₈', '8'),
    ('₉', '9'),
    ('₊', '+'),
    ('₋', '-'),
    ('₌', '='),
    ('₍', '('),
    ('₎', ')'),
    ('ₐ', 'a'),
    ('ₑ', 'e'),
    ('ₒ', 'o'),
    ('ₓ', 'x'),
    ('ₕ', 'h'),
    ('ₖ', 'k'),
    ('ₗ', 'l'),
    ('ₘ', 'm'),
    ('ₙ', 'n'),
    ('ₚ', 'p'),
    ('ₛ', 's'),
    ('ₜ', 't'),
];

pub fn superscript_value(c: char) -> Option<char> {
    SUPERSCRIPTS
        .iter()
        .find(|(from, _)| *from == c)
        .map(|(_, to)| *to)
}

pub fn subscript_value(c: char) -> Option<char> {
    SUBSCRIPTS
        .iter()
        .find(|(from, _)| *from == c)
        .map(|(_, to)| *to)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn names_are_unique() {
        let mut seen = HashSet::new();
        for s in SYMBOLS {
            assert!(seen.insert(s.name), "duplicate name {}", s.name);
        }
    }

    #[test]
    fn names_round_trip() {
        for s in SYMBOLS {
            let found = lookup_name(s.name).expect("name lookup");
            assert!(std::ptr::eq(found, s), "{} resolves elsewhere", s.name);
        }
    }

    #[test]
    fn latex_round_trip() {
        for s in SYMBOLS.iter().filter(|s| s.latex.starts_with('\\')) {
            let found = lookup_latex(s.latex).expect("latex lookup");
            assert_eq!(found.latex, s.latex);
        }
    }

    #[test]
    fn single_char_symbols_round_trip_through_char() {
        for s in SYMBOLS {
            let mut chars = s.unicode.chars();
            if let (Some(c), None) = (chars.next(), chars.next()) {
                let found = lookup_char(c).expect("char lookup");
                assert_eq!(found.unicode, s.unicode);
            }
        }
    }

    #[test]
    fn spot_checks() {
        let unicode = |name: &str| lookup_name(name).map(|s| s.unicode);
        assert_eq!(unicode("alpha"), Some("α"));
        assert_eq!(unicode("\\varepsilon"), Some("ε"));
        assert_eq!(unicode("epsilon"), Some("ϵ"));
        assert_eq!(unicode("nabla"), Some("∇"));
        assert_eq!(unicode("Alpha"), Some("\u{0391}"));
        assert_eq!(lookup_latex("\\phi").map(|s| s.unicode), Some("ϕ"));
        assert_eq!(lookup_latex("varphi").map(|s| s.unicode), Some("φ"));
        assert_eq!(lookup_char('∇').map(|s| s.name), Some("nabla"));
        assert!(lookup_char('A').is_none());
        assert!(lookup_name("nonsense").is_none());
    }

    #[test]
    fn font_styles() {
        assert_eq!(styled(Font::Blackboard, 'R'), Some('ℝ'));
        assert_eq!(styled(Font::Blackboard, 'N'), Some('ℕ'));
        assert_eq!(styled(Font::Blackboard, 'k'), Some('\u{1D55C}'));
        assert_eq!(styled(Font::Calligraphic, 'B'), Some('ℬ'));
        assert_eq!(styled(Font::Calligraphic, 'A'), Some('\u{1D49C}'));
        assert_eq!(styled(Font::Bold, 'A'), Some('\u{1D400}'));
        assert_eq!(styled(Font::Bold, '2'), Some('\u{1D7D0}'));
        assert_eq!(styled(Font::Fraktur, 'H'), Some('ℌ'));
        assert_eq!(styled(Font::Fraktur, 'a'), Some('\u{1D51E}'));
        assert_eq!(styled(Font::Roman, 'x'), Some('x'));
        assert_eq!(styled(Font::Bold, '+'), None);
    }

    #[test]
    fn super_and_subscripts() {
        assert_eq!(superscript_value('²'), Some('2'));
        assert_eq!(superscript_value('⁺'), Some('+'));
        assert_eq!(superscript_value('ⁿ'), Some('n'));
        assert_eq!(subscript_value('₀'), Some('0'));
        assert_eq!(subscript_value('₋'), Some('-'));
        assert_eq!(subscript_value('x'), None);
    }
}
