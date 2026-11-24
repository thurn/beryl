// Generated from the card store by `just card header`: notes go below the end marker.
//! Leonin Scimitar {1}
//! Artifact — Equipment
//!
//! Equipped creature gets +1/+1.
//! Equip {1} ({1}: Attach to target creature you control. Equip only as a sorcery.)
// End of the generated card header.

use crate::prelude::*;

/// Implemented so scenarios have an Equipment to attach.
pub static CARD: CardDef = Printed::LeoninScimitar
    .abilities(&[
        Ability::static_ability(&LayerSpec {
            affected: Objects::AttachedTo,
            condition: None,
            modifications: &[Modification::ModifyPt(1, 1)],
        }),
        Ability::equip(mana("{1}")),
    ])
    .card();
