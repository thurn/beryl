// Generated from the card store by `just card header`: notes go below the end marker.
//! Glorious Anthem {1}{W}{W}
//! Enchantment
//!
//! Creatures you control get +1/+1.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::GloriousAnthem
    .abilities(&[Ability::static_ability(&LayerSpec {
        affected: Objects::All {
            zone: Zone::Battlefield,
            filter: Filter::All(&[Filter::Types(Types::CREATURE), Filter::Whose(&Players::You)]),
        },
        condition: None,
        modifications: &[Modification::ModifyPt(1, 1)],
    })])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const ANTHEM: &str = Card::GloriousAnthem;

    pub const SCENARIOS: &[Scenario] = &[
        // Swift Reconfiguration makes the 2/1 a noncreature Vehicle in layer 4; crewing it
        // makes it an artifact creature again, also in layer 4, until end of turn. The
        // Anthem's +1/+1 applies in layer 7c to what layer 4 left a creature, so the crewed
        // Vehicle is a 3/2. Crew 5: the two others are 4/2 under the Anthem and both tap.
        Scenario::new("a permanent that becomes a creature gets +1/+1")
            .interaction(&[Card::SwiftReconfiguration])
            .hand(P0, &[Card::SwiftReconfiguration])
            .battlefield(
                P0,
                &[
                    ANTHEM,
                    Card::Plains,
                    Prop::Vanilla,
                    Prop::Flier,
                    Prop::Flier,
                ],
            )
            .library(P0, &[Card::Plains])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.pt(Prop::Vanilla, 3, 2),
                ME.casts(Card::SwiftReconfiguration),
                ME.targets(Prop::Vanilla),
                Resolve,
                ME.creature(Prop::Vanilla, false),
                Action::activate_granted(P0, Prop::Vanilla, 0),
                ME.tapped(Prop::Flier, 2),
                Resolve,
                ME.creature(Prop::Vanilla, true),
                ME.pt(Prop::Vanilla, 3, 2),
            ]),
    ];
}
