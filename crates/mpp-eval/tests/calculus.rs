//! Calculus queries in whole documents, typed the way users type them: LaTeX, the plain
//! shortcut form and Unicode.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use mpp_core::calc::numeric_integrate;
use mpp_core::Expr;
use mpp_eval::{Evaluated, LineResult, NoImports, Session};
use mpp_syntax::Severity;
use tempfile::TempDir;

fn run(src: &str) -> Evaluated {
    Session::new()
        .with_importer(Arc::new(NoImports))
        .run_str(src)
        .clone()
}

fn answers(ev: &Evaluated) -> Vec<&LineResult> {
    ev.queries().map(|(_, l)| l).collect()
}

fn errors(ev: &Evaluated) -> Vec<String> {
    ev.errors().iter().map(|d| d.message.clone()).collect()
}

fn warnings(ev: &Evaluated) -> Vec<String> {
    ev.all_diagnostics()
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .map(|d| d.message.clone())
        .collect()
}

fn no_errors(ev: &Evaluated) {
    let errs = errors(ev);
    assert!(errs.is_empty(), "unexpected errors: {errs:?}");
}

fn exact(line: &LineResult) -> String {
    line.plain.clone().unwrap_or_else(|| "<none>".into())
}

fn number(line: &LineResult) -> f64 {
    line.approx
        .as_deref()
        .and_then(|a| a.parse().ok())
        .unwrap_or_else(|| panic!("no number in {:?}", line.approx))
}

/// The answer text of a document with one query.
fn answer(src: &str) -> String {
    let ev = run(src);
    no_errors(&ev);
    let a = answers(&ev);
    assert_eq!(a.len(), 1, "{src}");
    exact(a[0])
}

/// The error messages of a document with one query that must fail.
fn failure(src: &str) -> String {
    let ev = run(src);
    let errs = errors(&ev);
    assert_eq!(errs.len(), 1, "{src}: {errs:?}");
    errs[0].clone()
}

#[test]
fn the_example_document_with_definitions_inlined() {
    let src = r"# Projectile motion
Let $g \equiv 9.81$ be gravity and $v_0 := 20$ the launch speed.

$$ \theta := \frac{\pi}{4} $$
$$ R := \frac{v_0^2 \sin 2\theta}{g} $$
$$ m := 2 $$

$$ y(x) := x \tan\theta - \frac{g x^2}{2 v_0^2 \cos^2\theta} $$
$$ E := \tfrac12 m v_0^2 $$

$$ \frac{d}{dx} y(x) = ? $$
$$ \int_0^{R} y(x)\,dx \approx ? $$
$$ \sum_{n=1}^{\infty} \frac{1}{n^2} = ? $$
$$ \lim_{x\to 0} \frac{\sin x}{x} = ? $$
$$ E = ? $$

Figure: $y(x)$, $x \in [0, R]$.
";
    let ev = run(src);
    no_errors(&ev);
    let a = answers(&ev);
    assert_eq!(a.len(), 5);
    // y = x − 981x²/40000, so y′ = 1 − 981x/20000 (g and v_0 put in, θ = π/4).
    assert_eq!(exact(a[0]), "-981*x/20000 + 1");
    // The area under the trajectory, checked against direct quadrature.
    let r = 40000.0 / 981.0;
    let x = Expr::sym("x");
    let y = x.clone() - Expr::rational(981, 40000) * x.pow(2);
    let (check, _) = numeric_integrate(&y, "x", 0.0, r, 1e-12).expect("quadrature");
    let area = number(a[1]);
    assert!((area - check).abs() < 1e-6 * check, "{area} vs {check}");
    assert!((area - r * r / 6.0).abs() < 1e-6, "{area}");
    assert_eq!(exact(a[2]), "π^2/6");
    assert_eq!(exact(a[3]), "1");
    assert_eq!(exact(a[4]), "400");
}

#[test]
fn the_example_document_with_fixture_imports() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let dir = TempDir::new().expect("temp dir");
    for name in ["launch.mpp", "constants.mpp"] {
        fs::copy(fixtures.join(name), dir.path().join(name)).expect("copy fixture");
    }
    let src = r"# Projectile motion
