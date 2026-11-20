//! Operator forms of the second set of calculus tools (`mpp_core::tools_b`): applications
//! of integrals, numerical methods, convergence tests, power-series intervals and the
//! differential-equation solver, plus the special functions `erf`, `Si` and `Ci` typed as
//! `\operatorname{erf}(x)`.
//!
//! Each operator is written `\operatorname{name}(arguments)`; scripts may carry an interval,
//! a method or a variant (`\operatorname{riemann}_{left}(x^2, [0, 1], 4)`,
//! `\operatorname{volume}_{shell}(x - x^2, [0, 1], x = 0)`). Arguments are recognised by
//! their shape: intervals `[a, b]` are bounds, `x = c` / `y = c` are axes (or starting
//! values), a lone name is the variable, words such as `shell` or `left` choose a method,
//! numbers are counts and points, and everything else is a function. The words are
//! recognised whether they arrive as one name (`\mathrm{shell}`) or as the product of their
//! letters (`shell` typed plainly).

use mpp_core::number::format_f64;
use mpp_core::tools_b::{self, Axis, QuadratureRule, VolumeMethod};
use mpp_core::{Expr, ExprNode, Func, MathError, MathResult, RelOp, Steps};

use crate::holders::Ctx;

/// Words that select a method or variant, recognised in scripts and arguments.
const KEYWORDS: &[&str] = &[
    "left",
    "right",
    "mid",
    "midpoint",
    "trapezoid",
    "trapezoidal",
    "simpson",
    "disk",
    "disc",
    "washer",
    "shell",
    "shells",
];

/// Evaluates the operators of this module; `None` for anything else.
pub fn evaluate(e: &Expr, ctx: &Ctx<'_>) -> Option<MathResult<Expr>> {
    let call = Call::read(e)?;
    if let Some(f) = special_function(&call.name) {
        return Some(match call.args.as_slice() {
            [a] => Expr::func(f, vec![a.clone()]),
            _ => Err(MathError::WrongArity {
                function: call.name.clone(),
                expected: "1".into(),
                got: call.args.len(),
            }),
        });
    }
    let result = match call.name.as_str() {
        "area" | "areabetween" => area(&call, ctx),
        "volume" | "vol" => volume(&call, ctx),
        "arclength" | "arc" => arc_length(&call, ctx),
        "surfacearea" | "surface" => surface(&call, ctx),
        "average" | "avg" | "averagevalue" => single_integral(&call, ctx, Single::Average),
        "work" => single_integral(&call, ctx, Single::Work),
        "centroid" => centroid(&call, ctx),
        "centreofmass" | "centerofmass" | "com" => single_integral(&call, ctx, Single::Rod),
        "accumulate" | "accumulation" => accumulate(&call, ctx),
        "riemann" | "leftsum" | "rightsum" | "midsum" | "midpoint" | "trapezoid"
        | "trapezoidal" | "simpson" => quadrature(&call, ctx),
        "newton" => newton(&call, ctx),
        "euler" | "rk4" | "rungekutta" => stepper(&call, ctx),
        "taylorbound" | "lagrange" | "remainder" => taylor_bound(&call, ctx),
        "termsneeded" | "taylorterms" => taylor_terms(&call, ctx),
        "tests" | "convtests" | "convergencetests" => tests(&call, ctx),
        "radius" | "roc" | "radiusofconvergence" => power_series(&call, ctx, true),
        "ioc" | "intervalofconvergence" => power_series(&call, ctx, false),
        _ => return None,
    };
    Some(result)
}

fn special_function(name: &str) -> Option<Func> {
    match name {
        "erf" => Some(Func::Erf),
        "si" => Some(Func::Si),
        "ci" => Some(Func::Ci),
        _ => None,
    }
}

/// An operator call with its scripts.
struct Call {
    /// Lower-case name.
    name: String,
    sub: Option<Expr>,
    sup: Option<Expr>,
    args: Vec<Expr>,
}

