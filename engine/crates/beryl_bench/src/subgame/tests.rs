use super::*;

/// Both sides have positions and take steps, so the instruction counts divide by something,
/// and the counts of steps are the same on a second run, since the benchmark compares counts
/// from separate runs.
#[test]
fn both_sides_roll_steps() {
    let w = SubgameWorkload::new();
    assert!(!w.subgame.is_empty(), "no game entered a subgame");
    assert!(w.subgame.iter().all(State::in_subgame));
    assert!(w.plain.iter().all(|s| !s.in_subgame()));
    let sub = roll(&w, Side::Subgame);
    let plain = roll(&w, Side::Plain);
    assert!(sub > 0 && plain > 0, "{sub} {plain}");
    assert_eq!((sub, plain), (roll(&w, Side::Subgame), roll(&w, Side::Plain)));
}
