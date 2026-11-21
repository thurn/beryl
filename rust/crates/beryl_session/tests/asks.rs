//! What each prompt says on the wire (`DecisionView::prompt`): the question's shape,
//! purpose and words.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::common::fixtures::{
    ADELINE_ATTACKS_PLANESWALKER, MAP_EXPLORES_NONLAND, OCULUS_EXILES_SIX_CARDS,
    OCULUS_MANIFESTS_DREAD, SPELLBINDER, STOCK_UP, SUBTLETY_HARD_CAST,
    TRINKET_MAGE_FINDS_PORTABLE_HOLE,
};

use beryl_cards::{CARDS, library, names};
use beryl_engine::card::{ManaCost, Part, Prompt, Purpose, Shape};
use beryl_engine::state::Pending;
use beryl_engine::two_player::TwoPlayerSeat as _;
use beryl_engine::{Observation, Scratch, view};
use beryl_scenario::Scenario;
use beryl_session::prompt::{Lies, PlaceName, PromptView, ShapeBody, shape_name};
use beryl_session::protocol::{CardHandling, DecisionView, Moment, Side, player_view};
use beryl_session::{DecisionId, SessionId, Status, StopsSetting};
use scenarios::{
    ICEBERG_BOTTOM, ICEBERG_MANA_SOURCES, ICEBERG_SCRY, SUBTLETY_PITCH, THUNDERTRAP_TRAINER,
    TRINKET_KNOWN_TOP, WINDS_OVERLOAD, WINDS_SINGLE,
};

mod scenarios;

/// A prompt in one line, as these tests expect it: the decision's kind, then the prompt's
/// shape, purpose and part, its title, its two numbers, its cost, where its cards go, whose
/// cards it shows and where they lie, and how many objects it is about. The numbers, the
/// cost and the places are the engine's `record`, which the wire carries only where a
/// shape's state shows them.
fn line(decision: &DecisionView, record: &Prompt) -> String {
    let PromptView {
        purpose,
        part,
        title,
        count,
        about,
        from,
        ..
    } = &*decision.prompt;
    assert_eq!(
        count.unwrap_or(0),
        record.count,
        "the wire's count is the record's"
    );
    let mut out = format!(
        "{} {} {} {}: {title:?} {}/{}",
        name(&decision.kind),
        shape_name(purpose.shape()),
        purpose.name(),
        name(part),
        record.count,
        record.most,
    );
    if record.cost != ManaCost::default() {
        write!(out, " {}", beryl_card::render::mana_text(record.cost)).unwrap();
    }
    for place in record.places.into_iter().filter_map(PlaceName::of) {
        write!(out, " >{}", name(&place)).unwrap();
    }
    if let Some(Lies { player, zone }) = from {
        write!(out, " {}'s {}", name(player), name(zone)).unwrap();
    }
    if !about.is_empty() {
        write!(out, " about {}", about.len()).unwrap();
    }
    out
}

/// A fieldless wire enum's wire name.
fn name<T: serde::Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        _ => unreachable!("only fieldless enums are named this way"),
    }
}

/// Every decision the scenarios reach, as its seat receives it.
fn decisions(scenarios: &[Scenario]) -> Vec<DecisionView> {
    asked_in(scenarios)
        .into_iter()
        .map(|(view, _)| view)
        .collect()
}

/// Every decision the scenarios reach, with the engine's record of its prompt.
fn asked_in(scenarios: &[Scenario]) -> Vec<(DecisionView, Prompt)> {
    let mut seen = Vec::new();
    for scenario in scenarios {
        beryl_scenario::visit(&CARDS, scenario, |t, s, d| {
            let mut x = Scratch::new();
            // A paused resolution's entry is still on top of the committed stack.
            let resolving = matches!(s.pending(), Pending::Resolving(_))
                .then(|| s.stack().last())
                .flatten();
            let id = DecisionId {
                session: SessionId(1),
                seq: 1,
            };
            let v = player_view(
                t,
                Observation::new(view(s, t, &mut x), d.decider),
                Moment::Deciding(id, d, resolving),
                StopsSetting::SMART,
                &[],
            );
            let Status::Deciding { decision } = v.status else {
                panic!("no decision is open");
            };
            seen.push((decision, d.prompt));
        })
        .unwrap_or_else(|e| panic!("{e}"));
    }
    seen
}

