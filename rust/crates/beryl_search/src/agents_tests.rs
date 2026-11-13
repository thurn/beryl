//! Every registered agent plays a game whose cards have no per-deck overlay entry
//! ([`crate::overlay`]): what it knows of them is all derived from their definitions, and
//! nothing in building or playing an agent requires a tuned card.

use beryl_cards::CARDS;
use beryl_engine::{
    Decision, Decklist, GameTable, Outcome, Profile, Replay, Scratch, Small, State, Step, apply,
    next, observe,
};

use crate::values::{CardInfo, info};
use crate::{
    AGENTS, Budget, DEEP_SEARCHER, DEEP_SEER, DEEP_SEER_UNANCHORED, Family, GameAgent, Knowledge,
    RETIRED, deep_family,
};

/// Basic Swamps and black vanilla creatures complete from the card store, with no file. Twelve
/// cards, not a legal 40, so that a library runs out within a few turns and the searching
/// agents' rollouts, each a game played out to its end, stay short.
const VANILLA: &Decklist = &[
    (6, "Swamp"),
    (2, "Scathe Zombies"),
    (2, "Bog Imp"),
    (1, "Stone-Throwing Devils"),
    (1, "Headless Horseman"),
];

/// The engine steps a long-think agent spends on a decision here, in place of its usual
/// 30M, so that a debug build plays a whole game in seconds.
const SMALL_WORK: u64 = 2_000;

/// The agent `name` with a small budget: the registry's own constructor for the agents that
/// take a budget, and the long-think agents' families at [`SMALL_WORK`], since those ignore
/// the budget they are given.
fn cheap(
    t: &GameTable,
    name: &str,
    new: fn(&GameTable, u64, Budget) -> Box<dyn GameAgent + Send>,
    seed: u64,
) -> Box<dyn GameAgent + Send> {
    let family = match name {
        "pimc18-omniscient-30m-anchor" => Some(DEEP_SEER),
        "pimc72-hidden-30m-anchor" => Some(DEEP_SEARCHER),
        "pimc18-omniscient-30m" => Some(DEEP_SEER_UNANCHORED),
        _ => None,
    };
    match family {
        Some(family) => deep_family(t, seed, family, SMALL_WORK),
        None => new(
            t,
            seed,
            Budget {
                iterations: 16,
                trees: 1,
                parallel: false,
            },
        ),
    }
}

#[test]
fn every_agent_plays_cards_with_no_overlay_entry_to_the_end() {
    let t = GameTable::new(&CARDS, [VANILLA, VANILLA]).unwrap();
    for &(_, name) in VANILLA {
        let def = CARDS.get(name).unwrap();
        // The tiers value an untuned card differently (`Knowledge::Tuned` by its face alone),
        // so only the flags show whether the overlay has an entry for it.
        let flags = |knowledge| CardInfo {
            value: 0,
            ..info(def, knowledge)
        };
        assert_eq!(
            format!("{:?}", flags(Knowledge::Tuned)),
            format!("{:?}", flags(Knowledge::Estimate)),
            "{name} has an overlay entry"
        );
    }
    for agent in AGENTS.iter().chain(RETIRED) {
        let agents = [
            cheap(&t, agent.name, agent.new, 1),
            cheap(&t, agent.name, agent.new, 2),
        ];
        let outcome = play(&t, agents);
        assert!(
            matches!(outcome, Outcome::Win(_) | Outcome::Draw),
            "{}: {outcome:?}",
            agent.name
        );
    }
}

/// The champion's family with derived knowledge only (`Knowledge::Estimate`), as the ladder
/// plays it by the family name `.../estimate`, plays a game to its end.
#[test]
fn the_derived_only_champion_plays_to_the_end() {
    let t = GameTable::new(&CARDS, [VANILLA, VANILLA]).unwrap();
    let family = Family {
        knowledge: Knowledge::Estimate,
        ..DEEP_SEARCHER
    };
    assert!(family.to_string().contains("/estimate"), "{family}");
    let agents = [
        deep_family(&t, 1, family, SMALL_WORK),
        deep_family(&t, 2, family, SMALL_WORK),
    ];
    let outcome = play(&t, agents);
    assert!(
        matches!(outcome, Outcome::Win(_) | Outcome::Draw),
        "{outcome:?}"
    );
}

/// Plays a game of `t` between `agents` to its end.
fn play(t: &GameTable, mut agents: [Box<dyn GameAgent + Send>; 2]) -> Outcome {
    let mut s: State<Small> = State::new(t, 7, Profile::SMART).unwrap();
    let mut x = Scratch::new();
    loop {
        match next(&mut s, t, &mut x, &mut Replay) {
            Step::Over(outcome) => return outcome,
            Step::Decide(d) => {
                let d: Decision<Small> = d;
                let c = agents[d.player.index()].choose(observe(&s, t, &mut x, d.player), &d);
                apply(&mut s, t, &mut x, &mut Replay, c);
            }
        }
    }
}
