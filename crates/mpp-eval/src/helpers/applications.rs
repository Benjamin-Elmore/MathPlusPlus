//! Applications of the definite integral as operators: `area`, `volume`, `arclength`,
//! `surfacearea`, `average`, `work`, `centroid`, `centreofmass` and `accumulate`, answered
//! by [`mpp_core::applications`].

use mpp_core::applications::{self, Axis, VolumeMethod};
use mpp_core::{Expr, ExprNode, MathError, MathResult};

use super::operator_args::*;
use crate::holders::Ctx;

pub(super) fn area(call: &Call, ctx: &Ctx<'_>) -> MathResult<Expr> {
    let p = call.parts();
    let var = p.variable("area")?;
    let (f, g) = match p.functions.as_slice() {
        [f] => (f.clone(), None),
        [f, g] => (f.clone(), Some(g.clone())),
        [] => match p.numbers.as_slice() {
            [c, ..] => (c.clone(), None),
            [] => return Err(missing("area", "a curve")),
        },
        _ => return Err(missing("area", "one or two curves")),
    };
    let bounds = p.intervals.first().cloned();
    let run = |g: Option<&Expr>| {
        logged(ctx, |s| {
            applications::area_between(&f, g, &var, bounds.as_ref().map(|(a, b)| (a, b)), s)
        })
    };
    let v = match run(g.as_ref()) {
        // `area(x^3, x)` with no interval: when `x` as the variable leaves no enclosed
        // region, it is the second curve `y = x` (they cross at −1, 0 and 1).
        Err(MathError::Domain { .. })
            if g.is_none() && bounds.is_none() && p.names.first() == Some(&var) =>
        {
            let curve = Expr::sym(&var);
            let v = run(Some(&curve))?;
            ctx.note(format!("{var} is read as the curve y = {var}"));
            v
        }
        other => other?,
    };
    note_numeric(ctx, &v, "this area");
    Ok(v)
}

pub(super) fn axis_of(p: &Parts) -> MathResult<Option<Axis>> {
    for (l, r) in &p.relations {
        match l.as_sym() {
            Some("x") => return Ok(Some(Axis::Vertical(r.clone()))),
            Some("y") => return Ok(Some(Axis::Horizontal(r.clone()))),
            _ => {}
        }
    }
    Ok(None)
}

pub(super) fn volume(call: &Call, ctx: &Ctx<'_>) -> MathResult<Expr> {
    let p = call.parts();
    let var = p.variable("volume")?;
    let (a, b) = p.interval("volume")?;
    let (f, g) = match p.functions.as_slice() {
        [f] => (f.clone(), None),
        [f, g] => (f.clone(), Some(g.clone())),
        _ => return Err(missing("volume", "one or two curves")),
    };
    let default_axis = if var == "y" {
        Axis::Vertical(Expr::zero())
    } else {
        Axis::x_axis()
    };
    let axis = axis_of(&p)?.unwrap_or(default_axis);
    let method = p
        .word(&["disk", "disc", "washer", "shell", "shells"])
        .map(|w| match w.as_str() {
            "shell" | "shells" => VolumeMethod::Shell,
            "washer" => VolumeMethod::Washer,
            _ => VolumeMethod::Disk,
        });
    let v = logged(ctx, |s| {
        applications::volume(&f, g.as_ref(), &var, &a, &b, &axis, method, s)
    })?;
    note_numeric(ctx, &v, "this volume");
    Ok(v)
}

pub(super) fn arc_length(call: &Call, ctx: &Ctx<'_>) -> MathResult<Expr> {
    let p = call.parts();
    let (a, b) = p.interval("arc length")?;
    let v = if let Some([xt, yt]) = p.tuples.first().map(Vec::as_slice) {
        let t = match p.names.first() {
            Some(t) => t.clone(),
            None => {
                let free: Vec<String> = xt
                    .free_symbols()
                    .iter()
                    .chain(yt.free_symbols().iter())
                    .map(|s| s.to_string())
                    .collect();
                free.first().cloned().unwrap_or_else(|| "t".into())
            }
        };
        logged(ctx, |s| {
            applications::arc_length_parametric(xt, yt, &t, &a, &b, s)
        })?
    } else {
        let var = p.variable("arc length")?;
        let f = p
            .functions
            .first()
            .ok_or_else(|| missing("arc length", "a curve"))?;
        logged(ctx, |s| applications::arc_length(f, &var, &a, &b, s))?
    };
    note_numeric(ctx, &v, "this arc length");
    Ok(v)
}

pub(super) fn surface(call: &Call, ctx: &Ctx<'_>) -> MathResult<Expr> {
    let p = call.parts();
    let var = p.variable("surface area")?;
    let (a, b) = p.interval("surface area")?;
    let f = p
        .functions
        .first()
        .ok_or_else(|| missing("surface area", "a curve"))?;
    let axis = axis_of(&p)?.unwrap_or_else(Axis::x_axis);
    let v = logged(ctx, |s| {
        applications::surface_area(f, &var, &a, &b, &axis, s)
    })?;
    note_numeric(ctx, &v, "this surface area");
    Ok(v)
}

pub(super) enum Single {
    Average,
    Work,
    Rod,
}

pub(super) fn single_integral(call: &Call, ctx: &Ctx<'_>, which: Single) -> MathResult<Expr> {
    let p = call.parts();
    let name = match which {
        Single::Average => "average value",
        Single::Work => "work",
        Single::Rod => "centre of mass",
    };
    let var = p.variable(name)?;
    let (a, b) = p.interval(name)?;
    let f = p
        .functions
        .first()
        .or(p.numbers.first())
        .ok_or_else(|| missing(name, "a function"))?;
    let v = logged(ctx, |s| match which {
        Single::Average => applications::average_value(f, &var, &a, &b, s),
        Single::Work => applications::work(f, &var, &a, &b, s),
        Single::Rod => applications::centre_of_mass_rod(f, &var, &a, &b, s),
    })?;
    note_numeric(ctx, &v, name);
    Ok(v)
}

pub(super) fn centroid(call: &Call, ctx: &Ctx<'_>) -> MathResult<Expr> {
    let p = call.parts();
    let var = p.variable("centroid")?;
    let (a, b) = p.interval("centroid")?;
    let (f, g) = match p.functions.as_slice() {
        [f] => (f.clone(), None),
        [f, g] => (f.clone(), Some(g.clone())),
        _ => return Err(missing("centroid", "one or two curves")),
    };
    let (cx, cy) = logged(ctx, |s| {
        applications::centroid(&f, g.as_ref(), &var, &a, &b, s)
    })?;
    let v = Expr::new(ExprNode::Tuple(vec![cx, cy]));
    note_numeric(ctx, &v, "this centroid");
    Ok(v)
}

pub(super) fn accumulate(call: &Call, ctx: &Ctx<'_>) -> MathResult<Expr> {
    let p = call.parts();
    let var = p.variable("accumulation")?;
    let f = p
        .functions
        .first()
        .ok_or_else(|| missing("accumulation", "a function"))?;
    let a = p
        .numbers
        .first()
        .cloned()
        .or_else(|| p.intervals.first().map(|(a, _)| a.clone()))
        .unwrap_or_else(Expr::zero);
    logged(ctx, |s| applications::accumulation(f, &var, &a, s))
}
