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

/// The subtype exception is part of the copy's copiable values, so a later copy is also
/// a Shapeshifter Rogue. [CR 707.9b] says “The final set of values for that characteristic
/// becomes part of the copiable values of the copy.”
///
/// [CR 707.9b]: https://yawgatog.com/resources/magic-rules/#R7079b
pub static CARD: CardDef = card_with_shape(
    Card::GlasspoolMimic,
    PrintedShape::GlasspoolMimic,
    &[
        Printed::GlasspoolMimic
            .abilities(&[Ability::replacing(&Replacement::enter_as_copy(
                &Filter::All(&[Filter::Types(Types::CREATURE), Filter::Whose(&Players::You)]),
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
    const DANDAN: &str = Card::Dandan;
    const MIND_BEND: &str = Card::MindBend;
    const ISLAND: TextWord = TextWord::LandType(Subtype::Island);
    const PLAINS: TextWord = TextWord::LandType(Subtype::Plains);

    pub const SCENARIOS: &[Scenario] = &[
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
