//! A recorded game can pause with the spell that created a subgame still resolving.

use beryl_card::{Ability, CardDef, Face, Says, Types};
use beryl_engine::card::Flow;
use beryl_engine::{CardTable, CustomBody, Game as RulesGame, Layout, Resolution, custom_body};
use beryl_session::protocol::Candidate;
use beryl_session::replay::Entry;
use beryl_session::{
    CandidateIndex, Game, PlayerView, ReplayFile, Seat, SessionId, StartGame, Status, StopsSetting,
};

const SESSION: SessionId = SessionId(1);

struct Subgame;

impl CustomBody for Subgame {
    const SAYS: Says = Says {
        starts_subgame: true,
        ..Says::NOTHING
    };

    fn resolve<L: Layout>(game: &mut RulesGame<'_, L>, resolution: &mut Resolution<'_, L>) -> Flow {
        resolution.play_subgame(game)?;
        Ok(())
    }
}

static SPELL: CardDef = CardDef {
    name: "Test Subgame Spell",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::SORCERY,
        abilities: &[Ability::spell().custom(custom_body::<Subgame>())],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static LAND: CardDef = CardDef {
    name: "Test Subgame Land",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::LAND,
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[&SPELL, &LAND]);

fn start() -> StartGame {
    StartGame {
        seed: 19,
        decks: [
            vec![(40, SPELL.name.to_owned())],
            vec![(40, LAND.name.to_owned())],
        ],
        human: Seat::P0,
        opponent: "random".to_owned(),
        stops: StopsSetting::EXACT,
        sideboards: Default::default(),
    }
}

fn new_game() -> Game {
    Game::new(&CARDS, &start(), SESSION).unwrap()
}

fn decision(game: &mut Game) -> PlayerView {
    for _ in 0..100 {
        let view = game
            .next_view()
            .expect("the game can play to an open decision");
        if matches!(view.status, Status::Deciding { .. }) {
            return view;
        }
    }
    panic!("no decision opened");
}

fn choose(game: &mut Game, view: &PlayerView, pick: impl Fn(&Candidate) -> bool) {
    let Status::Deciding { decision } = &view.status else {
        unreachable!()
    };
    let index = decision
        .candidates
        .iter()
        .position(|c| pick(&c.option))
        .unwrap_or(0);
    let open = game.open().unwrap();
    game.choose(
        open.answer(open.id(), CandidateIndex(u16::try_from(index).unwrap()))
            .unwrap(),
    );
}

fn recorded_subgame() -> (Game, PlayerView) {
    let mut game = new_game();
    for _ in 0..30 {
        let view = decision(&mut game);
        if game.replay().log.iter().any(
            |entry| matches!(entry, Entry::Checkpoint(checkpoint) if checkpoint.outer.is_some()),
        ) {
            return (game, view);
        }
        let entering = game.large().is_none();
        choose(&mut game, &view, |candidate| {
            (entering && matches!(candidate, Candidate::Cast { .. }))
                || matches!(candidate, Candidate::Keep)
        });
    }
    panic!("the game did not reach a subgame checkpoint");
}

#[test]
fn a_save_cut_off_in_a_subgame_replays_and_resumes_the_spell() {
    let (game, inner) = recorded_subgame();
    let file: ReplayFile =
        serde_json::from_str(&serde_json::to_string(game.replay()).unwrap()).unwrap();
    assert!(
        file.log
            .iter()
            .any(|e| matches!(e, Entry::SubgameStarter(_)))
    );
    let (mut replayed, view) = Game::replay_exactly(&CARDS, &file, SESSION).unwrap();
    assert_eq!(inner, view);
    let state = replayed.large().unwrap();
    assert_eq!(state.subgame_depth(), 1);
    assert_eq!(state.suspended_game().unwrap().stack().len(), 1);
    replayed.concede().expect("a concession in the subgame");
    let resumed = decision(&mut replayed);
    assert!(!matches!(resumed.status, Status::Over { .. }));
    let state = replayed.large().unwrap();
    assert_eq!(state.subgame_depth(), 0);
    assert!(state.stack().is_empty());
    assert_eq!(state.graveyard(Seat::P0).len(), 1);
    let (again, _) = Game::replay_exactly(&CARDS, replayed.replay(), SESSION).unwrap();
    assert_eq!(again.large().unwrap().subgame_depth(), 0);
}

#[test]
fn a_bad_answer_recovers_at_the_subgame_checkpoint() {
    let (game, _) = recorded_subgame();
    let mut file = game.replay().clone();
    let checkpoint = file
        .log
        .iter()
        .rposition(|e| matches!(e, Entry::Checkpoint(c) if c.outer.is_some()))
        .unwrap();
    // Keep the full log through the checkpoint. The last answer is impossible at any
    // ensuing decision.
    file.log.truncate(checkpoint + 1);
    file.log.push(Entry::Choice(beryl_session::save::Chosen {
        index: u16::MAX,
        continuation: false,
        what: None,
    }));

    assert!(Game::replay_exactly(&CARDS, &file, SESSION).is_err());
    let (mut recovered, _) = Game::from_replay(&CARDS, &file, SESSION).unwrap();
    assert_eq!(recovered.large().unwrap().subgame_depth(), 1);
    assert!(matches!(recovered.replay().log.last(),
        Some(Entry::Checkpoint(c)) if c.outer.is_some()));
    let _ = decision(&mut recovered);
    recovered.concede().unwrap();
    let _ = decision(&mut recovered);
    assert_eq!(recovered.large().unwrap().subgame_depth(), 0);
}

#[test]
fn a_subgame_concession_leaves_the_live_session_open() {
    use beryl_session::{GameUpdate, Sessions};
    use std::sync::mpsc::channel;
    use std::time::Duration;

    let (sent, received) = channel::<GameUpdate>();
    let sessions = Sessions::new(&CARDS, None, move |update| {
        let _ = sent.send(update);
    });
    let session = sessions.start_game(&start(), None).unwrap();
    let mut cast = false;
    let mut conceded = false;
    let next_view = || {
        received
            .recv_timeout(Duration::from_secs(120))
            .expect("the session stopped publishing views")
            .view
    };
    for _ in 0..100 {
        let view = next_view();
        let Status::Deciding { decision } = view.status else {
            continue;
        };
        if cast && view.turn.number == 0 {
            sessions.concede(session).unwrap();
            conceded = true;
            break;
        }
        let index = decision
            .candidates
            .iter()
            .position(|c| {
                matches!(c.option, Candidate::Keep)
                    || (!cast && matches!(c.option, Candidate::Cast { .. }))
            })
            .unwrap_or(0);
        cast |= matches!(decision.candidates[index].option, Candidate::Cast { .. });
        sessions
            .choose(decision.id, CandidateIndex(u16::try_from(index).unwrap()))
            .unwrap();
    }
    assert!(conceded, "the session reached a subgame decision");
    for _ in 0..100 {
        let view = next_view();
        if let Status::Deciding { decision } = view.status {
            assert!(view.turn.number > 0);
            sessions.choose(decision.id, CandidateIndex(0)).unwrap();
            return;
        }
        assert!(!matches!(view.status, Status::Over { .. }));
    }
    panic!("no decision after resuming the main game");
}

static RESTART: CardDef = CardDef {
    name: "Test Restart Spell",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::SORCERY,
        abilities: &[Ability::spell().effect(&beryl_card::Effect::Sequence(&[
            beryl_card::prelude::restart_game(beryl_card::Filter::Any),
            beryl_card::prelude::lose_life(beryl_card::Players::You, 20),
        ]))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static RESTART_CARDS: CardTable = CardTable::new(&[&RESTART, &LAND]);

/// Plays into the first turn of a restarted game. A reset is sent once, and the next
/// decision after the restart keeps an opening hand without a play-or-draw choice.
fn recorded_restart() -> Game {
    let start = StartGame {
        decks: [
            vec![(39, RESTART.name.to_owned()), (1, LAND.name.to_owned())],
            vec![(40, LAND.name.to_owned())],
        ],
        ..start()
    };
    let mut game = Game::new(&RESTART_CARDS, &start, SESSION).unwrap();
    let mut cast = false;
    let mut resets = 0;
    for _ in 0..100 {
        let view = game.next_view().expect("the game can advance");
        if cast && view.turn.number == 0 {
            if view.script.as_ref().is_some_and(|s| s.reset.is_some()) {
                resets += 1;
            }
            if let Status::Deciding { decision } = &view.status {
                assert!(
                    decision
                        .candidates
                        .iter()
                        .any(|c| c.option == Candidate::Keep)
                );
                assert_eq!(view.players.you.life, 20);
                assert_eq!(view.players.opponent.life, 20);
            }
        }
        if game
            .replay()
            .log
            .iter()
            .any(|e| matches!(e, Entry::Checkpoint(c) if c.turn == 1 && c.elapsed.is_some()))
        {
            assert_eq!(resets, 1);
            assert_eq!(game.state().turn().starting_player(), Seat::P0);
            assert_eq!(game.state().turns_for_limit(), 1);
            assert_eq!(
                game.state().elapsed_turns(),
                game.state().turn().number() + 1
            );
            return game;
        }
        if let Status::Deciding { decision } = &view.status {
            let should_cast = !cast
                && decision
                    .candidates
                    .iter()
                    .any(|c| matches!(c.option, Candidate::Cast { .. }));
            choose(&mut game, &view, |candidate| {
                matches!(candidate, Candidate::Keep)
                    || (should_cast && matches!(candidate, Candidate::Cast { .. }))
            });
            cast |= should_cast;
        }
    }
    panic!("the game did not reach its first post-restart checkpoint");
}

#[test]
fn a_save_through_a_restart_replays_under_each_perturbation() {
    use beryl_session::perturb::{self, Perturbation};

    let game = recorded_restart();
    let file: ReplayFile =
        serde_json::from_str(&serde_json::to_string(game.replay()).unwrap()).unwrap();
    let (replayed, _) = Game::replay_exactly(&RESTART_CARDS, &file, SESSION).unwrap();
    assert_eq!(replayed.state().turn().number(), 1);
    for (name, _, perturbation) in Perturbation::EACH {
        let (replayed, _) = perturb::with(perturbation, || {
            Game::replay_exactly(&RESTART_CARDS, &file, SESSION)
        })
        .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(
            replayed.state().turn().starting_player(),
            Seat::P0,
            "{name}"
        );
        assert_eq!(replayed.state().turn().number(), 1, "{name}");
    }
}

#[test]
fn a_bad_answer_recovers_at_the_post_restart_checkpoint() {
    let game = recorded_restart();
    let mut file = game.replay().clone();
    file.log.push(Entry::Choice(beryl_session::save::Chosen {
        index: u16::MAX,
        continuation: false,
        what: None,
    }));
    assert!(Game::replay_exactly(&RESTART_CARDS, &file, SESSION).is_err());
    let (recovered, _) = Game::from_replay(&RESTART_CARDS, &file, SESSION).unwrap();
    assert!(matches!(recovered.replay().log.last(),
        Some(Entry::Checkpoint(c)) if c.turn == 1 && c.elapsed == Some(2)));
    assert_eq!(recovered.state().turns_for_limit(), 1);
    assert_eq!(recovered.state().elapsed_turns(), 2);
}