impl Call {
    fn read(e: &Expr) -> Option<Call> {
        let ExprNode::Apply(name, args) = e.node() else {
            return None;
        };
        if &**name == "operatorname" {
            let [which, sub, sup, body] = args.as_slice() else {
                return None;
            };
            let ExprNode::Tuple(items) = body.node() else {
                return None;
            };
            let name = which.as_sym()?.to_lowercase();
            let script = |s: &Expr| (!is_empty_tuple(s)).then(|| s.clone());
            return Some(Call {
                name,
                sub: script(sub),
                sup: script(sup),
                args: items.clone(),
            });
        }
        Some(Call {
            name: name.to_lowercase(),
            sub: None,
            sup: None,
            args: args.clone(),
        })
    }

    /// The arguments and scripts, sorted by shape.
    fn parts(&self) -> Parts {
        let mut p = Parts::default();
        for a in &self.args {
            p.add(a);
        }
        for s in self.sub.iter().chain(self.sup.iter()) {
            match s.as_sym() {
                Some(name) if keyword(s).is_none() => p.names.push(name.to_string()),
                _ => p.add(s),
            }
        }
        p
    }
}

fn is_empty_tuple(e: &Expr) -> bool {
    matches!(e.node(), ExprNode::Tuple(items) if items.is_empty())
}

/// Arguments sorted by shape.
#[derive(Default)]
struct Parts {
    intervals: Vec<(Expr, Expr)>,
    relations: Vec<(Expr, Expr)>,
    names: Vec<String>,
    words: Vec<String>,
    numbers: Vec<Expr>,
    tuples: Vec<Vec<Expr>>,
    functions: Vec<Expr>,
}

impl Parts {
    fn add(&mut self, a: &Expr) {
        if let Some(w) = keyword(a) {
            self.words.push(w);
            return;
        }
        match a.node() {
            ExprNode::Interval { lo, hi, .. } => self.intervals.push((lo.clone(), hi.clone())),
            ExprNode::Relation(RelOp::Eq, l, r) => self.relations.push((l.clone(), r.clone())),
            // A lone name before any function is itself a function (`area(x, x^2)`); after
            // one it names the variable (`area(x^2, x, [0, 1])`).
            ExprNode::Sym(_) if self.functions.is_empty() && self.tuples.is_empty() => {
                self.functions.push(a.clone())
            }
            ExprNode::Sym(s) => self.names.push(s.to_string()),
            ExprNode::Tuple(items) => self.tuples.push(items.clone()),
            _ if a.is_constant() => self.numbers.push(a.clone()),
            _ => self.functions.push(a.clone()),
        }
    }

    fn interval(&self, op: &str) -> MathResult<(Expr, Expr)> {
        self.intervals
            .first()
            .cloned()
            .ok_or_else(|| missing(op, "an interval [a, b]"))
    }

    /// The variable: a lone name given, else the only free symbol of the functions, else
    /// `x`.
    fn variable(&self, op: &str) -> MathResult<String> {
        if let Some(n) = self.names.first() {
            return Ok(n.clone());
        }
        let mut free = std::collections::BTreeSet::new();
        for f in &self.functions {
            free.extend(f.free_symbols().iter().map(|s| s.to_string()));
        }
        match free.len() {
            0 => Ok("x".into()),
            1 => Ok(free.into_iter().next().unwrap_or_else(|| "x".into())),
            _ if free.contains("x") => Ok("x".into()),
            _ => Err(missing(
                op,
                "the variable (give it as an argument, such as x)",
            )),
        }
    }

    fn word(&self, choices: &[&str]) -> Option<String> {
        self.words
            .iter()
            .find(|w| choices.contains(&w.as_str()))
            .cloned()
    }
}

fn missing(op: &str, what: &str) -> MathError {
    MathError::Domain {
        function: op.to_string(),
        detail: format!("it needs {what}"),
    }
}

