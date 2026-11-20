//! The typesetting primitives: glyphs, atom lists with TeX spacing, scripts,
//! fractions, radicals, stretchy delimiters, big operators, limits, accents and
//! empty slots. Every length is in em at the base size.

use mpp_syntax::math::Accent;
use mpp_syntax::symbols::Font;
use mpp_syntax::Span;

use super::boxes::{vstack_centered, Content, GlyphBox, Hit, LBox, Placed};
use super::chars;
use super::style::{space_mu, Class, Style};
use super::{IntegralLimits, LayoutOptions};
use crate::display::{Color, HitKind, NodePath};
use crate::font::{FontId, Fonts, MathConstants, Stretched};

/// A laid-out piece of a formula with the classes of its two ends.
#[derive(Debug, Clone)]
pub(crate) struct Atom {
    pub left: Class,
    pub right: Class,
    pub b: LBox,
}

impl Atom {
    pub fn new(class: Class, b: LBox) -> Atom {
        Atom {
            left: class,
            right: class,
            b,
        }
    }

    pub fn ord(b: LBox) -> Atom {
        Atom::new(Class::Ord, b)
    }
}

/// Deepest nesting the engine follows before drawing a placeholder instead.
pub(crate) const MAX_DEPTH: u32 = 48;

/// Typesetting state.
pub(crate) struct Engine<'a> {
    pub fonts: &'static Fonts,
    pub c: MathConstants,
    pub opts: &'a LayoutOptions,
    /// Colour of new ink.
    pub color: Color,
    /// A font forced by `\mathbf{…}` and friends.
    pub font: Option<Font>,
    /// Current nesting depth of the node walk.
    pub depth: u32,
    /// When false, no hit boxes are recorded (results have no source).
    pub hits: bool,
}

impl<'a> Engine<'a> {
    pub fn new(fonts: &'static Fonts, opts: &'a LayoutOptions) -> Self {
        Engine {
            fonts,
            c: *fonts.constants(),
            opts,
            color: opts.theme.text,
            font: None,
            depth: 0,
            hits: true,
        }
    }

    pub fn integral_limits(&self) -> IntegralLimits {
        self.opts.integral_limits
    }

    /// Size multiplier of a style.
    pub fn scale(&self, st: Style) -> f32 {
        st.scale(&self.c)
    }

    /// One mu (1/18 em) in a style.
    pub fn mu(&self, st: Style) -> f32 {
        self.scale(st) / 18.0
    }

    /// The maths axis height in a style.
    pub fn axis(&self, st: Style) -> f32 {
        self.c.axis_height * self.scale(st)
    }

    /// A hit for a node, or nothing when hits are off.
    pub fn hit(&self, span: Span, path: &NodePath, kind: HitKind) -> Option<Hit> {
        self.hits.then(|| Hit {
            span,
            path: path.clone(),
            kind,
            label: None,
        })
    }

    // ----- glyphs -----

    /// A glyph box for a glyph id at a size.
    pub fn glyph_id_box(&self, font: FontId, glyph: u16, ch: char, size: f32) -> LBox {
        let m = self.fonts.metrics(font, glyph);
        let italic = if font == FontId::Math {
            self.fonts.italic_correction(glyph) * size
        } else {
            0.0
        };
        LBox {
            width: m.advance * size,
            height: m.height * size,
            depth: m.depth * size,
            italic,
            content: Content::Glyph(GlyphBox {
                font,
                glyph,
                ch,
                size,
                color: self.color,
            }),
            hit: None,
        }
    }

