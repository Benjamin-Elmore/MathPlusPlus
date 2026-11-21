//! The embedded fonts and everything read from them.
//!
//! Latin Modern Math (with its OpenType `MATH` table) sets the maths; Latin
//! Modern Roman sets `\text{…}`, labels and the prose of the HTML page. Both are
//! under the GUST Font License (`fonts/GUST-FONT-LICENSE.txt`). All metrics come back in **em** (the
//! font's units divided by its units per em), so callers multiply by a size.

use std::sync::OnceLock;

use ttf_parser::gsub::SubstitutionSubtable;
use ttf_parser::math::{GlyphAssembly, GlyphConstruction, MathValue};
use ttf_parser::{Face, GlyphId, OutlineBuilder, Tag};

/// Latin Modern Math (GUST Font License).
pub static LATIN_MODERN_MATH: &[u8] = include_bytes!("../fonts/latinmodern-math.otf");
/// Latin Modern Roman 10 Regular (GUST Font License).
pub static LM_ROMAN: &[u8] = include_bytes!("../fonts/lmroman10-regular.otf");
/// Latin Modern Roman 10 Italic (GUST Font License).
pub static LM_ROMAN_ITALIC: &[u8] = include_bytes!("../fonts/lmroman10-italic.otf");
/// Latin Modern Roman 10 Bold (GUST Font License).
pub static LM_ROMAN_BOLD: &[u8] = include_bytes!("../fonts/lmroman10-bold.otf");
/// Latin Modern Sans 10 Regular (GUST Font License): small interface text.
pub static LM_SANS: &[u8] = include_bytes!("../fonts/lmsans10-regular.otf");
/// Latin Modern Mono 10 Regular (GUST Font License): the file-text pop-out only.
pub static LM_MONO: &[u8] = include_bytes!("../fonts/lmmono10-regular.otf");
/// The licence the fonts are distributed under.
pub static FONT_LICENCE: &str = include_str!("../fonts/GUST-FONT-LICENSE.txt");

/// Which embedded font a glyph comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FontId {
    /// Latin Modern Math.
    Math,
    /// Latin Modern Roman Regular.
    Text,
    /// Latin Modern Roman Italic.
    TextItalic,
    /// Latin Modern Roman Bold.
    TextBold,
}

impl FontId {
    /// Every font, in a fixed order.
    pub const ALL: [FontId; 4] = [
        FontId::Math,
        FontId::Text,
        FontId::TextItalic,
        FontId::TextBold,
    ];

    /// The font's family name, for CSS and SVG text.
    pub fn family(self) -> &'static str {
        match self {
            FontId::Math => "Latin Modern Math",
            _ => "Latin Modern Roman",
        }
    }

    /// A short tag used in SVG element ids.
    pub fn tag(self) -> &'static str {
        match self {
            FontId::Math => "m",
            FontId::Text => "t",
            FontId::TextItalic => "i",
            FontId::TextBold => "b",
        }
    }

    /// The raw font file.
    pub fn bytes(self) -> &'static [u8] {
        match self {
            FontId::Math => LATIN_MODERN_MATH,
            FontId::Text => LM_ROMAN,
            FontId::TextItalic => LM_ROMAN_ITALIC,
            FontId::TextBold => LM_ROMAN_BOLD,
        }
    }
}

/// True when `bytes` starts like an OpenType or TrueType font file.
pub fn is_font_file(bytes: &[u8]) -> bool {
    matches!(
        bytes.get(..4),
        Some(b"OTTO") | Some([0, 1, 0, 0]) | Some(b"true")
    )
}

/// Ink extents of a glyph in em: `height` above the baseline, `depth` below it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GlyphMetrics {
    pub advance: f32,
    pub height: f32,
    pub depth: f32,
    /// Left and right ink edges (for combining accents, which sit left of their origin).
    pub x_min: f32,
    pub x_max: f32,
    /// Bottom and top ink edges, unclamped.
    pub y_min: f32,
    pub y_max: f32,
}

/// One piece of a stretchy glyph assembly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Part {
    pub glyph: u16,
    /// Distance from the previous piece's start to this piece's start, in em.
    pub offset: f32,
}

/// A glyph grown to a size: one variant glyph, or an assembly of pieces.
#[derive(Debug, Clone, PartialEq)]
pub enum Stretched {
    Glyph(u16),
    /// Pieces in order (bottom to top, or left to right) and the total extent in em.
    Assembly {
        parts: Vec<Part>,
        size: f32,
    },
}

