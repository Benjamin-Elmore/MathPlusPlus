//! `mpp run` keeps exact answers exact: a `= ?` query of a constant prints its closed
//! form (`2√2`), the same as `mpp calc`, and falls back to a decimal only when the value
//! is an unevaluated call (`erf(1/2)`).

use std::sync::Arc;

use mpp_cli::run_report;
use mpp_eval::{NoImports, Session};

fn report(src: &str) -> String {
    let ev = Session::new()
        .with_importer(Arc::new(NoImports))
        .run_str(src)
        .clone();
    run_report(&ev).trim().to_string()
}

#[test]
fn constant_queries_print_their_exact_form() {
    let cases = [
        (r"$$ \sqrt{8} = ? $$", "√8 = 2√2"),
        (r"$$ \sqrt{12} + \sqrt{27} = ? $$", "√12 + √27 = 5√3"),
        (r"$$ \cos(\pi/6) = ? $$", "cos(π/6) = √3/2"),
        (r"$$ \frac{1}{1 + i} = ? $$", "1/(1 + i) = 1/2 - i/2"),
        (r"$$ \Gamma(1/2) = ? $$", "Γ(1/2) = √π"),
        (r"$$ \psi(1) = ? $$", "ψ(1) = -γ"),
        (r"$$ \frac{\pi}{2} = ? $$", "π/2 = π/2"),
    ];
    for (src, want) in cases {
        assert_eq!(report(src), want, "{src}");
    }
}

#[test]
fn unevaluated_calls_still_fall_back_to_a_decimal() {
    assert_eq!(
        report(r"$$ \operatorname{erf}(1/2) = ? $$"),
        "erf(1/2) ≈ 0.5204998778"
    );
    assert_eq!(report(r"$$ \zeta(3) = ? $$"), "ζ(3) ≈ 1.202056903");
}

#[test]
fn the_one_line_answer_of_an_unsimplified_value_is_its_decimal() {
    // The editor's report, `mpp calc` and the kept answer all read the one-line answer:
    // `= ζ(3) ≈ 1.202056903` would repeat the question, so it is the decimal alone.
    let answer = |src: &str| {
        let ev = Session::new()
            .with_importer(Arc::new(NoImports))
            .run_str(src)
            .clone();
        let (_, line) = ev.queries().next().expect("a query");
        line.answer().expect("an answer")
    };
    assert_eq!(answer(r"$$ \zeta(3) = ? $$"), "≈ 1.202056903");
    assert_eq!(answer(r"$$ J_0(1) = ? $$"), "≈ 0.7651976866");
    assert_eq!(answer(r"$$ \zeta(2) = ? $$"), "= π²/6 ≈ 1.644934067");
}
