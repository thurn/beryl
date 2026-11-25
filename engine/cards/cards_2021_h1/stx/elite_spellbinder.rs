// Generated from the card store by `just card header`: notes go below the end marker.
//! Elite Spellbinder {2}{W}
//! Creature — Human Cleric, 3/1
//!
//! Flying
//! When this creature enters, look at target opponent's hand. You may exile a nonland card from it.
//! For as long as that card remains exiled, its owner may play it. A spell cast this way costs {2}
//! more to cast.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::EliteSpellbinder
    .keywords(Keywords::FLYING)
    .abilities(&[Ability::triggered(&Trigger::ENTERS)
        .target(&TargetSpec::opponent())
        .effect(&sequence(&[
            look_at(you(), hand_of(&target_player())),
            may_choose(
                you(),
                cards_in(Zone::Hand, &target_player(), nonland()),
                Prompt::new(Purpose::Look)
                    .most(1)
                    .places(Place::Exile, Place::None),
            ),
            exile(those()),
            permit_cast(
                owner_of(those()),
                those(),
                Duration::Indefinite,
                CastTerms::PLAIN.costing_more(2).played(),
            ),
        ]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const SPELLBINDER: &str = Card::EliteSpellbinder;

    pub const SCENARIOS: &[Scenario] = &[
        // The exiled artifact costs its owner {2}{W} on their turn: all three Plains.
        Scenario::new("Spellbinder's card costs its owner {2} more")
            .hand(P0, &[SPELLBINDER])
            .battlefield(P0, &[Card::Plains; 3])
            .hand(P1, &[Prop::Artifact, Card::Island])
            .battlefield(P1, &[Card::Plains; 3])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.casts(SPELLBINDER),
                Resolve,
                // The enters trigger, which targets the one opponent, resolves up to its question.
                stack(1),
                Resolve,
                ME.yes(),
                THEM.has(Zone::Exile, Prop::Artifact, 1),
                THEM.has(Zone::Hand, Prop::Artifact, 0),
                advance_to(2, TurnStep::PrecombatMain),
                // Pass, an Island to play, the one P0 saw or the one just drawn, and the artifact.
                THEM.deciding(DecisionKind::Priority, 4),
                THEM.casts(Prop::Artifact),
                THEM.tapped(Card::Plains, 3),
                Resolve,
                THEM.controls(Prop::Artifact, 1),
            ]),
    ];
}
