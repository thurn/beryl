//! The invariant check catches a corrupted state.

use super::{MIXED, begin, table};
use crate::state::{ObjectFlags, Zone};
use crate::{Profile, Seat, Slot, State};

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
            s.object_mut(slot).zone = Zone::Library;
        }),
        ("the library list disagrees", |s, p, _| {
            s.zones.library[p.index()].pop();
        }),
        ("a hand card its owner doesn't know", |s, p, slot| {
            s.knowledge[p.index()].remove(slot);
        }),
        ("permanent status off the battlefield", |s, _, slot| {
            s.object_mut(slot).flags.set(ObjectFlags::TAPPED, true);
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
