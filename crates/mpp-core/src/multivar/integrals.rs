//! Multiple integrals in polar, cylindrical and spherical coordinates (the Jacobian put
//! in automatically), general changes of variables, surface integrals over a
//! parametrisation, and checks of Green's, Stokes' and the divergence theorem with both
//! sides worked out.
//!
//! Bounds are listed outermost first, as they are written: `x ∈ [0, 1], y ∈ [0, x]`.

use std::cmp::Ordering;

use crate::calc::{diff, integrate_multiple, line_integral};
use crate::domains::util;
use crate::error::{MathError, MathResult};
use crate::expr::Expr;
use crate::functions::{func, Func};
use crate::simplify::{mul, pow, simplify};

use super::{agree, cross, dot, subs_all};

/// One integration variable with its bounds (the bounds may use outer variables).
pub type Bound = (String, Expr, Expr);

/// A coordinate system with its map to Cartesian coordinates and its Jacobian.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coordinates {
    /// `x = r cos θ, y = r sin θ`, `dA = r dr dθ`.
    Polar,
    /// `x = r cos θ, y = r sin θ, z = z`, `dV = r dz dr dθ`.
    Cylindrical,
    /// `x = ρ sin φ cos θ, y = ρ sin φ sin θ, z = ρ cos φ`, `dV = ρ² sin φ dρ dφ dθ`.
    Spherical,
}

/// The role of a coordinate name, accepting the spellings people type.
fn role(name: &str) -> Option<&'static str> {
    Some(match name {
        "r" => "r",
        "θ" | "theta" => "θ",
        "φ" | "ϕ" | "phi" => "φ",
        "ρ" | "rho" => "ρ",
        "z" => "z",
        _ => return None,
    })
}

impl Coordinates {
    /// The roles of the coordinates, in the order of [`Coordinates::map`].
    pub fn roles(self) -> &'static [&'static str] {
        match self {
            Coordinates::Polar => &["r", "θ"],
            Coordinates::Cylindrical => &["r", "θ", "z"],
            Coordinates::Spherical => &["ρ", "φ", "θ"],
        }
    }

    /// The Cartesian names this system replaces.
    pub fn cartesian(self) -> &'static [&'static str] {
        match self {
            Coordinates::Polar => &["x", "y"],
            _ => &["x", "y", "z"],
        }
    }

    /// `(x, y[, z])` in terms of the coordinate symbols `names` (in role order) and the
    /// Jacobian factor.
    pub fn map(self, names: &[Expr]) -> (Vec<Expr>, Expr) {
        let cos = |e: &Expr| func(Func::Cos, vec![e.clone()]);
        let sin = |e: &Expr| func(Func::Sin, vec![e.clone()]);
        match (self, names) {
            (Coordinates::Polar, [r, t]) => {
                (vec![r.clone() * cos(t), r.clone() * sin(t)], r.clone())
            }
            (Coordinates::Cylindrical, [r, t, z]) => (
                vec![r.clone() * cos(t), r.clone() * sin(t), z.clone()],
                r.clone(),
            ),
            (Coordinates::Spherical, [rho, phi, t]) => (
                vec![
                    rho.clone() * sin(phi) * cos(t),
                    rho.clone() * sin(phi) * sin(t),
                    rho.clone() * cos(phi),
                ],
                pow(rho.clone(), Expr::int(2)) * sin(phi),
            ),
            _ => (Vec::new(), Expr::one()),
        }
    }
}

