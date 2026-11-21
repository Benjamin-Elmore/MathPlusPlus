//! Transforms and helpers are typeset in textbook notation, never as their operator
//! names (`laplace`, `taylorbound`, `crit`).

use mpp_render::{layout_latex, LayoutOptions};

fn text(src: &str) -> String {
    layout_latex(src, &LayoutOptions::display(20.0)).text()
}

#[test]
fn transforms_are_script_letters_with_braces() {
    for (src, head) in [
        (r"\mathcal{L}_{t \to s}\left\{ t^2 \right\}", "ℒ"),
        (r"\mathcal{L}^{-1}\{\frac{1}{s}\}", "ℒ−1"),
        (r"\mathcal{F}\{e^{-x^2}\}", "ℱ"),
        (r"\mathcal{F}^{-1}_{\omega \to x}\left\{ 1 \right\}", "ℱ"),
        (r"\operatorname{laplace}(t^2)", "ℒ"),
        (r"\operatorname{invfourier}(1)", "ℱ−1"),
    ] {
        let t = text(src);
        assert!(t.starts_with(head), "{src}: {t}");
        assert!(t.contains('{') && t.contains('}'), "{src}: {t}");
        for word in ["laplace", "fourier", "inv"] {
            assert!(!t.contains(word), "{src}: {t}");
        }
    }
}

#[test]
fn helpers_read_as_words() {
    for (src, words) in [
        (r"\operatorname{taylorbound}(f, 0, 5, 1)", "Taylorbound"),
        (r"\operatorname{rk4}(f, 0, 1, 0.1, 5)", "RK4"),
        (r"\operatorname{surfacearea}(x, [0, 1])", "Surfacearea"),
        (r"\operatorname{mvt}_{[0, 1]} x^2", "Meanvaluetheoremfor"),
        (r"\operatorname{crit}_{[0,4]} f", "Criticalpointsof𝑓on[0,4]"),
        (
            r"\operatorname{solve}_{y(x)} \left( y' = y \right)",
            "Solvefor𝑦(𝑥):",
        ),
        (
            r"\operatorname{series}_{x=0}^{5} f",
            "Taylorseriesof𝑓at𝑥=0,order5",
        ),
        (r"\operatorname{eig}\left( A \right)", "Eigenvalues"),
    ] {
        let t: String = text(src).chars().filter(|c| !c.is_whitespace()).collect();
        assert!(t.contains(words), "{src}: {t}");
    }
}