/// The distinct prompts of `purposes` that these scenarios reach, in the order they first
/// come, each in one [`line`].
fn asks(scenarios: &[Scenario], purposes: &[Purpose]) -> Vec<String> {
    let mut seen = Vec::new();
    for (decision, record) in asked_in(scenarios) {
        let asked = line(&decision, &record);
        if purposes.iter().any(|&p| decision.prompt.is(p)) && !seen.contains(&asked) {
            seen.push(asked);
        }
    }
    seen
}

#[test]
fn a_token_awaiting_its_attack_target_is_public_from_both_seats() {
    let mut checked = false;
    beryl_scenario::visit(&CARDS, ADELINE_ATTACKS_PLANESWALKER, |t, s, d| {
        if d.prompt.purpose != Purpose::Attack || d.prompt.part != Part::Aim {
            return;
        }
        let attackers = d.about;
        let mut x = Scratch::new();
        let shown = view(s, t, &mut x);
        let resolving = matches!(s.pending(), Pending::Resolving(_))
            .then(|| s.stack().last())
            .flatten();
        if resolving.is_none() {
            return;
        }
        let id = DecisionId {
            session: SessionId(1),
            seq: 1,
        };
        let controller = player_view(
            t,
            Observation::new(shown, d.decider),
            Moment::Deciding(id, d, resolving),
            StopsSetting::SMART,
            &[],
        );
        let observer = player_view(
            t,
            Observation::new(shown, d.player.opponent()),
            Moment::Thinking {
                resolving,
                decision: Some(d),
            },
            StopsSetting::SMART,
            &[],
        );
        let Status::Deciding { decision } = controller.status else {
            panic!("the attack target is open");
        };
        assert_eq!(
            line(&decision, &d.prompt),
            "attackTarget declare attack aim: \"Choose attack target\" 0/0 \
             you's battlefield about 1"
        );
        let slots = decision.prompt.about.clone();
        assert_eq!(slots.len(), 1);
        assert_eq!(observer.status, Status::Thinking);
        assert_eq!(observer.pending_attackers, Some(slots.clone()));
        assert_eq!(controller.pending_attackers, Some(slots.clone()));
        assert_eq!(attackers.count() as usize, slots.len());
        assert!(
            observer
                .objects
                .iter()
                .any(|o| { o.slot == slots[0] && o.name.as_deref() == Some("Human Token") })
        );
        checked = true;
    })
    .unwrap();
    assert!(checked, "Adeline reached the Human's attack target");
}

#[test]
fn stock_up_observer_sees_the_group_size_without_seeing_its_cards_or_choices() {
    let mut checked = false;
    beryl_scenario::visit(&CARDS, STOCK_UP, |t, s, d| {
        if (d.prompt.purpose, d.prompt.part, d.prompt.most) != (Purpose::Arrange, Part::First, 2) {
            return;
        }
        let mut x = Scratch::new();
        let shown = view(s, t, &mut x);
        let resolving = s.stack().last();
        let id = DecisionId {
            session: SessionId(1),
            seq: 1,
        };
        let controller = player_view(
            t,
            Observation::new(shown, d.decider),
            Moment::Deciding(id, d, resolving),
            StopsSetting::SMART,
            &[],
        );
        let observer = player_view(
            t,
            Observation::new(shown, d.player.opponent()),
            Moment::Thinking {
                resolving,
                decision: Some(d),
            },
            StopsSetting::SMART,
            &[],
        );
        assert!(controller.handling.is_none());
        assert_eq!(observer.status, Status::Thinking);
        assert_eq!(
            observer.handling,
            Some(CardHandling::Library {
                side: Side::Opponent,
                count: Some(5),
                scry: None,
            }),
        );
        for slot in observer.players.opponent.library.iter().rev().take(5) {
            let object = |objects: &[beryl_session::protocol::ObjectView]| {
                objects
                    .iter()
                    .find(|o| o.slot == *slot)
                    .unwrap()
                    .name
                    .clone()
            };
            assert!(object(&controller.objects).is_some());
            assert!(object(&observer.objects).is_none());
        }
        checked = true;
    })
    .unwrap();
    assert!(checked, "Stock Up reached the first selection");
}

