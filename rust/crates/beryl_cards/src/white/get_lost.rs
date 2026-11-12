//! Get Lost. {1}{W} Instant. "Destroy target creature, enchantment, or planeswalker. Its
//! controller creates two Map tokens."
//!
//! Map: Token Artifact — Map. "{1}, {T}, Sacrifice this token: Target creature you control
//! explores. Activate only as a sorcery."

use crate::names;
use beryl_card::effect::build::{
    ITS_CONTROLLER, TARGET, create_tokens, destroy, explore, sequence,
};
use beryl_card::{
    Ability, CardDef, Colors, Cost, CostPart, Face, Filter, ManaCost, Players, Subtype, Subtypes,
    TargetKind, TargetSpec, Types, card, mana,
};

/// "Its controller" is the permanent's controller as it was destroyed
/// ([CR 608.2h](https://yawgatog.com/resources/magic-rules/#R6082h)): "If an effect requires
/// information from the game ..., the answer is determined only once, when the effect is
/// applied. If the effect requires information from a specific object, ... the effect uses
/// the current information of that object if it's in the public zone it was expected to be
/// in; if it's no longer in that zone, ... the effect uses the object's last known
/// information."
pub static CARD: CardDef = card(
    names::GET_LOST,
    &[Face::EMPTY
        .cost(mana("{1}{W}"))
        .types(Types::INSTANT)
        .colors(Colors::WHITE)
        .abilities(&[Ability::spell()
            .targeting(&[TargetSpec::one(
                TargetKind::Permanent,
                Filter::Types(
                    Types::CREATURE
                        .union(Types::ENCHANTMENT)
                        .union(Types::PLANESWALKER),
                ),
            )])
            .effect(&sequence(&[
                destroy(TARGET),
                create_tokens(ITS_CONTROLLER, &MAP, 2),
            ]))])],
)
.creating(&[&MAP]);

/// The Map token's name: its subtype plus the word "Token" (CR 111.4).
pub const MAP_TOKEN: &str = "Map Token";

