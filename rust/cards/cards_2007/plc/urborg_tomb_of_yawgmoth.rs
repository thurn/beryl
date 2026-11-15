// Generated from the card store by `just card header`: notes go below the end marker.
//! Urborg, Tomb of Yawgmoth
//! Legendary Land
//!
//! Each land is a Swamp in addition to its other land types.
// End of the generated card header.

use crate::prelude::*;

/// "Each land is a Swamp in addition to its other land types" gives each land on the
/// battlefield, its own included, the land type Swamp in layer 4, and with it the Swamp's
/// mana ability ([CR 305.7]): "If a land gains one or more land types in addition to its own,
/// it keeps its land types and rules text, and it gains the new land types and mana
/// abilities." Urborg has no mana ability of its own: it taps for {B} as a Swamp.
///
/// An effect that sets a land's land types, as Blood Moon's, removes this ability from Urborg
/// and so applies first whatever the timestamps ([CR 613.8b]): "An effect dependent on one or
/// more other effects waits to apply until just after all of those effects have been
/// applied."
///
/// [CR 305.7]: https://yawgatog.com/resources/magic-rules/#R3057
/// [CR 613.8b]: https://yawgatog.com/resources/magic-rules/#R6138b
pub static CARD: CardDef = Printed::UrborgTombOfYawgmoth
    .abilities(&[Ability::static_ability(&LayerSpec {
        affected: Objects::All {
            zone: Zone::Battlefield,
            filter: Filter::Types(Types::LAND),
        },
        condition: None,
        modifications: &[Modification::AddSubtype(Subtype::Swamp)],
    })])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const URBORG: &str = Card::UrborgTombOfYawgmoth;

    // Damn is a {B}{B} sorcery that destroys target creature. Each scenario has a second creature, so Damn's target is a
    // choice the scenario names, as the oracles need.

    pub const SCENARIOS: &[Scenario] = &[
        // Urborg is a Swamp and so is the Plains: together they pay Damn's {B}{B}.
        Scenario::new("Urborg and another land are Swamps")
            .interaction(&[Card::Damn])
            .battlefield(P0, &[URBORG, Card::Plains, Prop::Vanilla])
            .hand(P0, &[Card::Damn])
            .battlefield(P1, &[Prop::Flier])
            .actions(&[
                ME.casts(Card::Damn),
                ME.targets(Prop::Flier),
                Resolve,
                THEM.has(Zone::Graveyard, Prop::Flier, 1),
            ]),
        // Each land, whoever controls it: the opponent's Plains taps for {B} too.
        Scenario::new("the opponent's lands are Swamps too")
            .interaction(&[Card::Damn])
            .battlefield(P0, &[URBORG, Prop::Flier])
            .battlefield(P1, &[Card::Plains, Card::Plains, Prop::Vanilla])
            .hand(P1, &[Card::Damn])
            .actions(&[
                ME.done_attacking(),
                advance_to(2, TurnStep::PrecombatMain),
                THEM.casts(Card::Damn),
                THEM.targets(Prop::Flier),
                Resolve,
                ME.has(Zone::Graveyard, Prop::Flier, 1),
            ]),
        // Dryad Arbor keeps its Forest type and gains Swamp: it taps for {B}, with Urborg's
        // own {B} paying for Damn.
        Scenario::new("Dryad Arbor is a Swamp too")
            .interaction(&[Card::Damn])
            .battlefield(P0, &[URBORG, Card::DryadArbor])
            .hand(P0, &[Card::Damn])
            .battlefield(P1, &[Prop::Flier])
            .actions(&[
                ME.casts(Card::Damn),
                ME.targets(Prop::Flier),
                Resolve,
                THEM.has(Zone::Graveyard, Prop::Flier, 1),
            ]),
    ];
}
