//! The invariant check catches a corrupted state.

use super::{MIXED, begin, table};
use crate::state::Zone;
use crate::{Profile, Seat, Slot, State};
use beryl_kernel::board::Corruption as Break;

/// Breaks one invariant of a state, given the active player and a card in their hand.
type Corruption = fn(&mut State, Seat, Slot);

#[test]
fn the_invariants_hold_in_play_and_fail_on_corruption() {
    let t = table([MIXED, MIXED]);
    let mut s = State::new(&t, 2, Profile::EXACT);
    begin(&t, &mut s);
    assert_eq!(s.check_invariants(&t), Ok(()));
    let p = s.turn().active;
    let hand = s.zone(p, Zone::Hand).first().unwrap();
    let corruptions: [(&str, Corruption); 6] = [
        ("zone bitboards disagree", |s, _, slot| {
            s.board().corrupt(slot, Break::Zone(Zone::Library));
        }),
        ("the library list disagrees", |s, p, _| {
            s.zones.library[p.index()].pop();
        }),
        ("a hand card its owner doesn't know", |s, p, slot| {
            s.knowledge[p.index()].remove(slot);
        }),
        ("permanent status off the battlefield", |s, _, slot| {
            s.board().corrupt(slot, Break::Tapped);
        }),
        ("a player who has lost has priority", |s, p, _| {
            s.player_mut(p).life = 0;
        }),
        ("derived characteristics are stale", |s, _, _| {
            s.flags.derived_dirty = true;
        }),
    ];
    for (what, corrupt) in corruptions {
        let mut bad = s;
        corrupt(&mut bad, p, hand);
        assert_eq!(bad.check_invariants(&t).map_err(|v| v.what), Err(what));
    }
}

/// Debug builds check the invariants inside the engine (§17.4): `next` handed a corrupted
/// state panics, and so does `apply`, so every test that plays the engine checks them.
#[cfg(debug_assertions)]
#[test]
fn the_engine_panics_on_a_broken_invariant() {
    use crate::{Choice, Replay, Scratch, apply, next};
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let t = table([MIXED, MIXED]);
    let mut s = State::new(&t, 2, Profile::EXACT);
    begin(&t, &mut s);
    let p = s.turn().active;
    let hand = s.zone(p, Zone::Hand).first().unwrap();
    s.board().corrupt(hand, Break::Tapped);
    let message = |run: &mut dyn FnMut(&mut State)| {
        let mut bad = s;
        let err = catch_unwind(AssertUnwindSafe(|| run(&mut bad))).unwrap_err();
        err.downcast::<String>().map(|m| *m).unwrap_or_default()
    };
    let broke = "an engine invariant broke";
    let by_next = message(&mut |s| {
        next(s, &t, &mut Scratch::new(), &mut Replay);
    });
    assert!(by_next.starts_with(broke), "{by_next}");
    let by_apply = message(&mut |s| apply(s, &t, &mut Scratch::new(), &mut Replay, Choice(0)));
    assert!(by_apply.starts_with(broke), "{by_apply}");
}
