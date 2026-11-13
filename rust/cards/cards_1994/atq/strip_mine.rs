// Generated from the card store by `just card header`: notes go below the end marker.
//! Strip Mine
//! Land
//!
//! {T}: Add {C}.
//! {T}, Sacrifice this land: Destroy target land.
// End of the generated card header.

use crate::prelude::*;

/// The second ability is not a mana ability: it has a target ([CR 605.1a]), so it uses the stack.
/// [CR 701.8a]: "To destroy a permanent, move it from the battlefield to its owner's graveyard."
///
/// [CR 605.1a]: https://yawgatog.com/resources/magic-rules/#R6051a
/// [CR 701.8a]: https://yawgatog.com/resources/magic-rules/#R7018a
pub static CARD: CardDef = Printed::StripMine
    .abilities(&[
        Ability::tap_for_mana(ManaSet::one(ManaColor::Colorless)),
        Ability::activated(Cost::of_parts(&[CostPart::TAP, CostPart::SACRIFICE]))
            .target(&TargetSpec::permanent(Filter::Types(Types::LAND)))
            .effect(&destroy(target())),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    /// Its {C} pays a creature's generic mana. Strip Mine may target itself, since targets are
    /// chosen before costs are paid (CR 601.2c, 601.2h), so the target is a decision between the
    /// two lands; with a second Strip Mine, it is a decision between three, as targeting itself and
    /// its twin lead to different games.
    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Strip Mine destroys a basic land")
            .battlefield(P0, &[Card::StripMine])
            .battlefield(P1, &[Card::Plains])
            .actions(&[
                ME.activates(Card::StripMine, 1), // {T}, Sacrifice
                ME.targets(Card::Plains),
                ME.has(Zone::Graveyard, Card::StripMine, 1),
                THEM.has(Zone::Graveyard, Card::Plains, 1),
                ME.has_cards(Zone::Battlefield, 0),
                THEM.has_cards(Zone::Battlefield, 0),
            ]),
        Scenario::new("Strip Mine tells itself from its twin")
            .battlefield(P0, &[Card::StripMine, Card::StripMine])
            .battlefield(P1, &[Card::Plains])
            .actions(&[
                ME.activates(Card::StripMine, 1), // {T}, Sacrifice
                ME.deciding(DecisionKind::Target, 3),
                ME.targets(Card::Plains),
                Resolve,
                ME.has(Zone::Graveyard, Card::StripMine, 1),
                THEM.has(Zone::Graveyard, Card::Plains, 1),
                ME.has(Zone::Battlefield, Card::StripMine, 1),
            ]),
        // The Island pays the {U}, Strip Mine the {1}. The land still to play keeps the game in
        // P0's main phase.
        Scenario::new("Strip Mine's colorless mana")
            .battlefield(P0, &[Card::StripMine, Card::Island])
            .hand(P0, &[Prop::SmallCreature, Card::Plains])
            .actions(&[
                ME.casts(Prop::SmallCreature),
                ME.tapped(Card::StripMine, 1),
                Resolve,
                ME.controls(Prop::SmallCreature, 1),
            ]),
    ];
}