#[test]
fn aangs_iceberg_waterbends_its_cost_then_scries_two() {
    assert_eq!(
        asks(
            &[ICEBERG_BOTTOM, ICEBERG_SCRY, ICEBERG_MANA_SOURCES],
            &[Purpose::Waterbend, Purpose::Scry]
        ),
        [
            "manaPayment pay waterbend pick: \"Tap permanents to Waterbend\" 0/0 {3} \
             you's battlefield",
            // A card for the bottom, of the two and then of the one left; then the top's order.
            "chooseUpTo split scry first: \"Scry\" 2/2 >bottom >top you's library",
            "chooseUpTo split scry first: \"Scry\" 2/1 >bottom >top you's library",
            "chooseObject split scry second: \"Scry\" 2/0 >bottom >top you's library",
            // The mana-source scenario can continue after its first bender.
            "manaPayment pay waterbend pick: \"Tap permanents to Waterbend\" 0/0 {2} \
             you's battlefield",
        ]
    );
}

#[test]
fn thundertrap_trainer_looks_at_the_top_four_for_a_card_to_take() {
    assert_eq!(
        asks(&[THUNDERTRAP_TRAINER], &[Purpose::Look]),
        [
            "yesNo pick look whether: \"Put one of them into your hand?\" 4/1 >hand you's library about 2",
            "chooseObject pick look pick: \"Put one of them into your hand.\" 4/1 >hand you's library about 2",
        ]
    );
    let look = decisions(&[THUNDERTRAP_TRAINER])
        .into_iter()
        .find(|d| d.prompt.is(Purpose::Look))
        .unwrap();
    assert_eq!(look.prompt.words.confirm, "Submit {picked}");
    assert_eq!(look.prompt.words.decline, "Decline");
}

#[test]
fn elite_spellbinder_looks_at_the_opponents_hand_for_a_card_to_exile() {
    let asked = asks(&[*SPELLBINDER], &[Purpose::Look]);
    assert!(!asked.is_empty());
    for look in &asked {
        // A hand is looked at whole: the prompt counts no cards.
        let rest = look
            .strip_prefix("yesNo pick look whether: \"Exile one of them?\"")
            .or_else(|| look.strip_prefix("chooseObject pick look pick: \"Exile one of them.\""))
            .unwrap_or_else(|| panic!("{look}"));
        let about = rest
            .strip_prefix(" 0/1 >exile opponent's hand about ")
            .unwrap_or_else(|| panic!("{look}"));
        assert!(about.parse::<usize>().is_ok_and(|n| n > 0), "{look}");
    }
    let look = decisions(&[*SPELLBINDER])
        .into_iter()
        .find(|d| d.prompt.is(Purpose::Look))
        .unwrap();
    assert_eq!(look.prompt.words.confirm, "Submit {picked}");
    assert_eq!(look.prompt.words.decline, "Decline");
}

#[test]
fn jace_fateseals_and_puts_two_cards_back() {
    let scenarios: Vec<_> = library::actions::fateseal::SCENARIOS
        .iter()
        .chain(library::actions::fateseal::TOP_OR_BOTTOM)
        .chain(library::library::PUT_BACK_IN_ORDER)
        .chain(library::library::PUT_BACK)
        .copied()
        .collect();
    assert_eq!(
        asks(&scenarios, &[Purpose::Fateseal, Purpose::PutBack]),
        [
            "yesNo yesNo fateseal pick: \"Top of Library\" 1/0 >bottom >top \
             opponent's library about 1",
            "yesNo yesNo fateseal pick: \"Top of Library\" 1/0 >bottom >top \
             you's library about 1",
            "chooseObject split put-back first: \"Put two cards on top.\" 0/2 >onTop you's hand",
            "chooseObject split put-back first: \"Put a card on top.\" 0/1 >onTop you's hand",
        ]
    );
}

#[test]
fn a_maps_explore_asks_whether_the_revealed_card_goes_to_the_graveyard() {
    assert_eq!(
        asks(&[*MAP_EXPLORES_NONLAND], &[Purpose::Explore]),
        ["yesNo yesNo explore pick: \"Explore\" 1/0 >graveyard >top you's library about 1"]
    );
    let explore = decisions(&[*MAP_EXPLORES_NONLAND])
        .into_iter()
        .find(|d| d.prompt.is(Purpose::Explore))
        .unwrap();
    assert_eq!(explore.prompt.words.yes, "Graveyard");
    assert_eq!(explore.prompt.words.no, "Library");
}