/// A method word, typed as one name or as the product of its letters.
fn keyword(e: &Expr) -> Option<String> {
    if let Some(s) = e.as_sym() {
        let lower = s.to_lowercase();
        return KEYWORDS.contains(&lower.as_str()).then_some(lower);
    }
    let mut letters = Vec::new();
    collect_letters(e, &mut letters)?;
    if letters.len() < 3 {
        return None;
    }
    letters.sort_unstable();
    KEYWORDS
        .iter()
        .find(|k| {
            let mut w: Vec<char> = k.chars().collect();
            w.sort_unstable();
            w == letters
        })
        .map(|k| k.to_string())
}

fn collect_letters(e: &Expr, out: &mut Vec<char>) -> Option<()> {
    match e.node() {
        ExprNode::Sym(s) => {
            let mut chars = s.chars();
            let c = chars.next()?;
            if chars.next().is_some() || !c.is_ascii_alphabetic() {
                return None;
            }
            out.push(c.to_ascii_lowercase());
            Some(())
        }
        // `e` and `i` arrive as Euler's number and the imaginary unit.
        ExprNode::Const(mpp_core::Constant::E) => {
            out.push('e');
            Some(())
        }
        ExprNode::Num(n) if *n == mpp_core::Number::i() => {
            out.push('i');
            Some(())
        }
        ExprNode::Mul(fs) => fs.iter().try_for_each(|f| collect_letters(f, out)),
        ExprNode::Pow(b, x) => {
            let n = x.as_number()?.as_i64()?;
            if !(1..=4).contains(&n) {
                return None;
            }
            for _ in 0..n {
                collect_letters(b, out)?;
            }
            Some(())
        }
        _ => None,
    }
}

/// Runs a core routine with a step log when the statement asks for steps.
fn logged<T>(ctx: &Ctx<'_>, f: impl FnOnce(Option<&mut Steps>) -> MathResult<T>) -> MathResult<T> {
    if !ctx.logging() {
        return f(None);
    }
    let mut local = Steps::new();
    let out = f(Some(&mut local));
    ctx.append(local);
    out
}

fn has_float(e: &Expr) -> bool {
    match e.node() {
        ExprNode::Num(n) => !n.is_exact(),
        _ => e.children().into_iter().any(has_float),
    }
}

fn note_numeric(ctx: &Ctx<'_>, value: &Expr, what: &str) {
    if !ctx.numeric && has_float(value) {
        ctx.note(format!(
            "no closed form was found for {what}, so the value shown is numerical (≈)"
        ));
    }
}

fn to_f64(e: &Expr, what: &str) -> MathResult<f64> {
    e.eval_f64(&mpp_core::EmptyEnv)
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| MathError::NotANumber(format!("{what} ({e})")))
}

fn to_count(e: &Expr, what: &str) -> MathResult<usize> {
    e.as_number()
        .and_then(|n| n.as_i64())
        .and_then(|n| usize::try_from(n).ok())
        .filter(|n| *n > 0)
        .ok_or_else(|| MathError::Domain {
            function: what.into(),
            detail: format!("{e} is not a positive whole number"),
        })
}

// ---- applications ----------------------------------------------------------

fn area(call: &Call, ctx: &Ctx<'_>) -> MathResult<Expr> {
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
    let v = logged(ctx, |s| {
        tools_b::area_between(
            &f,
            g.as_ref(),
            &var,
            bounds.as_ref().map(|(a, b)| (a, b)),
            s,
        )
    })?;
    note_numeric(ctx, &v, "this area");
    Ok(v)
}

fn axis_of(p: &Parts) -> MathResult<Option<Axis>> {
    for (l, r) in &p.relations {
        match l.as_sym() {
            Some("x") => return Ok(Some(Axis::Vertical(r.clone()))),
            Some("y") => return Ok(Some(Axis::Horizontal(r.clone()))),
            _ => {}
        }
    }
    Ok(None)
}

