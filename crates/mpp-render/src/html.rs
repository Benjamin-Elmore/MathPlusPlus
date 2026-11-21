//! A static HTML page for a whole document: prose in a serif face, maths as
//! inline SVG, laid out like a paper.

use std::collections::{BTreeSet, HashMap};
use std::fmt::Write;

use mpp_syntax::document::{
    Align, Block, BlockKind, DataImport, DisplayMath, Document, Figure, FigureItemKind, HelperName,
    HelperVerb, Inline, InlineKind, List, Table, TheoremKind,
};
use mpp_syntax::math::{Clause, Line, MathAst, Node, NodeKind, RelOp, Statement};
use mpp_syntax::Span;

use crate::display::{Color, DisplayList};
use crate::font::{FontId, FONT_LICENCE};
use crate::layout::{
    layout_latex, layout_math, layout_node, IntegralLimits, LayoutOptions, LineResult, MathStyle,
};
use crate::svg::{glyph_defs, num, svg_body};
use crate::theme::Theme;

mod paper;
use paper::Paper;

/// Supplies evaluated results and figures to [`render_html`]. Every method has
/// a default that adds nothing, so `mpp-eval` and `mpp-plot` can plug in later.
pub trait ResultProvider {
    /// Results to fill into a display maths block, by statement line.
    fn display_results(&self, _block: &Block, _math: &DisplayMath) -> Vec<LineResult> {
        vec![]
    }

    /// Results to fill into inline maths (`$\int_0^1 x^2 dx = ?$`), by statement line.
    fn inline_results(&self, _ast: &MathAst) -> Vec<LineResult> {
        vec![]
    }

    /// An SVG drawing for a `Figure:` block.
    fn figure_svg(&self, _block: &Block, _figure: &Figure) -> Option<String> {
        None
    }

    /// The columns and rows of a `Data:` block's file, once it has been read.
    fn data_table(&self, _block: &Block, _data: &DataImport) -> Option<DataRows> {
        None
    }

    /// The text of a `References from [[file]]` file, so its entries can be listed.
    fn references_source(&self, _path: &str) -> Option<String> {
        None
    }
}

/// A table read for a `Data:` block, ready to typeset.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DataRows {
    /// Each column's variable as maths source (`v_0`) and its unit (`m/s`).
    pub columns: Vec<(String, Option<String>)>,
    /// The cells as text, row by row.
    pub rows: Vec<Vec<String>>,
}

/// A provider with no results and no figures.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoResults;

impl ResultProvider for NoResults {}

/// Options for [`render_html_with`].
#[derive(Debug, Clone, PartialEq)]
pub struct HtmlOptions {
    /// Prose font size in pixels; maths matches it.
    pub font_size: f32,
    /// Embed Latin Modern Roman in the page (about 450 KB) so it looks the
    /// same everywhere; otherwise fall back to installed serif fonts.
    pub embed_fonts: bool,
    /// Page title; defaults to the first heading.
    pub title: Option<String>,
    pub integral_limits: IntegralLimits,
    /// Colours of the page.
    pub theme: Theme,
    /// Colours used when the reader's system asks for a dark page
    /// (`prefers-color-scheme: dark`) or the page root has the `dark` class.
    pub dark_theme: Option<Theme>,
    /// Show formatted comments (`% note`, `%{ … %}`) as notes. Off by default:
    /// an export is the finished page, without the author's notes.
    pub include_comments: bool,
}

impl Default for HtmlOptions {
    fn default() -> Self {
        HtmlOptions {
            font_size: 19.0,
            embed_fonts: true,
            title: None,
            integral_limits: IntegralLimits::AboveBelow,
            theme: Theme::LIGHT,
            dark_theme: Some(Theme::DARK),
            include_comments: false,
        }
    }
}

/// Renders a document as a standalone HTML page.
pub fn render_html(doc: &Document, results: &dyn ResultProvider) -> String {
    render_html_with(doc, results, &HtmlOptions::default())
}

