// Generated from the card store by `just card header`: notes go below the end marker.
//! Opalescence {2}{W}{W}
//! Enchantment
//!
//! Each other non-Aura enchantment is a creature in addition to its other types and has base power
//! and base toughness each equal to its mana value.
// End of the generated card header.

use crate::prelude::*;

/// "Each other non-Aura enchantment is a creature in addition to its other types and has
/// base power and base toughness each equal to its mana value": one static ability whose
/// effect has a part in layer 4 and a part in layer 7b ([CR 613.6]): "If an effect should be
/// applied in different layers and/or sublayers, the parts of the effect each apply in their
/// appropriate ones. If an effect starts to apply in one layer and/or sublayer, it will
/// continue to be applied to the same set of objects in each other applicable layer and/or
/// sublayer, even if the ability generating the effect is removed during this process." So
/// an enchantment it makes a creature still gets its base power and toughness after Humility
/// has taken this ability away in layer 6, and with Humility the later timestamp decides the
/// size in layer 7b ([CR 613.7]): "An effect with an earlier timestamp is applied before an
/// effect with a later timestamp."
///
/// The base power and toughness are read for each enchantment it affects, as that layer
/// applies ([CR 613.4b]): "Effects that set power and/or toughness to a specific number or
/// value are applied."
///
/// [CR 613.6]: https://yawgatog.com/resources/magic-rules/#R6136
/// [CR 613.7]: https://yawgatog.com/resources/magic-rules/#R6137
/// [CR 613.4b]: https://yawgatog.com/resources/magic-rules/#R6134b
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

    // `Prop::SmallCreature` is Snapcaster Mage, a 2/1 for Swift Reconfiguration to enchant.

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

    const EIGHT_PLAINS: [&str; 8] = [Card::Plains; 8];

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
        // Swift Reconfiguration, an Aura, stays no creature once it enchants the Snapcaster.
        Scenario::new("an Aura is not made a creature")
            .interaction(&[Card::SwiftReconfiguration])
            .battlefield(P0, &[OPALESCENCE, Card::Plains, Prop::SmallCreature])
            .hand(P0, &[Card::SwiftReconfiguration])
            .actions(&[
                ME.casts(Card::SwiftReconfiguration),
                Resolve,
                ME.controls(Card::SwiftReconfiguration, 1),
                ME.creature(Card::SwiftReconfiguration, false),
            ]),
        // Humility first: it becomes a creature and loses its ability in layer 6, but its 1/1
        // applies in layer 7b before Opalescence's later one. Humility is 4/4, and Glorious
        // Anthem, its ability gone, is 3/3.
        Scenario::new("with Humility earlier, each is as big as its mana value")
            .interaction(&[Card::Humility, Card::GloriousAnthem])
            .battlefield(P0, &with_plains([Card::Humility, Card::GloriousAnthem]))
            .hand(P0, &[OPALESCENCE])
            .actions(&[
                ME.casts(OPALESCENCE),
                Resolve,
                ME.pt(Card::Humility, 4, 4),
                ME.pt(Card::GloriousAnthem, 3, 3),
                ME.creature(OPALESCENCE, false),
            ]),
        // Opalescence first: Humility's 1/1 comes later in layer 7b, so every enchantment
        // creature is 1/1, Humility with them.
        Scenario::new("with Humility later, each is 1/1")
            .interaction(&[Card::Humility, Card::GloriousAnthem])
            .battlefield(P0, &with_plains([OPALESCENCE, Card::GloriousAnthem]))
            .hand(P0, &[Card::Humility])
            .actions(&[
                ME.pt(Card::GloriousAnthem, 4, 4),
                ME.casts(Card::Humility),
                Resolve,
                ME.pt(Card::GloriousAnthem, 1, 1),
                ME.pt(Card::Humility, 1, 1),
                ME.creature(OPALESCENCE, false),
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
        // In timestamp order Opalescence, Humility, Opalescence, layer 7b applies the first
        // Opalescence's 4/4, Humility's 1/1, then the second Opalescence's 4/4: the second
        // Opalescence, which only the first sets, is 1/1, and the other two are 4/4.
        Scenario::new("between two Opalescences, Humility is 4/4 and the last is 1/1")
            .interaction(&[Card::Humility])
            .battlefield(P1, &[OPALESCENCE])
            .battlefield(P0, &EIGHT_PLAINS)
            .hand(P0, &[Card::Humility, OPALESCENCE])
            .actions(&[
                ME.casts(Card::Humility),
                Resolve,
                ME.casts(OPALESCENCE),
                Resolve,
                THEM.pt(OPALESCENCE, 4, 4),
                ME.pt(Card::Humility, 4, 4),
                ME.pt(OPALESCENCE, 1, 1),
            ]),
    ];
}
