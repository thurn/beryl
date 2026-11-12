//! Counterspell. {U}{U} Instant. "Counter target spell."

use crate::names;
use beryl_card::effect::build::{TARGET, counter};
use beryl_card::{
    Ability, CardDef, Colors, Face, Filter, TargetKind, TargetSpec, Types, card, mana,
};

pub static CARD: CardDef = card(
    names::COUNTERSPELL,
    &[Face::EMPTY
        .cost(mana("{U}{U}"))
        .types(Types::INSTANT)
        .colors(Colors::BLUE)
        .abilities(&[Ability::spell()
            .targeting(&[TargetSpec::one(TargetKind::Spell, Filter::Any)])
            .effect(&counter(TARGET))])],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_engine::{DecisionKind, Seat, TurnStep, Zone};
    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use crate::{names, props};

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
        // The artifact goes to its owner's graveyard, not exile, and never reaches the
        // battlefield.
        Scenario::new("Counterspell counters a spell into its owner's graveyard")
            .battlefield(P0, ISLANDS)
            .hand(P0, &[names::COUNTERSPELL])
            .battlefield(P1, &[Permanent::new(names::PLAINS)])
            .hand(P1, &[props::ARTIFACT, names::PLAINS])
            .library(P0, &[names::ISLAND])
            .at(2, P1, TurnStep::PrecombatMain)
            .actions(&[
                Action::cast(P1, props::ARTIFACT),
                Action::cast(P0, names::COUNTERSPELL),
                count(P1, Zone::Graveyard, props::ARTIFACT, 1),
                count(P1, Zone::Battlefield, props::ARTIFACT, 0),
                count(P0, Zone::Graveyard, names::COUNTERSPELL, 1),
            ]),
        // Two spells on the stack: P0's Counterspell targets the one beneath, P1's Counterspell
        // that targets the artifact, so the artifact resolves. A Plains pays for the artifact,
        // the Islands for Counterspell.
        Scenario::new("Counterspell chooses among spells on the stack")
            .battlefield(
                P0,
                &[
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::PLAINS),
                ],
            )
            .hand(P0, &[props::ARTIFACT, names::COUNTERSPELL, names::PLAINS])
            .battlefield(P1, ISLANDS)
            .hand(P1, &[names::COUNTERSPELL])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, props::ARTIFACT),
                Action::pass(P0),
                Action::cast(P1, names::COUNTERSPELL),
                Action::cast(P0, names::COUNTERSPELL),
                Action::answer(
                    P0,
                    DecisionKind::Target,
                    Pick::Card {
                        card: names::COUNTERSPELL,
                    },
                ),
                Action::Resolve,
                count(P1, Zone::Graveyard, names::COUNTERSPELL, 1),
                Action::Resolve,
                count(P0, Zone::Battlefield, props::ARTIFACT, 1),
            ]),
    ];
}
