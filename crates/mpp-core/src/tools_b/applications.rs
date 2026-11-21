//! Applications of the definite integral: area between curves (finding where they cross),
//! volumes of revolution by disks, washers and shells about any horizontal or vertical
//! line, arc length, surface area of revolution, average value, work, centres of mass and
//! the accumulation function `F(x) = ∫_a^x f(t) dt`.
//!
//! Curves are functions of one variable. When that variable is `y` the curves are
//! `x = f(y)` and the roles of the axes swap. Answers are exact when the integrals are,
//! otherwise numerical (approximate numbers); the steps say which integral was set up.

use std::fmt;

use crate::calc::diff::diff;
use crate::calc::integrate::integrate_definite;
use crate::domains::solve::{solve, Solutions};
use crate::domains::Ctx;
use crate::error::{MathError, MathResult};
use crate::expr::Expr;
use crate::simplify::{add, mul, pow, simplify};
use crate::steps::Steps;

use super::util::{
    at, fresh, integral_holder, integrate_abs, pythagorean_tidy, sub, value, zeros_inside,
};

/// A line of revolution: `y = c` (horizontal) or `x = c` (vertical).
#[derive(Clone, Debug, PartialEq)]
pub enum Axis {
    /// The line `y = c`.
    Horizontal(Expr),
    /// The line `x = c`.
    Vertical(Expr),
}

impl Axis {
    /// The x-axis.
    pub fn x_axis() -> Axis {
        Axis::Horizontal(Expr::zero())
    }

    /// The constant `c`.
    pub fn offset(&self) -> &Expr {
        match self {
            Axis::Horizontal(c) | Axis::Vertical(c) => c,
        }
    }

    /// True when the axis runs along the direction of `var` (so slices across it are
    /// disks or washers); false when it is perpendicular (cylindrical shells).
    fn parallel_to(&self, var: &str) -> bool {
        let var_is_y = var == "y";
        matches!(
            (self, var_is_y),
            (Axis::Horizontal(_), false) | (Axis::Vertical(_), true)
        )
    }
}

impl fmt::Display for Axis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Axis::Horizontal(c) => write!(f, "y = {c}"),
            Axis::Vertical(c) => write!(f, "x = {c}"),
        }
    }
}

/// How a volume of revolution is sliced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolumeMethod {
    /// Disks (one curve and the axis).
    Disk,
    /// Washers (two curves, a hole in the middle).
    Washer,
    /// Cylindrical shells.
    Shell,
}

impl VolumeMethod {
    /// The method's name.
    pub fn name(self) -> &'static str {
        match self {
            VolumeMethod::Disk => "disk method",
            VolumeMethod::Washer => "washer method",
            VolumeMethod::Shell => "shell method",
        }
    }
}

fn record(steps: &mut Option<&mut Steps>, rule: &str, before: &Expr, after: &Expr, note: String) {
    Steps::record(steps, rule, before, after, Some(note));
}

fn check_bounds(a: &Expr, b: &Expr) -> MathResult<(f64, f64)> {
    match (value(a), value(b)) {
        (Some(av), Some(bv)) if av < bv => Ok((av, bv)),
        (Some(_), Some(_)) => Err(MathError::Domain {
            function: "an application of the integral".into(),
            detail: format!("the interval [{a}, {b}] must have a < b"),
        }),
        _ => Err(MathError::Unsupported(
            "an interval whose ends are not real numbers".into(),
        )),
    }
}