    /// A character in a font at a style, with script alternates in maths, and
    /// falling back to the other font when the glyph is missing.
    pub fn char_box(&self, font: FontId, ch: char, st: Style) -> LBox {
        let size = self.scale(st);
        if ch == ' ' {
            let w = self
                .fonts
                .glyph(FontId::Text, ' ')
                .map(|g| self.fonts.metrics(FontId::Text, g).advance)
                .unwrap_or(0.25);
            return LBox::kern(w * size);
        }
        let (font, glyph) = match self.fonts.glyph(font, ch) {
            Some(g) => (font, g),
            None => {
                let other = if font == FontId::Math {
                    FontId::Text
                } else {
                    FontId::Math
                };
                match self.fonts.glyph(other, ch) {
                    Some(g) => (other, g),
                    None => (font, 0),
                }
            }
        };
        let glyph = if font == FontId::Math {
            self.fonts.script_variant(glyph, st.ssty())
        } else {
            glyph
        };
        self.glyph_id_box(font, glyph, ch, size)
    }

    /// An upright maths symbol.
    pub fn sym(&self, ch: char, st: Style) -> LBox {
        self.char_box(FontId::Math, ch, st)
    }

    /// A string of upright maths symbols.
    pub fn sym_str(&self, s: &str, st: Style) -> LBox {
        LBox::hlist(s.chars().map(|c| self.sym(c, st)).collect())
    }

    /// Text in a text font.
    pub fn text(&self, s: &str, font: FontId, st: Style) -> LBox {
        LBox::hlist(s.chars().map(|c| self.char_box(font, c, st)).collect())
    }

    /// A letter: italic unless a font is forced or `upright` is set.
    pub fn letter(&self, ch: char, st: Style, upright: bool) -> LBox {
        let mapped = match self.font {
            Some(f) => chars::in_font(f, ch),
            None if upright => ch,
            None => chars::italic(ch),
        };
        self.sym(mapped, st)
    }

    /// An upright word in the maths font (`sin`, `lim`, `max`), or in the forced font.
    pub fn word(&self, s: &str, st: Style) -> LBox {
        LBox::hlist(
            s.chars()
                .map(|c| match self.font {
                    Some(f) => self.sym(chars::in_font(f, c), st),
                    None => self.sym(c, st),
                })
                .collect(),
        )
    }

    // ----- lists -----

    /// Lays atoms out side by side with TeX's spacing, returning the box, the
    /// classes of its ends, and the x offset of every atom.
    pub fn hlist_offsets(&self, atoms: Vec<Atom>, st: Style) -> (LBox, Class, Class, Vec<f32>) {
        let mut atoms = atoms;
        // Binary operators become ordinary where they cannot be binary.
        let n = atoms.len();
        for i in 0..n {
            if atoms[i].left == Class::Bin {
                let prev = if i == 0 {
                    None
                } else {
                    Some(atoms[i - 1].right)
                };
                let next = atoms.get(i + 1).map(|a| a.left);
                let bad_prev = matches!(
                    prev,
                    None | Some(Class::Bin | Class::Op | Class::Rel | Class::Open | Class::Punct)
                );
                let bad_next =
                    matches!(next, None | Some(Class::Rel | Class::Close | Class::Punct));
                if bad_prev || bad_next {
                    atoms[i].left = Class::Ord;
                    if atoms[i].right == Class::Bin {
                        atoms[i].right = Class::Ord;
                    }
                }
            }
        }
        let left = atoms.first().map(|a| a.left).unwrap_or(Class::Ord);
        let right = atoms.last().map(|a| a.right).unwrap_or(Class::Ord);
        let mu = self.mu(st);
        let mut items = Vec::with_capacity(atoms.len() * 2);
        let mut offsets = Vec::with_capacity(atoms.len());
        let mut x = 0.0;
        let mut prev: Option<Class> = None;
        for a in atoms {
            if let Some(p) = prev {
                let s = space_mu(p, a.left, st) * mu;
                if s > 0.0 {
                    items.push(LBox::kern(s));
                    x += s;
                }
            }
            prev = Some(a.right);
            offsets.push(x);
            x += a.b.width;
            items.push(a.b);
        }
        let b = if items.is_empty() {
            LBox::empty()
        } else {
            LBox::hlist(items)
        };
        (b, left, right, offsets)
    }

