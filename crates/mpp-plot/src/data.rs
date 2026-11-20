//! Data layers for later statistics features: scatter plots and histograms.

use crate::sample::P;
use crate::spec::{Geometry, Layer};

/// A scatter plot of `(x, y)` pairs (non-finite pairs are dropped).
pub fn scatter(points: &[P]) -> Layer {
    Layer::new(Geometry::Scatter(
        points
            .iter()
            .copied()
            .filter(|p| p[0].is_finite() && p[1].is_finite())
            .collect(),
    ))
}

/// A histogram of `values` in `bins` equal-width bins over their range.
/// Returns `None` when there is no finite data.
pub fn histogram(values: &[f64], bins: usize) -> Option<Layer> {
    let data: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    let lo = data.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = data.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if data.is_empty() {
        return None;
    }
    let bins = bins.clamp(1, 1000);
    let (lo, hi) = if hi > lo {
        (lo, hi)
    } else {
        (lo - 0.5, hi + 0.5)
    };
    let width = (hi - lo) / bins as f64;
    let mut counts = vec![0.0; bins];
    for v in &data {
        let k = (((v - lo) / width).floor() as usize).min(bins - 1);
        counts[k] += 1.0;
    }
    let edges = (0..=bins).map(|k| lo + width * k as f64).collect();
    Some(Layer::new(Geometry::Histogram { edges, counts }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histogram_counts_everything_once() {
        let data = [1.0, 2.0, 2.0, 3.0, 3.0, 3.0, f64::NAN];
        let Some(Layer {
            geometry: Geometry::Histogram { edges, counts },
            ..
        }) = histogram(&data, 3)
        else {
            panic!("no histogram");
        };
        assert_eq!(edges.len(), 4);
        assert_eq!(counts, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn empty_and_constant_data() {
        assert!(histogram(&[], 5).is_none());
        assert!(histogram(&[f64::NAN], 5).is_none());
        assert!(histogram(&[2.0, 2.0], 4).is_some());
    }
}
