//! Sink into Stupor // Soporific Springs, a modal double-faced card.
//!
//! Sink into Stupor. {1}{U}{U} Instant. "Return target spell or nonland permanent an opponent
//! controls to its owner's hand."
//!
//! Soporific Springs. Land. "As this land enters, you may pay 3 life. If you don't, it enters
//! tapped. {T}: Add {U}."

use beryl_engine::card::{
    Ability, AbilityKind, CardDef, Colors, Face, Flow, ManaColor, ManaCost, ManaSet, Replacement,
    TargetKind, TargetSpec, Types,
};
use beryl_engine::effect::EffectCtx;
use beryl_engine::filter::{Filter, Whose};
use beryl_engine::{DecisionKind, Seat, TurnStep, Zone};
use beryl_engine::{Layout, bodies};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

use crate::names;

/// [CR 712.11b](https://yawgatog.com/resources/magic-rules/#R71211b): "A player casting a modal
/// double-faced card ... chooses which face they are casting"; only the instant can be, and
/// [CR 712.12](https://yawgatog.com/resources/magic-rules/#R71212): "A player playing a modal
/// double-faced card ... as a land chooses one of its faces that's a land ... It enters the
/// battlefield with that face up."
pub static CARD: CardDef = CardDef {
    name: names::SINK_INTO_STUPOR,
    faces: &[
        Face {
            cost: ManaCost::new(1, [0, 2, 0, 0, 0, 0]),
            types: Types::INSTANT,
            colors: Colors::BLUE,
            abilities: &[Ability {
                kind: AbilityKind::Spell,
                targets: &[TargetSpec::one(
                    TargetKind::SpellOrPermanent,
                    Filter::All(&[
                        Filter::Whose(Whose::Opponent),
                        Filter::Not(&Filter::Types(Types::LAND)),
                    ]),
                )
                .removal()],
                body: bodies!(bounce),
                ..Ability::NONE
            }],
            ..Face::EMPTY
        },
        Face {
            name: "Soporific Springs",
            types: Types::LAND,
            abilities: &[
                Ability::replacing(Replacement::EntersTappedUnlessPayLife(3)),
                Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)),
            ],
            ..Face::EMPTY
        },
    ],
    tokens: &[],
};

/// A spell returned to its owner's hand leaves the stack without resolving; a permanent
/// leaves the battlefield.
fn bounce<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    if let Some(target) = ctx.target_object(0) {
        ctx.return_to_hand(target);
    }
    Ok(())
}

const SPRINGS: &str = "Soporific Springs";

const THREE_ISLANDS: &[Permanent] = &[
    Permanent::new(names::ISLAND),
    Permanent::new(names::ISLAND),
    Permanent::new(names::ISLAND),
];

const fn count(player: Seat, zone: Zone, card: &'static str, count: u8) -> Action {
    Action::assert(Assertion::Count {
        player,
        zone,
        card: Some(card),
        count,
    })
}

const fn springs_tapped(count: u8) -> Action {
    Action::assert(Assertion::Tapped {
        player: P0,
        card: SPRINGS,
        count,
    })
}

const fn life(life: i16) -> Action {
    Action::assert(Assertion::Life { player: P0, life })
}

/// The only legal target is chosen silently: lands are not "nonland", and P0's own objects
/// are not an opponent's.
pub const SCENARIOS: &[Scenario] = &[
    Scenario::new("Sink into Stupor returns a spell to its owner's hand")
        .at(2, P1, TurnStep::PrecombatMain)
        .battlefield(P0, THREE_ISLANDS)
        .hand(P0, &[names::SINK_INTO_STUPOR])
        .battlefield(P1, THREE_ISLANDS)
        .hand(P1, &[names::TIMETWISTER])
        .library(P0, &[names::PLAINS])
        .actions(&[
            Action::cast(P1, names::TIMETWISTER),
            Action::cast(P0, names::SINK_INTO_STUPOR),
            count(P1, Zone::Hand, names::TIMETWISTER, 1),
            count(P1, Zone::Graveyard, names::TIMETWISTER, 0),
            count(P0, Zone::Graveyard, names::SINK_INTO_STUPOR, 1),
        ]),
    Scenario::new("Sink into Stupor returns a nonland permanent, never a land")
        .battlefield(P0, THREE_ISLANDS)
        .hand(P0, &[names::SINK_INTO_STUPOR])
        .battlefield(
            P1,
            &[
                Permanent::new(names::MANA_CRYPT),
                Permanent::new(names::ISLAND),
            ],
        )
        .library(P1, &[names::PLAINS])
        .actions(&[
            Action::cast(P0, names::SINK_INTO_STUPOR),
            count(P1, Zone::Hand, names::MANA_CRYPT, 1),
            count(P1, Zone::Battlefield, names::ISLAND, 1),
        ]),
    // Played as Soporific Springs paying 3 life, it taps for the {U} of the other copy cast
    // as Sink into Stupor.
    Scenario::new("Soporific Springs: 3 life paid, it enters untapped")
        .battlefield(
            P0,
            &[Permanent::new(names::ISLAND), Permanent::new(names::ISLAND)],
        )
        .hand(P0, &[names::SINK_INTO_STUPOR, names::SINK_INTO_STUPOR])
        .battlefield(P1, &[Permanent::new(names::MANA_CRYPT)])
        .library(P1, &[names::PLAINS])
        .actions(&[
            Action::PlayLand {
                player: P0,
                card: names::SINK_INTO_STUPOR,
            },
            Action::answer(P0, DecisionKind::PayLife, Pick::Yes),
            life(17),
            count(P0, Zone::Battlefield, SPRINGS, 1),
            springs_tapped(0),
            Action::cast(P0, names::SINK_INTO_STUPOR),
            springs_tapped(1),
            count(P1, Zone::Hand, names::MANA_CRYPT, 1),
        ]),
    Scenario::new("Soporific Springs: no life paid, it enters tapped")
        .hand(P0, &[names::SINK_INTO_STUPOR])
        .library(P1, &[names::PLAINS])
        .actions(&[
            Action::PlayLand {
                player: P0,
                card: names::SINK_INTO_STUPOR,
            },
            Action::answer(P0, DecisionKind::PayLife, Pick::No),
            life(20),
            springs_tapped(1),
        ]),
    // CR 119.4: a player "may [pay life] only if their life total is greater than or equal to
    // the amount of the payment."
    Scenario::new("Soporific Springs: with 2 life, it enters tapped unasked")
        .life(P0, 2)
        .hand(P0, &[names::SINK_INTO_STUPOR])
        .library(P1, &[names::PLAINS])
        .actions(&[
            Action::PlayLand {
                player: P0,
                card: names::SINK_INTO_STUPOR,
            },
            life(2),
            springs_tapped(1),
        ]),
];
