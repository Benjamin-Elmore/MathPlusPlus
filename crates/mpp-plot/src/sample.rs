//! Adaptive sampling of curves `t -> (x, y)`: refinement where the curve bends, and
//! breaks (not vertical joins) at asymptotes, jumps and the edges of the domain.

/// A point in the plane.
pub type P = [f64; 2];

/// Limits on the work a sampler does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleOpts {
    /// Evenly spaced samples before refinement.
    pub initial: usize,
    /// How many times an interval may be halved.
    pub max_depth: u32,
    /// Most function evaluations in total.
    pub max_evals: usize,
}

impl Default for SampleOpts {
    fn default() -> Self {
        SampleOpts {
            initial: 480,
            max_depth: 9,
            max_evals: 60_000,
        }
    }
}

/// A jump discontinuity of an explicit curve, where open and closed dots go.
#[derive(Clone, Debug, PartialEq)]
pub struct Jump {
    /// The x position of the jump (the simplest decimal inside the final bracket).
    pub x: f64,
    /// The value the curve approaches from the left.
    pub left: f64,
    /// The value the curve approaches from the right.
    pub right: f64,
    /// The curve takes the left value at `x`.
    pub left_closed: bool,
    /// The curve takes the right value at `x`.
    pub right_closed: bool,
    /// The curve takes some third value at `x` (a removable or isolated point).
    pub isolated: Option<f64>,
}

/// The result of sampling.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sampled {
    /// Connected runs of points; the curve is not drawn between runs.
    pub segments: Vec<Vec<P>>,
    /// Jump discontinuities found.
    pub jumps: Vec<Jump>,
    /// Parameter values (x for explicit curves) of vertical asymptotes found.
    pub asymptotes: Vec<f64>,
}

impl Sampled {
    /// Every sampled point.
    pub fn points(&self) -> impl Iterator<Item = &P> {
        self.segments.iter().flatten()
    }
}

/// How far (as a fraction of the plot scale) a refined interval may still jump
/// before it counts as a break.
const JUMP_FRAC: f64 = 0.05;
/// How far a midpoint may sit off the chord, as a fraction of the scale.
const BEND_TOL: f64 = 0.000_5;

#[derive(Clone, Copy, Debug)]
enum Ev {
    Pt(P),
    Gap,
}

type Sample = (f64, Option<P>);

struct Sampler<'a> {
    f: &'a mut dyn FnMut(f64) -> Option<P>,
    scale: P,
    opts: SampleOpts,
    events: Vec<Ev>,
    evals: usize,
    jumps: Vec<Jump>,
    asymptotes: Vec<f64>,
}

