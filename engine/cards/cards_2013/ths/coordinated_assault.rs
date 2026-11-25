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
