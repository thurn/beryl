// Generated from the card store by `just card header`: notes go below the end marker.
//! Strip Mine
//! Land
//!
//! {T}: Add {C}.
//! {T}, Sacrifice this land: Destroy target land.
// End of the generated card header.

use crate::prelude::*;

/// Strip Mine can target itself because its controller chooses a target before sacrificing
/// it to pay the activation cost ([CR 602.2b]): "The remainder of the process for activating
/// an ability is identical to the process for casting a spell listed in rules 601.2b–i."
///
/// [CR 602.2b]: https://yawgatog.com/resources/magic-rules/#R6022b
pub static CARD: CardDef = Printed::StripMine
    .abilities(&[
        Ability::tap_for_mana(ManaSet::one(ManaColor::Colorless)),
        Ability::activated(tap_and_sacrifice())
            .target(&TargetSpec::permanent(land()))
            .effect(&destroy(target())),
    ])
    .card();
