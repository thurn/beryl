// Generated from the card store by `just card header`: notes go below the end marker.
//! Lion's Eye Diamond {0}
//! Artifact
//!
//! Discard your hand, Sacrifice this artifact: Add three mana of any one color. Activate only as an
//! instant.
// End of the generated card header.

use crate::prelude::*;

/// "Activate only as an instant" requires priority. This overrides the permission for a
/// mana ability to be activated during a payment ([CR 605.3a]): "whenever they are casting
/// a spell or activating an ability that requires a mana payment". Lion's Eye Diamond
/// must be activated before casting a spell, so its cost would discard a spell still in hand.
///
/// [CR 605.3a]: https://yawgatog.com/resources/magic-rules/#R6053a
pub static CARD: CardDef = Printed::LionsEyeDiamond
    .abilities(&[Ability::mana(
        Cost::of_parts(&[CostPart::DiscardHand, CostPart::SACRIFICE]),
        ManaSet::any_color(3),
    )
    .as_an_instant()])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const LED: &str = Card::LionsEyeDiamond;

    pub const SCENARIOS: &[Scenario] = &[
        // Its controller discards both cards, sacrifices it and gets three mana of the color
        // they chose. The second Diamond keeps P0's priority open, so the mana is still in
        // the pool there.
        Scenario::new("it discards the hand for three mana of one color")
            .battlefield(P0, &[LED, LED])
            .hand(P0, &[Card::Island, Card::Plains])
            .actions(&[
                ME.activates(LED, 0),
                ME.chooses_color(Colors::BLUE),
                stack(0),
                ME.mana(ManaColor::Blue, 3),
                ME.mana(ManaColor::White, 0),
                ME.has_cards(Zone::Hand, 0),
                ME.has(Zone::Graveyard, Card::Island, 1),
                ME.has(Zone::Graveyard, Card::Plains, 1),
                ME.has(Zone::Graveyard, LED, 1),
                ME.controls(LED, 1),
            ]),
        // Brain Freeze costs {1}{U}, and the Diamond is the only source. It can't be
        // activated while the spell's cost is paid, and activating it first discards the
        // spell: P0 may only pass or activate it.
        Scenario::new("it can't pay for a spell cast from the hand")
            .interaction(&[Card::BrainFreeze])
            .battlefield(P0, &[LED])
            .hand(P0, &[Card::BrainFreeze])
            .actions(&[ME.deciding(DecisionKind::Priority, 2)]),
    ];
}
