// Generated from the card store by `just card header`: notes go below the end marker.
//! Student of Warfare {W}
//! Creature — Human Knight, 1/1
//!
//! Level up {W} ({W}: Put a level counter on this. Level up only as a sorcery.)
//! LEVEL 2-6
//! 3/3
//! First strike
//! LEVEL 7+
//! 4/4
//! Double strike
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::StudentOfWarfare
    .abilities(&[
        Ability::level_up(Cost::mana(mana("{W}"))),
        Ability::static_ability(&LayerSpec::leveled::<2, 6>(&[
            Modification::SetBasePt(&Amount::Fixed(3), &Amount::Fixed(3)),
            Modification::AddKeywords(Keywords::FIRST_STRIKE),
        ])),
        Ability::static_ability(&LayerSpec::leveled_from::<7>(&[
            Modification::SetBasePt(&Amount::Fixed(4), &Amount::Fixed(4)),
            Modification::AddKeywords(Keywords::DOUBLE_STRIKE),
        ])),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const STUDENT: &str = Card::StudentOfWarfare;
    const PLAINS: &str = Card::Plains;
    const BLOCKER: &str = Prop::FlashCreature;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("One level leaves Student of Warfare a 1/1")
            .battlefield(P0, &[STUDENT, PLAINS])
            .actions(&[
                ME.activates(STUDENT, 0),
                ME.tapped(PLAINS, 1),
                Resolve,
                ME.counters(STUDENT, CounterKind::Level, 1),
                ME.pt(STUDENT, 1, 1),
            ]),
        // The 3/2 blocker deals no damage: first strike kills it first.
        Scenario::new("Two levels make Student of Warfare a 3/3 with first strike")
            .battlefield(P0, &[STUDENT, PLAINS, PLAINS])
            .battlefield(P1, &[BLOCKER])
            .actions(&[
                ME.activates(STUDENT, 0),
                Resolve,
                ME.activates(STUDENT, 0),
                Resolve,
                ME.counters(STUDENT, CounterKind::Level, 2),
                ME.pt(STUDENT, 3, 3),
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(STUDENT),
                THEM.chooses(DecisionKind::Blocker, BLOCKER),
                advance_to(1, TurnStep::EndOfCombat),
                THEM.has(Zone::Graveyard, BLOCKER, 1),
                ME.controls(STUDENT, 1),
            ]),
        // Its level is the number of level counters, however they were put on it.
        Scenario::new("Seven levels make Student of Warfare a 4/4 with double strike")
            .permanents(
                P0,
                &[Permanent::new(STUDENT).counters(CounterKind::Level, 6)],
            )
            .battlefield(P0, &[PLAINS])
            .actions(&[
                ME.pt(STUDENT, 3, 3),
                ME.activates(STUDENT, 0),
                Resolve,
                ME.pt(STUDENT, 4, 4),
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(STUDENT),
                advance_to(1, TurnStep::EndOfCombat),
                THEM.life(12),
            ]),
        // Losing its level counters takes its level away, however it got them.
        Scenario::new("Vampire Hexmage takes Student of Warfare back to level 0")
            .interaction(&[Card::VampireHexmage])
            .permanents(
                P0,
                &[Permanent::new(STUDENT).counters(CounterKind::Level, 7)],
            )
            .battlefield(P0, &[Card::VampireHexmage])
            .actions(&[
                ME.pt(STUDENT, 4, 4),
                ME.activates(Card::VampireHexmage, 0),
                ME.targets(STUDENT),
                Resolve,
                ME.counters(STUDENT, CounterKind::Level, 0),
                ME.pt(STUDENT, 1, 1),
            ]),
        Scenario::new("Student of Warfare cannot level up in upkeep")
            .at(3, P0, TurnStep::Upkeep)
            .battlefield(P0, &[STUDENT, PLAINS])
            .actions(&[
                advance_to(3, TurnStep::Upkeep),
                ME.deciding(DecisionKind::Priority, 1),
            ]),
        // Level 2 sets base 3/3 in layer 7b; the counter and the Anthem then add in 7c.
        Scenario::new("A +1/+1 counter and an Anthem apply on top of a level's 3/3")
            .interaction(&[Card::GloriousAnthem])
            .permanents(
                P0,
                &[Permanent::new(STUDENT).counters(CounterKind::PlusOne, 1)],
            )
            .battlefield(P0, &[Card::GloriousAnthem, PLAINS, PLAINS])
            .actions(&[
                ME.pt(STUDENT, 3, 3),
                ME.activates(STUDENT, 0),
                Resolve,
                ME.activates(STUDENT, 0),
                Resolve,
                ME.pt(STUDENT, 5, 5),
            ]),
        // A level symbol's effect has the Student's timestamp, so Diminish, later, wins
        // even after another level counter.
        Scenario::new("Diminish overrides a level's base power and toughness")
            .interaction(&[Card::Diminish])
            .permanents(
                P0,
                &[Permanent::new(STUDENT).counters(CounterKind::Level, 2)],
            )
            .battlefield(P0, &[Card::Island, PLAINS])
            .hand(P0, &[Card::Diminish])
            .actions(&[
                ME.pt(STUDENT, 3, 3),
                ME.casts(Card::Diminish),
                Resolve,
                ME.pt(STUDENT, 1, 1),
                ME.activates(STUDENT, 0),
                Resolve,
                ME.counters(STUDENT, CounterKind::Level, 3),
                ME.pt(STUDENT, 1, 1),
            ]),
        // The copy has the level symbols but none of the counters.
        Scenario::new("A copy of a leveled Student of Warfare is a 1/1")
            .interaction(&[Card::CleverImpersonator])
            .permanents(
                P0,
                &[Permanent::new(STUDENT).counters(CounterKind::Level, 2)],
            )
            .battlefield(P1, &[Card::Island; 4])
            .hand(P1, &[Card::CleverImpersonator])
            .at(2, P1, TurnStep::PrecombatMain)
            .actions(&[
                THEM.casts(Card::CleverImpersonator),
                Resolve,
                THEM.picks_up_to(STUDENT),
                THEM.controls(STUDENT, 1),
                THEM.counters(STUDENT, CounterKind::Level, 0),
                THEM.pt(STUDENT, 1, 1),
                ME.pt(STUDENT, 3, 3),
            ]),
    ];
}
