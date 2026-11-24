// Generated from the card store by `just card header`: notes go below the end marker.
//! Tormod's Crypt {0}
//! Artifact
//!
//! {T}, Sacrifice this artifact: Exile target player's graveyard.
// End of the generated card header.

use crate::prelude::*;

/// Implemented for scenarios that remove a card from a graveyard in response to a spell or
/// ability, such as Animate Dead's enters trigger.
pub static CARD: CardDef = Printed::TormodsCrypt
    .abilities(&[Ability::activated(tap_and_sacrifice())
        .target(&TargetSpec::player())
        .effect(&exile(graveyard_of(&target_player())))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const CRYPT: &str = Card::TormodsCrypt;

    pub const SCENARIOS: &[Scenario] =
        &[
            Scenario::new("Tormod's Crypt exiles an opponent's graveyard")
                .battlefield(P0, &[CRYPT])
                .permanents(P0, &[Permanent::new(CRYPT).tapped()])
                .graveyard(P0, &[Prop::Vanilla])
                .graveyard(P1, &[Prop::Flier, Prop::Instant])
                .actions(&[
                    ME.deciding(DecisionKind::Priority, 2),
                    ME.activates(CRYPT, 0),
                    ME.targets_player(THEM),
                    Resolve,
                    ME.has(Zone::Graveyard, CRYPT, 1),
                    THEM.has_cards(Zone::Graveyard, 0),
                    THEM.has(Zone::Exile, Prop::Flier, 1),
                    THEM.has(Zone::Exile, Prop::Instant, 1),
                    ME.has(Zone::Graveyard, Prop::Vanilla, 1),
                ]),
        ];
}
