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

/// Flash, and "When this enchantment enters, draw a card."
///
/// "Creatures lose all abilities.": a static ability in layer 6 over every creature
/// ([CR 613.1f]): "Layer 6: Ability-adding effects, keyword counters, ability-removing
/// effects, and effects that say an object can't have an ability are applied." It removes
/// what the creatures have when it applies, and an effect that grants an ability later in
/// timestamp order still grants it. Their power and toughness stay, but a power that an
/// ability of the creature defined is lost with that ability.
///
/// "At the beginning of the end step, sacrifice this enchantment.": every player's end step
/// ([CR 603.2b]): "When a phase or step begins, all abilities that trigger 'at the beginning
/// of' that phase or step trigger."
///
/// [CR 613.1f]: https://yawgatog.com/resources/magic-rules/#R6131f
/// [CR 603.2b]: https://yawgatog.com/resources/magic-rules/#R6032b
pub static CARD: CardDef = Printed::DressDown
    .keywords(Keywords::FLASH)
    .abilities(&[
        Ability::triggered(&Trigger::ENTERS).effect(&draw(you(), 1)),
        Ability::static_ability(&LayerSpec {
            affected: Objects::All {
                zone: Zone::Battlefield,
                filter: Filter::Types(Types::CREATURE),
            },
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
        Scenario::new("it enters and draws a card")
            .hand(P0, &[DRESS_DOWN])
            .battlefield(P0, &[Card::Island, Card::Island])
            .library(P0, &[Card::Plains, Card::Island])
            .actions(&[
                ME.casts(DRESS_DOWN),
                Resolve,
                stack(1),
                Resolve,
                ME.has(Zone::Hand, Card::Plains, 1),
                ME.controls(DRESS_DOWN, 1),
            ]),
        // Adeline's power was defined by her own ability, which is gone: her printed power
        // is *, which is 0. The Spellbinder keeps its printed 3/1.
        Scenario::new("creatures keep printed power but lose defined power")
            .interaction(&[Card::AdelineResplendentCathar])
            .battlefield(
                P0,
                &[DRESS_DOWN, Card::AdelineResplendentCathar, Prop::Creature],
            )
            .actions(&[
                ME.pt(Card::AdelineResplendentCathar, 0, 4),
                ME.pt(Prop::Creature, 3, 1),
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
        // Flash: cast in the opponent's turn, it draws, and is sacrificed at that turn's end
        // step.
        Scenario::new("cast on the opponent's turn, it goes at that end step")
            .hand(P0, &[DRESS_DOWN])
            .battlefield(P0, &[Card::Island, Card::Island])
            .library(P0, &[Card::Plains, Card::Island, Card::Island])
            .library(P1, &[Card::Island, Card::Island])
            .actions(&[
                advance_to(2, TurnStep::PrecombatMain),
                THEM.passes(),
                ME.casts(DRESS_DOWN),
                Resolve,
                ME.controls(DRESS_DOWN, 1),
                advance_to(2, TurnStep::End),
                ME.has(Zone::Hand, Card::Plains, 1),
                stack(1),
                Resolve,
                ME.controls(DRESS_DOWN, 0),
            ]),
    ];
}
