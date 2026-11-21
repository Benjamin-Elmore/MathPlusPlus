//! Regression tests for bugs found by running whole documents: each test names the
//! wrong behaviour it pins down.

use std::sync::Arc;

use mpp_eval::{Evaluated, NoImports, Session};

fn run(src: &str) -> Evaluated {
    Session::new()
        .with_importer(Arc::new(NoImports))
        .run_str(src)
        .clone()
}

fn errors(src: &str) -> Vec<String> {
    run(src)
        .errors()
        .iter()
        .map(|d| mpp_core::note::plain(&d.message))
        .collect()
}

/// The exact answer of the first query (no errors allowed).
fn answer(src: &str) -> String {
    let ev = run(src);
    let errs: Vec<String> = ev
        .errors()
        .iter()
        .map(|d| mpp_core::note::plain(&d.message))
        .collect();
    assert!(errs.is_empty(), "{src}: {errs:?}");
    let (_, line) = ev.queries().next().expect("a query");
    line.plain.clone().unwrap_or_default()
}

/// The numeric answer of the first query (no errors allowed).
fn approx(src: &str) -> String {
    let ev = run(src);
    let errs: Vec<String> = ev
        .errors()
        .iter()
        .map(|d| mpp_core::note::plain(&d.message))
        .collect();
    assert!(errs.is_empty(), "{src}: {errs:?}");
    let (_, line) = ev.queries().next().expect("a query");
    line.approx.clone().unwrap_or_default()
}

#[test]
fn heaviside_and_delta_are_one_function_each() {
    // `H` and `\operatorname{heaviside}` mean the same step; at the jump the
    // conventions differ, so H(0) stays as written in both spellings.
    assert_eq!(answer(r"$$ H(0) = ? $$"), "H(0)");
    assert_eq!(answer(r"$$ \operatorname{heaviside}(0) = ? $$"), "H(0)");
    assert_eq!(answer(r"$$ \operatorname{heaviside}(2) = ? $$"), "1");
    assert_eq!(
        answer(r"$$ \operatorname{heaviside}(t - 1) = ? $$"),
        "H(t - 1)"
    );
    // H′ = δ and (δ^{(j)})′ = δ^{(j+1)}.
    assert_eq!(answer(r"$$ \frac{d}{dx} H(x) = ? $$"), "delta(x)");
    assert_eq!(
        answer(r"$$ \frac{d}{dx} H(x^2 - 1) = ? $$"),
        "2*x*delta(x^2 - 1)"
    );
    assert_eq!(answer(r"$$ \frac{d}{dx} \delta(x) = ? $$"), "delta(x, 1)");
}

#[test]
fn percent_after_a_number() {
    assert_eq!(answer(r"$$ 5\% \cdot 200 = ? $$"), "10");
    assert!(errors(r"At $r = 5\%$ a year.").is_empty());
}

#[test]
fn a_function_name_on_its_own_in_prose() {
    assert!(errors(r"Pairing $\delta'$ with $\sin$ gives $-\cos 0$.").is_empty());
    // With nothing after it in a query it is still an error.
    assert!(!errors(r"$$ \sin^2 = ? $$").is_empty());
}

#[test]
fn bold_one_is_the_indicator() {
    assert_eq!(
        answer(r"$$ \mathbf 1[3 > 2] + \mathbf 1[1 > 2] = ? $$"),
        "1"
    );
    assert_eq!(
        approx(r"$$ \int_0^2 \mathbf 1[x \le 1] \, dx \approx ? $$"),
        "1"
    );
}

#[test]
fn hamilton_with_an_undefined_hamiltonian_is_an_error() {
    // H has no value here: Hamilton's equations of a constant would be q̇ = ṗ = 0,
    // which is not what was meant.
    let errs = errors(r"$$ \operatorname{hamilton}(H, q, p) = ? $$");
    assert!(
        errs.iter().any(|e| e.contains("does not depend")),
        "{errs:?}"
    );
    assert_eq!(
        answer(concat!(
            r"$$ H := \tfrac12(p^2 + q^2) $$",
            "\n\n",
            r"$$ \operatorname{hamilton}(H, q, p) = ? $$"
        )),
        "((q, p), (p, -q))"
    );
}

#[test]
fn derivatives_of_special_functions() {
    // Γ′(1) = Γ(1) ψ(1) = −γ (it used to come out as 0).
    assert_eq!(answer(r"$$ \Gamma'(1) = ? $$"), "-γ");
    assert_eq!(approx(r"$$ \Gamma'(1) \approx ? $$"), "-0.5772156649");
    assert_eq!(answer(r"$$ \psi(4) = ? $$"), "-γ + 11/6");
    // ψ′ is the trigamma function: ψ′(1) = π²/6.
    assert_eq!(approx(r"$$ \psi'(1) \approx ? $$"), "1.644934067");
    assert_eq!(
        answer(r"$$ \frac{d}{dx} \operatorname{erf}(x) = ? $$"),
        "2*e^(-x^2)/sqrt(π)"
    );
    assert_eq!(
        answer(r"$$ \frac{d}{dx} \operatorname{Si}(x) = ? $$"),
        "sin(x)/x"
    );
    assert_eq!(
        answer(r"$$ \frac{d}{dx} \operatorname{Ci}(x) = ? $$"),
        "cos(x)/x"
    );
    // J₀′ = −J₁, W′ = W/(x(1 + W)), and the elliptic integrals (DLMF 19.4.1).
    assert_eq!(answer(r"$$ \frac{d}{dx} J_0(x) = ? $$"), "-besselj(1, x)");
    assert_eq!(approx(r"$$ J_0'(2) \approx ? $$"), "-0.5767248078");
    assert_eq!(approx(r"$$ W'(1) \approx ? $$"), "0.3618962566");
    assert_eq!(
        answer(r"$$ \frac{d}{dk} E(k) = ? $$"),
        "(ellipe(k) - ellipk(k))/k"
    );
    assert_eq!(
        answer(r"$$ \frac{d}{dk} K(k) = ? $$"),
        "ellipe(k)/(k*(-k^2 + 1)) - ellipk(k)/k"
    );
}