/// Renders a document as a standalone HTML page, with options.
pub fn render_html_with(
    doc: &Document,
    results: &dyn ResultProvider,
    opts: &HtmlOptions,
) -> String {
    let mut r = Renderer {
        opts,
        results,
        glyphs: BTreeSet::new(),
        footnotes: HashMap::new(),
        footnote_order: vec![],
        footnote_defs: HashMap::new(),
        theorem_count: 0,
        paper: Paper::scan(doc, results),
        in_comment: false,
    };
    let mut body = String::new();
    r.blocks(&doc.blocks, &mut body);
    let notes = r.footnote_section();
    let title = opts
        .title
        .clone()
        .or_else(|| r.paper.title_text())
        .or_else(|| first_heading(doc))
        .unwrap_or_else(|| "Math++ document".to_string());
    let mut out = String::new();
    out.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    let _ = writeln!(out, "<title>{}</title>", esc(&title));
    out.push_str("<style>\n");
    if opts.embed_fonts {
        out.push_str(&font_faces());
    }
    out.push_str(&theme_css(opts));
    out.push_str(&css(opts.font_size));
    out.push_str("</style>\n</head>\n<body>\n<main class=\"paper\">\n");
    if !r.glyphs.is_empty() {
        out.push_str("<svg class=\"defs\" width=\"0\" height=\"0\" aria-hidden=\"true\">\n");
        out.push_str(&glyph_defs(&r.glyphs));
        out.push_str("</svg>\n");
    }
    out.push_str(&body);
    out.push_str(&notes);
    out.push_str("</main>\n");
    if opts.embed_fonts {
        out.push_str("<!-- Latin Modern fonts: GUST Font License.\n");
        out.push_str(&esc(FONT_LICENCE).replace("--", "- -"));
        out.push_str("\n-->\n");
    }
    out.push_str("</body>\n</html>\n");
    out
}

struct Renderer<'a> {
    opts: &'a HtmlOptions,
    results: &'a dyn ResultProvider,
    glyphs: BTreeSet<(FontId, u16)>,
    /// Footnote name to its number.
    footnotes: HashMap<String, usize>,
    footnote_order: Vec<String>,
    footnote_defs: HashMap<String, String>,
    theorem_count: usize,
    /// What the whole document says about sections, citations and the title block.
    paper: Paper,
    /// Inside a comment: nothing is numbered, evaluated or collected.
    in_comment: bool,
}