Let $g \equiv 9.81$ be gravity and $v_0 := 20$ the launch speed.
Using $\theta, R(\theta)$ from [[launch.mpp]].
![[constants.mpp]]

$$ y(x) := x \tan\theta - \frac{g x^2}{2 v_0^2 \cos^2\theta} $$
$$ E := \tfrac12 m v_0^2 $$

$$ \frac{d}{dx} y(x) = ? $$
$$ \int_0^{R} y(x)\,dx \approx ? $$
$$ \sum_{n=1}^{\infty} \frac{1}{n^2} = ? $$
$$ \lim_{x\to 0} \frac{\sin x}{x} = ? $$
";
    let path = dir.path().join("projectile.mpp");
    fs::write(&path, src).expect("write");
    let mut session = Session::new();
    let ev = session.run_file(&path).expect("run").clone();
    no_errors(&ev);
    let a = answers(&ev);
    assert_eq!(exact(a[0]), "-981*x/20000 + 1");
    let r = 40000.0 / 981.0;
    assert!((number(a[1]) - r * r / 6.0).abs() < 1e-6);
    assert_eq!(exact(a[2]), "π^2/6");
    assert_eq!(exact(a[3]), "1");
}

/// Queries with exact answers, in LaTeX, plain and Unicode forms.
#[test]
fn queries_as_users_type_them() {
    let table: &[(&str, &str)] = &[
        // Derivatives.
        (r"$$ \frac{d}{dx} x^3 = ? $$", "3*x^2"),
        ("$$ d/dx x^2 = ? $$", "2*x"),
        (r"$$ \frac{d^2}{dx^2} \sin x = ? $$", "-sin(x)"),
        ("$$ d^2/dx^2 x^4 = ? $$", "12*x^2"),
        (r"$$ \frac{d}{dx} \sin(x^2) = ? $$", "2*x*cos(x^2)"),
        ("$$ f(x) := x^3 $$\n\n$$ f'(x) = ? $$", "3*x^2"),
        ("$$ f(x) := x^3 $$\n\n$$ f''(2) = ? $$", "12"),
        ("$$ g(t) := t^2 e^t $$\n\n$$ g''(0) = ? $$", "2"),
        (r"$$ \frac{\partial}{\partial y} x^2 y = ? $$", "x^2"),
        ("$$ partial/partial x (x^2 y^3) = ? $$", "2*x*y^3"),
        ("$$ ∂²/∂x² (x³ y) = ? $$", "6*x*y"),
        // ∇.
        (r"$$ \nabla (x^2 y) = ? $$", "(2*x*y, x^2)"),
        ("$$ nabla (x^2 y) = ? $$", "(2*x*y, x^2)"),
        ("$$ ∇(x² y) = ? $$", "(2*x*y, x^2)"),
        (
            "$$ f(x, y) := x^2 y + \\sin y $$\n\n$$ \\nabla f = ? $$",
            "(2*x*y, x^2 + cos(y))",
        ),
        (r"$$ \nabla \cdot (x^2, y z, z) = ? $$", "2*x + z + 1"),
        ("$$ ∇·(x, y, z) = ? $$", "3"),
        (r"$$ \nabla \times (y, -x, 0) = ? $$", "(0, 0, -2)"),
        ("$$ ∇×(-y, x, 0) = ? $$", "(0, 0, 2)"),
        (r"$$ \nabla^2 (x^2 + y^2) = ? $$", "4"),
        ("$$ ∇²(x² y²) = ? $$", "2*x^2 + 2*y^2"),
        // Limits.
        (r"$$ \lim_{x \to 0} \frac{\sin x}{x} = ? $$", "1"),
        ("$$ lim_(x->0) sin(x)/x = ? $$", "1"),
        ("$$ lim_{x→0} (1 - cos x)/x^2 = ? $$", "1/2"),
        (r"$$ \lim_{x \to 0^+} \ln x = ? $$", "-∞"),
        (r"$$ \lim_{x \to 0^-} \frac{1}{x} = ? $$", "-∞"),
        ("$$ lim_(x->0^-) abs(x)/x = ? $$", "-1"),
        (r"$$ \lim_{x \to \infty} \frac{1}{x} = ? $$", "0"),
        ("$$ lim_(x->inf) (1 + 1/x)^x = ? $$", "e"),
        (r"$$ \lim_{x \to -\infty} e^{x} = ? $$", "0"),
        ("$$ lim_{x→∞} x/e^x = ? $$", "0"),
        // Indefinite integrals show the constant.
        (r"$$ \int x^2 \, dx = ? $$", "x^3/3 + C"),
        ("$$ int x cos(x) dx = ? $$", "x*sin(x) + cos(x) + C"),
        ("$$ ∫ 1/x dx = ? $$", "ln(|x|) + C"),
        // Definite and improper integrals.
        (r"$$ \int_0^1 x^2 \, dx = ? $$", "1/3"),
        ("$$ int_0^1 x^2 dx = ? $$", "1/3"),
        ("$$ ∫₀¹ x² dx = ? $$", "1/3"),
        ("$$ int_0^pi sin(x) dx = ? $$", "2"),
        ("$$ int_0^inf e^(-x) dx = ? $$", "1"),
        (r"$$ \int_1^{\infty} \frac{1}{x^2} \, dx = ? $$", "1"),
        ("$$ ∫₁^∞ 1/x³ dx = ? $$", "1/2"),
        (r"$$ \int_0^1 \frac{1}{\sqrt{x}} \, dx = ? $$", "2"),
        (
            r"$$ \int_3^{\infty} \frac{1}{x \ln^2 x} \, dx = ? $$",
            "1/ln(3)",
        ),
        (
            r"$$ \int_e^{\infty} \frac{1}{x (\ln x)^2} \, dx = ? $$",
            "1",
        ),
        // Multiple integrals, rectangular and with variable bounds.
        (r"$$ \iint_0^1 x y \, dx \, dy = ? $$", "1/4"),
        ("$$ ∭_0^1 x y z dx dy dz = ? $$", "1/8"),
        (r"$$ \int_0^1 \int_0^x x y \, dy \, dx = ? $$", "1/8"),
        ("$$ ∫_0^1 ∫_0^(x^2) 1 dy dx = ? $$", "1/3"),
        (
            r"$$ \int_0^1 \int_0^{1-x} \int_0^{1-x-y} 1 \, dz \, dy \, dx = ? $$",
            "1/6",
        ),
        // Sums and products.
        (r"$$ \sum_{n=1}^{\infty} \frac{1}{n^2} = ? $$", "π^2/6"),
        ("$$ sum_(n=1)^inf 1/n^2 = ? $$", "π^2/6"),
        ("$$ ∑_{n=0}^∞ (1/3)^n = ? $$", "3/2"),
        (r"$$ \sum_{n=0}^{\infty} \frac{1}{n!} = ? $$", "e"),
        (r"$$ \sum_{k=1}^{n} k^2 = ? $$", "n*(n + 1)*(2*n + 1)/6"),
        ("$$ sum_(k=1)^n k = ? $$", "n*(n + 1)/2"),
        (r"$$ \prod_{k=1}^{5} k = ? $$", "120"),
        ("$$ prod_(k=1)^4 k = ? $$", "24"),
        ("$$ ∏_{k=1}^{6} k = ? $$", "720"),
        (
            r"$$ \prod_{n=2}^{\infty} \left(1 - \frac{1}{n^2}\right) = ? $$",
            "1/2",
        ),
        // Taylor series with the remainder term.
        (
            r"$$ \operatorname{series}_{x=0}^{5} \sin x = ? $$",
            "x - x^3/6 + O(x^5)",
        ),
        (
            r"$$ \operatorname{series}_{x=0}^{4} e^x = ? $$",
            "1 + x + x^2/2 + x^3/6 + O(x^4)",
        ),
        (
            r"$$ \operatorname{series}_{x=1}^{3} \ln x = ? $$",
            "x - 1 - (x - 1)^2/2 + O((x - 1)^3)",
        ),
    ];
    assert!(table.len() >= 30);
    for (src, expected) in table {
        assert_eq!(answer(src), *expected, "{src}");
    }
}

