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

/// "Target opponent's hand" is looked at in full, for "a nonland card from it" to exile.
///
/// Looking at a hand shows it to the looker alone; the exiled card is public. Its owner may cast it
/// from exile for as long as it stays there ([CR 601.3]), and the {2} is a cost increase
/// ([CR 601.2f]): the total cost is the mana cost "plus all additional costs and cost increases".
/// The card is nonland, so playing it is casting it ([CR 701.18b]): "To play a card means to play
/// that card as a land or to cast that card as a spell, whichever is appropriate." The whole hand
/// is looked at, for one card to exile.
///
/// [CR 601.3]: https://yawgatog.com/resources/magic-rules/#R6013
/// [CR 601.2f]: https://yawgatog.com/resources/magic-rules/#R6012f
/// [CR 701.18b]: https://yawgatog.com/resources/magic-rules/#R70118b
pub static CARD: CardDef = Printed::EliteSpellbinder
    .keywords(Keywords::FLYING)
    .abilities(&[Ability::triggered(&Trigger::ENTERS)
        .target(&TargetSpec::opponent())
        .effect(&sequence(&[
            look_at(you(), hand_of(&target_player())),
            may_choose(
                you(),
                cards_in(
                    Zone::Hand,
                    &target_player(),
                    Filter::Not(&Filter::Types(Types::LAND)),
                ),
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
        // Only lands: nothing to exile, nothing to ask.
        Scenario::new("Spellbinder sees a hand of lands")
            .hand(P0, &[SPELLBINDER])
            .battlefield(P0, &[Card::Plains; 3])
            .hand(P1, &[Card::Island, Card::Plains])
            .actions(&[
                ME.casts(SPELLBINDER),
                advance_to(1, TurnStep::PostcombatMain),
                THEM.has_cards(Zone::Exile, 0),
                THEM.has_cards(Zone::Hand, 2),
            ]),
    ];
}
