//! What each prompt says of itself on the wire (`DecisionView::prompt`), as the cards' own
//! scenarios reach it: the UI reads a question's shape, purpose and words from it, never from
//! the card asking.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use beryl_cards::{CARDS, SCENARIOS, names};
use beryl_engine::card::{Part, Purpose};
use beryl_engine::state::Pending;
use beryl_engine::two_player::TwoPlayerSeat as _;
use beryl_engine::{Observation, Scratch, view};
use beryl_session::prompt::{Lies, PlaceName, PromptView};
use beryl_session::protocol::{CardHandling, DecisionView, Moment, Side, player_view};
use beryl_session::{DecisionId, SessionId, Status, StopsSetting};

/// A prompt in one line, as these tests expect it: the decision's kind, then the prompt's
/// shape, purpose and part, its title, its two numbers, its cost, where its cards go, whose
/// cards it shows and where they lie, and how many objects it is about.
fn line(decision: &DecisionView) -> String {
    let PromptView {
        shape,
        purpose,
        part,
        title,
        count,
        most,
        places,
        cost,
        about,
        from,
        ..
    } = &*decision.prompt;
    let mut out = format!(
        "{} {} {purpose} {}: {title:?} {count}/{most}",
        name(&decision.kind),
        name(shape),
        name(part)
    );
    if let Some(cost) = cost {
        write!(out, " {cost}").unwrap();
    }
    for place in places.iter().filter(|&&place| place != PlaceName::None) {
        write!(out, " >{}", name(place)).unwrap();
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

/// Every decision `card`'s scenarios meet, as its seat is sent it.
fn decisions(card: &str) -> Vec<DecisionView> {
    let (_, scenarios) = SCENARIOS
        .iter()
        .find(|&&(name, _)| name == card)
        .expect("the card's scenarios");
    let mut seen = Vec::new();
    for scenario in *scenarios {
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
                Observation::new(view(s, t, &mut x), d.player),
                Moment::Deciding(id, d, resolving),
                StopsSetting::SMART,
                &[],
            );
            let Status::Deciding { decision } = v.status else {
                panic!("no decision is open");
            };
            seen.push(decision);
        })
        .unwrap_or_else(|e| panic!("{e}"));
    }
    seen
}

/// The distinct prompts of `purposes` that `card`'s scenarios meet, in the order they first
/// come, each in one [`line`].
fn asks(card: &str, purposes: &[Purpose]) -> Vec<String> {
    let mut seen = Vec::new();
    for decision in decisions(card) {
        let asked = line(&decision);
        if purposes.iter().any(|&p| decision.prompt.is(p)) && !seen.contains(&asked) {
            seen.push(asked);
        }
    }
    seen
}