    /// Lays atoms out as one atom.
    pub fn group(&self, atoms: Vec<Atom>, st: Style) -> Atom {
        if atoms.len() == 1 {
            if let Some(a) = atoms.into_iter().next() {
                return a;
            }
            return Atom::ord(LBox::empty());
        }
        let (b, left, right, _) = self.hlist_offsets(atoms, st);
        Atom { left, right, b }
    }

    // ----- scripts -----

    /// Attaches a subscript and/or superscript to a nucleus. `op` marks a big
    /// operator, whose subscript tucks in by the italic correction.
    pub fn scripts(
        &self,
        base: LBox,
        sub: Option<LBox>,
        sup: Option<LBox>,
        st: Style,
        op: bool,
    ) -> LBox {
        if sub.is_none() && sup.is_none() {
            return base;
        }
        let s = self.scale(st);
        let c = &self.c;
        let is_char = base.single_glyph().is_some() && !op;
        let italic = base.italic;
        let mut sup_shift = 0.0;
        let mut sub_shift = 0.0;
        if let Some(p) = &sup {
            let mut up = if st.cramped {
                c.superscript_shift_up_cramped
            } else {
                c.superscript_shift_up
            } * s;
            if !is_char {
                up = up.max(base.height - c.superscript_baseline_drop_max * s);
            }
            up = up.max(c.superscript_bottom_min * s + p.depth);
            sup_shift = up;
        }
        if let Some(b) = &sub {
            let mut down = (c.subscript_shift_down * s).max(b.height - c.subscript_top_max * s);
            if !is_char {
                down = down.max(base.depth + c.subscript_baseline_drop_min * s);
            }
            sub_shift = down;
        }
        if let (Some(p), Some(b)) = (&sup, &sub) {
            let gap = (sup_shift - p.depth) - (b.height - sub_shift);
            let min = c.sub_superscript_gap_min * s;
            if gap < min {
                sub_shift += min - gap;
                let lift = c.superscript_bottom_max_with_subscript * s - (sup_shift - p.depth);
                if lift > 0.0 {
                    sup_shift += lift;
                    sub_shift -= lift;
                }
            }
        }
        let after = c.space_after_script * s;
        let bw = base.width;
        let mut width = bw;
        let mut height = base.height;
        let mut depth = base.depth;
        let mut children = vec![Placed {
            x: 0.0,
            y: 0.0,
            b: base,
        }];
        let mut new_italic = 0.0;
        if let Some(p) = sup {
            let x = if op { bw } else { bw + italic };
            height = height.max(sup_shift + p.height);
            depth = depth.max(p.depth - sup_shift);
            width = width.max(x + p.width + after);
            new_italic = p.italic;
            children.push(Placed {
                x,
                y: -sup_shift,
                b: p,
            });
        }
        if let Some(b) = sub {
            let x = if op { bw - italic } else { bw };
            height = height.max(b.height - sub_shift);
            depth = depth.max(sub_shift + b.depth);
            width = width.max(x + b.width + after);
            children.push(Placed { x, y: sub_shift, b });
        }
        let mut out = LBox::with_children(width, height.max(0.0), depth.max(0.0), children);
        out.italic = new_italic;
        out
    }

    /// Limits above and below a big operator (centred, offset by its italic
    /// correction).
    pub fn limits(&self, op: LBox, lower: Option<LBox>, upper: Option<LBox>, st: Style) -> LBox {
        if lower.is_none() && upper.is_none() {
            return op;
        }
        let s = self.scale(st);
        let c = &self.c;
        let it = op.italic;
        let cx = op.width / 2.0;
        let mut parts: Vec<(f32, f32, LBox)> = vec![];
        let (oh, od) = (op.height, op.depth);
        if let Some(u) = upper {
            let rise =
                (c.upper_limit_baseline_rise_min * s).max(c.upper_limit_gap_min * s + u.depth);
            let x = cx + it / 2.0 - u.width / 2.0;
            parts.push((x, -(oh + rise), u));
        }
        if let Some(l) = lower {
            let drop =
                (c.lower_limit_baseline_drop_min * s).max(c.lower_limit_gap_min * s + l.height);
            let x = cx - it / 2.0 - l.width / 2.0;
            parts.push((x, od + drop, l));
        }
        parts.push((0.0, 0.0, op));
        let min_x = parts.iter().map(|p| p.0).fold(0.0, f32::min);
        let mut width: f32 = 0.0;
        let mut height: f32 = 0.0;
        let mut depth: f32 = 0.0;
        let mut children = vec![];
        for (x, y, b) in parts {
            let x = x - min_x;
            width = width.max(x + b.width);
            height = height.max(b.height - y);
            depth = depth.max(b.depth + y);
            children.push(Placed { x, y, b });
        }
        // A little room above and below, like TeX's \bigopspacing5.
        let pad = 0.1 * s;
        LBox::with_children(width, height + pad, depth + pad, children)
    }

