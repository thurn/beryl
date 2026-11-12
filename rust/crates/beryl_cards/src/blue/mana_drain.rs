//! Mana Drain. {U}{U} Instant. "Counter target spell. At the beginning of your next main phase,
//! add an amount of {C} equal to that spell's mana value."

use crate::names;
use beryl_card::effect::build::{
    CARRIED, TARGET, YOU, add_mana_of_kind, counter, delay_carrying, sequence,
};
use beryl_card::{
    Ability, Amount, CardDef, Colors, Effect, Face, Filter, ManaColor, Players, TargetKind,
    TargetSpec, Trigger, Types, card, mana,
};

/// The place of the delayed ability among the face's abilities.
const ADD_COLORLESS: u8 = 1;

pub static CARD: CardDef = card(
    names::MANA_DRAIN,
    &[Face::EMPTY
        .cost(mana("{U}{U}"))
        .types(Types::INSTANT)
        .colors(Colors::BLUE)
        .abilities(&[
            Ability::spell()
                .targeting(&[TargetSpec::one(TargetKind::Spell, Filter::Any)])
                .effect(&COUNTER_AND_DELAY),
            // CR 106.4: "When an effect instructs a player to add mana, that mana goes into
            // a player's mana pool."
            Ability::triggered(&Trigger::main_phase(Players::You).next())
                .effect(&add_mana_of_kind(YOU, ManaColor::Colorless, CARRIED)),
        ])],
);

/// The mana value is the spell's as it was on the stack, X included
/// ([CR 202.3e](https://yawgatog.com/resources/magic-rules/#R2023e)): "X is treated as the
/// number chosen for it while the object is on the stack." The second sentence creates a
/// delayed triggered ability ([CR 603.7a](https://yawgatog.com/resources/magic-rules/#R6037a)):
/// "Delayed triggered abilities are created during the resolution of spells or abilities".
static COUNTER_AND_DELAY: Effect = sequence(&[
    counter(TARGET),
    delay_carrying(ADD_COLORLESS, Amount::ManaValue(TARGET)),
]);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_engine::card::ManaColor as Color;

    use beryl_engine::{DecisionKind, TurnStep, Zone};

    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use crate::{names, props};

    /// P0 counters P1's Timetwister (mana value 3) in P1's precombat main phase. The mana waits
    /// for P0's own next main phase: not P1's postcombat main, but P0's precombat main of turn 3,
    /// where the delayed trigger goes on the stack as the phase begins.
    ///
    /// Cast in P0's own precombat main phase, against the Mana Tithe P1 aims at P0's Mana Crypt,
    /// the next main phase is the postcombat main of the same turn. The Plains in P0's hand
    /// keeps P0's priority a decision there.
    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new(
            "Mana Drain: the countered spell's mana value at the caster's next main phase",
        )
        .at(2, P1, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[Permanent::new(names::ISLAND), Permanent::new(names::ISLAND)],
        )
        .hand(P0, &[names::MANA_DRAIN])
        .library(P0, &[names::PLAINS])
        .battlefield(
            P1,
            &[
                Permanent::new(names::ISLAND),
                Permanent::new(names::ISLAND),
                Permanent::new(names::ISLAND),
            ],
        )
        .hand(P1, &[names::TIMETWISTER])
        .actions(&[
            Action::cast(P1, names::TIMETWISTER),
            Action::cast(P0, names::MANA_DRAIN),
            Action::assert(Assertion::Count {
                player: P1,
                zone: Zone::Graveyard,
                card: Some(names::TIMETWISTER),
                count: 1,
            }),
            Action::AdvanceTo {
                turn: 2,
                step: TurnStep::PostcombatMain,
            },
            Action::assert(Assertion::Mana {
                player: P0,
                color: Color::Colorless,
                amount: 0,
            }),
            Action::AdvanceTo {
                turn: 3,
                step: TurnStep::PrecombatMain,
            },
            Action::pass(P0),
            Action::assert(Assertion::Mana {
                player: P0,
                color: Color::Colorless,
                amount: 3,
            }),
        ]),
        // P1 counters P0's artifact with Counterspell, and P0 drains the Counterspell.
        Scenario::new("Mana Drain on the caster's own turn: mana at that turn's postcombat main")
            .battlefield(
                P0,
                &[
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::ISLAND),
                    Permanent::new(names::PLAINS),
                ],
            )
            .hand(P0, &[props::ARTIFACT, names::MANA_DRAIN, names::PLAINS])
            .battlefield(
                P1,
                &[Permanent::new(names::ISLAND), Permanent::new(names::ISLAND)],
            )
            .hand(P1, &[props::INSTANT])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, props::ARTIFACT),
                Action::pass(P0),
                Action::cast(P1, props::INSTANT),
                Action::cast(P0, names::MANA_DRAIN),
                Action::answer(
                    P0,
                    DecisionKind::Target,
                    Pick::Card {
                        card: props::INSTANT,
                    },
                ),
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Graveyard,
                    card: Some(props::INSTANT),
                    count: 1,
                }),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Battlefield,
                    card: Some(props::ARTIFACT),
                    count: 1,
                }),
                Action::assert(Assertion::Mana {
                    player: P0,
                    color: Color::Colorless,
                    amount: 0,
                }),
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::PostcombatMain,
                },
                Action::pass(P0),
                Action::assert(Assertion::Mana {
                    player: P0,
                    color: Color::Colorless,
                    amount: 2,
                }),
            ]),
    ];
}
