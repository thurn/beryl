// Generated from the card store by `just card header`: notes go below the end marker.
//! Mystic Gate
//! Land
//!
//! {T}: Add {C}.
//! {W/U}, {T}: Add {W}{W}, {W}{U}, or {U}{U}.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::MysticGate
    .abilities(&[
        Ability::tap_for_mana(ManaSet::one(ManaColor::Colorless)),
        Ability::mana_of(
            Cost::of(mana("{W/U}"), &[CostPart::TAP]),
            &[Term::each_of(
                (1 << ManaColor::White.index()) | (1 << ManaColor::Blue.index()),
                2,
            )],
        ),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        // Mystic Gate's first ability adds {C}, not {W}: with only Mystic Gate on the
        // battlefield, P0 cannot cast Mother of Runes ({W}), so Priority offers only Pass
        // and playing Island.
        Scenario::new("Mystic Gate cannot cast a white spell by itself")
            .interaction(&[Card::MotherOfRunes])
            .battlefield(P0, &[Card::MysticGate])
            .hand(P0, &[Card::MotherOfRunes, Card::Island])
            .actions(&[ME.deciding(DecisionKind::Priority, 2)]),
    ];
}
