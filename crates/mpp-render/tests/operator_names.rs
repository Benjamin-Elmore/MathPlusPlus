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
        (
            r"\operatorname{taylorbound}(f, 0, 5, 1)",
            "Errorboundforthedegree5Taylorpolynomialof𝑓about0,at1",
        ),
        (
            r"\operatorname{riemann}_{0}^{2}\left( x^2, 4, \text{mid} \right)",
            "MidpointRiemannsumof𝑥2on[0,2]with𝑛=4",
        ),
        (
            r"\operatorname{volume}_{0}^{1}\left( x, y = 0, \text{auto} \right)",
            "Volumefromrotating𝑥about𝑦=0on[0,1](methodchosenautomatically)",
        ),
        (
            r"\operatorname{rk4}\left( y' = y, y(0) = 1, 0.1, 10 \right)",
            "RK4solutionof𝑦′=𝑦with𝑦(0)=1,step",
        ),
        (r"\operatorname{rk4}(f, 0, 1, 0.1, 5)", "RK4"),
        (
            r"\operatorname{surfacearea}_{0}^{1}\left( x, y = 0 \right)",
            "Surfaceareafromrotating𝑥about𝑦=0on[0,1]",
        ),
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

#[test]
fn helpers_written_in_another_order_keep_their_plain_name() {
    for (src, words, not) in [
        (
            r"\operatorname{area}(\sin x, [0, 2\pi])",
            "Areabetweencurves",
            "and[",
        ),
        (
            r"\operatorname{riemann}_{left}(x^2, [0, 1], 4)",
            "Riemannsum",
            "rule,of",
        ),
    ] {
        let t: String = text(src).chars().filter(|c| !c.is_whitespace()).collect();
        assert!(t.contains(words) && !t.contains(not), "{src}: {t}");
    }
}