/// The kern of the band `height` falls in: the heights (ascending) split the line
/// into bands, band i (below height i) has kern i, and everything at or above the
/// last height has the last kern. Malformed data (a kern missing for a band) gives 0.
pub fn kern_band(heights: &[f32], kerns: &[f32], height: f32) -> f32 {
    let band = heights
        .iter()
        .position(|&h| height < h)
        .unwrap_or(heights.len());
    kerns
        .get(band)
        .copied()
        .filter(|k| k.is_finite())
        .unwrap_or(0.0)
}

/// A corner of a glyph, for [`Fonts::math_kern`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernCorner {
    TopRight,
    TopLeft,
    BottomRight,
    BottomLeft,
}

/// The MATH table constants, in em at the base size.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[allow(missing_docs)]
pub struct MathConstants {
    pub script_scale: f32,
    pub script_script_scale: f32,
    pub delimited_sub_formula_min_height: f32,
    pub display_operator_min_height: f32,
    pub axis_height: f32,
    pub accent_base_height: f32,
    pub subscript_shift_down: f32,
    pub subscript_top_max: f32,
    pub subscript_baseline_drop_min: f32,
    pub superscript_shift_up: f32,
    pub superscript_shift_up_cramped: f32,
    pub superscript_bottom_min: f32,
    pub superscript_baseline_drop_max: f32,
    pub sub_superscript_gap_min: f32,
    pub superscript_bottom_max_with_subscript: f32,
    pub space_after_script: f32,
    pub upper_limit_gap_min: f32,
    pub upper_limit_baseline_rise_min: f32,
    pub lower_limit_gap_min: f32,
    pub lower_limit_baseline_drop_min: f32,
    pub stack_top_shift_up: f32,
    pub stack_top_display_style_shift_up: f32,
    pub stack_bottom_shift_down: f32,
    pub stack_bottom_display_style_shift_down: f32,
    pub stack_gap_min: f32,
    pub stack_display_style_gap_min: f32,
    pub stretch_stack_gap_above_min: f32,
    pub stretch_stack_gap_below_min: f32,
    pub fraction_numerator_shift_up: f32,
    pub fraction_numerator_display_style_shift_up: f32,
    pub fraction_denominator_shift_down: f32,
    pub fraction_denominator_display_style_shift_down: f32,
    pub fraction_numerator_gap_min: f32,
    pub fraction_num_display_style_gap_min: f32,
    pub fraction_rule_thickness: f32,
    pub fraction_denominator_gap_min: f32,
    pub fraction_denom_display_style_gap_min: f32,
    pub overbar_vertical_gap: f32,
    pub overbar_rule_thickness: f32,
    pub overbar_extra_ascender: f32,
    pub radical_vertical_gap: f32,
    pub radical_display_style_vertical_gap: f32,
    pub radical_rule_thickness: f32,
    pub radical_extra_ascender: f32,
    pub radical_kern_before_degree: f32,
    pub radical_kern_after_degree: f32,
    pub radical_degree_bottom_raise: f32,
    pub min_connector_overlap: f32,
}

