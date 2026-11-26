//! Every registered agent plays a game whose cards have no per-deck overlay entry
//! (the overlay): what it knows of them is all derived from their definitions, and
//! nothing in building or playing an agent requires a tuned card.

use beryl_cards::CARDS;
use beryl_cards::decks::FORGETFUL_FISH;
use beryl_engine::{
    Decision, Decklist, Format, GameTable, Outcome, Profile, Replay, Scratch, Small, State, Step,
    apply, next, observe,
};
use beryl_search::{
    AGENTS, AgentDecision, Budget, CardValues, DEEP_SEARCHER, DEEP_SEER, DEEP_SEER_UNANCHORED,
    Family, GameAgent, Knowledge, RETIRED, deep_family,
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
    let t = GameTable::new(Format::CONSTRUCTED, &CARDS, [VANILLA, VANILLA]).unwrap();
    // The tiers value an untuned card differently (`Knowledge::Tuned` by its face alone), so
    // only the flags show whether the overlay has an entry for it.
    let tuned = CardValues::with(&t, Knowledge::Tuned);
    let estimated = CardValues::with(&t, Knowledge::Estimate);
    for &(_, name) in VANILLA {
        let card = t.index_named::<beryl_engine::CardIx>(name).unwrap();
        let flags = |values: &CardValues| {
            let mut info = values.get(card);
            info.value = 0;
            format!("{info:?}")
        };
        assert_eq!(
            flags(&tuned),
            flags(&estimated),
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
    let t = GameTable::new(Format::CONSTRUCTED, &CARDS, [VANILLA, VANILLA]).unwrap();
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

/// Every agent, the champion included at a small budget, plays seeded games of the shared-deck
/// format (`Format::SHARED_DECK`, one library and graveyard for both players) with the Dandân
/// deck to their end, against the random agent and against itself, answering every decision
/// legally while the invariants hold. A retired agent plays the random agent only.
#[test]
fn every_agent_plays_a_shared_deck_game_to_the_end() {
    let t = GameTable::new(Format::SHARED_DECK, &CARDS, [FORGETFUL_FISH, &[]]).unwrap();
    let random = RETIRED.iter().find(|a| a.name == "random").unwrap();
    for (i, agent) in AGENTS.iter().chain(RETIRED).enumerate() {
        let opponents: &[_] = if i < AGENTS.len() {
            &[random, agent]
        } else {
            &[random]
        };
        for (j, &opponent) in opponents.iter().enumerate() {
            let seed = 10 * i as u64 + j as u64;
            let agents = [
                cheap(&t, agent.name, agent.new, seed),
                cheap(&t, opponent.name, opponent.new, seed + 1),
            ];
            let outcome = play_seeded(&t, agents, seed, true);
            assert!(
                matches!(outcome, Outcome::Win(_) | Outcome::Draw),
                "{} against {}, seed {seed}: {outcome:?}",
                agent.name,
                opponent.name,
            );
        }
    }
}

/// Plays a game of `t` between `agents` to its end.
fn play(t: &GameTable, agents: [Box<dyn GameAgent + Send>; 2]) -> Outcome {
    play_seeded(t, agents, 7, false)
}

/// Plays game `seed` of `t` between `agents` to its end, checking that each answer is one of
/// the decision's options and, when `check` is set, the invariants at every decision.
fn play_seeded(
    t: &GameTable,
    mut agents: [Box<dyn GameAgent + Send>; 2],
    seed: u64,
    check: bool,
) -> Outcome {
    let mut s: State<Small> = State::new(t, seed, Profile::SMART).unwrap();
    let mut x = Scratch::new();
    let mut asked = [AgentDecision(0); 2];
    loop {
        match next(&mut s, t, &mut x, &mut Replay) {
            Step::Over(outcome) => return outcome,
            Step::Decide(d) => {
                let d: Decision<Small> = d;
                if check {
                    s.check_invariants(t)
                        .unwrap_or_else(|v| panic!("seed {seed}: {v:?}"));
                }
                let i = d.decider.index();
                let c = agents[i].choose(observe(&s, t, &mut x, d.decider), &d, asked[i].take());
                assert!(usize::from(c.0) < d.len(), "seed {seed}: {c:?} for {d:?}");
                apply(&mut s, t, &mut x, &mut Replay, c);
            }
        }
    }
}