#[test]
fn a_numeric_query_with_an_unknown_name_shows_no_result() {
    let ev = run(r"$$ \sin\theta \approx ? $$");
    assert_eq!(ev.errors().len(), 1);
    let (_, line) = ev.queries().next().expect("a query");
    assert_eq!(line.plain_answer(), None, "{:?}", line.plain);
    assert!(line.value.is_none());
}

#[test]
fn total_differential_reads_like_a_textbook() {
    let ev = run(concat!(r"$$ f(x, y) := x^2 y $$", "\n\n", r"$$ df = ? $$"));
    assert!(ev.errors().is_empty());
    let (_, line) = ev.queries().next().expect("a query");
    assert_eq!(line.latex.as_deref(), Some(r"2 x y\,dx + x^{2}\,dy"));
}

#[test]
fn line_integrals_of_differential_forms() {
    let (ast, diags) = mpp_syntax::math::parse_math(r"\oint_C (P\,dx + Q\,dy)", 0);
    assert!(diags.is_empty(), "{diags:?}");
    assert_eq!(
        mpp_syntax::math::sexpr::ast(&ast, mpp_syntax::math::sexpr::Mode::Semantic),
        "(oint :lower C (tuple paren P Q) (·d x y))"
    );
    assert_eq!(
        answer(r"$$ \oint_{x^2 + y^2 = 1} (-y\,dx + x\,dy) = ? $$"),
        "2*π"
    );
    // The differentials in either order, and a bare `dx` has the coefficient 1.
    assert_eq!(
        answer(r"$$ \oint_{x^2 + y^2 = 4} (x\,dy - y\,dx) = ? $$"),
        "8*π"
    );
    assert_eq!(answer(r"$$ \oint_{x^2 + y^2 = 1} (dx + x\,dy) = ? $$"), "π");
    // A named curve has no parametrisation to integrate over: said, not guessed.
    let errs = errors(r"$$ \oint_C (P\,dx + Q\,dy) = ? $$");
    assert_eq!(errs.len(), 1, "{errs:?}");
    // Prose about the form is not an error.
    assert!(errors(r"Green: $\oint_C (P\,dx + Q\,dy) = \iint_D (Q_x - P_y)\,dA$.").is_empty());
}

#[test]
fn textbook_forms_of_the_library_operators() {
    // |x|_p is the p-adic absolute value.
    assert_eq!(answer(r"$$ |8|_2 = ? $$"), "1/8");
    let mats = concat!(
        r"$$ A := \begin{pmatrix} 1 & 2 \\ 3 & 4 \end{pmatrix} $$",
        "\n\n",
        r"$$ B := \begin{pmatrix} 0 & 1 \\ 1 & 0 \end{pmatrix} $$",
        "\n\n",
        r"$$ J := \begin{pmatrix} 0 & -1 \\ 1 & 0 \end{pmatrix} $$",
        "\n\n",
    );
    let with = |q: &str| format!("{mats}{q}");
    assert_eq!(answer(&with(r"$$ \operatorname{Tr}(A) = ? $$")), "5");
    assert_eq!(answer(&with(r"$$ \operatorname{tr} A = ? $$")), "5");
    // [A, B] = AB − BA for matrices; for numbers it stays an interval.
    assert_eq!(answer(&with(r"$$ [A, B] = ? $$")), "[[-1, -3], [3, 1]]");
    assert_eq!(answer(r"$$ [1, 2] = ? $$"), "[1, 2]");
    // e^{tJ} is the matrix exponential.
    assert_eq!(
        answer(&with(r"$$ e^{tJ} = ? $$")),
        "[[cos(t), -sin(t)], [sin(t), cos(t)]]"
    );
    assert_eq!(
        answer(r"$$ \ker \begin{pmatrix} 1 & 2 \\ 2 & 4 \end{pmatrix} = ? $$"),
        "([[-2], [1]])"
    );
    // C_n is the Catalan number only when a query is made of nothing else …
    assert_eq!(answer(r"$$ C_5 = ? $$"), "42");
    assert_eq!(answer(r"$$ C_{10} - C_9 = ? $$"), "11934");
    // … and stays a name next to anything else, or when the document defines C.
    assert_eq!(answer(r"$$ C_1 x + C_2 = ? $$"), "C_1*x + C_2");
    assert_eq!(
        answer(concat!(r"$$ C_5 := 3 $$", "\n\n", r"$$ C_5 = ? $$")),
        "3"
    );
}

/// The verdict of the first check, as `mpp run` describes it.
fn verdict(src: &str) -> String {
    let ev = run(src);
    let errs: Vec<String> = ev
        .errors()
        .iter()
        .map(|d| mpp_core::note::plain(&d.message))
        .collect();
    assert!(errs.is_empty(), "{src}: {errs:?}");
    let line = ev
        .blocks
        .iter()
        .flat_map(|b| b.values.iter())
        .find(|l| l.verdict.is_some())
        .expect("a check");
    line.verdict
        .as_ref()
        .map(|v| v.describe())
        .unwrap_or_default()
}

