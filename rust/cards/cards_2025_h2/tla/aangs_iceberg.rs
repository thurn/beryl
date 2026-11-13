// Generated from the card store by `just card header`: notes go below the end marker.
//! Aang's Iceberg {2}{W}
//! Enchantment
//!
//! Flash
//! When this enchantment enters, exile up to one other target nonland permanent until this
//! enchantment leaves the battlefield.
//! Waterbend {3}: Sacrifice this enchantment. If you do, scry 2. (While paying a waterbend cost,
//! you can tap your artifacts and creatures to help. Each one pays for {1}.)
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::AangsIceberg
    .keywords(Keywords::FLASH)
    .abilities(&[
        // "Exile up to one other target nonland permanent until this enchantment leaves
        // the battlefield" ([CR 610.3](https://yawgatog.com/resources/magic-rules/#R6103)).
        // If the Iceberg has already left by the time this resolves, the permanent isn't
        // exiled ([CR 610.3b](https://yawgatog.com/resources/magic-rules/#R6103b)).
        Ability::triggered(&Trigger::ENTERS)
            .target(
                &TargetSpec::permanent(Filter::Not(&Filter::Types(Types::LAND)))
                    .optional()
                    .other(),
            )
            .effect(&exile_until_this_leaves(target())),
        // "Sacrifice this enchantment. If you do, scry 2." Its controller can sacrifice
        // it only if it is still on the battlefield
        // ([CR 701.21a](https://yawgatog.com/resources/magic-rules/#R70121a)).
        Ability::activated(Cost::of(ManaCost::generic(3), &[CostPart::waterbend(3)])).effect(
            &sequence(&[sacrifice(this()), if_then(if_you_do(), &scry(you(), 2))]),
        ),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;
    use crate::tokens::TREASURE_TOKEN;

    const ICEBERG: &str = Card::AangsIceberg;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("exiles a permanent until it leaves; waterbend taps a creature to pay")
            .at(2, P1, TurnStep::End)
            .hand(P0, &[ICEBERG])
            .battlefield(
                P0,
                &[Card::Plains, Card::Plains, Card::Plains, Prop::Creature],
            )
            .permanents(
                P1,
                &[Permanent::new(Prop::Planeswalker).counters(CounterKind::Loyalty, 5)],
            )
            .library(
                P0,
                &[Card::Island, Prop::Artifact, Card::Plains, Card::Island],
            )
            .library(P1, &[Card::Plains])
            .actions(&[
                // Flash: at the end of the opponent's turn, exiling their planeswalker rather
                // than the Iceberg's own controller's creature.
                ME.casts(ICEBERG),
                Resolve,
                ME.target_count(1),
                ME.targets(Prop::Planeswalker),
                Resolve,
                THEM.has(Zone::Exile, Prop::Planeswalker, 1),
                // Its controller's turn: three Plains untapped and a creature, for waterbend {3}.
                advance_to(3, TurnStep::PrecombatMain),
                ME.activates(ICEBERG, 1), // Waterbend {3}
                // CR 701.67a: the creature taps for one generic mana; two Plains pay the rest.
                ME.chooses(DecisionKind::ManaPayment, Prop::Creature),
                ME.tapped(Card::Plains, 2),
                ME.tapped(Prop::Creature, 1),
                // Scry 2 over the artifact and Plains: the artifact to the bottom, Plains stays
                // on top.
                ME.picks_up_to(Prop::Artifact),
                ME.done_picking(),
                ME.library_top(&[Card::Plains, Card::Island, Prop::Artifact]),
                // Sacrificed, so the planeswalker returns under its owner's control
                // (CR 610.3c): a new object, with its printed loyalty.
                ME.has(Zone::Graveyard, ICEBERG, 1),
                THEM.has_loyalty(Prop::Planeswalker, 3),
            ]),
        Scenario::new("waterbend taps a creature and an artifact; scry reorders the top")
            .battlefield(P0, &[ICEBERG, Card::Plains, Prop::Creature, Prop::Artifact])
            .library(P0, &[Card::Island, Card::Plains])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.activates(ICEBERG, 1), // Waterbend {3}
                // One Plains can't pay {3} alone: no paying the rest with mana yet.
                ME.deciding(DecisionKind::ManaPayment, 2),
                ME.chooses(DecisionKind::ManaPayment, Prop::Artifact),
                // Then the creature is the only way left to pay one more, and the Plains pays
                // the last.
                ME.tapped(Prop::Creature, 1),
                ME.tapped(Card::Plains, 1),
                // Nothing to the bottom; the Plains on top of the Island.
                ME.done_picking(),
                ME.picks(Card::Plains),
                ME.library_top(&[Card::Plains, Card::Island]),
            ]),
        Scenario::new("waterbend uses mana sources without sacrificing them")
            .interaction(&[
                Card::ManaCrypt,
                Card::JaceVrynsProdigy,
                Card::CurrencyConverter,
            ])
            .battlefield(
                P0,
                &[
                    ICEBERG,
                    Card::Plains,
                    Card::ManaCrypt,
                    Card::JaceVrynsProdigy,
                    Card::CurrencyConverter,
                ],
            )
            .hand(P0, &[Card::Plains])
            .library(P0, &[Card::Island, Card::Plains, Card::Island])
            .library(P1, &[Card::Plains])
            .actions(&[
                // Jace and the Converter create the Treasure while tapping themselves.
                ME.activates(Card::JaceVrynsProdigy, 0), // {T}: draw, then discard
                Resolve,
                ME.picks(Card::Plains),
                Resolve,
                ME.yes(),
                ME.activates(Card::CurrencyConverter, 2), // {T}: put an exiled card into a graveyard
                Resolve,
                ME.activates(ICEBERG, 1), // Waterbend {3}
                // A Treasure tapped to waterbend does not activate its own mana ability or
                // sacrifice itself (CR 701.67a and CR 111.10a).
                ME.chooses(DecisionKind::ManaPayment, TREASURE_TOKEN),
                ME.chooses(DecisionKind::ManaPayment, Card::ManaCrypt),
                ME.picks_up_to(Card::Island),
                ME.done_picking(),
                ME.tapped(TREASURE_TOKEN, 1),
                ME.tapped(Card::ManaCrypt, 1),
            ]),
    ];
}
