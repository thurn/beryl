//! Passing on identical triggers (`Profile::passes_identical_triggers`) changes only who is
//! asked, never the game: a seeded game played with the flag reaches the state of the same game
//! played without it, where each pass the flag made silently is made as an explicit
//! `Opt::Pass`.

use beryl_cards::{CARDS, names};
use beryl_engine::{
    Choice, Decision, Format, GameTable, Opt, Outcome, Profile, Replay, Scratch, State, Step,
    apply, next, observe,
};
use beryl_search::{Agent, AgentDecision, RandomAgent};

/// Four Angel's Feather ("Whenever a player casts a white spell, you may gain 1 life") a side,
/// white and blue instants to cast and to answer them with, so that a white spell puts a run of
/// identical triggers on the stack while both players hold something to do.
const FEATHERS: &[(u8, &str)] = &[
    (4, names::ANGELS_FEATHER),
    (4, names::MANA_TITHE),
    (4, names::REPRIEVE),
    (4, names::COUNTERSPELL),
    (22, names::PLAINS),
    (22, names::ISLAND),
];

/// The smart stops without passing on identical triggers.
const ASKED: Profile = Profile {
    passes_identical_triggers: [false; 2],
    ..Profile::SMART
};

/// The next decision, or the outcome.
fn step(s: &mut State, t: &GameTable, x: &mut Scratch) -> Result<Decision, Outcome> {
    match next(s, t, x, &mut Replay) {
        Step::Decide(d) => Ok(d),
        Step::Over(outcome) => Err(outcome),
    }
}

/// The position of `s` as the other game would hold it: under `ASKED`'s profile, with no
/// player noted as passing on identical triggers, and without the count of decisions this
/// turn, which the flag lowers.
fn position(s: &State) -> u64 {
    let mut s = *s;
    s.note_resolving(false);
    s.set_profile(ASKED);
    s.canonical_hash()
}

/// Plays game `seed` with the flag (`flagged`) and without it (`asked`) in lockstep, random
/// agents choosing from the flagged game's decisions. Where the game without the flag asks a
/// priority decision the flagged game passed silently, it passes explicitly. Returns how many
/// passes it made so, after checking both games end in the same state.
fn explicit_passes(t: &GameTable, seed: u64) -> usize {
    let mut flagged = State::new(t, seed, Profile::SMART).unwrap();
    let mut asked = State::new(t, seed, ASKED).unwrap();
    let (mut xf, mut xa) = (Scratch::new(), Scratch::new());
    let mut agents = [RandomAgent::new(seed * 2), RandomAgent::new(seed * 2 + 1)];
    let mut taken = [AgentDecision(0); 2];
    let mut passes = 0;
    loop {
        let ahead = step(&mut flagged, t, &mut xf);
        // Catch up: every decision the flagged game did not ask is a priority with something
        // on the stack, answered by passing.
        let behind = loop {
            let d = step(&mut asked, t, &mut xa);
            if position(&asked) == position(&flagged) {
                break d;
            }
            let d =
                d.unwrap_or_else(|o| panic!("seed {seed}: ended behind the flagged game: {o:?}"));
            assert_eq!(
                d.candidates.option(Choice(0)),
                Some(Opt::Pass),
                "seed {seed}"
            );
            assert!(
                !asked.stack().is_empty(),
                "seed {seed}: skipped with an empty stack"
            );
            apply(&mut asked, t, &mut xa, &mut Replay, Choice(0));
            passes += 1;
        };
        match (ahead, behind) {
            (Err(a), Err(b)) => {
                assert_eq!(a, b, "seed {seed}");
                return passes;
            }
            (Ok(a), Ok(b)) => {
                assert_eq!(
                    (a.player, a.kind, a.len()),
                    (b.player, b.kind, b.len()),
                    "seed {seed}"
                );
                let i = a.decider.index();
                let c = agents[i].choose(
                    observe(&flagged, t, &mut xf, a.decider),
                    &a,
                    taken[i].take(),
                );
                apply(&mut flagged, t, &mut xf, &mut Replay, c);
                apply(&mut asked, t, &mut xa, &mut Replay, c);
            }
            (a, b) => panic!("seed {seed}: {a:?} against {b:?}"),
        }
    }
}

#[test]
fn passing_on_identical_triggers_changes_no_game() {
    let t = GameTable::new(Format::CONSTRUCTED, &CARDS, [FEATHERS, FEATHERS]).unwrap();
    let passes: usize = (1..=4).map(|seed| explicit_passes(&t, seed)).sum();
    assert!(passes > 0, "no game passed on an identical trigger");
}