    // ----- fractions -----

    /// A fraction (with a rule) or a stack (without one, for binomials). The
    /// numerator and denominator must already be set in `st.num()` / `st.den()`.
    pub fn fraction(&self, num: LBox, den: LBox, st: Style, rule: bool) -> LBox {
        let s = self.scale(st);
        let c = &self.c;
        let axis = self.axis(st);
        let display = st.is_display();
        let pad = 0.12 * s;
        let width = num.width.max(den.width) + 2.0 * pad;
        let (mut up, mut down);
        let t;
        if rule {
            t = c.fraction_rule_thickness * s;
            up = if display {
                c.fraction_numerator_display_style_shift_up
            } else {
                c.fraction_numerator_shift_up
            } * s;
            down = if display {
                c.fraction_denominator_display_style_shift_down
            } else {
                c.fraction_denominator_shift_down
            } * s;
            let gap_num = if display {
                c.fraction_num_display_style_gap_min
            } else {
                c.fraction_numerator_gap_min
            } * s;
            let gap_den = if display {
                c.fraction_denom_display_style_gap_min
            } else {
                c.fraction_denominator_gap_min
            } * s;
            up = up.max(axis + t / 2.0 + gap_num + num.depth);
            down = down.max(den.height - axis + t / 2.0 + gap_den);
        } else {
            t = 0.0;
            up = if display {
                c.stack_top_display_style_shift_up
            } else {
                c.stack_top_shift_up
            } * s;
            down = if display {
                c.stack_bottom_display_style_shift_down
            } else {
                c.stack_bottom_shift_down
            } * s;
            let gap = if display {
                c.stack_display_style_gap_min
            } else {
                c.stack_gap_min
            } * s;
            let actual = (up - num.depth) - (den.height - down);
            if actual < gap {
                up += (gap - actual) / 2.0;
                down += (gap - actual) / 2.0;
            }
        }
        let height = (up + num.height).max(axis + t / 2.0);
        let depth = (down + den.depth).max(t / 2.0 - axis);
        let mut children = vec![
            Placed {
                x: (width - num.width) / 2.0,
                y: -up,
                b: num,
            },
            Placed {
                x: (width - den.width) / 2.0,
                y: down,
                b: den,
            },
        ];
        if rule {
            children.push(Placed {
                x: 0.0,
                y: -(axis - t / 2.0),
                b: LBox::rule(width, t, 0.0, self.color),
            });
        }
        LBox::with_children(width, height, depth.max(0.0), children)
    }

    // ----- radicals -----

