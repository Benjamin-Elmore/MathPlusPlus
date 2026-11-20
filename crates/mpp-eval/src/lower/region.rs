//! Integrals over regions written in the subscript: boxes (`\iint_{[0,1]\times[0,2]} f \, dA`),
//! discs and balls written as conditions (`\iint_{x^2+y^2 \le 1} f \, dA` switches to polar
//! coordinates, `\iiint_{x^2+y^2+z^2 \le 4} f \, dV` to spherical ones), and closed integrals
//! around a circle or over a sphere (`\oint_{x^2+y^2=1} \mathbf F \cdot d\mathbf r`,
//! `\oiint_{x^2+y^2+z^2=1} \mathbf F \cdot d\mathbf S`).
//!
//! Each becomes ordinary iterated integrals, so the calculus evaluates them as usual.
//! A region not of these shapes keeps the "not supported" message.

use mpp_core::{Expr, ExprNode, RelOp};
use mpp_syntax::math::{BinOp, IntKind, Integral, Node, NodeKind};

use super::{Located, Lowerer};
use crate::names::node_key;

/// `∫_lo^hi expr d(var)`.
fn integral(expr: Expr, var: &str, lo: Expr, hi: Expr) -> Expr {
    Expr::new(ExprNode::Integral {
        expr,
        var: var.into(),
        bounds: Some((lo, hi)),
    })
}

fn sin(e: Expr) -> Expr {
    mpp_core::functions::func(mpp_core::Func::Sin, vec![e])
}

fn cos(e: Expr) -> Expr {
    mpp_core::functions::func(mpp_core::Func::Cos, vec![e])
}

fn two_pi() -> Expr {
    Expr::int(2) * Expr::pi()
}

impl Lowerer<'_> {
    /// The iterated integral an integral over a region means, or `None` when the region
    /// is not one of the shapes above.
    pub(super) fn region_integral(
        &mut self,
        i: &Integral,
        region: &Node,
    ) -> Result<Option<Expr>, Located> {
        let diffs: Vec<String> = i.diffs.iter().filter_map(node_key).collect();
        let dim = match i.kind {
            IntKind::Double => 2,
            IntKind::Triple => 3,
            IntKind::Contour => return self.around_circle(i, region, &diffs),
            IntKind::Surface => return self.over_sphere(i, region, &diffs),
            IntKind::Single => return Ok(None),
        };
        let Some(coords) = coordinates(&diffs, dim) else {
            return Ok(None);
        };
        if let Some(sides) = box_sides(region) {
            if sides.len() != dim {
                return Ok(None);
            }
            let mut bounds = Vec::with_capacity(dim);
            for side in sides {
                let NodeKind::Interval { lo, hi, .. } = &side.kind else {
                    return Ok(None);
                };
                bounds.push((self.lower(lo)?, self.lower(hi)?));
            }
            let body = self.bound(&coords, |s| s.lower(&i.body))?;
            let mut out = body;
            for (v, (lo, hi)) in coords.iter().zip(bounds).rev() {
                out = integral(out, v, lo, hi);
            }
            return Ok(Some(out));
        }
        let condition = self.bound(&coords, |s| s.lower(region))?;
        let Some(radius) = centred_ball(&condition, &coords, false) else {
            return Ok(None);
        };
        let body = self.bound(&coords, |s| s.lower(&i.body))?;
        Ok(match dim {
            2 => polar(&body, &coords, radius),
            _ => spherical(&body, &coords, radius),
        })
    }

    /// `∮_{x²+y²=R²} F · dr` (counter-clockwise) and `∮ f ds` around a circle.
    fn around_circle(
        &mut self,
        i: &Integral,
        region: &Node,
        diffs: &[String],
    ) -> Result<Option<Expr>, Located> {
        let coords = vec!["x".to_string(), "y".to_string()];
        let condition = self.bound(&coords, |s| s.lower(region))?;
        let Some(r) = centred_ball(&condition, &coords, true) else {
            return Ok(None);
        };
        let body = self.bound(&coords, |s| s.lower(&i.body))?;
        if body.contains_sym("t") {
            return Ok(None);
        }
        let t = Expr::sym("t");
        let (x, y) = (r.clone() * cos(t.clone()), r.clone() * sin(t.clone()));
        let at = |e: &Expr| e.subs("x", &x).subs("y", &y);
        let integrand = match (i.dot, diffs, body.node()) {
            (true, [_], ExprNode::Tuple(f)) if f.len() == 2 => {
                let dx = -(r.clone() * sin(t.clone()));
                let dy = r.clone() * cos(t.clone());
                at(&f[0]) * dx + at(&f[1]) * dy
            }
            (false, [s], _) if s == "s" => at(&body) * r,
            _ => return Ok(None),
        };
        Ok(Some(integral(integrand, "t", Expr::zero(), two_pi())))
    }

    /// `∯_{x²+y²+z²=R²} F · dS` (outward) and `∯ f dS` over a sphere.
    fn over_sphere(
        &mut self,
        i: &Integral,
        region: &Node,
        diffs: &[String],
    ) -> Result<Option<Expr>, Located> {
        let coords = vec!["x".to_string(), "y".to_string(), "z".to_string()];
        let condition = self.bound(&coords, |s| s.lower(region))?;
        let Some(r) = centred_ball(&condition, &coords, true) else {
            return Ok(None);
        };
        let body = self.bound(&coords, |s| s.lower(&i.body))?;
        if body.contains_sym("φ") || body.contains_sym("θ") {
            return Ok(None);
        }
        let (phi, theta) = (Expr::sym("φ"), Expr::sym("θ"));
        let unit = [
            sin(phi.clone()) * cos(theta.clone()),
            sin(phi.clone()) * sin(theta.clone()),
            cos(phi.clone()),
        ];
        let at = |e: &Expr| {
            e.subs("x", &(r.clone() * unit[0].clone()))
                .subs("y", &(r.clone() * unit[1].clone()))
                .subs("z", &(r.clone() * unit[2].clone()))
        };
        let area = r.clone().pow(2) * sin(phi.clone());
        let integrand = match (i.dot, diffs, body.node()) {
            (true, [_], ExprNode::Tuple(f)) if f.len() == 3 => {
                let normal: Expr = f
                    .iter()
                    .zip(unit.iter())
                    .map(|(c, u)| at(c) * u.clone())
                    .fold(Expr::zero(), |acc, x| acc + x);
                normal * area
            }
            (false, [s], _) if s == "S" => at(&body) * area,
            _ => return Ok(None),
        };
        let inner = integral(integrand, "φ", Expr::zero(), Expr::pi());
        Ok(Some(integral(inner, "θ", Expr::zero(), two_pi())))
    }
}