#[test]
fn checks_of_lists_inequalities_and_units() {
    // Lists, matrices and sets, entry by entry.
    assert!(verdict(r"$$ (\sin^2 x + \cos^2 x, 2) \overset{?}{=} (1, 2) $$").starts_with('✓'));
    assert_eq!(
        verdict(r"$$ (1, 2) \overset{?}{=} (1, 3) $$"),
        "✗ (left side 2 in entry 2, right side 3)"
    );
    assert!(verdict(r"$$ (1, 2) \overset{?}{=} (1, 2, 3) $$").starts_with('✗'));
    assert!(verdict(r"$$ \{1, 2\} \overset{?}{=} \{2, 1\} $$").starts_with('✓'));
    assert!(verdict(r"$$ \{1, 2\} \overset{?}{=} \{1, 3\} $$").starts_with('✗'));
    assert!(verdict(
        r"$$ \begin{pmatrix} 1 & x \\ 0 & 1 \end{pmatrix} \overset{?}{=} \begin{pmatrix} 1 & x \\ 0 & 1 \end{pmatrix} $$"
    )
    .starts_with('✓'));
    // Inequalities against true or false: exact for numbers, at random points otherwise.
    assert!(verdict(r"$$ 3 < 2 \overset{?}{=} \text{false} $$").starts_with('✓'));
    assert!(verdict(r"$$ \pi > 3 \overset{?}{=} \text{true} $$").starts_with('✓'));
    assert!(verdict(r"$$ e^x \ge 1 + x \overset{?}{=} \text{true} $$").starts_with('✓'));
    let v = verdict(r"$$ x^2 \ge x \overset{?}{=} \text{true} $$");
    assert!(v.starts_with("✗ (at x = "), "{v}");
    // Values with units, as quantities.
    assert!(verdict(r"$$ 100\,\mathrm{cm} \overset{?}{=} 1\,\mathrm{m} $$").starts_with('✓'));
    assert!(verdict(r"$$ 3600\,\mathrm{s} \overset{?}{=} 1\,\mathrm{h} $$").starts_with('✓'));
    assert_eq!(
        verdict(r"$$ 1\,\mathrm{km} \overset{?}{=} 999\,\mathrm{m} $$"),
        "✗ (left side 1 km, right side 999 m)"
    );
    assert!(verdict(r"$$ 1\,\mathrm{kg} \overset{?}{=} 1\,\mathrm{m} $$").starts_with('✗'));
}

#[test]
fn results_read_as_unicode_text_with_a_stable_plain_form() {
    let ev = run(r"$$ 9\pi = ? $$");
    let (_, line) = ev.queries().next().expect("a query");
    assert_eq!(line.text.as_deref(), Some("9π"));
    assert_eq!(line.plain.as_deref(), Some("9*π"));
    let text = |src: &str| {
        let ev = run(src);
        let (_, line) = ev.queries().next().expect("a query");
        line.text.clone().unwrap_or_default()
    };
    assert_eq!(text(r"$$ \frac{\partial}{\partial x}(x^2 y) = ? $$"), "2xy");
    assert_eq!(
        text(r"$$ \frac{d}{dx} x^3 \sin x = ? $$"),
        "x³ cos(x) + 3x² sin(x)"
    );
    assert_eq!(text(r"$$ \int x e^x \, dx = ? $$"), "x e^x - e^x + C");
    assert_eq!(text(r"$$ \sqrt{8} = ? $$"), "2√2");
    // A · only where two factors would run together.
    assert_eq!(text(r"$$ 2 \cdot 3^x = ? $$"), "2·3^x");
    assert_eq!(text(r"$$ \frac{d}{dx} J_0(x) = ? $$"), "-J₁(x)");
}

/// Every `≈ ?`/`= ?` answer of a document, in order (no errors allowed).
fn approx_all(src: &str) -> Vec<String> {
    let ev = run(src);
    let errs: Vec<String> = ev
        .errors()
        .iter()
        .map(|d| mpp_core::note::plain(&d.message))
        .collect();
    assert!(errs.is_empty(), "{src}: {errs:?}");
    ev.queries()
        .map(|(_, l)| l.plain_answer().unwrap_or_default())
        .collect()
}

#[test]
fn a_definition_can_ask_for_its_value_and_share_a_line() {
    let src = concat!(
        r"Let $v := 20\,\mathrm{m/s}$ and $g \equiv 9.81\,\mathrm{m/s^2}$.",
        "\n\n",
        r"$$ R := \frac{v^2}{g} \approx ? \qquad T := \frac{2v}{g} \approx ? $$",
        "\n\n",
        r"$$ R + 1\,\mathrm m \approx ? $$",
    );
    assert_eq!(
        approx_all(src),
        vec!["≈ 40.77471967 m", "≈ 4.077471967 s", "≈ 41.77471967 m"]
    );
}

#[test]
fn figure_lines_may_describe_their_items_in_words() {
    let src = concat!(
        r"$$ f(x) := x^2 $$",
        "\n\n",
        r"Figure: $f(x)$ and the line $2x$, $x \in [0, 2]$, with the crossing marked. Both curves.",
        "\n\n",
        r"Figure: the first 2000 points, inside and outside the quarter circle.",
    );
    assert!(errors(src).is_empty(), "{:?}", errors(src));
}

#[test]
fn units_written_like_a_lab_report() {
    // `\mathrm K` without braces is the kelvin, and a product of units with a name of
    // its own is written with it.
    let src = concat!(
        r"Let $T := 298.15\,\mathrm K$, $n := 0.04\,\mathrm{mol}$ and $R \equiv 8.314\,\mathrm{J/(mol\,K)}$.",
        "\n\n",
        r"$$ nRT \approx ? $$",
    );
    assert_eq!(approx_all(src), vec!["≈ 99.152764 J"]);
    assert_eq!(
        answer(r"$$ 10\,\mathrm{k\Omega} \cdot 100\,\mathrm{\mu F} = ? $$"),
        "1 s"
    );
    // Digit groups: 10\,000 is ten thousand (it used to read as 10 · 000 = 0).
    assert_eq!(answer(r"$$ 10\,000 + 1 = ? $$"), "10001");
    assert_eq!(answer(r"$$ 3\, x = ?, \quad x = 2 $$"), "6");
    // A special function of a dimensionless quantity, and of an angle.
    let src = concat!(
        r"Let $\theta := 30°$.",
        "\n\n",
        r"$$ K\Big(\sin\frac{\theta}{2}\Big) \approx ? $$",
    );
    assert_eq!(approx_all(src), vec!["≈ 1.598142002"]);
    // A quantity whose value still has a name in it is not a number.
    let errs = errors(concat!(
        r"Let $R \equiv 8.314\,\mathrm{J/(mol\,K)}$.",
        "\n\n",
        r"$$ \frac{a}{R} \approx ? $$"
    ));
    assert!(errs.iter().any(|e| e.contains("`a`")), "{errs:?}");
}