    /// A radical over `radicand` (already set cramped), with an optional index.
    pub fn radical(&self, radicand: LBox, index: Option<LBox>, st: Style) -> LBox {
        let s = self.scale(st);
        let c = &self.c;
        let t = c.radical_rule_thickness * s;
        let mut gap = if st.is_display() {
            c.radical_display_style_vertical_gap
        } else {
            c.radical_vertical_gap
        } * s;
        let extra = c.radical_extra_ascender * s;
        let need = radicand.height + radicand.depth + gap + t;
        let sign = self.stretchy('√', need, true, st);
        let sign_total = sign.height + sign.depth;
        if sign_total > need {
            gap += (sign_total - need) / 2.0;
        }
        // Put the sign's top at the top of the rule.
        let rule_bottom = radicand.height + gap;
        let rule_top = rule_bottom + t;
        let sign_y = sign.height - rule_top; // offset to apply (downwards)
        let sign_w = sign.width;
        let sign_bottom = sign_total - rule_top;
        let mut x0 = 0.0;
        let mut children = vec![];
        let mut height = rule_top + extra;
        let mut depth = radicand.depth.max(sign_bottom);
        if let Some(idx) = index {
            let before = c.radical_kern_before_degree * s;
            let after = c.radical_kern_after_degree * s;
            let raise = c.radical_degree_bottom_raise * sign_total - sign_bottom;
            let iy = -(raise + idx.depth);
            height = height.max(idx.height - iy);
            depth = depth.max(idx.depth + iy);
            let iw = idx.width;
            children.push(Placed {
                x: before,
                y: iy,
                b: idx,
            });
            x0 = (before + iw + after).max(0.0);
        }
        let rw = radicand.width;
        children.push(Placed {
            x: x0,
            y: sign_y,
            b: sign,
        });
        children.push(Placed {
            x: x0 + sign_w,
            y: -rule_bottom,
            b: LBox::rule(rw, t, 0.0, self.color),
        });
        children.push(Placed {
            x: x0 + sign_w,
            y: 0.0,
            b: radicand,
        });
        LBox::with_children(x0 + sign_w + rw, height, depth, children)
    }

    // ----- stretchy glyphs -----

    /// A maths glyph grown to at least `size` em (tall when `vertical`, else
    /// wide). A variant comes back as a glyph box with its own metrics; an
    /// assembly is built with its bottom (or left end) on the baseline.
    pub fn stretchy(&self, ch: char, size: f32, vertical: bool, st: Style) -> LBox {
        let base_size = self.scale(st);
        let Some(gid) = self.fonts.glyph(FontId::Math, ch) else {
            return self.sym(ch, st);
        };
        // Variants are measured at the base size; compare in em of this style.
        let want = if base_size > 0.0 {
            size / base_size
        } else {
            size
        };
        match self.fonts.stretch(gid, want, vertical) {
            Stretched::Glyph(g) => self.glyph_id_box(FontId::Math, g, ch, base_size),
            Stretched::Assembly { parts, size: total } => {
                let mut children = vec![];
                let mut pos = 0.0;
                let mut cross: f32 = 0.0;
                for p in parts {
                    pos += p.offset * base_size;
                    let m = self.fonts.metrics(FontId::Math, p.glyph);
                    let b = self.glyph_id_box(FontId::Math, p.glyph, ch, base_size);
                    if vertical {
                        cross = cross.max(b.width);
                        // Parts run bottom to top: put each part's ink bottom at `pos`.
                        children.push(Placed {
                            x: 0.0,
                            y: -pos + m.y_min * base_size,
                            b,
                        });
                    } else {
                        cross = cross.max(b.height);
                        children.push(Placed {
                            x: pos - m.x_min * base_size,
                            y: 0.0,
                            b,
                        });
                    }
                }
                let total = total * base_size;
                let mut out = if vertical {
                    LBox::with_children(cross, total, 0.0, children)
                } else {
                    LBox::with_children(total, cross, 0.0, children)
                };
                out.italic = self.fonts.assembly_italic(gid, vertical) * base_size;
                out
            }
        }
    }

    /// The box moved vertically so it is centred on the maths axis.
    pub fn center_on_axis(&self, b: LBox, st: Style) -> LBox {
        let shift = self.axis(st) - (b.height - b.depth) / 2.0;
        b.raised(shift)
    }

    /// A delimiter big enough to enclose content of height `h` and depth `d`
    /// (TeX's \delimiterfactor 901 and \delimitershortfall 5pt).
    pub fn delimiter(&self, ch: char, h: f32, d: f32, st: Style) -> LBox {
        let s = self.scale(st);
        let axis = self.axis(st);
        let y = 2.0 * (h - axis).max(d + axis);
        let size = (y * 0.901).max(y - 0.5 * s);
        let normal = self.sym(ch, st);
        if normal.height + normal.depth >= size {
            return normal;
        }
        let big = self.stretchy(ch, size, true, st);
        self.center_on_axis(big, st)
    }