impl Sampler<'_> {
    fn eval(&mut self, t: f64) -> Option<P> {
        self.evals += 1;
        (self.f)(t).filter(|p| p.iter().all(|v| v.is_finite() && v.abs() < 1e30))
    }

    fn budget(&self) -> bool {
        self.evals < self.opts.max_evals
    }

    /// Distance between two points in units of the plot scale.
    fn dist(&self, a: P, b: P) -> f64 {
        ((a[0] - b[0]).abs() / self.scale[0]).max((a[1] - b[1]).abs() / self.scale[1])
    }

    /// How far `m` is from the chord `a`-`b`, in units of the scale.
    fn bend(&self, a: P, m: P, b: P) -> f64 {
        let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
        self.dist(m, mid)
    }

    /// Finds, by bisection, where the function stops being defined between
    /// `good` (defined) and `bad` (undefined).
    fn edge(&mut self, good: Sample, bad: f64) -> (f64, P) {
        let (mut g, mut b) = (good.0, bad);
        let mut gp = good.1.unwrap_or([0.0, 0.0]);
        for _ in 0..28 {
            let m = (g + b) / 2.0;
            if m == g || m == b {
                break;
            }
            match self.eval(m) {
                Some(p) => {
                    g = m;
                    gp = p;
                }
                None => b = m,
            }
        }
        (g, gp)
    }

    fn go(&mut self, a: Sample, b: Sample, depth: u32, anc: [f64; 3]) {
        match (a.1, b.1) {
            (None, None) => {
                // A short defined island can hide between two undefined samples.
                if depth < 2 && self.budget() {
                    let tm = (a.0 + b.0) / 2.0;
                    let fm = self.eval(tm);
                    if fm.is_some() {
                        let m = (tm, fm);
                        self.go(a, m, depth + 1, anc);
                        self.go(m, b, depth + 1, anc);
                    }
                }
            }
            (Some(_), None) => {
                let (t, p) = self.edge(a, b.0);
                if t != a.0 {
                    self.events.push(Ev::Pt(p));
                }
                self.events.push(Ev::Gap);
            }
            (None, Some(pb)) => {
                let (t, p) = self.edge((b.0, b.1), a.0);
                self.events.push(Ev::Gap);
                if t != b.0 {
                    self.events.push(Ev::Pt(p));
                }
                self.events.push(Ev::Pt(pb));
            }
            (Some(pa), Some(pb)) => {
                let d = self.dist(pa, pb);
                if depth < self.opts.max_depth && self.budget() {
                    let tm = (a.0 + b.0) / 2.0;
                    let fm = self.eval(tm);
                    let refine = match fm {
                        None => true,
                        Some(pm) => self.bend(pa, pm, pb) > BEND_TOL || d > JUMP_FRAC,
                    };
                    if refine {
                        let anc = [d, anc[0], anc[1]];
                        let m = (tm, fm);
                        self.go(a, m, depth + 1, anc);
                        self.go(m, b, depth + 1, anc);
                        return;
                    }
                }
                if d > JUMP_FRAC {
                    self.classify(a, b, pa, pb, d, anc[2]);
                } else {
                    self.events.push(Ev::Pt(pb));
                }
            }
        }
    }

    /// A refined interval that still jumps: a steep but continuous curve, a jump, or an
    /// asymptote.
    fn classify(&mut self, a: Sample, b: Sample, pa: P, pb: P, d: f64, ancestor: f64) {
        let ratio = if ancestor > 0.0 { d / ancestor } else { 1.0 };
        if ratio < 0.85 {
            // Still shrinking as the interval does: steep, not broken.
            self.events.push(Ev::Pt(pb));
            return;
        }
        let big = 8.0;
        let bounded = pa[1].abs() < big * self.scale[1] && pb[1].abs() < big * self.scale[1];
        let growing = ratio > 1.3;
        if bounded && !growing {
            let x = simplest_between(a.0, b.0);
            let at = self.eval(x);
            let jump = (pb[1] - pa[1]).abs();
            let same = |p: P, q: P| (p[1] - q[1]).abs() < 0.1 * jump;
            let (left_closed, right_closed, isolated) = match at {
                Some(p) if same(p, pa) => (true, false, None),
                Some(p) if same(p, pb) => (false, true, None),
                Some(p) => (false, false, Some(p[1])),
                None => (false, false, None),
            };
            self.jumps.push(Jump {
                x,
                left: pa[1],
                right: pb[1],
                left_closed,
                right_closed,
                isolated,
            });
        } else {
            self.asymptotes.push((a.0 + b.0) / 2.0);
        }
        self.events.push(Ev::Gap);
        self.events.push(Ev::Pt(pb));
    }
}

/// The number with the fewest decimal digits in `[lo, hi]` (the break of a piecewise
/// function is nearly always a short decimal).
pub fn simplest_between(lo: f64, hi: f64) -> f64 {
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    for digits in 0..=14 {
        let scale = 10f64.powi(digits);
        let v = (lo * scale).ceil() / scale;
        if v <= hi {
            return v;
        }
    }
    (lo + hi) / 2.0
}

/// The spread of the middle 90% of `values` (or the full spread when that is zero).
fn robust_spread(values: &mut Vec<f64>) -> f64 {
    values.retain(|v| v.is_finite());
    if values.len() < 2 {
        return 1.0;
    }
    values.sort_by(f64::total_cmp);
    let q = |f: f64| values[((values.len() - 1) as f64 * f).round() as usize];
    let mut s = q(0.95) - q(0.05);
    if s <= 0.0 {
        s = q(1.0) - q(0.0);
    }
    if s > 0.0 && s.is_finite() {
        s
    } else {
        1.0
    }
}

