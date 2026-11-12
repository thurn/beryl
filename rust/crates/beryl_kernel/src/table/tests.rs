//! Tests of the card tables.

use beryl_card::{
    Behavior, Condition, Cost, Effect, Filter, Keywords, LayerSpec, ManaColor, ManaSet,
    Modification, Objects, Players, Subject, Trigger, TriggerEvent, TurnStep, Types, Zone,
};

use super::*;

static ALPHA: CardDef = CardDef {
    name: "Alpha",
    faces: &[],
    tokens: &[],
};
static BETA: CardDef = CardDef {
    name: "Beta",
    faces: &[],
    tokens: &[],
};
/// A second definition under Alpha's name.
static ALPHA_AGAIN: CardDef = CardDef {
    name: "Alpha",
    faces: &[],
    tokens: &[],
};
static TABLE: CardTable = CardTable::new(&[&ALPHA, &BETA, &ALPHA_AGAIN]);

/// A planeswalker whose static abilities read the turn it entered and change a rule
/// for its controller's opponents.
static WALKER: CardDef = CardDef {
    name: "Walker",
    faces: &[Face {
        abilities: &[
            Ability {
                kind: AbilityKind::Static,
                rule: Rule::InstantLoyaltyTheTurnItEntered,
                ..Ability::NONE
            },
            Ability {
                kind: AbilityKind::Static,
                rule: Rule::OpponentsCastAsSorceries,
                ..Ability::NONE
            },
            Ability {
                kind: AbilityKind::Activated,
                cost: Cost::loyalty(1),
                ..Ability::NONE
            },
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};
static WALKERS: CardTable = CardTable::new(&[&ALPHA, &WALKER]);

#[test]
fn a_game_has_the_capabilities_of_its_cards_and_no_others() {
    let all = [Capability::EnteredThisTurn, Capability::LoyaltyActivated];
    let plain = GameTable::new(&WALKERS, [&[(4, "Alpha")], &[(4, "Alpha")]]).unwrap();
    assert!(all.iter().all(|&c| !plain.can(c)));
    // One deck's card is enough: both players' games read the same table.
    let walker = GameTable::new(&WALKERS, [&[(4, "Alpha")], &[(1, "Walker")]]).unwrap();
    assert!(all.iter().all(|&c| walker.can(c)));
}

#[test]
fn a_card_that_changes_a_rule_for_players_says_so_in_its_mask() {
    let t = GameTable::new(&WALKERS, [&[(4, "Alpha")], &[(1, "Walker")]]).unwrap();
    let (alpha, walker) = (CardIx::from_index(0), CardIx::from_index(1));
    assert_eq!(t.listens(alpha) & kind::RULE_CHANGE, 0);
    assert_eq!(t.listens(walker), kind::RULE_CHANGE);
    assert_eq!(t.listens_any(), kind::RULE_CHANGE);
    // Its rule about itself is none of the players' rules.
    assert_eq!(
        t.may_have_rules(walker).common(Rules::OF_PLAYERS),
        Rules::NONE.with(Rule::OpponentsCastAsSorceries)
    );
}

/// "When this enters" and "at the beginning of your upkeep" on the front, "whenever you
/// draw a card" on the back.
static HERALD: CardDef = CardDef {
    name: "Herald",
    faces: &[
        Face {
            abilities: &[
                Ability::triggered(&Trigger::ENTERS, Behavior::Data(&Effect::NOTHING)),
                Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)),
                Ability::triggered(
                    &Trigger::step(TurnStep::Upkeep, Players::You),
                    Behavior::Data(&Effect::NOTHING),
                ),
                Ability::triggered(&Trigger::ENTERS, Behavior::Data(&Effect::NOTHING)),
            ],
            ..Face::EMPTY
        },
        Face {
            abilities: &[Ability::triggered(
                &Trigger::draws(Players::You),
                Behavior::Data(&Effect::NOTHING),
            )],
            ..Face::EMPTY
        },
    ],
    tokens: &[],
};
/// "Whenever another creature enters", which the engine has no code to match, on a
/// token a card makes.
static GREETER: CardDef = CardDef {
    name: "Greeter",
    faces: &[Face {
        abilities: &[Ability::triggered(
            &Trigger::of(
                TriggerEvent::Enters,
                Subject::A(Filter::Types(Types::CREATURE)),
            ),
            Behavior::Data(&Effect::NOTHING),
        )],
        ..Face::EMPTY
    }],
    tokens: &[],
};
static HOST: CardDef = CardDef {
    name: "Host",
    faces: &[],
    tokens: &[&GREETER],
};
static TRIGGERS: CardTable = CardTable::new(&[&ALPHA, &HERALD, &GREETER, &HOST]);

#[test]
fn an_event_reads_only_the_listeners_of_its_kind_on_the_face_that_is_up() {
    let t = GameTable::new(&TRIGGERS, [&[(4, "Alpha")], &[(1, "Herald")]]).unwrap();
    let (alpha, herald) = (CardIx::from_index(0), CardIx::from_index(1));
    let listening = |card, face, kind| -> Vec<u8> {
        t.listeners(card, face, kind)
            .map(|l| l.ability.get())
            .collect()
    };
    assert_eq!(t.listens(herald), kind::ENTERS | kind::STEP | kind::DRAW);
    assert_eq!(t.listens_any(), t.listens(herald));
    assert_eq!(listening(herald, FaceIx::FRONT, kind::ENTERS), [0, 3]);
    assert_eq!(listening(herald, FaceIx::FRONT, kind::STEP), [2]);
    assert_eq!(listening(herald, FaceIx::FRONT, kind::DRAW), [0u8; 0]);
    assert_eq!(listening(herald, FaceIx::BACK, kind::DRAW), [0]);
    assert_eq!(listening(herald, FaceIx::DOWN, kind::ENTERS), [0u8; 0]);
    assert_eq!(listening(alpha, FaceIx::FRONT, kind::ENTERS), [0u8; 0]);
}

/// A trigger no code matches would never trigger: the table is not built, whether the
/// card is in a deck or is a token a deck's card makes.
#[test]
fn a_table_refuses_a_trigger_the_engine_cannot_match() {
    let refused = TableError::Trigger {
        card: "Greeter",
        unmatched: Unmatched::Subject,
    };
    for deck in [&[(1, "Greeter")], &[(1, "Host")]] {
        let table = GameTable::new(&TRIGGERS, [&[(4, "Alpha")], deck]);
        assert_eq!(table.err(), Some(refused.clone()));
    }
}

/// "Creatures you control have flying", which the layers have no code to apply, and
/// "as long as you do", which no static ability can mean.
static ANTHEM: CardDef = CardDef {
    name: "Anthem",
    faces: &[Face {
        abilities: &[Ability::static_ability(&LayerSpec {
            affected: Objects::All {
                zone: Zone::Battlefield,
                filter: Filter::Whose(&Players::You),
            },
            condition: None,
            modification: Modification::AddKeywords(Keywords::FLYING),
        })],
        ..Face::EMPTY
    }],
    tokens: &[],
};
/// Written as a literal: [`Ability::static_ability`] refuses it as it compiles.
static AFTERTHOUGHT: CardDef = CardDef {
    name: "Afterthought",
    faces: &[Face {
        abilities: &[Ability {
            layer: Some(&LayerSpec {
                affected: Objects::This,
                condition: Some(Condition::Did),
                modification: Modification::AddKeywords(Keywords::FLYING),
            }),
            ..Ability::NONE
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};
/// "Each player sacrifices a land that player controls": a filter read for a player
/// only a resolution knows.
static TOLL: CardDef = CardDef {
    name: "Toll",
    faces: &[Face {
        abilities: &[Ability::triggered(
            &Trigger::ENTERS,
            Behavior::Data(&Effect::ForEachPlayer {
                players: Players::EachPlayer,
                effect: &Effect::Sacrifice(Objects::All {
                    zone: Zone::Battlefield,
                    filter: Filter::All(&[
                        Filter::Types(Types::LAND),
                        Filter::Whose(&Players::ThatPlayer),
                    ]),
                }),
            }),
        )],
        ..Face::EMPTY
    }],
    tokens: &[],
};
static UNREAD: CardTable = CardTable::new(&[&ALPHA, &ANTHEM, &AFTERTHOUGHT, &TOLL]);

/// A static ability the layers cannot apply would change nothing, and a filter for
/// players the engine cannot read there would accept nothing: no table is built.
#[test]
fn a_table_refuses_what_the_engine_has_no_code_to_read() {
    let refused =
        |card: &'static str| GameTable::new(&UNREAD, [&[(4, "Alpha")], &[(1, card)]]).err();
    assert_eq!(
        refused("Anthem"),
        Some(TableError::Static {
            card: "Anthem",
            unapplied: Unapplied::Affected,
        })
    );
    assert_eq!(
        refused("Afterthought"),
        Some(TableError::Static {
            card: "Afterthought",
            unapplied: Unapplied::Resolving,
        })
    );
    assert_eq!(refused("Toll"), Some(TableError::Players { card: "Toll" }));
}

#[test]
fn get_finds_each_name_and_misses_unknown() {
    assert!(std::ptr::eq(TABLE.get("Beta").unwrap(), &raw const BETA));
    assert!(TABLE.get("Gamma").is_none());
    assert!(TABLE.get("").is_none());
}

#[test]
fn get_returns_the_first_definition_of_a_name() {
    assert!(std::ptr::eq(TABLE.get("Alpha").unwrap(), &raw const ALPHA));
    assert_eq!(TABLE.cards().len(), 3);
}
