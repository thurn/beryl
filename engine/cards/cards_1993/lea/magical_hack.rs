// Generated from the card store by `just card header`: notes go below the end marker.
//! Magical Hack {U}
//! Instant
//!
//! Change the text of target spell or permanent by replacing all instances of one basic land type
//! with another. (For example, you may change "swampwalk" to "plainswalk." This effect lasts
//! indefinitely.)
// End of the generated card header.

use crate::prelude::*;

/// A text change applied to a permanent spell carries over to the permanent it becomes
/// ([CR 400.7a]): "Effects from spells, activated abilities, and triggered abilities that
/// change the characteristics or controller of a permanent spell on the stack continue to
/// apply to the permanent that spell becomes."
///
/// [CR 400.7a]: https://yawgatog.com/resources/magic-rules/#R4007a
pub static CARD: CardDef = Printed::MagicalHack
    .abilities(&[Ability::spell()
        .target(&TargetSpec::one(TargetKind::SpellOrPermanent, Filter::Any))
        .effect(&change_text(
            target(),
            TextWords::LandTypes,
            Duration::Indefinite,
        ))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const HACK: &str = Card::MagicalHack;
    const DANDAN: &str = Card::Dandan;
    const ISLAND: TextWord = TextWord::LandType(Subtype::Island);
    const SWAMP: TextWord = TextWord::LandType(Subtype::Swamp);

    pub const SCENARIOS: &[Scenario] = &[
        // A Forest whose "Forest" reads "Island" is an Island, which taps for {U}: it pays
        // for a second Magical Hack.
        Scenario::new("Magical Hack makes a Forest an Island")
            .battlefield(P0, &[Card::Forest, Card::Island])
            .hand(P0, &[HACK, HACK])
            .actions(&[
                ME.casts(HACK),
                ME.targets(Card::Forest),
                Resolve,
                ME.chooses_word(TextWord::LandType(Subtype::Forest)),
                ME.chooses_word(ISLAND),
                ME.casts(HACK),
                ME.targets(Card::Forest),
                ME.tapped(Card::Forest, 1),
            ]),
        // The change lasts indefinitely: on P0's next turn the Forest is still an Island and
        // not a Forest.
        Scenario::new("Magical Hack's change outlasts the turn")
            .battlefield(P0, &[Card::Forest, Card::Island])
            .hand(P0, &[HACK])
            .library(P0, &[Card::Plains])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.casts(HACK),
                ME.targets(Card::Forest),
                Resolve,
                ME.chooses_word(TextWord::LandType(Subtype::Forest)),
                ME.chooses_word(ISLAND),
                advance_to(3, TurnStep::Upkeep),
                ME.has_subtype(Card::Forest, Subtype::Island, true),
                ME.has_subtype(Card::Forest, Subtype::Forest, false),
            ]),
        // A permanent with no land type words is a legal target. The first question offers
        // the five basic land types and no color word, and the second the four others.
        Scenario::new("Magical Hack asks for a land type and then another")
            .battlefield(P0, &[Card::Island])
            .battlefield(P1, &[Prop::Vanilla])
            .hand(P0, &[HACK])
            .actions(&[
                ME.casts(HACK),
                ME.targets(Prop::Vanilla),
                Resolve,
                ME.deciding(DecisionKind::ChooseObject, 5),
                ME.chooses_word(ISLAND),
                ME.deciding(DecisionKind::ChooseObject, 4),
                ME.chooses_word(SWAMP),
                THEM.controls(Prop::Vanilla, 1),
            ]),
        // Dandân's "Island" reads "Swamp", and P0 controls an Island but no Swamp.
        Scenario::new("Magical Hack has Dandân sacrificed for want of a Swamp")
            .interaction(&[DANDAN])
            .battlefield(P0, &[DANDAN, Card::Island])
            .hand(P0, &[HACK])
            .actions(&[
                ME.casts(HACK),
                ME.targets(DANDAN),
                Resolve,
                ME.chooses_word(ISLAND),
                ME.chooses_word(SWAMP),
                Resolve,
                ME.has(Zone::Graveyard, DANDAN, 1),
            ]),
        // Magical Hack changes the Dandân spell's "Island" to "Swamp" before it resolves.
        // The permanent keeps the change, so P0, who controls Islands and no Swamp, sacrifices
        // it as soon as it enters.
        Scenario::new("Magical Hack on a Dandân spell carries to the permanent")
            .interaction(&[DANDAN])
            .battlefield(P0, &[Card::Island, Card::Island, Card::Island])
            .hand(P0, &[DANDAN, HACK])
            .actions(&[
                ME.casts(DANDAN),
                ME.casts(HACK),
                ME.targets(DANDAN),
                Resolve,
                ME.chooses_word(ISLAND),
                ME.chooses_word(SWAMP),
                Resolve,
                Resolve,
                ME.has(Zone::Graveyard, DANDAN, 1),
            ]),
        Scenario::new(
            "Magical Hack on Dandân in a shared game causes it to be sacrificed to the shared graveyard",
        )
        .shared_library(&[Card::Island])
        .interaction(&[DANDAN])
        .battlefield(P0, &[DANDAN, Card::Island])
        .hand(P0, &[HACK])
        .actions(&[
            ME.casts(HACK),
            ME.targets(DANDAN),
            Resolve,
            ME.chooses_word(ISLAND),
            ME.chooses_word(SWAMP),
            Resolve,
            ME.has(Zone::Graveyard, DANDAN, 1),
            THEM.has(Zone::Graveyard, DANDAN, 1),
        ]),
    ];
}