fn volume(call: &Call, ctx: &Ctx<'_>) -> MathResult<Expr> {
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
        tools_b::volume(&f, g.as_ref(), &var, &a, &b, &axis, method, s)
    })?;
    note_numeric(ctx, &v, "this volume");
    Ok(v)
}

fn arc_length(call: &Call, ctx: &Ctx<'_>) -> MathResult<Expr> {
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
            tools_b::arc_length_parametric(xt, yt, &t, &a, &b, s)
        })?
    } else {
        let var = p.variable("arc length")?;
        let f = p
            .functions
            .first()
            .ok_or_else(|| missing("arc length", "a curve"))?;
        logged(ctx, |s| tools_b::arc_length(f, &var, &a, &b, s))?
    };
    note_numeric(ctx, &v, "this arc length");
    Ok(v)
}

fn surface(call: &Call, ctx: &Ctx<'_>) -> MathResult<Expr> {
    let p = call.parts();
    let var = p.variable("surface area")?;
    let (a, b) = p.interval("surface area")?;
    let f = p
        .functions
        .first()
        .ok_or_else(|| missing("surface area", "a curve"))?;
    let axis = axis_of(&p)?.unwrap_or_else(Axis::x_axis);
    let v = logged(ctx, |s| tools_b::surface_area(f, &var, &a, &b, &axis, s))?;
    note_numeric(ctx, &v, "this surface area");
    Ok(v)
}

enum Single {
    Average,
    Work,
    Rod,
}

fn single_integral(call: &Call, ctx: &Ctx<'_>, which: Single) -> MathResult<Expr> {
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
        Single::Average => tools_b::average_value(f, &var, &a, &b, s),
        Single::Work => tools_b::work(f, &var, &a, &b, s),
        Single::Rod => tools_b::centre_of_mass_rod(f, &var, &a, &b, s),
    })?;
    note_numeric(ctx, &v, name);
    Ok(v)
}

fn centroid(call: &Call, ctx: &Ctx<'_>) -> MathResult<Expr> {
    let p = call.parts();
    let var = p.variable("centroid")?;
    let (a, b) = p.interval("centroid")?;
    let (f, g) = match p.functions.as_slice() {
        [f] => (f.clone(), None),
        [f, g] => (f.clone(), Some(g.clone())),
        _ => return Err(missing("centroid", "one or two curves")),
    };
    let (cx, cy) = logged(ctx, |s| tools_b::centroid(&f, g.as_ref(), &var, &a, &b, s))?;
    let v = Expr::new(ExprNode::Tuple(vec![cx, cy]));
    note_numeric(ctx, &v, "this centroid");
    Ok(v)
}

fn accumulate(call: &Call, ctx: &Ctx<'_>) -> MathResult<Expr> {
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
    logged(ctx, |s| tools_b::accumulation(f, &var, &a, s))
}

// ---- numerical methods -------------------------------------------------------