/// Samples `f` over `[t0, t1]`.
pub fn sample_curve(
    f: &mut dyn FnMut(f64) -> Option<P>,
    t0: f64,
    t1: f64,
    opts: SampleOpts,
) -> Sampled {
    if !(t0.is_finite() && t1.is_finite()) || t1 <= t0 {
        return Sampled::default();
    }
    let n = opts.initial.clamp(2, 20_000);
    let mut sampler = Sampler {
        f,
        scale: [1.0, 1.0],
        opts,
        events: Vec::new(),
        evals: 0,
        jumps: Vec::new(),
        asymptotes: Vec::new(),
    };
    let grid: Vec<Sample> = (0..=n)
        .map(|i| {
            let t = if i == n {
                t1
            } else {
                t0 + (t1 - t0) * i as f64 / n as f64
            };
            (t, sampler.eval(t))
        })
        .collect();
    let mut xs: Vec<f64> = grid.iter().filter_map(|s| s.1.map(|p| p[0])).collect();
    let mut ys: Vec<f64> = grid.iter().filter_map(|s| s.1.map(|p| p[1])).collect();
    if xs.is_empty() {
        // Nothing defined on the grid; look for a short island between the samples.
        for w in grid.windows(2) {
            sampler.go(w[0], w[1], 0, [0.0; 3]);
        }
        return finish(sampler);
    }
    sampler.scale = [robust_spread(&mut xs), robust_spread(&mut ys)];
    if let Some(first) = grid.first().and_then(|s| s.1) {
        sampler.events.push(Ev::Pt(first));
    }
    for w in grid.windows(2) {
        sampler.go(w[0], w[1], 0, [0.0; 3]);
    }
    finish(sampler)
}

fn finish(sampler: Sampler<'_>) -> Sampled {
    let mut out = Sampled {
        jumps: sampler.jumps,
        asymptotes: sampler.asymptotes,
        ..Sampled::default()
    };
    let mut current: Vec<P> = Vec::new();
    let close = |current: &mut Vec<P>, out: &mut Sampled| {
        if current.len() >= 2 {
            out.segments.push(std::mem::take(current));
        } else {
            current.clear();
        }
    };
    for ev in sampler.events {
        match ev {
            Ev::Pt(p) => {
                if current.last() != Some(&p) {
                    current.push(p);
                }
            }
            Ev::Gap => close(&mut current, &mut out),
        }
    }
    close(&mut current, &mut out);
    out
}

