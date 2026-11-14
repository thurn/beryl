// Generated from the card store by `just card header`: notes go below the end marker.
//! Humility {2}{W}{W}
//! Enchantment
//!
//! All creatures lose all abilities and have base power and toughness 1/1.
// End of the generated card header.

use crate::prelude::*;

/// "All creatures lose all abilities and have base power and toughness 1/1": one static
/// ability whose effect has a part in layer 6 and a part in layer 7b ([CR 613.6]): "If an
/// effect should be applied in different layers and/or sublayers, the parts of the effect
/// each apply in their appropriate ones. If an effect starts to apply in one layer and/or
/// sublayer, it will continue to be applied to the same set of objects in each other applicable layer
/// and/or sublayer, even if the ability generating the effect is removed during this
/// process." So a Humility that has itself become a creature still makes the creatures 1/1
/// after it has lost its own ability in layer 6.
///
/// Losing all abilities takes mana abilities, triggered abilities and keywords with the
/// rest ([CR 613.1f]): "Layer 6: Ability-adding effects, keyword counters, ability-removing
/// effects, and effects that say an object can't have an ability are applied." An effect
/// that grants an ability later in timestamp order still grants it. Counters and effects
/// that modify power and toughness apply after the base 1/1 ([CR 613.4c]): "Effects and
/// counters that modify power and/or toughness (but don't set power and/or toughness to a
/// specific number or value) are applied."
///
/// [CR 613.6]: https://yawgatog.com/resources/magic-rules/#R6136
/// [CR 613.1f]: https://yawgatog.com/resources/magic-rules/#R6131f
/// [CR 613.4c]: https://yawgatog.com/resources/magic-rules/#R6134c
pub static CARD: CardDef = Printed::Humility
    .abilities(&[Ability::static_ability(&LayerSpec {
        affected: Objects::All {
            zone: Zone::Battlefield,
            filter: Filter::Types(Types::CREATURE),
        },
        condition: None,
        modifications: &[
            Modification::LoseAllAbilities,
            Modification::SetBasePt(&Amount::Fixed(1), &Amount::Fixed(1)),
        ],
    })])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const HUMILITY: &str = Card::Humility;

    // `Prop::Creature` is Elite Spellbinder, a 3/1 flier whose enters trigger looks at an
    // opponent's hand; `Prop::Artifact` is Portable Hole, a {W} artifact.

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("every creature is 1/1, on both sides")
            .interaction(&[Card::DrudgeSkeletons])
            .battlefield(P0, &[HUMILITY, Prop::Creature])
            .battlefield(P1, &[Card::DrudgeSkeletons])
            .actions(&[
                ME.pt(Prop::Creature, 1, 1),
                THEM.pt(Card::DrudgeSkeletons, 1, 1),
            ]),
        // Adeline's power, defined by her own ability, and her vigilance and attack trigger
        // are all gone: she attacks as a 1/1 that taps, and no Human arrives.
        Scenario::new("Adeline loses her power, vigilance and attack trigger")
            .interaction(&[Card::AdelineResplendentCathar])
            .battlefield(P0, &[HUMILITY, Card::AdelineResplendentCathar])
            .actions(&[
                ME.pt(Card::AdelineResplendentCathar, 1, 1),
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(Card::AdelineResplendentCathar),
                stack(0),
                ME.tapped(Card::AdelineResplendentCathar, 1),
                advance_to(1, TurnStep::EndOfCombat),
                ME.controls(Card::AdelineResplendentCathar, 1),
                THEM.life(19),
            ]),
        // The Spellbinder enters with no abilities, so its enters trigger never triggers and
        // the opponent's artifact stays in their hand.
        Scenario::new("a creature entering under it triggers nothing")
            .hand(P0, &[Prop::Creature])
            .battlefield(P0, &[HUMILITY, Card::Plains, Card::Plains, Card::Plains])
            .hand(P1, &[Prop::Artifact, Card::Island])
            .actions(&[
                ME.casts(Prop::Creature),
                Resolve,
                stack(0),
                ME.pt(Prop::Creature, 1, 1),
                THEM.has(Zone::Hand, Prop::Artifact, 1),
                THEM.has_cards(Zone::Exile, 0),
            ]),
        // The Skeletons' regeneration can't be activated: with nothing to do but pass, P0 is
        // given no priority in the main phase, and the first decision is the attack.
        Scenario::new("an activated ability can't be activated")
            .interaction(&[Card::DrudgeSkeletons])
            .battlefield(P0, &[HUMILITY, Card::Swamp, Card::DrudgeSkeletons])
            .actions(&[ME.deciding(DecisionKind::Attacker, 2)]),
        // Gold Myr's mana ability is gone too, so it can't pay for the {W} artifact, and P0
        // is given no priority in the main phase.
        Scenario::new("a mana creature makes no mana")
            .interaction(&[Card::GoldMyr])
            .hand(P0, &[Prop::Artifact])
            .battlefield(P0, &[HUMILITY, Card::GoldMyr])
            .actions(&[ME.deciding(DecisionKind::Attacker, 2)]),
        // Glorious Anthem is no creature and keeps its ability; its +1/+1 applies after the
        // base 1/1.
        Scenario::new("an anthem's bonus applies on top of the base 1/1")
            .interaction(&[Card::GloriousAnthem])
            .battlefield(P0, &[HUMILITY, Card::GloriousAnthem, Prop::Creature])
            .actions(&[ME.pt(Prop::Creature, 2, 2)]),
        // The Wandering Emperor's +1 puts a +1/+1 counter on the creature, the one target,
        // which counts on top of the base 1/1.
        Scenario::new("a +1/+1 counter counts on top of the base 1/1")
            .interaction(&[Card::TheWanderingEmperor])
            .battlefield(
                P0,
                &[HUMILITY, Card::TheWanderingEmperor, Prop::SmallCreature],
            )
            .actions(&[
                ME.activates(Card::TheWanderingEmperor, 1),
                ME.target_count(1),
                ME.counters(Prop::SmallCreature, CounterKind::PlusOne, 1),
                ME.pt(Prop::SmallCreature, 2, 2),
            ]),
        // Once Humility, the one target, returns to its owner's hand, the Spellbinder is a 3/1
        // again.
        Scenario::new("with it gone, the creatures are themselves again")
            .battlefield(P0, &[HUMILITY])
            .battlefield(
                P1,
                &[Prop::Creature, Card::Island, Card::Island, Card::Island],
            )
            .hand(P1, &[Prop::Removal])
            .actions(&[
                THEM.pt(Prop::Creature, 1, 1),
                THEM.casts(Prop::Removal),
                ME.has(Zone::Hand, HUMILITY, 1),
                THEM.pt(Prop::Creature, 3, 1),
            ]),
    ];
}
