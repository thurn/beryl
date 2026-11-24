// Generated from the card store by `just card header`: notes go below the end marker.
//! Thirst for Discovery {2}{U}
//! Instant
//!
//! Draw three cards. Then discard two cards unless you discard a basic land card.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::ThirstForDiscovery
    .abilities(&[Ability::spell().effect(&sequence(&[
        draw(you(), 3),
        unless(
            &discard(you(), 2),
            you(),
            Cost::part(&CostPart::discard(Filter::All(&[
                Filter::Types(Types::LAND),
                Filter::Supertypes(Supertypes::BASIC),
            ]))),
        ),
    ]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const THIRST: &str = Card::ThirstForDiscovery;

    /// P0's library, top first: three to draw, then one more so P0 doesn't lose by drawing from
    /// an empty library.
    const DRAWS: &[&str] = &[
        Card::Plains,
        Prop::Artifact,
        Card::Counterspell,
        Card::Island,
    ];

    pub const SCENARIOS: &[Scenario] = &[
        // Declining, P0 discards two cards, and the Plains may be one of them.
        Scenario::new("Thirst for Discovery: discarding two cards with a basic land in hand")
            .battlefield(P0, &[Card::Island; 3])
            .hand(P0, &[THIRST])
            .library(P0, DRAWS)
            .actions(&[
                ME.casts(THIRST),
                Resolve,
                ME.no(),
                ME.picks(Prop::Artifact),
                ME.picks(Card::Plains),
                ME.has(Zone::Graveyard, Prop::Artifact, 1),
                ME.has(Zone::Graveyard, Card::Plains, 1),
                ME.has(Zone::Hand, Card::Counterspell, 1),
            ]),
    ];
}
