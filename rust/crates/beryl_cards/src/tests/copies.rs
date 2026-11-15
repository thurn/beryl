//! A permanent that becomes a copy of a legendary permanent its controller already has meets
//! the legend rule ([CR 704.5j]): "If two or more legendary permanents with the same name are
//! controlled by the same player, that player chooses one of them, and the rest are put into
//! their owners' graveyards." Thespian's Stage that becomes a copy of its controller's Dark
//! Depths may be the one kept, and it has no ice counters, since it became a copy without
//! entering.
//!
//! The two are both named Dark Depths, and a scenario picks a permanent by name, so no
//! scenario can say which is kept: this test answers the legend rule with the Stage itself.
//!
//! [CR 704.5j]: https://yawgatog.com/resources/magic-rules/#R7045j

use beryl_engine::state::CounterKind;
use beryl_engine::{
    Choice, DecisionKind, GameTable, Opt, Replay, Scratch, Seat, Slot, State, Step, Zone, ZoneKey,
    apply, next,
};
use beryl_scenario::{Action, P0, Permanent, Pick, Scenario};

use crate::{CARDS, names};

const STAGE: &str = "Thespian's Stage";
const DEPTHS: &str = "Dark Depths";

/// P0's Thespian's Stage becomes a copy of P0's Dark Depths, which has its ten ice counters.
/// The scenario stops as the legend rule asks which to keep.
const STAGED: Scenario = Scenario::new("Thespian's Stage copies its controller's Dark Depths")
    .interaction(&[DEPTHS])
    .battlefield(P0, &[STAGE, names::ISLAND, names::ISLAND])
    .permanents(P0, &[Permanent::new(DEPTHS).counters(CounterKind::Ice, 10)])
    .actions(&[
        Action::activate(P0, STAGE, 1),
        Action::answer(P0, DecisionKind::Target, Pick::Card { card: DEPTHS }),
        Action::Resolve,
        Action::answer(P0, DecisionKind::ChooseObject, Pick::Card { card: DEPTHS }),
    ]);

/// The names of the cards `owner` has in `zone`, by the card each really is.
fn names_in(t: &GameTable, s: &State, owner: Seat, zone: Zone) -> Vec<&'static str> {
    let mut names: Vec<_> = (s.in_candidate_order(s.zone(ZoneKey::Of(owner, zone))))
        .map(|slot| s.definition_name(t, slot))
        .collect();
    names.sort_unstable();
    names
}

#[test]
fn the_stage_that_copies_dark_depths_may_be_kept_and_makes_marit_lage() {
    let (t, _) = beryl_scenario::position(&CARDS, &STAGED).unwrap_or_else(|e| panic!("{e}"));
    let mut states = Vec::new();
    beryl_scenario::visit(&CARDS, &STAGED, |_, s, _| states.push(*s))
        .unwrap_or_else(|e| panic!("{e}"));
    let mut s = states.pop().expect("the scenario has actions");
    let mut x = Scratch::new();
    let Step::Decide(d) = next(&mut s, &t, &mut x, &mut Replay) else {
        panic!("the game ended");
    };
    assert_eq!((d.kind, d.len()), (DecisionKind::ChooseObject, 2));
    let stage: Slot = (s.battlefield().iter().copied())
        .find(|&slot| s.definition_name(&t, slot) == STAGE)
        .expect("the Stage is on the battlefield");
    assert_eq!(
        s.name(&t, stage),
        DEPTHS,
        "the Stage is a copy of Dark Depths"
    );
    assert_eq!(
        s.counters_of(stage, CounterKind::Ice),
        0,
        "a copy has no counters"
    );
    let keep = (0..d.len())
        .map(|i| Choice(i as u16))
        .find(|&c| d.candidates.object(c) == Some(stage))
        .expect("the Stage is a candidate");
    apply(&mut s, &t, &mut x, &mut Replay, keep);
    // Until the stack is empty, the kept copy's state trigger goes on it and resolves.
    for _ in 0..8 {
        let Step::Decide(d) = next(&mut s, &t, &mut x, &mut Replay) else {
            panic!("the game ended");
        };
        if s.stack().is_empty() && s.triggers().is_empty() {
            break;
        }
        // Putting the one trigger on the stack has one answer; at priority, each passes.
        let answer = (0..d.len())
            .map(|i| Choice(i as u16))
            .find(|&c| {
                d.kind != DecisionKind::Priority || d.candidates.option(c) == Some(Opt::Pass)
            })
            .expect("an answer");
        apply(&mut s, &t, &mut x, &mut Replay, answer);
    }
    assert!(s.stack().is_empty());
    assert_eq!(
        names_in(&t, &s, P0, Zone::Graveyard),
        [DEPTHS, STAGE],
        "the original by the legend rule, and the Stage, sacrificed as itself"
    );
    let battlefield: Vec<_> = (s.battlefield().iter())
        .filter(|&&slot| s.controller(slot) == P0)
        .map(|&slot| s.name(&t, slot))
        .collect();
    assert!(battlefield.contains(&"Marit Lage"), "{battlefield:?}");
}