/// A y-range that ignores spikes: the full range of the data unless a few points are
/// far outside the bulk, in which case those are cut off.
pub fn robust_range(values: impl Iterator<Item = f64>) -> Option<(f64, f64)> {
    let mut v: Vec<f64> = values.filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    let q = |f: f64| v[((v.len() - 1) as f64 * f).round() as usize];
    let (q1, q3) = (q(0.25), q(0.75));
    let iqr = q3 - q1;
    let (min, max) = (v[0], v[v.len() - 1]);
    if iqr <= 0.0 {
        // Mostly constant data: keep the bulk and any modest excursions.
        let spread = (q(0.95) - q(0.05)).abs().max(1e-12);
        let fence = 6.0 * spread.max(q1.abs() * 0.5);
        return Some((min.max(q1 - fence), max.min(q3 + fence)));
    }
    let lo = min.max(q1 - 3.0 * iqr);
    let hi = max.min(q3 + 3.0 * iqr);
    Some((lo, hi))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn explicit(f: impl Fn(f64) -> f64 + 'static) -> impl FnMut(f64) -> Option<P> {
        move |x| {
            let y = f(x);
            y.is_finite().then_some([x, y])
        }
    }

    fn no_vertical_joins(s: &Sampled, min_jump: f64) -> bool {
        s.segments.iter().all(|seg| {
            seg.windows(2)
                .all(|w| !(w[0][1] * w[1][1] < 0.0 && (w[0][1] - w[1][1]).abs() > min_jump))
        })
    }

    #[test]
    fn tan_breaks_at_every_asymptote() {
        let mut f = explicit(f64::tan);
        let s = sample_curve(&mut f, -10.0, 10.0, SampleOpts::default());
        // Asymptotes at ±π/2, ±3π/2, ±5π/2 leave seven branches.
        assert_eq!(s.segments.len(), 7, "{}", s.segments.len());
        assert!(no_vertical_joins(&s, 10.0));
        assert!(s.jumps.is_empty());
        assert!(s.asymptotes.len() >= 6);
    }

    #[test]
    fn reciprocal_has_no_vertical_line() {
        let mut f = explicit(|x| 1.0 / x);
        let s = sample_curve(&mut f, -5.0, 5.0, SampleOpts::default());
        assert_eq!(s.segments.len(), 2);
        assert!(no_vertical_joins(&s, 10.0));
        // One branch is entirely negative, one entirely positive.
        assert!(s.segments[0].iter().all(|p| p[1] < 0.0));
        assert!(s.segments[1].iter().all(|p| p[1] > 0.0));
    }

    #[test]
    fn reciprocal_with_the_pole_off_the_grid() {
        // 1/(x - 0.0123): no sample lands on the pole.
        let mut f = explicit(|x| 1.0 / (x - 0.0123));
        let s = sample_curve(&mut f, -5.0, 5.0, SampleOpts::default());
        assert_eq!(s.segments.len(), 2);
        assert!(no_vertical_joins(&s, 10.0));
        assert!(!s.asymptotes.is_empty());
    }

    #[test]
    fn sqrt_stops_at_the_edge_of_its_domain() {
        let mut f = explicit(f64::sqrt);
        let s = sample_curve(&mut f, -4.0, 4.0, SampleOpts::default());
        assert_eq!(s.segments.len(), 1);
        let first = s.segments[0][0];
        assert!(first[0] >= 0.0 && first[0] < 1e-6, "{first:?}");
    }

    #[test]
    fn step_function_records_a_jump_with_the_right_dots() {
        // x < 1 -> x, x >= 1 -> x + 2: closed on the right.
        let mut f = explicit(|x| if x < 1.0 { x } else { x + 2.0 });
        let s = sample_curve(&mut f, -3.0, 4.0, SampleOpts::default());
        assert_eq!(s.segments.len(), 2);
        assert_eq!(s.jumps.len(), 1);
        let j = &s.jumps[0];
        assert!((j.x - 1.0).abs() < 1e-9, "{}", j.x);
        assert!((j.left - 1.0).abs() < 1e-3 && (j.right - 3.0).abs() < 1e-3);
        assert!(!j.left_closed && j.right_closed);
        assert!(s.asymptotes.is_empty());
    }

    #[test]
    fn smooth_functions_are_not_broken() {
        for g in [
            (|x: f64| x.sin()) as fn(f64) -> f64,
            |x| x * x * x,
            f64::cbrt,
            |x| (x * 3.0).sin() / (1.0 + x * x),
        ] {
            let mut f = explicit(g);
            let s = sample_curve(&mut f, -6.0, 6.0, SampleOpts::default());
            assert_eq!(s.segments.len(), 1);
            assert!(s.jumps.is_empty());
        }
    }

    #[test]
    fn refinement_resolves_a_sharp_feature() {
        let mut f = explicit(|x| (-x * x * 400.0).exp());
        let s = sample_curve(&mut f, -3.0, 3.0, SampleOpts::default());
        let peak = s.points().map(|p| p[1]).fold(0.0, f64::max);
        assert!(peak > 0.99, "{peak}");
    }

    #[test]
    fn hostile_functions_do_not_hang() {
        let mut f = |x: f64| Some([x, (1.0 / x.sin()).sin()]);
        let s = sample_curve(&mut f, -1.0, 1.0, SampleOpts::default());
        assert!(s.points().count() < 200_000);
        let mut nothing = |_x: f64| None;
        assert!(sample_curve(&mut nothing, 0.0, 1.0, SampleOpts::default())
            .segments
            .is_empty());
        let mut f = explicit(|x| x);
        assert!(sample_curve(&mut f, 1.0, 1.0, SampleOpts::default())
            .segments
            .is_empty());
        assert!(sample_curve(&mut f, f64::NAN, 1.0, SampleOpts::default())
            .segments
            .is_empty());
    }

    #[test]
    fn simplest_decimal() {
        assert_eq!(simplest_between(0.999_9, 1.000_1), 1.0);
        assert_eq!(simplest_between(0.12, 0.18), 0.12);
    }

    #[test]
    fn robust_range_ignores_spikes() {
        let mut v: Vec<f64> = (0..200).map(|i| (i as f64 / 20.0).sin()).collect();
        v.push(1e6);
        v.push(-1e6);
        let (lo, hi) = robust_range(v.into_iter()).unwrap();
        assert!(lo > -10.0 && hi < 10.0, "{lo} {hi}");
        let (lo, hi) = robust_range((0..100).map(|i| (i * i) as f64)).unwrap();
        assert_eq!((lo, hi), (0.0, 9801.0));
    }
}
