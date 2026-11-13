//! Historical bug conditions must be reachable in the actual independent Playground games,
//! not just in a fully scripted scenario replay. These check opportunity to review the UI;
//! the browser's motion and gesture checks still judge its presentation.

use super::*;
use beryl_engine::card::Purpose;
use beryl_session::protocol::{
    Candidate, CandidateIndex, CardHandling, GameEvent, Kind, ObjectId, PlayerView, Side, Status,
    ZoneName,
};
use std::sync::mpsc::channel;
use std::time::Duration;

fn variant(card: &str, fragment: &str) -> &'static Variant {
    super::super::spec(card)
        .unwrap()
        .playground()
        .examples
        .iter()
        .flat_map(|e| &e.variants)
        .find(|v| {
            v.label.to_lowercase().contains(&fragment.to_lowercase())
                || v.moment.name.contains(fragment)
        })
        .unwrap_or_else(|| panic!("missing {card}: {fragment}"))
}

fn exercise(
    card: &str,
    fragment: &str,
    opposite: bool,
    mut check: impl FnMut(&PlayerView) -> bool,
) {
    let variant = variant(card, fragment);
    let view = View::ALL[variant.controller.seat().index() ^ usize::from(opposite)];
    let (tx, rx) = channel();
    let session = start(variant, view, move |u, _| {
        let _ = tx.send(u.view);
    })
    .unwrap();
    let mut started = opposite;
    let mut prompts = Vec::new();
    for _ in 0..100 {
        let v = rx
            .recv_timeout(Duration::from_secs(3))
            .expect("Playground stalled");
        if check(&v) {
            return;
        }
        let Status::Deciding { decision } = &v.status else {
            continue;
        };
        prompts.push(format!(
            "{:?}: {:?}",
            decision.kind,
            decision
                .candidates
                .iter()
                .map(|c| &c.label)
                .collect::<Vec<_>>()
        ));
        let index = if decision.kind == Kind::Priority {
            let play = (!started).then(|| decision.candidates.iter().position(|c| {
                matches!(c.option, Candidate::Cast { slot, .. } | Candidate::PlayLand { slot } | Candidate::Activate { slot, .. }
                    if v.objects.iter().any(|o| o.slot == slot && o.name.as_deref() == Some(card)))
            })).flatten();
            if play.is_some() {
                started = true;
            }
            play.or_else(|| {
                decision
                    .candidates
                    .iter()
                    .position(|c| matches!(c.option, Candidate::Pass))
            })
            .unwrap()
        } else {
            decision
                .candidates
                .iter()
                .position(|c| {
                    matches!(
                        c.option,
                        Candidate::Number { n: 1 } | Candidate::Attack { .. }
                    )
                })
                .unwrap_or(0)
        };
        session
            .choose(decision.id, CandidateIndex(u16::try_from(index).unwrap()))
            .unwrap();
    }
    panic!("{card} / {fragment} did not reach review condition: {prompts:?}");
}

#[test]
fn subtlety_keeps_opponents_bottom_choice_and_viewers_prompt() {
    // by-nva: this used to emit libraryTop:true despite the recorded Bottom action.
    exercise("Subtlety", "evoked", false, |v| {
        v.events.iter().any(|e| json!(e)["libraryTop"] == false)
    });
    exercise("Subtlety", "evoked", true, |v| {
        matches!(&v.status, Status::Deciding { decision }
            if decision.prompt.is(Purpose::TopOrBottom) && decision.candidates.len() == 2)
    });
}

#[test]
fn iceberg_shows_a_private_scry_and_its_public_result() {
    let mut looked = false;
    exercise(
        "Aang's Iceberg",
        "waterbend-taps-a-creature-and-an-artifact-scry-reorders-the-top",
        true,
        |v| {
            looked |= matches!(
                v.handling,
                Some(CardHandling::Library {
                    side: Side::Opponent,
                    count: Some(2),
                    scry: Some(true),
                })
            );
            looked
                && v.events.iter().any(|e| {
                    matches!(
                        e,
                        GameEvent::Scry {
                            player: Side::Opponent,
                            top: 2,
                            bottom: 0
                        }
                    )
                })
        },
    );
}

