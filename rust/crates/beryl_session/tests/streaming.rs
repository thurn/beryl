//! How a game streams to the human: a view as it starts, as each step begins and around each
//! of the agent's decisions, every event in exactly one of them, and the halts the smart stops
//! add where the engine would pass the human's priority for them.

use beryl_cards::CARDS;
use beryl_cards::decks::{VINTAGE_WU_CONTROL, WU_CONTROL, fuzz_deck};
use beryl_cards::names;
use beryl_engine::event::{Event, Reason};
use beryl_engine::{
    Choice, GameTable, Large, Limits, Profile, Relayout, Replay, Scratch, State, Step, apply, next,
};
use beryl_session::protocol::{Kind, Side, StackItem, Step as TurnStep};
use beryl_session::replay::Entry;
use beryl_session::{
    CandidateIndex, Game, PlayerView, Seat, SessionId, StartGame, Status, StopsSetting, with_stops,
};

const SESSION: SessionId = SessionId(1);

fn owned(deck: &[(u8, &str)]) -> Vec<(u8, String)> {
    deck.iter().map(|&(n, name)| (n, name.to_owned())).collect()
}

fn start(seed: u64, decks: [Vec<(u8, String)>; 2], human: Seat, stops: StopsSetting) -> StartGame {
    StartGame {
        seed,
        decks,
        human,
        opponent: "random".to_owned(),
        stops,
    }
}

/// A deck that never casts anything: no lands, and nothing free.
fn no_plays() -> Vec<(u8, String)> {
    owned(&[(40, "Thieving Skydiver")])
}

/// The scripted human's answer: a fixed pattern over the candidates, never the agent's stream.
fn answer(game: &mut Game, view: &PlayerView) {
    let Status::Deciding { decision } = &view.status else {
        return;
    };
    let n = decision.candidates.len() as u32;
    let index = CandidateIndex(((decision.id.seq * 7 + 3) % n) as u16);
    game.choose(game.answer(decision.id, index).unwrap());
}

/// Every view of a whole game, each with the engine events it reported.
fn play_out(game: &mut Game) -> Vec<(PlayerView, Vec<Event<Large>>)> {
    let mut views = Vec::new();
    while let Some(view) = game.next_view() {
        answer(game, &view);
        views.push((view, game.events().to_vec()));
    }
    views
}

fn halt(view: &PlayerView) -> bool {
    matches!(&view.status, Status::Deciding { decision } if decision.halt)
}

/// A step the human is halted at, when the engine offers them nothing there.
fn halting_step(view: &PlayerView) -> bool {
    view.turn.step == TurnStep::PrecombatMain && view.turn.active == Side::You
}

#[test]
fn the_first_view_comes_before_the_agent_decides_anything() {
    let mut thinking = 0;
    for seed in 0..8 {
        let human = if seed % 2 == 0 { Seat::P0 } else { Seat::P1 };
        let decks = [owned(WU_CONTROL), owned(WU_CONTROL)];
        let mut game = Game::new(
            &CARDS,
            &start(seed, decks, human, StopsSetting::SMART),
            SESSION,
        )
        .unwrap();
        let first = game.next_view().unwrap();
        assert!(
            game.replay().log.is_empty(),
            "seed {seed}: a decision before the first view"
        );
        match first.status {
            Status::Thinking => thinking += 1,
            Status::Deciding { .. } => {}
            status => panic!("seed {seed}: the game starts {status:?}"),
        }
    }
    assert!(thinking > 0, "the agent never chose to play or draw");
}