#[test]
fn numeric_queries_and_numerical_fallbacks() {
    let ev = run(r"$$ \int_0^1 x^2 \, dx \approx ? $$

$$ \sum_{n=1}^{\infty} \frac{1}{n^3} \approx ? $$

$$ \int_0^1 e^{-x^3} \, dx = ? $$

$$ ∫_0^1 sin(x^2) dx ≈ ? $$
");
    no_errors(&ev);
    let a = answers(&ev);
    assert!((number(a[0]) - 1.0 / 3.0).abs() < 1e-9);
    assert!((number(a[1]) - 1.202_056_903_159_594).abs() < 1e-8);
    // No closed form: the exact query falls back to the number, shown with ≈.
    let x = Expr::sym("x");
    let cubic = Expr::e().pow(-x.pow(3));
    let (check, _) = numeric_integrate(&cubic, "x", 0.0, 1.0, 1e-12).expect("quadrature");
    assert!((number(a[2]) - check).abs() < 1e-9);
    assert!(a[2].plain_answer().is_some_and(|t| t.starts_with('≈')));
    assert!((number(a[3]) - 0.310_268_301_723_381).abs() < 1e-9);
    let w = warnings(&ev);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("no closed form"), "{}", w[0]);
}

#[test]
fn steps_are_collected_for_show_the_working() {
    let cases: &[(&str, &str)] = &[
        (r"$$ \frac{d}{dx} \sin(x^2) = ?! $$", "chain rule"),
        (r"$$ \frac{d}{dx} x^2 \sin x = ?! $$", "product rule"),
        (r"$$ \int x e^x \, dx = ?! $$", "integration by parts"),
        (
            r"$$ \int_0^1 x^2 \, dx = ?! $$",
            "fundamental theorem of calculus",
        ),
        (
            r"$$ \lim_{x \to 0} \frac{\sin x}{x} = ?! $$",
            "L'Hôpital's rule",
        ),
        (
            r"$$ \sum_{n=1}^{\infty} \frac{1}{2^n} = ?! $$",
            "geometric series",
        ),
        (
            "$$ a_1 := 1 $$\n\n$$ a_{n+1} := 2 a_n $$\n\n$$ a_{10} = ?! $$",
            "recurrence",
        ),
    ];
    for (src, rule) in cases {
        let ev = run(src);
        no_errors(&ev);
        let line = answers(&ev).pop().expect("a query");
        let steps = line.steps.as_ref().expect("steps");
        assert!(
            steps.0.iter().any(|s| s.rule.contains(rule)),
            "{src}: {:?}",
            steps.0.iter().map(|s| &s.rule).collect::<Vec<_>>()
        );
        // The block keeps the same working, by line.
        let block = ev
            .blocks
            .iter()
            .find(|b| b.values.iter().any(|v| v == line))
            .expect("block");
        let idx = block.values.iter().position(|v| v == line).expect("index");
        assert_eq!(block.steps_for(idx), Some(steps));
    }
    // Plain `= ?` records no steps.
    let ev = run(r"$$ \frac{d}{dx} x^2 = ? $$");
    assert!(answers(&ev)[0].steps.is_none());
}