#[test]
fn opponent_responds_above_the_viewers_spell() {
    // by-97z: both spells and the lower spell's target must be available to inspect.
    exercise("Oust", "opposing counterspell", false, |v| {
        v.stack.len() == 2
    });
    // by-xd4: a response to Staff must actually happen before testing the return to priority.
    let mut responded = false;
    exercise("Staff of the Storyteller", "opposing flash", false, |v| {
        responded |= v.events.iter().any(|e| {
            let e = json!(e);
            e["kind"] == "cast"
                && v.objects.iter().any(|o| {
                    o.name.as_deref() == Some("Snapcaster Mage") && json!(o.slot) == e["slot"]
                })
        });
        responded
            && v.objects.iter().any(|o| {
                o.name.as_deref() == Some("Spirit Token") && o.zone == ZoneName::Battlefield
            })
    });
}

#[test]
fn token_departure_is_visible_while_winds_waits_for_a_search() {
    // by-yfw: an exiled token and a still-resolving source differ from ordinary removal.
    let mut departed = false;
    exercise("Winds of Abandon", "exiles a token", true, |v| {
        departed |= v.events.iter().any(|e| {
            let e = json!(e);
            e["kind"] == "zoneChange" && e["from"] == "battlefield" && e["to"] == "exile"
        });
        departed
            && matches!(&v.status, Status::Deciding { decision }
            if decision.prompt.is(Purpose::Search) && decision.candidates.len() >= 3 && (!v.stack.is_empty() || decision.resolving.is_some()))
    });
}

#[test]
fn known_top_search_and_three_land_untap_are_interactive() {
    // by-kac: a known top used to add an unnecessary "Search?" question.
    let mut known_top = false;
    exercise("Trinket Mage", "known library top", false, |v| {
        known_top |= v
            .objects
            .iter()
            .any(|o| o.zone == ZoneName::Library && o.owner == Side::You && o.name.is_some());
        if let Status::Deciding { decision } = &v.status {
            assert!(!decision.prompt.is(Purpose::MaySearch));
            return known_top
                && decision.prompt.is(Purpose::Search)
                && decision.candidates.len() >= 2;
        }
        false
    });
    // by-svq/by-2ml: two tapped basics alone cannot expose a wrong dual-land suggestion.
    exercise("Teferi, Hero of Dominaria", "dual land", false, |v| {
        matches!(&v.status, Status::Deciding { decision }
            if decision.prompt.is(Purpose::Untap) && decision.candidates.len() >= 4 && decision.undo_depth > 0)
    });
}

#[test]
fn alternate_casts_cycling_and_tax_payment_remain_choices() {
    // by-lzl/by-9fb: both options must be affordable to review the card chooser.
    exercise("Subtlety", "mana and blue", false, |v| {
        matches!(&v.status, Status::Deciding { decision }
        if decision.candidates.iter().filter(|c| matches!(c.option, Candidate::Cast { .. })).count() >= 2)
    });
    exercise("Spara's Headquarters", "Cycling", false, |v| {
        matches!(&v.status, Status::Deciding { decision }
        if decision.candidates.iter().any(|c| matches!(c.option, Candidate::PlayLand { .. }))
        && decision.candidates.iter().any(|c| matches!(c.option, Candidate::Activate { .. })))
    });
    // by-dzp: the paying seat must answer its own question on the opponent-play board.
    exercise("Mana Tithe", "controller pays", true, |v| {
        matches!(&v.status, Status::Deciding { decision }
        if decision.prompt.is(Purpose::UnlessPays) && decision.candidates.len() == 2)
    });
}

#[test]
fn linked_exile_can_be_reviewed_with_host_slot_zero() {
    // by-8g5: treating the host slot as a boolean detached only links to slot zero.
    exercise("Portable Hole", "exiles until", false, |v| {
        v.objects
            .iter()
            .any(|o| o.zone == ZoneName::Exile && o.exiled_by == Some(ObjectId(0)))
    });
}

