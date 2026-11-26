//! The agents play the Dandân deck (`decks::FORGETFUL_FISH`, in `Format::SHARED_DECK`, where
//! both players draw from one library and share one graveyard). Its cards ask what no WU
//! Control card does: a word for a text-changing effect to replace and the word to replace it
//! with (Magical Hack, Crystal Spray), and a card name (Predict). The positions below check the
//! rollout policy's rule for each question, and that every agent answers it legally.

use std::panic::AssertUnwindSafe;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use beryl_cards::decks::FORGETFUL_FISH;
use beryl_cards::{CARDS, names};
use beryl_engine::card::{ChosenKind, Purpose, Subtype, TextWord};
use beryl_engine::chosen::ChosenValue;
use beryl_engine::{
    Choice, Decision, Format, GameTable, Opt, Outcome, Profile, Replay, Scratch, Small, State,
    Step, apply, next, observe,
};
use beryl_scenario::prelude::{ME, Resolve, THEM};
use beryl_scenario::{Action, Live, P0, P1, Scenario};
use beryl_search::{AGENTS, AgentDecision, GameAgent, RETIRED, RolloutPolicy};

use crate::common::{cheap, games, seeds};

/// Every agent of [`AGENTS`] plays games 0 to 199 of the Dandân deck against the random agent,
/// taking the first seat on even seeds. Each game must end in a win or a draw: an answer
/// outside the decision's options, a broken invariant, a stall or a panic fails the test, which
/// lists every failed game with its agent and seed. The games run on every core, since they
/// take about half an hour of CPU in a debug build. `BERYL_FUZZ_GAMES` and
/// `BERYL_FUZZ_FIRST_SEED` pick other seeds.
#[test]
#[ignore = "1,000 games with searching agents, minutes on every core: run it by name"]
fn every_agent_plays_the_dandan_deck_against_random() {
    let t = GameTable::new(Format::SHARED_DECK, &CARDS, [FORGETFUL_FISH, &[]]).unwrap();
    let random = RETIRED.iter().find(|a| a.name == "random").unwrap();
    let games: Vec<_> = AGENTS
        .iter()
        .flat_map(|agent| seeds(games(200)).map(move |seed| (agent, seed)))
        .collect();
    let taken = AtomicUsize::new(0);
    let failed = Mutex::new(Vec::new());
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                while let Some(&(agent, seed)) = games.get(taken.fetch_add(1, Ordering::Relaxed)) {
                    let ours = cheap(&t, agent.name, agent.new, seed);
                    let theirs = cheap(&t, random.name, random.new, seed + 1);
                    let agents = if seed.is_multiple_of(2) {
                        [ours, theirs]
                    } else {
                        [theirs, ours]
                    };
                    let played =
                        std::panic::catch_unwind(AssertUnwindSafe(|| legal_game(&t, agents, seed)));
                    let failure = match played {
                        Ok(Outcome::Win(_) | Outcome::Draw) => continue,
                        Ok(outcome) => format!("{outcome:?}"),
                        Err(panic) => {
                            let text = panic
                                .downcast_ref::<String>()
                                .map(String::as_str)
                                .or_else(|| panic.downcast_ref::<&str>().copied());
                            format!("panicked: {}", text.unwrap_or("?"))
                        }
                    };
                    let failure = format!("{} seed {seed}: {failure}", agent.name);
                    failed.lock().unwrap().push(failure);
                }
            });
        }
    });
    let failed = failed.into_inner().unwrap();
    assert!(
        failed.is_empty(),
        "{} failed:\n{}",
        failed.len(),
        failed.join("\n")
    );
}

/// Plays game `seed` of `t` between `agents` to its end, under the exact profile on even seeds
/// and smart stops on odd ones, checking the invariants at every decision and that each answer
/// is one of its options.
fn legal_game(t: &GameTable, mut agents: [Box<dyn GameAgent + Send>; 2], seed: u64) -> Outcome {
    let profile = if seed.is_multiple_of(2) {
        Profile::EXACT
    } else {
        Profile::SMART
    };
    let mut s: State<Small> = State::new(t, seed, profile).unwrap();
    let mut x = Scratch::new();
    let mut asked = [AgentDecision(0); 2];
    loop {
        match next(&mut s, t, &mut x, &mut Replay) {
            Step::Over(outcome) => return outcome,
            Step::Decide(d) => {
                s.check_invariants(t).unwrap_or_else(|v| panic!("{v:?}"));
                let i = d.decider.index();
                let c = agents[i].choose(observe(&s, t, &mut x, d.decider), &d, asked[i].take());
                assert!(usize::from(c.0) < d.len(), "{c:?} for {d:?}");
                apply(&mut s, t, &mut x, &mut Replay, c);
            }
        }
    }
}

/// The open decision of `live`, the one `next` would answer, with the rollout policy's answer
/// to it, after checking that every agent of [`AGENTS`] answers it with one of its options.
fn answers(live: &mut Live, next: Action) -> (Decision<Small>, Choice) {
    let Step::Decide(d) = live.position(next).unwrap() else {
        panic!("the game ended");
    };
    let (t, s) = (live.table(), live.state());
    let mut x = Scratch::new();
    for (i, agent) in AGENTS.iter().enumerate() {
        let mut agent = cheap(t, agent.name, agent.new, i as u64);
        let c = agent.choose(observe(s, t, &mut x, d.decider), &d, AgentDecision(0));
        assert!(usize::from(c.0) < d.len(), "{c:?} for {d:?}");
    }
    let c = RolloutPolicy::new(t).choose(t, observe(s, t, &mut x, d.decider), &d);
    assert!(usize::from(c.0) < d.len(), "{c:?} for {d:?}");
    (d, c)
}