fn quadrature(call: &Call, ctx: &Ctx<'_>) -> MathResult<Expr> {
    let p = call.parts();
    let rule = match call.name.as_str() {
        "leftsum" => QuadratureRule::Left,
        "rightsum" => QuadratureRule::Right,
        "midsum" | "midpoint" => QuadratureRule::Midpoint,
        "trapezoid" | "trapezoidal" => QuadratureRule::Trapezoid,
        "simpson" => QuadratureRule::Simpson,
        _ => match p
            .word(&["left", "right", "mid", "midpoint", "trapezoid", "trapezoidal", "simpson"])
            .as_deref()
        {
            Some("left") => QuadratureRule::Left,
            Some("right") => QuadratureRule::Right,
            Some("mid" | "midpoint") => QuadratureRule::Midpoint,
            Some("trapezoid" | "trapezoidal") => QuadratureRule::Trapezoid,
            Some("simpson") => QuadratureRule::Simpson,
            _ => {
                return Err(missing(
                    "riemann",
                    "the kind of sum: write riemann_{left}, riemann_{right}, riemann_{\\mathrm{mid}} or use midpoint, trapezoid or simpson",
                ))
            }
        },
    };
    let var = p.variable(rule.name())?;
    let (a, b) = p.interval(rule.name())?;
    let f = p
        .functions
        .first()
        .ok_or_else(|| missing(rule.name(), "a function"))?;
    let n = p
        .numbers
        .first()
        .ok_or_else(|| missing(rule.name(), "the number of subintervals n"))?;
    let n = to_count(n, rule.name())?;
    let q = logged(ctx, |s| {
        tools_b::quadrature_rule(f, &var, &a, &b, n, rule, s)
    })?;
    if let Some(bound) = q.error_bound {
        ctx.note(format!(
            "{} with n = {n}: error bound |E| ≤ {}",
            rule.name(),
            format_f64(bound, 4)
        ));
    }
    Ok(q.value)
}

fn newton(call: &Call, ctx: &Ctx<'_>) -> MathResult<Expr> {
    let p = call.parts();
    let var = p.variable("Newton's method")?;
    let f = match (p.functions.first(), p.relations.first()) {
        (Some(f), _) => f.clone(),
        (None, Some((l, r))) => mpp_core::simplify(&(l - r)),
        _ => return Err(missing("Newton's method", "a function")),
    };
    let x0 = p
        .numbers
        .first()
        .ok_or_else(|| missing("Newton's method", "a starting point x₀"))?;
    let x0 = to_f64(x0, "the starting point")?;
    let tol = match p.numbers.get(1) {
        Some(t) => to_f64(t, "the tolerance")?,
        None => 1e-12,
    };
    let (root, table) = logged(ctx, |s| tools_b::newton(&f, &var, x0, tol, s))?;
    ctx.note(format!(
        "Newton's method from x₀ = {} converged in {} iterations",
        format_f64(x0, 10),
        table.len()
    ));
    Ok(Expr::float(root))
}

fn stepper(call: &Call, ctx: &Ctx<'_>) -> MathResult<Expr> {
    let p = call.parts();
    let rk = call.name != "euler";
    let what = if rk { "RK4" } else { "Euler's method" };
    let rhs = match (p.functions.first(), p.relations.first()) {
        (Some(f), _) => f.clone(),
        (None, Some((_, r))) => r.clone(),
        _ => return Err(missing(what, "the slope f(x, y) of y′ = f(x, y)")),
    };
    let free: Vec<String> = rhs.free_symbols().iter().map(|s| s.to_string()).collect();
    let x = if free.iter().any(|s| s == "t") && !free.iter().any(|s| s == "x") {
        "t"
    } else {
        "x"
    };
    let y = "y";
    // Numbers: x₀, y₀, h, n — or a tuple (x₀, y₀) followed by h, n.
    let mut nums: Vec<Expr> = Vec::new();
    if let Some(t) = p.tuples.first() {
        nums.extend(t.iter().cloned());
    }
    nums.extend(p.numbers.iter().cloned());
    let [x0, y0, h, n] = nums.as_slice() else {
        return Err(missing(
            what,
            "x₀, y₀, the step h and the number of steps n",
        ));
    };
    let (x0, y0, h) = (
        to_f64(x0, "x₀")?,
        to_f64(y0, "y₀")?,
        to_f64(h, "the step h")?,
    );
    let n = to_count(n, what)?;
    let table = logged(ctx, |s| {
        if rk {
            tools_b::rk4(&rhs, x, y, x0, y0, h, n, s)
        } else {
            tools_b::euler(&rhs, x, y, x0, y0, h, n, s)
        }
    })?;
    let (xn, yn) = table.rows.last().copied().unwrap_or((x0, y0));
    ctx.note(format!(
        "{} with h = {}: y({}) ≈ {} after {n} steps",
        table.method,
        format_f64(h, 6),
        format_f64(xn, 8),
        format_f64(yn, 10)
    ));
    Ok(Expr::float(yn))
}

