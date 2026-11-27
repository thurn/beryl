// Generated from the card store by `just card header`: notes go below the end marker.
//! Necrotic Ooze {2}{B}{B}
//! Creature — Ooze, 4/3
//!
//! As long as this creature is on the battlefield, it has all activated abilities of all creature
//! cards in all graveyards.
// End of the generated card header.

use crate::prelude::*;

static LENT: Lent = Lent {
    from: LentFrom::Graveyards(Filter::Types(Types::CREATURE)),
    which: Lends::Activated,
};

static LEND_ABILITIES: LayerSpec = LayerSpec {
    affected: this(),
    condition: None,
    modifications: &[Modification::Lend(&LENT)],
};

pub static CARD: CardDef = Printed::NecroticOoze
    .abilities(&[Ability::static_ability(&LEND_ABILITIES)])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const OOZE: &str = Card::NecroticOoze;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Necrotic Ooze gains activated abilities of creature cards in graveyards")
            .interaction(&[Card::LilianasShade])
            .battlefield(P0, &[OOZE, Card::Swamp])
            .graveyard(P1, &[Card::LilianasShade])
            .actions(&[ME.activates_lent(OOZE, 0, 0), Resolve, ME.pt(OOZE, 5, 4)]),
        Scenario::new(
            "Other creatures do not gain activated abilities of creature cards in graveyards",
        )
        .interaction(&[Card::LilianasShade])
        .battlefield(P0, &[OOZE, Prop::Vanilla, Card::Swamp])
        .graveyard(P1, &[Card::LilianasShade])
        .actions(&[ME.deciding(DecisionKind::Priority, 2)]),
    ];
}