impl Renderer<'_> {
    fn math_opts(&self, style: MathStyle) -> LayoutOptions {
        LayoutOptions {
            style,
            font_size: self.opts.font_size,
            integral_limits: self.opts.integral_limits,
            theme: self.opts.theme,
            ..LayoutOptions::default()
        }
    }

    fn svg(&mut self, dl: &DisplayList, class: &str) -> String {
        let theme = self.opts.theme;
        let paint = move |c: Color| match theme.var_for(c) {
            Some(var) => format!("var({var}, {})", c.hex()),
            None => c.hex(),
        };
        let body = svg_body(dl, true, &mut self.glyphs, &paint);
        let label = esc(&dl.text());
        format!(
            "<svg class=\"{class}\" xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\" overflow=\"visible\" style=\"vertical-align:-{}px\" role=\"img\" aria-label=\"{label}\">{body}</svg>",
            num(dl.width),
            num(dl.total_height()),
            num(dl.width),
            num(dl.total_height()),
            num(dl.depth),
        )
    }

    fn inline_math(&mut self, ast: &MathAst) -> String {
        let mut opts = self.math_opts(MathStyle::Text);
        if !self.in_comment {
            opts.results = self.results.inline_results(ast);
        }
        let dl = layout_math(ast, &opts);
        self.svg(&dl, "m")
    }

    fn inline_node(&mut self, node: &Node) -> String {
        let dl = layout_node(node, &self.math_opts(MathStyle::Text));
        self.svg(&dl, "m")
    }

    fn blocks(&mut self, blocks: &[Block], out: &mut String) {
        for b in blocks {
            self.block(b, out);
        }
    }

    fn block(&mut self, b: &Block, out: &mut String) {
        match &b.kind {
            BlockKind::Heading { level, content } => {
                let l = (*level).clamp(1, 6);
                let number = self.heading_number(l);
                let _ = writeln!(out, "<h{l}>{number}{}</h{l}>", self.inlines(content));
            }
            BlockKind::Paragraph(content) => {
                let _ = writeln!(out, "<p>{}</p>", self.inlines(content));
            }
            BlockKind::Let(l) => {
                let _ = writeln!(out, "<p>{}</p>", self.inlines(&l.content));
            }
            BlockKind::List(list) => self.list(list, out),
            BlockKind::Table(t) => self.table(t, out),
            BlockKind::Quote(paras) => {
                out.push_str("<blockquote>\n");
                for p in paras {
                    let _ = writeln!(out, "<p>{}</p>", self.inlines(p));
                }
                out.push_str("</blockquote>\n");
            }
            BlockKind::Verbatim(v) => {
                let _ = writeln!(out, "<pre><code>{}</code></pre>", esc(&v.text));
            }
            BlockKind::Rule => out.push_str("<hr>\n"),
            BlockKind::DisplayMath(m) => self.display(b, m, out),
            BlockKind::Figure(f) => self.figure(b, f, out),
            BlockKind::EmbedImport { path, .. } => {
                let _ = writeln!(
                    out,
                    "<p class=\"import\">Imports everything from {}.</p>",
                    file_link(path, None)
                );
            }
            BlockKind::Using(u) => {
                let names = self.node_list(&u.names);
                let mut s = format!("Using {names}");
                if let Some(a) = &u.aliases {
                    let _ = write!(s, " as {}", self.node_list(a));
                }
                let _ = writeln!(
                    out,
                    "<p class=\"import\">{s} from {}.</p>",
                    file_link(&u.path, None)
                );
            }
            BlockKind::Helper(h) => {
                let mut s = format!(
                    "{} {} of {}",
                    verb(h.verb),
                    helper_text(h.helper),
                    self.inline_node(&h.target)
                );
                if let Some(i) = &h.interval {
                    let _ = write!(s, " on {}", self.inline_node(i));
                }
                if let Some(c) = &h.at {
                    let _ = write!(s, " at {}", self.clause(c));
                }
                let _ = writeln!(out, "<p class=\"helper\">{s}.</p>");
            }
            BlockKind::Analyse(a) => {
                let mut s = format!("Analyse {}", self.inline_node(&a.target));
                if let Some(i) = &a.interval {
                    let _ = write!(s, " on {}", self.inline_node(i));
                }
                let _ = writeln!(out, "<p class=\"helper\">{s}.</p>");
            }
            BlockKind::Solve(sv) => {
                let mut s = format!("Solve {}", self.inline_node(&sv.equation));
                if let Some(u) = &sv.unknowns {
                    let _ = write!(s, " for {}", self.node_list(u));
                }
                let _ = writeln!(out, "<p class=\"helper\">{s}.</p>");
            }
            BlockKind::FootnoteDef { .. } if self.in_comment => {}
            BlockKind::FootnoteDef { name, content } => {
                let html = self.inlines(content);
                self.footnote_defs.insert(name.clone(), html);
                self.footnote_number(name);
            }
            BlockKind::Directive(_) => {}
            BlockKind::Comment(c) => self.comment(c, out),
            BlockKind::Title(_) => self.title_block(out),
            BlockKind::References(r) => self.references_block(r, out),
            BlockKind::Assume(a) => {
                let mut s = String::from("Assume ");
                let n = a.conditions.len();
                for (i, c) in a.conditions.iter().enumerate() {
                    if i > 0 {
                        s.push_str(if i + 1 == n { " and " } else { ", " });
                    }
                    s.push_str(&self.inline_node(c));
                }
                let _ = writeln!(out, "<p class=\"assume\">{s}.</p>");
            }
            BlockKind::Data(d) => self.data_block(b, d, out),
            BlockKind::Theorem(t) => {
                let proof = t.kind == TheoremKind::Proof;
                let head = if proof || t.kind == TheoremKind::Remark {
                    format!("{}.", t.kind.as_str())
                } else {
                    self.theorem_label(t.kind)
                };
                let class = match t.kind {
                    TheoremKind::Theorem
                    | TheoremKind::Lemma
                    | TheoremKind::Corollary
                    | TheoremKind::Proposition => "theorem slanted",
                    TheoremKind::Proof => "theorem proof",
                    _ => "theorem",
                };
                let _ = writeln!(
                    out,
                    "<div class=\"{class}\"><span class=\"thm-head\">{}</span>",
                    esc(&head)
                );
                let mut body = String::new();
                self.blocks(&t.body, &mut body);
                if proof {
                    // Set the closing ∎ flush right, whether it was written or not.
                    let trimmed = body.trim_end();
                    let written = ["∎</p>", "\\qed</p>"]
                        .into_iter()
                        .find(|end| trimmed.ends_with(end));
                    if let Some(end) = written {
                        let cut = trimmed.len() - end.len();
                        body = format!(
                            "{}<span class=\"qed\">∎</span></p>\n",
                            trimmed[..cut].trim_end()
                        );
                    } else {
                        body.push_str("<span class=\"qed\">∎</span>\n");
                    }
                }
                out.push_str(&body);
                out.push_str("</div>\n");
            }
        }
    }

    fn data_block(&mut self, b: &Block, d: &DataImport, out: &mut String) {
        let name = self.inline_node(&d.name);
        let file = file_link(&d.path, None);
        let Some(t) = self.results.data_table(b, d) else {
            let _ = writeln!(out, "<p class=\"data\">Data {name} from {file}.</p>");
            return;
        };
        let _ = writeln!(out, "<figure class=\"data\">\n<table class=\"data\">");
        out.push_str("<thead><tr>");
        for (var, unit) in &t.columns {
            let dl = layout_latex(var, &self.math_opts(MathStyle::Text));
            let mut cell = self.svg(&dl, "m");
            if let Some(u) = unit {
                let _ = write!(cell, " <span class=\"unit\">({})</span>", esc(u));
            }
            let _ = write!(out, "<th>{cell}</th>");
        }
        out.push_str("</tr></thead>\n<tbody>\n");
        for row in &t.rows {
            out.push_str("<tr>");
            for cell in row {
                let _ = write!(out, "<td>{}</td>", esc(cell));
            }
            out.push_str("</tr>\n");
        }
        out.push_str("</tbody>\n</table>\n");
        let _ = writeln!(
            out,
            "<figcaption>{name} from {file}</figcaption>\n</figure>"
        );
    }

    fn display(&mut self, b: &Block, m: &DisplayMath, out: &mut String) {
        let mut opts = self.math_opts(MathStyle::Display);
        if !self.in_comment {
            opts.results = self.results.display_results(b, m);
        }
        let dl = layout_math(&m.ast, &opts);
        let svg = self.svg(&dl, "dm");
        let id = m
            .number
            .map(|n| format!(" id=\"eq-{n}\""))
            .unwrap_or_default();
        let num = m
            .number
            .map(|n| format!("<span class=\"eq-num\">({n})</span>"))
            .unwrap_or_default();
        let _ = writeln!(out, "<div class=\"eq\"{id}>{svg}{num}</div>");
    }

    fn figure(&mut self, b: &Block, f: &Figure, out: &mut String) {
        out.push_str("<figure>\n");
        match self.results.figure_svg(b, f) {
            Some(svg) => out.push_str(&svg),
            None => {
                let mut parts = vec![];
                for item in &f.items {
                    match &item.kind {
                        FigureItemKind::Expr(ast) => parts.push(self.inline_math(ast)),
                        FigureItemKind::Range(r) => {
                            let line = Line {
                                statement: Statement::Range(r.clone()),
                                clauses: vec![],
                                noeval: false,
                                align: vec![],
                                span: r.span,
                            };
                            let ast = MathAst {
                                lines: vec![line],
                                label: None,
                                aligned: false,
                                span: r.span,
                            };
                            parts.push(self.inline_math(&ast));
                        }
                        FigureItemKind::Keyword { keyword, arg } => {
                            let mut s = esc(keyword.as_str());
                            if let Some(c) = arg {
                                let _ = write!(s, " {}", self.clause(c));
                            }
                            parts.push(s);
                        }
                    }
                }
                let _ = writeln!(
                    out,
                    "<div class=\"fig-spec\">Graph of {}</div>",
                    parts.join(", ")
                );
            }
        }
        if !f.caption.is_empty() {
            let _ = writeln!(out, "<figcaption>{}</figcaption>", self.inlines(&f.caption));
        }
        out.push_str("</figure>\n");
    }

    fn clause(&mut self, c: &Clause) -> String {
        match c {
            Clause::Where { name, value, span } => {
                let n = Node::new(
                    NodeKind::Relation {
                        first: Box::new(name.clone()),
                        rest: vec![(RelOp::Eq, value.clone())],
                    },
                    *span,
                );
                self.inline_node(&n)
            }
            Clause::Range(r) => {
                let ast = MathAst {
                    lines: vec![Line {
                        statement: Statement::Range(r.clone()),
                        clauses: vec![],
                        noeval: false,
                        align: vec![],
                        span: r.span,
                    }],
                    label: None,
                    aligned: false,
                    span: r.span,
                };
                self.inline_math(&ast)
            }
            Clause::Condition(n) => self.inline_node(n),
            Clause::Assumption(n) => format!("({})", self.inline_node(n)),
        }
    }

    fn node_list(&mut self, nodes: &[Node]) -> String {
        let ast = MathAst {
            lines: vec![Line {
                statement: Statement::NameList(nodes.to_vec()),
                clauses: vec![],
                noeval: false,
                align: vec![],
                span: Span::default(),
            }],
            label: None,
            aligned: false,
            span: Span::default(),
        };
        self.inline_math(&ast)
    }

    fn list(&mut self, list: &List, out: &mut String) {
        let tag = if list.ordered { "ol" } else { "ul" };
        let start = list
            .items
            .first()
            .and_then(|i| i.number)
            .filter(|n| list.ordered && *n != 1)
            .map(|n| format!(" start=\"{n}\""))
            .unwrap_or_default();
        let _ = writeln!(out, "<{tag}{start}>");
        for item in &list.items {
            let _ = write!(out, "<li>{}", self.inlines(&item.content));
            for child in &item.children {
                out.push('\n');
                self.list(child, out);
            }
            out.push_str("</li>\n");
        }
        let _ = writeln!(out, "</{tag}>");
    }

    fn table(&mut self, t: &Table, out: &mut String) {
        out.push_str("<table>\n<thead><tr>");
        for (i, cell) in t.header.iter().enumerate() {
            let _ = write!(
                out,
                "<th{}>{}</th>",
                align(t.aligns.get(i)),
                self.inlines(cell)
            );
        }
        out.push_str("</tr></thead>\n<tbody>\n");
        for row in &t.rows {
            out.push_str("<tr>");
            for (i, cell) in row.iter().enumerate() {
                let _ = write!(
                    out,
                    "<td{}>{}</td>",
                    align(t.aligns.get(i)),
                    self.inlines(cell)
                );
            }
            out.push_str("</tr>\n");
        }
        out.push_str("</tbody>\n</table>\n");
    }

    fn footnote_number(&mut self, name: &str) -> usize {
        if let Some(n) = self.footnotes.get(name) {
            return *n;
        }
        let n = self.footnotes.len() + 1;
        self.footnotes.insert(name.to_string(), n);
        self.footnote_order.push(name.to_string());
        n
    }

    fn footnote_section(&self) -> String {
        let defined: Vec<&String> = self
            .footnote_order
            .iter()
            .filter(|n| self.footnote_defs.contains_key(*n))
            .collect();
        if defined.is_empty() {
            return String::new();
        }
        let mut out = String::from("<hr class=\"notes\">\n<ol class=\"footnotes\">\n");
        for name in &self.footnote_order {
            let n = self.footnotes.get(name).copied().unwrap_or(0);
            let text = self
                .footnote_defs
                .get(name)
                .cloned()
                .unwrap_or_else(|| "(missing footnote)".to_string());
            let id = esc(&slug(name));
            let _ = writeln!(
                out,
                "<li value=\"{n}\" id=\"fn-{id}\">{text} <a href=\"#fnref-{id}\">↩</a></li>"
            );
        }
        out.push_str("</ol>\n");
        out
    }

    fn inlines(&mut self, items: &[Inline]) -> String {
        let mut out = String::new();
        for i in items {
            match &i.kind {
                InlineKind::Text(t) => out.push_str(&esc(t)),
                InlineKind::SoftBreak => out.push('\n'),
                InlineKind::Emphasis(inner) => {
                    let _ = write!(out, "<em>{}</em>", self.inlines(inner));
                }
                InlineKind::Strong(inner) => {
                    let _ = write!(out, "<strong>{}</strong>", self.inlines(inner));
                }
                InlineKind::Math(m) => out.push_str(&self.inline_math(&m.ast)),
                InlineKind::Code(c) => {
                    let _ = write!(out, "<code>{}</code>", esc(c));
                }
                InlineKind::WikiLink { path, alias } => {
                    out.push_str(&file_link(path, alias.as_deref()));
                }
                InlineKind::Link { text, url } => {
                    let _ = write!(
                        out,
                        "<a href=\"{}\">{}</a>",
                        esc(&safe_url(url)),
                        self.inlines(text)
                    );
                }
                InlineKind::FootnoteRef(name) if self.in_comment => {
                    let _ = write!(out, "<sup class=\"fnref\">[^{}]</sup>", esc(name));
                }
                InlineKind::Cite(cites) => out.push_str(&self.cite(cites)),
                InlineKind::FootnoteRef(name) => {
                    let n = self.footnote_number(name);
                    let id = esc(&slug(name));
                    let hover = match self.paper.note_text(name) {
                        Some(t) => format!(" title=\"{}\"", esc(t)),
                        None => String::new(),
                    };
                    let _ = write!(
                        out,
                        "<sup class=\"fnref\" id=\"fnref-{id}\"{hover}><a href=\"#fn-{id}\">{n}</a></sup>"
                    );
                }
                InlineKind::EqRef { number, .. } => match number {
                    Some(n) => {
                        let _ = write!(out, "<a class=\"eqref\" href=\"#eq-{n}\">({n})</a>");
                    }
                    None => out.push_str("(?)"),
                },
                InlineKind::Escape(c) => out.push_str(&esc(&c.to_string())),
            }
        }
        out
    }
}

