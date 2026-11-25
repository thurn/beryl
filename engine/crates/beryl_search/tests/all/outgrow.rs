//! A search on a `Small` game scores an action whose result `Small` cannot hold as the game
//! goes on in `Large`, as a session's game would, and not as a stalled draw.
//!
//! Illusionary Mask's activation records the mana spent on X, which only `Large` holds, so
//! every line through it runs out of `Small`'s room at once. Here it is the one way to win:
//! Dandân cast face up can't attack an opponent who controls no Island, but cast face down
//! with the Mask it attacks as a 2/2, is turned face up as it would deal combat damage, and
//! deals its 4 to an opponent at 4 life.

use beryl_cards::{CARDS, names};
use beryl_engine::{
    Decision, DecisionKind, GameTable, Opt, Profile, Replay, Scratch, State, Step, TurnStep, next,
    observe,
};
use beryl_scenario::{P0, P1, Permanent, Scenario, position};
use beryl_search::{Agent, AgentDecision, Budget, Family, LongSearch, Mcts, Settings, Sight};

const ISLAND: &str = names::ISLAND;
const PLAINS: &str = names::PLAINS;
/// P0 draws one card next turn and none after, so a Dandân that is not cast this turn
/// never attacks before P0 loses for drawing from an empty library.
const ONE_ISLAND: &[&str] = &[ISLAND];
/// The opponent draws no Island, so Dandân may never attack face up. From twenty cards the
/// opponent never runs out; from one, they lose on turn 8 for drawing from an empty library,
/// before P0 would on turn 9.
const PLAINS_20: &[&str] = &[PLAINS; 20];
const PLAINS_1: &[&str] = &[PLAINS];

/// P0's permanents: the Mask and two Islands, the mana Dandân's cost needs spent on X.
const BATTLEFIELD: &[Permanent] = &[
    Permanent::new(names::ILLUSIONARY_MASK),
    Permanent::new(ISLAND),
    Permanent::new(ISLAND),
];

/// The position: on P0's turn 5 the face-down Dandân can attack on turn 7 and win.
const fn mask(opponent_library: &'static [&'static str]) -> Scenario {
    Scenario::new("Illusionary Mask wins soonest")
        .at(5, P0, TurnStep::PrecombatMain)
        .permanents(P0, BATTLEFIELD)
        .hand(P0, &[names::DANDAN])
        .library(P0, ONE_ISLAND)
        .life(P1, 4)
        .library(P1, opponent_library)
}

/// The position and P0's priority decision in it.
fn arrange(scenario: &Scenario) -> (GameTable, State, Decision) {
    let (t, mut s) = position(&CARDS, scenario).unwrap();
    s.set_profile(Profile::SMART);
    let Step::Decide(d) = next(&mut s, &t, &mut Scratch::new(), &mut Replay) else {
        panic!("the game ended");
    };
    assert_eq!((d.player, d.kind), (P0, DecisionKind::Priority));
    (t, s, d)
}

/// Whether the agent `new` makes activates the Mask at P0's priority in `scenario`.
fn activates<A: Agent>(scenario: &Scenario, new: impl FnOnce(&GameTable) -> A) -> bool {
    let (t, s, d) = arrange(scenario);
    let mut agent = new(&t);
    let mut x = Scratch::new();
    let obs = observe(&s, &t, &mut x, P0);
    let c = agent.choose(obs, &d, AgentDecision(0));
    matches!(d.candidates.option(c), Some(Opt::Activate { slot, .. })
        if s.copiable(slot).name(&t) == names::ILLUSIONARY_MASK)
}

/// Only the Mask wins. A line through it scored as a stalled draw ties with passing, after
/// which the rollout activates the Mask next turn and stalls just the same.
#[test]
fn mcts_activates_illusionary_mask_where_only_it_wins() {
    let budget = Budget {
        iterations: 60,
        trees: 1,
        parallel: false,
    };
    assert!(activates(&mask(PLAINS_20), |t| Mcts::new(
        t,
        Sight::Omniscient,
        budget,
        3
    )));
}

/// Every line wins, and the Mask's a turn sooner, which the long search's discount for
/// each turn rewards. A line through it scored as a stalled draw loses to the others.
#[test]
fn the_long_search_activates_illusionary_mask_where_it_wins_soonest() {
    let settings = Settings {
        family: "pimc4/omniscient/rollout/tuned".parse::<Family>().unwrap(),
        work: 200_000,
        threads: 1,
    };
    assert!(activates(&mask(PLAINS_1), |t| LongSearch::new(
        t, settings, 3
    )));
}
