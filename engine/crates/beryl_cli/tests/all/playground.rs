use crate::common::TEST;
use beryl_cards::names::ADVENTUROUS_EATER_HAVE_A_BITE;
use beryl_cli::specs::playground::*;
use beryl_cli::specs::{Phase, View};
use beryl_engine::card::{FaceIx, Types};
use beryl_session::protocol::{Candidate, Side, Status, ZoneName};
use std::sync::mpsc::channel;
use std::time::Duration;

fn first_decision(variant: &'static Variant, view: View) -> beryl_session::PlayerView {
    let (tx, rx) = channel();
    let _session = start(&TEST, variant, view, move |u, phase| {
        let _ = tx.send((u, phase));
    })
    .unwrap();
    loop {
        let (update, phase) = rx
            .recv_timeout(Duration::from_secs(3))
            .expect("playground stalled");
        if phase == Phase::Live
            && matches!(
                update.view.status,
                Status::Deciding { .. } | Status::Over { .. }
            )
        {
            return update.view;
        }
    }
}

#[test]
fn empty_library_manifest_example_has_no_filler() {
    let variant = find(
        &TEST,
        "abhorrent-oculus",
        "playground-manifest-dread-with-an-empty-library-does-nothing-0",
    )
    .expect("the empty-library variant");
    let (_, state) = prepare(&TEST, variant).unwrap();
    assert!(state.library(variant.controller.seat()).is_empty());
}

#[test]
fn one_card_manifest_example_has_no_filler_to_choose_or_mill() {
    let generated = Box::leak(Box::new(generate(&TEST, "Abhorrent Oculus")));
    let variant = generated
        .examples
        .iter()
        .flat_map(|e| &e.variants)
        .find(|v| v.label == "Manifest dread with one card in the library manifests it")
        .expect("the one-card variant");
    let (table, state) = prepare(&TEST, variant).unwrap();
    let library = state.library(variant.controller.seat());
    assert_eq!(library.len(), 1);
    assert_eq!(state.name(&table, library[0]), "Island");
}

#[test]
fn first_example_offers_the_card_in_hand() {
    for spec in beryl_cli::specs::sample(&TEST) {
        let card = spec.card;
        let generated = Box::leak(Box::new(generate(&TEST, card)));
        assert!(
            generated.missing.is_empty(),
            "{card}: {:?}",
            generated.missing
        );
        let first = &generated.examples[0];
        assert_eq!(first.title, "Play from hand", "{card}");
        let variant = &first.variants[0];
        let view = first_decision(variant, variant.controller);
        let hand_name = TEST.cards.get(card).unwrap().face_name(FaceIx::FRONT);
        let card = view
            .objects
            .iter()
            .find(|o| {
                o.name.as_deref() == Some(hand_name)
                    && o.zone == ZoneName::Hand
                    && o.controller == Side::You
            })
            .expect(card);
        let Status::Deciding { decision } = &view.status else {
            panic!("game ended")
        };
        assert!(decision.candidates.iter().any(|c| matches!(c.option, Candidate::Cast { slot, .. } | Candidate::PlayLand { slot } if slot == card.slot)), "card is not playable: {:?}", decision.candidates);
    }
}

#[test]
fn preparing_a_spell_offers_its_copy_in_a_separate_example() {
    let spec = beryl_cli::specs::spec(&TEST, ADVENTUROUS_EATER_HAVE_A_BITE).unwrap();
    let playground = spec.playground();
    let example = playground
        .examples
        .iter()
        .find(|example| example.title == "Cast")
        .expect("a separate example for casting the prepared spell");
    let variant = &example.variants[0];
    let view = first_decision(variant, variant.controller);
    let copy = view
        .objects
        .iter()
        .find(|object| {
            object.zone == ZoneName::Exile && object.name.as_deref() == Some("Have a Bite")
        })
        .expect("the prepared spell in exile");
    let Status::Deciding { decision } = view.status else {
        panic!("game ended");
    };
    assert!(decision.candidates.iter().any(|candidate| matches!(
        candidate.option,
        Candidate::Cast { slot, face, .. } if slot == copy.slot && face == FaceIx::BACK.get()
    )));
}

#[test]
fn every_prepared_example_reaches_its_interaction() {
    let mut failures = Vec::new();
    for spec in beryl_cli::specs::sample(&TEST) {
        for example in &spec.playground().examples {
            for variant in &example.variants {
                match prepare(&TEST, variant) {
                    Err(error) => {
                        failures.push(format!("{} / {}: {error}", spec.card, variant.label));
                    }
                    Ok((table, state)) => {
                        // A generic nonland target filler once put Counterspell on the battlefield.
                        for &slot in state.battlefield() {
                            assert!(
                                !state
                                    .derived(slot)
                                    .types
                                    .intersects(Types::INSTANT.union(Types::SORCERY)),
                                "{} / {}: {} is not a permanent",
                                spec.card,
                                variant.label,
                                state.name(&table, slot)
                            );
                        }
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn opponent_play_leaves_the_viewers_search_interactive() {
    let generated = Box::leak(Box::new(generate(&TEST, "Winds of Abandon")));
    let variant = &generated.examples[0].variants[0];
    let view = View::ALL[1 - variant.controller.seat().index()];
    let (tx, rx) = channel();
    let session = start(&TEST, variant, view, move |u, _| {
        let _ = tx.send(u);
    })
    .unwrap();
    for _ in 0..50 {
        let update = rx
            .recv_timeout(Duration::from_secs(3))
            .expect("search never reached");
        let Status::Deciding { decision } = update.view.status else {
            continue;
        };
        if decision.prompt.is(beryl_engine::card::Purpose::Search) {
            assert!(decision.candidates.len() > 1, "search needs a real choice");
            return;
        }
        let pass = decision
            .candidates
            .iter()
            .position(|c| matches!(c.option, Candidate::Pass))
            .expect("unexpected prompt before search");
        session
            .choose(
                decision.id,
                beryl_session::protocol::CandidateIndex(u16::try_from(pass).unwrap()),
            )
            .unwrap();
    }
    panic!("search never offered");
}

#[test]
fn a_typed_name_finds_the_closest_card() {
    let closest = |text| beryl_cli::specs::closest(&TEST, text).map(|spec| spec.card);
    assert_eq!(closest("Counterspell"), Some("Counterspell"));
    assert_eq!(closest("counterspell"), Some("Counterspell"));
    assert_eq!(closest("counterspel"), Some("Counterspell"));
    assert_eq!(closest("  "), None);
    assert_eq!(closest("zzqx no card"), None);
}
