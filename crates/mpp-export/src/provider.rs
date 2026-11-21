//! Feeds evaluated results, graphs and data tables into the page renderer.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use mpp_eval::{Evaluated, Importer, LineKind, QueryMode};
use mpp_plot::{Figure, PlotTheme};
use mpp_render::{DataRows, LineResult, ResultKind, ResultProvider, ResultValue, StepLine};
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

    /// Redraws a graph at slider values set in the editor.
    pub fn set_slider_values(
        &mut self,
        id: BlockId,
        values: &std::collections::BTreeMap<String, f64>,
    ) {
        if let Some(fig) = self.figures.get_mut(&id) {
            fig.rebuild(values);
        }
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
                        r.approx
                            .as_deref()
                            .map(approx_latex)
                            .or_else(|| r.latex.clone())
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

    fn display_steps(&self, block: &Block) -> Vec<Vec<StepLine>> {
        let Some(br) = self.ev.block(block.id) else {
            return vec![];
        };
        br.steps
            .iter()
            .map(|(_, steps)| {
                steps
                    .iter()
                    .map(|st| StepLine {
                        rule: st.rule.clone(),
                        before: st.before.to_latex(),
                        after: st.after.to_latex(),
                        note: st.note.clone(),
                    })
                    .collect()
            })
            .collect()
    }

    fn display_errors(&self, block: &Block) -> Vec<String> {
        if !self.results {
            return vec![];
        }
        let Some(br) = self.ev.block(block.id) else {
            return vec![];
        };
        br.diagnostics
            .iter()
            .filter(|d| {
                // Errors, and the warnings that say a question has no real answer.
                d.severity == mpp_syntax::Severity::Error
                    || ["kept as written", "not evaluated", "got no answer"]
                        .iter()
                        .any(|w| d.message.contains(w))
            })
            .map(|d| d.message.clone())
            .collect()
    }

    fn sentence_result(&self, block: &Block) -> Option<String> {
        if !self.results {
            return None;
        }
        let br = self.ev.block(block.id)?;
        br.values
            .iter()
            .filter(|v| matches!(v.kind, LineKind::Query { .. }) && !v.auto && !v.pending)
            .find_map(|v| v.latex.clone())
    }
}

/// A numerical answer as typeset maths. The `≈` text of a line is plain Unicode
/// (`686.4655 N`, `1.2097 × 10⁻⁴ yr⁻¹`, `[50.04 kg, 75.96 kg]`, `{4.605 s}`); in an
/// equation each number stays a number, a power of ten becomes `\times 10^{…}`, and a
/// unit after a number is set upright after a thin space.
pub fn approx_latex(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let is_sep = |c: char| matches!(c, ',' | ';' | '[' | ']' | '(' | ')' | '{' | '}');
    while i < chars.len() {
        let c = chars[i];
        let starts_number = c.is_ascii_digit()
            || (matches!(c, '-' | '−') && chars.get(i + 1).is_some_and(char::is_ascii_digit));
        if starts_number {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let number: String = chars[start..i].iter().collect();
            out.push_str(&number.replace('−', "-"));
            // ` × 10⁻⁴`
            let rest: String = chars[i..].iter().collect();
            if let Some(after) = rest.strip_prefix(" × 10") {
                let exp: String = after
                    .chars()
                    .take_while(|c| superscript(*c).is_some())
                    .collect();
                if !exp.is_empty() {
                    let digits: String = exp.chars().filter_map(superscript).collect();
                    out.push_str(&format!(" \\times 10^{{{digits}}}"));
                    i += " × 10".chars().count() + exp.chars().count();
                }
            }
            // ` kg`, ` m/s^2`, ` yr⁻¹`: a unit right after the number.
            if chars.get(i) == Some(&' ') {
                let unit: String = chars[i + 1..]
                    .iter()
                    .take_while(|c| !c.is_whitespace() && !is_sep(**c))
                    .collect();
                let ascii = unit
                    .chars()
                    .map(|c| match superscript(c) {
                        Some(d) => format!("^{d}"),
                        None => c.to_string(),
                    })
                    .collect::<String>()
                    .replace("^-^", "^-")
                    .replace('·', "*");
                if let (false, Ok(u)) = (unit.is_empty(), mpp_core::units::parse_unit(&ascii)) {
                    out.push_str("\\,");
                    out.push_str(&u.latex());
                    i += 1 + unit.chars().count();
                }
            }
            continue;
        }
        match c {
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '∞' => out.push_str("\\infty "),
            'π' => out.push_str("\\pi "),
            '∅' => out.push_str("\\emptyset "),
            '±' => out.push_str("\\pm "),
            c if c.is_ascii_alphabetic() => {
                // A word on its own (`i` of a complex number, `x =`): upright text
                // only when it is longer than a letter.
                let start = i;
                while i + 1 < chars.len() && chars[i + 1].is_ascii_alphabetic() {
                    i += 1;
                }
                let word: String = chars[start..=i].iter().collect();
                if word.chars().count() == 1 {
                    out.push_str(&word);
                } else {
                    out.push_str(&format!("\\mathrm{{{word}}}"));
                }
            }
            c if c.is_ascii() => out.push(c),
            c => out.push_str(&format!("\\text{{{c}}}")),
        }
        i += 1;
    }
    out
}

/// The digit or sign a superscript character stands for.
fn superscript(c: char) -> Option<char> {
    Some(match c {
        '⁰' => '0',
        '¹' => '1',
        '²' => '2',
        '³' => '3',
        '⁴' => '4',
        '⁵' => '5',
        '⁶' => '6',
        '⁷' => '7',
        '⁸' => '8',
        '⁹' => '9',
        '⁻' => '-',
        _ => return None,
    })
}

#[cfg(test)]
mod approx_tests {
    use super::approx_latex;

    #[test]
    fn numerical_answers_are_typeset() {
        assert_eq!(approx_latex("686.4655 N"), r"686.4655\,\mathrm{N}");
        assert_eq!(
            approx_latex("[50.03664728 kg, 75.96335272 kg]"),
            r"[50.03664728\,\mathrm{kg}, 75.96335272\,\mathrm{kg}]"
        );
        assert_eq!(approx_latex("1.2097 × 10⁻⁴"), r"1.2097 \times 10^{-4}");
        assert_eq!(
            approx_latex("{4.605170186 s}"),
            r"\{4.605170186\,\mathrm{s}\}"
        );
        assert_eq!(approx_latex("−0.5"), "-0.5");
    }
}