/// The area between `f` and `g` (the x-axis when `g` is `None`).
///
/// With an interval the area is `∫_a^b |f − g|`, split where the curves cross. Without one
/// the region is the one the curves enclose: from their first crossing to their last.
pub fn area_between(
    f: &Expr,
    g: Option<&Expr>,
    var: &str,
    bounds: Option<(&Expr, &Expr)>,
    mut steps: Option<&mut Steps>,
) -> MathResult<Expr> {
    let against_axis = g.is_none();
    let g = g.cloned().unwrap_or_else(Expr::zero);
    let h = sub(f, &g);
    let what = if against_axis {
        "the curve meets the axis"
    } else {
        "the curves cross"
    };
    let (a, b) = match bounds {
        Some((a, b)) => (a.clone(), b.clone()),
        None => {
            let crossings = all_crossings(&h, var)?;
            let (Some(first), Some(last)) = (crossings.first(), crossings.last()) else {
                return Err(MathError::Domain {
                    function: "area".into(),
                    detail: format!("{what} nowhere, so give an interval"),
                });
            };
            if crossings.len() < 2 {
                return Err(MathError::Domain {
                    function: "area".into(),
                    detail: format!("{what} only once (at {var} = {first}), so give an interval"),
                });
            }
            let listed: Vec<String> = crossings.iter().map(|c| c.to_string()).collect();
            record(
                &mut steps,
                "find where the curves cross",
                &Expr::equation(f.clone(), g.clone()),
                &Expr::new(crate::expr::ExprNode::Set(crossings.clone())),
                format!("{var} = {}", listed.join(", ")),
            );
            (first.clone(), last.clone())
        }
    };
    check_bounds(&a, &b)?;
    let (area, cuts) = integrate_abs(&h, var, &a, &b, None)?;
    let holder = integral_holder(
        &crate::functions::func(crate::functions::Func::Abs, vec![h.clone()]),
        var,
        &a,
        &b,
    );
    let note = if cuts.is_empty() {
        format!("area = ∫ |f − g| d{var} with f − g = {h}, which keeps one sign on the interval")
    } else {
        let listed: Vec<String> = cuts.iter().map(|c| c.to_string()).collect();
        format!(
            "the curves cross at {var} = {}, so the integral of |f − g| is split there and each piece is taken with the sign f − g has on it",
            listed.join(", ")
        )
    };
    record(&mut steps, "area between curves", &holder, &area, note);
    Ok(area)
}

/// Real solutions of `h = 0` (exact, or numeric on a wide window), sorted.
fn all_crossings(h: &Expr, var: &str) -> MathResult<Vec<Expr>> {
    let mut ctx = Ctx::new();
    let mut out: Vec<(Expr, f64)> = Vec::new();
    match solve(h, var, None, &mut ctx) {
        Ok(Solutions::Finite { values, .. }) => {
            for v in values {
                if let Some(x) = value(&v) {
                    if crate::domains::util::numeric(&v).is_some() && is_real_value(&v) {
                        out.push((v, x));
                    }
                }
            }
        }
        _ => {
            let window = (Expr::int(-100), Expr::int(100));
            out = zeros_inside(h, var, &window.0, &window.1);
        }
    }
    out.sort_by(|p, q| p.1.total_cmp(&q.1));
    out.dedup_by(|p, q| (p.1 - q.1).abs() < 1e-12);
    Ok(out.into_iter().map(|(e, _)| e).collect())
}

fn is_real_value(e: &Expr) -> bool {
    e.eval_complex(&crate::eval::EmptyEnv)
        .is_ok_and(|z| z.im.abs() <= 1e-12 * (1.0 + z.re.abs()))
}

