//! A prompt's answers played ahead on a copy of the game, for what a prompt opens with that
//! only the engine can say: which answer leaves the player able to cast more, which picks
//! each pick leaves.

use beryl_engine::card::Types;
use beryl_engine::state::Pending;
use beryl_engine::{
    Choice, Decision, Layout, Profile, Refusal, Replay, Scratch, Seat, State, Step, Stops, Zone,
    ZoneKey, apply, next, observe, refusal,
};

use super::held::Table;

/// A copy of `s` in which every question comes back as a decision, even one with a single
/// answer, and every priority too: play on the copy stops at the next thing asked
/// (architecture §7.2).
///
/// A resolution paused at a question keeps its stops. Each answer resumes it by running its
/// body again from the start, with the answers so far recorded in order, and under any stops
/// but forced ones a question with a single answer is answered silently and never recorded.
/// Switching to forced stops partway through would ask those questions too, and each would
/// take the recorded answer of the question after it.
pub(crate) fn forced<L: Layout>(s: &State<L>) -> State<L> {
    let mut copy = s.clone();
    if matches!(s.pending(), Pending::Resolving(_) | Pending::StepActions(_)) {
        return copy;
    }
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
    (crate::protocol::slots_by_id(held))
        .filter(|&slot| {
            after.contains(slot)
                && !shown.definition(slot).face(t).types.intersects(Types::LAND)
                && matches!(refusal(obs, t, slot), None | Some(Refusal::Target))
        })
        .count()
}

#[cfg(test)]
mod tests {
    use beryl_cards::{CARDS, names};
    use beryl_engine::{Opt, Profile, Replay, Scratch, Step, Stops, apply, next};
    use beryl_scenario::{P0, Permanent, Scenario};

    use super::*;

    /// A copy taken while Stock Up's resolution is paused at its first pick keeps smart
    /// stops, so the picks recorded under them still answer the questions they answered, and
    /// the resolution can be answered to its end on the copy. A copy taken at priority gets
    /// forced stops.
    #[test]
    fn a_paused_resolution_keeps_its_stops_on_a_forced_copy() {
        const ISLANDS: &[Permanent] = &[Permanent::new(names::ISLAND); 3];
        let scenario = Scenario::new("stock up")
            .hand(P0, &[names::STOCK_UP])
            .permanents(P0, ISLANDS)
            .library(
                P0,
                &[
                    names::PLAINS,
                    names::SWAMP,
                    names::ISLAND,
                    names::PLAINS,
                    names::SWAMP,
                ],
            );
        let (t, mut s) = beryl_scenario::position(&CARDS, &scenario).unwrap();
        s.set_profile(Profile::SMART);
        let mut x = Scratch::new();
        let mut cast = false;
        loop {
            let Step::Decide(d) = next(&mut s, &t, &mut x, &mut Replay) else {
                panic!("the game went on to a decision");
            };
            if matches!(s.pending(), Pending::Resolving(_)) {
                break;
            }
            let options: Vec<_> = (0..d.len())
                .filter_map(|i| d.candidates.option(Choice(u16::try_from(i).unwrap())))
                .collect();
            if !cast && let Some(i) = options.iter().position(|o| matches!(o, Opt::Cast { .. })) {
                assert_eq!(forced(&s).profile().stops, Stops::Forced);
                cast = true;
                apply(
                    &mut s,
                    &t,
                    &mut x,
                    &mut Replay,
                    Choice(u16::try_from(i).unwrap()),
                );
            } else {
                let i = options.iter().position(|o| *o == Opt::Pass).unwrap_or(0);
                apply(
                    &mut s,
                    &t,
                    &mut x,
                    &mut Replay,
                    Choice(u16::try_from(i).unwrap()),
                );
            }
        }
        let mut copy = forced(&s);
        assert_eq!(copy.profile().stops, s.profile().stops);
        for _ in 0..10 {
            let Step::Decide(_) = next(&mut copy, &t, &mut x, &mut Replay) else {
                break;
            };
            if !matches!(copy.pending(), Pending::Resolving(_)) {
                return;
            }
            apply(&mut copy, &t, &mut x, &mut Replay, Choice(0));
        }
        panic!("Stock Up resolved on the copy within ten answers");
    }
}
