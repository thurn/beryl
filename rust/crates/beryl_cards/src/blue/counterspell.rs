//! Counterspell. {U}{U} Instant. "Counter target spell."

use beryl_card::effect::build::{TARGET, counter};
use beryl_engine::card::{
    Ability, AbilityKind, Behavior, CardDef, Colors, Face, ManaCost, TargetKind, TargetSpec, Types,
};
use beryl_engine::filter::Filter;

use crate::names;

pub static CARD: CardDef = CardDef {
    name: names::COUNTERSPELL,
    faces: &[Face {
        cost: ManaCost::new(0, [0, 2, 0, 0, 0, 0]),
        types: Types::INSTANT,
        colors: Colors::BLUE,
        abilities: &[Ability {
            kind: AbilityKind::Spell,
            targets: &[TargetSpec::one(TargetKind::Spell, Filter::Any)],
            // CR 701.6a: "To counter a spell or ability means to cancel it, removing it from
            // the stack. It doesn't resolve and none of its effects occur. A countered spell
            // is put into its owner's graveyard."
            body: Behavior::Data(&counter(TARGET)),
            ..Ability::NONE
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};

#[cfg(feature = "scenarios")]
pub mod scenarios {

    use beryl_engine::{DecisionKind, Seat, TurnStep, Zone};
    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use crate::names;

    const ISLANDS: &[Permanent] = &[Permanent::new(names::ISLAND), Permanent::new(names::ISLAND)];

    const fn count(player: Seat, zone: Zone, card: &'static str, count: u8) -> Action {
        Action::assert(Assertion::Count {
            player,
            zone,
            card: Some(card),
            count,
        })
    }

    pub const SCENARIOS: &[Scenario] = &[
        // Mana Crypt goes to its owner's graveyard, not exile, and never reaches the battlefield.
        Scenario::new("Counterspell counters a spell into its owner's graveyard")
            .battlefield(P0, ISLANDS)
            .hand(P0, &[names::COUNTERSPELL])
            .battlefield(P1, &[Permanent::new(names::PLAINS)])
            .hand(P1, &[names::MANA_CRYPT, names::PLAINS])
            .library(P0, &[names::ISLAND])
            .at(2, P1, TurnStep::PrecombatMain)
            .actions(&[
                Action::cast(P1, names::MANA_CRYPT),
                Action::cast(P0, names::COUNTERSPELL),
                count(P1, Zone::Graveyard, names::MANA_CRYPT, 1),
                count(P1, Zone::Battlefield, names::MANA_CRYPT, 0),
                count(P0, Zone::Graveyard, names::COUNTERSPELL, 1),
            ]),
        // Two spells on the stack: Counterspell targets the one beneath, the Mana Tithe that
        // targets it, so the Crypt resolves.
        Scenario::new("Counterspell chooses among spells on the stack")
            .battlefield(P0, ISLANDS)
            .hand(P0, &[names::MANA_CRYPT, names::COUNTERSPELL, names::PLAINS])
            .battlefield(P1, &[Permanent::new(names::PLAINS)])
            .hand(P1, &[names::MANA_TITHE])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, names::MANA_CRYPT),
                Action::pass(P0),
                Action::cast(P1, names::MANA_TITHE),
                Action::cast(P0, names::COUNTERSPELL),
                Action::answer(
                    P0,
                    DecisionKind::Target,
                    Pick::Card {
                        card: names::MANA_TITHE,
                    },
                ),
                Action::Resolve,
                count(P1, Zone::Graveyard, names::MANA_TITHE, 1),
                Action::Resolve,
                count(P0, Zone::Battlefield, names::MANA_CRYPT, 1),
            ]),
    ];
}
