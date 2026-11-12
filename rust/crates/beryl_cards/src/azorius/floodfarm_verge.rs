//! Floodfarm Verge. Land. "{T}: Add {W}. {T}: Add {U}. Activate only if you control a Plains
//! or an Island."

use beryl_engine::card::{Ability, CardDef, Face, ManaColor, ManaSet, Subtype, Types};
use beryl_engine::filter::Filter;

use crate::names;

/// The {U} ability's restriction reads the land's subtypes, so a Tundra, a Plains Island,
/// counts, as a basic does
/// ([CR 305.6](https://yawgatog.com/resources/magic-rules/#R3056)): "The basic land types are
/// Plains, Island, Swamp, Mountain, and Forest."
pub static CARD: CardDef = CardDef {
    name: names::FLOODFARM_VERGE,
    faces: &[Face {
        types: Types::LAND,
        abilities: &[
            Ability::tap_for_mana(ManaSet::one(ManaColor::White)),
            Ability {
                only_if_you_control: Some(Filter::AnyOf(&[
                    Filter::Subtype(Subtype::Plains),
                    Filter::Subtype(Subtype::Island),
                ])),
                ..Ability::tap_for_mana(ManaSet::one(ManaColor::Blue))
            },
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

#[cfg(feature = "scenarios")]
pub mod scenarios {

    use beryl_engine::{TurnStep, Zone};
    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

    use crate::names;

    const VERGE: &str = names::FLOODFARM_VERGE;

    /// P1 casts Mana Crypt in their main phase, with a land still to play.
    const fn crypt_cast() -> Action {
        Action::cast(P1, names::MANA_CRYPT)
    }

    const fn verge_tapped(count: u8) -> Action {
        Action::assert(Assertion::Tapped {
            player: P0,
            card: VERGE,
            count,
        })
    }

    pub const SCENARIOS: &[Scenario] = &[
        // Tundra is a Plains and an Island: its {U} and the Verge's pay for Counterspell.
        Scenario::new("Floodfarm Verge's {U} with a Tundra")
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[Permanent::new(VERGE), Permanent::new(names::TUNDRA)])
            .hand(P0, &[names::COUNTERSPELL])
            .library(P0, &[names::ISLAND])
            .hand(P1, &[names::MANA_CRYPT, names::PLAINS])
            .actions(&[
                crypt_cast(),
                Action::cast(P0, names::COUNTERSPELL),
                verge_tapped(1),
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Graveyard,
                    card: Some(names::MANA_CRYPT),
                    count: 1,
                }),
            ]),
        // Seachrome Coast is neither: the Verge makes only {W}, so P0 can't cast Counterspell
        // and has nothing to do, and the Crypt resolves.
        Scenario::new("Floodfarm Verge's {U} without a Plains or an Island")
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(
                P0,
                &[
                    Permanent::new(VERGE),
                    Permanent::new(names::SEACHROME_COAST),
                ],
            )
            .hand(P0, &[names::COUNTERSPELL])
            .library(P0, &[names::ISLAND])
            .hand(P1, &[names::MANA_CRYPT, names::PLAINS])
            .actions(&[
                crypt_cast(),
                Action::assert(Assertion::Controls {
                    player: P1,
                    card: names::MANA_CRYPT,
                    count: 1,
                }),
                verge_tapped(0),
            ]),
        // A basic Plains enables the {U}: with the Plains's {W} for the generic mana, Jace.
        Scenario::new("Floodfarm Verge's {U} with a Plains")
            .battlefield(P0, &[Permanent::new(VERGE), Permanent::new(names::PLAINS)])
            .hand(P0, &[names::JACE_VRYNS_PRODIGY])
            .library(P0, &[names::ISLAND])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, names::JACE_VRYNS_PRODIGY),
                Action::Resolve,
                Action::assert(Assertion::Controls {
                    player: P0,
                    card: names::JACE_VRYNS_PRODIGY,
                    count: 1,
                }),
            ]),
        // Alone, its {W} pays for Mana Tithe against the Crypt.
        Scenario::new("Floodfarm Verge's {W}")
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[Permanent::new(VERGE)])
            .hand(P0, &[names::MANA_TITHE])
            .library(P0, &[names::ISLAND])
            .hand(P1, &[names::MANA_CRYPT, names::PLAINS])
            .actions(&[
                crypt_cast(),
                Action::cast(P0, names::MANA_TITHE),
                verge_tapped(1),
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Graveyard,
                    card: Some(names::MANA_CRYPT),
                    count: 1,
                }),
            ]),
    ];
}