impl MathConstants {
    /// Values close to Latin Modern Math's own, used if a table were missing.
    fn fallback() -> Self {
        MathConstants {
            script_scale: 0.7,
            script_script_scale: 0.55,
            delimited_sub_formula_min_height: 1.3,
            display_operator_min_height: 1.8,
            axis_height: 0.258,
            accent_base_height: 0.48,
            subscript_shift_down: 0.21,
            subscript_top_max: 0.4,
            subscript_baseline_drop_min: 0.16,
            superscript_shift_up: 0.36,
            superscript_shift_up_cramped: 0.25,
            superscript_bottom_min: 0.12,
            superscript_baseline_drop_max: 0.23,
            sub_superscript_gap_min: 0.15,
            superscript_bottom_max_with_subscript: 0.38,
            space_after_script: 0.04,
            upper_limit_gap_min: 0.15,
            upper_limit_baseline_rise_min: 0.3,
            lower_limit_gap_min: 0.15,
            lower_limit_baseline_drop_min: 0.6,
            stack_top_shift_up: 0.47,
            stack_top_display_style_shift_up: 0.78,
            stack_bottom_shift_down: 0.39,
            stack_bottom_display_style_shift_down: 0.7,
            stack_gap_min: 0.2,
            stack_display_style_gap_min: 0.44,
            stretch_stack_gap_above_min: 0.15,
            stretch_stack_gap_below_min: 0.15,
            fraction_numerator_shift_up: 0.47,
            fraction_numerator_display_style_shift_up: 0.78,
            fraction_denominator_shift_down: 0.39,
            fraction_denominator_display_style_shift_down: 0.7,
            fraction_numerator_gap_min: 0.07,
            fraction_num_display_style_gap_min: 0.2,
            fraction_rule_thickness: 0.068,
            fraction_denominator_gap_min: 0.07,
            fraction_denom_display_style_gap_min: 0.2,
            overbar_vertical_gap: 0.2,
            overbar_rule_thickness: 0.068,
            overbar_extra_ascender: 0.068,
            radical_vertical_gap: 0.085,
            radical_display_style_vertical_gap: 0.17,
            radical_rule_thickness: 0.068,
            radical_extra_ascender: 0.068,
            radical_kern_before_degree: 0.065,
            radical_kern_after_degree: -0.335,
            radical_degree_bottom_raise: 0.55,
            min_connector_overlap: 0.02,
        }
    }
}

/// One parsed font.
struct FontData {
    face: Option<Face<'static>>,
    upem: f32,
}

impl FontData {
    fn load(id: FontId) -> Self {
        let bytes = id.bytes();
        let face = if is_font_file(bytes) {
            Face::parse(bytes, 0).ok()
        } else {
            None
        };
        let upem = face
            .as_ref()
            .map(|f| f32::from(f.units_per_em()))
            .filter(|u| *u > 0.0)
            .unwrap_or(1000.0);
        FontData { face, upem }
    }
}

/// The embedded fonts, parsed once.
pub struct Fonts {
    data: [FontData; 4],
    constants: MathConstants,
}

