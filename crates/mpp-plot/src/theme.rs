//! Plot themes. Every colour a plot uses comes from here; nothing else in the crate
//! names a colour.

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
    /// Dark text on white.
    pub fn light() -> PlotTheme {
        PlotTheme {
            name: "light".into(),
            background: Color::hex("#ffffff"),
            grid: Color::hex("#e4e7eb"),
            axes: Color::hex("#2b3036"),
            text: Color::hex("#1f2328"),
            legend_background: Color::hex("#ffffff").with_alpha(230),
            legend_border: Color::hex("#b8bec6"),
            palette: [
                "#1f5fbf", "#d9480f", "#2b8a3e", "#862e9c", "#c2255c", "#0b7285", "#a16207",
                "#495057",
            ]
            .map(Color::hex)
            .to_vec(),
            surface_colormap: ["#440154", "#3b528b", "#21918c", "#5ec962", "#e8d82a"]
                .map(Color::hex)
                .to_vec(),
            wireframe: Color::hex("#1f2328").with_alpha(110),
            region_alpha: 70,
            font: FONT.into(),
            math_font: MATH_FONT.into(),
        }
    }

    /// Light text on a dark background, with brighter curve colours.
    pub fn dark() -> PlotTheme {
        PlotTheme {
            name: "dark".into(),
            background: Color::hex("#16181d"),
            grid: Color::hex("#2b2f37"),
            axes: Color::hex("#c9ced6"),
            text: Color::hex("#e6e9ee"),
            legend_background: Color::hex("#16181d").with_alpha(230),
            legend_border: Color::hex("#4a505b"),
            palette: [
                "#6ea8fe", "#ff922b", "#69db7c", "#da77f2", "#ff6b9d", "#3bc9db", "#ffd43b",
                "#ced4da",
            ]
            .map(Color::hex)
            .to_vec(),
            surface_colormap: ["#6c5ce7", "#3f8fd2", "#22b8a8", "#8fd66a", "#f5e663"]
                .map(Color::hex)
                .to_vec(),
            wireframe: Color::hex("#e6e9ee").with_alpha(100),
            region_alpha: 80,
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
    fn colormap_endpoints() {
        let t = PlotTheme::light();
        assert_eq!(t.colormap(0.0), t.surface_colormap[0]);
        assert_eq!(t.colormap(1.0), *t.surface_colormap.last().unwrap());
        assert_eq!(t.colormap(f64::NAN), t.surface_colormap[0]);
    }
}
