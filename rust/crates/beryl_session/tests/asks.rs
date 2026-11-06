//! What each resolution prompt asks (`DecisionView::ask`), as the cards' own scenarios reach
//! it: the UI and the agents read a question's meaning from it, never from the card asking.

use beryl_cards::{CARDS, SCENARIOS, names};
use beryl_engine::state::Pending;
use beryl_engine::{Observation, Scratch, view};
use beryl_session::protocol::{AskView, Kind, Moment, Side, ZoneName, player_view};
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
                StopsSetting::Smart,
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
fn subtlety_asks_the_spells_owner_top_or_bottom() {
    let asked = asks(names::SUBTLETY);
    assert!(!asked.is_empty());
    for ask in &asked {
        assert!(
            matches!(ask, (Kind::YesNo, AskView::TopOrBottom { .. })),
            "{asked:?}"
        );
    }
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