/// The coordinates of a region integral: `x, y (, z)` for `dA` / `dV`, or the written
/// differentials (in x, y, z order when they are those letters).
fn coordinates(diffs: &[String], dim: usize) -> Option<Vec<String>> {
    let standard = ["x", "y", "z"];
    match diffs {
        [d] if (dim == 2 && d == "A") || (dim == 3 && d == "V") => {
            Some(standard.iter().take(dim).map(|s| s.to_string()).collect())
        }
        ds if ds.len() == dim => {
            let mut out = ds.to_vec();
            if out.iter().all(|d| standard.contains(&d.as_str())) {
                out.sort();
            } else {
                out.reverse();
            }
            Some(out)
        }
        _ => None,
    }
}

/// The intervals of a box `[a, b] \times [c, d] (\times [e, f])`, outermost first.
fn box_sides(region: &Node) -> Option<Vec<&Node>> {
    let region = region.ungrouped();
    match &region.kind {
        NodeKind::Binary {
            op: BinOp::Times,
            lhs,
            rhs,
        } => {
            let mut out = box_sides(lhs)?;
            out.extend(box_sides(rhs)?);
            Some(out)
        }
        NodeKind::Interval { .. } => Some(vec![region]),
        _ => None,
    }
}

/// `R` when `condition` is `x² + y² (+ z²) ≤ R²` (or `<`, or `= R²` for a boundary when
/// `boundary` is set), centred at the origin.
fn centred_ball(condition: &Expr, coords: &[String], boundary: bool) -> Option<Expr> {
    let ExprNode::Relation(op, l, r) = condition.node() else {
        return None;
    };
    let g = match op {
        RelOp::Le | RelOp::Lt if !boundary => (l.clone() - r.clone()).simplify(),
        RelOp::Ge | RelOp::Gt if !boundary => (r.clone() - l.clone()).simplify(),
        RelOp::Eq if boundary => (l.clone() - r.clone()).simplify(),
        _ => return None,
    };
    let squares = coords
        .iter()
        .map(|c| Expr::sym(c).pow(2))
        .fold(Expr::zero(), |acc, x| acc + x)
        .simplify();
    let c = (squares - g).simplify();
    if coords.iter().any(|v| c.contains_sym(v)) || !c.is_constant() {
        return None;
    }
    let value = c.eval_f64(&mpp_core::EmptyEnv).ok()?;
    (value.is_finite() && value > 0.0).then(|| c.sqrt().simplify())
}

/// `∬_{x²+y²≤R²} f dA = ∫₀^{2π} ∫₀^R f(r cos θ, r sin θ) r dr dθ`.
fn polar(body: &Expr, coords: &[String], radius: Expr) -> Option<Expr> {
    if body.contains_sym("r") || body.contains_sym("θ") {
        return None;
    }
    let (r, theta) = (Expr::sym("r"), Expr::sym("θ"));
    let f = body
        .subs(&coords[0], &(r.clone() * cos(theta.clone())))
        .subs(&coords[1], &(r.clone() * sin(theta.clone())));
    let inner = integral(f * r, "r", Expr::zero(), radius);
    Some(integral(inner, "θ", Expr::zero(), two_pi()))
}

/// `∭_{x²+y²+z²≤R²} f dV = ∫₀^{2π} ∫₀^π ∫₀^R f ρ² sin φ dρ dφ dθ`.
fn spherical(body: &Expr, coords: &[String], radius: Expr) -> Option<Expr> {
    if ["ρ", "φ", "θ"].iter().any(|n| body.contains_sym(n)) {
        return None;
    }
    let (rho, phi, theta) = (Expr::sym("ρ"), Expr::sym("φ"), Expr::sym("θ"));
    let f = body
        .subs(
            &coords[0],
            &(rho.clone() * sin(phi.clone()) * cos(theta.clone())),
        )
        .subs(
            &coords[1],
            &(rho.clone() * sin(phi.clone()) * sin(theta.clone())),
        )
        .subs(&coords[2], &(rho.clone() * cos(phi.clone())));
    let jac = rho.clone().pow(2) * sin(phi.clone());
    let inner = integral(f * jac, "ρ", Expr::zero(), radius);
    let middle = integral(inner, "φ", Expr::zero(), Expr::pi());
    Some(integral(middle, "θ", Expr::zero(), two_pi()))
}