    /// `open inner close` with delimiters sized to the inner box.
    pub fn fenced(&self, open: Option<char>, inner: Atom, close: Option<char>, st: Style) -> Atom {
        let (h, d) = (inner.b.height, inner.b.depth);
        let mut atoms = vec![];
        let null = 0.12 * self.scale(st);
        match open {
            Some(c) => atoms.push(Atom::new(Class::Open, self.delimiter(c, h, d, st))),
            None if close.is_some() => atoms.push(Atom::new(Class::Open, LBox::kern(null))),
            None => {}
        }
        atoms.push(Atom::ord(inner.b));
        match close {
            Some(c) => atoms.push(Atom::new(Class::Close, self.delimiter(c, h, d, st))),
            None if open.is_some() => atoms.push(Atom::new(Class::Close, LBox::kern(null))),
            None => {}
        }
        let (b, _, _, _) = self.hlist_offsets(atoms, st);
        Atom {
            left: if open.is_some() || close.is_some() {
                Class::Open
            } else {
                inner.left
            },
            right: if open.is_some() || close.is_some() {
                Class::Close
            } else {
                inner.right
            },
            b,
        }
    }

    /// A big operator sign: the display-size variant in display style,
    /// centred on the axis.
    pub fn big_op(&self, ch: char, st: Style) -> LBox {
        let s = self.scale(st);
        let Some(gid) = self.fonts.glyph(FontId::Math, ch) else {
            return self.sym(ch, st);
        };
        let glyph = if st.is_display() {
            let min = self.c.display_operator_min_height;
            let variants = self.fonts.variants(gid, true);
            variants
                .iter()
                .find(|(_, h)| *h >= min)
                .or(variants.last())
                .map(|(g, _)| *g)
                .unwrap_or(gid)
        } else {
            gid
        };
        let b = self.glyph_id_box(FontId::Math, glyph, ch, s);
        let it = b.italic;
        let mut out = self.center_on_axis(b, st);
        out.italic = it;
        out
    }

    // ----- accents -----

    /// An accent over a base box.
    pub fn accent(&self, base: LBox, accent: Accent, st: Style) -> LBox {
        let s = self.scale(st);
        let c = &self.c;
        if accent == Accent::Overline {
            let gap = c.overbar_vertical_gap * s;
            let t = c.overbar_rule_thickness * s;
            let extra = c.overbar_extra_ascender * s;
            let (w, h, d, it) = (base.width, base.height, base.depth, base.italic);
            let mut out = LBox::with_children(
                w,
                h + gap + t + extra,
                d,
                vec![
                    Placed {
                        x: 0.0,
                        y: -(h + gap),
                        b: LBox::rule(w, t, 0.0, self.color),
                    },
                    Placed {
                        x: 0.0,
                        y: 0.0,
                        b: base,
                    },
                ],
            );
            out.italic = it;
            return out;
        }
        let (ch, wide) = match accent {
            Accent::Hat => ('\u{0302}', false),
            Accent::WideHat => ('\u{0302}', true),
            Accent::Bar => ('\u{0304}', false),
            Accent::Vec => ('\u{20D7}', false),
            Accent::Dot => ('\u{0307}', false),
            Accent::DDot => ('\u{0308}', false),
            Accent::Tilde => ('\u{0303}', false),
            Accent::WideTilde => ('\u{0303}', true),
            Accent::Overline => ('\u{0305}', false),
        };
        let Some(gid) = self.fonts.glyph(FontId::Math, ch) else {
            return base;
        };
        let single = base.single_glyph().cloned();
        let gid = if wide || single.is_none() {
            match self.fonts.stretch(gid, base.width / s.max(1e-6), false) {
                Stretched::Glyph(g) => g,
                Stretched::Assembly { .. } => gid,
            }
        } else {
            gid
        };
        let m = self.fonts.metrics(FontId::Math, gid);
        let accent_attach = self
            .fonts
            .top_accent_attachment(gid)
            .unwrap_or((m.x_min + m.x_max) / 2.0)
            * s;
        let base_attach = match &single {
            Some(g) if g.font == FontId::Math => self
                .fonts
                .top_accent_attachment(g.glyph)
                .map(|a| a * g.size)
                .unwrap_or(base.width / 2.0),
            _ => base.width / 2.0,
        };
        let lift = (base.height - c.accent_base_height * s).max(0.0);
        let acc = self.glyph_id_box(FontId::Math, gid, ch, s);
        let height = base.height.max(m.height * s + lift);
        let (w, d, it) = (base.width, base.depth, base.italic);
        let mut out = LBox::with_children(
            w,
            height,
            d,
            vec![
                Placed {
                    x: 0.0,
                    y: 0.0,
                    b: base,
                },
                Placed {
                    x: base_attach - accent_attach,
                    y: -lift,
                    b: acc,
                },
            ],
        );
        out.italic = it;
        out
    }

