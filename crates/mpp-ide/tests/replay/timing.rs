//! The Smooth typing target, part (1): every key shows its character in the
//! same frame it is pressed, and that frame is quick, on a 200-line file in a
//! debug build. The 16 ms target is for a release build on a desktop; a debug
//! build on a shared CI machine is several times slower, so the bound checked
//! here is looser and the numbers are printed (`--nocapture`) for the record.

use std::time::Duration;

use super::runner::{app_with, harness, run, Seen, Step};

/// A 200-line worksheet: headings, prose and maths lines with answers.
fn long_file() -> String {
    let mut s = String::from("# Timing\n\n");
    let mut lines = 1;
    let mut i = 0;
    while lines < 200 {
        match i % 4 {
            0 => s.push_str(&format!("## Part {i}\n\n")),
            1 => s.push_str(&format!("Some words about part {i}, to read.\n\n")),
            2 => s.push_str(&format!("$$ a_{{{i}}} := {i} $$\n\n")),
            _ => s.push_str(&format!("$$ 2 a_{{{}}} + 1 = $$\n\n", i - 1)),
        }
        lines += 2;
        i += 1;
    }
    s
}

#[test]
fn each_key_shows_in_its_own_frame_on_a_200_line_file() {
    let text = long_file();
    assert!(text.lines().count() >= 200);
    let mut h = harness(app_with(&text));
    h.run_steps(3);
    run(&mut h, &[Step::Pause], |_, _, _| {});
    let keys = "The answer is $x$ and more words here";
    let r = run(&mut h, &[Step::Type(keys)], |_, _, s: &Seen| {
        assert!(s.drawn_now, "a key was drawn a frame late");
    });
    assert_eq!(r.key_frames.len(), keys.chars().count());
    // The first frame after a pause pays for the answers that arrived; the
    // ones after it are what typing feels like.
    let mut ms: Vec<f64> = r
        .key_frames
        .iter()
        .map(|d| d.as_secs_f64() * 1000.0)
        .collect();
    let first = ms[0];
    ms.sort_by(f64::total_cmp);
    let median = ms[ms.len() / 2];
    let p90 = ms[ms.len() * 9 / 10];
    let max = ms[ms.len() - 1];
    println!(
        "U27 timing, 200-line file, debug build: first {first:.1} ms, median {median:.1} ms, \
         90th percentile {p90:.1} ms, max {max:.1} ms over {} keys",
        ms.len()
    );
    // Debug build: a loose bound (see the module note).
    let bound = Duration::from_millis(50).as_secs_f64() * 1000.0;
    assert!(median < bound, "median {median:.1} ms per key frame");
}
