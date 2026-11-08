//! Taking back the human's answers one at a time: each undo restores the game exactly as it
//! stood at the decision answered, the replay goes back the same way, the same answers play the
//! same game again, and no undo goes back past the agent's decisions or anything the human
//! could not know. Passing on to the next stop goes past the stops a pass would make in its
//! step, and a replay passes on the same way.

use beryl_cards::CARDS;
use beryl_cards::decks::WU_CONTROL;
use beryl_engine::{Seat, State};
use beryl_session::protocol::{Candidate, DecisionView, Kind, PassTo, Step};
use beryl_session::replay::Entry;
use beryl_session::{
    CandidateIndex, Game, PlayerView, SessionError, SessionId, StartGame, Status, StopsSetting,
};

fn owned(deck: &[(u8, &str)]) -> Vec<(u8, String)> {
    deck.iter().map(|&(n, name)| (n, name.to_owned())).collect()
}

const SESSION: SessionId = SessionId(1);

fn new_game(seed: u64) -> Game {
    let start = StartGame {
        seed,
        decks: [owned(WU_CONTROL), owned(WU_CONTROL)],
        human: Seat::P0,
        // Greedy has no random stream, so the same answers play the same game again.
        opponent: "greedy".to_owned(),
        stops: StopsSetting::SMART,
    };
    Game::new(&CARDS, &start, SESSION).unwrap()
}

/// Plays on to the human's next decision or the end, and returns the view there.
fn to_human(game: &mut Game) -> PlayerView {
    to_human_seeing(game).0
}

/// Plays on to the human's next decision or the end, and returns the view there and whether the
/// agent decided on the way.
fn to_human_seeing(game: &mut Game) -> (PlayerView, bool) {
    let (mut last, mut agent) = (None, false);
    while let Some(view) = game.next_view() {
        agent |= matches!(view.status, Status::Thinking);
        last = Some(view);
    }
    (last.expect("the game plays on from where it is"), agent)
}

fn deciding(view: &PlayerView) -> Option<&DecisionView> {
    match &view.status {
        Status::Deciding { decision } => Some(decision),
        _ => None,
    }
}

/// Answers `index` and plays on to the human's next decision.
fn answer(game: &mut Game, view: &PlayerView, index: usize) -> PlayerView {
    let id = deciding(view).unwrap().id;
    game.choose(
        game.answer(id, CandidateIndex(u16::try_from(index).unwrap()))
            .unwrap(),
    );
    to_human(game)
}

/// A human who plays a land when they can, casts every third spell they could, and otherwise
/// passes or takes the first candidate.
fn policy(d: &DecisionView, n: usize) -> usize {
    let find = |f: fn(&Candidate) -> bool| d.candidates.iter().position(|c| f(&c.option));
    if d.kind != Kind::Priority {
        return 0;
    }
    find(|c| matches!(c, Candidate::PlayLand { .. }))
        .or_else(|| {
            n.is_multiple_of(3)
                .then(|| find(|c| matches!(c, Candidate::Cast { .. })))
                .flatten()
        })
        .or_else(|| find(|c| matches!(c, Candidate::Pass)))
        .unwrap_or(0)
}

/// A decision the human answered, as the test saw it.
struct Answered {
    state: State,
    view: PlayerView,
    answer: usize,
}

fn without_id(mut view: PlayerView) -> PlayerView {
    if let Status::Deciding { decision } = &mut view.status {
        decision.id.seq = 0;
    }
    view.events.clear();
    view.undone = false;
    view
}

/// Plays `seed`'s game for `decisions` of the human's, and at every `every`th one with answers
/// to take back, takes them back one by one in a copy of the game and checks each. Returns how
/// many undos were checked.
fn play_and_check(seed: u64, decisions: usize, every: usize) -> usize {
    let mut game = new_game(seed);
    let mut view = to_human(&mut game);
    let mut answered: Vec<Answered> = Vec::new();
    let mut checked = 0;
    for n in 0..decisions {
        let Some(d) = deciding(&view) else {
            break;
        };
        let undo = usize::from(d.undo);
        assert!(
            undo <= answered.len(),
            "more to take back than was answered"
        );
        if undo > 0 && n % every == 0 {
            checked += check_undo(&game, &answered[answered.len() - undo..], &view);
        }
        let i = policy(d, n);
        answered.push(Answered {
            state: *game.state(),
            view: view.clone(),
            answer: i,
        });
        view = answer(&mut game, &view, i);
    }
    checked
}