fn taylor_parts(call: &Call, what: &str) -> MathResult<(Expr, String, Expr, Expr, Expr)> {
    let p = call.parts();
    let var = p.variable(what)?;
    let f = p
        .functions
        .first()
        .ok_or_else(|| missing(what, "a function"))?
        .clone();
    let [a, x, last] = p.numbers.as_slice() else {
        return Err(missing(
            what,
            "the centre a, the point x and the degree (or ε)",
        ));
    };
    Ok((f, var, a.clone(), x.clone(), last.clone()))
}

fn taylor_bound(call: &Call, ctx: &Ctx<'_>) -> MathResult<Expr> {
    let (f, var, a, x, n) = taylor_parts(call, "Taylor bound")?;
    let n = to_count(&mpp_core::simplify(&(n + Expr::one())), "the degree")? - 1;
    let n = u32::try_from(n).map_err(|_| MathError::TooLarge("the degree".into()))?;
    let tb = logged(ctx, |s| tools_b::taylor_bound(&f, &var, &a, &x, n, s))?;
    Ok(Expr::float(tb.bound))
}

fn taylor_terms(call: &Call, ctx: &Ctx<'_>) -> MathResult<Expr> {
    let (f, var, a, x, eps) = taylor_parts(call, "Taylor terms")?;
    let eps = to_f64(&eps, "ε")?;
    let tb = logged(ctx, |s| {
        tools_b::taylor_terms_needed(&f, &var, &a, &x, eps, s)
    })?;
    ctx.note(format!(
        "degree {} (the Lagrange bound is {} < {})",
        tb.degree,
        format_f64(tb.bound, 4),
        format_f64(eps, 4)
    ));
    Ok(Expr::int(i64::from(tb.degree)))
}

// ---- series ------------------------------------------------------------------

fn index_of(p: &Parts, term: &Expr) -> String {
    if let Some(k) = p.names.iter().find(|n| term.contains_sym(n)) {
        return k.clone();
    }
    ["k", "n", "i", "j", "m"]
        .iter()
        .find(|k| term.contains_sym(k))
        .map(|k| k.to_string())
        .unwrap_or_else(|| "k".into())
}

fn tests(call: &Call, ctx: &Ctx<'_>) -> MathResult<Expr> {
    let p = call.parts();
    let term = p
        .functions
        .first()
        .ok_or_else(|| missing("convergence tests", "the general term aₖ"))?;
    let k = index_of(&p, term);
    let (verdict, outcomes) = logged(ctx, |s| tools_b::series_tests(term, &k, s))?;
    for o in &outcomes {
        ctx.note(o.to_string());
    }
    match verdict {
        Some(v) => Ok(Expr::new(ExprNode::Bool(v))),
        None => Err(MathError::Undetermined(format!(
            "whether Σ {term} converges: no test decided"
        ))),
    }
}

fn power_series(call: &Call, ctx: &Ctx<'_>, radius: bool) -> MathResult<Expr> {
    let p = call.parts();
    let term = p
        .functions
        .first()
        .ok_or_else(|| missing("interval of convergence", "the general term"))?;
    let k = index_of(&p, term);
    let x = p
        .names
        .iter()
        .find(|n| **n != k)
        .cloned()
        .or_else(|| {
            term.free_symbols()
                .iter()
                .map(|s| s.to_string())
                .find(|s| *s != k)
        })
        .ok_or_else(|| missing("interval of convergence", "the variable x"))?;
    let r = logged(ctx, |s| tools_b::power_series_interval(term, &k, &x, s))?;
    for (end, report) in &r.endpoints {
        ctx.note(format!("at {x} = {end}: {report}"));
    }
    Ok(if radius { r.radius } else { r.interval })
}