#[test]
fn substituted_values_appear_as_the_first_step() {
    let ev = run("$$ k := 3 $$\n\n$$ \\int_0^k x^2 \\, dx = ?! $$\n");
    no_errors(&ev);
    let line = answers(&ev)[0];
    assert_eq!(exact(line), "9");
    let steps = line.steps.as_ref().expect("steps");
    assert_eq!(steps.0[0].rule, "substitute the known values");
    assert_eq!(steps.0[0].after.to_string(), "∫_0^3 x^2 dx");
}

#[test]
fn divergence_and_missing_limits_are_explained() {
    let m = failure(r"$$ \sum_{n=1}^{\infty} \frac{1}{n} = ? $$");
    assert!(
        m.contains("p-series test") && m.ends_with("diverges"),
        "{m}"
    );
    let m = failure("$$ ∑_{n=1}^∞ 1/√n = ? $$");
    assert!(m.contains("p-series test") && m.contains("1/2"), "{m}");
    let m = failure("$$ sum_(n=1)^inf n = ? $$");
    assert!(m.contains("divergence test"), "{m}");
    let m = failure(r"$$ \sum_{n=0}^{\infty} 2^n = ? $$");
    assert!(m.contains("geometric series test"), "{m}");
    let m = failure("$$ prod_(n=1)^inf (1 + 1/n) = ? $$");
    assert!(m.contains("diverges"), "{m}");
    let m = failure(r"$$ \int_1^{\infty} \frac{1}{x} \, dx = ? $$");
    assert!(m.contains("diverges"), "{m}");
    let m = failure(r"$$ \int_2^{\infty} \frac{1}{x \ln x} \, dx = ? $$");
    assert!(m.contains("diverges"), "{m}");
    let m = failure("$$ ∫_{-1}^{1} 1/x^2 dx = ? $$");
    assert!(m.contains("diverges") && m.contains("x = 0"), "{m}");
    let m = failure(r"$$ \lim_{x \to 0} \frac{1}{x} = ? $$");
    assert!(m.contains("limit does not exist"), "{m}");
    let m = failure(r"$$ \int e^{x^3} \, dx = ? $$");
    assert!(m.contains("no closed form"), "{m}");
    let m = failure(r"$$ \oint_C F \cdot dr = ? $$");
    assert!(m.contains("not supported yet"), "{m}");
}

