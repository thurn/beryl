// Generated from the card store by `just card header`: notes go below the end marker.
//! Cabal Coffers
//! Land
//!
//! {2}, {T}: Add {B} for each Swamp you control.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::CabalCoffers
    .abilities(&[Ability::mana_of(
        Cost::of(ManaCost::generic(2), &[CostPart::TAP]),
        &[Term::counted(
            ManaColor::Black,
            Amount::Count(&permanents(Filter::All(&[
                Filter::Subtype(Subtype::Swamp),
                yours(),
            ]))),
        )],
    )])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        // Three Swamps alone produce only three mana, so casting Liliana's Shade ({2}{B}{B})
        // requires activating Cabal Coffers: two Swamps pay {2}, Cabal Coffers adds {B}{B}{B},
        // and the third Swamp adds {B}.
        Scenario::new("Cabal Coffers pays for a four-mana spell with three Swamps")
            .interaction(&[Card::LilianasShade])
            .battlefield(
                P0,
                &[Card::CabalCoffers, Card::Swamp, Card::Swamp, Card::Swamp],
            )
            .hand(P0, &[Card::LilianasShade])
            .actions(&[
                ME.casts(Card::LilianasShade),
                ME.tapped(Card::CabalCoffers, 1),
                Resolve,
                Resolve,
                ME.no(),
                ME.controls(Card::LilianasShade, 1),
            ]),
    ];
}