fn first_heading(doc: &Document) -> Option<String> {
    doc.blocks.iter().find_map(|b| match &b.kind {
        BlockKind::Heading { content, .. } => Some(plain_text(content)),
        _ => None,
    })
}

fn plain_text(items: &[Inline]) -> String {
    let mut s = String::new();
    for i in items {
        match &i.kind {
            InlineKind::Text(t) | InlineKind::Code(t) => s.push_str(t),
            InlineKind::SoftBreak => s.push(' '),
            InlineKind::Emphasis(inner) | InlineKind::Strong(inner) => {
                s.push_str(&plain_text(inner))
            }
            InlineKind::Link { text, .. } => s.push_str(&plain_text(text)),
            InlineKind::WikiLink { path, alias } => s.push_str(alias.as_deref().unwrap_or(path)),
            InlineKind::Escape(c) => s.push(*c),
            _ => {}
        }
    }
    s.trim().to_string()
}

fn verb(v: HelperVerb) -> &'static str {
    match v {
        HelperVerb::Find => "Find",
        HelperVerb::Compute => "Compute",
        HelperVerb::Determine => "Determine",
    }
}

fn helper_text(h: HelperName) -> &'static str {
    match h {
        HelperName::Domain => "the domain",
        HelperName::Range => "the range",
        HelperName::Intercepts => "the intercepts",
        HelperName::CriticalPoints => "the critical points",
        HelperName::Extrema => "the extrema",
        HelperName::LocalExtrema => "the local extrema",
        HelperName::AbsoluteExtrema => "the absolute extrema",
        HelperName::IntervalsOfIncrease => "the intervals of increase",
        HelperName::Concavity => "the concavity",
        HelperName::InflectionPoints => "the inflection points",
        HelperName::Asymptotes => "the asymptotes",
        HelperName::TangentLine => "the tangent line",
        HelperName::NormalLine => "the normal line",
        HelperName::Linearisation => "the linearisation",
    }
}