#[test]
fn costs_tokens_returned_spells_and_coin_flips_are_reachable() {
    // by-0dg: seven graveyard cards expose the additional-cost picker during the cast.
    exercise("Abhorrent Oculus", "six of seven", false, |v| {
        matches!(&v.status, Status::Deciding { decision }
        if decision.prompt.is(Purpose::ExileCost) && decision.candidates.len() > 1)
    });
    // by-kgc: the source of two tokens is a resolving spell, unlike Staff's permanent.
    exercise("Get Lost", "on a creature", false, |v| {
        v.objects
            .iter()
            .filter(|o| o.name.as_deref() == Some("Map Token") && o.zone == ZoneName::Battlefield)
            .count()
            == 2
    });
    // by-b0j/by-afh/by-cqf/by-wkr/by-1w9: permanent triggers and activated ability sources.
    exercise(
        "Beza, the Bounding Spring",
        "every comparison true",
        false,
        |v| {
            v.objects
                .iter()
                .filter(|o| o.token && o.zone == ZoneName::Battlefield && o.controller == Side::You)
                .count()
                >= 3
        },
    );
    exercise(
        "Staff of the Storyteller",
        "story-counter-spent-to-draw-2",
        false,
        |v| v.events.iter().any(|e| json!(e)["kind"] == "draw"),
    );
    exercise(
        "Currency Converter",
        "exiles-its-own-loots-discard-for-a-rogue-0",
        false,
        |v| {
            v.objects
                .iter()
                .any(|o| o.zone == ZoneName::Exile && o.exiled_with.is_some())
        },
    );
    // by-e0l/by-8uw: the countered spell travels from the stack back to a hand.
    exercise("Reprieve", "spell with X", false, |v| {
        v.events.iter().any(|e| {
            let e = json!(e);
            e["kind"] == "zoneChange" && e["from"] == "stack" && e["to"] == "hand"
        })
    });
    // by-4og: the prepared upkeep exposes the coin-flip animation from either seat.
    for opposite in [false, true] {
        exercise("Mana Crypt", "flip lost", opposite, |v| {
            v.events.iter().any(|e| json!(e)["kind"] == "coinFlip")
        });
    }
}

#[test]
fn a_response_waits_until_the_permanent_has_resolved() {
    // Filtering out the viewer's actions alone would let Sink target the Hole spell,
    // although this example is about removing the permanent underneath its trigger.
    exercise("Portable Hole", "gone before its trigger", false, |v| {
        v.events.iter().any(|e| {
            let e = json!(e);
            e["kind"] == "cast"
                && v.objects.iter().any(|o| {
                    o.name.as_deref() == Some("Sink into Stupor") && json!(o.slot) == e["slot"]
                })
        }) && v
            .objects
            .iter()
            .any(|o| o.name.as_deref() == Some("Portable Hole") && o.zone == ZoneName::Battlefield)
    });
}

#[test]
fn undo_rewinds_the_opponents_response_too() {
    let variant = variant("Oust", "opposing counterspell");
    let (tx, rx) = channel();
    let session = start(variant, variant.controller, move |u, _| {
        let _ = tx.send(u.view);
    })
    .unwrap();
    let mut responses = 0;
    let mut rewinding = false;
    let mut repeated = false;
    for _ in 0..80 {
        let v = rx.recv_timeout(Duration::from_secs(3)).unwrap();
        responses += usize::from(v.events.iter().any(|e| {
            let e = json!(e);
            e["kind"] == "cast"
                && v.objects.iter().any(|o| {
                    o.name.as_deref() == Some("Counterspell") && json!(o.slot) == e["slot"]
                })
        }));
        if responses >= 2 {
            return;
        }
        let Status::Deciding { decision } = &v.status else {
            continue;
        };
        if responses == 1 && !repeated {
            rewinding = true;
        }
        let cast = decision
            .candidates
            .iter()
            .position(|c| matches!(c.option, Candidate::Cast { slot, .. } if v.objects.iter().any(|o| o.slot == slot && o.name.as_deref() == Some("Oust"))));
        if rewinding && cast.is_none() {
            session.undo(decision.id).unwrap();
            continue;
        }
        if rewinding {
            rewinding = false;
            repeated = true;
        }
        let index = cast
            .or_else(|| {
                decision
                    .candidates
                    .iter()
                    .position(|c| matches!(c.option, Candidate::Pass))
            })
            .unwrap_or(0);
        session
            .choose(decision.id, CandidateIndex(u16::try_from(index).unwrap()))
            .unwrap();
    }
    panic!("Undo consumed the opponent's response instead of replaying it");
}

#[test]
fn adeline_attack_offers_the_opposing_planeswalker() {
    exercise(
        "Adeline, Resplendent Cathar",
        "attacks a planeswalker",
        false,
        |v| {
            matches!(&v.status, Status::Deciding { decision }
            if decision.candidates.iter().any(|c| matches!(c.option, Candidate::Defender { planeswalker: Some(_) }))
            && decision.candidates.iter().any(|c| matches!(c.option, Candidate::Defender { planeswalker: None })))
        },
    );
}
