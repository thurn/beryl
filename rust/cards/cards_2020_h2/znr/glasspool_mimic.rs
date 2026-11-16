// Generated from the card store by `just card header`: notes go below the end marker.
//! Glasspool Mimic {2}{U}
//! Creature — Shapeshifter Rogue, 0/0
//!
//! You may have this creature enter as a copy of a creature you control, except it's a Shapeshifter
//! Rogue in addition to its other types.
//!
//! Glasspool Shore
//! Land
//!
//! This land enters tapped.
//! {T}: Add {U}.
// End of the generated card header.

use crate::prelude::*;

/// [CR 712.8b](https://yawgatog.com/resources/magic-rules/#R7128b): "A player casting a modal
/// double-faced card ... chooses which face they are casting"; [CR 712.9] lets it be played as
/// the land face. The land face enters tapped and taps for {U}.
///
/// "You may have this creature enter as a copy of a creature you control" is a replacement
/// effect that has it become a copy as it enters ([CR 707.5]): "It doesn't enter the
/// battlefield, and then become a copy of that permanent." The creature is chosen, not
/// targeted. Declining, it enters as itself, a 0/0 that dies.
///
/// The exception adds two creature types to the copy's copiable values ([CR 707.9b]): "Some
/// copy effects modify a characteristic as part of the copying process. The final set of
/// values for that characteristic becomes part of the copiable values of the copy." So a copy
/// of the Mimic is a Shapeshifter Rogue too.
///
/// [CR 712.9]: https://yawgatog.com/resources/magic-rules/#R7129
/// [CR 712.8b]: https://yawgatog.com/resources/magic-rules/#R7128b
/// [CR 707.5]: https://yawgatog.com/resources/magic-rules/#R7075
/// [CR 707.9b]: https://yawgatog.com/resources/magic-rules/#R7079b
pub static CARD: CardDef = card(
    Card::GlasspoolMimic,
    &[
        Printed::GlasspoolMimic
            .abilities(&[Ability::replacing(&Replacement::enter_as_copy(
                &CREATURES_YOU_CONTROL,
                Some(ExceptIx::new(0)),
            ))])
            .excepts(&[CopyException(&[
                CopyChange::AddSubtype(Subtype::Shapeshifter),
                CopyChange::AddSubtype(Subtype::Rogue),
            ])]),
        Printed::GlasspoolShore.abilities(&[
            Ability::replacing(&Replacement::ENTERS_TAPPED),
            Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)),
        ]),
    ],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const MIMIC: &str = Card::GlasspoolMimic;
    const SHORE: &str = "Glasspool Shore";
    const DANDAN: &str = Card::Dandan;
    const MIND_BEND: &str = Card::MindBend;
    const ISLAND: TextWord = TextWord::LandType(Subtype::Island);
    const PLAINS: TextWord = TextWord::LandType(Subtype::Plains);

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Glasspool Shore enters tapped and later pays for Glasspool Mimic")
            .battlefield(P0, &[Card::Island, Card::Island])
            .permanents(P0, &[Permanent::new(Card::Ornithopter).tapped()])
            .hand(P0, &[MIMIC, MIMIC])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.plays(MIMIC),
                ME.tapped(SHORE, 1),
                advance_to(3, TurnStep::PrecombatMain),
                ME.tapped(SHORE, 0),
                ME.casts(MIMIC),
                ME.tapped(SHORE, 1),
                Resolve,
                ME.picks_up_to(Card::Ornithopter),
                ME.controls(Card::Ornithopter, 2),
            ]),
        // The Mimic enters as a copy of Dandan, a 4/1 Fish, and is a Shapeshifter Rogue as well.
        Scenario::new("the Mimic enters as a Shapeshifter Rogue copy")
            .interaction(&[DANDAN])
            .battlefield(P0, &[DANDAN, Card::Island, Card::Island, Card::Island])
            .hand(P0, &[MIMIC])
            .actions(&[
                ME.casts(MIMIC),
                Resolve,
                ME.picks_up_to(DANDAN),
                ME.controls(DANDAN, 2),
            ]),
        // Declining, it enters as itself, a 0/0, and dies.
        Scenario::new("a Mimic that copies nothing dies")
            .interaction(&[DANDAN])
            .battlefield(P0, &[DANDAN, Card::Island, Card::Island, Card::Island])
            .hand(P0, &[MIMIC])
            .actions(&[
                ME.casts(MIMIC),
                Resolve,
                ME.done_picking(),
                ME.has(Zone::Graveyard, MIMIC, 1),
            ]),
        // Mind Bend changes "Island" to "Plains" in the copy's text, and the copy is still a
        // Shapeshifter Rogue ([CR 707.9b]). The copy is the newest Dandan, which Mind Bend
        // targets; the original is never a Rogue, so a Rogue among the two is the copy. A
        // second Mind Bend in hand keeps P0 at a decision, where the game stops.
        Scenario::new("a text change on the copy keeps its added subtypes")
            .interaction(&[DANDAN, MIND_BEND])
            .battlefield(
                P0,
                &[
                    DANDAN,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Plains,
                ],
            )
            .hand(P0, &[MIMIC, MIND_BEND, MIND_BEND])
            .actions(&[
                ME.casts(MIMIC),
                Resolve,
                ME.picks_up_to(DANDAN),
                ME.has_subtype(DANDAN, Subtype::Rogue, true),
                ME.casts(MIND_BEND),
                ME.targets(DANDAN),
                Resolve,
                ME.chooses_word(ISLAND),
                ME.chooses_word(PLAINS),
                ME.controls(DANDAN, 2),
                ME.has_subtype(DANDAN, Subtype::Rogue, true),
                ME.has_subtype(DANDAN, Subtype::Shapeshifter, true),
            ]),
    ];
}
