// Generated from the card store by `just card header`: notes go below the end marker.
//! Twisted Image {U}
//! Instant
//!
//! Switch target creature's power and toughness until end of turn.
//! Draw a card.
// End of the generated card header.

use crate::prelude::*;

/// The switch applies after other power and toughness modifications, including +1/+1
/// counters ([CR 613.4c], [CR 613.4d]).
///
/// [CR 613.4c]: https://yawgatog.com/resources/magic-rules/#R6134c
/// [CR 613.4d]: https://yawgatog.com/resources/magic-rules/#R6134d
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

    pub const SCENARIOS: &[Scenario] = &[
        // The Wandering Emperor's +1 puts a +1/+1 counter on the flier: 4/2 from layer
        // 7c, switched afterwards in layer 7d.
        Scenario::new("a creature with a +1/+1 counter is switched after the counter")
            .interaction(&[Card::TheWanderingEmperor])
            .battlefield(P0, &[Card::Island, Card::TheWanderingEmperor, Prop::Flier])
            .hand(P0, &[IMAGE])
            .library(P0, &[Card::Plains, Card::Island])
            .actions(&[
                ME.activates(Card::TheWanderingEmperor, 1),
                ME.target_count(1),
                Resolve,
                ME.counters(Prop::Flier, CounterKind::PlusOne, 1),
                ME.pt(Prop::Flier, 4, 2),
                ME.casts(IMAGE),
                Resolve,
                ME.pt(Prop::Flier, 2, 4),
            ]),
        // An until-end-of-turn effect ends with the turn, so the
        // creature is its printed 2/1 again in the next turn.
        Scenario::new("Twisted Image's switch ends with the turn")
            .interaction(&[Card::TwistedImage, Card::Island])
            .at(1, P0, TurnStep::PrecombatMain)
            .hand(P0, &[Card::TwistedImage])
            .battlefield(P0, &[Card::Island, Prop::Vanilla])
            .library(P0, &[Card::Plains, Card::Plains])
            .library(P1, &[Card::Plains, Card::Plains])
            .actions(&[
                ME.casts(Card::TwistedImage),
                Resolve,
                ME.pt(Prop::Vanilla, 1, 2),
                ME.passes(),
                ME.done_attacking(),
                advance_to(2, TurnStep::PrecombatMain),
                ME.pt(Prop::Vanilla, 2, 1),
            ]),
    ];
}