/// Integrates `f(x, y[, z])` over a region described in `coords`: substitutes the map,
/// multiplies by the Jacobian and integrates in the order of `bounds` (outermost first).
pub fn integrate_in(f: &Expr, coords: Coordinates, bounds: &[Bound]) -> MathResult<Expr> {
    let roles = coords.roles();
    if bounds.len() != roles.len() {
        return Err(MathError::WrongArity {
            function: "the integral".into(),
            expected: roles.len().to_string(),
            got: bounds.len(),
        });
    }
    // The symbol the user wrote for each role.
    let mut names: Vec<Expr> = Vec::with_capacity(roles.len());
    for want in roles {
        let found = bounds
            .iter()
            .find(|(v, _, _)| role(v) == Some(want))
            .ok_or_else(|| MathError::Undefined(format!("the bounds for {want}")))?;
        names.push(Expr::sym(&found.0));
    }
    let (xyz, jac) = coords.map(&names);
    let cart = coords.cartesian();
    let integrand = simplify(&mul(vec![subs_all(f, cart, &xyz), jac]));
    integrate_bounds(&integrand, bounds)
}

/// `∫…∫ e` over `bounds` written outermost first.
fn integrate_bounds(e: &Expr, bounds: &[Bound]) -> MathResult<Expr> {
    let inner_first: Vec<(&str, Expr, Expr)> = bounds
        .iter()
        .rev()
        .map(|(v, a, b)| (v.as_str(), a.clone(), b.clone()))
        .collect();
    integrate_multiple(e, &inner_first, None)
}

/// The Jacobian determinant `∂(x, y[, z])/∂(u, v[, w])` of a map.
pub fn jacobian_det(map: &[Expr], params: &[&str]) -> MathResult<Expr> {
    let rows = map
        .iter()
        .map(|c| {
            params
                .iter()
                .map(|p| diff(c, p, 1, None))
                .collect::<MathResult<Vec<_>>>()
        })
        .collect::<MathResult<Vec<_>>>()?;
    let rows: Vec<&[Expr]> = rows.iter().map(Vec::as_slice).collect();
    let det = match rows.as_slice() {
        [[a, b], [c, d]] => a.clone() * d.clone() - b.clone() * c.clone(),
        [[a, b, c], [d, e, f], [g, h, i]] => {
            a.clone() * (e.clone() * i.clone() - f.clone() * h.clone())
                - b.clone() * (d.clone() * i.clone() - f.clone() * g.clone())
                + c.clone() * (d.clone() * h.clone() - e.clone() * g.clone())
        }
        _ => {
            return Err(MathError::WrongArity {
                function: "the change of variables".into(),
                expected: "2 or 3".into(),
                got: map.len(),
            })
        }
    };
    Ok(simplify(&det))
}

/// The sign of `e` over the region of `bounds`, sampled at interior points; `None` when it
/// changes or cannot be evaluated.
fn sign_over(e: &Expr, bounds: &[Bound]) -> Option<Ordering> {
    if let Some(s) = util::sign(e).filter(|_| e.is_constant()) {
        return Some(s);
    }
    let fractions = [0.13, 0.5, 0.87];
    let mut seen: Option<Ordering> = None;
    let mut samples = vec![Vec::<(String, Expr)>::new()];
    for (v, a, b) in bounds {
        let mut next = Vec::new();
        for s in &samples {
            let names: Vec<&str> = s.iter().map(|(n, _)| n.as_str()).collect();
            let vals: Vec<Expr> = s.iter().map(|(_, e)| e.clone()).collect();
            let lo = util::numeric(&subs_all(a, &names, &vals))?;
            let hi = util::numeric(&subs_all(b, &names, &vals))?;
            for f in fractions {
                let mut s2 = s.clone();
                s2.push((v.clone(), Expr::float(lo + f * (hi - lo))));
                next.push(s2);
            }
        }
        samples = next;
    }
    for s in samples {
        let names: Vec<&str> = s.iter().map(|(n, _)| n.as_str()).collect();
        let vals: Vec<Expr> = s.iter().map(|(_, e)| e.clone()).collect();
        let v = util::numeric(&subs_all(e, &names, &vals))?;
        let o = v.partial_cmp(&0.0)?;
        if o == Ordering::Equal {
            continue;
        }
        match seen {
            None => seen = Some(o),
            Some(p) if p != o => return None,
            _ => {}
        }
    }
    seen
}