#[test]
fn a_token_awaiting_its_attack_target_is_public_from_both_seats() {
    let (_, scenarios) = SCENARIOS
        .iter()
        .find(|&&(name, _)| name == names::ADELINE_RESPLENDENT_CATHAR)
        .unwrap();
    let scenario = scenarios
        .iter()
        .find(|scenario| scenario.name == "Adeline's Human attacks a planeswalker")
        .unwrap();
    let mut checked = false;
    beryl_scenario::visit(&CARDS, scenario, |t, s, d| {
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
            Observation::new(shown, d.player),
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
            line(&decision),
            "attackTarget declare attack aim: \"Choose attack target\" 0/0 \
             you's battlefield about 1"
        );
        let slots = decision.prompt.about;
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
    let (_, scenarios) = SCENARIOS
        .iter()
        .find(|&&(name, _)| name == names::STOCK_UP)
        .unwrap();
    let mut checked = false;
    beryl_scenario::visit(&CARDS, &scenarios[0], |t, s, d| {
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
            Observation::new(shown, d.player),
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
        asks(names::AANGS_ICEBERG, &[Purpose::Waterbend, Purpose::Scry]),
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
        asks(names::THUNDERTRAP_TRAINER, &[Purpose::Look]),
        [
            "yesNo pick look whether: \"Top Four\" 4/1 >hand you's library about 2",
            "chooseObject pick look pick: \"Top Four\" 4/1 >hand you's library about 2",
        ]
    );
    let look = decisions(names::THUNDERTRAP_TRAINER)
        .into_iter()
        .find(|d| d.prompt.is(Purpose::Look))
        .unwrap();
    assert_eq!(look.prompt.words.confirm, "Put in Hand");
    assert_eq!(look.prompt.words.decline, "Take Nothing");
}

#[test]
fn elite_spellbinder_looks_at_the_opponents_hand_for_a_card_to_exile() {
    let asked = asks(names::ELITE_SPELLBINDER, &[Purpose::Look]);
    assert!(!asked.is_empty());
    for look in &asked {
        // A hand is looked at whole: the prompt counts no cards.
        let rest = look
            .strip_prefix("yesNo pick look whether: ")
            .or_else(|| look.strip_prefix("chooseObject pick look pick: "))
            .unwrap_or_else(|| panic!("{look}"));
        let about = rest
            .strip_prefix("\"Opponent's Hand\" 0/1 >exile opponent's hand about ")
            .unwrap_or_else(|| panic!("{look}"));
        assert!(about.parse::<usize>().is_ok_and(|n| n > 0), "{look}");
    }
    let look = decisions(names::ELITE_SPELLBINDER)
        .into_iter()
        .find(|d| d.prompt.is(Purpose::Look))
        .unwrap();
    assert_eq!(look.prompt.words.confirm, "Exile");
    assert_eq!(look.prompt.words.decline, "Exile Nothing");
}

#[test]
fn jace_fateseals_and_puts_two_cards_back() {
    assert_eq!(
        asks(
            names::JACE_THE_MIND_SCULPTOR,
            &[Purpose::Fateseal, Purpose::PutBack]
        ),
        [
            "yesNo yesNo fateseal pick: \"Top of Library\" 1/0 >bottom >top \
             opponent's library about 1",
            "yesNo yesNo fateseal pick: \"Top of Library\" 1/0 >bottom >top \
             you's library about 1",
            "chooseObject split put-back first: \"Put Back\" 0/2 >onTop you's hand",
            "chooseObject split put-back first: \"Put Back\" 0/1 >onTop you's hand",
        ]
    );
}

#[test]
fn a_maps_explore_asks_whether_the_revealed_card_goes_to_the_graveyard() {
    assert_eq!(
        asks(names::GET_LOST, &[Purpose::Explore]),
        ["yesNo yesNo explore pick: \"Revealed\" 1/0 >graveyard >top you's library about 1"]
    );
    let explore = decisions(names::GET_LOST)
        .into_iter()
        .find(|d| d.prompt.is(Purpose::Explore))
        .unwrap();
    assert_eq!(explore.prompt.words.yes, "Graveyard");
    assert_eq!(explore.prompt.words.no, "Keep on Top");
}

#[test]
fn time_spiral_untaps_up_to_six_lands() {
    assert_eq!(
        asks(names::TIME_SPIRAL, &[Purpose::Untap]),
        ["chooseUpTo pick untap pick: \"Untap\" 0/6 you's battlefield"]
    );
}

#[test]
fn trinket_mage_may_search_for_up_to_one_card() {
    assert_eq!(
        asks(
            names::TRINKET_MAGE,
            &[Purpose::MaySearch, Purpose::Search, Purpose::Fateseal]
        ),
        [
            "yesNo yesNo may-search pick: \"Search?\" 0/0",
            "chooseUpTo pick search pick: \"Search\" 0/1 you's library",
            // The known-top search scenario first uses Jace to look at that card.
            "yesNo yesNo fateseal pick: \"Top of Library\" 1/0 >bottom >top \
             you's library about 1",
        ]
    );
}

#[test]
fn winds_of_abandon_searches_for_a_land_per_creature_exiled() {
    assert_eq!(
        asks(names::WINDS_OF_ABANDON, &[Purpose::Search]),
        [
            // Each creature's controller searches their own library.
            "chooseUpTo pick search pick: \"Search\" 0/1 you's library",
            "chooseUpTo pick search pick: \"Search\" 0/2 you's library",
        ]
    );
}

#[test]
fn mana_tithe_asks_its_target_to_pay_one() {
    assert_eq!(
        asks(names::MANA_TITHE, &[Purpose::UnlessPays]),
        ["yesNo yesNo unless-pays pick: \"Pay {1}?\" 0/0 {1}"]
    );
    let pays = decisions(names::MANA_TITHE)
        .into_iter()
        .find(|d| d.prompt.is(Purpose::UnlessPays))
        .unwrap();
    assert_eq!(pays.prompt.words.yes, "Pay {1}");
    assert_eq!(pays.prompt.words.no, "Don't Pay");
}

#[test]
fn subtlety_asks_the_spells_owner_top_or_bottom_and_its_evoke_exile() {
    assert_eq!(
        asks(names::SUBTLETY, &[Purpose::TopOrBottom, Purpose::ExileCost]),
        [
            "yesNo yesNo top-or-bottom pick: \"Top or Bottom?\" 0/0 >top >bottom \
             you's stack about 1",
            "chooseObject pick exile-cost pick: \"Exile a card\" 0/1 you's hand",
        ]
    );
}

#[test]
fn abhorrent_oculus_asks_for_graveyard_cards_then_a_card_to_manifest() {
    let asked = asks(
        names::ABHORRENT_OCULUS,
        &[Purpose::ExileCost, Purpose::ManifestDread],
    );
    // Six cards from the graveyard, one pick at a time, the count falling; the graveyard's
    // are laid out under the one word.
    assert_eq!(
        asked[0],
        "chooseObject pick exile-cost pick: \"Exile\" 0/6 you's graveyard"
    );
    assert_eq!(
        asked.last().map(String::as_str),
        Some(
            "chooseObject pick manifest-dread pick: \"Manifest Dread\" 2/1 \
             >battlefield >graveyard you's library"
        )
    );
}

/// What the scenarios' prompts of one purpose are titled.
enum Titled {
    /// Each of these, and nothing else.
    As(&'static [&'static str]),
    /// By the name of a card.
    ByName,
}

/// Every purpose the cards' scenarios reach, with its shape and what its prompts are titled:
/// a scenario that reaches another purpose adds its line here.
const REACHED: &[(Purpose, &str, Titled)] = &[
    (Purpose::Priority, "pick", Titled::As(&[""])),
    (Purpose::Target, "pick", Titled::As(&["Choose a target"])),
    (Purpose::Choose, "pick", Titled::As(&["Choose a card"])),
    (
        Purpose::Discard,
        "pick",
        Titled::As(&["Discard a basic land or 2 cards", "Discard a card"]),
    ),
    (
        Purpose::Look,
        "pick",
        Titled::As(&["Opponent's Hand", "Top Four"]),
    ),
    (Purpose::Search, "pick", Titled::As(&["Search"])),
    (Purpose::Untap, "pick", Titled::As(&["Untap"])),
    (
        Purpose::ManifestDread,
        "pick",
        Titled::As(&["Manifest Dread"]),
    ),
    (
        Purpose::ExileCost,
        "pick",
        Titled::As(&["Exile", "Exile a card"]),
    ),
    (
        Purpose::TriggerOrder,
        "order",
        Titled::As(&["Put a trigger on the stack"]),
    ),
    (Purpose::Scry, "split", Titled::As(&["Scry"])),
    (
        Purpose::Arrange,
        "split",
        Titled::As(&["Top Five", "Top Four", "Top Three", "Top Two"]),
    ),
    (Purpose::PutBack, "split", Titled::As(&["Put Back"])),
    (Purpose::MaySearch, "yesNo", Titled::As(&["Search?"])),
    (
        Purpose::Move,
        "yesNo",
        Titled::As(&["Exile that card from your graveyard?"]),
    ),
    (Purpose::Fateseal, "yesNo", Titled::As(&["Top of Library"])),
    (
        Purpose::TopOrBottom,
        "yesNo",
        Titled::As(&["Top or Bottom?"]),
    ),
    (Purpose::Explore, "yesNo", Titled::As(&["Revealed"])),
    (
        Purpose::UnlessPays,
        "yesNo",
        Titled::As(&["Pay {1}?", "Pay {3}?"]),
    ),
    (Purpose::PayLife, "yesNo", Titled::ByName),
    (Purpose::X, "number", Titled::As(&["Choose X"])),
    (Purpose::CastOffer, "mode", Titled::ByName),
    (
        Purpose::Waterbend,
        "pay",
        Titled::As(&["Tap permanents to Waterbend"]),
    ),
    (
        Purpose::Attack,
        "declare",
        Titled::As(&["Choose attack target", "Declare attackers"]),
    ),
    (Purpose::Block, "declare", Titled::As(&["Declare blockers"])),
];

#[test]
fn every_purpose_the_scenarios_reach_is_sent_with_its_shape_and_title() {
    let mut titles: BTreeMap<String, (String, BTreeSet<String>)> = BTreeMap::new();
    for &(card, _) in SCENARIOS {
        for decision in decisions(card) {
            let prompt = *decision.prompt;
            // The state is not the session's yet.
            assert_eq!((prompt.revision, &prompt.state), (0, &None), "{card}");
            assert!(
                prompt.marks.is_empty() && prompt.buttons.is_empty(),
                "{card}"
            );
            let (shape, seen) = titles.entry(prompt.purpose).or_default();
            *shape = name(&prompt.shape);
            seen.insert(prompt.title);
        }
    }
    for (purpose, shape, titled) in REACHED {
        let (sent, seen) = titles
            .remove(purpose.name())
            .unwrap_or_else(|| panic!("no scenario reaches {purpose:?}"));
        assert_eq!(sent, *shape, "{purpose:?}");
        match titled {
            Titled::As(expected) => {
                assert_eq!(seen.iter().collect::<Vec<_>>(), *expected, "{purpose:?}");
            }
            Titled::ByName => assert!(
                seen.iter()
                    .all(|title| !title.is_empty() && !title.contains('{')),
                "{purpose:?}: {seen:?}"
            ),
        }
    }
    assert!(
        titles.is_empty(),
        "purposes reached and not expected: {titles:?}"
    );
}

#[test]
fn the_observer_does_not_see_handling_for_a_graveyard_exile_cost() {
    use beryl_engine::{Choice, DecisionKind, Zone};
    use beryl_scenario::{Action, P0, Permanent, Scenario};

    const SCENARIO: Scenario =
        Scenario::new("Oculus paid from the graveyard with a card still in hand")
            .battlefield(P0, &[Permanent::new(names::ISLAND); 3])
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
