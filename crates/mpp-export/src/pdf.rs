//! PDF export: the page is composed from the same layout boxes as the equation
//! view and the HTML page (Latin Modern glyph outlines, TeX-style maths), drawn
//! as one vector SVG per page and turned into PDF with `svg2pdf`.

mod compose;

use std::collections::HashMap;

use mpp_eval::Evaluated;
use mpp_render::{DisplayList, SvgOptions};
use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref, TextStr};
use svg2pdf::usvg;

use crate::{provider::EvalProvider, ExportError, ExportOptions};

/// Paper sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageSize {
    A4,
    Letter,
}

impl PageSize {
    /// Width and height in points.
    pub fn points(self) -> (f32, f32) {
        match self {
            PageSize::A4 => (595.276, 841.89),
            PageSize::Letter => (612.0, 792.0),
        }
    }
}

/// The pages of the PDF as SVG documents (what the PDF draws), for previews and tests.
pub fn pdf_pages_svg(ev: &Evaluated, p: &EvalProvider<'_>, o: &ExportOptions) -> Vec<String> {
    compose::compose(ev, p, o)
        .iter()
        .map(|pg| page_svg(pg, o))
        .collect()
}

fn page_svg(page: &compose::Page, o: &ExportOptions) -> String {
    let (w, h) = o.page.points();
    let dl = DisplayList {
        width: w,
        height: h,
        depth: 0.0,
        baseline: h,
        font_size: 11.0,
        items: page.items.clone(),
        hits: vec![],
    };
    let svg = mpp_render::to_svg_with(
        &dl,
        &SvgOptions {
            outlines: true,
            padding: 0.0,
        },
    );
    let mut extra = String::new();
    for f in &page.figures {
        extra.push_str(&f.svg_fragment());
    }
    match svg.rfind("</svg>") {
        Some(i) => format!("{}{extra}</svg>\n", &svg[..i]),
        None => svg,
    }
}

fn font_db() -> usvg::fontdb::Database {
    let mut db = usvg::fontdb::Database::new();
    for f in mpp_render::FontId::ALL {
        db.load_font_data(f.bytes().to_vec());
    }
    db
}

pub(crate) fn export(
    ev: &Evaluated,
    p: &EvalProvider<'_>,
    o: &ExportOptions,
) -> Result<Vec<u8>, ExportError> {
    let pages = compose::compose(ev, p, o);
    let (pw, ph) = o.page.points();
    let options = usvg::Options {
        fontdb: std::sync::Arc::new(font_db()),
        ..usvg::Options::default()
    };

    let mut next = 1;
    let mut alloc = || {
        let r = Ref::new(next);
        next += 1;
        r
    };
    let catalog_id = alloc();
    let tree_id = alloc();
    let info_id = alloc();
    let mut pdf = Pdf::new();
    pdf.catalog(catalog_id).pages(tree_id);

    let mut kids = vec![];
    let mut chunks = vec![];
    for page in &pages {
        let svg = page_svg(page, o);
        let tree = usvg::Tree::from_str(&svg, &options)
            .map_err(|e| ExportError::Pdf(format!("could not read a page: {e}")))?;
        let (chunk, svg_ref) = svg2pdf::to_chunk(&tree, svg2pdf::ConversionOptions::default())
            .map_err(|e| ExportError::Pdf(format!("could not draw a page: {e}")))?;
        let mut map = HashMap::new();
        let chunk = chunk.renumber(|old| *map.entry(old).or_insert_with(&mut alloc));
        let Some(svg_id) = map.get(&svg_ref).copied() else {
            return Err(ExportError::Pdf("a page came out empty".into()));
        };
        let page_id = alloc();
        let content_id = alloc();
        kids.push(page_id);
        let mut pg = pdf.page(page_id);
        pg.media_box(Rect::new(0.0, 0.0, pw, ph));
        pg.parent(tree_id);
        pg.contents(content_id);
        pg.resources().x_objects().pair(Name(b"P"), svg_id);
        pg.finish();
        let mut content = Content::new();
        content
            .transform([pw, 0.0, 0.0, ph, 0.0, 0.0])
            .x_object(Name(b"P"));
        pdf.stream(content_id, &content.finish());
        chunks.push(chunk);
    }
    pdf.pages(tree_id)
        .kids(kids.iter().copied())
        .count(i32::try_from(kids.len()).unwrap_or(i32::MAX));
    let title = ev
        .document
        .blocks
        .iter()
        .find_map(|b| match &b.kind {
            mpp_syntax::document::BlockKind::Heading { content, .. } => Some(plain(content)),
            mpp_syntax::document::BlockKind::Title(t)
                if t.kind == mpp_syntax::document::TitleKind::Title && !t.is_empty() =>
            {
                Some(plain(&t.content))
            }
            _ => None,
        })
        .unwrap_or_else(|| "Math++ document".into());
    pdf.document_info(info_id)
        .title(TextStr(&title))
        .creator(TextStr("Math++"));
    for c in &chunks {
        pdf.extend(c);
    }
    Ok(pdf.finish())
}

pub(crate) fn plain(items: &[mpp_syntax::document::Inline]) -> String {
    use mpp_syntax::document::InlineKind;
    let mut s = String::new();
    for i in items {
        match &i.kind {
            InlineKind::Text(t) | InlineKind::Code(t) => s.push_str(t),
            InlineKind::SoftBreak => s.push(' '),
            InlineKind::Emphasis(x) | InlineKind::Strong(x) => s.push_str(&plain(x)),
            InlineKind::Link { text, .. } => s.push_str(&plain(text)),
            InlineKind::Escape(c) => s.push(*c),
            _ => {}
        }
    }
    s.trim().to_string()
}