fn align(a: Option<&Align>) -> &'static str {
    match a {
        Some(Align::Left) => " style=\"text-align:left\"",
        Some(Align::Right) => " style=\"text-align:right\"",
        Some(Align::Center) => " style=\"text-align:center\"",
        _ => "",
    }
}

/// A link to another `.mpp` file's page.
fn file_link(path: &str, alias: Option<&str>) -> String {
    let href = match path.strip_suffix(".mpp") {
        Some(stem) => format!("{stem}.html"),
        None => path.to_string(),
    };
    format!(
        "<a class=\"file\" href=\"{}\">{}</a>",
        esc(&safe_url(&href)),
        esc(alias.unwrap_or(path))
    )
}

/// Refuses script URLs.
fn safe_url(url: &str) -> String {
    let lower: String = url
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_control())
        .collect::<String>()
        .to_ascii_lowercase();
    if lower.starts_with("javascript:")
        || lower.starts_with("vbscript:")
        || lower.starts_with("data:")
    {
        "#".to_string()
    } else {
        url.to_string()
    }
}

fn slug(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect()
}

/// HTML-escapes text for content and attributes.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn font_faces() -> String {
    let mut out = String::new();
    for (id, style, weight) in [
        (FontId::Text, "normal", "normal"),
        (FontId::TextItalic, "italic", "normal"),
        (FontId::TextBold, "normal", "bold"),
    ] {
        let _ = writeln!(
            out,
            "@font-face {{ font-family: 'Latin Modern Roman'; font-style: {style}; font-weight: {weight}; src: url(data:font/otf;base64,{}) format('opentype'); }}",
            base64(id.bytes())
        );
    }
    out
}

