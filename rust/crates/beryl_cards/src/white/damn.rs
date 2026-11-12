//! Damn. {B}{B} Sorcery. "Destroy target creature. A creature destroyed this way can't be
//! regenerated. Overload {2}{W}{W} (You may cast this spell for its overload cost. If you do,
//! change "target" in its text to "each.")"

use beryl_card::Objects;
use beryl_card::effect::build::{TARGET, destroy};
use beryl_engine::Zone;
use beryl_engine::card::{
    Ability, AbilityKind, Behavior, CardDef, CastingOption, Colors, Face, ManaCost, TargetKind,
    TargetSpec, Types,
};
use beryl_engine::filter::Filter;

use crate::names;

const CREATURES: Filter = Filter::Types(Types::CREATURE);

/// Its color is its mana cost's, black: the overload cost doesn't change it
/// ([CR 202.2](https://yawgatog.com/resources/magic-rules/#R2022)): "An object is the color or
/// colors of the mana symbols in its mana cost". The cube lists it with white for its
/// overload.
pub static CARD: CardDef = CardDef {
    name: names::DAMN,
    faces: &[Face {
        cost: ManaCost::new(0, [0, 0, 2, 0, 0, 0]),
        types: Types::SORCERY,
        colors: Colors::BLACK,
        abilities: &[
            // "Can't be regenerated"
            // ([CR 701.19c](https://yawgatog.com/resources/magic-rules/#R70119c)): such
            // effects "cause regeneration shields to not be applied." No card in the cube
            // regenerates, so destroying is all it takes.
            Ability {
                kind: AbilityKind::Spell,
                targets: &[TargetSpec::one(TargetKind::Permanent, CREATURES)],
                body: Behavior::Data(&destroy(TARGET)),
                ..Ability::NONE
            },
            // Overloaded, "Destroy each creature"
            // ([CR 702.96a](https://yawgatog.com/resources/magic-rules/#R70296a)). It targets
            // nothing, so hexproof and protection don't stop it
            // ([CR 702.96b](https://yawgatog.com/resources/magic-rules/#R70296b)): "If a
            // player chooses to pay the overload cost of a spell, that spell won't require
            // any targets." The creatures are destroyed at once.
            Ability {
                kind: AbilityKind::Spell,
                body: Behavior::Data(&destroy(Objects::All {
                    zone: Zone::Battlefield,
                    filter: CREATURES,
                })),
                ..Ability::NONE
            },
        ],
        casting: &[CastingOption::overload(
            ManaCost::new(2, [2, 0, 0, 0, 0, 0]),
            1,
        )],
        ..Face::EMPTY
    }],
    tokens: &[],
};

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_engine::card::{Castings, FaceIx};

    use beryl_engine::{DecisionKind, Zone};
    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

    use crate::white::adeline_resplendent_cathar::HUMAN_TOKEN;
    use crate::{names, props};

    const SPELLBINDER: &str = names::ELITE_SPELLBINDER;

    const TWO_DROP: &str = props::SMALL_CREATURE;

    const fn creatures(player: beryl_engine::Seat, card: &'static str, count: u8) -> Action {
        Action::assert(Assertion::Controls {
            player,
            card,
            count,
        })
    }

    pub const SCENARIOS: &[Scenario] = &[
        // For {B}{B}, one target: P0 destroys P1's Elite Spellbinder and keeps its own two-drop.
        Scenario::new("Damn for {B}{B} destroys its target alone")
            .battlefield(
                P0,
                &[
                    Permanent::new(names::SWAMP),
                    Permanent::new(names::SWAMP),
                    Permanent::new(TWO_DROP),
                ],
            )
            .hand(P0, &[names::DAMN])
            .battlefield(P1, &[Permanent::new(SPELLBINDER)])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, names::DAMN),
                Action::choose(P0, DecisionKind::Target, SPELLBINDER),
                Action::Resolve,
                creatures(P1, SPELLBINDER, 0),
                creatures(P0, TWO_DROP, 1),
            ]),
        // With only white mana, the overload is the one way to cast it: two candidates, passing
        // and the overloaded cast. It asks for no target and destroys every creature, P0's own
        // among them.
        Scenario::new("Damn overloaded destroys each creature, targeting none")
            .battlefield(
                P0,
                &[
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                    Permanent::new(TWO_DROP),
                ],
            )
            .hand(P0, &[names::DAMN])
            .battlefield(P1, &[Permanent::new(SPELLBINDER), Permanent::new(TWO_DROP)])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::assert(Assertion::Decision {
                    player: P0,
                    kind: DecisionKind::Priority,
                    candidates: 2,
                }),
                Action::Cast {
                    player: P0,
                    card: names::DAMN,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                Action::Resolve,
                creatures(P0, TWO_DROP, 0),
                creatures(P1, SPELLBINDER, 0),
                creatures(P1, TWO_DROP, 0),
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Graveyard,
                    card: None,
                    count: 2,
                }),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Graveyard,
                    card: None,
                    count: 2,
                }),
            ]),
        // Overloaded, it takes every kind of creature in one move: a token, which ceases to exist
        // once in the graveyard ([CR 704.5d](https://yawgatog.com/resources/magic-rules/#R7045d):
        // "If a token is in a zone other than the battlefield, it ceases to exist"), and P1's
        // face-down creature, revealed as it leaves
        // ([CR 708.9](https://yawgatog.com/resources/magic-rules/#R7089): "If a face-down
        // permanent ... moves from the battlefield to any other zone, its owner must reveal it").
        // Adeline's Human is the token, and a token's maker is on the battlefield with it.
        Scenario::new("Damn overloaded takes tokens and face-down creatures at once")
            .interaction(&[names::ADELINE_RESPLENDENT_CATHAR])
            .battlefield(
                P0,
                &[
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::ADELINE_RESPLENDENT_CATHAR),
                    Permanent::token(HUMAN_TOKEN),
                ],
            )
            .hand(P0, &[names::DAMN])
            .battlefield(
                P1,
                &[
                    Permanent::new(SPELLBINDER),
                    Permanent::new(names::SNAPCASTER_MAGE).face_down(),
                ],
            )
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: names::DAMN,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                Action::Resolve,
                creatures(P0, names::ADELINE_RESPLENDENT_CATHAR, 0),
                creatures(P0, HUMAN_TOKEN, 0),
                creatures(P1, SPELLBINDER, 0),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Graveyard,
                    card: None,
                    count: 2,
                }),
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Graveyard,
                    card: Some(names::SNAPCASTER_MAGE),
                    count: 1,
                }),
            ]),
    ];
}
