//! Random documents never make the evaluator panic, in a full run or an update.

use mpp_eval::Session;
use proptest::prelude::*;

/// Pieces of `.mpp` text, valid and broken, that random documents are built from.
const PIECES: &[&str] = &[
    "$$ a := 3 $$",
    "$$ a := a + 1 $$",
    "$$ b := a^2 $$",
    "$$ g \\equiv 9.81 $$",
    "$$ g := 1 $$",
    "$$ f(x) := x^2 + a $$",
    "$$ f(x, y) := x y $$",
    "$$ f(2) = ? $$",
    "$$ f(1, 2) \\approx ? $$",
    "$$ f'(x) = ? $$",
    "$$ F(n) := \\begin{cases} 1 & n \\le 0 \\\\ n F(n-1) & \\text{otherwise} \\end{cases} $$",
    "$$ F(6) = ? $$",
    "$$ F(k) = ? $$",
    "$$ G(n) := G(n + 1) $$",
    "$$ G(1) = ? $$",
    "$$ a_1 := 1 $$",
    "$$ a_{n+1} := 2 a_n $$",
    "$$ a_{n+1} := a_{n+2} $$",
    "$$ a_{30} = ? $$",
    "$$ a_{100000} = ? $$",
    "$$ v_0 := 20 $$",
    "$$ E := \\tfrac12 m v_0^2 $$",
    "$$ E = ? $$",
    "$$ x \\approx ? $$",
    "$$ \\sqrt{-1} \\approx ? $$",
    "$$ 1/0 = ? $$",
    "$$ 0^0 \\approx ? $$",
    "$$ \\ln(0) \\approx ? $$",
    "$$ 10^{10^{10}} = ? $$",
    "$$ 100000! = ? $$",
    "$$ (x+1)^{200} = ? $$",
    "$$ \\operatorname{expand}((x+1)^{60}) = ? $$",
    "$$ \\sum_{k=1}^{100000} k = ? $$",
    "$$ \\sum_{k=1}^{n} k^2 = ? $$",
    "$$ \\int_0^1 x \\, dx \\approx ? $$",
    "$$ \\lim_{x \\to 0} x = ? $$",
    "$$ \\frac{d}{dx} x^2 = ?! $$",
    "$$ \\nabla f = ? $$",
    "$$ \\sin^2 x + \\cos^2 x \\overset{?}{=} 1 $$",
    "$$ x \\overset{?}{=} y $$",
    "$$ A = \\pi r^2, \\quad r = 3 $$",
    "$$ A = ?, \\quad r = $$",
    "$$ a := 2, \\; a \\in [0, 5] $$",
    "$$ \\begin{pmatrix} 1 & 2 \\\\ 3 \\end{pmatrix} = ? $$",
    "$$ \\begin{vmatrix} 1 & 2 \\\\ 3 & 4 \\end{vmatrix} \\approx ? $$",
    "$$ \\{1, 2\\} \\cup [0, 1) = ? $$",
    "$$ x + $$",
    "$$ = ? $$",
    "$$ \\frac{ $$",
    "$$ e := i $$",
    "$$ e^{i\\pi} = ? $$",
    "$$ p := q \\\\ q := p \\\\ p = ? $$",
    "Let $c = 4$ and $d := c + 1$.",
    "Using $\\theta, R(\\theta)$ from [[launch.mpp]].",
    "![[constants.mpp]]",
    "Figure: $y(x)$, $x \\in [0, R]$.",
    "Find the critical points of $f$ on $[0, 4]$.",
    "**Theorem.** If $a = ?$ then $b$.",
    "**Proof.** Clear. ∎",
    "# Heading with $1 + 1 = ?$",
    "- item $x := 1$",
    "| a | b |\n|---|---|\n| $1 = ?$ | 2 |",
    "%noeval",
    "Plain prose.",
    "$$",
    "$",
    "\\",
    "{",
];

fn document() -> impl Strategy<Value = String> {
    prop::collection::vec(0..PIECES.len(), 0..14).prop_map(|idx| {
        idx.into_iter()
            .map(|i| PIECES[i])
            .collect::<Vec<_>>()
            .join("\n\n")
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 96, ..ProptestConfig::default() })]

    #[test]
    fn run_str_never_panics(doc in document()) {
        let mut s = Session::new();
        let ev = s.run_str(&doc);
        let _ = ev.report();
    }

    #[test]
    fn update_never_panics_and_matches_a_full_run(a in document(), b in document()) {
        let mut s = Session::new();
        s.run_str(&a);
        let incremental = s.update(&b).report();
        let full = Session::new().run_str(&b).report();
        prop_assert_eq!(incremental, full);
    }

    #[test]
    fn arbitrary_text_never_panics(text in "[ -~\\n$\\\\{}^_αβπ∫∑≈≡]{0,200}") {
        let mut s = Session::new();
        let _ = s.run_str(&text).report();
    }
}

#[test]
fn deep_recursion_fits_in_a_small_stack() {
    let doc = r"
$$ F(n) := \begin{cases} 1 & n \le 0 \\ n F(n-1) & \text{otherwise} \end{cases} $$

$$ F(120) = ? $$

$$ F(400) = ? $$

$$ G(n) := G(n + 1) + 1 $$

$$ G(1) = ? $$

$$ a_1 := 1 \\ a_{n+1} := a_n + 1 $$

$$ a_{9000} = ? $$
";
    let handle = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || Session::new().run_str(doc).report())
        .expect("spawn");
    let report = handle.join().expect("no stack overflow");
    assert!(report.contains("calls itself too deeply"), "{report}");
    assert!(report.contains("= 9000"), "{report}");
    // 120! is computed by 120 nested calls.
    assert!(
        report.contains("= 66895029134491270575881180540903725867527463"),
        "{report}"
    );
}