#[test]
fn each_step_and_each_agent_decision_has_a_view_and_each_event_one_view() {
    for seed in 0..4 {
        let stops = if seed % 2 == 0 {
            StopsSetting::SMART
        } else {
            StopsSetting::EXACT
        };
        let decks = [owned(&fuzz_deck(seed)), owned(&fuzz_deck(seed ^ 1 << 40))];
        let game_start = start(seed, decks, Seat::P0, stops);
        let mut game = Game::new(&CARDS, &game_start, SESSION).unwrap();
        let views = play_out(&mut game);
        assert!(matches!(
            views.last().unwrap().0.status,
            Status::Over { .. }
        ));

        // The agent's decisions are the engine's that the human did not answer.
        let log = &game.replay().log;
        let choices = log.iter().filter(|e| matches!(e, Entry::Choice(_))).count();
        let human = views
            .iter()
            .filter(|(v, _)| matches!(v.status, Status::Deciding { .. }) && !halt(v))
            .count();
        let thinking = views
            .iter()
            .filter(|(v, _)| v.status == Status::Thinking)
            .count();
        assert_eq!(
            thinking,
            choices - human,
            "seed {seed}: a view for each agent decision"
        );

        // A view shows each step as it begins, and only then does the step change.
        let mut began_steps = 0;
        for pair in views.windows(2) {
            let [(before, _), (view, events)] = pair else {
                unreachable!()
            };
            let began = view.status == Status::Playing || halt(view);
            let moved =
                (before.turn.number, before.turn.step) != (view.turn.number, view.turn.step);
            if matches!(view.status, Status::Over { .. }) {
                continue;
            }
            assert_eq!(
                began, moved,
                "seed {seed}: {:?} at {:?}",
                view.status, view.turn
            );
            if began {
                began_steps += 1;
                assert!(
                    events.iter().any(|e| matches!(e, Event::StepBegins { .. })),
                    "seed {seed}: a step's view without its beginning"
                );
            }
        }
        assert!(began_steps > 50, "seed {seed}: only {began_steps} steps");

        // The views' events, in order, are the game's.
        let streamed: Vec<Event<Large>> =
            views.iter().flat_map(|(_, e)| e.iter().cloned()).collect();
        assert_eq!(streamed, events_of(&game_start, log), "seed {seed}");
    }
}

/// A loot asks for its discard with the card it drew in hand
/// ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c): "The controller of the
/// spell or ability follows its instructions in the order written"): the view that asks it
/// reports the draw, and the views after it report the discard but not the draw again.
#[test]
fn a_loot_reports_its_draw_before_it_asks_for_the_discard() {
    let decks = [owned(VINTAGE_WU_CONTROL), owned(VINTAGE_WU_CONTROL)];
    let mut begin = start(1, decks, Seat::P0, StopsSetting::SMART);
    begin.opponent = "greedy".to_owned();
    let mut game = Game::new(&CARDS, &begin, SESSION).unwrap();
    game.set_stand_in("greedy").unwrap();
    let looting = |view: &PlayerView| {
        matches!(&view.status, Status::Deciding { decision }
            if decision.kind == Kind::Discard
                && matches!(&decision.resolving,
                    Some(StackItem::Ability { name, .. }) if name == names::JACE_VRYNS_PRODIGY))
    };
    let draws = |events: &[Event<Large>]| {
        events
            .iter()
            .filter(|e| matches!(e, Event::Draw { player, .. } if *player == Seat::P0))
            .count()
    };
    loop {
        let view = game.next_view().expect("the game reaches Jace's loot");
        if looting(&view) {
            break;
        }
        game.answer_by_stand_in();
    }
    assert_eq!(draws(game.events()), 1, "the draw comes with the question");
    game.answer_by_stand_in();
    let after = game.next_view().unwrap();
    assert!(!looting(&after));
    assert_eq!(draws(game.events()), 0, "the draw is not reported again");
    assert!(game.events().iter().any(|e| matches!(
        e,
        Event::ZoneChange {
            reason: Reason::Discard,
            ..
        }
    )));
}

