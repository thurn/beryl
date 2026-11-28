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
    AGENTS, AgentDecision, CHAMPION, CardValues, DEEP_SEARCHER, DEEP_WORK, Family, GameAgent,
    Knowledge, RETIRED, agent_at_work, champion_at, deep_family,
};

use crate::common::{SMALL_WORK, cheap};

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

/// The start screen offers the champion alone. At its usual work it keeps its registry name;
/// at another, its name carries the family and work, which builds a searching agent.
#[test]
fn the_champion_is_offered_alone_and_named_with_its_work() {
    assert_eq!(
        AGENTS.iter().map(|a| a.name).collect::<Vec<_>>(),
        [CHAMPION]
    );
    assert_eq!(champion_at(DEEP_WORK), CHAMPION);
    let name = champion_at(20_000_000);
    assert_eq!(name, format!("{DEEP_SEARCHER}@20M"));
    let t = GameTable::new(Format::CONSTRUCTED, &CARDS, [VANILLA, VANILLA]).unwrap();
    assert!(agent_at_work(&name, &t, 1).is_some());
    assert!(agent_at_work(CHAMPION, &t, 1).is_none());
    assert!(agent_at_work(&format!("{DEEP_SEARCHER}@0"), &t, 1).is_none());
}
