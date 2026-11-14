// Generated from the card store by `just card header`: notes go below the end marker.
//! Twisted Image {U}
//! Instant
//!
//! Switch target creature's power and toughness until end of turn.
//! Draw a card.
// End of the generated card header.

use crate::prelude::*;

/// "Switch target creature's power and toughness until end of turn" is a layer 7d effect
/// ([CR 613.4d]): "Effects that switch a creature's power and toughness are applied." It applies after the modifying effects and counters of layer 7c
/// whenever they began, so a creature with a +1/+1 counter is switched after the counter.
/// It lasts until the turn ends ([CR 514.2]). Then its controller draws a card.
///
/// [CR 613.4d]: https://yawgatog.com/resources/magic-rules/#R6134d
/// [CR 514.2]: https://yawgatog.com/resources/magic-rules/#R5142
pub static CARD: CardDef = Printed::TwistedImage
    .abilities(&[Ability::spell()
        .target(&TargetSpec::creature())
        .effect(&sequence(&[
            modify(target(), &[Modification::SwitchPt], Duration::EndOfTurn),
            draw(you(), 1),
        ]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const IMAGE: &str = Card::TwistedImage;

    // `Prop::Creature` is Elite Spellbinder, a 3/1 flier.
    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("switching a 3/1 makes it a 1/3 and draws a card")
            .battlefield(P0, &[Card::Island, Prop::Creature])
            .hand(P0, &[IMAGE])
            .library(P0, &[Card::Plains, Card::Island])
            .actions(&[
                ME.casts(IMAGE),
                Resolve,
                ME.pt(Prop::Creature, 1, 3),
                ME.has(Zone::Hand, Card::Plains, 1),
                ME.has(Zone::Graveyard, IMAGE, 1),
            ]),
        // Switching twice puts the creature back to 3/1.
        Scenario::new("switching twice restores the original values")
            .battlefield(P0, &[Card::Island, Card::Island, Prop::Creature])
            .hand(P0, &[IMAGE, IMAGE])
            .library(P0, &[Card::Plains, Card::Island, Card::Island])
            .actions(&[
                ME.casts(IMAGE),
                Resolve,
                ME.pt(Prop::Creature, 1, 3),
                ME.casts(IMAGE),
                Resolve,
                ME.pt(Prop::Creature, 3, 1),
            ]),
        // The Wandering Emperor's +1 puts a +1/+1 counter on the Spellbinder: 4/2 from layer
        // 7c, switched afterwards in layer 7d.
        Scenario::new("a creature with a +1/+1 counter is switched after the counter")
            .interaction(&[Card::TheWanderingEmperor])
            .battlefield(
                P0,
                &[Card::Island, Card::TheWanderingEmperor, Prop::Creature],
            )
            .hand(P0, &[IMAGE])
            .library(P0, &[Card::Plains, Card::Island])
            .actions(&[
                ME.activates(Card::TheWanderingEmperor, 1),
                ME.target_count(1),
                Resolve,
                ME.counters(Prop::Creature, CounterKind::PlusOne, 1),
                ME.pt(Prop::Creature, 4, 2),
                ME.casts(IMAGE),
                Resolve,
                ME.pt(Prop::Creature, 2, 4),
            ]),
    ];
}
