//! Sessions end to end: a scripted human against an agent through the session API, the replay
//! file it leaves, and what its views may show.

use std::sync::mpsc::channel;

use beryl_cards::CARDS;
use beryl_cards::decks::{WU_CONTROL, fuzz_deck};
use beryl_engine::rng::SplitMix64;
use beryl_engine::state::Pending;
use beryl_engine::{
    Decision, GameTable, Observation, Relayout, Replay, Scratch, Seat, Slot, SlotId, State, Step,
    next,
};
use beryl_session::preview::add_previews;
use beryl_session::protocol::{
    DecisionView, GameEvent, LossReason, Moment, OutcomeView, Side, StackItem, player_view,
};
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
    (d.id, CandidateIndex(((d.id.seq * 7 + 3) % n) as u16))
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
                ..start(11, StopsSetting::SMART)
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
            sessions.set_stops(session, StopsSetting::EXACT).unwrap();
            let again = views.recv().unwrap().view;
            assert_eq!(decision(&again).id, decision(&view).id);
            assert_eq!(again.stops, StopsSetting::EXACT);
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
        let past = CandidateIndex(candidates as u16);
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
        // A second click on the same candidate finds the decision answered: none open yet, or
        // already the next one, if the game got there first.
        let again = sessions.choose(id, index);
        let answered = match again {
            Err(SessionError::NotDeciding) => true,
            Err(SessionError::StaleDecision { decision, open }) => {
                decision == id && open.seq > id.seq
            }
            _ => false,
        };
        assert!(answered, "{again:?}");
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
    // The end says why the loser lost.
    if let Status::Over {
        outcome: OutcomeView::Win { winner, reason, .. },
    } = last.status
    {
        let loser = match winner {
            Side::You => &last.players.opponent,
            Side::Opponent => &last.players.you,
        };
        let by_life = loser.life <= 0;
        assert_eq!(reason == LossReason::Life, by_life, "{reason:?}");
        assert!(by_life || reason == LossReason::Library || reason == LossReason::Effect);
    }
}

/// The views a session emits until the next that asks the human something or ends the game.
/// A session that stops emitting fails the test rather than hanging it (the limit is far
/// beyond any machine's time for these games against the random agent).
fn to_decision(views: &std::sync::mpsc::Receiver<beryl_session::GameUpdate>) -> PlayerView {
    loop {
        let view = views
            .recv_timeout(std::time::Duration::from_secs(120))
            .expect("the session emits a view")
            .view;
        if matches!(view.status, Status::Deciding { .. } | Status::Over { .. }) {
            return view;
        }
    }
}

#[test]
fn the_latest_view_of_the_game_in_progress_is_kept_for_a_reloaded_ui() {
    let (sent, views) = channel();
    let sessions = Sessions::new(None, move |u| sent.send(u).unwrap());
    assert_eq!(sessions.current(), None, "no game");
    let first = sessions
        .start_game(&start(2, StopsSetting::SMART), None)
        .unwrap();
    let view = to_decision(&views);
    // The session waits on the human, so nothing follows the decision.
    let current = sessions.current().unwrap();
    assert_eq!((current.session, &current.view), (first, &view));
    let (id, index) = script(&view);
    sessions.choose(id, index).unwrap();
    let view = to_decision(&views);
    assert_eq!(sessions.current().unwrap().view, view);

    // A new game's views replace the old game's.
    let second = sessions
        .start_game(&start(3, StopsSetting::SMART), None)
        .unwrap();
    assert!(sessions.current().is_none_or(|u| u.session == second));
    let view = loop {
        let update = views
            .recv_timeout(std::time::Duration::from_secs(120))
            .expect("the session emits a view");
        if update.session == second && matches!(update.view.status, Status::Deciding { .. }) {
            break update.view;
        }
    };
    assert_eq!(sessions.current().unwrap().view, view);
}

