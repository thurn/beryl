// Generated from the card store by `just card header`: notes go below the end marker.
//! Coordinated Assault {R}
//! Instant
//!
//! Up to two target creatures each get +1/+0 and gain first strike until end of turn. (They deal
//! combat damage before creatures without first strike.)
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::CoordinatedAssault
    .abilities(&[Ability::spell()
        .target(&TargetSpec::up_to(2, TargetKind::Permanent, creature()))
        .effect(&modify(
            target(),
            &[
                Modification::ModifyPt(1, 0),
                Modification::AddKeywords(Keywords::FIRST_STRIKE),
            ],
            Duration::EndOfTurn,
        ))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const ASSAULT: &str = Card::CoordinatedAssault;

    pub const SCENARIOS: &[Scenario] = &[
        // An until-end-of-turn effect ends with the turn, so the
        // creature is its printed 2/1 again in the next turn.
        Scenario::new("Coordinated Assault's bonus ends with the turn")
            .interaction(&[Card::Mountain])
            .at(1, P0, TurnStep::PrecombatMain)
            .hand(P0, &[ASSAULT])
            .battlefield(P0, &[Card::Mountain, Prop::Vanilla])
            .library(P0, &[Card::Plains, Card::Plains])
            .library(P1, &[Card::Plains, Card::Plains])
            .actions(&[
                ME.casts(ASSAULT),
                ME.target_count(1),
                Resolve,
                ME.pt(Prop::Vanilla, 3, 1),
                ME.passes(),
                ME.done_attacking(),
                advance_to(2, TurnStep::PrecombatMain),
                ME.pt(Prop::Vanilla, 2, 1),
            ]),
    ];
}