/// Takes back, in a copy of `game` at `now`, each of the answers `points` holds, the latest
/// first, checking each, then plays them again. Returns how many it took back.
fn check_undo(game: &Game, points: &[Answered], now: &PlayerView) -> usize {
    let (mut probe, resumed) = Game::from_replay(&CARDS, game.replay(), SESSION).unwrap();
    assert_eq!(without_id(resumed.clone()), without_id(now.clone()));
    let mut back = now.clone();
    for (left, point) in points.iter().enumerate().rev() {
        let id = deciding(&back).unwrap().id;
        assert!(probe.open().unwrap().undo(id).is_ok());
        back = probe.undo().unwrap();
        assert_eq!(*probe.state(), point.state, "the game is as it stood");
        assert!(back.events.is_empty(), "nothing happened");
        assert!(back.undone);
        assert_eq!(usize::from(deciding(&back).unwrap().undo), left);
        assert_eq!(without_id(back.clone()), without_id(point.view.clone()));
        assert_eq!(probe.replay().log.last(), Some(&Entry::Undo));
    }
    assert_eq!(probe.undo(), Err(SessionError::NothingToUndo));

    // A replay goes back the same way.
    let (_, replayed) = Game::from_replay(&CARDS, probe.replay(), SESSION).unwrap();
    assert_eq!(without_id(replayed), without_id(back.clone()));

    // The same answers play the same game again.
    let mut view = back;
    for point in points {
        view = answer(&mut probe, &view, point.answer);
    }
    assert_eq!(
        probe.state(),
        game.state(),
        "the same answers, the same game"
    );
    assert_eq!(without_id(view), without_id(now.clone()));
    points.len()
}

#[test]
fn each_undo_takes_back_one_answer_and_replays() {
    let checked: usize = [1, 2]
        .into_iter()
        .map(|seed| play_and_check(seed, 160, 3))
        .sum();
    assert!(checked >= 20, "only {checked} undos checked");
}

#[test]
fn no_undo_goes_back_past_the_agent_or_a_card_seen() {
    let (mut after_land, mut after_agent, mut after_seen) = (0, 0, 0);
    for seed in 4..12 {
        let mut game = new_game(seed);
        let mut view = to_human(&mut game);
        for n in 0..150 {
            let Some(d) = deciding(&view) else {
                break;
            };
            let i = policy(d, n);
            let land = matches!(d.candidates[i].option, Candidate::PlayLand { .. });
            let id = d.id;
            let before = view.clone();
            game.choose(
                game.answer(id, CandidateIndex(u16::try_from(i).unwrap()))
                    .unwrap(),
            );
            let agent;
            (view, agent) = to_human_seeing(&mut game);
            let Some(after) = deciding(&view) else {
                break;
            };
            let seen = saw_unseen(&before, &view);
            if seen {
                assert_eq!(after.undo, 0, "seed {seed}: an undo past a card seen");
                after_seen += usize::from(!agent);
            }
            if agent {
                assert_eq!(
                    after.undo, 0,
                    "seed {seed}: an undo past the agent's decision"
                );
                after_agent += 1;
            } else if land && !seen {
                // A land from hand shows the human nothing new.
                assert!(after.undo > 0, "seed {seed}: a land can be taken back");
                after_land += 1;
            }
        }
    }
    assert!(after_land > 0, "no land played");
    assert!(after_agent > 0, "the agent never decided between answers");
    assert!(
        after_seen > 0,
        "no card seen between answers but for the agent's"
    );
}

/// Whether `after` shows the human a card of either deck by name that `before` did not.
fn saw_unseen(before: &PlayerView, after: &PlayerView) -> bool {
    after.objects.iter().any(|o| {
        o.name.is_some()
            && !o.token
            && before
                .objects
                .iter()
                .any(|b| b.slot == o.slot && b.name.is_none())
    })
}

/// Where the human stands: the turn and its step.
fn at(view: &PlayerView) -> (u16, Step) {
    (view.turn.number, view.turn.step)
}

#[test]
fn passing_on_goes_past_the_stops_in_its_step() {
    let mut passed = 0;
    for seed in 1..10 {
        let mut game = new_game(seed);
        let mut view = to_human(&mut game);
        for n in 0..200 {
            let Some(d) = deciding(&view) else {
                break;
            };
            let Some(to) = d.pass_on.clone() else {
                view = answer(&mut game, &view, policy(d, n));
                continue;
            };
            assert!(matches!(
                d.pass_to,
                Some(PassTo::Resolve | PassTo::CombatDamage)
            ));
            let from = at(&view);
            let id = d.id;
            let pass = game.open().unwrap().pass_on(id).unwrap();
            game.pass_on(pass);
            let resumed = view.clone();
            view = to_human(&mut game);
            assert!(
                game.replay().log.contains(&Entry::PassOn),
                "the log records the pass on"
            );
            passed += 1;
            // It stops at a later step, or sooner for a decision other than priority or
            // something of the opponent's on the stack.
            if let Some(next) = deciding(&view)
                && next.kind == Kind::Priority
                && !next.halt
                && view.stack.is_empty()
            {
                let (turn, step) = at(&view);
                assert!(
                    (turn, step) != from || matches!(to, PassTo::NextTurn),
                    "seed {seed}: stopped again in the step it passed on from"
                );
            }
            // A replay passes on the same way, to the same decision.
            if deciding(&view).is_some() {
                let (_, replayed) = Game::from_replay(&CARDS, game.replay(), SESSION).unwrap();
                assert_eq!(without_id(replayed), without_id(view.clone()));
            }
            // An undo takes the pass on back, if nothing came out since.
            if let Some(next) = deciding(&view)
                && next.undo > 0
            {
                let back = game.undo().unwrap();
                assert_eq!(without_id(back.clone()), without_id(resumed));
                view = back;
            }
        }
    }
    assert!(passed >= 5, "only {passed} passes on");
}
