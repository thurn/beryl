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

/// P0's permanents for the Shahrazad turn search: two Plains for `{W}{W}`.
const SHAHRAZAD_BATTLEFIELD: &[Permanent] = &[Permanent::new(PLAINS), Permanent::new(PLAINS)];
/// Eight cards in P0's library so P0 draws a full opening hand in the subgame while P1
/// (`PLAINS_1`) loses the subgame immediately for having fewer than seven cards.
const EIGHT_PLAINS: &[&str] = &[PLAINS; 8];

/// On P0's turn 5, P1 is at 1 life with one card in library: passing wins on turn 8 when P1
/// draws from an empty library, while Shahrazad starts a subgame that P1 loses immediately
/// (halving 1 life rounded up to 0) and wins on turn 5. A subgame only fits in `Large`.
const fn shahrazad() -> Scenario {
    Scenario::new("Shahrazad wins immediately")
        .at(5, P0, TurnStep::PrecombatMain)
        .permanents(P0, SHAHRAZAD_BATTLEFIELD)
        .hand(P0, &[names::SHAHRAZAD])
        .library(P0, EIGHT_PLAINS)
        .life(P1, 1)
        .library(P1, PLAINS_1)
}

/// P0's permanents when Shahrazad is on top of the library: two Plains for `{W}{W}` and a
/// Strip Mine whose activation can destroy one of P0's own Plains, making Shahrazad uncastable.
const ROLLOUT_SHAHRAZAD_BATTLEFIELD: &[Permanent] = &[
    Permanent::new(PLAINS),
    Permanent::new(PLAINS),
    Permanent::new(names::STRIP_MINE),
];
/// Shahrazad on top (drawn on turn 7 during the scoring rollout), followed by eight Plains so
/// P0 has a full opening hand in the subgame.
const SHAHRAZAD_ON_TOP: &[&str] = &[
    names::SHAHRAZAD,
    PLAINS,
    PLAINS,
    PLAINS,
    PLAINS,
    PLAINS,
    PLAINS,
    PLAINS,
    PLAINS,
];
const PLAINS_2: &[&str] = &[PLAINS; 2];

/// On P0's turn 5, nothing outgrows `Small` during the turn itself (`propose` and `finish`
/// stay in `Small`). Passing lets P0 draw and cast Shahrazad on turn 7 during `reward`'s
/// rollout (winning on turn 7); destroying a Plains with Strip Mine prevents Shahrazad and
/// wins on turn 10 when P1 decks out. Without widening the scoring rollout, passing stalls
/// and scores 0.5, so the search destroys its own Plains instead of passing.
const fn rollout_shahrazad() -> Scenario {
    Scenario::new("Shahrazad drawn in the scoring rollout wins soonest")
        .at(5, P0, TurnStep::PrecombatMain)
        .permanents(P0, ROLLOUT_SHAHRAZAD_BATTLEFIELD)
        .library(P0, SHAHRAZAD_ON_TOP)
        .life(P1, 1)
        .library(P1, PLAINS_2)
}

/// The turn search (`turnbudget72`, used by the Deep Searcher) widens into `Large` when a
/// line enters a subgame during the turn: without widening, Shahrazad stalls and scores as a
/// draw (0.5), losing to passing (which wins on turn 8).
#[test]
fn the_turn_search_casts_shahrazad_where_it_wins_soonest() {
    let settings = Settings {
        family: "turnbudget72/omniscient/rollout/tuned"
            .parse::<Family>()
            .unwrap(),
        work: 200_000,
        threads: 1,
    };
    let (t, s, d) = arrange(&shahrazad());
    let mut agent = LongSearch::new(&t, settings, 3);
    let mut x = Scratch::new();
    let obs = observe(&s, &t, &mut x, P0);
    let c = agent.choose(obs, &d, AgentDecision(0));
    assert!(
        matches!(d.candidates.option(c), Some(Opt::Cast { slot, .. })
        if s.copiable(slot).name(&t) == names::SHAHRAZAD)
    );
}

/// The turn search widens into `Large` when `reward`'s scoring rollout outgrows `Small` after
/// the turn itself finished in `Small`: without widening, passing stalls in the rollout on
/// turn 7 and scores 0.5, losing to destroying a Plains with Strip Mine.
#[test]
fn the_turn_search_widens_when_a_scoring_rollout_outgrows_small() {
    let settings = Settings {
        family: "turnbudget72/omniscient/rollout/tuned"
            .parse::<Family>()
            .unwrap(),
        work: 200_000,
        threads: 1,
    };
    let (t, s, d) = arrange(&rollout_shahrazad());
    let mut agent = LongSearch::new(&t, settings, 3);
    let mut x = Scratch::new();
    let obs = observe(&s, &t, &mut x, P0);
    let c = agent.choose(obs, &d, AgentDecision(0));
    assert_eq!(d.candidates.option(c), Some(Opt::Pass));
}