impl std::fmt::Debug for Fonts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fonts")
            .field(
                "loaded",
                &self
                    .data
                    .iter()
                    .map(|d| d.face.is_some())
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

static FONTS: OnceLock<Fonts> = OnceLock::new();

impl Fonts {
    /// The shared, lazily parsed fonts.
    pub fn get() -> &'static Fonts {
        FONTS.get_or_init(|| {
            let data = FontId::ALL.map(FontData::load);
            let constants = data[0]
                .face
                .as_ref()
                .and_then(|f| read_constants(f, data[0].upem))
                .unwrap_or_else(MathConstants::fallback);
            Fonts { data, constants }
        })
    }

    fn data(&self, id: FontId) -> &FontData {
        match id {
            FontId::Math => &self.data[0],
            FontId::Text => &self.data[1],
            FontId::TextItalic => &self.data[2],
            FontId::TextBold => &self.data[3],
        }
    }

    fn face(&self, id: FontId) -> Option<&Face<'static>> {
        self.data(id).face.as_ref()
    }

    /// True when the font parsed.
    pub fn is_loaded(&self, id: FontId) -> bool {
        self.face(id).is_some()
    }

    /// The MATH table constants.
    pub fn constants(&self) -> &MathConstants {
        &self.constants
    }

    fn em(&self, id: FontId, units: f32) -> f32 {
        units / self.data(id).upem
    }

    /// The glyph for a character, if the font has it.
    pub fn glyph(&self, id: FontId, c: char) -> Option<u16> {
        self.face(id)?.glyph_index(c).map(|g| g.0)
    }

    /// Ink extents and advance of a glyph, in em.
    pub fn metrics(&self, id: FontId, glyph: u16) -> GlyphMetrics {
        let Some(face) = self.face(id) else {
            return GlyphMetrics::default();
        };
        let g = GlyphId(glyph);
        let advance = self.em(id, f32::from(face.glyph_hor_advance(g).unwrap_or(0)));
        match face.glyph_bounding_box(g) {
            Some(r) => GlyphMetrics {
                advance,
                height: self.em(id, f32::from(r.y_max)).max(0.0),
                depth: self.em(id, -f32::from(r.y_min)).max(0.0),
                x_min: self.em(id, f32::from(r.x_min)),
                x_max: self.em(id, f32::from(r.x_max)),
                y_min: self.em(id, f32::from(r.y_min)),
                y_max: self.em(id, f32::from(r.y_max)),
            },
            None => GlyphMetrics {
                advance,
                ..GlyphMetrics::default()
            },
        }
    }

    fn math_value(&self, v: Option<MathValue<'_>>) -> Option<f32> {
        v.map(|v| self.em(FontId::Math, f32::from(v.value)))
    }

    /// The italic correction of a maths glyph, in em.
    pub fn italic_correction(&self, glyph: u16) -> f32 {
        let info = self
            .face(FontId::Math)
            .and_then(|f| f.tables().math)
            .and_then(|m| m.glyph_info);
        let v = info
            .and_then(|i| i.italic_corrections)
            .and_then(|ic| ic.get(GlyphId(glyph)));
        self.math_value(v).unwrap_or(0.0)
    }

    /// The MATH table's cut-in for a corner of a maths glyph at `height` (em above its
    /// baseline), in em: negative where a script may tuck into the glyph's shape (the
    /// top right of an upright `f`, the bottom right of a `V`). Zero when the font
    /// gives the glyph no kerning at that corner.
    pub fn math_kern(&self, glyph: u16, corner: KernCorner, height: f32) -> f32 {
        let Some(info) = self
            .face(FontId::Math)
            .and_then(|f| f.tables().math)
            .and_then(|m| m.glyph_info)
            .and_then(|i| i.kern_infos)
            .and_then(|k| k.get(GlyphId(glyph)))
        else {
            return 0.0;
        };
        let kern = match corner {
            KernCorner::TopRight => info.top_right,
            KernCorner::TopLeft => info.top_left,
            KernCorner::BottomRight => info.bottom_right,
            KernCorner::BottomLeft => info.bottom_left,
        };
        let Some(kern) = kern else {
            return 0.0;
        };
        let n = kern.count();
        let mut heights = Vec::with_capacity(usize::from(n));
        let mut kerns = Vec::with_capacity(usize::from(n) + 1);
        for i in 0..n {
            match self.math_value(kern.height(i)) {
                Some(h) => heights.push(h),
                None => return 0.0,
            }
        }
        for i in 0..=n {
            match self.math_value(kern.kern(i)) {
                Some(k) => kerns.push(k),
                None => return 0.0,
            }
        }
        kern_band(&heights, &kerns, height)
    }

    /// Where an accent attaches to a maths glyph, in em from its origin.
    pub fn top_accent_attachment(&self, glyph: u16) -> Option<f32> {
        let info = self
            .face(FontId::Math)
            .and_then(|f| f.tables().math)
            .and_then(|m| m.glyph_info)?;
        let v = info.top_accent_attachments?.get(GlyphId(glyph));
        self.math_value(v)
    }

    /// The script-size alternate of a maths glyph (`ssty`, level 1 or 2).
    pub fn script_variant(&self, glyph: u16, level: u8) -> u16 {
        if level == 0 {
            return glyph;
        }
        self.ssty(glyph, level).unwrap_or(glyph)
    }

    fn ssty(&self, glyph: u16, level: u8) -> Option<u16> {
        let gsub = self.face(FontId::Math)?.tables().gsub?;
        let feature = gsub.features.find(Tag::from_bytes(b"ssty"))?;
        let g = GlyphId(glyph);
        for index in feature.lookup_indices {
            let Some(lookup) = gsub.lookups.get(index) else {
                continue;
            };
            for sub in lookup.subtables.into_iter::<SubstitutionSubtable>() {
                match sub {
                    SubstitutionSubtable::Single(single) => {
                        let Some(cov) = single.coverage().get(g) else {
                            continue;
                        };
                        return match single {
                            ttf_parser::gsub::SingleSubstitution::Format1 { delta, .. } => {
                                Some((i32::from(glyph) + i32::from(delta)) as u16)
                            }
                            ttf_parser::gsub::SingleSubstitution::Format2 {
                                substitutes, ..
                            } => substitutes.get(cov).map(|s| s.0),
                        };
                    }
                    SubstitutionSubtable::Alternate(alt) => {
                        let Some(cov) = alt.coverage.get(g) else {
                            continue;
                        };
                        let set = alt.alternate_sets.get(cov)?;
                        let n = set.alternates.len();
                        if n == 0 {
                            return None;
                        }
                        let i = (u16::from(level) - 1).min(n - 1);
                        return set.alternates.get(i).map(|s| s.0);
                    }
                    _ => {}
                }
            }
        }
        None
    }

    fn construction(&self, glyph: u16, vertical: bool) -> Option<GlyphConstruction<'static>> {
        let variants = self.face(FontId::Math)?.tables().math?.variants?;
        let set = if vertical {
            variants.vertical_constructions
        } else {
            variants.horizontal_constructions
        };
        set.get(GlyphId(glyph))
    }

    /// Size variants of a maths glyph, smallest first, with their extent in em.
    pub fn variants(&self, glyph: u16, vertical: bool) -> Vec<(u16, f32)> {
        let mut out = vec![];
        if let Some(c) = self.construction(glyph, vertical) {
            for v in c.variants {
                out.push((
                    v.variant_glyph.0,
                    self.em(FontId::Math, f32::from(v.advance_measurement)),
                ));
            }
        }
        out
    }

    /// The glyph (or assembly) for `glyph` at least `size` em tall (or wide).
    /// Falls back to the largest variant when nothing reaches the size.
    pub fn stretch(&self, glyph: u16, size: f32, vertical: bool) -> Stretched {
        let Some(c) = self.construction(glyph, vertical) else {
            return Stretched::Glyph(glyph);
        };
        let mut last = glyph;
        for v in c.variants {
            last = v.variant_glyph.0;
            // Vertical variants are compared by their ink, which is what has to
            // cover the content (the advance measurement can be larger).
            let extent = if vertical {
                let m = self.metrics(FontId::Math, last);
                m.y_max - m.y_min
            } else {
                self.em(FontId::Math, f32::from(v.advance_measurement))
            };
            if extent >= size {
                return Stretched::Glyph(last);
            }
        }
        match c.assembly {
            Some(a) => self.assemble(a, size).unwrap_or(Stretched::Glyph(last)),
            None => Stretched::Glyph(last),
        }
    }

    fn assemble(&self, a: GlyphAssembly<'_>, size: f32) -> Option<Stretched> {
        let parts: Vec<_> = a.parts.into_iter().collect();
        if parts.is_empty() {
            return None;
        }
        let min_overlap = self.constants.min_connector_overlap;
        let units = |v: u16| self.em(FontId::Math, f32::from(v));
        // Find how many repeats of the extenders reach the size.
        let mut repeats = 0usize;
        let expand = |repeats: usize| -> Vec<ttf_parser::math::GlyphPart> {
            let mut v = vec![];
            for p in &parts {
                let n = if p.part_flags.extender() { repeats } else { 1 };
                v.extend(std::iter::repeat_n(*p, n));
            }
            v
        };
        let has_extender = parts.iter().any(|p| p.part_flags.extender());
        let mut seq = expand(0);
        loop {
            let total: f32 = seq.iter().map(|p| units(p.full_advance)).sum::<f32>()
                - min_overlap * seq.len().saturating_sub(1) as f32;
            if total >= size || !has_extender || repeats >= 200 {
                break;
            }
            repeats += 1;
            seq = expand(repeats);
        }
        if seq.is_empty() {
            return None;
        }
        // Spread the overlap evenly, within what each joint allows.
        let joints = seq.len().saturating_sub(1);
        let full: f32 = seq.iter().map(|p| units(p.full_advance)).sum();
        let mut overlap = if joints > 0 {
            ((full - size) / joints as f32).max(min_overlap)
        } else {
            0.0
        };
        for w in seq.windows(2) {
            let allowed = units(w[0].end_connector_length.min(w[1].start_connector_length));
            overlap = overlap.min(allowed.max(min_overlap));
        }
        let mut out = Vec::with_capacity(seq.len());
        let mut prev_adv = 0.0;
        for (i, p) in seq.iter().enumerate() {
            let offset = if i == 0 { 0.0 } else { prev_adv - overlap };
            out.push(Part {
                glyph: p.glyph_id.0,
                offset,
            });
            prev_adv = units(p.full_advance);
        }
        let total = full - overlap * joints as f32;
        Some(Stretched::Assembly {
            parts: out,
            size: total,
        })
    }

    /// The italic correction of an assembled glyph (used for big integrals).
    pub fn assembly_italic(&self, glyph: u16, vertical: bool) -> f32 {
        let v = self
            .construction(glyph, vertical)
            .and_then(|c| c.assembly)
            .map(|a| a.italics_correction);
        self.math_value(v).unwrap_or(0.0)
    }

    /// The glyph outline as SVG path data in font units (y up), if it has one.
    pub fn outline(&self, id: FontId, glyph: u16) -> Option<String> {
        let face = self.face(id)?;
        let mut b = SvgPathBuilder::default();
        face.outline_glyph(GlyphId(glyph), &mut b)?;
        Some(b.out)
    }

    /// Units per em of a font.
    pub fn units_per_em(&self, id: FontId) -> f32 {
        self.data(id).upem
    }
}

