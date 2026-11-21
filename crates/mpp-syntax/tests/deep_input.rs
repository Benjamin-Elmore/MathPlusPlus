//! Deep and long input: up to 256 levels of brackets and 10 000 operators on a line
//! parse without error, more is a clear error, and no input overflows the stack,
//! even on a 1 MB thread (the parser moves deep input to a large stack of its own).

use mpp_syntax::math::parse_math;

const STACK: usize = 1 << 20;

/// Runs `f` on a thread with a 1 MB stack, like a Windows main thread.
fn small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(f)
        .expect("spawn")
        .join()
        .expect("no stack overflow")
}

fn messages(src: String) -> Vec<String> {
    small_stack(move || {
        let (ast, diags) = parse_math(&src, 0);
        // Dropping a deep tree is part of the test.
        drop(ast);
        diags.into_iter().map(|d| d.message).collect()
    })
}

fn nested(open: &str, close: &str, levels: usize) -> String {
    format!("{}x{}", open.repeat(levels), close.repeat(levels))
}

#[test]
fn two_hundred_and_fifty_six_levels_of_brackets_parse() {
    for (open, close) in [("(", ")"), ("[", "]"), ("{", "}"), (r"\left(", r"\right)")] {
        let m = messages(nested(open, close, 256));
        assert!(m.is_empty(), "{open}: {m:?}");
    }
    let m = messages(nested(r"\sqrt{", "}", 200));
    assert!(m.is_empty(), "{m:?}");
}

#[test]
fn deeper_nesting_is_an_error_not_a_crash() {
    for levels in [300, 2_000, 50_000] {
        let m = messages(nested("(", ")", levels));
        assert!(
            m.iter().any(|s| s.contains("nested too deeply")),
            "{levels}: {m:?}"
        );
    }
}

#[test]
fn ten_thousand_operators_parse() {
    let sum = format!("{}1", "1 + ".repeat(9_999));
    assert!(messages(sum).is_empty());
    let product = format!("{}x", "x ".repeat(9_999));
    assert!(messages(product).is_empty());
    let mixed = format!("{}a", r"a - b \cdot ".repeat(4_000));
    assert!(messages(mixed).is_empty());
}

#[test]
fn longer_lines_are_an_error_not_a_crash() {
    let m = messages(format!("{}1", "1 + ".repeat(12_000)));
    assert!(m.iter().any(|s| s.contains("too long")), "{m:?}");
}

/// A small deterministic generator (no crate needed): xorshift.
struct Gen(u64);

impl Gen {
    fn next(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

/// Random deep expressions: nested brackets, roots, fractions, powers, signs and
/// long sums, up to the limits and past them.
fn random_expr(g: &mut Gen, depth: usize, budget: &mut usize) -> String {
    if depth == 0 || *budget == 0 {
        return ["x", "1", r"\pi", "y_1"][g.next(4) as usize].to_string();
    }
    *budget -= 1;
    let inner = random_expr(g, depth - 1, budget);
    match g.next(9) {
        0 => format!("({inner})"),
        1 => format!(r"\sqrt{{{inner}}}"),
        2 => format!(r"\frac{{{inner}}}{{2}}"),
        3 => format!("x^{{{inner}}}"),
        4 => format!("-{inner}"),
        5 => format!(r"\left[{inner}\right]"),
        6 => format!("{inner} + {}", "1 + ".repeat(g.next(3000) as usize)),
        7 => format!(r"\sin({inner})"),
        _ => format!("{{{inner}}}"),
    }
}

#[test]
fn random_deep_input_never_overflows() {
    let mut g = Gen(0x9E37_79B9_7F4A_7C15);
    for _ in 0..40 {
        let depth = g.next(400) as usize;
        let mut budget = 400;
        let src = format!("{} = ?", random_expr(&mut g, depth, &mut budget));
        // Errors are fine; a crash is not.
        let _ = messages(src);
    }
}
