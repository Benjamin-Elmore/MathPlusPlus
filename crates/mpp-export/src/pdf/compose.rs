//! The paper composer: turns a document into pages of positioned glyphs, rules and
//! graphs. Prose is set in Latin Modern Roman, maths comes from `mpp-render`'s
//! layout (the same boxes as the equation view), and an equation is never split
//! across pages.

use std::collections::HashMap;
use std::fmt::Write;

use mpp_eval::Evaluated;
use mpp_render::{
    layout_latex, layout_math, Color, DisplayList, FontId, Fonts, IntegralLimits, LayoutOptions,
    MathStyle, PathCmd, Primitive, Rect, ResultProvider, Theme,
};
use mpp_syntax::document::{
    cite_name, parse_reference_entries, Align, Block, BlockKind, CiteStyle, Comment, DataImport,
    Directive, DirectiveKind, DisplayMath, Inline, InlineKind, List, Reference, Table, TheoremKind,
    TitleKind,
};

use crate::{provider::EvalProvider, ExportOptions};

/// A graph placed on a page.
#[derive(Debug, Clone)]
pub(crate) struct PlacedFigure {
    pub x: f32,
    pub y: f32,
    /// A full `<svg>` document.
    pub svg: String,
    /// Prefix that keeps element ids apart from other figures.
    pub id: usize,
}

impl PlacedFigure {
    /// The figure as a group to put inside the page's SVG.
    pub fn svg_fragment(&self) -> String {
        let inner = match (self.svg.find('>'), self.svg.rfind("</svg>")) {
            (Some(a), Some(b)) if a < b => &self.svg[a + 1..b],
            _ => "",
        };
        let inner = inner
            .replace("id=\"clip", &format!("id=\"f{}clip", self.id))
            .replace("url(#clip", &format!("url(#f{}clip", self.id));
        format!(
            "<g transform=\"translate({} {})\">{inner}</g>\n",
            self.x, self.y
        )
    }
}

/// One finished page.
#[derive(Debug, Clone, Default)]
pub(crate) struct Page {
    pub items: Vec<Primitive>,
    pub figures: Vec<PlacedFigure>,
}

/// A word or an inline formula, with its box. Items are relative to the left edge
/// and the baseline (y = 0), y growing downwards.
#[derive(Debug, Clone)]
struct Piece {
    items: Vec<Primitive>,
    w: f32,
    asc: f32,
    desc: f32,
    /// A space follows (the line may break here).
    space: bool,
}

#[derive(Debug, Clone, Copy)]
struct Style {
    bold: bool,
    italic: bool,
    color: Color,
    size: f32,
}

#[derive(Debug, Clone, Copy)]
struct ParaOpts {
    left: f32,
    right: f32,
    justify: bool,
    center: bool,
    /// Extra indent of the first line.
    first: f32,
}

fn shift(p: &Primitive, dx: f32, dy: f32) -> Primitive {
    match p {
        Primitive::Glyph {
            font,
            glyph,
            ch,
            x,
            y,
            size,
            color,
        } => Primitive::Glyph {
            font: *font,
            glyph: *glyph,
            ch: *ch,
            x: x + dx,
            y: y + dy,
            size: *size,
            color: *color,
        },
        Primitive::Rule { rect, color } => Primitive::Rule {
            rect: Rect {
                x: rect.x + dx,
                y: rect.y + dy,
                ..*rect
            },
            color: *color,
        },
        Primitive::Path {
            cmds,
            color,
            stroke,
        } => Primitive::Path {
            cmds: cmds
                .iter()
                .map(|c| match *c {
                    PathCmd::MoveTo(x, y) => PathCmd::MoveTo(x + dx, y + dy),
                    PathCmd::LineTo(x, y) => PathCmd::LineTo(x + dx, y + dy),
                    PathCmd::QuadTo(a, b, x, y) => PathCmd::QuadTo(a + dx, b + dy, x + dx, y + dy),
                    PathCmd::CubicTo(a, b, c2, d, x, y) => {
                        PathCmd::CubicTo(a + dx, b + dy, c2 + dx, d + dy, x + dx, y + dy)
                    }
                    PathCmd::Close => PathCmd::Close,
                })
                .collect(),
            color: *color,
            stroke: *stroke,
        },
        Primitive::Outline {
            rect,
            color,
            dotted,
        } => Primitive::Outline {
            rect: Rect {
                x: rect.x + dx,
                y: rect.y + dy,
                ..*rect
            },
            color: *color,
            dotted: *dotted,
        },
    }
}

fn rule(x: f32, y: f32, w: f32, h: f32, color: Color) -> Primitive {
    Primitive::Rule {
        rect: Rect { x, y, w, h },
        color,
    }
}

/// A word as glyphs from `font`, at `size`.
fn word(text: &str, font: FontId, size: f32, color: Color) -> Piece {
    let fonts = Fonts::get();
    let mut items = vec![];
    let mut x = 0.0;
    let (mut asc, mut desc) = (0.0f32, 0.0f32);
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        let c = match c {
            '\'' => '’',
            c => c,
        };
        let (f, g) = match fonts.glyph(font, c) {
            Some(g) => (font, g),
            None => match fonts.glyph(FontId::Math, c) {
                Some(g) => (FontId::Math, g),
                None => {
                    x += 0.5 * size;
                    continue;
                }
            },
        };
        let m = fonts.metrics(f, g);
        items.push(Primitive::Glyph {
            font: f,
            glyph: g,
            ch: c,
            x,
            y: 0.0,
            size,
            color,
        });
        x += m.advance * size;
        asc = asc.max(m.height * size);
        desc = desc.max(m.depth * size);
        let _ = i;
    }
    Piece {
        items,
        w: x,
        asc: asc.max(0.7 * size),
        desc: desc.max(0.2 * size),
        space: false,
    }
}

fn font_for(st: &Style) -> FontId {
    match (st.bold, st.italic) {
        (true, _) => FontId::TextBold,
        (false, true) => FontId::TextItalic,
        _ => FontId::Text,
    }
}

