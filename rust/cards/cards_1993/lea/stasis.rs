// Generated from the card store by `just card header`: notes go below the end marker.
//! Stasis {1}{U}
//! Enchantment
//!
//! Players skip their untap steps.
//! At the beginning of your upkeep, sacrifice this enchantment unless you pay {U}.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::Stasis
    .abilities(&[
        Ability::rule_change(&RuleChange {
            players: Players::EachPlayer,
            what: Governed::TakeStep(TurnStep::Untap),
            when: When::Never,
        }),
        Ability::triggered(&Trigger::step(TurnStep::Upkeep, you())).effect(&unless_pays(
            &sacrifice(this()),
            you(),
            mana("{U}"),
        )),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const STASIS: &str = Card::Stasis;
    const TEFERI: &str = Card::TeferiHeroOfDominaria;
    const ISLAND: &str = Card::Island;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("the skipped untap leaves lands tapped but a delayed ability untaps them")
            .interaction(&[TEFERI])
            .hand(P0, &[STASIS])
            .battlefield(P0, &[ISLAND; 3])
            .permanents(
                P1,
                &[
                    Permanent::new(Card::Plains).tapped(),
                    Permanent::new(TEFERI),
                ],
            )
            .library(P1, &[ISLAND; 3])
            .actions(&[
                ME.casts(STASIS),
                Resolve,
                advance_to(2, TurnStep::PrecombatMain),
                THEM.tapped(Card::Plains, 1),
                THEM.activates(TEFERI, 0),
                Resolve,
                advance_to(2, TurnStep::End),
                Resolve,
                THEM.picks_up_to(Card::Plains),
                THEM.done_picking(),
                THEM.tapped(Card::Plains, 0),
                ME.tapped(ISLAND, 2),
                advance_to(3, TurnStep::Upkeep),
                Resolve,
                ME.no(),
                ME.has(Zone::Graveyard, STASIS, 1),
            ]),
        Scenario::new("sacrificing Stasis in upkeep does not restore that turn's untap")
            .at(2, P1, TurnStep::End)
            .permanents(
                P0,
                &[
                    Permanent::new(STASIS),
                    Permanent::new(ISLAND).tapped(),
                    Permanent::new(Card::Plains),
                ],
            )
            .permanents(P1, &[Permanent::new(ISLAND).tapped()])
            .actions(&[
                advance_to(3, TurnStep::Upkeep),
                ME.tapped(ISLAND, 1),
                Resolve,
                ME.has(Zone::Graveyard, STASIS, 1),
                ME.tapped(ISLAND, 1),
                advance_to(4, TurnStep::Upkeep),
                THEM.tapped(ISLAND, 0),
            ]),
        Scenario::new("paying blue keeps Stasis")
            .at(2, P1, TurnStep::End)
            .battlefield(P0, &[STASIS, ISLAND])
            .actions(&[
                advance_to(3, TurnStep::Upkeep),
                Resolve,
                ME.yes(),
                ME.controls(STASIS, 1),
                ME.tapped(ISLAND, 1),
            ]),
        Scenario::new("a skipped untap leaves phased-out permanents phased out")
            .interaction(&[Card::TeferisProtection])
            .at(1, P0, TurnStep::PostcombatMain)
            .hand(P0, &[Card::TeferisProtection])
            .battlefield(P0, &[Card::Plains; 3])
            .battlefield(P1, &[STASIS, ISLAND])
            .actions(&[
                ME.casts(Card::TeferisProtection),
                Resolve,
                advance_to(2, TurnStep::Upkeep),
                Resolve,
                THEM.yes(),
                advance_to(3, TurnStep::Upkeep),
                ME.controls(Card::Plains, 0),
                THEM.controls(STASIS, 1),
            ]),
    ];
}
