// Generated from the card store by `just card header`: notes go below the end marker.
//! Sink into Stupor {1}{U}{U}
//! Instant
//!
//! Return target spell or nonland permanent an opponent controls to its owner's hand.
//!
//! Soporific Springs
//! Land
//!
//! As this land enters, you may pay 3 life. If you don't, it enters tapped.
//! {T}: Add {U}.
// End of the generated card header.

use crate::prelude::*;

/// [CR 712.8b](https://yawgatog.com/resources/magic-rules/#R7128b): "A player casting a modal
/// double-faced card ... chooses which face they are casting"; only the instant can be, and
/// [CR 712.9](https://yawgatog.com/resources/magic-rules/#R7129): "A player playing a modal
/// double-faced card ... as a land chooses one of its faces that's a land ... It enters the
/// battlefield with that face up."
///
/// A spell returned to its owner's hand leaves the stack without resolving; a permanent
/// leaves the battlefield.
pub static CARD: CardDef = card(
    Card::SinkIntoStupor,
    &[
        Printed::SinkIntoStupor.abilities(&[Ability::spell()
            .target(&TargetSpec::one(
                TargetKind::SpellOrPermanent,
                Filter::All(&[
                    Filter::Whose(&Players::EachOpponent),
                    Filter::Not(&Filter::Types(Types::LAND)),
                ]),
            ))
            .effect(&return_to_hand(target()))]),
        Printed::SoporificSprings.abilities(&[
            Ability::replacing(Replacement::EntersTappedUnlessPayLife(3)),
            Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)),
        ]),
    ],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const SPRINGS: &str = "Soporific Springs";

    /// The only legal target is chosen silently: lands are not "nonland", and P0's own objects
    /// are not an opponent's.
    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Sink into Stupor returns a spell to its owner's hand")
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[Card::Island; 3])
            .hand(P0, &[Card::SinkIntoStupor])
            .battlefield(P1, &[Card::Island; 3])
            .hand(P1, &[Card::Timetwister])
            .library(P0, &[Card::Plains])
            .actions(&[
                THEM.casts(Card::Timetwister),
                ME.casts(Card::SinkIntoStupor),
                THEM.has(Zone::Hand, Card::Timetwister, 1),
                THEM.has(Zone::Graveyard, Card::Timetwister, 0),
                ME.has(Zone::Graveyard, Card::SinkIntoStupor, 1),
            ]),
        Scenario::new("Sink into Stupor returns a nonland permanent, never a land")
            .battlefield(P0, &[Card::Island; 3])
            .hand(P0, &[Card::SinkIntoStupor])
            .battlefield(P1, &[Prop::Artifact, Card::Island])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.casts(Card::SinkIntoStupor),
                THEM.has(Zone::Hand, Prop::Artifact, 1),
                THEM.has(Zone::Battlefield, Card::Island, 1),
            ]),
        // Played as Soporific Springs paying 3 life, it taps for the {U} of the other copy cast
        // as Sink into Stupor.
        Scenario::new("Soporific Springs: 3 life paid, it enters untapped")
            .battlefield(P0, &[Card::Island, Card::Island])
            .hand(P0, &[Card::SinkIntoStupor, Card::SinkIntoStupor])
            .battlefield(P1, &[Prop::Artifact])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.plays(Card::SinkIntoStupor),
                ME.yes(),
                ME.life(17),
                ME.has(Zone::Battlefield, SPRINGS, 1),
                ME.tapped(SPRINGS, 0),
                ME.casts(Card::SinkIntoStupor),
                ME.tapped(SPRINGS, 1),
                THEM.has(Zone::Hand, Prop::Artifact, 1),
            ]),
        Scenario::new("Soporific Springs: no life paid, it enters tapped")
            .hand(P0, &[Card::SinkIntoStupor])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.plays(Card::SinkIntoStupor),
                ME.no(),
                ME.life(20),
                ME.tapped(SPRINGS, 1),
            ]),
        // CR 119.4: a player "may [pay life] only if their life total is greater than or equal to
        // the amount of the payment."
        Scenario::new("Soporific Springs: with 2 life, it enters tapped unasked")
            .life(P0, 2)
            .hand(P0, &[Card::SinkIntoStupor])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.plays(Card::SinkIntoStupor),
                ME.life(2),
                ME.tapped(SPRINGS, 1),
            ]),
    ];
}
