//! Taking back a cast before its costs are paid: the game returns to the human's priority as
//! it was, and the replay never records the cast.

use beryl_cards::CARDS;
use beryl_cards::decks::WU_CONTROL;
use beryl_engine::Seat;
use beryl_session::protocol::{Candidate, Kind, StackItem};
use beryl_session::replay::Entry;
use beryl_session::{
    CandidateIndex, Game, PlayerView, SessionError, SessionId, StartGame, Status, StopsSetting,
};

fn owned(deck: &[(u8, &str)]) -> Vec<(u8, String)> {
    deck.iter().map(|&(n, name)| (n, name.to_owned())).collect()
}

const SESSION: SessionId = SessionId(1);

fn new_game() -> Game {
    let start = StartGame {
        seed: 1,
        decks: [owned(WU_CONTROL), owned(WU_CONTROL)],
        human: Seat::P0,
        opponent: "random".to_owned(),
        stops: StopsSetting::SMART,
    };
    Game::new(&CARDS, &start, SESSION).unwrap()
}

/// Plays on to the human's next decision or the end, and returns the view there.
fn to_human(game: &mut Game) -> PlayerView {
    let mut last = None;
    while let Some(view) = game.next_view() {
        last = Some(view);
    }
    last.expect("the game plays on from where it is")
}

fn deciding(view: &PlayerView) -> Option<&beryl_session::protocol::DecisionView> {
    match &view.status {
        Status::Deciding { decision } => Some(decision),
        _ => None,
    }
}

/// Answers `index` and plays on to the human's next decision.
fn answer(game: &mut Game, view: &PlayerView, index: usize) -> PlayerView {
    let id = deciding(view).unwrap().id;
    game.choose(game.answer(id, CandidateIndex(index as u16)).unwrap());
    to_human(game)
}

/// Plays the game with the human passing or playing a land, until they hold priority with a
/// spell whose cast asks for a target: the view there, and that cast's index. On the way, a
/// trigger's target asked once a cast is complete (an enters trigger, as smart stops let the
/// spell resolve) is checked to be no part of the cast: it cannot be taken back.
fn to_a_targeted_cast(game: &mut Game) -> (PlayerView, usize) {
    let mut view = to_human(game);
    for _ in 0..400 {
        let d = deciding(&view).expect("the game goes on");
        if d.kind == Kind::Priority && !d.halt {
            for (i, c) in d.candidates.iter().enumerate() {
                if !matches!(c.option, Candidate::Cast { .. }) {
                    continue;
                }
                // Look ahead in a copy of the game: does the cast ask a target?
                let mut ahead = Game::from_replay(&CARDS, game.replay(), SESSION).unwrap().0;
                let next = answer(&mut ahead, &view, i);
                let Some(n) = deciding(&next).filter(|n| n.kind == Kind::Target) else {
                    continue;
                };
                let Candidate::Cast { slot, .. } = c.option else {
                    unreachable!("a cast");
                };
                match next.stack.last() {
                    Some(StackItem::Spell { slot: s, .. }) if *s == slot => return (view, i),
                    Some(StackItem::Ability {
                        triggered: true, ..
                    }) => {
                        assert!(!n.cancellable, "a trigger's target is no part of a cast");
                    }
                    _ => {}
                }
            }
        }
        let land = d
            .candidates
            .iter()
            .position(|c| matches!(c.option, Candidate::PlayLand { .. }));
        // Pass, or play a land; the first candidate of any other decision.
        let pass = d
            .candidates
            .iter()
            .position(|c| matches!(c.option, Candidate::Pass));
        view = answer(game, &view, land.or(pass).unwrap_or(0));
    }
    panic!("no targeted cast in 400 decisions");
}

#[test]
fn a_cast_taken_back_at_its_target_returns_to_priority_as_it_was() {
    let mut game = new_game();
    let (before, cast) = to_a_targeted_cast(&mut game);
    assert!(
        !deciding(&before).unwrap().cancellable,
        "priority itself is not"
    );
    let state = *game.state();
    let log = game.replay().log.clone();

    let targeting = answer(&mut game, &before, cast);
    let asked = deciding(&targeting).unwrap();
    assert_eq!(asked.kind, Kind::Target);
    assert!(asked.cancellable, "an unpaid cast can be taken back");
    assert!(game.open().unwrap().cancel(asked.id).is_ok());

    let again = game.cancel().unwrap();
    let d = deciding(&again).unwrap();
    assert_eq!(d.kind, Kind::Priority);
    assert!(!d.cancellable);
    assert_ne!(d.id, deciding(&before).unwrap().id, "a new decision");
    assert!(again.events.is_empty(), "nothing happened");
    assert_eq!(*game.state(), state, "the game is as it was");
    assert_eq!(game.replay().log, log, "the replay forgets the cast");
    assert_eq!(again.objects, before.objects);
    assert_eq!(again.stack, before.stack);
    assert_eq!(game.cancel(), Err(SessionError::NotCancellable));
    // The old decision's answers are stale.
    assert!(game.answer(asked.id, CandidateIndex(0)).is_err());

    // Cast again, and switch to full control while choosing its target: taking the cast back
    // keeps the new stops, recorded at the priority.
    let targeting = answer(&mut game, &again, cast);
    assert!(deciding(&targeting).unwrap().cancellable);
    game.set_stops(StopsSetting::EXACT).unwrap();
    let again = game.cancel().unwrap();
    assert_eq!(again.stops, StopsSetting::EXACT);
    assert_eq!(game.replay().log[..log.len()], log[..]);
    assert_eq!(
        game.replay().log[log.len()..],
        [Entry::Stops(StopsSetting::EXACT)]
    );

    // Cast again and see it through: once its costs are paid, it stays cast.
    let mut view = answer(&mut game, &again, cast);
    while let Some(d) = deciding(&view)
        && d.cancellable
    {
        view = answer(&mut game, &view, 0);
    }
    assert!(!view.stack.is_empty(), "the spell was cast");
    assert_eq!(game.cancel(), Err(SessionError::NotCancellable));
    // The replay reproduces the game without the cast taken back, but for the count of
    // decisions in the ids, which counted the one taken back.
    let (_, mut replayed) = Game::from_replay(&CARDS, game.replay(), SESSION).unwrap();
    if let (Status::Deciding { decision: r }, Status::Deciding { decision: v }) =
        (&mut replayed.status, &view.status)
    {
        assert!(r.id.seq < v.id.seq);
        r.id = v.id;
    }
    assert_eq!(replayed, view);
}