fn read_constants(face: &Face<'_>, upem: f32) -> Option<MathConstants> {
    let math = face.tables().math?;
    let c = math.constants?;
    let v = |m: MathValue<'_>| f32::from(m.value) / upem;
    let u = |x: u16| f32::from(x) / upem;
    let min_overlap = math
        .variants
        .map(|vv| u(vv.min_connector_overlap))
        .unwrap_or(0.0);
    let pct = |p: i16, default: f32| {
        if p > 0 {
            f32::from(p) / 100.0
        } else {
            default
        }
    };
    Some(MathConstants {
        script_scale: pct(c.script_percent_scale_down(), 0.7),
        script_script_scale: pct(c.script_script_percent_scale_down(), 0.5),
        delimited_sub_formula_min_height: u(c.delimited_sub_formula_min_height()),
        display_operator_min_height: u(c.display_operator_min_height()),
        axis_height: v(c.axis_height()),
        accent_base_height: v(c.accent_base_height()),
        subscript_shift_down: v(c.subscript_shift_down()),
        subscript_top_max: v(c.subscript_top_max()),
        subscript_baseline_drop_min: v(c.subscript_baseline_drop_min()),
        superscript_shift_up: v(c.superscript_shift_up()),
        superscript_shift_up_cramped: v(c.superscript_shift_up_cramped()),
        superscript_bottom_min: v(c.superscript_bottom_min()),
        superscript_baseline_drop_max: v(c.superscript_baseline_drop_max()),
        sub_superscript_gap_min: v(c.sub_superscript_gap_min()),
        superscript_bottom_max_with_subscript: v(c.superscript_bottom_max_with_subscript()),
        space_after_script: v(c.space_after_script()),
        upper_limit_gap_min: v(c.upper_limit_gap_min()),
        upper_limit_baseline_rise_min: v(c.upper_limit_baseline_rise_min()),
        lower_limit_gap_min: v(c.lower_limit_gap_min()),
        lower_limit_baseline_drop_min: v(c.lower_limit_baseline_drop_min()),
        stack_top_shift_up: v(c.stack_top_shift_up()),
        stack_top_display_style_shift_up: v(c.stack_top_display_style_shift_up()),
        stack_bottom_shift_down: v(c.stack_bottom_shift_down()),
        stack_bottom_display_style_shift_down: v(c.stack_bottom_display_style_shift_down()),
        stack_gap_min: v(c.stack_gap_min()),
        stack_display_style_gap_min: v(c.stack_display_style_gap_min()),
        stretch_stack_gap_above_min: v(c.stretch_stack_gap_above_min()),
        stretch_stack_gap_below_min: v(c.stretch_stack_gap_below_min()),
        fraction_numerator_shift_up: v(c.fraction_numerator_shift_up()),
        fraction_numerator_display_style_shift_up: v(c.fraction_numerator_display_style_shift_up()),
        fraction_denominator_shift_down: v(c.fraction_denominator_shift_down()),
        fraction_denominator_display_style_shift_down: v(
            c.fraction_denominator_display_style_shift_down()
        ),
        fraction_numerator_gap_min: v(c.fraction_numerator_gap_min()),
        fraction_num_display_style_gap_min: v(c.fraction_num_display_style_gap_min()),
        fraction_rule_thickness: v(c.fraction_rule_thickness()),
        fraction_denominator_gap_min: v(c.fraction_denominator_gap_min()),
        fraction_denom_display_style_gap_min: v(c.fraction_denom_display_style_gap_min()),
        overbar_vertical_gap: v(c.overbar_vertical_gap()),
        overbar_rule_thickness: v(c.overbar_rule_thickness()),
        overbar_extra_ascender: v(c.overbar_extra_ascender()),
        radical_vertical_gap: v(c.radical_vertical_gap()),
        radical_display_style_vertical_gap: v(c.radical_display_style_vertical_gap()),
        radical_rule_thickness: v(c.radical_rule_thickness()),
        radical_extra_ascender: v(c.radical_extra_ascender()),
        radical_kern_before_degree: v(c.radical_kern_before_degree()),
        radical_kern_after_degree: v(c.radical_kern_after_degree()),
        radical_degree_bottom_raise: pct(c.radical_degree_bottom_raise_percent(), 0.6),
        min_connector_overlap: min_overlap,
    })
}