/// The volume of the solid made by revolving the region between `f` and `g` (the axis
/// line itself when `g` is `None`) over `[a, b]` about `axis`.
///
/// An axis along the direction of `var` gives disks or washers,
/// `π ∫ |(f − c)² − (g − c)²| d(var)`; a perpendicular axis gives shells,
/// `2π ∫ |var − c| · |f − g| d(var)`. The axis must not pass through the region. A
/// requested method that needs the curves solved for the other variable is replaced by
/// the method that gives the same volume, and the steps say so.
#[allow(clippy::too_many_arguments)]
pub fn volume(
    f: &Expr,
    g: Option<&Expr>,
    var: &str,
    a: &Expr,
    b: &Expr,
    axis: &Axis,
    method: Option<VolumeMethod>,
    mut steps: Option<&mut Steps>,
) -> MathResult<Expr> {
    let (av, bv) = check_bounds(a, b)?;
    let c = axis.offset().clone();
    let cv = value(&c).ok_or_else(|| {
        MathError::Unsupported("an axis of revolution that is not a fixed number".into())
    })?;
    let parallel = axis.parallel_to(var);
    let g_given = g.is_some();
    let g = g
        .cloned()
        .unwrap_or_else(|| if parallel { c.clone() } else { Expr::zero() });
    let natural = if !parallel {
        VolumeMethod::Shell
    } else if sub(&g, &c).is_zero() {
        VolumeMethod::Disk
    } else {
        VolumeMethod::Washer
    };
    let x = Expr::sym(var);
    let (integrand, factor, holder_note) = if parallel {
        // Each slice is a washer with radii |f − c| and |g − c|.
        let fr = sub(f, &c);
        let gr = sub(&g, &c);
        if crosses_axis(&fr, &gr, var, av, bv) {
            return Err(MathError::Domain {
                function: "volume".into(),
                detail: format!("the axis {axis} passes through the region, so the solid is not a solid of revolution of one region; split the region at the axis"),
            });
        }
        let h = sub(
            &pow(fr.clone(), Expr::int(2)),
            &pow(gr.clone(), Expr::int(2)),
        );
        let note = if natural == VolumeMethod::Disk {
            format!("disks of radius R = |{fr}|: V = π ∫ R² d{var}")
        } else {
            format!("washers with radii |{fr}| and |{gr}|: V = π ∫ (R² − r²) d{var}")
        };
        (h, Expr::pi(), note)
    } else {
        // Each shell has radius |var − c| and height |f − g|.
        if cv > av && cv < bv {
            return Err(MathError::Domain {
                function: "volume".into(),
                detail: format!(
                    "the axis {axis} passes through the region; split the region at {var} = {c}"
                ),
            });
        }
        let radius = sub(&x, &c);
        let height = sub(f, &g);
        if zeros_inside(&height, var, a, b).is_empty() {
            let sign_r = if at(&radius, var, 0.5 * (av + bv)).unwrap_or(1.0) < 0.0 {
                -1
            } else {
                1
            };
            let sign_h = if at(&height, var, 0.5 * (av + bv)).unwrap_or(1.0) < 0.0 {
                -1
            } else {
                1
            };
            let h = simplify(&mul(vec![
                Expr::int(sign_r * sign_h),
                radius.clone(),
                height.clone(),
            ]));
            let note = format!(
                "cylindrical shells of radius |{radius}| and height |{height}|: V = 2π ∫ r·h d{var}"
            );
            (h, mul(vec![Expr::int(2), Expr::pi()]), note)
        } else {
            // The curves cross: integrate |r·h| piecewise.
            let h = mul(vec![radius.clone(), height.clone()]);
            let note = format!(
                "cylindrical shells of radius |{radius}| and height |{height}| (the curves cross, so the height changes sign): V = 2π ∫ |r·h| d{var}"
            );
            (simplify(&h), mul(vec![Expr::int(2), Expr::pi()]), note)
        }
    };
    let (inner, _) = integrate_abs(&integrand, var, a, b, None)?;
    let v = simplify(&mul(vec![factor, inner]));
    let used = natural;
    if let Some(m) = method {
        let compatible = match m {
            VolumeMethod::Shell => used == VolumeMethod::Shell,
            VolumeMethod::Disk | VolumeMethod::Washer => used != VolumeMethod::Shell,
        };
        if !compatible {
            let other = if var == "y" { "x" } else { "y" };
            Steps::record(
                &mut steps,
                "choose the slicing",
                &Expr::zero(),
                &Expr::zero(),
                Some(format!(
                    "the {} about {axis} needs the curves written as functions of {other}; the {} gives the same volume, so it is used instead",
                    m.name(),
                    used.name()
                )),
            );
        }
    }
    let _ = g_given;
    let holder = integral_holder(&integrand, var, a, b);
    record(
        &mut steps,
        used.name(),
        &holder,
        &v,
        format!("revolve about {axis} using {holder_note}"),
    );
    Ok(v)
}

/// True when `fr` and `gr` (signed distances from the axis) have opposite signs somewhere
/// inside the interval, so the axis cuts the region.
fn crosses_axis(fr: &Expr, gr: &Expr, var: &str, a: f64, b: f64) -> bool {
    let n = 400;
    (1..n).any(|i| {
        let x = a + (b - a) * i as f64 / n as f64;
        matches!((at(fr, var, x), at(gr, var, x)), (Some(p), Some(q)) if p * q < -1e-12)
    })
}