/// Every event of the game `log` records, played on the engine alone.
fn events_of(begin: &StartGame, log: &[Entry]) -> Vec<Event<Large>> {
    let decks = begin.decks.each_ref().map(|deck| {
        deck.iter()
            .map(|(n, name)| (*n, CARDS.get(name).unwrap().name))
            .collect::<Vec<_>>()
    });
    let t = GameTable::new(&CARDS, [decks[0].as_slice(), decks[1].as_slice()]).unwrap();
    let profile = with_stops(Profile::SMART, begin.stops, begin.human);
    let mut s = State::new(&t, begin.seed, profile);
    let mut x = Scratch::recording();
    let mut choices = log.iter().filter_map(|e| match e {
        Entry::Choice(c) | Entry::Auto(c) => Some(Choice(c.index)),
        _ => None,
    });
    while let Step::Decide(_) = next(&mut s, &t, &mut x, &mut Replay) {
        apply(&mut s, &t, &mut x, &mut Replay, choices.next().unwrap());
    }
    assert_eq!(choices.next(), None);
    x.events.drain().map(|e| e.relayout()).collect()
}

#[test]
fn smart_stops_halt_a_human_with_no_plays_at_their_main_phase_only() {
    let decks = [no_plays(), owned(WU_CONTROL)];
    let game_start = start(3, decks, Seat::P0, StopsSetting::SMART);
    let mut game = Game::new(&CARDS, &game_start, SESSION).unwrap();
    let mut views = Vec::new();
    let mut resumed = 0;
    while let Some(view) = game.next_view() {
        if halt(&view) && resumed < 3 {
            // A replay cut at a halt resumes there.
            let (_, again) = Game::from_replay(&CARDS, game.replay(), SESSION).unwrap();
            assert_eq!(again, view);
            resumed += 1;
        }
        answer(&mut game, &view);
        views.push(view);
    }
    let mains = views.iter().filter(|v| halt(v)).count();
    assert!(mains >= 3, "{mains} halts");
    for view in &views {
        let began = view.status == Status::Playing || halt(view);
        assert_eq!(
            halt(view),
            began && halting_step(view),
            "a halt where none belongs, or none where one does, at {:?}",
            view.turn
        );
    }
    let (_, last) = Game::from_replay(&CARDS, game.replay(), SESSION).unwrap();
    assert_eq!(&last, views.last().unwrap());
}

#[test]
fn exact_stops_halt_nowhere() {
    let decks = [no_plays(), owned(WU_CONTROL)];
    let mut game = Game::new(
        &CARDS,
        &start(3, decks, Seat::P0, StopsSetting::EXACT),
        SESSION,
    )
    .unwrap();
    let views = play_out(&mut game);
    assert!(views.len() > 100);
    assert!(views.iter().all(|(v, _)| !halt(v)));
}

#[test]
fn a_human_who_can_play_stops_at_their_main_phase() {
    for seed in 0..3 {
        let decks = [owned(WU_CONTROL), owned(WU_CONTROL)];
        let mut game = Game::new(
            &CARDS,
            &start(seed, decks, Seat::P1, StopsSetting::SMART),
            SESSION,
        )
        .unwrap();
        let views: Vec<PlayerView> = play_out(&mut game).into_iter().map(|(v, _)| v).collect();
        let mut stops = 0;
        for (i, view) in views.iter().enumerate() {
            let began = view.status == Status::Playing || halt(view);
            if !(began && halting_step(view)) {
                continue;
            }
            // The human decides in this step before the agent does: a halt, or the engine's
            // own decision.
            let first = views[i..]
                .iter()
                .find(|v| {
                    v.status == Status::Thinking || matches!(v.status, Status::Deciding { .. })
                })
                .unwrap();
            assert!(
                matches!(first.status, Status::Deciding { .. })
                    && (first.turn.number, first.turn.step) == (view.turn.number, view.turn.step)
                    || halt(view),
                "seed {seed}: the human was not stopped at {:?}",
                view.turn
            );
            stops += 1;
        }
        assert!(stops > 4, "seed {seed}: only {stops} stops");
    }
}

#[test]
fn a_player_game_lifts_the_decision_and_turn_budgets() {
    for stops in [StopsSetting::SMART, StopsSetting::EXACT] {
        for human in Seat::BOTH {
            let limits = with_stops(Profile::SMART, stops, human).limits;
            assert_eq!(limits, Limits::PLAYER);
            assert_eq!(limits.decisions_per_turn, u16::MAX);
            assert_eq!(limits.turns, u16::MAX);
        }
    }
}
