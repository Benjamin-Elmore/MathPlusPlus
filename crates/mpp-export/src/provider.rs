//! Feeds evaluated results, graphs and data tables into the page renderer.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use mpp_eval::{Evaluated, Importer, LineKind, QueryMode};
use mpp_plot::{Figure, PlotTheme};
use mpp_render::{DataRows, LineResult, ResultKind, ResultProvider, ResultValue};
use mpp_syntax::document::{Block, BlockId, DataImport, DisplayMath, Figure as FigureBlock};
use mpp_syntax::math::{MathAst, RelOp};

/// Width and height graphs are drawn at, in pixels.
pub const FIGURE_SIZE: (f32, f32) = (520.0, 380.0);

/// A [`ResultProvider`] over an evaluated document.
pub struct EvalProvider<'a> {
    ev: &'a Evaluated,
    importer: Arc<dyn Importer>,
    base_dir: Option<PathBuf>,
    figures: HashMap<BlockId, Figure>,
    theme: PlotTheme,
    results: bool,
}

impl<'a> EvalProvider<'a> {
    /// Builds the provider; `base_dir` is the folder of the document (for data files).
    pub fn new(
        ev: &'a Evaluated,
        importer: Arc<dyn Importer>,
        base_dir: Option<PathBuf>,
        dark: bool,
        results: bool,
    ) -> Self {
        let figures = mpp_plot::figures(ev)
            .into_iter()
            .filter_map(|f| f.figure.ok().map(|fig| (f.block, fig)))
            .collect();
        EvalProvider {
            ev,
            importer,
            base_dir,
            figures,
            theme: if dark {
                PlotTheme::dark()
            } else {
                PlotTheme::light()
            },
            results,
        }
    }

    /// The graph of a `Figure:` block, if it could be built.
    pub fn figure(&self, id: BlockId) -> Option<&Figure> {
        self.figures.get(&id)
    }

    /// The graph's SVG at a size.
    pub fn figure_svg_sized(&self, id: BlockId, w: f32, h: f32) -> Option<String> {
        let fig = self.figures.get(&id)?;
        Some(mpp_plot::to_svg(&fig.scene(w, h, &self.theme)))
    }

    /// The graph as a PNG at a size and scale.
    pub fn figure_png(&self, id: BlockId, w: f32, h: f32, scale: f32) -> Option<Vec<u8>> {
        let fig = self.figures.get(&id)?;
        mpp_plot::to_png(&fig.scene(w, h, &self.theme), scale).ok()
    }

    /// The columns and rows of a `Data:` block.
    pub fn table(&self, d: &DataImport) -> Option<DataRows> {
        let (_, text) = self
            .importer
            .read_data(self.base_dir.as_deref(), &d.path)
            .ok()?;
        let table = mpp_eval::data::parse_csv(&text).ok()?;
        let columns = table
            .columns
            .iter()
            .map(|c| (c.name.clone(), c.unit.clone()))
            .collect();
        let rows = (0..table.rows())
            .map(|r| {
                table
                    .columns
                    .iter()
                    .map(|c| c.values[r].to_string())
                    .collect()
            })
            .collect();
        Some(DataRows { columns, rows })
    }

    /// The evaluated results to fill into one display block.
    pub fn results_for(&self, block: &Block, math: &DisplayMath) -> Vec<LineResult> {
        let Some(br) = self.ev.block(block.id) else {
            return vec![];
        };
        self.results_in(&br.values, &math.ast)
    }

    /// The evaluated results of the lines of an inline formula.
    pub fn results_for_ast(&self, ast: &MathAst) -> Vec<LineResult> {
        let values: Vec<&mpp_eval::LineResult> = self
            .ev
            .blocks
            .iter()
            .filter(|b| b.span.start <= ast.span.start && ast.span.end <= b.span.end)
            .flat_map(|b| b.values.iter())
            .collect();
        self.results_among(values.into_iter(), ast)
    }

    fn results_in(&self, values: &[mpp_eval::LineResult], ast: &MathAst) -> Vec<LineResult> {
        self.results_among(values.iter(), ast)
    }

    fn results_among<'v>(
        &self,
        values: impl Iterator<Item = &'v mpp_eval::LineResult> + Clone,
        ast: &MathAst,
    ) -> Vec<LineResult> {
        if !self.results {
            return vec![];
        }
        let mut out = vec![];
        for (i, line) in ast.lines.iter().enumerate() {
            let Some(r) = values.clone().find(|v| v.span == line.span) else {
                continue;
            };
            // Automatic answers are a preview in the editor, never part of the
            // document; one still being worked out has nothing to show.
            if r.auto || r.pending {
                continue;
            }
            match &r.kind {
                LineKind::Query { mode, .. } => {
                    // `latex`, `text` and `approx` are already in the line's display
                    // format (`%sf 4`, `%sci` …), recorded in `number_format`.
                    let approx = matches!(mode, QueryMode::Approx);
                    // An exact query shows its exact answer only: the decimal the
                    // editor adds beside it is a preview, and `≈ ?` asks for one.
                    let latex = if approx {
                        r.approx.clone().or_else(|| r.latex.clone())
                    } else {
                        r.latex.clone()
                    };
                    if let Some(latex) = latex {
                        out.push(LineResult {
                            line: i,
                            value: ResultValue::Latex(latex),
                            kind: ResultKind::Filled,
                            relation: approx.then_some(RelOp::Approx),
                        });
                    }
                }
                LineKind::Check => {
                    if let Some(v) = &r.verdict {
                        out.push(LineResult::filled(i, v.mark()));
                    }
                }
                _ => {}
            }
        }
        out
    }
}

impl ResultProvider for EvalProvider<'_> {
    fn display_results(&self, block: &Block, math: &DisplayMath) -> Vec<LineResult> {
        self.results_for(block, math)
    }

    fn inline_results(&self, ast: &MathAst) -> Vec<LineResult> {
        self.results_for_ast(ast)
    }

    fn figure_svg(&self, block: &Block, _figure: &FigureBlock) -> Option<String> {
        self.figure_svg_sized(block.id, FIGURE_SIZE.0, FIGURE_SIZE.1)
    }

    fn data_table(&self, _block: &Block, data: &DataImport) -> Option<DataRows> {
        self.table(data)
    }

    fn references_source(&self, path: &str) -> Option<String> {
        let (_, text) = self
            .importer
            .read_data(self.base_dir.as_deref(), path)
            .ok()?;
        Some(text)
    }
}