/// Change of variables: `∬ f(x, y) dA = ∬ f(x(u, v), y(u, v)) |∂(x, y)/∂(u, v)| du dv`,
/// with the region given in `u, v` (outermost first). The sign of the Jacobian must not
/// change over the region.
pub fn change_of_variables(
    f: &Expr,
    vars: &[&str],
    map: &[Expr],
    bounds: &[Bound],
) -> MathResult<(Expr, Expr)> {
    if map.len() != vars.len() || bounds.len() != vars.len() {
        return Err(MathError::WrongArity {
            function: "the change of variables".into(),
            expected: vars.len().to_string(),
            got: map.len(),
        });
    }
    let params: Vec<&str> = bounds.iter().map(|(v, _, _)| v.as_str()).collect();
    let det = jacobian_det(map, &params)?;
    let abs = match sign_over(&det, bounds) {
        Some(Ordering::Less) => simplify(&(-det.clone())),
        Some(_) => det.clone(),
        None => {
            return Err(MathError::Unsupported(format!(
                "a change of variables whose Jacobian {det} changes sign on the region"
            )))
        }
    };
    let integrand = simplify(&mul(vec![subs_all(f, vars, map), abs.clone()]));
    Ok((integrate_bounds(&integrand, bounds)?, abs))
}

/// A parametrised surface (or plane region) `(u, v) ↦ Φ(u, v)`, with `u ∈ [a, b]` and
/// `v` between bounds that may depend on `u`.
#[derive(Clone, Debug, PartialEq)]
pub struct Patch {
    pub map: Vec<Expr>,
    pub u: Bound,
    pub v: Bound,
}

/// A plane region for Green's theorem is a two-component patch.
pub type Region = Patch;

impl Patch {
    fn names(&self) -> [&str; 2] {
        [self.u.0.as_str(), self.v.0.as_str()]
    }

    fn bounds(&self) -> Vec<Bound> {
        vec![self.u.clone(), self.v.clone()]
    }

    fn tangents(&self) -> MathResult<(Vec<Expr>, Vec<Expr>)> {
        let [u, v] = self.names();
        let du = self
            .map
            .iter()
            .map(|c| diff(c, u, 1, None))
            .collect::<MathResult<Vec<_>>>()?;
        let dv = self
            .map
            .iter()
            .map(|c| diff(c, v, 1, None))
            .collect::<MathResult<Vec<_>>>()?;
        Ok((du, dv))
    }

    /// `Φ_u × Φ_v` (the normal that orients the surface).
    pub fn normal(&self) -> MathResult<Vec<Expr>> {
        let (du, dv) = self.tangents()?;
        cross(&du, &dv)
    }

    /// The four edges of the parameter region, counterclockwise in `(u, v)`, as curves
    /// `τ ↦ Φ` with their parameter range and orientation sign.
    fn edges(&self, tau: &str) -> Vec<(Vec<Expr>, Expr, Expr, bool)> {
        let [u, v] = self.names();
        let t = Expr::sym(tau);
        let (a, b) = (self.u.1.clone(), self.u.2.clone());
        let lo_of = |x: &Expr| self.v.1.subs(u, x);
        let hi_of = |x: &Expr| self.v.2.subs(u, x);
        let on = |uu: &Expr, vv: &Expr| -> Vec<Expr> {
            self.map
                .iter()
                .map(|c| simplify(&c.subs_all(&[(u, uu.clone()), (v, vv.clone())])))
                .collect()
        };
        vec![
            (on(&t, &lo_of(&t)), a.clone(), b.clone(), true),
            (on(&b, &t), simplify(&lo_of(&b)), simplify(&hi_of(&b)), true),
            (on(&t, &hi_of(&t)), a.clone(), b.clone(), false),
            (
                on(&a, &t),
                simplify(&lo_of(&a)),
                simplify(&hi_of(&a)),
                false,
            ),
        ]
    }
}