/// The length of the curve `y = f(var)` over `[a, b]`: `∫ √(1 + f′²)`.
pub fn arc_length(
    f: &Expr,
    var: &str,
    a: &Expr,
    b: &Expr,
    mut steps: Option<&mut Steps>,
) -> MathResult<Expr> {
    check_bounds(a, b)?;
    let df = diff(f, var, 1, None)?;
    let integrand = simplify(&pow(
        add(vec![Expr::one(), pow(df.clone(), Expr::int(2))]),
        Expr::rational(1, 2),
    ));
    let v = integrate_definite(&integrand, var, a, b, None)?;
    record(
        &mut steps,
        "arc length",
        &integral_holder(&integrand, var, a, b),
        &v,
        format!("L = ∫ √(1 + (f′)²) d{var} with f′ = {df}"),
    );
    Ok(v)
}

/// The length of the parametric curve `(x(t), y(t))` for `t ∈ [a, b]`: `∫ √(x′² + y′²)`.
pub fn arc_length_parametric(
    xt: &Expr,
    yt: &Expr,
    t: &str,
    a: &Expr,
    b: &Expr,
    mut steps: Option<&mut Steps>,
) -> MathResult<Expr> {
    check_bounds(a, b)?;
    let dx = diff(xt, t, 1, None)?;
    let dy = diff(yt, t, 1, None)?;
    let integrand = simplify(&pow(
        pythagorean_tidy(&add(vec![
            pow(dx.clone(), Expr::int(2)),
            pow(dy.clone(), Expr::int(2)),
        ])),
        Expr::rational(1, 2),
    ));
    let v = integrate_definite(&integrand, t, a, b, None)?;
    record(
        &mut steps,
        "arc length of a parametric curve",
        &integral_holder(&integrand, t, a, b),
        &v,
        format!("L = ∫ √(x′² + y′²) d{t} with x′ = {dx}, y′ = {dy}"),
    );
    Ok(v)
}

/// The area of the surface made by revolving `y = f(var)` over `[a, b]` about `axis`:
/// `2π ∫ r √(1 + f′²)` with `r` the distance to the axis.
pub fn surface_area(
    f: &Expr,
    var: &str,
    a: &Expr,
    b: &Expr,
    axis: &Axis,
    mut steps: Option<&mut Steps>,
) -> MathResult<Expr> {
    let (av, bv) = check_bounds(a, b)?;
    let c = axis.offset().clone();
    let df = diff(f, var, 1, None)?;
    let ds = pow(
        add(vec![Expr::one(), pow(df.clone(), Expr::int(2))]),
        Expr::rational(1, 2),
    );
    let radius = if axis.parallel_to(var) {
        sub(f, &c)
    } else {
        let cv = value(&c)
            .ok_or_else(|| MathError::Unsupported("an axis that is not a fixed number".into()))?;
        if cv > av && cv < bv {
            return Err(MathError::Domain {
                function: "surface area".into(),
                detail: format!("the axis {axis} passes through the curve's interval"),
            });
        }
        sub(&Expr::sym(var), &c)
    };
    let integrand = simplify(&mul(vec![radius.clone(), ds]));
    let (inner, _) = integrate_abs(&integrand, var, a, b, None)?;
    let v = simplify(&mul(vec![Expr::int(2), Expr::pi(), inner]));
    record(
        &mut steps,
        "surface area of revolution",
        &integral_holder(&integrand, var, a, b),
        &v,
        format!(
            "S = 2π ∫ r √(1 + (f′)²) d{var} with r = |{radius}| (distance to {axis}) and f′ = {df}"
        ),
    );
    Ok(v)
}

