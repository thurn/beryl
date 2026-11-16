// Generated from the card store by `just card header`: notes go below the end marker.
//! Double Major {G}{U}
//! Instant
//!
//! Copy target creature spell you control, except it isn't legendary if the spell is legendary. (A
//! copy of a creature spell becomes a token.)
// End of the generated card header.

use crate::prelude::*;

/// "Copy target creature spell you control, except it isn't legendary if the spell is
/// legendary." The copy has the spell's targets and costs paid and is not cast ([CR 707.10]):
/// "a copy of a spell isn't cast ... A copy of a spell or ability copies both the
/// characteristics of the spell or ability and all decisions made for it". It is on the stack
/// above the spell, so it resolves first. Removing legendary is part of its copiable values
/// ([CR 707.9b]): "Some copy effects modify a characteristic as part of the copying process."
/// A copy of a permanent spell becomes a token as it resolves ([CR 608.3f]).
///
/// [CR 707.10]: https://yawgatog.com/resources/magic-rules/#R70710
/// [CR 707.9b]: https://yawgatog.com/resources/magic-rules/#R7079b
/// [CR 608.3f]: https://yawgatog.com/resources/magic-rules/#R6083f
pub static CARD: CardDef = Printed::DoubleMajor
    .abilities(&[Ability::spell()
        .target(&TargetSpec::one(
            TargetKind::Spell,
            Filter::All(&[Filter::Types(Types::CREATURE), Filter::Whose(&Players::You)]),
        ))
        .effect(&copy_spell_except_keeping_targets(
            target(),
            ExceptIx::new(0),
        ))])
    .excepts(&[CopyException::NOT_LEGENDARY])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const DOUBLE_MAJOR: &str = Card::DoubleMajor;
    const CAELORNA: &str = Card::CaelornaCoralTyrant;

    pub const SCENARIOS: &[Scenario] = &[
        // The copy resolves first, as a token with the spell's power and toughness; the
        // original then resolves as a card.
        Scenario::new("Double Major's copy of a creature spell becomes a token")
            .battlefield(
                P0,
                &[
                    Card::Forest,
                    Card::Forest,
                    Card::Forest,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .hand(P0, &[Prop::Vanilla, DOUBLE_MAJOR])
            .actions(&[
                ME.casts(Prop::Vanilla),
                ME.casts(DOUBLE_MAJOR),
                Resolve,
                Resolve,
                ME.token_pt(Prop::Vanilla, 2, 1),
                ME.controls(Prop::Vanilla, 1),
                Resolve,
                ME.pt(Prop::Vanilla, 2, 1),
                ME.controls(Prop::Vanilla, 2),
            ]),
        // The copy of a legendary creature spell isn't legendary, so the legend rule keeps the
        // token and the card. Reki sees only the one cast: the copy is not cast. Reki is cast
        // first, so it is not on the battlefield when the scenario starts.
        Scenario::new("Double Major's copy of a legendary creature spell is not legendary")
            .interaction(&[Card::RekiTheHistoryOfKamigawa])
            .battlefield(
                P0,
                &[
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Forest,
                    Card::Forest,
                    Card::Forest,
                    Card::Forest,
                    Card::Forest,
                    Card::Forest,
                ],
            )
            .hand(
                P0,
                &[Card::RekiTheHistoryOfKamigawa, CAELORNA, DOUBLE_MAJOR],
            )
            .library(P0, &[Card::Plains; 3])
            .actions(&[
                ME.casts(Card::RekiTheHistoryOfKamigawa),
                Resolve,
                ME.casts(CAELORNA),
                ME.casts(DOUBLE_MAJOR),
                Resolve,
                Resolve,
                ME.token_pt(CAELORNA, 0, 8),
                ME.controls(CAELORNA, 1),
                Resolve,
                ME.has(Zone::Hand, Card::Plains, 1),
                Resolve,
                ME.controls(CAELORNA, 2),
                ME.has(Zone::Hand, Card::Plains, 1),
            ]),
    ];
}