/// Collects a glyph outline as SVG path data.
#[derive(Default)]
struct SvgPathBuilder {
    out: String,
}

impl SvgPathBuilder {
    fn push(&mut self, cmd: char, coords: &[f32]) {
        use std::fmt::Write;
        self.out.push(cmd);
        for (i, c) in coords.iter().enumerate() {
            if i > 0 {
                self.out.push(' ');
            }
            let _ = write!(self.out, "{}", c.round() as i32);
        }
    }
}

impl OutlineBuilder for SvgPathBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.push('M', &[x, y]);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.push('L', &[x, y]);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.push('Q', &[x1, y1, x, y]);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.push('C', &[x1, y1, x2, y2, x, y]);
    }
    fn close(&mut self) {
        self.out.push('Z');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_files_are_fonts() {
        for id in FontId::ALL {
            assert!(is_font_file(id.bytes()), "{id:?} is not an OpenType file");
            assert!(Fonts::get().is_loaded(id), "{id:?} did not parse");
        }
        assert!(FONT_LICENCE.contains("GUST Font License"));
    }

    #[test]
    fn math_table_is_read() {
        let f = Fonts::get();
        let c = f.constants();
        assert!(c.axis_height > 0.2 && c.axis_height < 0.3);
        assert!(c.script_scale > 0.6 && c.script_scale < 0.8);
        assert!(c.fraction_rule_thickness > 0.0);
        let sum = f.glyph(FontId::Math, '∑').unwrap();
        assert!(f.variants(sum, true).len() >= 2);
        let paren = f.glyph(FontId::Math, '(').unwrap();
        match f.stretch(paren, 6.0, true) {
            Stretched::Assembly { size, parts } => {
                assert!(size >= 6.0 - 1e-3);
                assert!(parts.len() >= 3);
            }
            Stretched::Glyph(_) => panic!("a 6em paren should be assembled"),
        }
    }

    #[test]
    fn math_kern_bands() {
        // Two heights, three bands.
        let h = [0.2, 0.5];
        let k = [-0.1, -0.05, 0.0];
        assert_eq!(kern_band(&h, &k, 0.0), -0.1);
        assert_eq!(kern_band(&h, &k, 0.3), -0.05);
        assert_eq!(kern_band(&h, &k, 0.5), 0.0);
        assert_eq!(kern_band(&h, &k, 9.0), 0.0);
        // No heights: one band.
        assert_eq!(kern_band(&[], &[-0.2], 0.4), -0.2);
        // A kern missing for the band is no kerning, never a guess.
        assert_eq!(kern_band(&h, &k[..2], 0.9), 0.0);
    }

    #[test]
    fn glyphs_without_math_kerning_kern_nothing() {
        let f = Fonts::get();
        // Latin Modern Math has no MathKernInfo table, so every cut-in is zero and
        // scripts sit where the italic correction puts them.
        for c in ['f', 'V', 'P', 'x'] {
            let g = f.glyph(FontId::Math, c).unwrap();
            assert_eq!(f.math_kern(g, KernCorner::TopRight, 0.5), 0.0);
            assert_eq!(f.math_kern(g, KernCorner::BottomRight, -0.1), 0.0);
        }
        assert_eq!(f.math_kern(u16::MAX, KernCorner::TopLeft, 0.5), 0.0);
    }

    #[test]
    fn script_variants_exist() {
        let f = Fonts::get();
        let x = f.glyph(FontId::Math, '\u{1D465}').unwrap();
        // Latin Modern Math has optical script sizes for letters.
        assert_ne!(f.script_variant(x, 1), x);
    }

    #[test]
    fn outlines_are_paths() {
        let f = Fonts::get();
        let g = f.glyph(FontId::Math, 'x').unwrap();
        let d = f.outline(FontId::Math, g).unwrap();
        assert!(d.starts_with('M') && d.contains('Z'));
    }
}