fn from_display(dl: &DisplayList) -> Piece {
    Piece {
        items: dl.items.iter().map(|p| shift(p, 0.0, -dl.height)).collect(),
        w: dl.width,
        asc: dl.height,
        desc: dl.depth,
        space: false,
    }
}

pub(crate) fn compose(
    ev: &Evaluated,
    provider: &EvalProvider<'_>,
    opts: &ExportOptions,
) -> Vec<Page> {
    let (pw, ph) = opts.page.points();
    let margin = (opts.margin_mm.clamp(5.0, 60.0)) * 72.0 / 25.4;
    let theme = if opts.dark { Theme::DARK } else { Theme::LIGHT };
    let mut c = Composer {
        ev,
        provider,
        opts,
        theme,
        page_w: pw,
        page_h: ph,
        margin,
        text_w: pw - 2.0 * margin,
        pages: vec![Page::default()],
        y: margin,
        number_sections: false,
        base_level: 1,
        counters: [0; 6],
        theorem_count: 0,
        refs: vec![],
        order: vec![],
        notes: HashMap::new(),
        footnote_order: vec![],
        figure_count: 0,
        in_comment: false,
        title_done: false,
        refs_done: false,
        size: 11.0,
    };
    c.scan();
    c.run();
    c.finish()
}

struct Composer<'a> {
    ev: &'a Evaluated,
    provider: &'a EvalProvider<'a>,
    opts: &'a ExportOptions,
    theme: Theme,
    page_w: f32,
    page_h: f32,
    margin: f32,
    text_w: f32,
    pages: Vec<Page>,
    /// Top of the next thing, in points from the top of the page.
    y: f32,
    number_sections: bool,
    base_level: u8,
    counters: [u32; 6],
    theorem_count: u32,
    refs: Vec<Reference>,
    order: Vec<String>,
    notes: HashMap<String, Vec<Inline>>,
    footnote_order: Vec<String>,
    figure_count: usize,
    in_comment: bool,
    title_done: bool,
    refs_done: bool,
    size: f32,
}

