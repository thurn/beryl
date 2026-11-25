// Generated from the card store by `just card header`: notes go below the end marker.
//! Humility {2}{W}{W}
//! Enchantment
//!
//! All creatures lose all abilities and have base power and toughness 1/1.
// End of the generated card header.

use crate::prelude::*;

/// If Humility becomes a creature, it removes its own ability in layer 6. The same
/// effect still sets base power and toughness in layer 7b ([CR 613.6]): "it will continue
/// to be applied to the same set of objects in each other applicable layer and/or sublayer,
/// even if the ability generating the effect is removed during this process."
///
/// [CR 613.6]: https://yawgatog.com/resources/magic-rules/#R6136
pub static CARD: CardDef = Printed::Humility
    .abilities(&[Ability::static_ability(&LayerSpec {
        affected: permanents(creature()),
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

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("every creature is 1/1, on both sides")
            .interaction(&[Card::DrudgeSkeletons])
            .battlefield(P0, &[HUMILITY, Prop::Flier])
            .battlefield(P1, &[Card::DrudgeSkeletons])
            .actions(&[
                ME.pt(Prop::Flier, 1, 1),
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
        // Elite Spellbinder enters with no abilities, so its enters trigger, which would look
        // at the opponent's hand and exile a card from it, never triggers.
        Scenario::new("a creature entering under it triggers nothing")
            .interaction(&[Card::EliteSpellbinder])
            .hand(P0, &[Card::EliteSpellbinder])
            .battlefield(P0, &[HUMILITY, Card::Plains, Card::Plains, Card::Plains])
            .hand(P1, &[Prop::Artifact, Card::Island])
            .actions(&[
                ME.casts(Card::EliteSpellbinder),
                Resolve,
                stack(0),
                ME.pt(Card::EliteSpellbinder, 1, 1),
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
        // Dryad Arbor's {G} comes from its Forest type, not its text (CR 305.6), and it goes
        // with the creature's other abilities in layer 6. With the Plains alone P0 can't pay
        // for Gold Myr, and is given no priority in the main phase.
        Scenario::new("Dryad Arbor loses the mana ability of its land type")
            .interaction(&[Card::GoldMyr])
            .hand(P0, &[Card::GoldMyr])
            .battlefield(P0, &[HUMILITY, Card::DryadArbor, Card::Plains])
            .actions(&[
                ME.pt(Card::DryadArbor, 1, 1),
                ME.deciding(DecisionKind::Attacker, 2),
            ]),
        // Glorious Anthem is no creature and keeps its ability; its +1/+1 applies after the
        // base 1/1.
        Scenario::new("an anthem's bonus applies on top of the base 1/1")
            .interaction(&[Card::GloriousAnthem])
            .battlefield(P0, &[HUMILITY, Card::GloriousAnthem, Prop::Flier])
            .actions(&[ME.pt(Prop::Flier, 2, 2)]),
        // The Wandering Emperor's +1 puts a +1/+1 counter on the creature, the one target,
        // which counts on top of the base 1/1.
        Scenario::new("a +1/+1 counter counts on top of the base 1/1")
            .interaction(&[Card::TheWanderingEmperor])
            .battlefield(P0, &[HUMILITY, Card::TheWanderingEmperor, Prop::Vanilla])
            .actions(&[
                ME.activates(Card::TheWanderingEmperor, 1),
                ME.target_count(1),
                ME.counters(Prop::Vanilla, CounterKind::PlusOne, 1),
                ME.pt(Prop::Vanilla, 2, 2),
            ]),
        // Tarmogoyf's characteristic-defining ability is lost in layer 6, before layer 7a would
        // apply it, so base 1/1 is all it has however full the graveyards (CR 613.1f).
        Scenario::new("Tarmogoyf under it is 1/1")
            .interaction(&[Card::Tarmogoyf])
            .battlefield(P0, &[HUMILITY, Card::Tarmogoyf])
            .graveyard(P0, &[Prop::Flier, Prop::Sorcery, Prop::Instant])
            .actions(&[ME.pt(Card::Tarmogoyf, 1, 1)]),
    ];
}
