// Generated from the card store by `just card header`: notes go below the end marker.
//! Gathan Raiders {3}{R}{R}
//! Creature — Human Warrior, 3/3
//!
//! Hellbent — This creature gets +2/+2 as long as you have no cards in hand.
//! Morph—Discard a card. (You may cast this card face down as a 2/2 creature for {3}. Turn it face
//! up any time for its morph cost.)
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::GathanRaiders
    .abilities(&[Ability::static_ability(&LayerSpec {
        affected: Objects::This,
        condition: Some(Condition::Compare(
            Amount::Count(&hand_of(&Players::You)),
            Comparison::Equal,
            Amount::Fixed(0),
        )),
        modifications: &[Modification::ModifyPt(2, 2)],
    })])
    .casting(&[CastingOption::Morph(Cost::part(&CostPart::discard(
        Filter::Any,
    )))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const RAIDERS: &str = Card::GathanRaiders;
    const MOUNTAIN: &str = Card::Mountain;
    const ISLAND: &str = Card::Island;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Discarding the last card to turn the Raiders up gives them hellbent")
            .hand(P0, &[RAIDERS, ISLAND])
            .battlefield(P0, &[MOUNTAIN, MOUNTAIN, MOUNTAIN])
            .actions(&[
                ME.casts_face(RAIDERS, FaceIx::DOWN),
                Resolve,
                ME.pt(RAIDERS, 2, 2),
                Action::TurnFaceUp {
                    player: P0,
                    card: RAIDERS,
                },
                ME.has(Zone::Graveyard, ISLAND, 1),
                ME.pt(RAIDERS, 5, 5),
            ]),
        Scenario::new("Drawing a card makes marked damage lethal to the Raiders")
            .interaction(&[Card::LightningBolt])
            .at(2, P1, TurnStep::Upkeep)
            .battlefield(P0, &[MOUNTAIN, Prop::Vanilla])
            .hand(P0, &[Card::LightningBolt])
            .battlefield(P1, &[RAIDERS])
            .library(P1, &[ISLAND])
            .actions(&[
                THEM.pt(RAIDERS, 5, 5),
                ME.pt(Prop::Vanilla, 2, 1),
                ME.casts(Card::LightningBolt),
                ME.targets(RAIDERS),
                advance_to(2, TurnStep::Draw),
                THEM.has(Zone::Hand, ISLAND, 1),
                THEM.has(Zone::Graveyard, RAIDERS, 1),
            ]),
    ];
}