/// `∮ F · dr` around the image of the parameter region's boundary.
fn boundary_circulation(field: &[Expr], vars: &[&str], patch: &Patch) -> MathResult<Expr> {
    let tau = "τ";
    let mut parts = Vec::new();
    for (curve, t0, t1, forward) in patch.edges(tau) {
        if util::equal(&t0, &t1) {
            continue;
        }
        let w = line_integral(field, vars, &curve, tau, &t0, &t1, None)?;
        parts.push(if forward { w } else { -w });
    }
    Ok(simplify(&crate::simplify::add(parts)))
}

/// `∬_S f dS = ∬ f(Φ) |Φ_u × Φ_v| du dv`.
pub fn surface_integral(f: &Expr, vars: &[&str], patch: &Patch) -> MathResult<Expr> {
    let n = patch.normal()?;
    let len = simplify(&pow(dot(&n, &n)?, Expr::rational(1, 2)));
    let integrand = simplify(&mul(vec![subs_all(f, vars, &patch.map), len]));
    integrate_bounds(&integrand, &patch.bounds())
}

/// `∬_S F · dS = ∬ F(Φ) · (Φ_u × Φ_v) du dv` (the flux, oriented by `Φ_u × Φ_v`).
pub fn flux_integral(field: &[Expr], vars: &[&str], patch: &Patch) -> MathResult<Expr> {
    let n = patch.normal()?;
    let on: Vec<Expr> = field
        .iter()
        .map(|c| subs_all(c, vars, &patch.map))
        .collect();
    let integrand = dot(&on, &n)?;
    integrate_bounds(&integrand, &patch.bounds())
}

/// Both sides of an integral theorem.
#[derive(Clone, Debug, PartialEq)]
pub struct TheoremCheck {
    /// The boundary side (line integral or flux through the closed surface).
    pub boundary: Expr,
    /// The interior side (double, surface or triple integral).
    pub interior: Expr,
    /// True when the two agree.
    pub agree: bool,
}

/// Green's theorem: `∮_C P dx + Q dy` (counterclockwise) against `∬_D (Q_x − P_y) dA`.
pub fn check_greens_theorem(
    field: &[Expr],
    vars: &[&str],
    region: &Region,
) -> MathResult<TheoremCheck> {
    if field.len() != 2 || vars.len() != 2 || region.map.len() != 2 {
        return Err(MathError::WrongArity {
            function: "Green's theorem".into(),
            expected: "2".into(),
            got: field.len(),
        });
    }
    let params = region.names();
    let det = jacobian_det(&region.map, &params)?;
    let sign = sign_over(&det, &region.bounds())
        .ok_or_else(|| MathError::Unsupported("a region map whose Jacobian changes sign".into()))?;
    let mut boundary = boundary_circulation(field, vars, region)?;
    let abs = if sign == Ordering::Less {
        boundary = simplify(&(-boundary));
        simplify(&(-det))
    } else {
        det
    };
    let rot = super::vector::curl(field, vars)?
        .pop()
        .unwrap_or_else(Expr::zero);
    let integrand = simplify(&mul(vec![subs_all(&rot, vars, &region.map), abs]));
    let interior = integrate_bounds(&integrand, &region.bounds())?;
    Ok(TheoremCheck {
        agree: agree(&boundary, &interior),
        boundary,
        interior,
    })
}

/// Stokes' theorem: `∮_∂S F · dr` against `∬_S (∇ × F) · dS`, the boundary oriented by the
/// surface's normal `Φ_u × Φ_v`.
pub fn check_stokes_theorem(
    field: &[Expr],
    vars: &[&str],
    surface: &Patch,
) -> MathResult<TheoremCheck> {
    let c = super::vector::curl(field, vars)?;
    let interior = flux_integral(&c, vars, surface)?;
    let boundary = boundary_circulation(field, vars, surface)?;
    Ok(TheoremCheck {
        agree: agree(&boundary, &interior),
        boundary,
        interior,
    })
}

/// A solid given by constant bounds in a coordinate system (`None` for Cartesian `x, y,
/// z`), outermost first.
#[derive(Clone, Debug, PartialEq)]
pub struct Solid {
    pub coords: Option<Coordinates>,
    pub bounds: Vec<Bound>,
}

