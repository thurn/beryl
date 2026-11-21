use beryl_scenario::{Action, Scenario};
use describe::reach;
use start::{at_checkpoint, start_review};

use crate::common::TEST;
use beryl_cli::specs::*;
use beryl_session::Status;

#[test]
fn a_card_without_hand_written_scenarios_gets_playable_moments() {
    let spec = spec(&TEST, beryl_card_names::names::HOLY_STRENGTH).unwrap();
    let moment = spec
        .moments()
        .iter()
        .find(|m| m.name.starts_with("generated-"))
        .expect("a moment from a generated scenario");
    for view in View::ALL {
        assert!(reach(&TEST, moment, view).is_ok(), "{view:?}");
    }
}

#[test]
fn review_shares_the_payment_choice_and_its_result() {
    use beryl_card_names::names::MANA_TITHE;
    const ARTIFACT: &str = beryl_cards::props::ARTIFACT.card;
    use beryl_session::CandidateIndex;
    use beryl_session::protocol::{Candidate, ZoneName};

    let spec = spec(&TEST, MANA_TITHE).unwrap();
    let payment = spec
        .checkpoints()
        .examples
        .iter()
        .find(|c| c.moment.group == "The controller pays {1}" && c.moment.title == "Pay {1}.")
        .expect("Mana Tithe's artifact payment checkpoint");
    assert_eq!(payment.first, View::Yours);
    for (answer, expected) in [
        (Candidate::Yes, ZoneName::Battlefield),
        (Candidate::No, ZoneName::Graveyard),
    ] {
        let (send, receive) = std::sync::mpsc::channel();
        let sessions = start_review(&TEST, &payment.moment, move |updates, live| {
            if at_checkpoint(&updates, live) {
                let _ = send.send(updates);
            }
        })
        .unwrap();
        let [payer, caster] = receive
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert_eq!(caster.view.status, Status::Thinking);
        let Status::Deciding { decision } = payer.view.status else {
            panic!("payer has no prompt")
        };
        assert!(decision.prompt.is(beryl_engine::card::Purpose::UnlessPays));
        let index = decision
            .candidates
            .iter()
            .position(|c| c.option == answer)
            .unwrap();
        sessions
            .choose(decision.id, CandidateIndex(u16::try_from(index).unwrap()))
            .unwrap();
        let views = receive
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        for update in views {
            assert!(
                update
                    .view
                    .objects
                    .iter()
                    .any(|o| { o.name.as_deref() == Some(ARTIFACT) && o.zone == expected }),
                "{:?}",
                update
                    .view
                    .objects
                    .iter()
                    .map(|o| (o.name.clone(), o.zone))
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn counter_review_starts_before_the_target_spell_is_cast() {
    use beryl_card_names::names::{MANA_TITHE, PLAINS};
    use beryl_scenario::{P0, P1};
    use beryl_session::protocol::ZoneName;

    const ARTIFACT: &str = beryl_cards::props::ARTIFACT.card;
    const SCENARIOS: &[Scenario] = &[Scenario::new("Mana Tithe: the caster cannot pay")
        .battlefield(P0, &[PLAINS])
        .hand(P0, &[ARTIFACT])
        .battlefield(P1, &[PLAINS])
        .hand(P1, &[MANA_TITHE])
        .actions(&[Action::cast(P0, ARTIFACT), Action::cast(P1, MANA_TITHE)])];
    let def = TEST
        .cards
        .cards()
        .iter()
        .copied()
        .find(|d| d.name == MANA_TITHE)
        .unwrap();
    let checkpoints = auto::checkpoints(&TEST, def, SCENARIOS).examples;
    let counter = checkpoints
        .iter()
        .find(|c| !c.review_casts.is_empty())
        .expect("a checkpoint with both recorded casts");
    assert_eq!(counter.first, View::Yours);

    let (send, receive) = std::sync::mpsc::channel();
    let casts = std::sync::Mutex::new(counter.review_casts);
    let sessions = start_review(&TEST, &counter.moment, move |updates, live| {
        if live {
            let steps = serve::recorded_cast_moves(&mut casts.lock().unwrap(), &updates);
            if at_checkpoint(&updates, live) {
                let _ = send.send((updates, steps));
            }
        }
    })
    .unwrap();
    let next = || {
        receive
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
    };
    for (actor, card) in [(0, ARTIFACT), (1, MANA_TITHE)] {
        let (views, steps) = next();
        let Status::Deciding { decision } = &views[actor].view.status else {
            panic!("expected the {card} caster to act");
        };
        assert!(steps[actor].is_none());
        let (id, index) = steps[1 - actor].expect("the observer can play the cast");
        assert_eq!(id, decision.id);
        assert_eq!(
            decision.candidates[usize::from(index.0)].label,
            format!("Cast {card}")
        );
        sessions.choose(id, index).unwrap();
    }
    let (views, steps) = next();
    assert!(steps.iter().all(Option::is_none));
    for update in views {
        assert!(!matches!(update.view.status, Status::Deciding { decision }
                if decision.prompt.is(beryl_engine::card::Purpose::UnlessPays)));
        assert!(
            update
                .view
                .objects
                .iter()
                .any(|o| { o.name.as_deref() == Some(ARTIFACT) && o.zone == ZoneName::Graveyard })
        );
    }
}

/// A scripted action that never fits says so when the game ends without it: which action
/// waited, and the decision it fit no candidate of, not just that the game ended.
#[test]
fn a_stalled_script_names_the_decision_it_missed() {
    use beryl_card_names::names::{COUNTERSPELL, ISLAND};
    use beryl_scenario::{P0, Permanent};
    static MOMENT: Moment = Moment::new(
        "stalled",
        "Stalled",
        Scenario::new("Counterspell with nothing to counter")
            .permanents(P0, &[Permanent::new(ISLAND), Permanent::new(ISLAND)])
            .hand(P0, &[COUNTERSPELL])
            .library(P0, &[ISLAND])
            .library(beryl_scenario::P1, &[ISLAND])
            .at(2, beryl_scenario::P1, beryl_engine::TurnStep::PrecombatMain),
    )
    .play(&[Action::cast(P0, COUNTERSPELL)]);
    let error = reach(&TEST, &MOMENT, View::Yours)
        .expect_err("Counterspell has no target")
        .to_string();
    assert!(error.contains("Counterspell"), "{error}");
    assert!(error.contains("fit no candidate"), "{error}");
}

/// Asking for one card's spec generates that card and no other.
#[test]
fn a_spec_generates_only_its_card() {
    use beryl_card_names::names::COUNTERSPELL;
    let library = Library::new(&TEST);
    let spec = library
        .find(&slug(COUNTERSPELL))
        .expect("Counterspell has a spec");
    assert!(library.specs.iter().all(|s| s.generated.get().is_none()));
    assert!(!spec.moments().is_empty());
    let generated: Vec<&str> = library
        .specs
        .iter()
        .filter(|s| s.generated.get().is_some())
        .map(|s| s.card)
        .collect();
    assert_eq!(generated, [COUNTERSPELL]);
}

/// Generating a spec plays every moment from both seats: a moment is kept only once it
/// reaches a live decision from each.
/// A stand-in pays for the viewer while the spell that asks resolves: every view they
/// watch meanwhile keeps that spell on the stack, where its card still lies.
#[test]
fn a_spell_that_asks_a_stand_in_stays_on_the_watched_stack() {
    use beryl_session::protocol::{StackItem, ZoneName};
    let (_, moment) = find(&TEST, "mana-tithe", "the-controller-pays-1").expect("the moment");
    let (_, views) = describe::trace(&TEST, moment, View::Yours).expect("the moment plays");
    let mut watched = 0;
    for view in &views {
        if matches!(view.status, Status::Deciding { .. }) {
            continue;
        }
        for card in view.objects.iter().filter(|o| o.zone == ZoneName::Stack) {
            watched += 1;
            assert!(
                view.stack.iter().any(
                    |item| matches!(item, StackItem::Spell { slot, .. } if *slot == card.slot)
                ),
                "turn {} {:?}: {:?} lies on the stack, which lists {:?}",
                view.turn.number,
                view.turn.step,
                card.name,
                view.stack
            );
        }
    }
    assert!(watched > 0, "no view shows a spell on the stack");
}

#[test]
fn moments_are_named_uniquely() {
    let specs = beryl_cli::specs::sample(&TEST);
    generate(&specs);
    for spec in specs {
        let moments = spec.moments();
        for (i, m) in moments.iter().enumerate() {
            assert!(
                moments[..i].iter().all(|n| n.name != m.name),
                "{}: two moments are named {}",
                spec.card,
                m.name
            );
        }
    }
}