#[test]
fn time_spiral_untaps_up_to_six_lands() {
    use beryl_engine::DecisionKind;
    use beryl_scenario::{Action, P0, P1, Pick};

    const SCENARIO: Scenario = Scenario::new("Time Spiral offers to untap lands after the shuffle")
        .battlefield(P0, &[names::ISLAND; 6])
        .hand(P0, &[names::TIME_SPIRAL])
        .library(P0, &[names::ISLAND; 7])
        .library(P1, &[names::PLAINS; 7])
        .actions(&[
            Action::cast(P0, names::TIME_SPIRAL),
            Action::answer(P0, DecisionKind::ChooseUpTo, Pick::Done),
        ]);
    let (decision, record) = asked_in(&[SCENARIO])
        .into_iter()
        .find(|(decision, _)| decision.prompt.is(Purpose::Untap))
        .expect("Time Spiral offers to untap lands");
    assert_eq!(
        line(&decision, &record),
        "chooseUpTo pick untap pick: \"Untap up to six lands.\" 0/6 you's battlefield"
    );
}

#[test]
fn trinket_mage_may_search_for_up_to_one_card() {
    assert_eq!(
        asks(
            &[*TRINKET_MAGE_FINDS_PORTABLE_HOLE, TRINKET_KNOWN_TOP],
            &[Purpose::MaySearch, Purpose::Search, Purpose::Fateseal]
        ),
        [
            "yesNo yesNo may-search pick: \"Search your library?\" 0/0",
            "chooseUpTo pick search pick: \"Search for a card.\" 0/1 you's library",
            // The known-top search scenario first uses Jace to look at that card.
            "yesNo yesNo fateseal pick: \"Top of Library\" 1/0 >bottom >top \
             you's library about 1",
        ]
    );
}

#[test]
fn winds_of_abandon_searches_for_a_land_per_creature_exiled() {
    assert_eq!(
        asks(&[WINDS_SINGLE, WINDS_OVERLOAD], &[Purpose::Search]),
        [
            // Each creature's controller searches their own library.
            "chooseUpTo pick search pick: \"Search for a card.\" 0/1 you's library",
            "chooseUpTo pick search pick: \"Search for a card.\" 0/2 you's library",
        ]
    );
}

#[test]
fn mana_tithe_asks_its_target_to_pay_one() {
    assert_eq!(
        asks(
            library::actions::counter::UNLESS_PAID,
            &[Purpose::UnlessPays]
        ),
        ["yesNo yesNo unless-pays pick: \"Pay {1}.\" 0/0 {1}"]
    );
    let pays = decisions(library::actions::counter::UNLESS_PAID)
        .into_iter()
        .find(|d| d.prompt.is(Purpose::UnlessPays))
        .unwrap();
    assert_eq!(pays.prompt.words.yes, "Pay {1}");
    assert_eq!(pays.prompt.words.no, "Decline");
}

#[test]
fn subtlety_asks_the_spells_owner_top_or_bottom_and_its_evoke_exile() {
    assert_eq!(
        asks(
            &[*SUBTLETY_HARD_CAST, SUBTLETY_PITCH],
            &[Purpose::TopOrBottom, Purpose::ExileCost]
        ),
        [
            "yesNo yesNo top-or-bottom pick: \"Subtlety\" 0/0 >top >bottom \
             you's stack about 1",
            "chooseObject pick exile-cost pick: \"Exile a card.\" 0/1 you's hand",
        ]
    );
}

#[test]
fn abhorrent_oculus_asks_for_graveyard_cards_then_a_card_to_manifest() {
    let asked = asks(
        &[*OCULUS_EXILES_SIX_CARDS, *OCULUS_MANIFESTS_DREAD],
        &[Purpose::ExileCost, Purpose::ManifestDread],
    );
    // Six cards from the graveyard, one pick at a time, the count falling; the graveyard's
    // are laid out under the one word.
    assert_eq!(
        asked[0],
        "chooseObject pick exile-cost pick: \"Exile six other cards.\" 0/6 you's graveyard"
    );
    assert_eq!(
        asked.last().map(String::as_str),
        Some(
            "chooseObject pick manifest-dread pick: \"Manifest one of them.\" 2/1 \
             >battlefield >graveyard you's library"
        )
    );
}