#[test]
fn a_differential_equation_statement_defines_its_solution_for_later_use() {
    let src = concat!(
        r"Let $V_0 := 5\,\mathrm V$, $R := 10\,\mathrm{k\Omega}$ and $C := 100\,\mathrm{\mu F}$.",
        "\n\n",
        r"$$ RC\,\frac{dV_C}{dt} + V_C = V_0, \quad V_C(0) = 0 $$",
        "\n\n",
        r"$$ V_C(RC) \approx ? $$",
        "\n\n",
        r"$$ \operatorname{solve}_t \big(V_C(t) = 0.99\, V_0\big) \approx ? $$",
    );
    assert_eq!(
        approx_all(src),
        vec!["≈ 3.160602794 V", "≈ {4.605170186 s}"]
    );
    // Starting values may be names.
    assert_eq!(
        answer(r"$$ \operatorname{dsolve}(y' = -y, (0, a)) = ? $$"),
        "y(x) = e^(-x)*a"
    );
    // A name met only inside differential equations stays the unknown of each.
    let src = concat!(
        r"$$ y' = 2y, \quad y(0) = 1 $$",
        "\n\n",
        r"$$ y' = 3y = ?, \quad y(0) = 2 $$",
    );
    assert_eq!(approx_all(src), vec!["= y(x) = 2*e^(3*x)"]);
}

#[test]
fn conditions_in_solve_filter_the_solutions() {
    // x > 0 used to be ignored, so −2 came back too.
    assert_eq!(
        answer(r"$$ \operatorname{solve}_x (x^2 = 4, x > 0) = ? $$"),
        "{2}"
    );
    assert_eq!(
        answer(r"$$ \operatorname{solve}(x^2 = 4, x > 0) = ? $$"),
        "{2}"
    );
    assert_eq!(
        answer(r"$$ \operatorname{solve}_{x, y} (x y = 0, x + y = 1, x > 0) = ? $$"),
        "{(x = 1, y = 0)}"
    );
}

#[test]
fn one_equation_of_a_coupled_system_is_not_solved_alone() {
    // With y an unknown function too, solving ẋ = x − xy for x alone (y as a
    // constant) gave a wrong x(t); now nothing is defined from it.
    let src = concat!(
        r"$$ \dot x = x - x y, \quad \dot y = x y - y, \quad x(0) = 2, \ y(0) = 1 $$",
        "\n\n",
        r"$$ x(1) \approx ? $$",
    );
    let errs = errors(src);
    assert!(errs.iter().any(|e| e.contains("`x`")), "{errs:?}");
    // The unknowns of the document's equations are functions where they are called
    // at the variable: the chain rule keeps their derivatives.
    let src = concat!(
        r"$$ \dot x = x - x y, \quad \dot y = x y - y $$",
        "\n\n",
        r"$$ V(x, y) := x + \ln y $$",
        "\n\n",
        r"$$ \frac{d}{dt} V\big(x(t), y(t)\big) = ? $$",
    );
    let ev = run(src);
    let line = ev.queries().last().and_then(|(_, l)| l.plain.clone());
    assert_eq!(line.as_deref(), Some("d/dt(y(t))/y(t) + d/dt(x(t))"));
    // A product in brackets stays a product.
    let src = concat!(
        r"$$ \dot x = -x $$",
        "\n\n",
        r"$$ x (1 - x) = ?, \quad x = 3 $$",
    );
    assert_eq!(approx_all(src), vec!["= -6"]);
}

#[test]
fn names_with_fraction_and_list_subscripts_and_years() {
    let src = concat!(
        r"Let $t_{1/2} \equiv 5730\,\mathrm{yr}$.",
        "\n\n",
        r"$$ \lambda := \frac{\ln 2}{t_{1/2}} \approx ? $$",
        "\n\n",
        r"$$ \frac{dN}{dt} = -\lambda N, \quad N(0) = N_0 $$",
        "\n\n",
        r"$$ \frac{N(10\,000\,\mathrm{yr})}{N_0} \approx ? $$",
        "\n\n",
        r"$$ \operatorname{solve}_t \Big(\frac{N(t)}{N_0} = 0.40\Big) \approx ? $$",
    );
    assert_eq!(
        approx_all(src),
        vec![
            "≈ 0.0001209680943 1/yr",
            "≈ 0.2982924364",
            "≈ {7574.647984 yr}"
        ]
    );
}

#[test]
fn fit_sentences_against_a_table() {
    // A linear model with units in the header written as maths: a in kPa·L = J.
    let src = concat!(
        "| $V$ ($\\mathrm L$) | $P$ ($\\mathrm{kPa}$) |\n",
        "|---|---|\n",
        "| 1.0 | 99.3 |\n",
        "| 1.5 | 66.0 |\n",
        "| 2.0 | 49.7 |\n",
        "| 2.5 | 39.6 |\n",
        "| 3.0 | 33.1 |\n",
        "\n",
        r"Fit $P = a \cdot \dfrac{1}{V} + b$ to the table:",
        "\n\n",
        r"$$ a \approx ? \qquad b \approx ? $$",
    );
    assert_eq!(
        approx_all(src),
        vec!["≈ 99.30094044 J", "≈ -0.05454545489 kPa"]
    );
    let ev = run(src);
    assert!(ev.report().contains("R² ≈ 0.999985"), "{}", ev.report());
    // A nonlinear model with a known value in it.
    let src = concat!(
        r"Let $V_0 := 5\,\mathrm V$.",
        "\n\n",
        "| $t$ ($\\mathrm s$) | $V$ ($\\mathrm V$) |\n",
        "|---|---|\n",
        "| 0.0 | 0.00 |\n",
        "| 0.5 | 1.98 |\n",
        "| 1.0 | 3.15 |\n",
        "| 2.0 | 4.32 |\n",
        "| 4.0 | 4.91 |\n",
        "\n",
        r"Fit $V = V_0(1 - e^{-t/\tau})$ to the table:",
        "\n\n",
        r"$$ \tau \approx ? $$",
    );
    let got = approx_all(src);
    assert!(
        got[0].starts_with("≈ 1.00") && got[0].ends_with(" s"),
        "{got:?}"
    );
    // Without a table there is nothing to fit.
    let errs = errors(r"Fit $y = a x$ to the table.");
    assert!(errs.iter().any(|e| e.contains("data columns")), "{errs:?}");
}