/// The text word option `c` of `d` names.
fn word(d: &Decision<Small>, c: Choice) -> TextWord {
    match d.candidates.option(c) {
        Some(Opt::Number(n)) => TextWord::of_number(n).unwrap(),
        other => panic!("{other:?}"),
    }
}

/// Answers a text-changing effect's two questions about the Dandân in `scenario` as the
/// rollout policy does, checking each answer against the word given.
fn change_dandan(scenario: &Scenario, from: Subtype, to: Subtype) {
    let mut live = Live::new(&CARDS, scenario).unwrap();
    live.act(ME.casts(names::MAGICAL_HACK)).unwrap();
    live.act(ME.targets(names::DANDAN)).unwrap();
    live.act(Resolve).unwrap();
    let probe = ME.chooses_word(TextWord::LandType(from));
    let (d, c) = answers(&mut live, probe);
    assert_eq!(d.prompt.purpose, Purpose::ReplaceWord);
    assert_eq!(word(&d, c), TextWord::LandType(from), "{}", scenario.name);
    live.act(ME.chooses_word(word(&d, c))).unwrap();
    let (d, c) = answers(&mut live, ME.chooses_word(TextWord::LandType(to)));
    assert_eq!(d.prompt.purpose, Purpose::NewWord);
    assert_eq!(word(&d, c), TextWord::LandType(to), "{}", scenario.name);
}

/// On the player's own Dandân, which "can't attack unless defending player controls an
/// Island" and is sacrificed "when you control no Islands", the policy replaces "Island", the
/// land type its text names, with the land type both players control: Swamp, which lets it
/// attack and keeps it on the battlefield.
#[test]
fn a_word_on_your_own_creature_lifts_its_restriction() {
    const OWN: Scenario = Scenario::new("Magical Hack on your own Dandân")
        .battlefield(
            P0,
            &[names::DANDAN, names::ISLAND, names::ISLAND, names::SWAMP],
        )
        .battlefield(P1, &[names::SWAMP, names::SWAMP])
        .hand(P0, &[names::MAGICAL_HACK]);
    change_dandan(&OWN, Subtype::Island, Subtype::Swamp);
}

/// On the opponent's Dandân the policy replaces "Island" with the land type with the fewest
/// permanents, the first of equals: Mountain, which nobody controls, so the Dandân cannot
/// attack and is sacrificed.
#[test]
fn a_word_on_the_opponents_creature_adds_a_restriction() {
    const THEIRS: Scenario = Scenario::new("Magical Hack on the opponent's Dandân")
        .battlefield(P0, &[names::ISLAND, names::SWAMP])
        .battlefield(P1, &[names::DANDAN, names::ISLAND, names::PLAINS])
        .hand(P0, &[names::MAGICAL_HACK]);
    change_dandan(&THEIRS, Subtype::Island, Subtype::Mountain);
}

/// The shared library, top first: two Islands and a Dandân to draw, then six Counterspells,
/// the card most of the library holds.
const LIBRARY: &[&str] = &[
    names::ISLAND,
    names::ISLAND,
    names::DANDAN,
    names::COUNTERSPELL,
    names::COUNTERSPELL,
    names::COUNTERSPELL,
    names::COUNTERSPELL,
    names::COUNTERSPELL,
    names::COUNTERSPELL,
];

const PREDICT: Scenario = Scenario::new("Predict")
    .shared_library(LIBRARY)
    .battlefield(P0, &[names::ISLAND, names::ISLAND, names::ISLAND])
    .hand(P0, &[names::BRAINSTORM, names::PREDICT]);

/// The card name the rollout policy chooses for Predict from where `live` stands.
fn predicted(live: &mut Live) -> &'static str {
    live.act(ME.casts(names::PREDICT)).unwrap();
    live.act(ME.targets_player(THEM)).unwrap();
    live.act(Resolve).unwrap();
    let (d, c) = answers(live, ME.names(names::ISLAND));
    assert_eq!(d.prompt.purpose, Purpose::CardName);
    let Some(Opt::Number(n)) = d.candidates.option(c) else {
        panic!("{d:?}");
    };
    (ChosenValue::<Small>::of_number(ChosenKind::CardName, n).unwrap())
        .card_name(live.table())
        .unwrap()
}

/// Not knowing the library's top card, the policy names the card the deck list has most
/// copies of that it has not seen leave the library: Counterspell.
#[test]
fn a_card_name_is_the_most_numerous_card_left() {
    let mut live = Live::new(&CARDS, &PREDICT).unwrap();
    assert_eq!(predicted(&mut live), names::COUNTERSPELL);
}

/// After Brainstorm puts an Island and then a Dandân back, the player knows the Dandân is on
/// top of the library, and the policy names it.
#[test]
fn a_card_name_is_the_known_top_card() {
    let mut live = Live::new(&CARDS, &PREDICT).unwrap();
    live.act(ME.casts(names::BRAINSTORM)).unwrap();
    live.act(Resolve).unwrap();
    live.act(ME.picks(names::ISLAND)).unwrap();
    live.act(ME.picks(names::DANDAN)).unwrap();
    assert_eq!(predicted(&mut live), names::DANDAN);
}
