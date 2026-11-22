//! Script 1: Ben's video (2026-11-21), exactly as he typed it, pauses included.

use egui::Key;
use egui_kittest::kittest::Queryable;
use mpp_syntax::document::{parse_document, BlockKind};

use super::runner::{fresh, run, snapshot, Seen, Step, Watch};

use Step::{Pause, Type};

/// The keys, with a pause wherever Ben stopped to think.
const SCRIPT: &[Step] = &[
    Type("#This "),
    Pause,
    Type("is a test version "),
    Pause,
    Type("of the math implementation"),
    Pause,
    Step::Key(Key::Enter),
    Type("x "),
    Pause,
    Type(":= 5"),
    Pause,
    Step::Key(Key::Enter),
    Type("E := "),
    Pause,
    Type("2x + 5"),
    Pause,
    Step::Key(Key::Enter),
    Type("1."),
    Pause,
    Type("321321312 + 5 ="),
    Pause,
];

#[test]
fn bens_video_exactly_as_he_typed_it() {
    let mut h = fresh();
    let mut w = Watch::at_end();
    let r = run(&mut h, SCRIPT, |step, kind, s| w.frame(step, kind, s));
    assert!(r.frames > 80, "only {} frames ran", r.frames);
    // Exactly Ben's keys, no Enter at the end: a heading and three maths lines
    // (one box, a run of maths shares one). `E` shows 15, and the last line,
    // ending in `=`, counts as finished after his pause (Design rule 6): it
    // became maths and shows its answer.
    let s = Seen::of(&h);
    let doc = parse_document(&s.text);
    assert!(
        matches!(doc.blocks[0].kind, BlockKind::Heading { .. }),
        "{:?}",
        s.text
    );
    assert_eq!(
        s.visible[0], "Thisisatestversionofthemathimplementation",
        "the heading as drawn (glyphs only, no #)"
    );
    assert_eq!((s.maths, s.bands), (3, 1), "{:?}", s.text);
    assert!(
        s.text.ends_with(
            "$$ E := 2x + 5 $$

$$ 1.321321312 + 5 = $$"
        ),
        "{:?}",
        s.text
    );
    assert_eq!(s.answers, ["15", "6.321321312"]);
    assert!(h.query_by_label("2 definitions").is_some(), "the count");
    snapshot(&mut h, "replay_video_typed");
}