#[test]
fn definitions_after_a_comma_and_numbers_that_are_not_numbers() {
    let src = concat!(
        r"$$ n_a := 2 \cdot 3, \qquad n_b(v) := 4 v $$",
        "\n\n",
        r"$$ n_a + n_b(1) = ? $$",
    );
    assert_eq!(approx_all(src), vec!["= 10"]);
    // A piecewise value whose case cannot be decided is not a number, so `≈ ?`
    // reports it instead of showing the piecewise expression as the answer.
    let errs = errors(concat!(
        r"$$ f(v) := \begin{cases} 1 & v < w \ 2 & v \ge w \end{cases} $$",
        "\n\n",
        r"$$ f(3\,\mathrm{mL}) \approx ? $$"
    ));
    assert!(!errs.is_empty());
}

#[test]
fn sums_over_an_indexed_family_keep_each_member() {
    // `a_i` used to be one name, so Σ_{i=1}^3 a_i came back as 3 a_i; the members
    // print as subscripts, never as calls a(1).
    assert_eq!(answer(r"$$ \sum_{i=1}^{3} a_i = ? $$"), "a_1 + a_2 + a_3");
    assert_eq!(
        answer(r"$$ \sum_{j=1}^{2} x_j y_j = ? $$"),
        "x_1*y_1 + x_2*y_2"
    );
    // A bare sum takes its index from the subscript, over i = 1, …, n.
    let src = r"Let $A := \sum a_i^2$ and $C := \sum a_i b_i$.

$$ A = ? $$";
    assert!(errors(src).is_empty(), "{:?}", errors(src));
    assert!(answer(src).contains("a_i^2"), "{}", answer(src));
    // A defined member is still that name.
    let src = r"Let $a_i := 2$.

$$ \sum_{i=1}^{3} a_i = ? $$";
    assert_eq!(answer(src), "6");
}

#[test]
fn sums_and_products_written_with_an_ellipsis() {
    assert_eq!(answer(r"$$ 1 + 2 + \dots + n = ? $$"), "n*(n + 1)/2");
    assert_eq!(answer(r"$$ n + (n-1) + \dots + 1 = ? $$"), "n*(n + 1)/2");
    assert_eq!(answer(r"$$ 1 + 3 + 5 + \dots + 9 = ? $$"), "25");
    assert_eq!(answer(r"$$ 1 \cdot 2 \cdots 5 = ? $$"), "120");
    assert!(answer(r"$$ p_1 p_2 \cdots p_k = ? $$").contains("p_j"));
    // A decimal expansion with symbolic digits is prose, not a syntax error.
    let src = r"Write $f(n) = 0.d_{n1} d_{n2} d_{n3} \dots$ and $x = 0.e_1 e_2 \dots$ here.";
    assert!(errors(src).is_empty(), "{:?}", errors(src));
    // A last term the progression never reaches is not guessed at.
    assert!(!errors(r"$$ 1 + 4 + \dots + 99 = ? $$").is_empty());
}

#[test]
fn numerical_solutions_define_functions() {
    let src = r"Let $\alpha := 1.1$, $\beta := 0.4$, $\delta := 0.1$ and $\gamma := 0.4$.

$$ \dot x = \alpha x - \beta x y, \quad \dot y = \delta x y - \gamma y, \quad x(0) = 10, \ y(0) = 5 $$

$$ V(x, y) := \delta x - \gamma \ln x + \beta y - \alpha \ln y $$
$$ \frac{d}{dt} V\big(x(t), y(t)\big) = ? $$
$$ \max_{t \in [0, 50]} \frac{|V(t) - V(0)|}{|V(0)|} \approx ? $$
$$ x(0) \approx ? $$";
    let ev = run(src);
    let errs: Vec<String> = ev
        .errors()
        .iter()
        .map(|d| mpp_core::note::plain(&d.message))
        .collect();
    assert!(errs.is_empty(), "{errs:?}");
    let shown: Vec<(Option<String>, Option<String>)> = ev
        .queries()
        .map(|(_, l)| (l.plain.clone(), l.approx.clone()))
        .collect();
    // The chain rule on the numerical solution: V is conserved exactly.
    assert_eq!(shown[0].0.as_deref(), Some("0"));
    let drift: f64 = shown[1]
        .1
        .as_deref()
        .unwrap_or("")
        .replace(" × 10⁻", "e-")
        .replace(['¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹', '⁰'], "")
        .split('e')
        .next()
        .and_then(|m| m.parse().ok())
        .unwrap_or(f64::NAN);
    assert!(drift.is_finite(), "{shown:?}");
    assert_eq!(shown[2].1.as_deref(), Some("10"));
    // The harmonic oscillator through the numerical path agrees with cos t.
    let src = r"$$ \ddot\theta + \sin\theta = 0, \quad \theta(0) = 0.1, \ \dot\theta(0) = 0 $$

$$ \theta(1) \approx ? $$";
    let v: f64 = approx(src).parse().unwrap_or(f64::NAN);
    // For a small swing, θ ≈ 0.1 cos t (to the size of the nonlinearity).
    assert!((v - 0.1 * 1f64.cos()).abs() < 1e-3, "{v}");
}