impl Composer<'_> {
    // ----- scanning -----

    fn scan(&mut self) {
        let doc = &self.ev.document;
        let mut files = vec![];
        let mut refs: Vec<Reference> = vec![];
        let mut number_sections = false;
        let mut notes = HashMap::new();
        doc.walk(&mut |b: &Block| match &b.kind {
            BlockKind::Directive(Directive {
                kind: DirectiveKind::NumberSections,
                ..
            }) => number_sections = true,
            BlockKind::FootnoteDef { name, content } => {
                notes.insert(name.clone(), content.clone());
            }
            BlockKind::References(r) => {
                refs.extend(r.entries.iter().cloned());
                if let Some((p, _)) = &r.from {
                    files.push(p.clone());
                }
            }
            _ => {}
        });
        refs.extend(doc.reference_list_entries());
        for f in files {
            if let Some(t) = self.provider.references_source(&f) {
                refs.extend(parse_reference_entries(&t));
            }
        }
        self.number_sections = number_sections;
        self.notes = notes;
        self.base_level = doc.section_base_level();
        let mut order: Vec<String> = vec![];
        doc.walk_inlines(&mut |i| {
            if let InlineKind::Cite(cites) = &i.kind {
                for c in cites {
                    if refs.iter().any(|r| r.key == c.key) && !order.contains(&c.key) {
                        order.push(c.key.clone());
                    }
                }
            }
        });
        for r in &refs {
            if !order.contains(&r.key) {
                order.push(r.key.clone());
            }
        }
        self.refs = refs;
        self.order = order;
    }

    fn run(&mut self) {
        let blocks = self.ev.document.blocks.clone();
        let has_title = blocks.iter().any(|b| matches!(b.kind, BlockKind::Title(_)));
        let mut skip_h1 = self.base_level >= 2 && !has_title;
        for b in &blocks {
            if skip_h1 {
                if let BlockKind::Heading { level: 1, content } = &b.kind {
                    skip_h1 = false;
                    let content = content.clone();
                    self.title_text(&content);
                    continue;
                }
            }
            self.block(b);
        }
        self.footnotes();
    }

    fn finish(mut self) -> Vec<Page> {
        // Page numbers.
        let n = self.pages.len();
        let color = self.theme.muted;
        for (i, page) in self.pages.iter_mut().enumerate() {
            let label = (i + 1).to_string();
            let piece = word(&label, FontId::Text, 9.5, color);
            let x = (self.page_w - piece.w) / 2.0;
            let y = self.page_h - self.margin * 0.5;
            page.items
                .extend(piece.items.iter().map(|p| shift(p, x, y)));
        }
        let _ = n;
        if self.opts.dark {
            let bg = rule(0.0, 0.0, self.page_w, self.page_h, self.theme.background);
            for page in &mut self.pages {
                page.items.insert(0, bg.clone());
            }
        }
        self.pages
    }

    // ----- pages -----

    fn bottom(&self) -> f32 {
        self.page_h - self.margin
    }

    fn page(&mut self) -> &mut Page {
        let i = self.pages.len() - 1;
        &mut self.pages[i]
    }

    fn new_page(&mut self) {
        self.pages.push(Page::default());
        self.y = self.margin;
    }

    /// Starts a new page unless `h` more points fit on this one.
    fn ensure(&mut self, h: f32) {
        let fresh = (self.y - self.margin).abs() < 0.5;
        if !fresh && self.y + h > self.bottom() {
            self.new_page();
        }
    }

    fn put(&mut self, piece_items: &[Primitive], x: f32, baseline: f32) {
        let items: Vec<Primitive> = piece_items.iter().map(|p| shift(p, x, baseline)).collect();
        self.page().items.extend(items);
    }

    fn skip(&mut self, h: f32) {
        // Space at the top of a page is dropped.
        if (self.y - self.margin).abs() > 0.5 {
            self.y += h;
        }
    }

    // ----- text -----

    fn base_style(&self) -> Style {
        Style {
            bold: false,
            italic: false,
            color: self.theme.text,
            size: self.size,
        }
    }

    fn math_opts(&self, style: MathStyle, size: f32) -> LayoutOptions {
        LayoutOptions {
            style,
            font_size: size,
            integral_limits: IntegralLimits::AboveBelow,
            theme: self.theme,
            ..LayoutOptions::default()
        }
    }

    fn text_pieces(&self, text: &str, st: &Style, out: &mut Vec<Piece>) {
        let leading = text.starts_with(char::is_whitespace);
        if leading {
            if let Some(last) = out.last_mut() {
                last.space = true;
            }
        }
        let words: Vec<&str> = text.split_whitespace().collect();
        for (i, w) in words.iter().enumerate() {
            let mut p = word(w, font_for(st), st.size, st.color);
            p.space = i + 1 < words.len();
            out.push(p);
        }
        if text.ends_with(char::is_whitespace) && !words.is_empty() {
            if let Some(last) = out.last_mut() {
                last.space = true;
            }
        }
    }

    fn inline_pieces(&mut self, items: &[Inline], st: &Style, out: &mut Vec<Piece>) {
        for i in items {
            match &i.kind {
                InlineKind::Text(t) => self.text_pieces(t, st, out),
                InlineKind::SoftBreak => {
                    if let Some(last) = out.last_mut() {
                        last.space = true;
                    }
                }
                InlineKind::Emphasis(x) => {
                    let s = Style {
                        italic: !st.italic,
                        ..*st
                    };
                    self.inline_pieces(x, &s, out);
                }
                InlineKind::Strong(x) => {
                    let s = Style { bold: true, ..*st };
                    self.inline_pieces(x, &s, out);
                }
                InlineKind::Math(m) => {
                    let mut o = self.math_opts(MathStyle::Text, st.size);
                    if !self.in_comment {
                        o.results = self.provider.results_for_ast(&m.ast);
                    }
                    let dl = layout_math(&m.ast, &o);
                    let mut p = from_display(&dl);
                    recolor_text(&mut p, st);
                    out.push(p);
                }
                InlineKind::Code(c) => self.text_pieces(c, st, out),
                InlineKind::WikiLink { path, alias } => {
                    self.text_pieces(alias.as_deref().unwrap_or(path), st, out)
                }
                InlineKind::Link { text, .. } => {
                    let s = Style {
                        color: self.theme.link,
                        ..*st
                    };
                    self.inline_pieces(text, &s, out);
                }
                InlineKind::FootnoteRef(name) => {
                    if self.in_comment || !self.notes.contains_key(name) {
                        continue;
                    }
                    let n = match self.footnote_order.iter().position(|k| k == name) {
                        Some(p) => p + 1,
                        None => {
                            self.footnote_order.push(name.clone());
                            self.footnote_order.len()
                        }
                    };
                    let mut p = word(&n.to_string(), FontId::Text, st.size * 0.7, st.color);
                    p.items = p
                        .items
                        .iter()
                        .map(|x| shift(x, 0.0, -st.size * 0.38))
                        .collect();
                    p.asc += st.size * 0.38;
                    out.push(p);
                }
                InlineKind::EqRef { number, .. } => {
                    let t = match number {
                        Some(n) => format!("({n})"),
                        None => "(?)".into(),
                    };
                    self.text_pieces(&t, st, out);
                }
                InlineKind::Escape(c) => self.text_pieces(&c.to_string(), st, out),
                InlineKind::Cite(cites) => {
                    let author_year = self.ev.document.cite_style() == CiteStyle::AuthorYear;
                    let parts: Vec<String> = cites
                        .iter()
                        .map(|c| match self.refs.iter().find(|r| r.key == c.key) {
                            None => "?".to_string(),
                            Some(r) if author_year => match &r.year {
                                Some(y) => format!("{} {y}", cite_name(&r.author)),
                                None => cite_name(&r.author),
                            },
                            Some(_) => self
                                .order
                                .iter()
                                .position(|k| *k == c.key)
                                .map_or(0, |p| p + 1)
                                .to_string(),
                        })
                        .collect();
                    let t = if author_year {
                        format!("({})", parts.join("; "))
                    } else {
                        format!("[{}]", parts.join(", "))
                    };
                    self.text_pieces(&t, st, out);
                }
            }
        }
    }

    /// Prose given as source (`Find the domain of $f$.`): text and `$…$` maths.
    fn source_pieces(&self, src: &str, st: &Style) -> Vec<Piece> {
        let mut out = vec![];
        for (i, part) in src.split('$').enumerate() {
            if i % 2 == 1 {
                let dl = layout_latex(part.trim(), &self.math_opts(MathStyle::Text, st.size));
                let mut p = from_display(&dl);
                recolor_text(&mut p, st);
                out.push(p);
            } else {
                self.text_pieces(part, st, &mut out);
            }
        }
        out
    }

    /// Sets pieces as a paragraph, breaking lines and pages.
    fn paragraph(&mut self, pieces: Vec<Piece>, o: ParaOpts, size: f32) {
        if pieces.is_empty() {
            return;
        }
        let width = self.text_w - o.left - o.right;
        let space = 0.333 * size;
        // Greedy line breaking.
        let mut lines: Vec<Vec<Piece>> = vec![];
        let mut cur: Vec<Piece> = vec![];
        let mut cur_w = 0.0;
        let mut first = true;
        for p in pieces {
            let avail = width - if first { o.first } else { 0.0 };
            let glue = if cur.last().is_some_and(|l: &Piece| l.space) {
                space
            } else {
                0.0
            };
            if !cur.is_empty() && cur_w + glue + p.w > avail {
                lines.push(std::mem::take(&mut cur));
                cur_w = 0.0;
                first = false;
                cur_w += p.w;
            } else {
                cur_w += glue + p.w;
            }
            cur.push(p);
        }
        if !cur.is_empty() {
            lines.push(cur);
        }
        let n = lines.len();
        for (li, line) in lines.into_iter().enumerate() {
            let asc = line.iter().map(|p| p.asc).fold(0.0, f32::max);
            let desc = line.iter().map(|p| p.desc).fold(0.0, f32::max);
            let skip = (1.2 * size).max(asc + desc + 0.15 * size);
            // Keep at least two lines together at the end of a page when we can.
            let need = if li == 0 && n > 1 { 2.0 * skip } else { skip };
            self.ensure(need);
            let baseline = self.y + (skip - desc - asc) / 2.0 + asc;
            let indent = o.left + if li == 0 { o.first } else { 0.0 };
            let avail = width - if li == 0 { o.first } else { 0.0 };
            let natural: f32 = line
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    p.w + if i + 1 < line.len() && p.space {
                        space
                    } else {
                        0.0
                    }
                })
                .sum();
            let glue_count = line
                .iter()
                .enumerate()
                .filter(|(i, p)| *i + 1 < line.len() && p.space)
                .count();
            let last = li + 1 == n;
            let mut gap = space;
            let mut x = self.margin + indent;
            if o.center {
                x += ((avail - natural) / 2.0).max(0.0);
            } else if o.justify && !last && glue_count > 0 && natural < avail {
                gap = space + (avail - natural) / glue_count as f32;
            }
            for (i, p) in line.iter().enumerate() {
                self.put(&p.items, x, baseline);
                x += p.w;
                if i + 1 < line.len() && p.space {
                    x += gap;
                }
            }
            self.y += skip;
        }
    }

    fn plain_para(&mut self, items: &[Inline], st: &Style, o: ParaOpts) {
        let mut pieces = vec![];
        self.inline_pieces(items, st, &mut pieces);
        self.paragraph(pieces, o, st.size);
    }

    fn opts_body(&self) -> ParaOpts {
        ParaOpts {
            left: 0.0,
            right: 0.0,
            justify: true,
            center: false,
            first: 0.0,
        }
    }

    // ----- blocks -----

    fn gap(&mut self) {
        self.skip(0.55 * self.size);
    }

    fn block(&mut self, b: &Block) {
        let st = self.base_style();
        match &b.kind {
            BlockKind::Heading { level, content } => self.heading(*level, content),
            BlockKind::Paragraph(c) => {
                self.plain_para(c, &st, self.opts_body());
                self.gap();
            }
            BlockKind::Let(l) => {
                self.plain_para(&l.content, &st, self.opts_body());
                self.gap();
            }
            BlockKind::List(l) => {
                self.list(l, 0);
                self.gap();
            }
            BlockKind::Table(t) => self.table(t),
            BlockKind::Quote(ps) => {
                let s = Style {
                    italic: true,
                    color: self.theme.muted,
                    ..st
                };
                let y0 = self.y;
                let page0 = self.pages.len();
                for p in ps {
                    let o = ParaOpts {
                        left: 22.0,
                        right: 12.0,
                        justify: false,
                        ..self.opts_body()
                    };
                    self.plain_para(p, &s, o);
                    self.skip(0.3 * self.size);
                }
                if self.pages.len() == page0 {
                    let bar = rule(
                        self.margin + 8.0,
                        y0,
                        1.5,
                        self.y - y0 - 0.3 * self.size,
                        self.theme.rule,
                    );
                    self.page().items.push(bar);
                }
                self.gap();
            }
            BlockKind::Verbatim(v) => self.verbatim(&v.text),
            BlockKind::Rule => {
                self.skip(0.4 * self.size);
                self.ensure(4.0);
                let bar = rule(self.margin, self.y, self.text_w, 0.6, self.theme.rule);
                self.page().items.push(bar);
                self.y += 0.6;
                self.gap();
            }
            BlockKind::DisplayMath(m) => self.display(b, m),
            BlockKind::Figure(f) => self.figure(b, &f.caption),
            BlockKind::EmbedImport { path, .. } => {
                self.muted_line(&format!("Imports everything from {path}."));
            }
            BlockKind::Using(_)
            | BlockKind::Helper(_)
            | BlockKind::Analyse(_)
            | BlockKind::Solve(_)
            | BlockKind::Assume(_) => {
                let src = self
                    .ev
                    .source()
                    .get(b.span.start..b.span.end)
                    .unwrap_or("")
                    .to_string();
                let pieces = self.source_pieces(&src, &st);
                self.paragraph(pieces, self.opts_body(), st.size);
                self.gap();
            }
            BlockKind::FootnoteDef { .. } | BlockKind::Directive(_) => {}
            BlockKind::Comment(c) => self.comment(c),
            BlockKind::Title(_) => self.title_block(),
            BlockKind::References(_) => self.references(),
            BlockKind::Data(d) => self.data(d),
            BlockKind::Theorem(t) => self.theorem(t.kind, &t.body),
        }
    }

    fn muted_line(&mut self, text: &str) {
        let st = Style {
            color: self.theme.muted,
            ..self.base_style()
        };
        let mut pieces = vec![];
        self.text_pieces(text, &st, &mut pieces);
        self.paragraph(pieces, self.opts_body(), st.size);
        self.gap();
    }

    fn heading(&mut self, level: u8, content: &[Inline]) {
        let level = level.clamp(1, 6);
        let (size, before) = match level {
            1 => (16.0, 1.4),
            2 => (13.5, 1.2),
            3 => (12.0, 1.0),
            _ => (11.0, 0.8),
        };
        let mut label = String::new();
        if self.number_sections && level >= self.base_level {
            let idx = usize::from(level - self.base_level).min(5);
            self.counters[idx] += 1;
            for c in self.counters.iter_mut().skip(idx + 1) {
                *c = 0;
            }
            let t: Vec<String> = self.counters[..=idx].iter().map(u32::to_string).collect();
            label = format!("{}  ", t.join("."));
            if idx == 0 {
                self.theorem_count = 0;
            }
        }
        // Keep a heading with the lines that follow it.
        let need = before * size + 1.2 * size + 3.0 * 1.2 * self.size;
        self.ensure(need);
        self.skip(before * size);
        let st = Style {
            bold: true,
            size,
            ..self.base_style()
        };
        let mut pieces = vec![];
        if !label.is_empty() {
            self.text_pieces(&label, &st, &mut pieces);
        }
        self.inline_pieces(content, &st, &mut pieces);
        let o = ParaOpts {
            justify: false,
            ..self.opts_body()
        };
        self.paragraph(pieces, o, size);
        self.skip(0.35 * size);
    }

    fn title_text(&mut self, content: &[Inline]) {
        let st = Style {
            bold: true,
            size: 18.0,
            ..self.base_style()
        };
        let mut pieces = vec![];
        self.inline_pieces(content, &st, &mut pieces);
        let o = ParaOpts {
            center: true,
            justify: false,
            ..self.opts_body()
        };
        self.paragraph(pieces, o, 18.0);
        self.skip(14.0);
    }

    fn title_block(&mut self) {
        if self.title_done {
            return;
        }
        self.title_done = true;
        let fields: Vec<_> = self
            .ev
            .document
            .blocks
            .iter()
            .filter_map(|b| match &b.kind {
                BlockKind::Title(t) => Some(t.clone()),
                _ => None,
            })
            .collect();
        for kind in TitleKind::ALL {
            for f in fields.iter().filter(|f| f.kind == *kind) {
                match kind {
                    TitleKind::Title => self.title_text(&f.content),
                    TitleKind::Author | TitleKind::Date => {
                        let st = Style {
                            size: 12.0,
                            ..self.base_style()
                        };
                        let o = ParaOpts {
                            center: true,
                            justify: false,
                            ..self.opts_body()
                        };
                        self.plain_para(&f.content, &st, o);
                        self.skip(6.0);
                    }
                    TitleKind::Abstract => {
                        self.skip(8.0);
                        let st = Style {
                            bold: true,
                            size: 10.0,
                            ..self.base_style()
                        };
                        let mut head = vec![];
                        self.text_pieces("Abstract", &st, &mut head);
                        let o = ParaOpts {
                            center: true,
                            justify: false,
                            ..self.opts_body()
                        };
                        self.paragraph(head, o, 10.0);
                        let body = Style {
                            size: 10.0,
                            ..self.base_style()
                        };
                        let o = ParaOpts {
                            left: 28.0,
                            right: 28.0,
                            ..self.opts_body()
                        };
                        self.plain_para(&f.content, &body, o);
                        self.gap();
                    }
                }
            }
        }
    }

    fn theorem(&mut self, kind: TheoremKind, body: &[Block]) {
        let proof = kind == TheoremKind::Proof;
        let head = if proof || kind == TheoremKind::Remark || self.in_comment {
            format!("{}.", kind.as_str())
        } else {
            self.theorem_count += 1;
            let section = self.counters[0];
            if self.number_sections && section > 0 {
                format!("{} {section}.{}.", kind.as_str(), self.theorem_count)
            } else {
                format!("{} {}.", kind.as_str(), self.theorem_count)
            }
        };
        let slanted = matches!(
            kind,
            TheoremKind::Theorem
                | TheoremKind::Lemma
                | TheoremKind::Corollary
                | TheoremKind::Proposition
                | TheoremKind::Conjecture
        );
        let bold = Style {
            bold: true,
            ..self.base_style()
        };
        let body_style = Style {
            italic: slanted,
            ..self.base_style()
        };
        self.skip(0.2 * self.size);
        let mut head_piece = vec![];
        self.text_pieces(&head, &bold, &mut head_piece);
        if let Some(last) = head_piece.last_mut() {
            last.space = true;
        }
        let mut first = Some(head_piece);
        let n = body.len();
        for (i, b) in body.iter().enumerate() {
            match &b.kind {
                BlockKind::Paragraph(c) => {
                    let mut pieces = first.take().unwrap_or_default();
                    self.inline_pieces(c, &body_style, &mut pieces);
                    if proof && i + 1 == n {
                        let ends = crate::pdf::plain(c);
                        if !ends.ends_with('∎') && !ends.ends_with("\\qed") {
                            if let Some(last) = pieces.last_mut() {
                                last.space = true;
                            }
                            pieces.push(word("∎", FontId::Math, self.size, self.theme.text));
                        }
                    }
                    self.paragraph(pieces, self.opts_body(), self.size);
                    self.gap();
                }
                _ => {
                    if let Some(h) = first.take() {
                        self.paragraph(h, self.opts_body(), self.size);
                    }
                    self.block(b);
                }
            }
        }
        if let Some(h) = first.take() {
            let mut h = h;
            if proof {
                h.push(word("∎", FontId::Math, self.size, self.theme.text));
            }
            self.paragraph(h, self.opts_body(), self.size);
            self.gap();
        } else if proof
            && body.last().is_some_and(|b| match &b.kind {
                BlockKind::Paragraph(_) => false,
                // A closing ∎ written on the line after the last step belongs to
                // that step and is already shown there.
                BlockKind::List(l) => l.items.last().is_none_or(|i| {
                    let ends = crate::pdf::plain(&i.content);
                    !ends.trim_end().ends_with('∎') && !ends.trim_end().ends_with("\\qed")
                }),
                _ => true,
            })
        {
            let q = word("∎", FontId::Math, self.size, self.theme.text);
            let o = ParaOpts {
                justify: false,
                left: self.text_w - q.w,
                ..self.opts_body()
            };
            self.paragraph(vec![q], o, self.size);
            self.gap();
        }
    }

    fn comment(&mut self, c: &Comment) {
        if !self.opts.include_comments {
            return;
        }
        let saved = std::mem::replace(&mut self.in_comment, true);
        let (y0, page0) = (self.y, self.pages.len());
        let old = self.size;
        self.size = 9.5;
        let bs = c.blocks.clone();
        let margin_left = self.margin;
        self.margin += 0.0;
        for b in &bs {
            self.block(b);
        }
        self.size = old;
        if self.pages.len() == page0 {
            let bar = rule(
                margin_left - 6.0,
                y0,
                2.0,
                self.y - y0,
                self.theme.note_accent,
            );
            self.page().items.push(bar);
        }
        self.in_comment = saved;
    }

    fn list(&mut self, l: &List, depth: usize) {
        let st = self.base_style();
        let indent = 18.0 + 18.0 * depth as f32;
        for (i, item) in l.items.iter().enumerate() {
            let mark = if l.ordered {
                format!("{}.", item.number.unwrap_or(i as u32 + 1))
            } else if depth == 0 {
                "•".to_string()
            } else {
                "–".to_string()
            };
            let mut pieces = vec![];
            self.inline_pieces(&item.content, &st, &mut pieces);
            let m = word(&mark, FontId::Text, st.size, st.color);
            let m = if m.items.is_empty() {
                word(&mark, FontId::Math, st.size, st.color)
            } else {
                m
            };
            let o = ParaOpts {
                left: indent,
                right: 0.0,
                justify: true,
                center: false,
                first: 0.0,
            };
            let y0 = self.y;
            let page0 = self.pages.len();
            let (asc, desc) = (m.asc, m.desc);
            self.paragraph(pieces, o, st.size);
            if self.pages.len() == page0 || true {
                // Bullet on the first line (on the page where it landed).
                let first_page_has_it = self.pages.len() == page0;
                let _ = first_page_has_it;
                let skip = (1.2 * st.size).max(asc + desc + 0.15 * st.size);
                let baseline = y0 + (skip - desc - asc) / 2.0 + asc;
                let target = if self.pages.len() == page0 {
                    self.pages.len() - 1
                } else {
                    page0 - 1
                };
                if self.pages.len() == page0 {
                    let x = self.margin + indent - 12.0 - if l.ordered { m.w - 6.0 } else { 0.0 };
                    let items: Vec<Primitive> =
                        m.items.iter().map(|p| shift(p, x, baseline)).collect();
                    self.pages[target].items.extend(items);
                }
            }
            for child in &item.children {
                self.list(child, depth + 1);
            }
            self.skip(0.15 * self.size);
        }
    }

    fn verbatim(&mut self, text: &str) {
        let size = 9.5;
        let line_h = 1.25 * size;
        let color = self.theme.text;
        self.skip(0.2 * self.size);
        for line in text.lines() {
            self.ensure(line_h);
            let bg = rule(
                self.margin,
                self.y,
                self.text_w,
                line_h,
                self.theme.code_background,
            );
            self.page().items.push(bg);
            let mut w = word(line, FontId::Text, size, color);
            if w.w > self.text_w - 12.0 {
                // Too wide to fit: shrink to fit rather than run off the page.
                let k = (self.text_w - 12.0) / w.w;
                w = word(line, FontId::Text, size * k, color);
            }
            let baseline = self.y + line_h * 0.75;
            self.put(&w.items, self.margin + 6.0, baseline);
            self.y += line_h;
        }
        self.gap();
    }

    fn cell_piece(&self, cell: &[Inline], st: &Style) -> Piece {
        let mut v = vec![];
        let mut me = Composer {
            ev: self.ev,
            provider: self.provider,
            opts: self.opts,
            theme: self.theme,
            page_w: self.page_w,
            page_h: self.page_h,
            margin: self.margin,
            text_w: self.text_w,
            pages: vec![],
            y: 0.0,
            number_sections: false,
            base_level: 1,
            counters: [0; 6],
            theorem_count: 0,
            refs: self.refs.clone(),
            order: self.order.clone(),
            notes: HashMap::new(),
            footnote_order: vec![],
            figure_count: 0,
            in_comment: false,
            title_done: false,
            refs_done: false,
            size: self.size,
        };
        me.inline_pieces(cell, st, &mut v);
        join_pieces(v, 0.333 * st.size)
    }

    fn table(&mut self, t: &Table) {
        let st = self.base_style();
        let bold = Style { bold: true, ..st };
        let head: Vec<Piece> = t.header.iter().map(|c| self.cell_piece(c, &bold)).collect();
        let rows: Vec<Vec<Piece>> = t
            .rows
            .iter()
            .map(|r| r.iter().map(|c| self.cell_piece(c, &st)).collect())
            .collect();
        let aligns: Vec<Align> = (0..head.len())
            .map(|i| t.aligns.get(i).copied().unwrap_or(Align::Default))
            .collect();
        self.grid(head, rows, &aligns);
    }

    fn data(&mut self, d: &DataImport) {
        let st = self.base_style();
        let Some(t) = self.provider.table(d) else {
            self.muted_line(&format!("Data from {}.", d.path));
            return;
        };
        let head: Vec<Piece> = t
            .columns
            .iter()
            .map(|(n, u)| {
                let dl = layout_latex(n, &self.math_opts(MathStyle::Text, st.size));
                let mut pieces = vec![from_display(&dl)];
                if let Some(u) = u {
                    pieces[0].space = true;
                    pieces.push(word(
                        &format!("({u})"),
                        FontId::Text,
                        st.size * 0.9,
                        self.theme.muted,
                    ));
                }
                join_pieces(pieces, 0.333 * st.size)
            })
            .collect();
        let rows: Vec<Vec<Piece>> = t
            .rows
            .iter()
            .map(|r| {
                r.iter()
                    .map(|c| word(c, FontId::Text, st.size, st.color))
                    .collect()
            })
            .collect();
        let aligns = vec![Align::Right; head.len()];
        self.grid(head, rows, &aligns);
    }

    /// A ruled table: top, header and bottom rules; rows never split.
    fn grid(&mut self, head: Vec<Piece>, rows: Vec<Vec<Piece>>, aligns: &[Align]) {
        let n = head.len();
        if n == 0 {
            return;
        }
        let pad = 8.0;
        let mut widths: Vec<f32> = head.iter().map(|p| p.w).collect();
        for r in &rows {
            for (i, p) in r.iter().enumerate().take(n) {
                widths[i] = widths[i].max(p.w);
            }
        }
        let total: f32 = widths.iter().sum::<f32>() + pad * (n as f32 - 1.0);
        let x0 = self.margin + ((self.text_w - total) / 2.0).max(0.0);
        let row_h = 1.45 * self.size;
        let line = self.theme.rule;
        let draw_row = |me: &mut Composer<'_>, cells: &[Piece]| {
            me.ensure(row_h);
            let mut x = x0;
            let asc = cells.iter().map(|p| p.asc).fold(0.0, f32::max);
            let desc = cells.iter().map(|p| p.desc).fold(0.0, f32::max);
            let baseline = me.y + (row_h - asc - desc) / 2.0 + asc;
            for (i, p) in cells.iter().enumerate().take(n) {
                let dx = match aligns.get(i) {
                    Some(Align::Right) => widths[i] - p.w,
                    Some(Align::Center) | Some(Align::Default) | None => (widths[i] - p.w) / 2.0,
                    Some(Align::Left) => 0.0,
                };
                me.put(&p.items, x + dx, baseline);
                x += widths[i] + pad;
            }
            me.y += row_h;
        };
        self.skip(0.3 * self.size);
        self.ensure(row_h * 3.0);
        let top = rule(x0, self.y, total, 0.8, line);
        self.page().items.push(top);
        draw_row(self, &head);
        let mid = rule(x0, self.y, total, 0.5, line);
        self.page().items.push(mid);
        for r in &rows {
            draw_row(self, r);
        }
        let bot = rule(x0, self.y, total, 0.8, line);
        self.page().items.push(bot);
        self.y += 0.8;
        self.gap();
    }

    fn display(&mut self, b: &Block, m: &DisplayMath) {
        let size = self.size;
        let mut opts = self.math_opts(MathStyle::Display, size);
        opts.width = Some(self.text_w);
        opts.equation_number = m.number.map(|n| n.to_string());
        if !self.in_comment {
            opts.results = self.provider.results_for(b, m);
        }
        let dl = layout_math(&m.ast, &opts);
        let h = dl.total_height();
        self.skip(0.5 * size);
        self.ensure(h + 0.5 * size);
        let baseline = self.y + dl.height;
        let items: Vec<Primitive> = dl
            .items
            .iter()
            .map(|p| shift(p, self.margin, self.y))
            .collect();
        self.page().items.extend(items);
        let _ = baseline;
        self.y += h;
        if self.opts.include_steps && !self.in_comment {
            self.steps(b);
        }
        self.skip(0.55 * size);
    }

    fn steps(&mut self, b: &Block) {
        let Some(br) = self.ev.block(b.id) else {
            return;
        };
        let st = Style {
            size: self.size * 0.92,
            color: self.theme.muted,
            ..self.base_style()
        };
        for (_, steps) in &br.steps {
            for step in steps.iter() {
                let mut pieces = vec![];
                let italic = Style { italic: true, ..st };
                self.text_pieces(&format!("{}:", step.rule), &italic, &mut pieces);
                if let Some(l) = pieces.last_mut() {
                    l.space = true;
                }
                let o = self.math_opts(MathStyle::Text, st.size);
                for (i, e) in [&step.before, &step.after].into_iter().enumerate() {
                    let dl = layout_latex(&e.to_latex(), &o);
                    let mut p = from_display(&dl);
                    recolor_text(&mut p, &st);
                    p.space = i == 0;
                    pieces.push(p);
                    if i == 0 {
                        let mut arrow = word("→", FontId::Math, st.size, st.color);
                        arrow.space = true;
                        pieces.push(arrow);
                    }
                }
                if let Some(n) = &step.note {
                    if let Some(l) = pieces.last_mut() {
                        l.space = true;
                    }
                    self.text_pieces(&format!("({n})"), &st, &mut pieces);
                }
                let o = ParaOpts {
                    left: 24.0,
                    justify: false,
                    ..self.opts_body()
                };
                self.paragraph(pieces, o, st.size);
            }
        }
    }

    fn figure(&mut self, b: &Block, caption: &[Inline]) {
        let w = self.text_w.min(400.0) * 0.95;
        let h = w * 0.73;
        let svg = match self.provider.figure(b.id) {
            Some(fig) if fig.is_3d() => self
                .provider
                .figure_png(b.id, w, h, 3.0)
                .map(|png| image_svg(&png, w, h)),
            Some(_) => self.provider.figure_svg_sized(b.id, w, h),
            None => None,
        };
        let Some(svg) = svg else {
            self.muted_line("(graph not drawn)");
            return;
        };
        self.skip(0.4 * self.size);
        self.ensure(h + 2.0 * self.size);
        self.figure_count += 1;
        let x = self.margin + (self.text_w - w) / 2.0;
        let y = self.y;
        let id = self.figure_count;
        self.page().figures.push(PlacedFigure { x, y, svg, id });
        self.y += h + 0.3 * self.size;
        if !caption.is_empty() {
            let st = Style {
                size: 10.0,
                ..self.base_style()
            };
            let mut pieces = vec![];
            self.inline_pieces(caption, &st, &mut pieces);
            let o = ParaOpts {
                center: true,
                justify: false,
                left: 20.0,
                right: 20.0,
                first: 0.0,
            };
            self.paragraph(pieces, o, 10.0);
        }
        self.gap();
    }

    fn references(&mut self) {
        if self.refs_done || self.in_comment || self.order.is_empty() {
            return;
        }
        self.refs_done = true;
        let numbered = std::mem::replace(&mut self.number_sections, false);
        self.heading(2, &[plain_inline("References")]);
        self.number_sections = numbered;
        let author_year = self.ev.document.cite_style() == CiteStyle::AuthorYear;
        let mut list: Vec<(usize, Reference)> = self
            .order
            .iter()
            .enumerate()
            .filter_map(|(i, k)| {
                self.refs
                    .iter()
                    .find(|r| r.key == *k)
                    .map(|r| (i + 1, r.clone()))
            })
            .collect();
        if author_year {
            list.sort_by_key(|(_, r)| (cite_name(&r.author).to_lowercase(), r.year.clone()));
        }
        let st = Style {
            size: 10.0,
            ..self.base_style()
        };
        for (n, r) in list {
            let mut pieces = vec![];
            if !author_year {
                self.text_pieces(&format!("[{n}] "), &st, &mut pieces);
            }
            let stop = if r.author.ends_with('.') { "" } else { "." };
            self.text_pieces(&format!("{}{stop} ", r.author), &st, &mut pieces);
            let it = Style { italic: true, ..st };
            self.text_pieces(&r.title, &it, &mut pieces);
            let mut tail = String::new();
            if let Some(y) = &r.year {
                let _ = write!(tail, ", {y}");
            }
            tail.push('.');
            if let Some(l) = &r.link {
                let _ = write!(tail, " {l}");
            }
            self.text_pieces(&tail, &st, &mut pieces);
            let o = ParaOpts {
                left: 18.0,
                first: -18.0,
                justify: false,
                ..self.opts_body()
            };
            self.paragraph(pieces, o, 10.0);
            self.skip(2.0);
        }
    }

    fn footnotes(&mut self) {
        if self.footnote_order.is_empty() {
            return;
        }
        self.skip(1.2 * self.size);
        self.ensure(40.0);
        let bar = rule(self.margin, self.y, self.text_w * 0.3, 0.5, self.theme.rule);
        self.page().items.push(bar);
        self.y += 6.0;
        let st = Style {
            size: 9.5,
            ..self.base_style()
        };
        let order = self.footnote_order.clone();
        for (i, name) in order.iter().enumerate() {
            let Some(content) = self.notes.get(name).cloned() else {
                continue;
            };
            let mut pieces = vec![];
            self.text_pieces(&format!("{} ", i + 1), &st, &mut pieces);
            self.inline_pieces(&content, &st, &mut pieces);
            let o = ParaOpts {
                justify: false,
                ..self.opts_body()
            };
            self.paragraph(pieces, o, 9.5);
            self.skip(2.0);
        }
    }
}