/// The average value `1/(b − a) ∫_a^b f`.
pub fn average_value(
    f: &Expr,
    var: &str,
    a: &Expr,
    b: &Expr,
    mut steps: Option<&mut Steps>,
) -> MathResult<Expr> {
    check_bounds(a, b)?;
    let integral = integrate_definite(f, var, a, b, None)?;
    let v = simplify(&mul(vec![integral.clone(), pow(sub(b, a), Expr::int(-1))]));
    record(
        &mut steps,
        "average value",
        &integral_holder(f, var, a, b),
        &v,
        format!(
            "f_avg = (1/(b − a)) ∫ f d{var} = ({integral})/({})",
            sub(b, a)
        ),
    );
    Ok(v)
}

/// The work done by a variable force `F(var)` moving from `a` to `b`: `W = ∫_a^b F`.
pub fn work(
    force: &Expr,
    var: &str,
    a: &Expr,
    b: &Expr,
    mut steps: Option<&mut Steps>,
) -> MathResult<Expr> {
    let v = integrate_definite(force, var, a, b, None)?;
    record(
        &mut steps,
        "work",
        &integral_holder(force, var, a, b),
        &v,
        format!("W = ∫ F d{var} with F = {force}"),
    );
    Ok(v)
}

/// The centroid `(x̄, ȳ)` of the region between `f` (top) and `g` (bottom, the axis when
/// `None`) over `[a, b]`, for a uniform density. With `var = y` the curves are `x = f(y)`
/// and the coordinates are still returned as `(x̄, ȳ)`.
pub fn centroid(
    f: &Expr,
    g: Option<&Expr>,
    var: &str,
    a: &Expr,
    b: &Expr,
    mut steps: Option<&mut Steps>,
) -> MathResult<(Expr, Expr)> {
    let (av, bv) = check_bounds(a, b)?;
    let g = g.cloned().unwrap_or_else(Expr::zero);
    let h = sub(f, &g);
    let n = 200;
    let below = (0..=n)
        .any(|i| at(&h, var, av + (bv - av) * i as f64 / n as f64).is_some_and(|v| v < -1e-12));
    if below {
        return Err(MathError::Domain {
            function: "centroid".into(),
            detail: format!(
                "the first curve {f} must lie above the second {g} on the interval; swap them or split the region where they cross"
            ),
        });
    }
    let area = integrate_definite(&h, var, a, b, None)?;
    if value(&area).is_some_and(|v| v.abs() < 1e-300) {
        return Err(MathError::DivisionByZero);
    }
    let x = Expr::sym(var);
    let m_along = integrate_definite(&simplify(&mul(vec![x, h.clone()])), var, a, b, None)?;
    let half_sq = simplify(&mul(vec![
        Expr::rational(1, 2),
        sub(&pow(f.clone(), Expr::int(2)), &pow(g.clone(), Expr::int(2))),
    ]));
    let m_across = integrate_definite(&half_sq, var, a, b, None)?;
    let along = simplify(&mul(vec![
        m_along.clone(),
        pow(area.clone(), Expr::int(-1)),
    ]));
    let across = simplify(&mul(vec![
        m_across.clone(),
        pow(area.clone(), Expr::int(-1)),
    ]));
    let point = if var == "y" {
        (across.clone(), along.clone())
    } else {
        (along.clone(), across.clone())
    };
    record(
        &mut steps,
        "centroid of a region",
        &integral_holder(&h, var, a, b),
        &Expr::new(crate::expr::ExprNode::Tuple(vec![point.0.clone(), point.1.clone()])),
        format!(
            "A = ∫ (f − g) d{var} = {area}; moment along {var}: ∫ {var}(f − g) d{var} = {m_along}; moment across: ∫ (f² − g²)/2 d{var} = {m_across}"
        ),
    );
    Ok(point)
}

/// The centre of mass `x̄ = ∫ x ρ / ∫ ρ` of a rod with density `ρ(var)` on `[a, b]`.
pub fn centre_of_mass_rod(
    density: &Expr,
    var: &str,
    a: &Expr,
    b: &Expr,
    mut steps: Option<&mut Steps>,
) -> MathResult<Expr> {
    check_bounds(a, b)?;
    let mass = integrate_definite(density, var, a, b, None)?;
    if mass.is_zero() {
        return Err(MathError::DivisionByZero);
    }
    let moment = integrate_definite(
        &simplify(&mul(vec![Expr::sym(var), density.clone()])),
        var,
        a,
        b,
        None,
    )?;
    let v = simplify(&mul(vec![moment.clone(), pow(mass.clone(), Expr::int(-1))]));
    record(
        &mut steps,
        "centre of mass",
        &integral_holder(density, var, a, b),
        &v,
        format!("mass m = ∫ ρ d{var} = {mass}; moment M = ∫ {var} ρ d{var} = {moment}; x̄ = M/m"),
    );
    Ok(v)
}

