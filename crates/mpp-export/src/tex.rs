//! LaTeX export: a standalone `.tex` file for pdflatex, with figures beside it.

use std::collections::HashMap;
use std::fmt::Write;

use mpp_eval::Evaluated;
use mpp_render::ResultProvider;
use mpp_syntax::document::{
    cite_name, parse_reference_entries, Align, Block, BlockKind, CiteStyle, Comment, DataImport,
    Directive, DirectiveKind, DisplayMath, Inline, InlineKind, List, Reference, Table, TheoremKind,
    TitleKind,
};
use mpp_syntax::symbols::lookup_char;

use crate::{provider::EvalProvider, ExportOptions, Exported};

pub(crate) fn export(
    ev: &Evaluated,
    provider: &EvalProvider<'_>,
    name: &str,
    opts: &ExportOptions,
) -> Exported {
    let mut w = Writer {
        ev,
        provider,
        opts,
        name: slug(name),
        out: String::new(),
        files: vec![],
        notes: HashMap::new(),
        figure_count: 0,
        refs: vec![],
        number_sections: false,
        base_level: 1,
        title_done: false,
        in_comment: false,
    };
    w.scan();
    w.document();
    Exported {
        main: w.out.into_bytes(),
        files: w.files,
    }
}

struct Writer<'a> {
    ev: &'a Evaluated,
    provider: &'a EvalProvider<'a>,
    opts: &'a ExportOptions,
    name: String,
    out: String,
    files: Vec<(String, Vec<u8>)>,
    /// Footnote text (already LaTeX) by name.
    notes: HashMap<String, String>,
    figure_count: usize,
    refs: Vec<Reference>,
    number_sections: bool,
    base_level: u8,
    title_done: bool,
    in_comment: bool,
}

fn slug(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let t = t.trim_matches('-').to_string();
    if t.is_empty() {
        "document".into()
    } else {
        t
    }
}

/// Escapes running text for LaTeX.
pub(crate) fn escape_text(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\textbackslash{}"),
            '{' => o.push_str("\\{"),
            '}' => o.push_str("\\}"),
            '$' => o.push_str("\\$"),
            '&' => o.push_str("\\&"),
            '#' => o.push_str("\\#"),
            '%' => o.push_str("\\%"),
            '_' => o.push_str("\\_"),
            '^' => o.push_str("\\^{}"),
            '~' => o.push_str("\\~{}"),
            '–' => o.push_str("--"),
            '—' => o.push_str("---"),
            '‘' => o.push('`'),
            '’' => o.push('\''),
            '“' => o.push_str("``"),
            '”' => o.push_str("''"),
            '∎' => o.push_str("$\\blacksquare$"),
            '✎' => {}
            c if (c as u32) > 0x7f => match lookup_char(c) {
                Some(s) if !s.latex.is_empty() => {
                    let _ = write!(o, "${}$", s.latex);
                }
                _ => unicode_fallback(c, false, &mut o),
            },
            c => o.push(c),
        }
    }
    o
}

/// Turns maths source (LaTeX with Unicode symbols) into ASCII LaTeX.
pub(crate) fn texify_math(src: &str) -> String {
    let mut o = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ':' if chars.peek() == Some(&'=') => {
                chars.next();
                o.push_str("\\coloneqq ");
            }
            '≔' => o.push_str("\\coloneqq "),
            c if (c as u32) > 0x7f => match lookup_char(c) {
                Some(s) if !s.latex.is_empty() => {
                    o.push_str(s.latex);
                    o.push(' ');
                }
                _ => unicode_fallback(c, true, &mut o),
            },
            c => o.push(c),
        }
    }
    o
}

/// A character with no LaTeX symbol: a raised or lowered letter or digit, or a visible `?`.
fn unicode_fallback(c: char, math: bool, o: &mut String) {
    use mpp_syntax::symbols::{subscript_value, superscript_value};
    let (script, mark) = if let Some(n) = superscript_value(c) {
        (n, '^')
    } else if let Some(n) = subscript_value(c) {
        (n, '_')
    } else if c.is_ascii() {
        o.push(c);
        return;
    } else {
        o.push('?');
        return;
    };
    if math {
        let _ = write!(o, "{mark}{{{script}}}");
    } else {
        let _ = write!(o, "${mark}{{{script}}}$");
    }
}

