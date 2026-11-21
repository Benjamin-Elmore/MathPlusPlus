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
        .map(|d| d.message.clone())
        .collect()
}

/// The exact answer of the first query (no errors allowed).
fn answer(src: &str) -> String {
    let ev = run(src);
    let errs: Vec<String> = ev.errors().iter().map(|d| d.message.clone()).collect();
    assert!(errs.is_empty(), "{src}: {errs:?}");
    let (_, line) = ev.queries().next().expect("a query");
    line.text.clone().unwrap_or_default()
}

/// The numeric answer of the first query (no errors allowed).
fn approx(src: &str) -> String {
    let ev = run(src);
    let errs: Vec<String> = ev.errors().iter().map(|d| d.message.clone()).collect();
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
    assert_eq!(line.answer(), None, "{:?}", line.text);
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