#[test]
fn a_concession_ends_the_game_in_defeat_and_its_replay_reproduces_it() {
    let dir = std::env::temp_dir().join(format!("beryl-concede-{}", std::process::id()));
    let (sent, views) = channel();
    let sessions = Sessions::new(Some(dir.clone()), move |u| sent.send(u).unwrap());
    let session = sessions
        .start_game(&start(4, StopsSetting::SMART), None)
        .unwrap();
    let mut view = to_decision(&views);
    for _ in 0..10 {
        let (id, index) = script(&view);
        sessions.choose(id, index).unwrap();
        view = to_decision(&views);
    }
    assert!(matches!(view.status, Status::Deciding { .. }));
    sessions.concede(session).unwrap();
    let over = to_decision(&views);
    assert_eq!(
        over.status,
        Status::Over {
            outcome: OutcomeView::Win {
                winner: Side::Opponent,
                reason: LossReason::Concede,
                by: None,
            }
        }
    );
    assert!(
        over.events
            .iter()
            .any(|e| matches!(e, GameEvent::Lose { player: Side::You })),
        "the log hears of the loss"
    );
    assert_eq!(sessions.current().unwrap().view, over);
    // The decision open when they conceded is closed.
    let (id, index) = script(&view);
    assert_eq!(sessions.choose(id, index), Err(SessionError::NotDeciding));

    let replays: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
    let file = ReplayFile::load(&replays[0].as_ref().unwrap().path()).unwrap();
    let (_, replayed) = Game::from_replay(&CARDS, &file, session).unwrap();
    assert_eq!(replayed.status, over.status);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_concession_is_taken_where_the_game_waits_on_a_decision() {
    let mut game = Game::new(&CARDS, &start(6, StopsSetting::EXACT), SESSION).unwrap();
    let mut seen = (false, false);
    loop {
        let view = game
            .next_view()
            .expect("the agent decides before the game ends");
        match view.status {
            // A step began and the game plays on: there is no decision to concede at.
            Status::Playing => {
                assert_eq!(game.concede(), None);
                seen.0 = true;
            }
            // The agent decides next; it never does.
            Status::Thinking if seen.0 => {
                let before = game.replay().log.len();
                let over = game.concede().unwrap();
                assert!(matches!(
                    over.status,
                    Status::Over {
                        outcome: OutcomeView::Win {
                            winner: Side::Opponent,
                            reason: LossReason::Concede,
                            by: None,
                        }
                    }
                ));
                assert_eq!(
                    game.replay().log.len(),
                    before + 1,
                    "the agent never decided"
                );
                assert_eq!(game.next_view(), None, "the game is over");
                assert_eq!(game.concede(), None, "a game ends once");
                seen.1 = true;
                break;
            }
            Status::Deciding { .. } => {
                let (id, index) = script(&view);
                game.choose(game.answer(id, index).unwrap());
            }
            _ => {}
        }
    }
    assert!(seen.1);
    let (_, replayed) = Game::from_replay(&CARDS, game.replay(), SESSION).unwrap();
    assert!(matches!(replayed.status, Status::Over { .. }));
}

#[test]
fn a_replay_cut_at_a_decision_resumes_there() {
    let mut game = Game::new(&CARDS, &start(3, StopsSetting::EXACT), SESSION).unwrap();
    let mut view = to_human(&mut game);
    for _ in 0..30 {
        view = play(&mut game, &view);
    }
    let (_, resumed) = Game::from_replay(&CARDS, game.replay(), SESSION).unwrap();
    assert_eq!(resumed, view);
}

/// A fresh directory for one test's replay files.
fn replay_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("beryl-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// The one replay file in `dir`.
fn only_replay(dir: &std::path::Path) -> std::path::PathBuf {
    let files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");
    files[0].clone()
}

/// Answers `n` of the human's decisions through `sessions` by the script, from `view`, and
/// returns each view they were asked at after it.
fn answer(
    sessions: &Sessions,
    views: &std::sync::mpsc::Receiver<beryl_session::GameUpdate>,
    mut view: PlayerView,
    n: usize,
) -> Vec<PlayerView> {
    let mut seen = Vec::new();
    for _ in 0..n {
        let (id, index) = script(&view);
        sessions.choose(id, index).unwrap();
        view = to_decision(views);
        seen.push(view.clone());
    }
    seen
}

#[test]
fn a_game_resumed_from_its_save_plays_on_as_if_it_never_stopped() {
    // The same game played through without stopping.
    let mut game = Game::new(&CARDS, &start(3, StopsSetting::EXACT), SESSION).unwrap();
    let mut expected = vec![to_human(&mut game)];
    for _ in 0..30 {
        let next = play(&mut game, expected.last().unwrap());
        expected.push(next);
    }

    let dir = replay_dir("resume");
    let (sent, views) = channel();
    let first = Sessions::new(Some(dir.clone()), move |u| sent.send(u).unwrap());
    first
        .start_game(&start(3, StopsSetting::EXACT), None)
        .unwrap();
    let mut played = vec![to_decision(&views)];
    played.extend(answer(&first, &views, played[0].clone(), 15));
    // The app quits at the human's decision.
    drop(first);
    let path = only_replay(&dir);
    let mut saved = ReplayFile::load(&path).unwrap();
    assert!(saved.commit.is_some(), "the save names its build");
    // A file from before saves named their build still resumes.
    saved.commit = None;
    saved.save(&path).unwrap();

    let (sent, views) = channel();
    let second = Sessions::new(Some(dir.clone()), move |u| sent.send(u).unwrap());
    let session = second.resume_latest().expect("the unfinished game resumes");
    assert_eq!(
        second.resume_latest(),
        Some(session),
        "a game is in progress"
    );
    let resumed = to_decision(&views);
    assert_eq!(
        &resumed,
        played.last().unwrap(),
        "it resumes where it stopped"
    );
    played.extend(answer(&second, &views, resumed, 15));
    assert_eq!(played, expected);

    // The resumed game keeps saving to its file.
    assert_eq!(only_replay(&dir), path);
    let saved = ReplayFile::load(&path).unwrap();
    assert_eq!(saved.log, game.replay().log);
    assert!(saved.commit.is_some());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn only_an_unfinished_game_that_still_replays_is_resumed() {
    let dir = replay_dir("no-resume");
    let (sent, views) = channel();
    let sessions = Sessions::new(Some(dir.clone()), move |u| sent.send(u).unwrap());
    assert_eq!(sessions.resume_latest(), None, "no replay directory");
    assert_eq!(
        Sessions::new(None, |_| {}).resume_latest(),
        None,
        "no replays at all"
    );

    // A game that ended is not resumed.
    let session = sessions
        .start_game(&start(4, StopsSetting::SMART), None)
        .unwrap();
    let view = to_decision(&views);
    answer(&sessions, &views, view, 3);
    sessions.concede(session).unwrap();
    assert!(matches!(to_decision(&views).status, Status::Over { .. }));
    drop(sessions);
    let over = only_replay(&dir);
    let (sent, views) = channel();
    let sessions = Sessions::new(Some(dir.clone()), move |u| sent.send(u).unwrap());
    assert_eq!(sessions.resume_latest(), None, "the newest game is over");
    assert_eq!(sessions.current(), None);

    // Nor is a newer file that no longer replays: the player starts a new game.
    let mut unknown = ReplayFile::load(&over).unwrap();
    unknown.log.truncate(unknown.log.len() / 2);
    unknown.decks[1][0].1 = "No Such Card".to_owned();
    let newer = dir.join("9999999998-1.json");
    unknown.save(&newer).unwrap();
    assert_eq!(sessions.resume_latest(), None, "an unknown card");
    std::fs::write(dir.join("9999999999-1.json"), "{ not a replay").unwrap();
    assert_eq!(sessions.resume_latest(), None, "a corrupt file");
    assert!(views.try_recv().is_err(), "nothing was shown");
    let session = sessions
        .start_game(&start(5, StopsSetting::SMART), None)
        .unwrap();
    assert!(matches!(
        to_decision(&views).status,
        Status::Deciding { .. }
    ));
    assert_eq!(
        sessions.resume_latest(),
        Some(session),
        "a game is in progress"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_stopped_game_lets_the_agent_decide_no_more() {
    let mut game = Game::new(&CARDS, &start(7, StopsSetting::EXACT), SESSION).unwrap();
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
            StopsSetting::SMART
        } else {
            StopsSetting::EXACT
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
                        seen.knows(Seat::P0, Slot::from_index(usize::from(o.slot))),
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
            if resolving && matches!(view.status, Status::Thinking) {
                // The agent answering it, the entry stays on the human's stack, its card too.
                let top = view.stack.last().expect("a paused resolution's entry");
                if let StackItem::Spell { slot, .. } = top {
                    assert!(
                        view.objects.iter().any(|o| o.slot == *slot),
                        "seed {seed}: a resolving spell without its card"
                    );
                }
            }
            if !resolving {
                let mut d = None;
                let moment = match &view.status {
                    Status::Deciding { decision } if decision.halt => Moment::Halted(decision.id),
                    Status::Deciding { decision } => {
                        Moment::Deciding(decision.id, d.insert(open_decision(s, t)), None)
                    }
                    Status::Thinking => Moment::Thinking(None),
                    Status::Playing => Moment::Playing,
                    // The end shows what the human knows by then; the game is over.
                    Status::Over { .. } | Status::Stopped => break,
                };
                let det = Observation::new(s, Seat::P0).determinize(t, &mut rng);
                let mut x = Scratch::new();
                let mut theirs = player_view(
                    t,
                    Observation::new(beryl_engine::view(&det, t, &mut x), Seat::P0),
                    moment,
                    view.stops,
                    &game
                        .events()
                        .iter()
                        .map(Relayout::relayout)
                        .collect::<Vec<_>>(),
                );
                // What a cast taps and whom a blocker may block, played out on the redealt state.
                if let Some(d) = &d {
                    add_previews(&mut theirs, &det, t, d);
                }
                // Who won the opening flip, whether a cast can be taken back, and whether the
                // turn can, are the session's to say, not the position's.
                theirs.turn.flip_winner = view.turn.flip_winner;
                if let (Status::Deciding { decision: a }, Status::Deciding { decision: b }) =
                    (&mut theirs.status, &view.status)
                {
                    a.cancellable = b.cancellable;
                    a.undo = b.undo;
                }
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

#[test]
fn the_game_keeps_the_agents_decisions_for_a_report() {
    let mut game = Game::new(&CARDS, &start(5, StopsSetting::SMART), SESSION).unwrap();
    let mut view = to_human(&mut game);
    for _ in 0..12 {
        if !matches!(view.status, Status::Deciding { .. }) {
            break;
        }
        view = play(&mut game, &view);
    }
    let moves = beryl_session::ai::latest(&game.agent_moves());
    assert!(!moves.is_empty(), "the agent decided by now");
    let log = game.replay().log.len();
    for m in &moves {
        assert!(usize::from(m.chosen) < m.candidates.len(), "{m:?}");
        assert!(m.candidates.iter().all(|c| !c.is_empty()), "{m:?}");
        assert!(m.log < log);
        // The random agent does not search.
        assert_eq!(m.search, None);
    }
    assert!(moves.windows(2).all(|w| w[0].log < w[1].log));
}