/// The theme's colours as CSS variables, with the dark variant.
fn theme_css(opts: &HtmlOptions) -> String {
    let vars = |t: &Theme| {
        t.css_vars()
            .iter()
            .map(|(name, c)| format!("{name}: {};", c.hex()))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut out = format!(":root {{ {} }}\n", vars(&opts.theme));
    if let Some(dark) = &opts.dark_theme {
        let _ = writeln!(
            out,
            "@media (prefers-color-scheme: dark) {{ :root:not(.light) {{ {} }} }}",
            vars(dark)
        );
        let _ = writeln!(out, ":root.dark {{ {} }}", vars(dark));
    }
    out
}

fn css(size: f32) -> String {
    let size = if size.is_finite() && size > 4.0 {
        size
    } else {
        19.0
    };
    format!(
        r#"html {{ background: var(--mpp-background); }}
body {{ margin: 0; color: var(--mpp-text); background: var(--mpp-background); font-family: 'Latin Modern Roman', 'LM Roman 10', 'CMU Serif', 'Cambria', 'Times New Roman', serif; font-size: {size}px; line-height: 1.45; }}
main.paper {{ max-width: 40em; margin: 3em auto 5em; padding: 0 1.5em; text-align: justify; hyphens: auto; }}
h1 {{ font-size: 1.7em; font-weight: normal; text-align: center; margin: 0.4em 0 1em; }}
h2 {{ font-size: 1.3em; margin: 1.6em 0 0.6em; }}
h3 {{ font-size: 1.1em; margin: 1.3em 0 0.5em; }}
h4, h5, h6 {{ font-size: 1em; font-style: italic; margin: 1em 0 0.4em; }}
p {{ margin: 0 0 0.7em; }}
svg.defs {{ position: absolute; width: 0; height: 0; overflow: hidden; }}
svg.m {{ display: inline; }}
.eq {{ position: relative; display: flex; justify-content: center; align-items: center; margin: 0.9em 0; overflow-x: auto; }}
.eq-num {{ position: absolute; right: 0; top: 50%; transform: translateY(-50%); }}
.theorem {{ margin: 1em 0; }}
.theorem > p:first-of-type {{ display: inline; }}
.theorem.slanted p {{ font-style: italic; }}
.thm-head {{ font-weight: bold; margin-right: 0.4em; }}
.proof .thm-head {{ font-weight: normal; font-style: italic; }}
.qed {{ float: right; }}
.import, .helper, .assume {{ color: var(--mpp-muted); }}
.assume {{ font-style: italic; }}
.unit {{ font-size: 0.85em; color: var(--mpp-muted); }}
blockquote {{ margin: 1em 2em; color: var(--mpp-muted); }}
table {{ border-collapse: collapse; margin: 1em auto; }}
th, td {{ padding: 0.2em 0.8em; }}
thead th {{ border-top: 1.5px solid var(--mpp-rule); border-bottom: 1px solid var(--mpp-rule); }}
tbody tr:last-child td {{ border-bottom: 1.5px solid var(--mpp-rule); }}
pre {{ background: var(--mpp-code-background); padding: 0.7em 1em; overflow-x: auto; font-size: 0.85em; }}
code {{ font-family: 'Consolas', 'DejaVu Sans Mono', monospace; font-size: 0.9em; }}
figure {{ margin: 1.2em 0; text-align: center; }}
figcaption {{ font-size: 0.92em; margin-top: 0.4em; }}
.fig-spec {{ border: 1px dashed var(--mpp-placeholder); padding: 1.5em; color: var(--mpp-muted); }}
hr {{ border: none; border-top: 1px solid var(--mpp-rule); opacity: 0.35; margin: 1.5em 0; }}
ol.footnotes {{ font-size: 0.88em; }}
.title-block {{ text-align: center; margin: 0 0 2em; }}
.doc-title {{ font-size: 1.9em; margin: 0.2em 0 0.5em; }}
.doc-author {{ margin: 0.1em 0; font-size: 1.1em; }}
.doc-date {{ margin: 0.1em 0; color: var(--mpp-muted); }}
.abstract {{ margin: 1.4em 2.5em 0; font-size: 0.92em; text-align: justify; }}
.abstract-head {{ text-align: center; font-weight: bold; margin: 0 0 0.3em; }}
.sec-num {{ margin-right: 0.5em; }}
.cite-missing {{ color: var(--mpp-error); font-weight: bold; }}
.references h2 {{ margin-top: 2em; }}
ol.refs {{ list-style: none; padding-left: 0; font-size: 0.95em; }}
ol.refs li {{ margin: 0.35em 0; padding-left: 2.6em; text-indent: -2.6em; text-align: left; }}
ol.refs.author-year li {{ padding-left: 1.6em; text-indent: -1.6em; }}
.ref-num {{ display: inline-block; width: 2.6em; text-indent: 0; }}
.note {{ display: flex; gap: 0.7em; align-items: flex-start; margin: 0.9em 0; padding: 0.55em 0.9em; background: var(--mpp-note-background); border-left: 3px solid var(--mpp-note-accent); border-radius: 0 6px 6px 0; font-size: 0.86em; line-height: 1.4; color: var(--mpp-muted); text-align: left; }}
.note-mark {{ color: var(--mpp-note-accent); font-size: 1.1em; line-height: 1.2; }}
.note-body {{ flex: 1; min-width: 0; }}
.note-body > :first-child {{ margin-top: 0; }}
.note-body > :last-child {{ margin-bottom: 0; }}
.note.off {{ border-left-style: dashed; }}
.note .eq {{ opacity: 0.6; }}
.eq + .note {{ margin-top: -0.4em; }}
a {{ color: var(--mpp-link); text-decoration: none; }}
a:hover {{ text-decoration: underline; }}
"#
    )
}

/// Standard base64 with padding.
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
        out.push(char::from(T[(n >> 18) as usize & 63]));
        out.push(char::from(T[(n >> 12) as usize & 63]));
        out.push(if chunk.len() > 1 {
            char::from(T[(n >> 6) as usize & 63])
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            char::from(T[n as usize & 63])
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_values() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn script_urls_are_refused() {
        assert_eq!(safe_url("javascript:alert(1)"), "#");
        assert_eq!(safe_url(" JavaScript:x"), "#");
        assert_eq!(safe_url("https://example.org"), "https://example.org");
    }
}
