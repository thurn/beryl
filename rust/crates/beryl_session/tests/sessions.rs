//! Sessions end to end: a scripted human against an agent through the session API, the replay
//! file it leaves, and what its views may show.

use std::sync::mpsc::channel;

use beryl_cards::CARDS;
use beryl_cards::decks::{WU_CONTROL, fuzz_deck};
use beryl_engine::rng::SplitMix64;
use beryl_engine::state::Pending;
use beryl_engine::{
    Decision, GameTable, Observation, Replay, Scratch, Seat, Slot, State, Step, next,
};
use beryl_session::protocol::{DecisionView, Moment, player_view};
use beryl_session::{
    CandidateIndex, DecisionId, Game, PlayerView, ReplayFile, SessionError, SessionId, Sessions,
    StartGame, Status, StopsSetting,
};

fn owned(deck: &[(u8, &str)]) -> Vec<(u8, String)> {
    deck.iter().map(|&(n, name)| (n, name.to_owned())).collect()
}

/// A game against the random agent between two fuzz decks drawn from `seed`.
fn start(seed: u64, stops: StopsSetting) -> StartGame {
    StartGame {
        seed,
        decks: [owned(&fuzz_deck(seed)), owned(&fuzz_deck(seed ^ 1 << 40))],
        human: Seat::P0,
        opponent: "random".to_owned(),
        stops,
    }
}

const SESSION: SessionId = SessionId(1);

fn decision(view: &PlayerView) -> &DecisionView {
    match &view.status {
        Status::Deciding { decision } => decision,
        status => panic!("no decision is open: {status:?}"),
    }
}

/// The scripted human: a fixed pattern over the candidates, never the agent's stream.
fn script(view: &PlayerView) -> (DecisionId, CandidateIndex) {
    let d = decision(view);
    let n = d.candidates.len() as u32;
    (d.id, CandidateIndex(((d.id.seq * 7 + 3) % n) as u8))
}

/// Plays on to the human's next decision or the end, and returns the view there.
fn to_human(game: &mut Game) -> PlayerView {
    let mut last = None;
    while let Some(view) = game.next_view() {
        last = Some(view);
    }
    last.expect("the game plays on from where it is")
}

/// Plays the scripted human's answer to `view`, and on to their next decision or the end.
fn play(game: &mut Game, view: &PlayerView) -> PlayerView {
    let (id, index) = script(view);
    game.choose(game.answer(id, index).unwrap());
    to_human(game)
}