#[test]
fn definitions_and_constants_are_substituted() {
    let src = r"$$ g \equiv 9.81 $$

$$ k := 3 $$

$$ y(x) := k x^2 + x $$

$$ \frac{d}{dx} y(x) = ? $$

$$ \int_0^k y(x) \, dx = ? $$

$$ \int_0^b c x \, dx = ? $$

$$ \frac{d}{dx} (a x^2 + k x) = ? $$

$$ \lim_{x \to \infty} \frac{g x}{x + 1} = ? $$

$$ \sum_{n=0}^{\infty} \frac{y(1)}{2^n} = ? $$
";
    let ev = run(src);
    no_errors(&ev);
    let a = answers(&ev);
    assert_eq!(exact(a[0]), "6*x + 1");
    assert_eq!(exact(a[1]), "63/2");
    // Unknown names stay symbolic in exact mode.
    assert_eq!(exact(a[2]), "b^2*c/2");
    assert_eq!(exact(a[3]), "2*a*x + 3");
    assert_eq!(exact(a[4]), "981/100");
    assert_eq!(exact(a[5]), "8");
}

#[test]
fn integration_variables_are_not_replaced_by_definitions() {
    // `x := 5` is a document variable, but inside ∫ … dx and d/dx it is bound.
    let src = r"$$ x := 5 $$

$$ \int_0^2 x \, dx = ? $$

$$ \frac{d}{dx} x^3 = ? $$

$$ \operatorname{series}_{x=0}^{3} e^x = ? $$

$$ x^2 = ? $$
";
    let ev = run(src);
    no_errors(&ev);
    let a = answers(&ev);
    assert_eq!(exact(a[0]), "2");
    assert_eq!(exact(a[1]), "3*x^2");
    assert_eq!(exact(a[2]), "1 + x + x^2/2 + O(x^3)");
    assert_eq!(exact(a[3]), "25");
}

#[test]
fn recurrences_use_the_core_sequence_engine() {
    let src = r"$$ a_1 := 1 $$

$$ a_{n+1} := a_n + 2 $$

$$ a_{50} = ? $$

$$ F_1 := 1 \\ F_2 := 1 \\ F_{n+1} := F_n + F_{n-1} $$

$$ F_{50} = ? $$

$$ b_0 := 1 $$

$$ b_{n+1} := \frac{b_n}{2} + 1 $$

$$ b_{20} = ? $$
";
    let ev = run(src);
    no_errors(&ev);
    let a = answers(&ev);
    assert_eq!(exact(a[0]), "99");
    assert_eq!(exact(a[1]), "12586269025");
    // b_n = 2 − 2^(−n).
    assert_eq!(exact(a[2]), "2097151/1048576");
}

#[test]
fn vector_fields_given_as_functions() {
    let src = r"$$ F(x, y, z) := (x y, y z, z x) $$

$$ \nabla \cdot F = ? $$

$$ \nabla \times F = ? $$

$$ \phi(x, y) := x^2 - y^2 $$

$$ \nabla^2 \phi = ? $$
";
    let ev = run(src);
    no_errors(&ev);
    let a = answers(&ev);
    assert_eq!(exact(a[0]), "x + y + z");
    assert_eq!(exact(a[1]), "(-y, -z, -x)");
    assert_eq!(exact(a[2]), "0");
}
