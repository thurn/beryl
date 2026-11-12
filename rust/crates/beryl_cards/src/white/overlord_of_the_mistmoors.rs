//! Overlord of the Mistmoors. {5}{W}{W} Enchantment Creature — Avatar Horror, 6/6.
//! "Impending 4—{2}{W}{W} (If you cast this spell for its impending cost, it enters with four
//! time counters and isn't a creature until the last is removed. At the beginning of your end
//! step, remove a time counter from it.) Whenever this permanent enters or attacks, create two
//! 2/1 white Insect creature tokens with flying."

use beryl_engine::card::{
    Ability, CardDef, CastingOption, Colors, Face, Flow, Keywords, ManaCost, Subtype, Subtypes,
    Trigger, Types,
};
use beryl_engine::effect::EffectCtx;
use beryl_engine::event::TokenEntry;
use beryl_engine::{Layout, bodies};

use crate::names;

/// The Insect token's name ([CR 111.4](https://yawgatog.com/resources/magic-rules/#R1114)):
/// its subtype plus the word "Token".
pub const INSECT_TOKEN: &str = "Insect Token";

/// The Insect token: a 2/1 white Insect creature token with flying.
pub static INSECT: CardDef = CardDef {
    name: INSECT_TOKEN,
    faces: &[Face {
        types: Types::CREATURE,
        subtypes: Subtypes::of(&[Subtype::Insect]),
        colors: Colors::WHITE,
        power: Some(2),
        toughness: Some(1),
        keywords: Keywords::FLYING,
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "Create two 2/1 white Insect creature tokens with flying": one event
/// ([CR 111.1](https://yawgatog.com/resources/magic-rules/#R1111)).
fn two_insects<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    ctx.create_tokens(&INSECT, 2, TokenEntry::default());
    Ok(())
}

/// Impending is four abilities
/// ([CR 702.176a](https://yawgatog.com/resources/magic-rules/#R702176a)): the alternative cost,
/// the time counters it enters with (both in the casting option), "isn't a creature" while it
/// has one, and the end-step countdown.
pub static CARD: CardDef = CardDef {
    name: names::OVERLORD_OF_THE_MISTMOORS,
    faces: &[Face {
        cost: ManaCost::new(5, [2, 0, 0, 0, 0, 0]),
        types: Types::ENCHANTMENT.union(Types::CREATURE),
        subtypes: Subtypes::of(&[Subtype::Avatar, Subtype::Horror]),
        colors: Colors::WHITE,
        power: Some(6),
        toughness: Some(6),
        abilities: &[
            Ability::impending(),
            Ability::impending_countdown(),
            Ability::triggered(&Trigger::ENTERS, bodies!(two_insects)),
            Ability::triggered(&Trigger::ATTACKS, bodies!(two_insects)),
        ],
        casting: &[CastingOption::Impending {
            cost: ManaCost::new(2, [2, 0, 0, 0, 0, 0]),
            counters: 4,
        }],
        ..Face::EMPTY
    }],
    tokens: &[&INSECT],
};

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_engine::card::{Castings, FaceIx};

    use beryl_engine::state::CounterKind;
    use beryl_engine::{DecisionKind, TurnStep};

    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use super::INSECT_TOKEN;
    use crate::names;

    const OVERLORD: &str = names::OVERLORD_OF_THE_MISTMOORS;

    const fn plains(n: usize) -> &'static [Permanent] {
        const SEVEN: &[Permanent] = &[Permanent::new(names::PLAINS); 7];
        SEVEN.split_at(n).0
    }

    /// One of your turns from its beginning of combat: no attack, then the end step's countdown
    /// trigger resolves.
    macro_rules! countdown_turn {
        ($turn:expr) => {
            [
                Action::AdvanceTo {
                    turn: $turn,
                    step: TurnStep::BeginningOfCombat,
                },
                Action::Answer {
                    player: P0,
                    kind: DecisionKind::Priority,
                    pick: Pick::Pass,
                },
                Action::Answer {
                    player: P0,
                    kind: DecisionKind::Attacker,
                    pick: Pick::Done,
                },
                Action::AdvanceTo {
                    turn: $turn,
                    step: TurnStep::End,
                },
                Action::Resolve,
            ]
        };
    }

    const fn time_counters(count: u8) -> Action {
        Action::Assert {
            assertion: Assertion::Counters {
                player: P0,
                card: OVERLORD,
                kind: CounterKind::Time,
                count,
            },
        }
    }

    const fn is_creature(creature: bool) -> Action {
        Action::Assert {
            assertion: Assertion::Creature {
                player: P0,
                card: OVERLORD,
                creature,
            },
        }
    }

    const IMPENDING_TURNS: [[Action; 5]; 3] =
        [countdown_turn!(3), countdown_turn!(5), countdown_turn!(7)];

    pub const SCENARIOS: &[Scenario] = &[
        // Cast for {2}{W}{W}: four time counters, not a creature, two Insects all the same. Each
        // of its controller's end steps removes one; with the last gone it is a 6/6.
        Scenario::new("Overlord cast for impending")
            .hand(P0, &[OVERLORD])
            .battlefield(P0, plains(4))
            .library(P0, &[names::PLAINS, names::PLAINS, names::PLAINS])
            .library(P1, &[names::ISLAND, names::ISLAND, names::ISLAND])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: OVERLORD,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                Action::Resolve,
                is_creature(false),
                time_counters(4),
                Action::Resolve,
                Action::Assert {
                    assertion: Assertion::Controls {
                        player: P0,
                        card: INSECT_TOKEN,
                        count: 2,
                    },
                },
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::End,
                },
                Action::Resolve,
                time_counters(3),
                IMPENDING_TURNS[0][0],
                IMPENDING_TURNS[0][1],
                IMPENDING_TURNS[0][2],
                IMPENDING_TURNS[0][3],
                IMPENDING_TURNS[0][4],
                time_counters(2),
                IMPENDING_TURNS[1][0],
                IMPENDING_TURNS[1][1],
                IMPENDING_TURNS[1][2],
                IMPENDING_TURNS[1][3],
                IMPENDING_TURNS[1][4],
                time_counters(1),
                is_creature(false),
                IMPENDING_TURNS[2][0],
                IMPENDING_TURNS[2][1],
                IMPENDING_TURNS[2][2],
                IMPENDING_TURNS[2][3],
                IMPENDING_TURNS[2][4],
                time_counters(0),
                is_creature(true),
                Action::Assert {
                    assertion: Assertion::Pt {
                        player: P0,
                        card: OVERLORD,
                        token: false,
                        power: 6,
                        toughness: 6,
                    },
                },
            ]),
        // Cast in full it is a creature at once, and its attack makes two more Insects, which
        // are not attacking.
        Scenario::new("Overlord cast in full, then attacking")
            .hand(P0, &[OVERLORD])
            .battlefield(P0, plains(7))
            .library(P0, &[names::PLAINS])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: OVERLORD,
                    face: FaceIx::FRONT,
                    costs: Castings::NONE,
                },
                Action::Resolve,
                is_creature(true),
                time_counters(0),
                Action::Resolve,
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::BeginningOfCombat,
                },
                Action::Answer {
                    player: P0,
                    kind: DecisionKind::Priority,
                    pick: Pick::Pass,
                },
                Action::Answer {
                    player: P0,
                    kind: DecisionKind::Attacker,
                    pick: Pick::Card { card: OVERLORD },
                },
                Action::Answer {
                    player: P0,
                    kind: DecisionKind::Attacker,
                    pick: Pick::Done,
                },
                Action::Resolve,
                Action::Assert {
                    assertion: Assertion::Controls {
                        player: P0,
                        card: INSECT_TOKEN,
                        count: 4,
                    },
                },
                Action::Assert {
                    assertion: Assertion::Attacking {
                        player: P0,
                        card: INSECT_TOKEN,
                        count: 0,
                    },
                },
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::EndOfCombat,
                },
                Action::Assert {
                    assertion: Assertion::Life {
                        player: P1,
                        life: 14,
                    },
                },
            ]),
    ];
}
