//! Why the Windows binaries ask the linker for a 256 MB main-thread stack and the other
//! platforms do not: the parser runs deep input on its own big-stack thread, and the page
//! layout of the deepest maths the parser accepts fits the 8 MB that Linux and macOS give
//! the main thread. Windows gives only 1 MB, hence `/STACK` in the two build scripts.
//!
//! Layout runs here on a thread with exactly 8 MB (in a debug build, the worst case for
//! frame sizes) so a deeper layout recursion shows up as a failing test, not as a crash
//! for users on another platform.

use mpp_render::{layout_math, LayoutOptions};
use mpp_syntax::math::parse_math;

const MAIN_THREAD_STACK: usize = 8 << 20;

fn lay_out_on_a_main_sized_stack(src: String) {
    let (ast, _) = parse_math(&src, 0);
    let worker = std::thread::Builder::new()
        .stack_size(MAIN_THREAD_STACK)
        .spawn(move || {
            let list = layout_math(&ast, &LayoutOptions::default());
            assert!(list.total_height().is_finite());
        })
        .expect("thread");
    worker.join().expect("layout must fit an 8 MB stack");
}

#[test]
fn deeply_nested_fractions_lay_out_on_an_8_mb_stack() {
    let depth = 250;
    lay_out_on_a_main_sized_stack(format!(
        "{}1{}",
        "\\frac{1}{".repeat(depth),
        "}".repeat(depth)
    ));
}

#[test]
fn deeply_nested_powers_lay_out_on_an_8_mb_stack() {
    let depth = 250;
    lay_out_on_a_main_sized_stack(format!("{}x{}", "x^{".repeat(depth), "}".repeat(depth)));
}

#[test]
fn a_ten_thousand_term_sum_lays_out_on_an_8_mb_stack() {
    lay_out_on_a_main_sized_stack(vec!["x"; 10_000].join(" + "));
}