    /// `=` with a small `?` over it (`\overset{?}{=}`).
    pub fn overset(&self, over: LBox, base: LBox, st: Style) -> LBox {
        let gap = self.c.stretch_stack_gap_above_min * self.scale(st) * 0.6;
        let y = -(base.height + gap + over.depth);
        vstack_centered(vec![(over, y), (base, 0.0)])
    }

    // ----- slots -----

    /// An empty slot: a faint dotted box with a grey label.
    pub fn slot(&self, label: Option<&str>, st: Style, span: Span, path: &NodePath) -> LBox {
        let s = self.scale(st);
        let named = self
            .opts
            .slot_labels
            .iter()
            .find(|(at, _)| *at == span.start)
            .map(|(_, l)| l.as_str());
        let label = named.or(label).filter(|l| !l.is_empty());
        let lab = label.map(|l| {
            let mut e = self.clone_shallow();
            e.color = self.opts.theme.placeholder_label;
            let mut b = e.text(l, FontId::TextItalic, st);
            // Labels are set at 60% of the current size.
            scale_box(&mut b, 0.6);
            b
        });
        let pad = 0.12 * s;
        let lw = lab.as_ref().map(|b| b.width).unwrap_or(0.0);
        let width = (lw + 2.0 * pad).max(0.7 * s);
        let height = 0.72 * s;
        let depth = 0.16 * s;
        let mut children = vec![Placed {
            x: 0.0,
            y: 0.0,
            b: LBox {
                content: Content::Slot(self.opts.theme.placeholder_outline),
                ..LBox::strut(width, height, depth)
            },
        }];
        if let Some(l) = lab {
            let y = (depth - height) / 2.0 + (l.height - l.depth) / 2.0;
            children.push(Placed {
                x: (width - l.width) / 2.0,
                y,
                b: l,
            });
        }
        let b = LBox::with_children(width, height, depth, children);
        match self.hits {
            true => b.with_hit(Hit {
                span,
                path: path.clone(),
                kind: HitKind::Slot,
                label: label.map(str::to_string),
            }),
            false => b,
        }
    }

    fn clone_shallow(&self) -> Engine<'a> {
        Engine {
            fonts: self.fonts,
            c: self.c,
            opts: self.opts,
            color: self.color,
            font: self.font,
            depth: self.depth,
            hits: self.hits,
        }
    }
}

/// Scales a box and everything in it (used for slot labels).
fn scale_box(b: &mut LBox, k: f32) {
    b.width *= k;
    b.height *= k;
    b.depth *= k;
    b.italic *= k;
    match &mut b.content {
        Content::Glyph(g) => g.size *= k,
        Content::List(children) => {
            for c in children {
                c.x *= k;
                c.y *= k;
                scale_box(&mut c.b, k);
            }
        }
        _ => {}
    }
}
