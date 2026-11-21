//! Random variables in documents: `X \sim D` declarations, exact `E`, `Var` and `P`
//! from the distribution, and seeded Monte Carlo sums over i.i.d. families.

use std::sync::Arc;

use mpp_eval::{Evaluated, NoImports, Session};

fn run(src: &str) -> Evaluated {
    Session::new()
        .with_importer(Arc::new(NoImports))
        .run_str(src)
        .clone()
}

fn errors(ev: &Evaluated) -> Vec<String> {
    ev.errors().iter().map(|d| d.message.clone()).collect()
}

fn shown(ev: &Evaluated) -> Vec<String> {
    ev.queries()
        .map(|(_, l)| {
            l.plain
                .clone()
                .or_else(|| l.approx.clone())
                .unwrap_or_else(|| "<none>".into())
        })
        .collect()
}

fn answers(src: &str) -> Vec<String> {
    let ev = run(src);
    let errs = errors(&ev);
    assert!(errs.is_empty(), "{src}: unexpected errors {errs:?}");
    shown(&ev)
}

#[test]
fn exact_answers_from_each_distribution() {
    let src = r"$$ X \sim \mathrm{Uniform}[0, 1] $$
$$ B \sim \mathrm{Binomial}(10, 1/4) $$
$$ K \sim \mathrm{Poisson}(3) $$
$$ Z \sim \mathcal{N}(0, 1) $$
$$ T \sim \mathrm{Exponential}(2) $$
$$ C \sim \mathrm{Bernoulli}(1/3) $$
$$ W \sim \mathrm{Normal}(2, 9) $$
$$ \mathbb{E}[X] = ? $$
$$ E[X^2] = ? $$
$$ \operatorname{Var}(X) = ? $$
$$ \operatorname{Var}(B) = ? $$
$$ \mathbb{E}[3B + 2K] = ? $$
$$ P(X \le 1/4) = ? $$
$$ P(B = 2) = ? $$
$$ \mathbb{P}(T > 1) = ? $$
$$ P(Z \le 0) = ? $$
$$ \operatorname{Var}(C) = ? $$
$$ \mathbb{E}[W^2] = ? $$
$$ \mathbb{E}[X K] = ? $$
$$ \Pr(K = 0) \approx ? $$
";
    let got = answers(src);
    assert_eq!(
        got,
        [
            "1/2",
            "1/3",
            "1/12",
            "15/8",
            "27/2",
            "1/4",
            "295245/1048576",
            "1/e^2",
            "1/2",
            "2/9",
            "13",
            "3/2",
            "1/e^3"
        ],
        "{got:?}"
    );
}

#[test]
fn symbolic_parameters() {
    let got = answers(
        r"$$ X \sim \mathrm{Bernoulli}(p) $$
$$ \mathbb{E}[X] = ? $$
$$ \operatorname{Var}(X) = ? $$",
    );
    assert_eq!(got[0], "p");
    assert!(got[1].contains('p'), "{got:?}");
}

#[test]
fn bad_parameters_are_errors() {
    for src in [
        r"$$ X \sim \mathrm{Uniform}[1, 0] $$",
        r"$$ X \sim \mathrm{Bernoulli}(2) $$",
        r"$$ X \sim \mathcal{N}(0, -1) $$",
        r"$$ X \sim \mathrm{Poisson}(1, 2) $$",
    ] {
        assert!(!errors(&run(src)).is_empty(), "{src}");
    }
}

#[test]
fn several_variables_are_refused_not_estimated() {
    let ev = run(r"$$ X, Y \sim \mathrm{Uniform}[0, 1] $$
$$ P(X^2 + Y^2 \le 1) = ? $$");
    assert_eq!(errors(&ev).len(), 1, "{:?}", errors(&ev));
    assert_eq!(shown(&ev), ["<none>"]);
}

const PI: &str = r"%seed 42

Let $N := 20000$.
$$ X_j, Y_j \sim \mathrm{Uniform}[0, 1] $$
$$ \hat\pi := \frac{4}{N} \sum_{j=1}^{N} \mathbf 1\big[X_j^2 + Y_j^2 \le 1\big] \approx ? $$
";

fn estimate(src: &str) -> f64 {
    let ev = run(src);
    assert!(errors(&ev).is_empty(), "{:?}", errors(&ev));
    let s = shown(&ev);
    s[0].parse().unwrap_or_else(|_| panic!("{s:?}"))
}

#[test]
fn monte_carlo_sums_are_seeded_and_reproducible() {
    let a = estimate(PI);
    let b = estimate(PI);
    assert_eq!(a, b);
    // σ = 4 √(p(1 − p)/N) with p = π/4.
    let p = std::f64::consts::FRAC_PI_4;
    let sigma = 4.0 * (p * (1.0 - p) / 20000.0).sqrt();
    assert!((a - std::f64::consts::PI).abs() < 4.0 * sigma, "{a}");
    let c = estimate(&PI.replace("%seed 42", "%seed 7"));
    assert_ne!(a, c);
}

#[test]
fn empirical_and_exact_means_agree() {
    let src = r"%seed 1
$$ D_k \sim \mathrm{Poisson}(4) $$
$$ \frac{1}{5000} \sum_{k=1}^{5000} D_k \approx ? $$
$$ \mathbb{E}[D_k] = ? $$
";
    let got = answers(src);
    let m: f64 = got[0].parse().unwrap();
    assert!((m - 4.0).abs() < 5.0 * (4.0f64 / 5000.0).sqrt(), "{m}");
    assert_eq!(got[1], "4");
}

#[test]
fn without_a_seed_the_line_says_so() {
    let ev = run(r"$$ X_j \sim \mathrm{Uniform}[0, 1] $$
$$ \sum_{j=1}^{10} X_j \approx ? $$");
    assert!(errors(&ev).is_empty());
    assert!(ev
        .all_diagnostics()
        .iter()
        .any(|d| d.message.contains("seed is 0")));
}
