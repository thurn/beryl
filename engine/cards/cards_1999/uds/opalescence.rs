// Generated from the card store by `just card header`: notes go below the end marker.
//! Opalescence {2}{W}{W}
//! Enchantment
//!
//! Each other non-Aura enchantment is a creature in addition to its other types and has base power
//! and base toughness each equal to its mana value.
// End of the generated card header.

use crate::prelude::*;

/// Opalescence begins applying in layer 4, so it still sets the animated enchantments'
/// base power and toughness after Humility removes its ability ([CR 613.6]): "it will
/// continue to be applied to the same set of objects in each other applicable layer and/or
/// sublayer, even if the ability generating the effect is removed during this process."
/// The later timestamp determines which sets power and toughness last in layer 7b
/// ([CR 613.7]): "An effect with an earlier timestamp is applied before an effect with a
/// later timestamp."
///
/// [CR 613.6]: https://yawgatog.com/resources/magic-rules/#R6136
/// [CR 613.7]: https://yawgatog.com/resources/magic-rules/#R6137
pub static CARD: CardDef = Printed::Opalescence
    .abilities(&[Ability::static_ability(&LayerSpec {
        affected: Objects::Other(&Objects::All {
            zone: Zone::Battlefield,
            filter: Filter::All(&[
                Filter::Types(Types::ENCHANTMENT),
                Filter::Not(&Filter::Subtype(Subtype::Aura)),
            ]),
        }),
        condition: None,
        modifications: &[
            Modification::AddTypes(Types::CREATURE),
            Modification::SetBasePt(
                &Amount::ManaValue(Objects::Those),
                &Amount::ManaValue(Objects::Those),
            ),
        ],
    })])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const OPALESCENCE: &str = Card::Opalescence;

    // `Prop::Vanilla` is the creature for Swift Reconfiguration to enchant.

    /// Four Plains, after `first`, for one four-mana spell.
    const fn with_plains(first: [&'static str; 2]) -> [&'static str; 6] {
        [
            first[0],
            first[1],
            Card::Plains,
            Card::Plains,
            Card::Plains,
            Card::Plains,
        ]
    }

    pub const SCENARIOS: &[Scenario] = &[
        // Glorious Anthem, mana value 3, is a 3/3 creature that its own +1/+1 makes 4/4.
        // Opalescence is no creature.
        Scenario::new("another enchantment is a creature as big as its mana value")
            .interaction(&[Card::GloriousAnthem])
            .battlefield(P0, &[OPALESCENCE, Card::GloriousAnthem])
            .actions(&[
                ME.creature(Card::GloriousAnthem, true),
                ME.pt(Card::GloriousAnthem, 4, 4),
                ME.creature(OPALESCENCE, false),
            ]),
        // Swift Reconfiguration, an Aura, stays no creature once it enchants the creature.
        Scenario::new("an Aura is not made a creature")
            .interaction(&[Card::SwiftReconfiguration])
            .battlefield(P0, &[OPALESCENCE, Card::Plains, Prop::Vanilla])
            .hand(P0, &[Card::SwiftReconfiguration])
            .actions(&[
                ME.casts(Card::SwiftReconfiguration),
                Resolve,
                ME.controls(Card::SwiftReconfiguration, 1),
                ME.creature(Card::SwiftReconfiguration, false),
            ]),
        // As creatures, Humility and Dress Down each remove the other's ability in layer 6:
        // a loop of dependencies, applied in timestamp order (CR 613.8b). Humility, the
        // earlier, applies and takes Dress Down's ability, enters trigger included, so its
        // 1/1 makes both 1/1.
        Scenario::new("Humility before Dress Down: Dress Down's ability is lost")
            .interaction(&[Card::Humility, Card::DressDown])
            .battlefield(
                P0,
                &[OPALESCENCE, Card::Humility, Card::Island, Card::Island],
            )
            .hand(P0, &[Card::DressDown])
            .library(P0, &[Card::Plains, Card::Island])
            .actions(&[
                ME.casts(Card::DressDown),
                Resolve,
                stack(0),
                ME.has(Zone::Hand, Card::Plains, 0),
                ME.pt(Card::Humility, 1, 1),
                ME.pt(Card::DressDown, 1, 1),
            ]),
        // Dress Down, the earlier, applies and takes Humility's ability before it begins, so
        // no 1/1 applies: each is as big as its mana value.
        Scenario::new("Dress Down before Humility: Humility's ability is lost")
            .interaction(&[Card::Humility, Card::DressDown])
            .battlefield(P0, &with_plains([OPALESCENCE, Card::DressDown]))
            .hand(P0, &[Card::Humility])
            .actions(&[
                ME.casts(Card::Humility),
                Resolve,
                ME.pt(Card::Humility, 4, 4),
                ME.pt(Card::DressDown, 2, 2),
            ]),
        // Each Opalescence makes the other a 4/4 creature.
        Scenario::new("two Opalescences make each other creatures")
            .battlefield(P0, &[OPALESCENCE])
            .battlefield(P1, &[OPALESCENCE])
            .actions(&[ME.pt(OPALESCENCE, 4, 4), THEM.pt(OPALESCENCE, 4, 4)]),
        // Summoning sickness applies to a permanent that became a creature the turn it came
        // under its controller's control (CR 302.6): Glorious Anthem, cast this turn, can't
        // attack; the Flier can.
        Scenario::new("an enchantment cast this turn can't attack")
            .interaction(&[Card::GloriousAnthem])
            .battlefield(
                P0,
                &[
                    OPALESCENCE,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Prop::Flier,
                ],
            )
            .hand(P0, &[Card::GloriousAnthem])
            .actions(&[
                ME.casts(Card::GloriousAnthem),
                Resolve,
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.deciding(DecisionKind::Attacker, 2),
            ]),
        // An Anthem controlled since the turn began can attack on the turn it becomes
        // a creature. Only how long it has been under its controller's control matters.
        Scenario::new("an older enchantment can attack on the turn it becomes a creature")
            .interaction(&[Card::GloriousAnthem])
            .hand(P0, &[OPALESCENCE])
            .battlefield(
                P0,
                &[
                    Card::GloriousAnthem,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                ],
            )
            .actions(&[
                ME.casts(OPALESCENCE),
                Resolve,
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(Card::GloriousAnthem),
                advance_to(1, TurnStep::EndOfCombat),
                THEM.life(16),
            ]),
        // Without Opalescence on the battlefield an enchantment is no creature.
        Scenario::new("an enchantment is no creature without it")
            .interaction(&[Card::GloriousAnthem])
            .battlefield(P0, &[Card::GloriousAnthem])
            .actions(&[
                ME.creature(Card::GloriousAnthem, false),
                advance_to(2, TurnStep::Upkeep),
            ]),
    ];
}
