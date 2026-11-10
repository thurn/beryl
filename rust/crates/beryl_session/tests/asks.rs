//! What each resolution prompt asks (`DecisionView::ask`), as the cards' own scenarios reach
//! it: the UI and the agents read a question's meaning from it, never from the card asking.

use beryl_cards::{CARDS, SCENARIOS, names};
use beryl_engine::state::Pending;
use beryl_engine::{Ask, Observation, Scratch, view};
use beryl_session::protocol::{AskView, CardHandling, Kind, Moment, Side, ZoneName, player_view};
use beryl_session::{DecisionId, SessionId, Status, StopsSetting};

/// The distinct asks other than [`AskView::Plain`] that `card`'s scenarios meet, in the order
/// they first come, with the kind of decision each comes with.
fn asks(card: &str) -> Vec<(Kind, AskView)> {
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
            let asked = (decision.kind, decision.ask);
            if asked.1 != AskView::Plain && !seen.contains(&asked) {
                seen.push(asked);
            }
        })
        .unwrap_or_else(|e| panic!("{e}"));
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
        let Ask::AttackTarget { attackers } = d.ask else {
            return;
        };
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
        let AskView::AttackTarget { attackers: slots } = decision.ask else {
            panic!("the target choice names its attackers");
        };
        assert_eq!(slots.len(), 1);
        assert_eq!(observer.status, Status::Thinking);
        assert_eq!(observer.pending_attackers, Some(slots.clone()));
        assert_eq!(controller.pending_attackers, Some(slots.clone()));
        assert_eq!(attackers.iter().count(), slots.len());
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
        if !matches!(d.ask, Ask::Take { take: 2, .. }) {
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
    let scry = AskView::Scry { count: 2 };
    assert_eq!(
        asks(names::AANGS_ICEBERG),
        [
            (
                Kind::Waterbend,
                AskView::Waterbend {
                    cost: "{3}".to_owned()
                }
            ),
            (Kind::ChooseUpTo, scry.clone()),
            (Kind::ChooseObject, scry)
        ]
    );
}

#[test]
fn thundertrap_trainer_looks_at_the_top_four_for_a_card_to_take() {
    let asked = asks(names::THUNDERTRAP_TRAINER);
    let [(Kind::YesNo, look), ..] = asked.as_slice() else {
        panic!("no look: {asked:?}");
    };
    let AskView::Look {
        player: Side::You,
        zone: ZoneName::Library,
        count: 4,
        to: ZoneName::Hand,
        eligible,
    } = look
    else {
        panic!("not a look at the top four: {look:?}");
    };
    assert!(!eligible.is_empty());
    assert!(asked.iter().any(|(kind, _)| *kind == Kind::ChooseObject));
}

#[test]
fn elite_spellbinder_looks_at_the_opponents_hand_for_a_card_to_exile() {
    let asked = asks(names::ELITE_SPELLBINDER);
    assert!(!asked.is_empty());
    for (kind, look) in &asked {
        assert!(matches!(kind, Kind::YesNo | Kind::ChooseObject), "{kind:?}");
        assert!(
            matches!(
                look,
                AskView::Look {
                    player: Side::Opponent,
                    zone: ZoneName::Hand,
                    to: ZoneName::Exile,
                    ..
                }
            ),
            "{look:?}"
        );
    }
}

#[test]
fn jace_fateseals_and_puts_two_cards_back() {
    assert_eq!(
        asks(names::JACE_THE_MIND_SCULPTOR),
        [
            (
                Kind::YesNo,
                AskView::Fateseal {
                    player: Side::Opponent
                }
            ),
            (Kind::YesNo, AskView::Fateseal { player: Side::You }),
            (Kind::ChooseObject, AskView::PutBack { count: 2 }),
            (Kind::ChooseObject, AskView::PutBack { count: 1 }),
        ]
    );
}

#[test]
fn a_maps_explore_asks_whether_the_revealed_card_goes_to_the_graveyard() {
    assert_eq!(asks(names::GET_LOST), [(Kind::YesNo, AskView::Explore)]);
}

#[test]
fn time_spiral_untaps_up_to_six_lands() {
    assert_eq!(
        asks(names::TIME_SPIRAL),
        [(Kind::ChooseUpTo, AskView::Untap { max: 6 })]
    );
}

#[test]
fn trinket_mage_may_search_for_up_to_one_card() {
    assert_eq!(
        asks(names::TRINKET_MAGE),
        [
            (Kind::YesNo, AskView::MaySearch),
            (Kind::ChooseUpTo, AskView::Search { max: 1 }),
            // The known-top search scenario first uses Jace to look at that card.
            (Kind::YesNo, AskView::Fateseal { player: Side::You })
        ]
    );
}

#[test]
fn winds_of_abandon_searches_for_a_land_per_creature_exiled() {
    assert_eq!(
        asks(names::WINDS_OF_ABANDON),
        [
            (Kind::ChooseUpTo, AskView::Search { max: 1 }),
            (Kind::ChooseUpTo, AskView::Search { max: 2 })
        ]
    );
}

#[test]
fn mana_tithe_asks_its_target_to_pay_one() {
    assert_eq!(
        asks(names::MANA_TITHE),
        [(
            Kind::YesNo,
            AskView::UnlessPays {
                cost: "{1}".to_owned()
            }
        )]
    );
}

#[test]
fn subtlety_asks_the_spells_owner_top_or_bottom_and_its_evoke_exile() {
    let asked = asks(names::SUBTLETY);
    let top_or_bottom =
        |ask: &(Kind, AskView)| matches!(ask, (Kind::YesNo, AskView::TopOrBottom { .. }));
    let exile = |ask: &(Kind, AskView)| {
        matches!(
            ask,
            (
                Kind::ExileCost,
                AskView::ExileCards {
                    zone: ZoneName::Hand,
                    count: 1
                }
            )
        )
    };
    assert!(asked.iter().any(top_or_bottom), "{asked:?}");
    assert!(asked.iter().any(exile), "{asked:?}");
    assert!(
        asked.iter().all(|a| top_or_bottom(a) || exile(a)),
        "{asked:?}"
    );
}

#[test]
fn abhorrent_oculus_asks_for_graveyard_cards_then_a_card_to_manifest() {
    assert_eq!(
        asks(names::ABHORRENT_OCULUS),
        [
            (
                Kind::ExileCost,
                AskView::ExileCards {
                    zone: ZoneName::Graveyard,
                    count: 6
                }
            ),
            (Kind::ChooseObject, AskView::ManifestDread),
        ]
    );
}

#[test]
fn the_observer_does_not_see_handling_for_a_graveyard_exile_cost() {
    use beryl_engine::{DecisionKind, Zone};
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
                Action::choose(P0, DecisionKind::ExileCost, names::THIRST_FOR_DISCOVERY),
            ]);
    let mut checked = false;
    beryl_scenario::visit(&CARDS, &SCENARIO, |t, s, d| {
        if !matches!(
            d.ask,
            Ask::ExileCards {
                zone: Zone::Graveyard,
                ..
            }
        ) {
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
