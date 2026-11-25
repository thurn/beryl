//! The session plays each keystone that changes the shape of a turn or a game: an additional
//! upkeep or combat, an untap step skipped, a turn ended during a resolution, a turn skipped
//! for Time Vault, an extra turn, a restarted game and a subgame. Each game is a deck of the
//! keystone and the lands that cast it against a deck of lands, with the human casting and
//! activating the keystone whenever they can and every update drawn for both seats, so each
//! script is held to the replay check of a debug build (`script/build.rs`).
//!
//! At the first decision of each new state the game reaches, its save is cut off there and
//! replayed: to the same view as the live game, to the same position under each perturbation
//! a save must not depend on (`beryl_session::perturb`), and then on for one more decision.

use std::collections::BTreeSet;

use beryl_cards::{CARDS, names};
use beryl_engine::state::Pending;
use beryl_engine::{Layout, Seat, State, TurnStep};
use beryl_session::perturb::{self, Perturbation};
use beryl_session::protocol::Candidate;
use beryl_session::{
    CandidateIndex, Game, PlayerView, ReplayFile, SessionId, StartGame, Status, StopsSetting,
};

const SESSION: SessionId = SessionId(1);

/// A new state a game can be in at a decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Moment {
    /// The first decision once the keystone the human cast has left the stack.
    Resolved,
    /// The first decision once an ability of the keystone the human activated has left the
    /// stack, as Sundial of the Infinite's, which ends the turn.
    Activated,
    /// A step that already happened in this turn, as a second upkeep or combat.
    Repeated,
    /// A player asked whether to skip the turn that would begin, as for Time Vault.
    TurnStart,
    /// A turn taken by the player who took the turn before it.
    ExtraTurn,
    /// The first decision of a restarted game.
    Restarted,
    /// A decision in a subgame.
    Subgame,
}

/// A keystone's game: the human's deck is `lands` of `land` and the keystone for the rest, and
/// the game must reach each of `expect`.
struct Keystone {
    card: &'static str,
    land: &'static str,
    lands: u8,
    expect: &'static [Moment],
}

const KEYSTONES: &[Keystone] = &[
    Keystone {
        card: names::PARADOX_HAZE,
        land: names::ISLAND,
        lands: 20,
        expect: &[Moment::Resolved, Moment::Repeated],
    },
    Keystone {
        card: names::STASIS,
        land: names::ISLAND,
        lands: 20,
        expect: &[Moment::Resolved],
    },
    Keystone {
        card: names::RELENTLESS_ASSAULT,
        land: names::MOUNTAIN,
        lands: 20,
        expect: &[Moment::Resolved, Moment::Repeated],
    },
    Keystone {
        card: names::TIME_STOP,
        land: names::ISLAND,
        lands: 24,
        expect: &[Moment::Resolved],
    },
    Keystone {
        card: names::SUNDIAL_OF_THE_INFINITE,
        land: names::ISLAND,
        lands: 20,
        expect: &[Moment::Resolved, Moment::Activated],
    },
    Keystone {
        card: names::TIME_VAULT,
        land: names::ISLAND,
        lands: 20,
        expect: &[
            Moment::Resolved,
            Moment::TurnStart,
            Moment::Activated,
            Moment::ExtraTurn,
        ],
    },
    Keystone {
        card: names::FINAL_FORTUNE,
        land: names::MOUNTAIN,
        lands: 20,
        expect: &[Moment::Resolved, Moment::ExtraTurn],
    },
    Keystone {
        card: names::KARN_LIBERATED,
        land: names::ISLAND,
        lands: 26,
        expect: &[Moment::Resolved, Moment::Restarted],
    },
    Keystone {
        card: names::SHAHRAZAD,
        land: names::PLAINS,
        lands: 20,
        expect: &[Moment::Subgame],
    },
];

/// What the human prefers among a decision's candidates, the highest first: keep a hand, play
/// a land, cast the keystone, then activate it. Karn Liberated's restart comes before his +4,
/// and his −3 never, so he reaches the restart. In a subgame the human casts nothing, so that
/// it does not start another.
fn rank(candidate: &Candidate, card: &str, in_subgame: bool) -> i32 {
    match *candidate {
        Candidate::Keep => 6,
        Candidate::PlayLand { .. } => 5,
        Candidate::Cast { .. } if !in_subgame => 4,
        Candidate::Activate { ability, .. } if card == names::KARN_LIBERATED => match ability {
            2 => 5,
            0 => 3,
            _ => -1,
        },
        Candidate::Activate { .. } => 3,
        Candidate::Object { .. } | Candidate::Player { .. } | Candidate::Yes => 1,
        _ => 0,
    }
}

/// The candidate the human takes: the first of those [`rank`] puts highest.
fn preferred(options: &[&Candidate], card: &str, in_subgame: bool) -> usize {
    (0..options.len())
        .max_by_key(|&i| (rank(options[i], card, in_subgame), std::cmp::Reverse(i)))
        .expect("a decision has a candidate")
}

/// The new states a decision at `now` is in, besides the keystone resolving: `last` is the
/// position at the first decision of the turn before.
fn moments(last: Option<&Position>, now: &Position, turn_start: bool) -> Vec<Moment> {
    let mut moments = Vec::new();
    if now.depth > 0 {
        moments.push(Moment::Subgame);
    }
    if !now.first && now.step != TurnStep::Cleanup {
        moments.push(Moment::Repeated);
    }
    if turn_start {
        moments.push(Moment::TurnStart);
    }
    if let Some(last) = last
        && last.depth == now.depth
    {
        if now.turn < last.turn {
            moments.push(Moment::Restarted);
        } else if now.turn > last.turn && now.active == last.active && now.turn > 1 {
            moments.push(Moment::ExtraTurn);
        }
    }
    moments
}