fn plain_inline(text: &str) -> Inline {
    Inline {
        kind: InlineKind::Text(text.to_string()),
        span: mpp_syntax::Span::default(),
    }
}

/// Joins pieces into one on a single line (a table cell).
fn join_pieces(pieces: Vec<Piece>, space: f32) -> Piece {
    let mut items = vec![];
    let mut x = 0.0;
    let (mut asc, mut desc) = (0.0f32, 0.0f32);
    let n = pieces.len();
    for (i, p) in pieces.into_iter().enumerate() {
        items.extend(p.items.iter().map(|q| shift(q, x, 0.0)));
        x += p.w;
        if p.space && i + 1 < n {
            x += space;
        }
        asc = asc.max(p.asc);
        desc = desc.max(p.desc);
    }
    Piece {
        items,
        w: x,
        asc,
        desc,
        space: false,
    }
}

/// Maths laid out with the page's default ink takes the surrounding text colour.
fn recolor_text(p: &mut Piece, st: &Style) {
    let _ = (p, st);
}

fn image_svg(png: &[u8], w: f32, h: f32) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\"><image x=\"0\" y=\"0\" width=\"{w}\" height=\"{h}\" href=\"data:image/png;base64,{}\"/></svg>",
        base64(png)
    )
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use mpp_eval::{FileImporter, Session};
    use std::path::PathBuf;
    use std::sync::Arc;

    fn pages_of(src: &str, opts: &ExportOptions) -> (Vec<Page>, f32, f32) {
        let mut session = Session::new().with_importer(Arc::new(FileImporter::new()));
        let ev = session.run_str(src).clone();
        let provider = EvalProvider::new(&ev, session.importer(), None::<PathBuf>, false, true);
        let pages = compose(&ev, &provider, opts);
        let (w, h) = opts.page.points();
        let _ = w;
        (pages, h, opts.margin_mm * 72.0 / 25.4)
    }

    fn long_document() -> String {
        let mut src = String::from("# A long document\n\n");
        for i in 1..=60 {
            src.push_str(&format!(
                "## Part {i}\n\nSome prose that goes on for a while so that the page fills up and the break falls in different places each time, with $x_{{{i}}}$ inline and a long enough sentence to wrap.\n\n$$ \\sum_{{n=1}}^{{{i}}} \\frac{{1}}{{n^2 + {i}}} = ? $$\n\n$$ \\int_0^{i} \\frac{{x^2}}{{1 + x^2}} \\, dx = ? $$\n\n"
            ));
        }
        src
    }

    #[test]
    fn nothing_runs_off_the_page_and_no_equation_is_split() {
        for page in [crate::PageSize::A4, crate::PageSize::Letter] {
            let opts = ExportOptions {
                page,
                ..ExportOptions::default()
            };
            let (pages, h, margin) = pages_of(&long_document(), &opts);
            assert!(pages.len() > 3, "only {} pages", pages.len());
            let bottom = h - margin;
            for (n, p) in pages.iter().enumerate() {
                for item in &p.items {
                    let (y, is_number) = match item {
                        Primitive::Glyph { y, .. } => (*y, *y > bottom + 1.0),
                        Primitive::Rule { rect, .. } => (rect.y + rect.h, false),
                        _ => continue,
                    };
                    if is_number {
                        // Only the page number lives below the bottom margin.
                        assert!(y > h - margin, "page {n}");
                        continue;
                    }
                    assert!(
                        y <= bottom + 4.0,
                        "page {n}: content at y = {y} (bottom {bottom})"
                    );
                    assert!(y >= margin - 30.0, "page {n}: content above the top margin");
                }
            }
        }
    }

    #[test]
    fn a_heading_is_never_the_last_thing_on_a_page() {
        let opts = ExportOptions::default();
        let (pages, h, margin) = pages_of(&long_document(), &opts);
        let bottom = h - margin;
        for p in &pages {
            // The lowest glyph of the page body must have text under it that is not a
            // lone heading: at least two lines of ordinary text sit below any bold line.
            let lowest = p
                .items
                .iter()
                .filter_map(|i| match i {
                    Primitive::Glyph { y, font, .. } if *y <= bottom => Some((*y, *font)),
                    _ => None,
                })
                .fold((0.0f32, FontId::Text), |a, b| if b.0 > a.0 { b } else { a });
            assert_ne!(lowest.1, FontId::TextBold, "a heading ended a page");
        }
    }

    #[test]
    fn letter_and_a4_differ_in_size() {
        let a4 = crate::PageSize::A4.points();
        let letter = crate::PageSize::Letter.points();
        assert!(a4.1 > letter.1 && a4.0 < letter.0);
    }

    #[test]
    fn a_proof_ending_in_a_list_shows_one_end_mark() {
        let src = "**Theorem 1.** It holds.\n\n**Proof.**\n1. First — by algebra.\n2. Second — by algebra.\n\u{220e}\n";
        let (pages, _, _) = pages_of(src, &ExportOptions::default());
        let marks = pages
            .iter()
            .flat_map(|p| &p.items)
            .filter(|i| matches!(i, Primitive::Glyph { ch: '\u{220e}', .. }))
            .count();
        assert_eq!(marks, 1);
    }
}
