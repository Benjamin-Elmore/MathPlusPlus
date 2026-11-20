//! Plot themes. Every colour a plot uses comes from here; nothing else in the crate
//! names a colour.

use mpp_render::Theme as RenderTheme;

use crate::color::Color;

/// Colours and fonts for a figure.
#[derive(Clone, Debug, PartialEq)]
pub struct PlotTheme {
    /// Short name (`light`, `dark`).
    pub name: String,
    /// Page behind the plot.
    pub background: Color,
    /// Light grid lines at the major ticks.
    pub grid: Color,
    /// Axis lines and tick marks.
    pub axes: Color,
    /// Tick labels, axis names and captions.
    pub text: Color,
    /// Fill behind the legend.
    pub legend_background: Color,
    /// Outline of the legend and of 3D box edges that sit behind the surface.
    pub legend_border: Color,
    /// Colours of successive curves.
    pub palette: Vec<Color>,
    /// Gradient stops from low to high, used to colour surfaces by height.
    pub surface_colormap: Vec<Color>,
    /// Wireframe and mesh edges on surfaces.
    pub wireframe: Color,
    /// Opacity (0 to 255) of shaded regions.
    pub region_alpha: u8,
    /// Font for tick labels and prose.
    pub font: String,
    /// Font for maths labels (the IDE typesets these from their LaTeX).
    pub math_font: String,
}

impl PlotTheme {
    /// Dark text on the light page.
    pub fn light() -> PlotTheme {
        PlotTheme::from_render(&RenderTheme::LIGHT)
    }

    /// Light text on a dark background, with brighter curve colours.
    pub fn dark() -> PlotTheme {
        PlotTheme::from_render(&RenderTheme::DARK)
    }

    /// The plot theme that goes with a document theme: background, ink, grid, axes and
    /// the eight curve colours all come from the shared theme, so a figure always matches
    /// the page around it. Only the colours no document element uses (the height colour
    /// map, the wireframe, how opaque shaded regions are) are chosen here.
    pub fn from_render(t: &RenderTheme) -> PlotTheme {
        let c = |c: mpp_render::Color| Color::rgb(c.r, c.g, c.b);
        let (background, text, axes, grid) = (c(t.background), c(t.text), c(t.axis), c(t.grid));
        let dark = background.luminance() < 0.2;
        let stops: [&str; 5] = if dark {
            ["#6c5ce7", "#3f8fd2", "#22b8a8", "#8fd66a", "#f5e663"]
        } else {
            ["#440154", "#3b528b", "#21918c", "#5ec962", "#e8d82a"]
        };
        PlotTheme {
            name: if dark { "dark" } else { "light" }.into(),
            background,
            grid,
            axes,
            text,
            legend_background: background.with_alpha(230),
            legend_border: axes.lerp(background, 0.55),
            palette: t.series.iter().map(|s| c(*s)).collect(),
            surface_colormap: stops.map(Color::hex).to_vec(),
            wireframe: text.with_alpha(if dark { 100 } else { 110 }),
            region_alpha: if dark { 80 } else { 70 },
            font: FONT.into(),
            math_font: MATH_FONT.into(),
        }
    }

    /// The colour of curve number `i` (wraps around the palette).
    pub fn series(&self, i: usize) -> Color {
        if self.palette.is_empty() {
            return self.text;
        }
        self.palette[i % self.palette.len()]
    }

    /// The surface colour at `t` in 0..=1 along the colour map.
    pub fn colormap(&self, t: f64) -> Color {
        let stops = &self.surface_colormap;
        match stops.len() {
            0 => return self.text,
            1 => return stops[0],
            _ => {}
        }
        let t = if t.is_finite() {
            t.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let x = t * (stops.len() - 1) as f64;
        let i = (x.floor() as usize).min(stops.len() - 2);
        stops[i].lerp(stops[i + 1], (x - i as f64) as f32)
    }
}

impl Default for PlotTheme {
    fn default() -> Self {
        PlotTheme::light()
    }
}

const FONT: &str = "Latin Modern Roman, STIX Two Text, Times New Roman, serif";
const MATH_FONT: &str = "Latin Modern Math, STIX Two Math, Cambria Math, serif";

#[cfg(test)]
mod tests {
    use super::*;

    fn check(theme: &PlotTheme) {
        // Text and axes meet WCAG AA for normal text (4.5:1).
        assert!(theme.text.contrast(theme.background) >= 4.5, "text");
        assert!(theme.axes.contrast(theme.background) >= 4.5, "axes");
        // Curves meet the 3:1 non-text contrast.
        for (i, c) in theme.palette.iter().enumerate() {
            assert!(
                c.contrast(theme.background) >= 3.0,
                "{} palette {i}: {}",
                theme.name,
                c.contrast(theme.background)
            );
        }
        for c in &theme.surface_colormap {
            assert!(c.contrast(theme.background) >= 1.3);
        }
    }

    #[test]
    fn light_meets_contrast_targets() {
        check(&PlotTheme::light());
    }

    #[test]
    fn dark_meets_contrast_targets() {
        check(&PlotTheme::dark());
    }

    #[test]
    fn colours_come_from_the_shared_theme() {
        for (plot, render) in [
            (PlotTheme::light(), &RenderTheme::LIGHT),
            (PlotTheme::dark(), &RenderTheme::DARK),
        ] {
            assert_eq!(plot.background.to_hex(), render.background.hex());
            assert_eq!(plot.text.to_hex(), render.text.hex());
            assert_eq!(plot.axes.to_hex(), render.axis.hex());
            assert_eq!(plot.grid.to_hex(), render.grid.hex());
            let series: Vec<String> = render.series.iter().map(|c| c.hex()).collect();
            let palette: Vec<String> = plot.palette.iter().map(|c| c.to_hex()).collect();
            assert_eq!(series, palette);
            assert_eq!(PlotTheme::from_render(render), plot);
        }
        assert_eq!(PlotTheme::light().name, "light");
        assert_eq!(PlotTheme::dark().name, "dark");
    }
    #[test]
    fn colormap_endpoints() {
        let t = PlotTheme::light();
        assert_eq!(t.colormap(0.0), t.surface_colormap[0]);
        assert_eq!(t.colormap(1.0), *t.surface_colormap.last().unwrap());
        assert_eq!(t.colormap(f64::NAN), t.surface_colormap[0]);
    }
}