/// The predefined Map token
/// ([CR 111.10s](https://yawgatog.com/resources/magic-rules/#R11110s)): "A Map token is a
/// colorless Map artifact token with "{1}, {T}, Sacrifice this token: Target creature you
/// control explores. Activate only as a sorcery.""
/// Explore is [CR 701.44a](https://yawgatog.com/resources/magic-rules/#R70144a).
pub static MAP: CardDef = card(
    MAP_TOKEN,
    &[Face::EMPTY
        .types(Types::ARTIFACT)
        .subtypes(Subtypes::of(&[Subtype::Map]))
        .abilities(&[Ability::activated(Cost::of(
            ManaCost::generic(1),
            &[CostPart::TAP, CostPart::SACRIFICE],
        ))
        .targeting(&[TargetSpec::one(
            TargetKind::Permanent,
            Filter::All(&[Filter::Types(Types::CREATURE), Filter::Whose(&Players::You)]),
        )])
        .as_a_sorcery()
        .effect(&explore(TARGET))])],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_card::CounterKind;
    use beryl_card::{DecisionKind, Seat, TurnStep, Zone};

    use beryl_scenario_types::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use super::MAP;
    use crate::{names, props};

    const SPELLBINDER: &str = names::ELITE_SPELLBINDER;

    const JACE: &str = names::JACE_THE_MIND_SCULPTOR;

    const MAP_NAME: &str = MAP.name;

    const THREE_PLAINS: &[Permanent] = &[
        Permanent::new(names::PLAINS),
        Permanent::new(names::PLAINS),
        Permanent::new(names::PLAINS),
    ];

    const fn count(player: Seat, zone: Zone, card: &'static str, count: u8) -> Action {
        Action::assert(Assertion::Count {
            player,
            zone,
            card: Some(card),
            count,
        })
    }

    const fn library(top: &'static [&'static str]) -> Action {
        Action::assert(Assertion::Library { player: P1, top })
    }

    /// In P1's turn 2, P0 destroys one of P1's two Elite Spellbinders with Get Lost at instant
    /// speed; P1 then sacrifices a Map, paying {1} with their Plains, and their other Spellbinder
    /// explores the top of `top`. The Island in P1's hand keeps their priority a decision.
    const fn exploring(name: &'static str, top: &'static [&'static str]) -> Scenario {
        const SPELLBINDERS_AND_PLAINS: &[Permanent] = &[
            Permanent::new(SPELLBINDER),
            Permanent::new(SPELLBINDER),
            Permanent::new(names::PLAINS),
        ];
        Scenario::new(name)
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, THREE_PLAINS)
            .hand(P0, &[names::GET_LOST])
            .library(P0, &[names::PLAINS])
            .battlefield(P1, SPELLBINDERS_AND_PLAINS)
            .hand(P1, &[names::ISLAND])
            .library(P1, top)
    }

    const LET_P0_ACT: Action = Action::pass(P1);

    const GET_LOST: Action = Action::cast(P0, names::GET_LOST);

    const SACRIFICE_A_MAP: Action = Action::activate(P1, MAP_NAME, 0);

    /// Explore ([CR 701.44a](https://yawgatog.com/resources/magic-rules/#R70144a)): "...that
    /// permanent's controller reveals the top card of their library. If a land card is revealed
    /// this way, that player puts that card into their hand. Otherwise, that player puts a +1/+1
    /// counter on the exploring permanent and may put the revealed card into their graveyard." With
    /// nothing to reveal, no land is revealed, so the creature gets the counter.
    const fn counters(count: u8) -> Action {
        Action::assert(Assertion::Counters {
            player: P1,
            card: SPELLBINDER,
            kind: CounterKind::PlusOne,
            count,
        })
    }

    pub const SCENARIOS: &[Scenario] = &[
        // With no legal target it can't be cast
        // ([CR 601.2c](https://yawgatog.com/resources/magic-rules/#R6012c)): P1's Mana Crypt is
        // an artifact, so passing is P0's only option.
        Scenario::new("Get Lost can't be cast without a target")
            .battlefield(P0, THREE_PLAINS)
            .hand(P0, &[names::GET_LOST])
            .battlefield(P1, &[Permanent::new(props::ARTIFACT)])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::PrecombatMain,
                },
                Action::assert(Assertion::Decision {
                    player: P0,
                    kind: DecisionKind::Priority,
                    candidates: 1,
                }),
            ]),
        // Its mana cost is {1}{W}: two Plains cast it. The oracles caught it costing {1}{W}{W}.
        Scenario::new("Get Lost costs one and a white")
            .battlefield(
                P0,
                &[Permanent::new(names::PLAINS), Permanent::new(names::PLAINS)],
            )
            .hand(P0, &[names::GET_LOST, names::ISLAND])
            .battlefield(P1, &[Permanent::new(SPELLBINDER)])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, names::GET_LOST),
                count(P1, Zone::Graveyard, SPELLBINDER, 1),
                Action::assert(Assertion::Tapped {
                    player: P0,
                    card: names::PLAINS,
                    count: 2,
                }),
            ]),
        // The Island left in P0's hand keeps the game in P0's main phase once Get Lost has
        // resolved.
        Scenario::new("Get Lost on a planeswalker gives its controller two Maps")
            .battlefield(P0, THREE_PLAINS)
            .hand(P0, &[names::GET_LOST, names::ISLAND])
            .battlefield(P1, &[Permanent::new(JACE)])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, names::GET_LOST),
                count(P1, Zone::Graveyard, JACE, 1),
                count(P1, Zone::Battlefield, MAP_NAME, 2),
                count(P0, Zone::Battlefield, MAP_NAME, 0),
            ]),
        exploring(
            "Get Lost on a creature gives two Maps; one explores a land into hand",
            &[names::PLAINS, names::ISLAND],
        )
        .actions(&[
            LET_P0_ACT,
            GET_LOST,
            count(P1, Zone::Graveyard, SPELLBINDER, 1),
            count(P1, Zone::Battlefield, MAP_NAME, 2),
            count(P0, Zone::Battlefield, MAP_NAME, 0),
            SACRIFICE_A_MAP,
            count(P1, Zone::Battlefield, MAP_NAME, 1),
            count(P1, Zone::Hand, names::PLAINS, 1),
            library(&[names::ISLAND]),
            counters(0),
        ]),
        exploring(
            "A Map explores a nonland into a counter and the graveyard",
            &[names::OUST, names::ISLAND],
        )
        .actions(&[
            LET_P0_ACT,
            GET_LOST,
            SACRIFICE_A_MAP,
            Action::answer(P1, DecisionKind::YesNo, Pick::Yes),
            counters(1),
            count(P1, Zone::Graveyard, names::OUST, 1),
            library(&[names::ISLAND]),
        ]),
        exploring(
            "A Map explores a nonland into a counter, left on top",
            &[names::OUST, names::ISLAND],
        )
        .actions(&[
            LET_P0_ACT,
            GET_LOST,
            SACRIFICE_A_MAP,
            Action::answer(P1, DecisionKind::YesNo, Pick::No),
            counters(1),
            count(P1, Zone::Graveyard, names::OUST, 0),
            library(&[names::OUST, names::ISLAND]),
        ]),
        exploring("A Map explores an empty library into a counter", &[]).actions(&[
            LET_P0_ACT,
            GET_LOST,
            SACRIFICE_A_MAP,
            counters(1),
        ]),
    ];
}
