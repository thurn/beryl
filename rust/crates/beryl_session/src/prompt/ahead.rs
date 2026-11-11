//! A prompt's answers played ahead on a copy of the game, for what a prompt opens with that
//! only the engine can say: which answer leaves the player able to cast more, which picks
//! each pick leaves.

use beryl_engine::card::Types;
use beryl_engine::{
    Choice, Decision, Layout, Profile, Refusal, Replay, Scratch, Seat, State, Step, Stops, Zone,
    ZoneKey, apply, next, observe, refusal,
};

use super::held::Table;

/// A copy of `s` in which every question comes back as a decision, even one with a single
/// answer, and every priority too: play on the copy stops at the next thing asked
/// (architecture §7.2).
pub(crate) fn forced<L: Layout>(s: &State<L>) -> State<L> {
    let mut copy = s.clone();
    copy.set_profile(Profile {
        stops: Stops::Forced,
        ..s.profile()
    });
    copy
}

/// Answers the decision open in `copy` with `choice` and plays on to the next decision, if
/// the game asks another.
pub(crate) fn answer<L: Layout>(
    table: &Table<'_, L>,
    copy: &mut State<L>,
    choice: Choice,
) -> Option<Decision<L>> {
    let mut scratch = Scratch::new();
    apply(copy, table.t, &mut scratch, &mut Replay, choice);
    match next(copy, table.t, &mut scratch, &mut Replay) {
        Step::Decide(d) => Some(d),
        Step::Over(_) => None,
    }
}

/// How many of the nonland cards in `viewer`'s hand on `table` they could pay for in `then`,
/// a copy of its game played ahead, at a time they may cast them. A spell with nothing to
/// target yet counts, as a counterspell held for the opponent's turn. Only cards in their
/// hand now count, so nothing drawn on the way says anything here.
pub(crate) fn castable<L: Layout>(table: &Table<'_, L>, viewer: Seat, then: &State<L>) -> usize {
    let (shown, t) = (table.shown, table.t);
    let held = shown.zone(ZoneKey::Of(viewer, Zone::Hand));
    let after = then.zone(ZoneKey::Of(viewer, Zone::Hand));
    let mut scratch = Scratch::new();
    let obs = observe(then, t, &mut scratch, viewer);
    (shown.in_candidate_order(held))
        .filter(|&slot| {
            after.contains(slot)
                && !shown.copiable(slot).face(t).types.intersects(Types::LAND)
                && matches!(refusal(obs, t, slot), None | Some(Refusal::Target))
        })
        .count()
}