/// Where a game stands, as far as what a save must not depend on lets two replays agree.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Position {
    depth: usize,
    turn: u16,
    step: TurnStep,
    active: Seat,
    first: bool,
    life: [i16; 2],
    library: [usize; 2],
    graveyard: [usize; 2],
    battlefield: usize,
    stack: usize,
}

fn position_of<L: Layout>(s: &State<L>) -> Position {
    Position {
        depth: s.subgame_depth(),
        turn: s.turn().number(),
        step: s.turn().step(),
        active: s.turn().active(),
        first: s.current_step_is_first(),
        life: Seat::BOTH.map(|p| s.player(p).life()),
        library: Seat::BOTH.map(|p| s.library(p).len()),
        graveyard: Seat::BOTH.map(|p| s.graveyard(p).len()),
        battlefield: s.battlefield().len(),
        stack: s.stack().len(),
    }
}

fn starting_turn<L: Layout>(s: &State<L>) -> bool {
    matches!(s.pending(), Pending::TurnStart(_))
}

/// The game's position, and whether a turn's start is waiting on a choice.
fn position(game: &Game) -> (Position, bool) {
    match game.large() {
        Some(s) => (position_of(s), starting_turn(s)),
        None => (position_of(game.state()), starting_turn(game.state())),
    }
}

/// Cuts `game`'s save off at the decision `view` shows and replays it: to `view` itself, to
/// the same position under each perturbation, and on through one more decision.
fn cut(game: &Game, view: &PlayerView, card: &str, moment: Moment) {
    let file: ReplayFile =
        serde_json::from_str(&serde_json::to_string(game.replay()).unwrap()).unwrap();
    let (mut replayed, again) = Game::replay_exactly(&CARDS, &file, SESSION)
        .unwrap_or_else(|e| panic!("{card}, {moment:?}: the save does not replay: {e}"));
    assert_eq!(&again, view, "{card}, {moment:?}: the replayed view");
    let (at, _) = position(game);
    assert_eq!(position(&replayed).0, at, "{card}, {moment:?}");
    for (name, _, perturbation) in Perturbation::EACH {
        let (perturbed, _) = perturb::with(perturbation, || {
            Game::replay_exactly(&CARDS, &file, SESSION)
        })
        .unwrap_or_else(|e| panic!("{card}, {moment:?}, {name}: {e}"));
        assert_eq!(position(&perturbed).0, at, "{card}, {moment:?}, {name}");
    }
    let open = replayed.open().expect("the replayed game is at a decision");
    let answer = open.answer(open.id(), CandidateIndex(0)).unwrap();
    replayed.choose(answer);
    replayed.opponent_view();
    assert!(
        replayed.next_view().is_some(),
        "{card}, {moment:?}: the replayed game plays on"
    );
}

/// Plays `keystone`'s game until it has reached and cut each moment it expects.
fn plays(keystone: &Keystone) {
    let card = keystone.card;
    let start = StartGame {
        seed: 7,
        decks: [
            vec![
                (keystone.lands, keystone.land.to_owned()),
                (40 - keystone.lands, card.to_owned()),
            ],
            vec![(40, keystone.land.to_owned())],
        ],
        human: Seat::P0,
        opponent: "random".to_owned(),
        stops: StopsSetting::EXACT,
    };
    let mut game = Game::new(&CARDS, &start, SESSION).unwrap();
    let mut seen = BTreeSet::new();
    // What the stack emptying next means: the keystone the human cast or activated resolved.
    let mut using: Option<Moment> = None;
    let mut last: Option<Position> = None;
    for _ in 0..4000 {
        if keystone.expect.iter().all(|m| seen.contains(m)) {
            return;
        }
        let Some(view) = game.next_view() else {
            break;
        };
        game.opponent_view();
        if matches!(view.status, Status::Over { .. }) {
            break;
        }
        let Status::Deciding { decision } = &view.status else {
            continue;
        };
        let (now, turn_start) = position(&game);
        let mut moments = moments(last.as_ref(), &now, turn_start);
        if now.stack == 0
            && let Some(used) = using.take()
        {
            moments.push(used);
        }
        let mut conceded = false;
        for moment in moments {
            if keystone.expect.contains(&moment) && seen.insert(moment) {
                cut(&game, &view, card, moment);
                if moment == Moment::Subgame {
                    // The subgame would play out to an empty library; the cut is what is tested.
                    game.concede();
                    conceded = true;
                }
            }
        }
        if last
            .as_ref()
            .is_none_or(|l| (l.turn, l.depth) != (now.turn, now.depth))
        {
            last = Some(now.clone());
        }
        if conceded {
            continue;
        }
        let Some(open) = game.open() else {
            continue;
        };
        let options: Vec<&Candidate> = decision.candidates.iter().map(|c| &c.option).collect();
        let index = preferred(&options, card, now.depth > 0);
        let chosen = options[index];
        match chosen {
            Candidate::Cast { .. } => using = Some(Moment::Resolved),
            Candidate::Activate { .. } => using = Some(Moment::Activated),
            _ => {}
        }
        let answer = open
            .answer(open.id(), CandidateIndex(u16::try_from(index).unwrap()))
            .unwrap();
        game.choose(answer);
    }
    let missing: Vec<_> = keystone
        .expect
        .iter()
        .filter(|m| !seen.contains(m))
        .collect();
    assert!(missing.is_empty(), "{card}: never reached {missing:?}");
}

#[test]
fn saves_cut_in_each_new_turn_or_game_state_replay_under_each_perturbation() {
    for keystone in KEYSTONES {
        plays(keystone);
    }
}