#[test]
fn maxima_and_minima_over_an_interval() {
    assert_eq!(approx(r"$$ \max_{x \in [0, 2]} x(2 - x) \approx ? $$"), "1");
    assert_eq!(
        approx(r"$$ \min_{x \in [0, 4]} (x - 1)^2 + 3 \approx ? $$"),
        "3"
    );
    assert_eq!(
        approx(r"$$ \max_{t \in [0, 5]} |\sin t - 0.5| \approx ? $$"),
        "1.5"
    );
}

#[test]
fn heat_and_wave_equations_written_as_statements() {
    // One sine mode decays on its own, with units carried through.
    let src = r"Let $L := 1\,\mathrm m$ and $\alpha := 1.0 \times 10^{-4}\,\mathrm{m^2/s}$.

$$ \frac{\partial u}{\partial t} = \alpha \frac{\partial^2 u}{\partial x^2}, \quad u(0, t) = u(L, t) = 0, \quad u(x, 0) = 100\,\mathrm K \cdot \sin\frac{\pi x}{L} $$

$$ \frac{\partial u}{\partial t} - \alpha \frac{\partial^2 u}{\partial x^2} \overset{?}{=} 0 $$
$$ \operatorname{solve}_t \Big(u\big(\tfrac L2, t\big) = 50\,\mathrm K\Big) \approx ? $$";
    let ev = run(src);
    let errs: Vec<String> = ev
        .errors()
        .iter()
        .map(|d| mpp_core::note::plain(&d.message))
        .collect();
    assert!(errs.is_empty(), "{errs:?}");
    let lines: Vec<_> = ev.queries().map(|(_, l)| l.clone()).collect();
    assert_eq!(
        lines[0].verdict.as_ref().and_then(|v| v.passed()),
        Some(true)
    );
    // ln 2 / (α π² / L²) = 702.3049 s.
    assert_eq!(lines[1].approx.as_deref(), Some("{702.3049277 s}"));
    // Two modes and the wave equation, without units.
    let src = r"$$ \frac{\partial^2 u}{\partial t^2} = 4 \frac{\partial^2 u}{\partial x^2}, \quad u(0, t) = 0, \ u(\pi, t) = 0, \quad u(x, 0) = \sin x + 3 \sin 2x $$

$$ u(1, 2) \approx ? $$";
    let ev = run(src);
    let want = (1f64).sin() * (4f64).cos() + 3.0 * (2f64).sin() * (8f64).cos();
    let got: Option<f64> = ev
        .queries()
        .filter_map(|(_, l)| l.approx.clone())
        .next()
        .and_then(|a| a.parse().ok());
    assert!(
        got.is_some_and(|g| (g - want).abs() < 1e-9),
        "{got:?} vs {want}"
    );
    // The Fourier series for data that are not whole modes (no units): u(x, t) is a
    // series, and data with sines that are not modes are refused, never guessed.
    let src = r"$$ \frac{\partial u}{\partial t} = \frac{\partial^2 u}{\partial x^2}, \quad u(0, t) = u(1, t) = 0, \quad u(x, 0) = \sin(\pi x / 2) $$

$$ u(x, t) = ? $$";
    let ev = run(src);
    assert!(ev
        .all_diagnostics()
        .iter()
        .any(|d| d.message.contains("u is not defined")));
}

#[test]
fn decimals_are_shown_as_typed() {
    let src = r"Let $g \equiv 9.81\,\mathrm{m/s^2}$ and $k := 0.1$.

$$ g = ? \qquad k = ? \qquad 2k = ? $$";
    let ev = run(src);
    let defs: Vec<Option<String>> = ev
        .blocks
        .iter()
        .flat_map(|b| b.values.iter())
        .filter(|l| matches!(l.kind, mpp_eval::LineKind::Definition { .. }))
        .map(|l| l.text.clone())
        .collect();
    assert_eq!(defs, vec![Some("9.81 m/s²".into()), Some("0.1".into())]);
    let shown: Vec<(Option<String>, Option<String>)> = ev
        .queries()
        .map(|(_, l)| (l.text.clone(), l.latex.clone()))
        .collect();
    assert_eq!(shown[0].0.as_deref(), Some("9.81 m/s²"));
    assert!(shown[0]
        .1
        .as_deref()
        .is_some_and(|l| l.starts_with(r"9.81\,")));
    assert_eq!(shown[1].0.as_deref(), Some("0.1"));
    // A value computed from a decimal is shown as a decimal too.
    assert_eq!(shown[2].0.as_deref(), Some("0.2"));
}

#[test]
fn an_accented_pi_is_a_name() {
    // `\hat\pi` is an estimate, never the constant π with an accent.
    let src = r"$$ \hat\pi := 3.1 $$

$$ \hat\pi - \pi \approx ? $$";
    let v: f64 = approx(src).parse().unwrap_or(f64::NAN);
    assert!((v - (3.1 - std::f64::consts::PI)).abs() < 1e-9, "{v}");
}

/// `velocity := 3` used to be an error ("expected a name…") and `velocity · 2` the
/// product of eight letters. Letters side by side on the left of `:=` define that
/// name, and the same letters later mean it.
#[test]
fn a_spelled_out_name_can_be_defined_and_used() {
    assert_eq!(
        answer("$$ velocity := 3 $$\n\n$$ velocity \\cdot 2 = ? $$\n"),
        "6"
    );
    assert_eq!(answer("$$ speed := 2 $$\n\n$$ 3 speed^2 = ? $$\n"), "12");
    // Unknown letters still multiply.
    assert_eq!(
        answer("$$ x := 2 $$\n\n$$ y := 5 $$\n\n$$ xy = ? $$\n"),
        "10"
    );
}