/// The divergence theorem: the outward flux through the boundary of the solid (all six
/// faces of its parameter box) against `∭ ∇ · F dV`.
pub fn check_divergence_theorem(
    field: &[Expr],
    vars: &[&str],
    solid: &Solid,
) -> MathResult<TheoremCheck> {
    if solid.bounds.len() != 3 || field.len() != 3 || vars.len() != 3 {
        return Err(MathError::WrongArity {
            function: "the divergence theorem".into(),
            expected: "3".into(),
            got: solid.bounds.len(),
        });
    }
    let names: Vec<&str> = solid.bounds.iter().map(|(v, _, _)| v.as_str()).collect();
    for (_, a, b) in &solid.bounds {
        if names.iter().any(|n| a.contains_sym(n) || b.contains_sym(n)) {
            return Err(MathError::Unsupported(
                "the divergence theorem for bounds that depend on another variable".into(),
            ));
        }
    }
    // Ψ: the parameter box → space, in the order of the bounds.
    let map: Vec<Expr> = match solid.coords {
        None => vars
            .iter()
            .map(|v| {
                if names.contains(v) {
                    Ok(Expr::sym(v))
                } else {
                    Err(MathError::Undefined(format!("the bounds for {v}")))
                }
            })
            .collect::<MathResult<_>>()?,
        Some(c) => {
            let mut by_role = Vec::new();
            for want in c.roles() {
                let n = names
                    .iter()
                    .find(|n| role(n) == Some(want))
                    .ok_or_else(|| MathError::Undefined(format!("the bounds for {want}")))?;
                by_role.push(Expr::sym(n));
            }
            c.map(&by_role).0
        }
    };
    // The known Jacobian of a standard system is simpler than the expanded determinant.
    let det = match solid.coords {
        Some(c) => {
            let by_role: Vec<Expr> = c
                .roles()
                .iter()
                .filter_map(|want| names.iter().find(|n| role(n) == Some(want)))
                .map(|n| Expr::sym(n))
                .collect();
            let jac = c.map(&by_role).1;
            // The bounds may list the coordinates in another order than the roles; an odd
            // permutation flips the orientation of the parameter box.
            let det = jacobian_det(&map, &names)?;
            match sign_over(&det, &solid.bounds) {
                Some(Ordering::Less) => simplify(&(-jac)),
                _ => jac,
            }
        }
        None => jacobian_det(&map, &names)?,
    };
    let sign = sign_over(&det, &solid.bounds).ok_or_else(|| {
        MathError::Unsupported("a solid whose coordinate Jacobian changes sign".into())
    })?;
    let orient = |e: Expr| {
        if sign == Ordering::Less {
            simplify(&(-e))
        } else {
            e
        }
    };
    let div = super::vector::divergence(field, vars)?;
    let integrand = simplify(&mul(vec![subs_all(&div, vars, &map), orient(det)]));
    let interior = integrate_bounds(&integrand, &solid.bounds)?;
    // Faces: for the k-th parameter at its upper bound the outward normal is
    // Ψ_{k+1} × Ψ_{k+2} (cyclic); at its lower bound the opposite.
    let mut flux_parts = Vec::new();
    for k in 0..3 {
        let (p1, p2) = ((k + 1) % 3, (k + 2) % 3);
        for (value, outward) in [(&solid.bounds[k].2, true), (&solid.bounds[k].1, false)] {
            let face_map: Vec<Expr> = map
                .iter()
                .map(|c| simplify(&c.subs(names[k], value)))
                .collect();
            let patch = Patch {
                map: face_map,
                u: solid.bounds[p1].clone(),
                v: solid.bounds[p2].clone(),
            };
            let flux = flux_integral(field, vars, &patch)?;
            flux_parts.push(if outward { flux } else { -flux });
        }
    }
    let boundary = orient(simplify(&crate::simplify::add(flux_parts)));
    Ok(TheoremCheck {
        agree: agree(&boundary, &interior),
        boundary,
        interior,
    })
}
