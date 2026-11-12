//! Elite Spellbinder. {2}{W} Creature — Human Cleric, 3/1. "Flying. When this creature
//! enters, look at target opponent's hand. You may exile a nonland card from it. For as long
//! as that card remains exiled, its owner may play it. A spell cast this way costs {2} more
//! to cast."

use crate::names;
use beryl_card::effect::build::{
    TARGET_PLAYER, THOSE, YOU, exile, look_at, may_choose, owner_of, permit_cast, sequence,
};
use beryl_card::{
    Ability, CardDef, CastTerms, Colors, Duration, Effect, Face, Filter, Keywords, Objects, Place,
    Players, Prompt, Purpose, Subtype, Subtypes, TargetKind, TargetSpec, Trigger, Types, Zone,
    card, mana,
};

/// "Target opponent's hand".
const THEIR_HAND: Objects = Objects::InZoneOf {
    zone: Zone::Hand,
    players: &TARGET_PLAYER,
    filter: Filter::Any,
};

/// "A nonland card from it".
const A_NONLAND_CARD_FROM_IT: Objects = Objects::InZoneOf {
    zone: Zone::Hand,
    players: &TARGET_PLAYER,
    filter: Filter::Not(&Filter::Types(Types::LAND)),
};

/// Looking at a hand shows it to the looker alone; the exiled card is public. Its owner may
/// cast it from exile for as long as it stays there
/// ([CR 601.3](https://yawgatog.com/resources/magic-rules/#R6013)), and the {2} is a cost
/// increase ([CR 601.2f](https://yawgatog.com/resources/magic-rules/#R6012f)): the total cost
/// is the mana cost "plus all additional costs and cost increases". The card is nonland, so
/// playing it is casting it. The whole hand is looked at, for one card to exile.
static SPELLBIND: Effect = sequence(&[
    look_at(YOU, THEIR_HAND),
    may_choose(
        YOU,
        A_NONLAND_CARD_FROM_IT,
        Prompt::new(Purpose::Look)
            .most(1)
            .places(Place::Exile, Place::None),
    ),
    exile(THOSE),
    permit_cast(
        owner_of(THOSE),
        THOSE,
        Duration::Indefinite,
        CastTerms::PLAIN.costing_more(2),
    ),
]);

pub static CARD: CardDef = card(
    names::ELITE_SPELLBINDER,
    &[Face::EMPTY
        .cost(mana("{2}{W}"))
        .types(Types::CREATURE)
        .subtypes(Subtypes::of(&[Subtype::Human, Subtype::Cleric]))
        .colors(Colors::WHITE)
        .power_toughness(3, 1)
        .keywords(Keywords::FLYING)
        .abilities(&[Ability::triggered(&Trigger::ENTERS)
            .targeting(&[TargetSpec::one(
                TargetKind::Player,
                Filter::Whose(&Players::EachOpponent),
            )])
            .effect(&SPELLBIND)])],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_engine::card::{Castings, FaceIx};

    use beryl_engine::{DecisionKind, TurnStep, Zone};

    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use crate::{names, props};

    const SPELLBINDER: &str = names::ELITE_SPELLBINDER;

    const ARTIFACT: &str = props::ARTIFACT;

    const THREE_PLAINS: &[Permanent] = &[
        Permanent::new(names::PLAINS),
        Permanent::new(names::PLAINS),
        Permanent::new(names::PLAINS),
    ];

    pub const SCENARIOS: &[Scenario] = &[
        // The exiled artifact costs its owner {2}{W} on their turn: all three Plains.
        Scenario::new("Spellbinder's card costs its owner {2} more")
            .hand(P0, &[SPELLBINDER])
            .battlefield(P0, THREE_PLAINS)
            .hand(P1, &[ARTIFACT, names::ISLAND])
            .battlefield(P1, THREE_PLAINS)
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: SPELLBINDER,
                    face: FaceIx::FRONT,
                    costs: Castings::NONE,
                },
                Action::Resolve,
                // The enters trigger, which targets the one opponent, resolves up to its question.
                Action::Assert {
                    assertion: Assertion::Stack { count: 1 },
                },
                Action::Resolve,
                Action::Answer {
                    player: P0,
                    kind: DecisionKind::YesNo,
                    pick: Pick::Yes,
                },
                Action::Assert {
                    assertion: Assertion::Count {
                        player: P1,
                        zone: Zone::Exile,
                        card: Some(ARTIFACT),
                        count: 1,
                    },
                },
                Action::Assert {
                    assertion: Assertion::Count {
                        player: P1,
                        zone: Zone::Hand,
                        card: Some(ARTIFACT),
                        count: 0,
                    },
                },
                Action::AdvanceTo {
                    turn: 2,
                    step: TurnStep::PrecombatMain,
                },
                // Pass, an Island to play, the one P0 saw or the one just drawn, and the artifact.
                Action::Assert {
                    assertion: Assertion::Decision {
                        player: P1,
                        kind: DecisionKind::Priority,
                        candidates: 4,
                    },
                },
                Action::Cast {
                    player: P1,
                    card: ARTIFACT,
                    face: FaceIx::FRONT,
                    costs: Castings::NONE,
                },
                Action::Assert {
                    assertion: Assertion::Tapped {
                        player: P1,
                        card: names::PLAINS,
                        count: 3,
                    },
                },
                Action::Resolve,
                Action::Assert {
                    assertion: Assertion::Controls {
                        player: P1,
                        card: ARTIFACT,
                        count: 1,
                    },
                },
            ]),
        // Only lands: nothing to exile, nothing to ask.
        Scenario::new("Spellbinder sees a hand of lands")
            .hand(P0, &[SPELLBINDER])
            .battlefield(P0, THREE_PLAINS)
            .hand(P1, &[names::ISLAND, names::PLAINS])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: SPELLBINDER,
                    face: FaceIx::FRONT,
                    costs: Castings::NONE,
                },
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::PostcombatMain,
                },
                Action::Assert {
                    assertion: Assertion::Count {
                        player: P1,
                        zone: Zone::Exile,
                        card: None,
                        count: 0,
                    },
                },
                Action::Assert {
                    assertion: Assertion::Count {
                        player: P1,
                        zone: Zone::Hand,
                        card: None,
                        count: 2,
                    },
                },
            ]),
    ];
}