#[test]
fn a_scripted_human_plays_a_whole_game_and_its_replay_reproduces_it() {
    let dir = std::env::temp_dir().join(format!("beryl-session-{}", std::process::id()));
    let (sent, views) = channel();
    let sessions = Sessions::new(Some(dir.clone()), move |u| sent.send(u).unwrap());
    let session = sessions
        .start_game(
            &StartGame {
                decks: [owned(WU_CONTROL), owned(WU_CONTROL)],
                ..start(11, StopsSetting::Smart)
            },
            None,
        )
        .unwrap();
    let mut decisions = 0;
    let last = loop {
        let update = views.recv().unwrap();
        assert_eq!(update.session, session);
        let view = update.view;
        match view.status {
            Status::Over { .. } => break view,
            Status::Deciding { .. } => {}
            _ => continue,
        }
        decisions += 1;
        // Halfway through, the human asks for every priority, which the replay must record.
        if decisions == 20 {
            sessions.set_stops(session, StopsSetting::Exact).unwrap();
            let again = views.recv().unwrap().view;
            assert_eq!(decision(&again).id, decision(&view).id);
            assert_eq!(again.stops, StopsSetting::Exact);
        }
        let (id, index) = script(&view);
        let candidates = decision(&view).candidates.len();
        let stale = DecisionId {
            seq: id.seq + 1,
            ..id
        };
        assert_eq!(
            sessions.choose(stale, index),
            Err(SessionError::StaleDecision {
                decision: stale,
                open: id
            })
        );
        let past = CandidateIndex(candidates as u8);
        assert_eq!(
            sessions.choose(id, past),
            Err(SessionError::NoSuchCandidate {
                index: past,
                candidates
            })
        );
        let other = SessionId(session.0 + 1);
        assert_eq!(
            sessions.choose(
                DecisionId {
                    session: other,
                    ..id
                },
                index
            ),
            Err(SessionError::StaleSession { session: other })
        );
        sessions.choose(id, index).unwrap();
        // A second click on the same candidate finds the decision answered.
        assert_eq!(sessions.choose(id, index), Err(SessionError::NotDeciding));
    };
    assert!(decisions > 40, "a short game: {decisions} decisions");
    let past = DecisionId {
        session,
        seq: decisions,
    };
    assert_eq!(
        sessions.choose(past, CandidateIndex(0)),
        Err(SessionError::NotDeciding)
    );

    let replays: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
    assert_eq!(replays.len(), 1, "one replay file per game");
    let file = ReplayFile::load(&replays[0].as_ref().unwrap().path()).unwrap();
    assert!(file.log.len() > decisions as usize);
    let (_, replayed) = Game::from_replay(&CARDS, &file, session).unwrap();
    assert_eq!(replayed, last);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_replay_cut_at_a_decision_resumes_there() {
    let mut game = Game::new(&CARDS, &start(3, StopsSetting::Exact), SESSION).unwrap();
    let mut view = to_human(&mut game);
    for _ in 0..30 {
        view = play(&mut game, &view);
    }
    let (_, resumed) = Game::from_replay(&CARDS, game.replay(), SESSION).unwrap();
    assert_eq!(resumed, view);
}

#[test]
fn a_stopped_game_lets_the_agent_decide_no_more() {
    let mut game = Game::new(&CARDS, &start(7, StopsSetting::Exact), SESSION).unwrap();
    let mut view = to_human(&mut game);
    // Play to a decision whose answer hands the agent the next one, found on a replayed copy.
    let agent_decides_next = |game: &Game, view: &PlayerView| {
        let (mut probe, _) = Game::from_replay(&CARDS, game.replay(), SESSION).unwrap();
        play(&mut probe, view);
        probe.replay().log.len() > game.replay().log.len() + 1
    };
    while !agent_decides_next(&game, &view) {
        view = play(&mut game, &view);
        assert!(
            matches!(view.status, Status::Deciding { .. }),
            "the agent never decided"
        );
    }
    let before = game.replay().log.len();
    game.stop_flag()
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let stopped = play(&mut game, &view);
    assert_eq!(game.replay().log.len(), before + 1, "the agent decided");
    assert_eq!(stopped.status, Status::Stopped);
    assert_eq!(
        game.answer(decision(&view).id, CandidateIndex(0)),
        Err(SessionError::NotDeciding)
    );
}

#[test]
fn no_view_shows_an_identity_its_viewer_does_not_know() {
    let mut rng = SplitMix64::new(5);
    let mut events = 0;
    for seed in 0..6 {
        let stops = if seed % 2 == 0 {
            StopsSetting::Smart
        } else {
            StopsSetting::Exact
        };
        let mut game = Game::new(&CARDS, &start(seed, stops), SESSION).unwrap();
        while let Some(view) = game.next_view() {
            let (s, t) = (game.state(), game.table());
            // The position the human decides in: a paused resolution's, replayed.
            let mut x = Scratch::new();
            let seen = beryl_engine::view(s, t, &mut x);
            for o in &view.objects {
                if o.name.is_some() {
                    assert!(
                        seen.knows(Seat::P0, Slot::new(o.slot)),
                        "seed {seed}: slot {} is named but unknown",
                        o.slot
                    );
                }
            }
            // A state with every unknown card dealt anew, consistent with what the human
            // knows, gives the same view, events included: nothing in it depends on a hidden
            // identity. The redealt view converts the same engine events, so this covers what
            // a conversion reads through the observation; engine events name objects by slot
            // only, so that is all an identity could come from. A paused resolution replays
            // its body under the redealt state's own random streams, so those positions are
            // left to the check above.
            let resolving = matches!(s.pending(), Pending::Resolving(_));
            if resolving && let Status::Deciding { decision } = &view.status {
                // The paused resolution's entry, the top of the committed stack, is named.
                assert!(
                    decision.resolving.is_some(),
                    "seed {seed}: a paused resolution without its entry"
                );
            }
            if !resolving {
                let d;
                let moment = match &view.status {
                    Status::Deciding { decision } if decision.halt => Moment::Halted(decision.id),
                    Status::Deciding { decision } => {
                        d = open_decision(s, t);
                        Moment::Deciding(decision.id, &d, None)
                    }
                    Status::Thinking => Moment::Thinking,
                    Status::Playing => Moment::Playing,
                    // The end shows what the human knows by then; the game is over.
                    Status::Over { .. } | Status::Stopped => break,
                };
                let det = Observation::new(s, Seat::P0).determinize(t, &mut rng);
                let mut x = Scratch::new();
                let theirs = player_view(
                    t,
                    Observation::new(beryl_engine::view(&det, t, &mut x), Seat::P0),
                    moment,
                    view.stops,
                    game.events(),
                );
                assert_eq!(theirs, view, "seed {seed}");
                events += view.events.len();
            }
            if matches!(view.status, Status::Deciding { .. }) {
                let (id, index) = script(&view);
                game.choose(game.answer(id, index).unwrap());
            }
        }
    }
    assert!(events > 1000, "only {events} events compared");
}

/// The open decision of a state the human (seat 0) is deciding in.
fn open_decision(s: &State, t: &GameTable) -> Decision {
    let mut s = *s;
    match next(&mut s, t, &mut Scratch::new(), &mut Replay) {
        Step::Decide(d) => {
            assert_eq!(d.player, Seat::P0);
            d
        }
        Step::Over(o) => panic!("the game is over: {o:?}"),
    }
}
