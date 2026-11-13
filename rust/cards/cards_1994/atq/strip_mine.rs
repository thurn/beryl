// Generated from the card store by `just card header`: notes go below the end marker.
//! Strip Mine
//! Land
//!
//! {T}: Add {C}.
//! {T}, Sacrifice this land: Destroy target land.
// End of the generated card header.

use crate::{names, printed};
use beryl_card::effect::build::{TARGET, destroy};
use beryl_card::{
    Ability, CardDef, Cost, CostPart, Filter, ManaColor, ManaSet, TargetKind, TargetSpec, Types,
    card,
};

/// The second ability is not a mana ability: it has a target
/// ([CR 605.1a](https://yawgatog.com/resources/magic-rules/#R6051a)), so it uses the stack.
/// [CR 701.8a](https://yawgatog.com/resources/magic-rules/#R7018a): "To destroy a permanent,
/// move it from the battlefield to its owner's graveyard."
pub static CARD: CardDef = card(
    names::STRIP_MINE,
    &[printed::STRIP_MINE.abilities(&[
        Ability::tap_for_mana(ManaSet::one(ManaColor::Colorless)),
        Ability::activated(Cost::of_parts(&[CostPart::TAP, CostPart::SACRIFICE]))
            .targeting(&[TargetSpec::one(
                TargetKind::Permanent,
                Filter::Types(Types::LAND),
            )])
            .effect(&destroy(TARGET)),
    ])],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_card::{DecisionKind, Seat, Zone};

    use beryl_scenario_types::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use crate::{names, props};

    const fn count(player: Seat, zone: Zone, card: Option<&'static str>, count: u8) -> Action {
        Action::assert(Assertion::Count {
            player,
            zone,
            card,
            count,
        })
    }

    /// Its {C} pays a creature's generic mana. Strip Mine may target itself, since targets are
    /// chosen before costs are paid (CR 601.2c, 601.2h), so the target is a decision between the
    /// two lands; with a second Strip Mine, it is a decision between three, as targeting itself and
    /// its twin lead to different games.
    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Strip Mine destroys a basic land")
            .permanents(P0, &[Permanent::new(names::STRIP_MINE)])
            .permanents(P1, &[Permanent::new(names::PLAINS)])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::activate(P0, names::STRIP_MINE, 1),
                Action::answer(
                    P0,
                    DecisionKind::Target,
                    Pick::Card {
                        card: names::PLAINS,
                    },
                ),
                count(P0, Zone::Graveyard, Some(names::STRIP_MINE), 1),
                count(P1, Zone::Graveyard, Some(names::PLAINS), 1),
                count(P0, Zone::Battlefield, None, 0),
                count(P1, Zone::Battlefield, None, 0),
            ]),
        Scenario::new("Strip Mine tells itself from its twin")
            .permanents(
                P0,
                &[
                    Permanent::new(names::STRIP_MINE),
                    Permanent::new(names::STRIP_MINE),
                ],
            )
            .permanents(P1, &[Permanent::new(names::PLAINS)])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::activate(P0, names::STRIP_MINE, 1),
                Action::assert(Assertion::Decision {
                    player: P0,
                    kind: DecisionKind::Target,
                    candidates: 3,
                }),
                Action::answer(
                    P0,
                    DecisionKind::Target,
                    Pick::Card {
                        card: names::PLAINS,
                    },
                ),
                Action::Resolve,
                count(P0, Zone::Graveyard, Some(names::STRIP_MINE), 1),
                count(P1, Zone::Graveyard, Some(names::PLAINS), 1),
                count(P0, Zone::Battlefield, Some(names::STRIP_MINE), 1),
            ]),
        // The Island pays the {U}, Strip Mine the {1}. The land still to play keeps the game in
        // P0's main phase.
        Scenario::new("Strip Mine's colorless mana")
            .permanents(
                P0,
                &[
                    Permanent::new(names::STRIP_MINE),
                    Permanent::new(names::ISLAND),
                ],
            )
            .hand(P0, &[props::SMALL_CREATURE, names::PLAINS])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, props::SMALL_CREATURE),
                Action::assert(Assertion::Tapped {
                    player: P0,
                    card: names::STRIP_MINE,
                    count: 1,
                }),
                Action::Resolve,
                Action::assert(Assertion::Controls {
                    player: P0,
                    card: props::SMALL_CREATURE,
                    count: 1,
                }),
            ]),
    ];
}
