// Generated from the card store by `just card header`: notes go below the end marker.
//! Gold Myr {2}
//! Artifact Creature — Myr, 1/1
//!
//! {T}: Add {W}.
// End of the generated card header.

use crate::prelude::*;

/// "{T}: Add {W}.": a mana ability of a creature, so it can't be activated the turn the Myr
/// comes under its controller's control ([CR 302.6]): "A creature's activated ability with
/// the tap symbol or the untap symbol in its activation cost can't be activated unless the
/// creature has been under its controller's control continuously since their most recent
/// turn began."
///
/// It is in no deck: it was implemented for scenarios that need a creature that makes mana,
/// such as one that loses its abilities to Humility.
///
/// [CR 302.6]: https://yawgatog.com/resources/magic-rules/#R3026
pub static CARD: CardDef = Printed::GoldMyr
    .abilities(&[Ability::tap_for_mana(ManaSet::one(ManaColor::White))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const MYR: &str = Card::GoldMyr;

    pub const SCENARIOS: &[Scenario] = &[
        // The Myr pays for Portable Hole, a {W} artifact, with nothing else to tap.
        Scenario::new("its {W} casts a white spell")
            .hand(P0, &[Prop::Artifact])
            .battlefield(P0, &[MYR])
            .actions(&[
                ME.casts(Prop::Artifact),
                ME.tapped(MYR, 1),
                Resolve,
                ME.controls(Prop::Artifact, 1),
            ]),
    ];
}
// probe
