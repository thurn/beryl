// Generated from the card store by `just card header`: notes go below the end marker.
//! Agatha's Soul Cauldron {2}
//! Legendary Artifact
//!
//! You may spend mana as though it were mana of any color to activate abilities of creatures you
//! control.
//! Creatures you control with +1/+1 counters on them have all activated abilities of all creature
//! cards exiled with Agatha's Soul Cauldron.
//! {T}: Exile target card from a graveyard. When a creature card is exiled this way, put a +1/+1
//! counter on target creature you control.
// End of the generated card header.

use crate::prelude::*;

static SPEND_AS_ANY_COLOR: RuleChange = RuleChange {
    players: Players::You,
    what: Governed::SpendAsAnyColor(creature_you_control()),
    when: When::AsUsual,
};

static CREATURE_WITH_COUNTER: Filter = Filter::All(&[
    Filter::Types(Types::CREATURE),
    Filter::Whose(&Players::You),
    Filter::WithCounter(CounterKind::PlusOne),
]);

static LENT: Lent = Lent {
    from: LentFrom::ExiledWithThis(Filter::Types(Types::CREATURE)),
    which: Lends::Activated,
};

static LEND_ABILITIES: LayerSpec = LayerSpec {
    affected: permanents(CREATURE_WITH_COUNTER),
    condition: None,
    modifications: &[Modification::Lend(&LENT)],
};

static EXILE_AND_REFLECT: Effect = sequence(&[
    exile_linked(target()),
    reflexive(Condition::Is(those(), Filter::Types(Types::CREATURE)), 3),
]);

pub static CARD: CardDef = Printed::AgathasSoulCauldron
    .abilities(&[
        Ability::rule_change(&SPEND_AS_ANY_COLOR),
        Ability::static_ability(&LEND_ABILITIES),
        Ability::activated(Cost::TAP)
            .target(&TargetSpec::one(TargetKind::GraveyardCard, Filter::Any))
            .effect(&EXILE_AND_REFLECT),
        Ability::triggered(&Trigger::REFLEXIVE)
            .target(&TargetSpec::permanent(creature_you_control()))
            .effect(&add_counters(target(), CounterKind::PlusOne, 1)),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const CAULDRON: &str = Card::AgathasSoulCauldron;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new(
            "A creature you control with a +1/+1 counter gains exiled activated abilities",
        )
        .interaction(&[Card::LilianasShade])
        .battlefield(P0, &[CAULDRON, Prop::Vanilla, Card::Swamp])
        .graveyard(P1, &[Card::LilianasShade])
        .actions(&[
            ME.activates(CAULDRON, 2),
            Resolve,
            Resolve,
            ME.activates_lent(Prop::Vanilla, 0, 0),
            Resolve,
            ME.pt(Prop::Vanilla, 4, 3),
        ]),
        Scenario::new(
            "A creature you control without a +1/+1 counter does not gain exiled abilities",
        )
        .interaction(&[Card::LilianasShade])
        .battlefield(P0, &[CAULDRON, Prop::Flier, Prop::Vanilla, Card::Swamp])
        .graveyard(P1, &[Card::LilianasShade])
        .actions(&[
            ME.activates(CAULDRON, 2),
            Resolve,
            ME.deciding(DecisionKind::Target, 2),
            ME.targets(Prop::Flier),
            Resolve,
            ME.deciding(DecisionKind::Priority, 2),
        ]),
        Scenario::new("An opponent's creature with a +1/+1 counter does not gain exiled abilities")
            .interaction(&[Card::LilianasShade])
            .permanents(
                P0,
                &[
                    Permanent::new(CAULDRON),
                    Permanent::new(Prop::Vanilla).tapped(),
                ],
            )
            .permanents(
                P1,
                &[
                    Permanent::new(Prop::Flier).counters(CounterKind::PlusOne, 1),
                    Permanent::new(Card::Swamp),
                ],
            )
            .graveyard(P1, &[Card::LilianasShade])
            .actions(&[
                ME.activates(CAULDRON, 2),
                Resolve,
                Resolve,
                advance_to(2, TurnStep::PrecombatMain),
                THEM.deciding(DecisionKind::Priority, 2),
            ]),
        Scenario::new(
            "You may spend mana of any color to activate abilities of creatures you control",
        )
        .interaction(&[Card::DrudgeSkeletons])
        .battlefield(P0, &[CAULDRON, Card::DrudgeSkeletons, Card::Plains])
        .actions(&[
            ME.activates(Card::DrudgeSkeletons, 0),
            Resolve,
            ME.tapped(Card::Plains, 1),
        ]),
        // Under an opponent's Mana Flare no land is a source the cached funds count, so the
        // reference check for abilities once compared the plain funds, which saw only the
        // pool, and found that they disagreed with the reference about the Plains paying {B}.
        Scenario::new("Lands under an opponent's Mana Flare pay a creature's ability")
            .interaction(&[Card::DrudgeSkeletons, Card::ManaFlare])
            .battlefield(P0, &[CAULDRON, Card::DrudgeSkeletons, Card::Plains])
            .battlefield(P1, &[Card::ManaFlare])
            .actions(&[
                ME.activates(Card::DrudgeSkeletons, 0),
                Resolve,
                ME.tapped(Card::Plains, 1),
            ]),
        Scenario::new("An opponent cannot spend mana of any color for their creatures' abilities")
            .interaction(&[Card::DrudgeSkeletons])
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[CAULDRON])
            .battlefield(P1, &[Card::DrudgeSkeletons, Card::Plains])
            .hand(P1, &[Card::Island])
            .actions(&[THEM.deciding(DecisionKind::Priority, 2)]),
        Scenario::new("Noncreature permanents do not get the mana-spending rule")
            .interaction(&[Card::StaffOfTheStoryteller])
            .permanents(
                P0,
                &[
                    Permanent::new(CAULDRON),
                    Permanent::new(Card::StaffOfTheStoryteller).counters(CounterKind::Story, 1),
                    Permanent::new(Card::Island),
                ],
            )
            .hand(P0, &[Card::Island])
            .actions(&[ME.deciding(DecisionKind::Priority, 2)]),
    ];
}