/// Every prompt the cards' scenarios reach is sent whole, whatever card asks it and whatever
/// it is for: the shape its purpose fixes, and a title with every hole filled. No list of the
/// purposes reached or of their titles is kept here, so a card with a new question, or a new
/// purpose, changes nothing in this file.
#[test]
fn every_prompt_the_scenarios_reach_is_sent_with_its_shape_and_a_whole_title() {
    let shapes: BTreeMap<&str, &str> = (Purpose::ALL.iter())
        .map(|purpose| (purpose.name(), shape_name(purpose.shape())))
        .collect();
    let mut reached = BTreeSet::new();
    for &(card, scenarios) in beryl_cards::scenarios() {
        for decision in decisions(scenarios) {
            let prompt = *decision.prompt;
            // The state is the game's to add: a view of a position has none.
            assert_eq!(
                (prompt.revision, &prompt.body),
                (0, &ShapeBody::Candidates),
                "{card}"
            );
            assert!(
                prompt.marks.is_empty() && prompt.buttons.is_empty(),
                "{card}"
            );
            let purpose = prompt.purpose.name();
            assert_eq!(
                Some(&shape_name(prompt.shape())),
                shapes.get(purpose),
                "{card}: {purpose}"
            );
            // The turn button is the priority prompt: it has no title.
            let title = &prompt.title;
            assert_eq!(title.is_empty(), purpose == "priority", "{card}: {purpose}");
            // A hole left unfilled is a lowercase word in braces; a mana symbol is not.
            let unfilled = (title.split('{').skip(1))
                .any(|rest| rest.starts_with(|c: char| c.is_ascii_lowercase()));
            assert!(!unfilled, "{card}: {purpose} is titled {title:?}");
            reached.insert(purpose);
        }
    }
    // The scenarios reach a prompt of every shape but the one no card asks yet.
    for &shape in Shape::ALL {
        let asked = shape != Shape::Divide;
        let seen = (Purpose::ALL.iter())
            .any(|purpose| purpose.shape() == shape && reached.contains(purpose.name()));
        assert!(seen || !asked, "no scenario reaches a prompt of {shape:?}");
    }
}

#[test]
fn the_observer_does_not_see_handling_for_a_graveyard_exile_cost() {
    use beryl_engine::{Choice, DecisionKind, Zone};
    use beryl_scenario::{Action, P0, Permanent};

    const SCENARIO: Scenario =
        Scenario::new("Oculus paid from the graveyard with a card still in hand")
            .permanents(P0, &[Permanent::new(names::ISLAND); 3])
            .hand(P0, &[names::ABHORRENT_OCULUS, names::ISLAND])
            .graveyard(
                P0,
                &[
                    names::COUNTERSPELL,
                    names::MANA_DRAIN,
                    names::STOCK_UP,
                    names::REPRIEVE,
                    names::OUST,
                    names::THIRST_FOR_DISCOVERY,
                    names::PLAINS,
                ],
            )
            .actions(&[
                Action::cast(P0, names::ABHORRENT_OCULUS),
                Action::choose(P0, DecisionKind::ChooseObject, names::THIRST_FOR_DISCOVERY),
            ]);
    let mut checked = false;
    beryl_scenario::visit(&CARDS, &SCENARIO, |t, s, d| {
        let in_graveyard = |slot| s.object(slot).zone() == Zone::Graveyard;
        if d.prompt.purpose != Purpose::ExileCost
            || !d.candidates.object(Choice(0)).is_some_and(in_graveyard)
        {
            return;
        }
        let mut scratch = Scratch::new();
        let observer = player_view(
            t,
            Observation::new(view(s, t, &mut scratch), d.player.opponent()),
            Moment::Thinking {
                resolving: None,
                decision: Some(d),
            },
            StopsSetting::SMART,
            &[],
        );
        assert!(observer.handling.is_none());
        checked = true;
    })
    .unwrap();
    assert!(checked, "Oculus reached the graveyard exile payment");
}