/// A decimal the user typed gives decimal answers: `g + 1` after `g := 9.81` is
/// 10.81, not 1081/100, and so is a name worked out from it. `%exact` asks for the
/// fraction again; exact input stays exact.
#[test]
fn decimals_in_give_decimals_out() {
    assert_eq!(answer("$$ g := 9.81 $$\n\n$$ g + 1 = ? $$\n"), "10.81");
    assert_eq!(answer("$$ 9.81 + 1 = ? $$\n"), "10.81");
    assert_eq!(
        answer("$$ g := 9.81 $$\n\n$$ h := 2 g $$\n\n$$ h + 1 = ? $$\n"),
        "20.62"
    );
    assert_eq!(
        answer("%exact\n\n$$ g := 9.81 $$\n\n$$ g + 1 = ? $$\n"),
        "1081/100"
    );
    assert_eq!(answer("$$ 1/3 + 1 = ? $$\n"), "4/3");
}

/// The text of the first query line (no errors allowed).
fn shown(src: &str) -> String {
    let ev = run(src);
    let errs: Vec<String> = ev
        .errors()
        .iter()
        .map(|d| mpp_core::note::plain(&d.message))
        .collect();
    assert!(errs.is_empty(), "{src}: {errs:?}");
    let (_, line) = ev.queries().next().expect("a query");
    line.text.clone().unwrap_or_default()
}

const FALLING: &str = r"Let $h_0 = 45\,\mathrm{m}$ and $g = 9.81\,\mathrm{m/s^2}$.

";

#[test]
fn a_solve_sentence_with_units_gives_quantities() {
    // It used to give `10·sqrt(1090)·sqrt(m^2/s^2)/109 s^2/m`.
    let src = format!(
        "{FALLING}{}",
        r"Solve $h_0 - \frac{1}{2} g t^2 = 0$ for $t$."
    );
    assert_eq!(
        shown(&src),
        "t = 10√1090/109 s ≈ 3.028912664 s or t = -10√1090/109 s ≈ -3.028912664 s"
    );
    // `%sf` applies to the decimal value of each root.
    let text = shown(&format!("%sf 3\n\n{src}"));
    assert!(text.contains("≈ 3.03 s"), "{text}");
    assert!(!text.contains("m^2"), "{text}");
}

#[test]
fn a_root_of_a_unit_with_whole_powers_is_simplified() {
    // `√(m·m/s²)` used to read `J^(1/2)/kg^(1/2)`.
    let src = format!("{FALLING}{}", r"$$ (h_0 g)^{1/2} \approx ? $$");
    assert_eq!(approx(&src), "21.01071155 m/s");
}