/// Puts a result where the `?` of a query is, or a mark after a check.
fn fill_result(text: &mut String, value: &mpp_render::ResultValue) {
    let value = match value {
        mpp_render::ResultValue::Latex(l) => texify_math(l),
        mpp_render::ResultValue::Node(_) => String::new(),
    };
    match value.as_str() {
        "✓" => text.push_str(" \\quad \\checkmark"),
        "✗" => text.push_str(" \\quad \\times"),
        "?" => {}
        v => {
            if let Some(q) = text.rfind('?') {
                let after = text[q + 1..].to_string();
                let rest = after.strip_prefix('!').unwrap_or(&after).to_string();
                text.truncate(q);
                let _ = write!(text, "{v}{rest}");
            }
        }
    }
}

fn strip_label(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("\\label{") {
        out.push_str(&rest[..i]);
        match rest[i..].find('}') {
            Some(j) => rest = &rest[i + j + 1..],
            None => rest = "",
        }
    }
    out.push_str(rest);
    out
}

fn bib_key(k: &str) -> String {
    k.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == ':' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

impl Writer<'_> {
    fn scan(&mut self) {
        let doc = &self.ev.document;
        let (mut h1, mut deeper) = (0, false);
        let mut refs: Vec<Reference> = vec![];
        let mut files: Vec<String> = vec![];
        let mut number_sections = false;
        doc.walk(&mut |b: &Block| match &b.kind {
            BlockKind::Directive(Directive {
                kind: DirectiveKind::NumberSections,
                ..
            }) => number_sections = true,
            BlockKind::Heading { level, .. } => {
                if *level == 1 {
                    h1 += 1;
                } else {
                    deeper = true;
                }
            }
            BlockKind::References(r) => {
                refs.extend(r.entries.iter().cloned());
                if let Some((p, _)) = &r.from {
                    files.push(p.clone());
                }
            }
            _ => {}
        });
        self.number_sections = number_sections;
        for f in files {
            if let Some(t) = self.provider.references_source(&f) {
                refs.extend(parse_reference_entries(&t));
            }
        }
        self.refs = refs;
        self.base_level = if h1 == 1 && deeper { 2 } else { 1 };
        let mut defs: Vec<(String, Vec<Inline>)> = vec![];
        doc.walk(&mut |b: &Block| {
            if let BlockKind::FootnoteDef { name, content } = &b.kind {
                defs.push((name.clone(), content.clone()));
            }
        });
        for (n, c) in defs {
            let t = self.inlines(&c);
            self.notes.insert(n, t);
        }
    }

    fn document(&mut self) {
        let has_title = self
            .ev
            .document
            .blocks
            .iter()
            .any(|b| matches!(&b.kind, BlockKind::Title(t) if t.kind == TitleKind::Title));
        let mut head = String::new();
        head.push_str("% Exported from Math++. Compile with pdflatex.\n");
        head.push_str("\\documentclass[11pt,a4paper]{article}\n");
        head.push_str(
            "\\usepackage[T1]{fontenc}\n\\usepackage[utf8]{inputenc}\n\\usepackage{lmodern}\n",
        );
        head.push_str("\\usepackage{amsmath,amssymb,amsthm,mathtools}\n");
        head.push_str(
            "\\usepackage{graphicx,booktabs,xcolor}\n\\usepackage[hidelinks]{hyperref}\n",
        );
        head.push_str("\\usepackage[margin=25mm]{geometry}\n");
        if self.opts.dark {
            head.push_str("\\pagecolor[HTML]{1b1c1e}\\color[HTML]{e6e6e3}\n");
        }
        let within = if self.number_sections {
            "[section]"
        } else {
            ""
        };
        head.push_str("\\theoremstyle{plain}\n");
        let _ = writeln!(head, "\\newtheorem{{theorem}}{{Theorem}}{within}");
        for (env, label) in [
            ("lemma", "Lemma"),
            ("corollary", "Corollary"),
            ("proposition", "Proposition"),
        ] {
            let _ = writeln!(head, "\\newtheorem{{{env}}}[theorem]{{{label}}}");
        }
        head.push_str("\\theoremstyle{definition}\n");
        head.push_str("\\newtheorem{definition}[theorem]{Definition}\n");
        head.push_str("\\newtheorem{example}[theorem]{Example}\n");
        head.push_str("\\theoremstyle{remark}\n\\newtheorem*{remark}{Remark}\n");
        head.push_str("\\setlength{\\parindent}{0pt}\\setlength{\\parskip}{0.6em}\n");
        let first_h1 = if self.base_level == 2 && !has_title {
            self.ev.document.blocks.iter().find_map(|b| match &b.kind {
                BlockKind::Heading { level: 1, content } => Some(content.clone()),
                _ => None,
            })
        } else {
            None
        };
        if let Some(c) = &first_h1 {
            let t = self.inlines(c);
            let _ = writeln!(head, "\\title{{{t}}}\n\\date{{}}");
        }
        self.out.push_str(&head);
        self.out.push_str("\\begin{document}\n");
        if first_h1.is_some() {
            self.out.push_str("\\maketitle\n");
        }
        let blocks = self.ev.document.blocks.clone();
        let mut skip_h1 = first_h1.is_some();
        for b in &blocks {
            if skip_h1 {
                if let BlockKind::Heading { level: 1, .. } = &b.kind {
                    skip_h1 = false;
                    continue;
                }
            }
            self.block(b);
        }
        self.out.push_str("\\end{document}\n");
    }

    fn src(&self, span: mpp_syntax::Span) -> String {
        self.ev
            .source()
            .get(span.start..span.end)
            .unwrap_or("")
            .to_string()
    }

    fn blocks(&mut self, bs: &[Block]) {
        for b in bs {
            self.block(b);
        }
    }

    fn block(&mut self, b: &Block) {
        match &b.kind {
            BlockKind::Heading { level, content } => {
                let l = (*level).max(self.base_level) - self.base_level;
                let cmd =
                    ["section", "subsection", "subsubsection", "paragraph"][usize::from(l).min(3)];
                let star = if self.number_sections { "" } else { "*" };
                let t = self.inlines(content);
                let _ = writeln!(self.out, "\\{cmd}{star}{{{t}}}\n");
            }
            BlockKind::Paragraph(c) => {
                let t = self.inlines(c);
                let _ = writeln!(self.out, "{t}\n");
            }
            BlockKind::Let(l) => {
                let t = self.inlines(&l.content);
                let _ = writeln!(self.out, "{t}\n");
            }
            BlockKind::List(l) => self.list(l),
            BlockKind::Table(t) => self.table(t),
            BlockKind::Quote(ps) => {
                self.out.push_str("\\begin{quote}\n");
                for p in ps {
                    let t = self.inlines(p);
                    let _ = writeln!(self.out, "{t}\n");
                }
                self.out.push_str("\\end{quote}\n");
            }
            BlockKind::Verbatim(v) => {
                let _ = writeln!(
                    self.out,
                    "\\begin{{verbatim}}\n{}\n\\end{{verbatim}}",
                    v.text
                );
            }
            BlockKind::Rule => self.out.push_str("\\medskip\\hrule\\medskip\n"),
            BlockKind::DisplayMath(m) => self.display(b, m),
            BlockKind::Figure(_) => self.figure(b),
            BlockKind::EmbedImport { path, .. } => {
                let _ = writeln!(
                    self.out,
                    "\\textit{{Imports everything from {}.}}\n",
                    escape_text(path)
                );
            }
            BlockKind::Using(_)
            | BlockKind::Helper(_)
            | BlockKind::Analyse(_)
            | BlockKind::Solve(_)
            | BlockKind::Assume(_) => {
                let t = self.source_prose(&self.src(b.span));
                let _ = writeln!(self.out, "{t}\n");
            }
            BlockKind::FootnoteDef { .. } | BlockKind::Directive(_) => {}
            BlockKind::Comment(c) => self.comment(c),
            BlockKind::Title(_) => self.title(),
            BlockKind::References(_) => self.references(),
            BlockKind::Data(d) => self.data(d),
            BlockKind::Theorem(t) => {
                let env = match t.kind {
                    TheoremKind::Definition => "definition",
                    TheoremKind::Theorem => "theorem",
                    TheoremKind::Lemma => "lemma",
                    TheoremKind::Corollary => "corollary",
                    TheoremKind::Proposition => "proposition",
                    TheoremKind::Example => "example",
                    TheoremKind::Remark => "remark",
                    TheoremKind::Proof => "proof",
                };
                let _ = writeln!(self.out, "\\begin{{{env}}}");
                self.blocks(&t.body);
                if t.kind == TheoremKind::Proof {
                    // amsthm adds the ∎ itself; drop a written one.
                    let trimmed = self.out.trim_end().to_string();
                    for end in ["$\\blacksquare$", "\\qed"] {
                        if let Some(s) = trimmed.strip_suffix(end) {
                            self.out = format!("{}\n", s.trim_end());
                        }
                    }
                }
                let _ = writeln!(self.out, "\\end{{{env}}}");
            }
        }
    }

    /// Prose given as source (`Find the domain of $f$.`): text escaped, `$…$` as maths.
    fn source_prose(&self, src: &str) -> String {
        let mut out = String::new();
        for (i, part) in src.split('$').enumerate() {
            if i % 2 == 1 {
                let _ = write!(out, "${}$", texify_math(part.trim()));
            } else {
                out.push_str(&escape_text(part));
            }
        }
        out.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn comment(&mut self, c: &Comment) {
        if !self.opts.include_comments {
            return;
        }
        let saved = std::mem::replace(&mut self.in_comment, true);
        self.out.push_str("\\begin{quote}\\small\n");
        let bs = c.blocks.clone();
        self.blocks(&bs);
        self.out.push_str("\\end{quote}\n");
        self.in_comment = saved;
    }

    fn title(&mut self) {
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
        let mut pre = String::new();
        let mut abstracts = String::new();
        let mut has_date = false;
        for kind in TitleKind::ALL {
            for f in fields.iter().filter(|f| f.kind == *kind) {
                let t = self.inlines(&f.content);
                match kind {
                    TitleKind::Title => {
                        let _ = writeln!(pre, "\\title{{{t}}}");
                    }
                    TitleKind::Author => {
                        let _ = writeln!(pre, "\\author{{{t}}}");
                    }
                    TitleKind::Date => {
                        has_date = true;
                        let _ = writeln!(pre, "\\date{{{t}}}");
                    }
                    TitleKind::Abstract => {
                        let _ = writeln!(abstracts, "\\begin{{abstract}}\n{t}\n\\end{{abstract}}");
                    }
                }
            }
        }
        if !has_date {
            pre.push_str("\\date{}\n");
        }
        // These commands may follow \begin{document} as long as \maketitle comes after them.
        self.out.push_str(&pre);
        self.out.push_str("\\maketitle\n");
        self.out.push_str(&abstracts);
    }

    fn references(&mut self) {
        if self.refs.is_empty() {
            return;
        }
        let author_year = self.ev.document.cite_style() == CiteStyle::AuthorYear;
        let mut list = self.refs.clone();
        if author_year {
            list.sort_by_key(|r| (cite_name(&r.author).to_lowercase(), r.year.clone()));
        }
        let _ = writeln!(self.out, "\\begin{{thebibliography}}{{99}}");
        for r in &list {
            let mut line = format!(
                "{}. \\emph{{{}}}",
                escape_text(r.author.trim_end_matches('.')),
                escape_text(&r.title)
            );
            if let Some(y) = &r.year {
                let _ = write!(line, ", {}", escape_text(y));
            }
            line.push('.');
            if let Some(l) = &r.link {
                let _ = write!(line, " \\url{{{}}}", l.replace(['{', '}'], ""));
            }
            let _ = writeln!(self.out, "\\bibitem{{{}}} {line}", bib_key(&r.key));
        }
        let _ = writeln!(self.out, "\\end{{thebibliography}}");
    }

    fn data(&mut self, d: &DataImport) {
        let Some(t) = self.provider.table(d) else {
            let _ = writeln!(
                self.out,
                "\\textit{{Data from {}.}}\n",
                escape_text(&d.path)
            );
            return;
        };
        let cols = "r".repeat(t.columns.len().max(1));
        let _ = writeln!(
            self.out,
            "\\begin{{center}}\n\\begin{{tabular}}{{{cols}}}\n\\toprule"
        );
        let head: Vec<String> = t
            .columns
            .iter()
            .map(|(n, u)| {
                let mut s = format!("${}$", texify_math(n));
                if let Some(u) = u {
                    let _ = write!(s, " ({})", escape_text(u));
                }
                s
            })
            .collect();
        let _ = writeln!(self.out, "{} \\\\\n\\midrule", head.join(" & "));
        for row in &t.rows {
            let cells: Vec<String> = row.iter().map(|c| escape_text(c)).collect();
            let _ = writeln!(self.out, "{} \\\\", cells.join(" & "));
        }
        let _ = writeln!(self.out, "\\bottomrule\n\\end{{tabular}}\n\\end{{center}}");
    }

    fn list(&mut self, l: &List) {
        let env = if l.ordered { "enumerate" } else { "itemize" };
        let _ = writeln!(self.out, "\\begin{{{env}}}");
        for item in &l.items {
            let t = self.inlines(&item.content);
            let _ = writeln!(self.out, "\\item {t}");
            for c in &item.children {
                self.list(c);
            }
        }
        let _ = writeln!(self.out, "\\end{{{env}}}");
    }

    fn table(&mut self, t: &Table) {
        let cols: String = (0..t.header.len())
            .map(|i| match t.aligns.get(i) {
                Some(Align::Right) => 'r',
                Some(Align::Center) => 'c',
                _ => 'l',
            })
            .collect();
        let _ = writeln!(
            self.out,
            "\\begin{{center}}\n\\begin{{tabular}}{{{cols}}}\n\\toprule"
        );
        let head: Vec<String> = t.header.iter().map(|c| self.inlines(c)).collect();
        let _ = writeln!(self.out, "{} \\\\\n\\midrule", head.join(" & "));
        for row in &t.rows {
            let cells: Vec<String> = row.iter().map(|c| self.inlines(c)).collect();
            let _ = writeln!(self.out, "{} \\\\", cells.join(" & "));
        }
        let _ = writeln!(self.out, "\\bottomrule\n\\end{{tabular}}\n\\end{{center}}");
    }

    fn display(&mut self, b: &Block, m: &DisplayMath) {
        let results = self.provider.results_for(b, m);
        let mut rows: Vec<String> = vec![];
        for (i, line) in m.ast.lines.iter().enumerate() {
            let text = strip_label(&self.src(line.span))
                .replace("%noeval", "")
                .trim()
                .to_string();
            if text.is_empty() {
                continue;
            }
            let mut text = texify_math(&text);
            if let Some(r) = results.iter().find(|r| r.line == i) {
                fill_result(&mut text, &r.value);
            }
            rows.push(text);
        }
        if rows.is_empty() {
            return;
        }
        let label = m.label.as_deref().map(bib_key);
        let aligned = m.ast.aligned || rows.iter().any(|r| r.contains('&'));
        let env = match (rows.len() > 1, aligned) {
            (_, true) => "align",
            (false, false) => "equation",
            (true, false) => "gather",
        };
        let star = if label.is_some() { "" } else { "*" };
        let _ = writeln!(self.out, "\\begin{{{env}{star}}}");
        let n = rows.len();
        for (i, r) in rows.iter().enumerate() {
            let sep = if i + 1 < n { " \\\\" } else { "" };
            let mut extra = String::new();
            if i + 1 == n {
                if let Some(l) = &label {
                    let _ = write!(extra, " \\label{{{l}}}");
                } else if let Some(num) = m.number {
                    let _ = write!(extra, " \\tag{{{num}}}");
                }
            }
            let _ = writeln!(self.out, "  {r}{extra}{sep}");
        }
        let _ = writeln!(self.out, "\\end{{{env}{star}}}");
        if self.opts.include_steps && !self.in_comment {
            self.steps(b);
        }
    }

    /// The working of `= ?!` queries, one line per step.
    fn steps(&mut self, b: &Block) {
        let Some(br) = self.ev.block(b.id) else {
            return;
        };
        for (_, steps) in &br.steps {
            let _ = writeln!(self.out, "\\begin{{quote}}\\small");
            for step in steps.iter() {
                let mut line = format!(
                    "\\textit{{{}}}: ${}$ $\\to$ ${}$",
                    escape_text(&step.rule),
                    texify_math(&step.before.to_latex()),
                    texify_math(&step.after.to_latex())
                );
                if let Some(n) = &step.note {
                    let _ = write!(line, " ({})", escape_text(n));
                }
                let _ = writeln!(self.out, "{line}\\\\");
            }
            let _ = writeln!(self.out, "\\end{{quote}}");
        }
    }

    fn figure(&mut self, b: &Block) {
        let Some(png) = self.provider.figure_png(b.id, 520.0, 380.0, 3.0) else {
            let _ = writeln!(self.out, "\\textit{{(graph not drawn)}}\n");
            return;
        };
        self.figure_count += 1;
        let file = format!("{}-fig{}.png", self.name, self.figure_count);
        self.files.push((file.clone(), png));
        let caption = match &b.kind {
            BlockKind::Figure(f) if !f.caption.is_empty() => {
                let t = self.inlines(&f.caption);
                format!("\\caption{{{t}}}\n")
            }
            _ => String::new(),
        };
        let _ = writeln!(
            self.out,
            "\\begin{{figure}}[htbp]\n\\centering\n\\includegraphics[width=0.8\\linewidth]{{{file}}}\n{caption}\\end{{figure}}"
        );
    }

    fn inlines(&mut self, items: &[Inline]) -> String {
        let mut out = String::new();
        for i in items {
            match &i.kind {
                InlineKind::Text(t) => out.push_str(&escape_text(t)),
                InlineKind::SoftBreak => out.push('\n'),
                InlineKind::Emphasis(x) => {
                    let _ = write!(out, "\\emph{{{}}}", self.inlines(x));
                }
                InlineKind::Strong(x) => {
                    let _ = write!(out, "\\textbf{{{}}}", self.inlines(x));
                }
                InlineKind::Math(m) => {
                    let mut text = texify_math(self.src(m.raw).trim());
                    if let Some(r) = self.provider.results_for_ast(&m.ast).first() {
                        fill_result(&mut text, &r.value);
                    }
                    let _ = write!(out, "${text}$");
                }
                InlineKind::Code(c) => {
                    let _ = write!(out, "\\texttt{{{}}}", escape_text(c));
                }
                InlineKind::WikiLink { path, alias } => {
                    let _ = write!(
                        out,
                        "\\texttt{{{}}}",
                        escape_text(alias.as_deref().unwrap_or(path))
                    );
                }
                InlineKind::Link { text, url } => {
                    let t = self.inlines(text);
                    if url.starts_with("http://") || url.starts_with("https://") {
                        let _ = write!(
                            out,
                            "\\href{{{}}}{{{t}}}",
                            url.replace(['{', '}', '\\'], "")
                        );
                    } else {
                        out.push_str(&t);
                    }
                }
                InlineKind::FootnoteRef(name) => {
                    if let Some(t) = self.notes.get(name) {
                        if !self.in_comment {
                            let _ = write!(out, "\\footnote{{{t}}}");
                        }
                    }
                }
                InlineKind::EqRef { label, .. } => {
                    let _ = write!(out, "\\eqref{{{}}}", bib_key(label));
                }
                InlineKind::Escape(c) => out.push_str(&escape_text(&c.to_string())),
                InlineKind::Cite(cites) => {
                    let keys: Vec<String> = cites.iter().map(|c| bib_key(&c.key)).collect();
                    let _ = write!(out, "\\cite{{{}}}", keys.join(","));
                }
            }
        }
        out
    }
}
