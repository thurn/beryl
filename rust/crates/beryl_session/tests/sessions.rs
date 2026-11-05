//! Sessions end to end: a scripted human against an agent through the session API, the replay
//! file it leaves, and what its views may show.

use std::sync::mpsc::channel;

use beryl_cards::CARDS;
use beryl_cards::decks::{WU_CONTROL, fuzz_deck};
use beryl_engine::rng::SplitMix64;
use beryl_engine::state::Pending;
use beryl_engine::{Decision, GameTable, Observation, Replay, Scratch, State, Step, next};
use beryl_session::protocol::player_view;
use beryl_session::{
    Game, GameSession, PlayerView, ReplayFile, SessionError, StartGame, StopsSetting,
};

fn owned(deck: &[(u8, &str)]) -> Vec<(u8, String)> {
    deck.iter().map(|&(n, name)| (n, name.to_owned())).collect()
}

/// A game against the random agent between two fuzz decks drawn from `seed`.
fn start(seed: u64, stops: StopsSetting) -> StartGame {
    StartGame {
        seed,
        decks: [owned(&fuzz_deck(seed)), owned(&fuzz_deck(seed ^ 1 << 40))],
        human: 0,
        opponent: "random".to_owned(),
        stops,
    }
}

/// The scripted human: a fixed pattern over the candidates, never the agent's stream.
fn script(view: &PlayerView) -> usize {
    let n = view.decision.as_ref().unwrap().candidates.len();
    (view.seq as usize * 7 + 3) % n
}

fn without_events(mut view: PlayerView) -> PlayerView {
    view.events.clear();
    view
}

#[test]
fn a_scripted_human_plays_a_whole_game_and_its_replay_reproduces_it() {
    let dir = std::env::temp_dir().join(format!("beryl-session-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("game.json");
    let (sent, views) = channel();
    let session = GameSession::start(
        &CARDS,
        &StartGame {
            decks: [owned(WU_CONTROL), owned(WU_CONTROL)],
            ..start(11, StopsSetting::Smart)
        },
        Some(path.clone()),
        move |v| sent.send(v).unwrap(),
    )
    .unwrap();
    let mut decisions = 0;
    let last = loop {
        let view = views.recv().unwrap();
        if view.outcome.is_some() {
            break view;
        }
        assert_eq!(view.decision.as_ref().unwrap().player, 0);
        decisions += 1;
        // Halfway through, the human asks for every priority, which the replay must record.
        if decisions == 20 {
            session.set_stops(StopsSetting::Exact).unwrap();
            let again = views.recv().unwrap();
            assert_eq!((again.seq, again.stops), (view.seq, StopsSetting::Exact));
        }
        let stale = session.choose(view.seq + 1, 0);
        assert_eq!(
            stale,
            Err(SessionError::StaleDecision {
                seq: view.seq + 1,
                open: view.seq
            })
        );
        session.choose(view.seq, script(&view)).unwrap();
    };
    assert!(decisions > 40, "a short game: {decisions} decisions");
    assert!(last.decision.is_none());
    assert_eq!(session.choose(last.seq, 0), Err(SessionError::NotDeciding));

    let file = ReplayFile::load(&path).unwrap();
    assert!(file.log.len() > decisions);
    let (_, replayed) = Game::from_replay(&CARDS, &file).unwrap();
    assert_eq!(replayed, last);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_replay_cut_at_a_decision_resumes_there() {
    let mut game = Game::new(&CARDS, &start(3, StopsSetting::Exact)).unwrap();
    let mut view = game.begin();
    for _ in 0..30 {
        view = game.choose(view.seq, script(&view)).unwrap();
    }
    let (_, resumed) = Game::from_replay(&CARDS, game.replay()).unwrap();
    assert_eq!(resumed, view);
}

#[test]
fn a_stopped_game_lets_the_agent_decide_no_more() {
    let mut game = Game::new(&CARDS, &start(7, StopsSetting::Exact)).unwrap();
    let mut view = game.begin();
    // Play to a decision whose answer hands the agent the next one, found on a replayed copy.
    let agent_decides_next = |game: &Game, view: &PlayerView| {
        let (mut probe, _) = Game::from_replay(&CARDS, game.replay()).unwrap();
        probe.choose(view.seq, script(view)).unwrap();
        probe.replay().log.len() > game.replay().log.len() + 1
    };
    while !agent_decides_next(&game, &view) {
        view = game.choose(view.seq, script(&view)).unwrap();
        assert!(view.outcome.is_none(), "the agent never decided");
    }
    let before = game.replay().log.len();
    game.stop_flag()
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let stopped = game.choose(view.seq, script(&view)).unwrap();
    assert_eq!(game.replay().log.len(), before + 1, "the agent decided");
    assert!(stopped.decision.is_none() && stopped.outcome.is_none());
    assert_eq!(game.choose(stopped.seq, 0), Err(SessionError::NotDeciding));
}

#[test]
fn no_view_shows_an_identity_its_viewer_does_not_know() {
    let mut rng = SplitMix64::new(5);
    for seed in 0..6 {
        let stops = if seed % 2 == 0 {
            StopsSetting::Smart
        } else {
            StopsSetting::Exact
        };
        let mut game = Game::new(&CARDS, &start(seed, stops)).unwrap();
        let mut view = game.begin();
        while view.outcome.is_none() {
            let (s, t) = (game.state(), game.table());
            // The position the human decides in: a paused resolution's, replayed.
            let mut x = Scratch::new();
            let seen = beryl_engine::view(s, t, &mut x);
            for o in &view.objects {
                if o.name.is_some() {
                    assert!(
                        seen.knows(0, o.slot),
                        "seed {seed}: slot {} is named but unknown",
                        o.slot
                    );
                }
            }
            // A state with every unknown card dealt anew, consistent with what the human
            // knows, gives the same view: nothing in it depends on a hidden identity. A paused
            // resolution replays its body under the redealt state's own random streams, so
            // those positions are left to the check above.
            if matches!(s.pending(), Pending::Resolving(_)) {
                view = game.choose(view.seq, script(&view)).unwrap();
                continue;
            }
            let d = view_decision(&view, s, t);
            let det = Observation::new(s, 0).determinize(t, &mut rng);
            let mut x = Scratch::new();
            let theirs = player_view(
                t,
                Observation::new(beryl_engine::view(&det, t, &mut x), 0),
                view.seq,
                Some(&d),
                None,
                view.stops,
                Vec::new(),
            );
            assert_eq!(theirs, without_events(view.clone()), "seed {seed}");
            view = game.choose(view.seq, script(&view)).unwrap();
        }
    }
}

/// The open decision of a state the human is deciding in.
fn view_decision(view: &PlayerView, s: &State, t: &GameTable) -> Decision {
    let mut s = *s;
    match next(&mut s, t, &mut Scratch::new(), &mut Replay) {
        Step::Decide(d) => {
            assert_eq!(d.player, view.viewer);
            d
        }
        Step::Over(o) => panic!("the game is over: {o:?}"),
    }
}