/// The accumulation function `F(var) = ∫_a^var f(t) dt`, with `F′ = f` by the
/// fundamental theorem of calculus.
pub fn accumulation(
    f: &Expr,
    var: &str,
    a: &Expr,
    mut steps: Option<&mut Steps>,
) -> MathResult<Expr> {
    let t = fresh(f, &["t", "s", "u"]);
    let ft = f.subs(var, &Expr::sym(&t));
    let big_f = integrate_definite(&ft, &t, a, &Expr::sym(var), None)?;
    record(
        &mut steps,
        "accumulation function",
        &integral_holder(&ft, &t, a, &Expr::sym(var)),
        &big_f,
        format!("F({var}) = ∫_{a}^{var} f({t}) d{t}; by the fundamental theorem of calculus F′({var}) = {f}"),
    );
    Ok(big_f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functions::{func, Func};

    fn x() -> Expr {
        Expr::sym("x")
    }
    fn n(k: i64) -> Expr {
        Expr::int(k)
    }
    fn approx(e: &Expr, v: f64) {
        let got = value(e).unwrap_or(f64::NAN);
        assert!(
            (got - v).abs() < 1e-8 * (1.0 + v.abs()),
            "{e} ≈ {got}, expected {v}"
        );
    }

    #[test]
    fn areas_between_curves() {
        // y = x and y = x² enclose 1/6, crossing at 0 and 1.
        assert_eq!(
            area_between(&x(), Some(&x().pow(2)), "x", None, None).unwrap(),
            Expr::rational(1, 6)
        );
        // Under sin on [0, 2π]: the area counts both humps.
        let s = func(Func::Sin, vec![x()]);
        let two_pi = n(2) * Expr::pi();
        assert_eq!(
            area_between(&s, None, "x", Some((&n(0), &two_pi)), None).unwrap(),
            n(4)
        );
        // y = x³ and y = x on [−1, 1]: 1/2.
        let v = area_between(&x().pow(3), Some(&x()), "x", Some((&n(-1), &n(1))), None).unwrap();
        assert_eq!(v, Expr::rational(1, 2));
        // Curves given as functions of y: x = y² and x = y + 2 enclose 9/2.
        let y = Expr::sym("y");
        let v = area_between(&(y.clone() + n(2)), Some(&y.pow(2)), "y", None, None).unwrap();
        assert_eq!(v, Expr::rational(9, 2));
    }

    #[test]
    fn volumes_of_revolution() {
        let pi = Expr::pi();
        // Disk: y = √x on [0, 4] about the x-axis: 8π.
        let v = volume(
            &x().sqrt(),
            None,
            "x",
            &n(0),
            &n(4),
            &Axis::x_axis(),
            None,
            None,
        )
        .unwrap();
        assert_eq!(v, n(8) * pi.clone());
        // Washer: between y = x and y = x² about the x-axis: 2π/15.
        let v = volume(
            &x(),
            Some(&x().pow(2)),
            "x",
            &n(0),
            &n(1),
            &Axis::x_axis(),
            None,
            None,
        )
        .unwrap();
        assert_eq!(v, Expr::rational(2, 15) * pi.clone());
        // Shell: the same region about the y-axis: π/6.
        let v = volume(
            &x(),
            Some(&x().pow(2)),
            "x",
            &n(0),
            &n(1),
            &Axis::Vertical(n(0)),
            None,
            None,
        )
        .unwrap();
        assert_eq!(v, Expr::rational(1, 6) * pi.clone());
        // About y = −1 (washer): π ∫ ((x+1)² − (x²+1)²) = 7π/15.
        let v = volume(
            &x(),
            Some(&x().pow(2)),
            "x",
            &n(0),
            &n(1),
            &Axis::Horizontal(n(-1)),
            None,
            None,
        )
        .unwrap();
        assert_eq!(v, Expr::rational(7, 15) * pi.clone());
        // About x = 2 (shells): 2π ∫ (2 − x)(x − x²) = π/2.
        let v = volume(
            &x(),
            Some(&x().pow(2)),
            "x",
            &n(0),
            &n(1),
            &Axis::Vertical(n(2)),
            None,
            None,
        )
        .unwrap();
        assert_eq!(v, Expr::rational(1, 2) * pi.clone());
        // Sphere of radius 2 from the half circle: 32π/3.
        let semi = (n(4) - x().pow(2)).sqrt();
        let v = volume(
            &semi,
            None,
            "x",
            &n(-2),
            &n(2),
            &Axis::x_axis(),
            Some(VolumeMethod::Disk),
            None,
        )
        .unwrap();
        assert_eq!(v, Expr::rational(32, 3) * pi);
        // An axis through the region is refused.
        assert!(volume(
            &x(),
            None,
            "x",
            &n(0),
            &n(2),
            &Axis::Vertical(n(1)),
            None,
            None
        )
        .is_err());
    }

    #[test]
    fn lengths_surfaces_and_averages() {
        // y = x^(3/2) on [0, 4]: (80√10 − 8)/27.
        let v = arc_length(&x().pow(Expr::rational(3, 2)), "x", &n(0), &n(4), None).unwrap();
        approx(&v, (80.0 * 10f64.sqrt() - 8.0) / 27.0);
        // A circle of radius 1 as (cos t, sin t): 2π.
        let t = Expr::sym("t");
        let c = func(Func::Cos, vec![t.clone()]);
        let s = func(Func::Sin, vec![t]);
        let v = arc_length_parametric(&c, &s, "t", &n(0), &(n(2) * Expr::pi()), None).unwrap();
        assert_eq!(v, n(2) * Expr::pi());
        // Sphere surface from the half circle of radius 1: 4π.
        let semi = (n(1) - x().pow(2)).sqrt();
        let v = surface_area(&semi, "x", &n(-1), &n(1), &Axis::x_axis(), None).unwrap();
        approx(&v, 4.0 * std::f64::consts::PI);
        // Cone y = x on [0, 1] about the x-axis: π√2.
        let v = surface_area(&x(), "x", &n(0), &n(1), &Axis::x_axis(), None).unwrap();
        assert_eq!(v, Expr::int(2).sqrt() * Expr::pi());
        assert_eq!(
            average_value(&x().pow(2), "x", &n(0), &n(3), None).unwrap(),
            n(3)
        );
        // Hooke's law F = 5x from 0 to 0.2: 1/10.
        assert_eq!(
            work(&(n(5) * x()), "x", &n(0), &Expr::rational(1, 5), None).unwrap(),
            Expr::rational(1, 10)
        );
    }

    #[test]
    fn centres_of_mass_and_accumulation() {
        // Under y = x² on [0, 1]: (3/4, 3/10).
        let (cx, cy) = centroid(&x().pow(2), None, "x", &n(0), &n(1), None).unwrap();
        assert_eq!((cx, cy), (Expr::rational(3, 4), Expr::rational(3, 10)));
        // Between y = x and y = x²: (1/2, 2/5).
        let (cx, cy) = centroid(&x(), Some(&x().pow(2)), "x", &n(0), &n(1), None).unwrap();
        assert_eq!((cx, cy), (Expr::rational(1, 2), Expr::rational(2, 5)));
        assert!(centroid(&x().pow(2), Some(&x()), "x", &n(0), &n(1), None).is_err());
        // Rod with density 1 + x on [0, 2]: 10/6 / 4 → x̄ = 7/6.
        let v = centre_of_mass_rod(&(n(1) + x()), "x", &n(0), &n(2), None).unwrap();
        assert_eq!(v, Expr::rational(7, 6));
        let big_f = accumulation(&(n(2) * x()), "x", &n(1), None).unwrap();
        assert_eq!(big_f, x().pow(2) - n(1));
    }
}