/// A cast taken back has no event: the script beside its view puts the card in the hand
/// again, by what the two tables differ in.
#[test]
fn a_cast_taken_back_flies_its_card_to_the_hand_again() {
    let mut game = new_game();
    let (before, cast) = to_a_targeted_cast(&mut game);
    let Candidate::Cast { slot, .. } = deciding(&before).unwrap().candidates[cast].option else {
        unreachable!("a cast");
    };
    let card = before.objects.iter().find(|o| o.slot == slot).unwrap();
    let name = card.name.as_deref().expect("the human's own card");
    let casting = answer(&mut game, &before, cast);
    let text = |view: &PlayerView| view.script.as_ref().expect("a script").to_string();
    assert_eq!(
        text(&casting),
        format!("fly #{slot} ({name}) hand>stack cast <- you\n")
    );
    let back = game.cancel().unwrap();
    assert_eq!(
        text(&back),
        format!("fly #{slot} ({name}) stack>hand return\n")
    );
}

use beryl_cards::names;
use beryl_scenario::{P0, P1, Permanent, Scenario};

const EXILE_PAYMENT_SETUP: Scenario = Scenario::new("mana before an exile selection")
    .battlefield(P0, &[Permanent::new(names::ISLAND); 3])
    .hand(P0, &[names::ABHORRENT_OCULUS])
    .graveyard(
        P0,
        &[
            names::PLAINS,
            names::ISLAND,
            names::OUST,
            names::REPRIEVE,
            names::COUNTERSPELL,
            names::STOCK_UP,
            names::THIRST_FOR_DISCOVERY,
        ],
    )
    .library(P0, &[names::ISLAND; 10])
    .library(P1, &[names::ISLAND; 10]);

/// Paying mana before the additional-cost picker is still part of the cancellable cast.
#[test]
fn exile_cost_picker_follows_mana_and_cancel_restores_payment() {
    use beryl_search::Budget;
    use beryl_session::Prepared;
    use beryl_session::protocol::{GameEvent, ZoneName};

    let stops = StopsSetting::EXACT;
    let (table, state) = beryl_scenario::position(&CARDS, &EXILE_PAYMENT_SETUP).unwrap();
    let random = beryl_search::agent("random").unwrap();
    let mut game = Game::prepared(
        Prepared {
            opponent: (random.new)(&table, 1, Budget::DEFAULT),
            stand_in: (random.new)(&table, 2, Budget::DEFAULT),
            scripted: false,
            hand_over: None,
            table,
            state,
            human: Seat::P0,
            stops,
        },
        SESSION,
    );
    let before = to_human(&mut game);
    let state = *game.state();
    let cast = deciding(&before)
        .unwrap()
        .candidates
        .iter()
        .position(|c| matches!(c.option, Candidate::Cast { .. }))
        .unwrap();
    {
        let mut choosing = answer(&mut game, &before, cast);
        while matches!(
            deciding(&choosing).unwrap().kind,
            Kind::ManaPayment | Kind::PaymentPlan
        ) {
            choosing = answer(&mut game, &choosing, 0);
        }
        assert_eq!(deciding(&choosing).unwrap().kind, Kind::ExileCost);
        assert_eq!(
            choosing
                .objects
                .iter()
                .filter(|o| o.zone == ZoneName::Battlefield && o.tapped)
                .count(),
            3,
            "mana sources tapped before selection"
        );
        assert_eq!(choosing.players.you.graveyard.len(), 7);
        assert!(deciding(&choosing).unwrap().cancellable);
        let restored = game.cancel().unwrap();
        assert_eq!(*game.state(), state);
        assert_eq!(restored.objects, before.objects);
        // A restored decision has a fresh id; use it for the next attempt.
        let mut again = answer(&mut game, &restored, cast);
        while matches!(
            deciding(&again).unwrap().kind,
            Kind::ManaPayment | Kind::PaymentPlan
        ) {
            again = answer(&mut game, &again, 0);
        }
        while deciding(&again).is_some_and(|d| d.kind == Kind::ExileCost) {
            again = answer(&mut game, &again, 0);
            assert!(
                !again
                    .events
                    .iter()
                    .any(|e| matches!(e, GameEvent::Mana { .. } | GameEvent::Tap { .. })),
                "no selection pays or taps mana twice"
            );
        }
        assert_eq!(again.players.you.graveyard.len(), 1);
        assert_eq!(
            again
                .objects
                .iter()
                .filter(|o| o.zone == ZoneName::Exile)
                .count(),
            6
        );
        assert!(matches!(again.stack.last(), Some(StackItem::Spell { .. })));
        assert!(!deciding(&again).unwrap().cancellable);
    }
}
