// Generated from the card store by `just card header`: notes go below the end marker.
//! Tormod's Crypt {0}
//! Artifact
//!
//! {T}, Sacrifice this artifact: Exile target player's graveyard.
// End of the generated card header.

use crate::prelude::*;

/// Implemented for scenarios: an artifact that empties a graveyard at instant speed, so that
/// a card can leave a graveyard while a spell or ability that names it waits, as Animate
/// Dead's enters ability does.
///
/// "{T}, Sacrifice this artifact: Exile target player's graveyard." Every card in it goes to
/// exile at once ([CR 406.2]): "To exile an object is to put it into the exile zone from
/// whatever zone it's currently in."
///
/// [CR 406.2]: https://yawgatog.com/resources/magic-rules/#R4062
pub static CARD: CardDef = Printed::TormodsCrypt
    .abilities(&[Ability::activated(tap_and_sacrifice())
        .target(&TargetSpec::player())
        .effect(&exile(graveyard_of(&target_player())))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const CRYPT: &str = Card::TormodsCrypt;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Tormod's Crypt exiles an opponent's graveyard")
            .battlefield(P0, &[CRYPT])
            .graveyard(P0, &[Prop::Vanilla])
            .graveyard(P1, &[Prop::Flier, Prop::Instant])
            .actions(&[
                ME.activates(CRYPT, 0),
                ME.targets_player(THEM),
                Resolve,
                ME.has(Zone::Graveyard, CRYPT, 1),
                THEM.has_cards(Zone::Graveyard, 0),
                THEM.has(Zone::Exile, Prop::Flier, 1),
                THEM.has(Zone::Exile, Prop::Instant, 1),
                ME.has(Zone::Graveyard, Prop::Vanilla, 1),
            ]),
        Scenario::new("Tormod's Crypt exiles its controller's graveyard")
            .battlefield(P0, &[CRYPT])
            .graveyard(P0, &[Prop::Vanilla])
            .graveyard(P1, &[Prop::Flier])
            .actions(&[
                ME.activates(CRYPT, 0),
                ME.targets_player(ME),
                Resolve,
                ME.has(Zone::Exile, Prop::Vanilla, 1),
                // The Crypt was sacrificed as a cost, before the graveyard was exiled.
                ME.has(Zone::Exile, CRYPT, 1),
                THEM.has(Zone::Graveyard, Prop::Flier, 1),
            ]),
    ];
}