#[test]
fn significant_figures_leave_symbolic_coefficients_alone() {
    // `%sf 4` used to print `50.00·e^(3.000·t/10)`.
    let src = r"%sf 4

$$ \operatorname{dsolve}(P' = 0.3 P, P, t, P(0) = 50) = ? $$

$$ \frac{\ln 2}{0.3} \approx ? $$
";
    let ev = run(src);
    let lines: Vec<_> = ev.queries().map(|(_, l)| l.clone()).collect();
    let text = lines[0].text.clone().unwrap_or_default();
    assert!(!text.contains("50.00") && !text.contains("3.000"), "{text}");
    assert_eq!(lines[1].approx.as_deref(), Some("2.310"));
}

#[test]
fn a_micro_sign_before_an_upright_unit_is_the_prefix() {
    // `100\,\mu\mathrm{F}` used to read as 100·μ·F and print `100Fμ`.
    assert_eq!(shown(r"$$ 100\,\mu\mathrm{F} = ? $$"), "100 µF");
    let src = r"$$ R := 4.7\,\mathrm{k\Omega} $$

$$ C := 100\,\mu\mathrm{F} $$

$$ R C = ? $$
";
    assert_eq!(answer(src), "0.47 s");
}

#[test]
fn a_change_in_a_quantity_is_typeset_as_delta_x() {
    // The LaTeX of the name `Δx` used to be `\mathrm{\Delta x}` (upright x).
    let ev = run(r"$$ 2 \Delta x + \Delta x = ? $$");
    let (_, line) = ev.queries().next().expect("a query");
    assert_eq!(line.latex.as_deref(), Some("3 \\Delta x"));
    assert_eq!(mpp_core::Expr::sym("Δx").to_latex(), "\\Delta x");
}

/// `∫ 1/(x⁴+1) dx` was not found: x⁴ + 1 factors over Q(√2) into two quadratics, giving
/// logarithms and arctangents (checked by differentiating back).
#[test]
fn quartic_denominator_split_over_a_quadratic_field() {
    let got = answer(r"$$ \int \frac{1}{x^4 + 1} \, dx = ? $$");
    assert!(got.contains("atan") && got.contains("ln"), "{got}");
    // F(1) − F(0) against ∫₀¹ dx/(x⁴+1) = 0.86697298733991… (mpmath)
    let v = approx(r"$$ \int_0^1 \frac{1}{x^4 + 1} \, dx \approx ? $$");
    assert_eq!(v, "0.8669729873");
    let got = answer(r"$$ \int \frac{x^2}{x^4 + 1} \, dx = ? $$");
    assert!(got.contains("atan"), "{got}");
}

/// Convergent improper integrals were left undecided: limit comparison decides them and
/// the value follows.
#[test]
fn convergent_improper_integrals_get_values() {
    let rows = [
        (r"$$ \int_0^2 (x-1)^{-2/3} \, dx \approx ? $$", "6"),
        (
            r"$$ \int_0^{\infty} \frac{\sin^2 x}{x^2} \, dx \approx ? $$",
            "1.570796327",
        ),
        (r"$$ \int_0^1 x^{-0.99} \, dx \approx ? $$", "100"),
    ];
    for (src, want) in rows {
        assert_eq!(approx(src), want, "{src}");
    }
}

/// Boundary value problems: exact for constant coefficients, including the cases with no
/// solution and infinitely many; shooting otherwise.
#[test]
fn boundary_value_problems() {
    let many = answer(r"$$ \operatorname{dsolve}(y'' + y = 0, y(0) = 0, y(\pi) = 0) = ? $$");
    assert!(many.contains("sin(x)") && many.contains("C"), "{many}");
    let errs = errors(r"$$ \operatorname{dsolve}(y'' + y = 0, y(0) = 0, y(\pi) = 1) = ? $$");
    assert!(errs.iter().any(|e| e.contains("no solution")), "{errs:?}");
    assert_eq!(
        answer(r"$$ \operatorname{dsolve}(y'' = 6x, y(0) = 0, y(1) = 2) = ? $$"),
        "y(x) = x^3 + x"
    );
    // y″ + x y = 0, y(0) = 0, y(1) = 1 has no closed form here: shooting meets y(1) = 1.
    let ev = run(r"$$ \operatorname{dsolve}(y'' + x y = 0, y(0) = 0, y(1) = 1) = ? $$");
    let (_, line) = ev.queries().next().expect("a query");
    let shown = line
        .plain
        .clone()
        .or(line.approx.clone())
        .unwrap_or_default();
    assert!(shown.contains("y(1)") && shown.contains('1'), "{shown}");
    let notes: Vec<String> = ev
        .all_diagnostics()
        .iter()
        .map(|d| mpp_core::note::plain(&d.message))
        .collect();
    assert!(notes.iter().any(|n| n.contains("shooting")), "{notes:?}");
}

/// A false congruence says which remainders differ, not "left side 17 ≡ 2".
#[test]
fn a_false_congruence_gives_the_two_remainders() {
    let ev = run(r"$$ 17 \equiv 3 \pmod{5} = ? $$");
    let (_, line) = ev.queries().next().expect("a query");
    let text = line.plain.clone().unwrap_or_default();
    assert!(
        text.contains("false") && text.contains("17 mod 5 = 2, but 3 mod 5 = 3"),
        "{text}"
    );
}

/// Relations answer true ✓ / false ✗ with or without units.
#[test]
fn relation_answers_carry_a_mark_with_units_too() {
    for (src, want) in [
        (r"$$ 1 < 2 = ? $$", "true ✓"),
        (r"$$ 3\,\mathrm{m} < 5\,\mathrm{m} = ? $$", "true ✓"),
        (r"$$ 3\,\mathrm{m} > 5\,\mathrm{m} = ? $$", "false ✗"),
    ] {
        let ev = run(src);
        let (_, line) = ev.queries().next().expect("a query");
        let text = line.plain_answer().unwrap_or_default();
        assert!(text.contains(want), "{src}: {text}");
    }
}

/// An operator with a script that Math++ does not know is warned about and has no
/// answer, instead of echoing its letters as a product.
#[test]
fn an_unknown_scripted_operator_is_warned_about() {
    let ev = run(r"$$ \operatorname{fit}_{linear}(\{(1,2),(2,4.1)\}) = ? $$");
    let notes: Vec<String> = ev
        .all_diagnostics()
        .iter()
        .map(|d| d.message.clone())
        .collect();
    assert!(
        notes.iter().any(|n| n.contains("`fit` is not a function")),
        "{notes:?}"
    );
    let (_, line) = ev.queries().next().expect("a query");
    assert!(line
        .plain_answer()
        .is_none_or(|a| !a.contains("operatorname")));
}

/// A symbolic proof that cancelled a factor which can be zero says where it holds.
#[test]
fn proofs_that_cancel_a_factor_name_the_points_left_out() {
    for (src, note) in [
        (r"$$ \frac{1}{\frac{1}{x}} \overset{?}{=} x $$", "for x ≠ 0"),
        (
            r"$$ \frac{x^2 - 1}{x - 1} \overset{?}{=} x + 1 $$",
            "for x ≠ 1",
        ),
        (
            r"$$ \operatorname{sign}(x) \overset{?}{=} \frac{x}{|x|} $$",
            "for x ≠ 0",
        ),
        (
            r"$$ \frac{d}{dx} |x| \overset{?}{=} \operatorname{sign}(x) $$",
            "for x ≠ 0",
        ),
    ] {
        let ev = run(src);
        let line = ev.blocks[0].values.first().expect("a check");
        let text = line.verdict_text().unwrap_or_default();
        assert!(
            text.starts_with('✓') && text.contains(note),
            "{src}: {text}"
        );
    }
    // Where both sides are undefined at the same points, or the proof cancels nothing,
    // there is nothing to add.
    for src in [
        r"$$ \frac{1}{x} + \frac{1}{x} \overset{?}{=} \frac{2}{x} $$",
        r"$$ \sin^2 x + \cos^2 x \overset{?}{=} 1 $$",
    ] {
        let ev = run(src);
        let line = ev.blocks[0].values.first().expect("a check");
        assert_eq!(
            line.verdict_text().as_deref(),
            Some("✓ (proved symbolically)")
        );
    }
}

/// A binomial identity is tried on the usual domain, not at negative whole numbers
/// where the extended definition breaks the symmetry; a false one still fails.
#[test]
fn binomial_identities_are_checked_on_the_usual_domain() {
    let ev = run(r"$$ \binom{n}{k} \overset{?}{=} \binom{n}{n-k} $$");
    let line = ev.blocks[0].values.first().expect("a check");
    assert_eq!(line.verdict.as_ref().and_then(|v| v.passed()), Some(true));
    let ev = run(r"$$ \binom{n}{k} \overset{?}{=} \binom{n}{k+1} $$");
    let line = ev.blocks[0].values.first().expect("a check");
    assert_eq!(line.verdict.as_ref().and_then(|v| v.passed()), Some(false));
}

/// A finite-range integral that will not settle was not computed reliably; its
/// convergence is not in question.
#[test]
fn a_refused_finite_integral_is_not_called_a_convergence_question() {
    for src in [
        r"$$ \int_0^{100} \sin(x^2) \, dx = ? $$",
        r"$$ \int_0^1 \sin(1/x) \, dx = ? $$",
    ] {
        let errs = errors(src);
        assert!(
            errs.iter()
                .any(|e| e.contains("reliably") && !e.contains("converges")),
            "{src}: {errs:?}"
        );
    }
}
