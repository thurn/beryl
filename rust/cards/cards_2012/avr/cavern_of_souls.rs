// Generated from the card store by `just card header`: notes go below the end marker.
//! Cavern of Souls
//! Land
//!
//! As this land enters, choose a creature type.
//! {T}: Add {C}.
//! {T}: Add one mana of any color. Spend this mana only to cast a creature spell of the chosen
//! type, and that spell can't be countered.
// End of the generated card header.

use crate::prelude::*;

/// "That spell can't be countered": a rule change the mana starts on the spell it is spent on
/// ([CR 106.6]), asked of the spell whoever would counter it.
///
/// [CR 106.6]: https://yawgatog.com/resources/magic-rules/#R1066
static CANT_BE_COUNTERED: RuleChange = RuleChange {
    players: Players::EachPlayer,
    what: Governed::Counter(Objects::This),
    when: When::Never,
};

/// "Spend this mana only to cast a creature spell of the chosen type, and that spell can't be
/// countered."
static CREATURE_OF_THE_CHOSEN_TYPE: ManaRestriction = ManaRestriction {
    cast: Filter::Types(Types::CREATURE),
    of_chosen_type: true,
    rider: Some(&CANT_BE_COUNTERED),
};

/// The creature type is chosen as the land enters, before it is on the battlefield
/// ([CR 614.12a]): "If a replacement effect that modifies how a permanent enters the battlefield
/// requires a choice, that choice is made before the permanent enters the battlefield." It must
/// be a creature type (CR 205.3m); one that no card of the game has stands for every other.
///
/// Its colored mana remembers the chosen type and pays only for a creature spell that has it
/// ([CR 106.6]): "Some spells or abilities that produce mana restrict how that mana can be
/// spent, have an additional effect that affects the spell or ability that mana is spent on".
/// Spent on any cost of that spell, a kicker cost among them, it makes the spell one that can't
/// be countered: a counterspell may still target it, and does nothing to it ([CR 101.2]): "When
/// a rule or effect allows or directs something to happen, and another effect states that it
/// can't happen, the 'can't' effect takes precedence."
///
/// [CR 614.12a]: https://yawgatog.com/resources/magic-rules/#R61412a
/// [CR 106.6]: https://yawgatog.com/resources/magic-rules/#R1066
/// [CR 101.2]: https://yawgatog.com/resources/magic-rules/#R1012
pub static CARD: CardDef = Printed::CavernOfSouls
    .abilities(&[
        Ability::replacing(&Replacement::choose_as_enters(ChosenKind::CreatureType)),
        Ability::tap_for_mana(ManaSet::one(ManaColor::Colorless)),
        Ability::tap_for_mana(ManaSet {
            colors: 0b1_1111,
            amount: 1,
        })
        .spent_only(&CREATURE_OF_THE_CHOSEN_TYPE),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    /// Snapcaster Mage is a Human Wizard, {1}{U}: the creature spell of the chosen type. Its
    /// enters trigger finds no instant in its controller's graveyard, and is removed.
    pub const SCENARIOS: &[Scenario] = &[
        // The Plains can't make {U}, so the Cavern's colored mana pays it.
        Scenario::new("Cavern of Souls' creature spell can't be countered")
            .battlefield(P0, &[Card::Plains])
            .hand(P0, &[Card::CavernOfSouls, Prop::SmallCreature])
            .battlefield(P1, &[Card::Island, Card::Island])
            .hand(P1, &[Card::Counterspell])
            .actions(&[
                ME.plays(Card::CavernOfSouls),
                ME.chooses_named("Human"),
                ME.casts(Prop::SmallCreature),
                ME.tapped(Card::CavernOfSouls, 1),
                THEM.casts(Card::Counterspell),
                Resolve,
                THEM.has(Zone::Graveyard, Card::Counterspell, 1),
                ME.controls(Prop::SmallCreature, 1),
            ]),
        // Paid with the Cavern's {C} and an Island's {U}, the spell is countered: a payment
        // that spends the colored mana and one that doesn't are both offered, the colored first.
        Scenario::new("Cavern of Souls' colorless mana gives no protection")
            .battlefield(P0, &[Card::Island])
            .hand(P0, &[Card::CavernOfSouls, Prop::SmallCreature])
            .battlefield(P1, &[Card::Island, Card::Island])
            .hand(P1, &[Card::Counterspell])
            .actions(&[
                ME.plays(Card::CavernOfSouls),
                ME.chooses_named("Human"),
                ME.casts(Prop::SmallCreature),
                ME.deciding(DecisionKind::PaymentPlan, 2),
                ME.answers(DecisionKind::PaymentPlan, Pick::Plan { i: 1 }),
                ME.tapped(Card::Island, 1),
                THEM.casts(Card::Counterspell),
                ME.has(Zone::Graveyard, Prop::SmallCreature, 1),
            ]),
        // Of another creature type, the colored mana pays for nothing here: the Cavern's {C}
        // and the Plains can't pay Snapcaster Mage's {U}, and the colorless mana pays for the
        // artifact's generic cost.
        Scenario::new("Cavern of Souls' colored mana pays only for the chosen type")
            .battlefield(P0, &[Card::Plains])
            .hand(
                P0,
                &[Card::CavernOfSouls, Prop::SmallCreature, Prop::Artifact],
            )
            .actions(&[
                ME.plays(Card::CavernOfSouls),
                ME.chooses_named("Another creature type"),
                ME.deciding(DecisionKind::Priority, 2),
                ME.casts(Prop::Artifact),
                ME.tapped(Card::Plains, 1),
                ME.tapped(Card::CavernOfSouls, 0),
            ]),
        // A noncreature spell: the Island makes {U}, and the Cavern's colored mana can't make
        // the artifact's {W}, so only the creature can be cast.
        Scenario::new("Cavern of Souls' colored mana pays for no noncreature spell")
            .battlefield(P0, &[Card::Island])
            .hand(
                P0,
                &[Card::CavernOfSouls, Prop::SmallCreature, Prop::Artifact],
            )
            .actions(&[
                ME.plays(Card::CavernOfSouls),
                ME.chooses_named("Human"),
                ME.deciding(DecisionKind::Priority, 2),
                ME.casts(Prop::SmallCreature),
                ME.answers(DecisionKind::PaymentPlan, Pick::Plan { i: 0 }),
                ME.tapped(Card::Island, 1),
                ME.tapped(Card::CavernOfSouls, 1),
            ]),
    ];
}
