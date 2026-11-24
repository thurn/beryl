//! Taking back the human's answers one at a time: each undo restores the game exactly as it
//! stood at the decision answered, across the agent's decisions, the cards drawn and turns,
//! the replay goes back the same way, and the same answers play the same game again. Passing
//! on to the next stop goes past the stops a pass would make in its step, and a replay passes
//! on the same way.

use std::sync::{Arc, Mutex};

use beryl_cards::CARDS;
use beryl_cards::decks::WU_CONTROL;
use beryl_cards::names::{
    ISLAND, MANA_CRYPT, PLAINS, STAFF_OF_THE_STORYTELLER, TEFERI_HERO_OF_DOMINARIA,
};
use beryl_engine::card::Purpose;
use beryl_engine::{Choice, Decision, Layout, Observation, Seat, State, TurnStep};
use beryl_scenario::{P0, P1, Permanent, Scenario};
use beryl_search::{Agent, AgentDecision, Budget};
use beryl_session::prompt::ShapeBody;
use beryl_session::protocol::ObjectId;
use beryl_session::protocol::{Candidate, DecisionView, Kind, PassTo, Side, Step, ZoneName};
use beryl_session::replay::{Entry, ReplayFile};
use beryl_session::{
    CandidateIndex, Game, PlayerView, Prepared, SessionError, SessionId, StartGame, Status,
    StopsSetting, UNDO_POINTS,
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

#[test]
fn undo_groups_a_casts_automatic_payment_even_after_resume() {
    let start = StartGame {
        seed: 7,
        decks: [
            owned(&[(4, MANA_CRYPT), (4, STAFF_OF_THE_STORYTELLER), (16, PLAINS)]),
            owned(&[(24, ISLAND)]),
        ],
        human: Seat::P0,
        opponent: "greedy".to_owned(),
        stops: StopsSetting::SMART,
    };
    let mut game = Game::new(&CARDS, &start, SESSION).unwrap();
    let mut view = to_human(&mut game);
    let before = loop {
        let d = deciding(&view).expect("the human can cast Staff before the game ends");
        if let Some((cast, _)) = d.candidates.iter().enumerate().find(|(_, c)| {
            c.label == format!("Cast {STAFF_OF_THE_STORYTELLER}") && c.payments.len() > 1
        }) {
            let before = view;
            view = answer(&mut game, &before, cast);
            break before;
        }
        let cast_crypt = d
            .candidates
            .iter()
            .position(|c| c.label == format!("Cast {MANA_CRYPT}"));
        view = answer(&mut game, &view, cast_crypt.unwrap_or_else(|| policy(d, 1)));
    };
    // The smart stops pay with the plan that leaves the least floating without asking.
    assert!(deciding(&view).is_none_or(|d| d.kind != Kind::PaymentPlan));

    let saved = serde_json::to_string(game.replay()).unwrap();
    let file: ReplayFile = serde_json::from_str(&saved).unwrap();
    let (mut resumed, _) = Game::from_replay(&CARDS, &file, SESSION).unwrap();
    let undone = resumed.undo().unwrap();
    assert_eq!(without_id(undone), without_id(before.clone()));

    // The grouped Undo in the new record goes back to the same priority when replayed.
    let (_, replayed) = Game::from_replay(&CARDS, resumed.replay(), SESSION).unwrap();
    assert_eq!(without_id(replayed), without_id(before));
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
        // Back across a turn or two, and all the way back now and then.
        if undo > 0 && n % every == 0 {
            let all = n % (every * 8) == 0;
            let back = if all { undo } else { undo.min(BACK) };
            checked += check_undo(&game, &answered[answered.len() - back..], &view, all);
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
/// first, checking each, then plays them again. With `all`, `points` are all there are to
/// take back. Returns how many it took back.
fn check_undo(game: &Game, points: &[Answered], now: &PlayerView, all: bool) -> usize {
    let (mut probe, resumed) = Game::from_replay(&CARDS, game.replay(), SESSION).unwrap();
    assert_eq!(without_id(resumed.clone()), without_id(now.clone()));
    // The replayed game numbers its decisions itself: it was asked the targets the live
    // game's session took.
    let mut back = resumed;
    for (left, point) in points.iter().enumerate().rev() {
        let id = deciding(&back).unwrap().id;
        assert!(probe.open().unwrap().undo(id).is_ok());
        back = probe.undo().unwrap();
        assert_eq!(*probe.state(), point.state, "the game is as it stood");
        assert!(back.events.is_empty(), "nothing happened");
        assert!(back.undone);
        let undo = usize::from(deciding(&back).unwrap().undo);
        assert_eq!(
            undo + points.len(),
            left + usize::from(deciding(now).unwrap().undo)
        );
        assert_eq!(deciding(&back).unwrap().undo_depth as usize, undo);
        assert_eq!(without_id(back.clone()), without_id(point.view.clone()));
        assert_eq!(probe.replay().log.last(), Some(&Entry::Undo));
    }
    if all {
        assert_eq!(probe.undo(), Err(SessionError::NothingToUndo));
    }

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

/// How many answers the undo checks take back, but now and then: a turn's worth or two.
const BACK: usize = 12;

#[test]
fn undo_goes_back_past_the_agent_and_cards_seen() {
    let (mut after_agent, mut after_seen) = (0, 0);
    for seed in 4..12 {
        let mut game = new_game(seed);
        let mut view = to_human(&mut game);
        for n in 0..150 {
            let Some(d) = deciding(&view) else {
                break;
            };
            let i = policy(d, n);
            let (id, undo) = (d.id, d.undo);
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
            // Every answer can be taken back, whatever came out since.
            assert_eq!(after.undo, undo + 1, "seed {seed}: one more to take back");
            assert_eq!(u32::from(after.undo), after.undo_depth);
            after_agent += usize::from(agent);
            after_seen += usize::from(saw_unseen(&before, &view));
        }
    }
    assert!(after_agent > 0, "the agent never decided between answers");
    assert!(after_seen > 0, "no card seen between answers");
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
            // It stops at a later step, or sooner only for a decision other than priority.
            if let Some(next) = deciding(&view)
                && next.kind == Kind::Priority
                && !next.halt
            {
                assert!(
                    at(&view) != from || matches!(to, PassTo::NextTurn),
                    "seed {seed}: stopped again in the step it passed on from"
                );
            }
            // A replay passes on the same way, to the same decision.
            if deciding(&view).is_some() {
                let (_, replayed) = Game::from_replay(&CARDS, game.replay(), SESSION).unwrap();
                assert_eq!(without_id(replayed), without_id(view.clone()));
            }
            // An undo takes the pass on back, and the human passes once instead.
            if deciding(&view).is_some() {
                let back = game.undo().unwrap();
                assert_eq!(without_id(back.clone()), without_id(resumed));
                view = answer(&mut game, &back, 0);
            }
        }
    }
    assert!(passed >= 5, "only {passed} passes on");
}

/// An opponent that takes each decision's first candidate and records, in `log`, the index
/// among its own of each decision it is asked.
struct Counting(Arc<Mutex<AgentLog>>);

#[derive(Default)]
struct AgentLog {
    asked: Vec<u32>,
}

impl AgentLog {
    /// The index of the decision after the last one asked.
    fn next(&self) -> u32 {
        self.asked.last().map_or(0, |&n| n + 1)
    }
}

impl<L: Layout> Agent<L> for Counting {
    fn choose(&mut self, _: Observation<'_, L>, _: &Decision<L>, at: AgentDecision) -> Choice {
        self.0.lock().unwrap().asked.push(at.0);
        Choice(0)
    }
}

/// The human's turn, with Teferi, Hero of Dominaria to +1 and two of their three lands
/// tapped.
const TEFERI_UNTAP: Scenario = Scenario::new("teferi-untap")
    .at(3, P0, TurnStep::PrecombatMain)
    .permanents(
        P0,
        &[
            Permanent::new(TEFERI_HERO_OF_DOMINARIA),
            Permanent::new(ISLAND).tapped(),
            Permanent::new(PLAINS).tapped(),
            Permanent::new(ISLAND),
        ],
    )
    .library(P0, &[ISLAND, PLAINS, ISLAND, PLAINS, ISLAND, PLAINS])
    .permanents(P1, &[Permanent::new(ISLAND), Permanent::new(PLAINS)])
    .library(P1, &[ISLAND, ISLAND, ISLAND, ISLAND, ISLAND, ISLAND]);

/// Plays on to the human's next decision or the end, and returns the view there and every
/// view on the way.
fn to_human_watching(game: &mut Game) -> (PlayerView, Vec<PlayerView>) {
    let mut views = Vec::new();
    while let Some(view) = game.next_view() {
        views.push(view);
    }
    (views.last().cloned().expect("the game plays on"), views)
}

/// Whether the object in `slot` is tapped in `view`.
fn tapped(view: &PlayerView, slot: ObjectId) -> bool {
    view.objects.iter().any(|o| o.slot == slot && o.tapped)
}

/// Whether each of `lands` is tapped in the first view of the opponent's turn 4, among
/// `views`.
fn tapped_in_their_turn(views: &[PlayerView], lands: &[ObjectId]) -> Vec<bool> {
    let view = views
        .iter()
        .find(|v| v.turn.number == 4)
        .expect("the opponent's turn is seen");
    lands.iter().map(|&slot| tapped(view, slot)).collect()
}

/// The cards in the human's hand, by name.
fn hand(view: &PlayerView) -> Vec<Option<String>> {
    view.objects
        .iter()
        .filter(|o| o.zone == ZoneName::Hand && o.owner == Side::You)
        .map(|o| o.name.clone())
        .collect()
}

/// The player's report (by-2ml): Teferi's end-step untap took the wrong lands, and by their
/// next turn no undo went back to it. From the human's next turn, past the agent's turn and
/// their draw, one undo goes back to the untap, the agent goes back with the game, and the
/// human chooses again: the lands they choose are untapped through the opponent's turn, and
/// the same card is drawn.
#[test]
fn undo_goes_back_across_turns_to_teferis_untap() {
    let (table, state) = beryl_scenario::position(&CARDS, &TEFERI_UNTAP).unwrap();
    let log = Arc::new(Mutex::new(AgentLog::default()));
    let random = beryl_search::agent("random").unwrap();
    let mut game = Game::prepared(
        Prepared {
            opponent: Box::new(Counting(Arc::clone(&log))),
            stand_in: (random.new)(&table, 2, Budget::DEFAULT),
            scripted: false,
            hand_over: None,
            table,
            state,
            human: Seat::P0,
            stops: StopsSetting::SMART,
        },
        SESSION,
    );
    // The human activates Teferi's +1 and passes to the end step's untap.
    let mut view = to_human(&mut game);
    let d = deciding(&view).unwrap();
    let plus = d
        .candidates
        .iter()
        .position(|c| matches!(c.option, Candidate::Activate { ability: 0, .. }))
        .unwrap();
    view = answer(&mut game, &view, plus);
    while deciding(&view).unwrap().kind == Kind::Priority {
        view = answer(&mut game, &view, 0);
    }
    let untap = view.clone();
    let d = deciding(&untap).unwrap();
    assert!(d.prompt.is(Purpose::Untap));
    assert!(
        matches!(d.prompt.body, ShapeBody::Pick { most: 2, .. }),
        "{:?}",
        d.prompt.body
    );
    assert_eq!((untap.turn.number, untap.turn.step), (3, Step::End));
    let agent_there = log.lock().unwrap().next();
    // The two tapped lands it may untap.
    let lands: Vec<ObjectId> = d
        .candidates
        .iter()
        .filter_map(|c| match c.option {
            Candidate::Object { slot } => Some(slot),
            _ => None,
        })
        .collect();
    assert_eq!(lands.len(), 2);

    // They untap nothing, and the opponent's turn goes by with both lands tapped.
    let done = d
        .candidates
        .iter()
        .position(|c| c.option == Candidate::Done)
        .unwrap();
    game.choose(game.answer(d.id, CandidateIndex(done as u16)).unwrap());
    let (next_turn, views) = to_human_watching(&mut game);
    assert_eq!(tapped_in_their_turn(&views, &lands), [true, true]);
    assert_eq!(next_turn.turn.number, 5);
    let agent_after = log.lock().unwrap().next();
    assert!(agent_after > agent_there, "the agent decided in its turn");

    // One undo from the next turn goes back to the untap, asked again as it was.
    let d = deciding(&next_turn).unwrap();
    assert_eq!(d.undo, d.undo_depth as u16);
    assert!(game.open().unwrap().undo(d.id).is_ok());
    let back = game.undo().unwrap();
    assert!(back.undone && back.events.is_empty());
    assert_eq!(without_id(back.clone()), without_id(untap.clone()));
    let asked_before_undo = log.lock().unwrap().asked.len();
    assert_eq!(
        game.replay().log.last(),
        Some(&Entry::Undo),
        "the log records the undo"
    );

    // They untap both lands this time, which stay untapped through the opponent's turn.
    let mut view = back;
    for (n, &slot) in lands.iter().enumerate() {
        let d = deciding(&view).unwrap();
        let i = d
            .candidates
            .iter()
            .position(|c| c.option == Candidate::Object { slot })
            .unwrap();
        game.choose(game.answer(d.id, CandidateIndex(i as u16)).unwrap());
        let views;
        (view, views) = to_human_watching(&mut game);
        if n == 1 {
            assert_eq!(tapped_in_their_turn(&views, &lands), [false, false]);
        }
    }
    assert_eq!(view.turn.number, 5);
    let log = log.lock().unwrap();
    assert_eq!(
        log.asked.get(asked_before_undo),
        Some(&agent_there),
        "the agent goes back with the game"
    );
    assert_eq!(log.next(), agent_after, "the agent decides again");
    assert_eq!(hand(&view), hand(&next_turn), "the same card is drawn");
}

/// Two players with nothing but Islands, a long game.
const ISLANDS: Scenario = Scenario::new("islands")
    .at(3, P0, TurnStep::PrecombatMain)
    .library(P0, &[ISLAND; 60])
    .library(P1, &[ISLAND; 60]);

/// A game keeps at most `UNDO_POINTS` to go back to, dropping the oldest (a whole game against
/// greedy under the exact stops took 339 answers): the depth goes on counting every answer,
/// and an undo goes back as far as the points kept.
#[test]
fn undo_keeps_the_latest_points() {
    let (table, state) = beryl_scenario::position(&CARDS, &ISLANDS).unwrap();
    let random = beryl_search::agent("random").unwrap();
    let mut game = Game::prepared(
        Prepared {
            opponent: Box::new(Counting(Arc::default())),
            stand_in: (random.new)(&table, 2, Budget::DEFAULT),
            scripted: false,
            hand_over: None,
            table,
            state,
            human: Seat::P0,
            stops: StopsSetting::EXACT,
        },
        SESSION,
    );
    let mut view = to_human(&mut game);
    let mut n = 0;
    while deciding(&view).is_some_and(|d| d.undo_depth as usize <= UNDO_POINTS + 10) {
        let d = deciding(&view).unwrap();
        assert_eq!(usize::from(d.undo), n.min(UNDO_POINTS));
        assert_eq!(d.undo_depth as usize, n);
        view = answer(&mut game, &view, policy(d, n));
        n += 1;
    }
    assert!(deciding(&view).is_some(), "the game lasts {n} answers");
    for _ in 0..UNDO_POINTS {
        view = game.undo().unwrap();
    }
    let d = deciding(&view).unwrap();
    assert_eq!((d.undo, d.undo_depth as usize), (0, n - UNDO_POINTS));
    assert_eq!(game.undo(), Err(SessionError::NothingToUndo));
}
