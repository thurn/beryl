// Generated from the card store by `just card header`: notes go below the end marker.
//! Dress Down {1}{U}
//! Enchantment
//!
//! Flash
//! When this enchantment enters, draw a card.
//! Creatures lose all abilities.
//! At the beginning of the end step, sacrifice this enchantment.
// End of the generated card header.

use crate::prelude::*;

/// A creature whose power was defined by its own ability loses that definition too.
/// For example, Adeline's printed asterisk is zero while her ability is absent.
/// [CR 208.5] says “If a creature somehow has no value for its power, its power is 0.”
///
/// [CR 208.5]: https://yawgatog.com/resources/magic-rules/#R2085
pub static CARD: CardDef = Printed::DressDown
    .keywords(Keywords::FLASH)
    .abilities(&[
        when_enters(&draw(you(), 1)),
        Ability::static_ability(&LayerSpec {
            affected: permanents(creature()),
            condition: None,
            modifications: &[Modification::LoseAllAbilities],
        }),
        Ability::triggered(&Trigger::step(TurnStep::End, Players::EachPlayer))
            .effect(&sacrifice(this())),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const DRESS_DOWN: &str = Card::DressDown;

    pub const SCENARIOS: &[Scenario] = &[
        // Adeline's power was defined by her own ability, which is gone: her printed power
        // is *, which is 0. The flier keeps its printed 3/1.
        Scenario::new("creatures keep printed power but lose defined power")
            .interaction(&[Card::AdelineResplendentCathar])
            .battlefield(
                P0,
                &[DRESS_DOWN, Card::AdelineResplendentCathar, Prop::Flier],
            )
            .actions(&[
                ME.pt(Card::AdelineResplendentCathar, 0, 4),
                ME.pt(Prop::Flier, 3, 1),
            ]),
        // The Skeletons' regeneration can't be activated: with nothing to do but pass, P0 is
        // given no priority in the main phase, and the first decision is the attack.
        Scenario::new("an activated ability can't be activated")
            .interaction(&[Card::DrudgeSkeletons])
            .battlefield(P0, &[DRESS_DOWN, Card::Swamp, Card::DrudgeSkeletons])
            .actions(&[ME.deciding(DecisionKind::Attacker, 2)]),
        // At the end step it triggers and is sacrificed, and Adeline's power is hers again.
        Scenario::new("it is sacrificed at the beginning of the end step")
            .interaction(&[Card::AdelineResplendentCathar])
            .battlefield(P0, &[DRESS_DOWN, Card::AdelineResplendentCathar])
            .library(P0, &[Card::Island, Card::Island])
            .library(P1, &[Card::Island, Card::Island])
            .actions(&[
                ME.done_attacking(),
                advance_to(1, TurnStep::End),
                stack(1),
                Resolve,
                ME.controls(DRESS_DOWN, 0),
                ME.has(Zone::Graveyard, DRESS_DOWN, 1),
                ME.pt(Card::AdelineResplendentCathar, 1, 4),
            ]),
        // Made a 2/2 creature by Opalescence, it loses its own abilities with every other
        // creature's, as its ruling has it, the end step trigger among them: it is still there
        // in the next turn.
        Scenario::new("made a creature, it loses its own sacrifice trigger")
            .interaction(&[Card::Opalescence])
            .battlefield(P0, &[Card::Opalescence, DRESS_DOWN])
            .library(P0, &[Card::Island, Card::Island])
            .library(P1, &[Card::Island, Card::Island])
            .actions(&[
                ME.pt(DRESS_DOWN, 2, 2),
                ME.done_attacking(),
                advance_to(2, TurnStep::PrecombatMain),
                ME.controls(DRESS_DOWN, 1),
            ]),
        // Entering as a creature under Opalescence, it has already lost its enters trigger
        // when the event is checked (CR 603.10), so it draws nothing.
        Scenario::new("entering as a creature, it draws nothing")
            .interaction(&[Card::Opalescence])
            .hand(P0, &[DRESS_DOWN])
            .battlefield(P0, &[Card::Opalescence, Card::Island, Card::Island])
            .library(P0, &[Card::Plains, Card::Island])
            .actions(&[
                ME.casts(DRESS_DOWN),
                Resolve,
                stack(0),
                ME.has(Zone::Hand, Card::Plains, 0),
                ME.pt(DRESS_DOWN, 2, 2),
            ]),
    ];
}
