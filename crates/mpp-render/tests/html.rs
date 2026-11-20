//! The static page: prose, inline and display maths, numbers, theorems,
//! results from a provider.

use mpp_render::{
    render_html, render_html_with, HtmlOptions, LineResult, NoResults, ResultProvider, Theme,
};
use mpp_syntax::document::{parse_document, Block, DisplayMath};

const PAPER: &str = r"# Projectile motion

Let $g \equiv 9.81$ be gravity and $v_0 := 20$ the launch speed. The height of the
projectile after travelling a distance $x$ is given in \eqref{traj}.

$$ y(x) := x \tan\theta - \frac{g x^2}{2 v_0^2 \cos^2\theta} \label{traj} $$

## Results

**Theorem.** For every $n \ge 1$ the partial sums are bounded by $2$.

**Proof.** Compare with a telescoping series. ∎

$$ \sum_{n=1}^{\infty} \frac{1}{n^2} = ? $$

- the range is $R = v_0^2 \sin 2\theta / g$;
- the time of flight is $T = \frac{2 v_0 \sin\theta}{g}$.

| Quantity | Value |
|---|--:|
| $g$ | 9.81 |

> A quotation[^q].

[^q]: From a textbook.
";

struct Basel;

impl ResultProvider for Basel {
    fn display_results(&self, _block: &Block, math: &DisplayMath) -> Vec<LineResult> {
        match &math.ast.lines[0].statement {
            mpp_syntax::math::Statement::Query { .. } => {
                vec![LineResult::filled(0, r"\frac{\pi^2}{6}")]
            }
            _ => vec![],
        }
    }
}

#[test]
fn renders_a_paper() {
    let doc = parse_document(PAPER);
    let html = render_html(&doc, &Basel);
    assert!(html.starts_with("<!DOCTYPE html>"));
    assert!(html.contains("<title>Projectile motion</title>"));
    assert!(html.contains("<h1>Projectile motion</h1>"));
    assert!(html.contains("<h2>Results</h2>"));
    assert!(html.contains("class=\"eq\" id=\"eq-1\""));
    assert!(html.contains("<span class=\"eq-num\">(1)</span>"));
    assert!(html.contains("href=\"#eq-1\">(1)</a>"));
    assert!(
        html.contains("<div class=\"theorem slanted\"><span class=\"thm-head\">Theorem 1.</span>")
    );
    assert!(html.contains("class=\"qed\""));
    assert!(html.contains("<svg class=\"m\""));
    assert!(html.contains("<svg class=\"dm\""));
    assert!(html.contains("<table>"));
    assert!(html.contains("style=\"text-align:right\""));
    assert!(html.contains("<blockquote>"));
    assert!(html.contains("id=\"fn-q\""));
    assert!(html.contains("@font-face"));
    // The filled-in result is drawn in the result colour.
    assert!(html.contains("fill=\"var(--mpp-result, #1f5fb8)\""));
    // Ink follows the theme, with a dark variant.
    assert!(html.contains("fill=\"var(--mpp-text, "));
    assert!(html.contains("@media (prefers-color-scheme: dark)"));
    assert!(html.contains(&format!(
        "--mpp-background: {};",
        Theme::DARK.background.hex()
    )));
    assert!(html.contains("font-family: 'Latin Modern Roman'"));
    // Glyphs are outlines shared through one <defs>.
    assert_eq!(html.matches("<defs>").count(), 1);
    assert!(!html.contains("<text"));
    let path = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("mpp-render-png");
    let _ = std::fs::create_dir_all(&path);
    let _ = std::fs::write(path.join("paper.html"), &html);
}

#[test]
fn fonts_can_be_left_out() {
    let doc = parse_document("Just $x$.");
    let html = render_html_with(
        &doc,
        &NoResults,
        &HtmlOptions {
            embed_fonts: false,
            ..HtmlOptions::default()
        },
    );
    assert!(!html.contains("@font-face"));
    assert!(html.len() < 100_000);
}

struct Table;

impl ResultProvider for Table {
    fn data_table(
        &self,
        _block: &Block,
        _data: &mpp_syntax::document::DataImport,
    ) -> Option<mpp_render::DataRows> {
        Some(mpp_render::DataRows {
            columns: vec![
                ("t".into(), Some("s".into())),
                ("x".into(), Some("m".into())),
            ],
            rows: vec![
                vec!["0".into(), "1.5".into()],
                vec!["1".into(), "2.5".into()],
            ],
        })
    }
}

#[test]
fn assume_and_data_blocks_render() {
    let doc = parse_document(
        "Assume $x > 0$ and $n = 2$.

Data: $T$ from [[m.csv]].
",
    );
    let plain = render_html(&doc, &NoResults);
    assert!(plain.contains("<p class=\"assume\">Assume "), "{plain}");
    assert!(plain.contains(" and "));
    assert!(plain.contains("<p class=\"data\">Data "));
    let tab = render_html(&doc, &Table);
    assert!(tab.contains("<table class=\"data\">"));
    assert!(tab.contains("<td>2.5</td>"));
    assert!(tab.contains("(s)"));
}
