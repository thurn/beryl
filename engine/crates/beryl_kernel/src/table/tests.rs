//! Tests of the card tables.

use beryl_card::{
    Ability, Effect, Filter, Keywords, LayerSpec, ManaColor, ManaSet, Modification, Objects,
    Players, Subject, Trigger, TriggerEvent, TurnStep, Types, Zone,
};

use super::*;
use crate::event::{EventKind, Listens, Replaces};
use crate::format::Format;
use crate::rule_changes::fixtures::{LIMIT, WALKER};

static ALPHA: CardDef = CardDef {
    name: "Alpha",
    shape: beryl_card::definition::Shape::Single,
    faces: &[],
    tokens: &[],
};
static BETA: CardDef = CardDef {
    name: "Beta",
    shape: beryl_card::definition::Shape::Single,
    faces: &[],
    tokens: &[],
};
/// A second definition under Alpha's name.
static ALPHA_AGAIN: CardDef = CardDef {
    name: "Alpha",
    shape: beryl_card::definition::Shape::Single,
    faces: &[],
    tokens: &[],
};
static TABLE: CardTable = CardTable::new(&[&ALPHA, &BETA, &ALPHA_AGAIN]);

static WALKERS: CardTable = CardTable::new(&[&ALPHA, &WALKER]);

#[test]
fn a_game_has_the_capabilities_of_its_cards_and_no_others() {
    let all = [Capability::EnteredThisTurn, Capability::LoyaltyActivated];
    let plain = GameTable::new(
        Format::CONSTRUCTED,
        &WALKERS,
        [&[(4, "Alpha")], &[(4, "Alpha")]],
    )
    .unwrap();
    assert!(all.iter().all(|&c| !plain.can(c)));
    // One deck's card is enough: both players' games read the same table.
    let walker = GameTable::new(
        Format::CONSTRUCTED,
        &WALKERS,
        [&[(4, "Alpha")], &[(1, "Walker")]],
    )
    .unwrap();
    assert!(all.iter().all(|&c| walker.can(c)));
}

#[test]
fn a_card_that_changes_a_rule_for_players_says_so_in_its_mask() {
    let t = GameTable::new(
        Format::CONSTRUCTED,
        &WALKERS,
        [&[(4, "Alpha")], &[(1, "Walker")]],
    )
    .unwrap();
    let (alpha, walker) = (CardIx::from_index(0), CardIx::from_index(1));
    assert!(!t.replaces(alpha).changes_player_rules());
    assert_eq!(t.replaces(walker), Replaces::PLAYER_RULE_CHANGE);
    assert_eq!(t.replaces_any(), Replaces::PLAYER_RULE_CHANGE);
    // A rule change is no event a trigger listens to.
    assert!(t.listens_any().is_empty());
    // Its rule about itself is none of the players' rules.
    let limit = t.rule_id(&LIMIT).unwrap();
    assert_eq!(
        t.may_have_rules(walker).common(t.player_rules()),
        Rules::NONE.with(limit)
    );
}

/// "When this enters" and "at the beginning of your upkeep" on the front, "whenever you
/// draw a card" on the back.
static HERALD: CardDef = CardDef {
    name: "Herald",
    shape: beryl_card::definition::Shape::Single,
    faces: &[
        Face {
            abilities: &[
                Ability::triggered(&Trigger::ENTERS).effect(&Effect::NOTHING),
                Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)),
                Ability::triggered(&Trigger::step(TurnStep::Upkeep, Players::You))
                    .effect(&Effect::NOTHING),
                Ability::triggered(&Trigger::ENTERS).effect(&Effect::NOTHING),
            ],
            ..Face::EMPTY
        },
        Face {
            abilities: &[Ability::triggered(&Trigger::draws(Players::You)).effect(&Effect::NOTHING)],
            ..Face::EMPTY
        },
    ],
    tokens: &[],
};
/// "Whenever a creature attacks" as a trigger of one attacking creature, which the engine
/// has no code to match, on a token a card makes.
static GREETER: CardDef = CardDef {
    name: "Greeter",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        abilities: &[Ability::triggered(&Trigger::of(
            TriggerEvent::Attacks,
            Subject::A(Filter::Types(Types::CREATURE)),
        ))
        .effect(&Effect::NOTHING)],
        ..Face::EMPTY
    }],
    tokens: &[],
};
static HOST: CardDef = CardDef {
    name: "Host",
    shape: beryl_card::definition::Shape::Single,
    faces: &[],
    tokens: &[&GREETER],
};
static TRIGGERS: CardTable = CardTable::new(&[&ALPHA, &HERALD, &GREETER, &HOST]);

#[test]
fn an_event_reads_only_the_listeners_of_its_kind_on_the_face_that_is_up() {
    let t = GameTable::new(
        Format::CONSTRUCTED,
        &TRIGGERS,
        [&[(4, "Alpha")], &[(1, "Herald")]],
    )
    .unwrap();
    let (alpha, herald) = (CardIx::from_index(0), CardIx::from_index(1));
    let listening = |card, face, kind| -> Vec<u8> {
        t.listeners(card, face, kind)
            .map(|l| l.ability.get())
            .collect()
    };
    assert_eq!(
        t.listens(herald),
        Listens::event(EventKind::Enters)
            | Listens::event(EventKind::Step)
            | Listens::event(EventKind::Draw)
    );
    assert_eq!(t.listens_any(), t.listens(herald));
    assert_eq!(listening(herald, FaceIx::FRONT, EventKind::Enters), [0, 3]);
    assert_eq!(listening(herald, FaceIx::FRONT, EventKind::Step), [2]);
    assert_eq!(listening(herald, FaceIx::FRONT, EventKind::Draw), [0u8; 0]);
    assert_eq!(listening(herald, FaceIx::BACK, EventKind::Draw), [0]);
    assert_eq!(listening(herald, FaceIx::DOWN, EventKind::Enters), [0u8; 0]);
    assert_eq!(listening(alpha, FaceIx::FRONT, EventKind::Enters), [0u8; 0]);
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
        let table = GameTable::new(Format::CONSTRUCTED, &TRIGGERS, [&[(4, "Alpha")], deck]);
        assert_eq!(table.err(), Some(refused.clone()));
    }
}

/// "Cards in your hand have flying": cards in a hand have no layer results, so the layers
/// have no code to apply it.
static ANTHEM: CardDef = CardDef {
    name: "Anthem",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        abilities: &[Ability::static_ability(&LayerSpec {
            affected: Objects::All {
                zone: Zone::Hand,
                filter: Filter::Whose(&Players::You),
            },
            condition: None,
            modifications: &[Modification::AddKeywords(Keywords::FLYING)],
        })],
        ..Face::EMPTY
    }],
    tokens: &[],
};
/// "Each player sacrifices a land that player controls": a filter read for a player
/// only a resolution knows.
static TOLL: CardDef = CardDef {
    name: "Toll",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        abilities: &[
            Ability::triggered(&Trigger::ENTERS).effect(&Effect::ForEachPlayer {
                players: Players::EachPlayer,
                effect: &Effect::Sacrifice(Objects::All {
                    zone: Zone::Battlefield,
                    filter: Filter::All(&[
                        Filter::Types(Types::LAND),
                        Filter::Whose(&Players::ThatPlayer),
                    ]),
                }),
            }),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};
/// "Creatures you control have 'When this creature dies, ...'": a granted triggered ability
/// that looks back at a permanent that left, which the engine does not match as a granted one.
static HERALDRY: CardDef = CardDef {
    name: "Heraldry",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        abilities: &[Ability::static_ability(&LayerSpec {
            affected: Objects::All {
                zone: Zone::Battlefield,
                filter: Filter::All(&[
                    Filter::Types(Types::CREATURE),
                    Filter::Whose(&Players::You),
                ]),
            },
            condition: None,
            modifications: &[Modification::grant(0)],
        })],
        grants: &[Ability::triggered(&Trigger::DIES).effect(&Effect::NOTHING)],
        ..Face::EMPTY
    }],
    tokens: &[],
};
static UNREAD: CardTable = CardTable::new(&[&ALPHA, &ANTHEM, &TOLL, &HERALDRY]);

/// A static ability the layers cannot apply would change nothing, a filter for players the
/// engine cannot read there would accept nothing, and a granted "dies" trigger would never
/// trigger: no table is built.
#[test]
fn a_table_refuses_what_the_engine_has_no_code_to_read() {
    let refused = |card: &'static str| {
        GameTable::new(
            Format::CONSTRUCTED,
            &UNREAD,
            [&[(4, "Alpha")], &[(1, card)]],
        )
        .err()
    };
    assert_eq!(
        refused("Anthem"),
        Some(TableError::Static {
            card: "Anthem",
            unapplied: Unapplied::Affected,
        })
    );
    assert_eq!(refused("Toll"), Some(TableError::Players { card: "Toll" }));
    assert_eq!(
        refused("Heraldry"),
        Some(TableError::Granted { card: "Heraldry" })
    );
}

/// "Create a Beta token", by a card that lists none, and by one that lists it.
static MAKER: CardDef = CardDef {
    name: "Maker",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        abilities: &[
            Ability::triggered(&Trigger::ENTERS).effect(&Effect::CreateTokens {
                player: Players::You,
                token: &BETA,
                count: beryl_card::Amount::Fixed(1),
            }),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};
static LISTING_MAKER: CardDef = CardDef {
    name: "Listing Maker",
    shape: beryl_card::definition::Shape::Single,
    faces: MAKER.faces,
    tokens: &[&BETA],
};
static MAKERS: CardTable = CardTable::new(&[&ALPHA, &MAKER, &LISTING_MAKER]);

/// A token its card does not list is no card of the game, and creating it would stop the
/// game where it happened: no table is built.
#[test]
fn a_table_refuses_a_card_that_creates_what_it_does_not_list() {
    let table = |card: &'static str| {
        GameTable::new(
            Format::CONSTRUCTED,
            &MAKERS,
            [&[(4, "Alpha")], &[(1, card)]],
        )
    };
    assert_eq!(
        table("Maker").err(),
        Some(TableError::Unlisted {
            card: "Maker",
            creates: "Beta"
        })
    );
    assert!(table("Listing Maker").is_ok());
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

#[test]
fn determinization_preserves_sideboard_admissions_when_the_cards_are_shuffled() {
    use crate::State;
    use crate::observation::Observation;
    use crate::profile::Profile;
    use crate::rng::SplitMix64;

    let player = Seat::P0;
    let t = GameTable::with_sideboards(
        Format::CONSTRUCTED,
        &TABLE,
        [&[(2, "Alpha")], &[]],
        [&[(2, "Beta")], &[]],
    )
    .unwrap();
    let mut state = State::new(&t, 0, Profile::EXACT).unwrap();
    assert_eq!(state.sideboard(&t, player).count(), 2);
    assert_eq!(state.library(player).len(), 2);
    let slot = state
        .prepare_sideboard_card(&t, player, 0)
        .unwrap()
        .unwrap();
    assert_eq!(state.prepare_sideboard_card(&t, player, 0).unwrap(), None);
    state.board().place(slot, Zone::Hand).unwrap();
    state.board().place(slot, Zone::Library).unwrap();
    state.shuffle_library(player).unwrap();

    let beta = t.index_named::<CardIx>("Beta").unwrap();
    let mut rng = SplitMix64::new(17);
    let mut places = std::collections::BTreeSet::new();
    for _ in 0..12 {
        let dealt = Observation::new(&state, Seat::P1).determinize(&t, &mut rng);
        assert_eq!(dealt.sideboard(&t, player).collect::<Vec<_>>(), [(1, beta)]);
        let cards = dealt.library(player);
        assert_eq!(cards.len(), 3);
        let mut matches = cards
            .iter()
            .filter(|&&slot| dealt.object(slot).card == beta);
        places.insert(*matches.next().unwrap());
        assert_eq!(matches.count(), 0);
        assert_eq!(dealt.widen().sideboard(&t, player).count(), 1);
    }
    assert_eq!(places.len(), 3);
}

#[test]
fn a_restart_redeals_sideboard_cards_that_entered_the_game() {
    use crate::State;
    use crate::bitboard::Bitboard;
    use crate::profile::Profile;
    use crate::state::ZoneKey;

    let player = Seat::P0;
    let table = GameTable::with_sideboards(
        Format::CONSTRUCTED,
        &TABLE,
        [&[(2, "Alpha")], &[]],
        [&[(2, "Beta")], &[]],
    )
    .unwrap();
    let mut state = State::new(&table, 0, Profile::EXACT).unwrap();
    state.board().begin_at(9, player, TurnStep::PrecombatMain);
    let admitted = state
        .prepare_sideboard_card(&table, player, 0)
        .unwrap()
        .unwrap();
    state.board().place(admitted, Zone::Hand).unwrap();
    state.board().set_life(player, 5);
    let graveyard = state.library(player)[0];
    state.board().place(graveyard, Zone::Graveyard).unwrap();

    state.restart(&table, Seat::P1, Bitboard::EMPTY).unwrap();

    assert_eq!(state.sideboard(&table, player).count(), 1);
    let mut names: Vec<_> = state
        .library(player)
        .iter()
        .map(|&slot| state.definition_name(&table, slot))
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["Alpha", "Alpha", "Beta"]);
    assert!(state.zone(ZoneKey::Of(player, Zone::Hand)).is_empty());
    assert!(state.graveyard(player).is_empty());
    assert_eq!(state.player(player).life(), 20);
    assert_eq!(state.turn().starting_player(), Seat::P1);
    assert_eq!(state.turns_for_limit(), 0);
    assert_eq!(state.elapsed_turns(), 9);
    assert_eq!(state.check_invariants(&table), Ok(()));
}

#[test]
fn a_shared_deck_table_takes_one_list_and_refuses_a_second_or_a_sideboard() {
    use crate::State;
    use crate::profile::Profile;

    let t = GameTable::new(
        Format::SHARED_DECK,
        &TABLE,
        [&[(3, "Alpha"), (2, "Beta")], &[]],
    )
    .unwrap();
    assert_eq!(t.deck_len(Seat::P0), 5);
    assert_eq!(t.deck::<CardIx>(Seat::P1).len(), 0);
    assert_eq!(t.sideboard::<CardIx>(Seat::P0).len(), 0);

    assert_eq!(
        GameTable::new(
            Format::SHARED_DECK,
            &TABLE,
            [&[(3, "Alpha")], &[(2, "Beta")]]
        )
        .err(),
        Some(TableError::SecondDeck { count: 2 })
    );
    assert_eq!(
        GameTable::with_sideboards(
            Format::SHARED_DECK,
            &TABLE,
            [&[(3, "Alpha")], &[]],
            [&[], &[(1, "Beta")]],
        )
        .err(),
        Some(TableError::SideboardTooLarge {
            player: Seat::P1,
            count: 1
        })
    );

    let state = State::new(&t, 0, Profile::EXACT).unwrap();
    assert!(state.shares_zones());
    assert_eq!(state.library(Seat::P0), state.library(Seat::P1));
    assert_eq!(state.library(Seat::P0).len(), 5);
}

/// A combat requirement, restriction or cost to attack in each shape the declarations read:
/// "All creatures able to block this creature do so.", "All creatures able to block enchanted
/// creature do so.", "Target creature must be blocked this turn if able.", "This creature
/// attacks each combat if able.", "This creature can't attack or block alone.", "No more than
/// one creature can attack each combat." and "Creatures can't attack you unless their
/// controller pays {2} for each creature they control that's attacking you."
mod combat {
    use beryl_card::{
        Ability, CardDef, Cost, Face, Filter, Governed, ManaCost, Objects, Players, RuleChange,
        SpecIx, TargetSpec, Types, When,
    };

    use crate::format::Format;
    use crate::rule_changes::Unchecked;
    use crate::table::{Capability, CardTable, GameTable, TableError};

    const fn rule(players: Players, what: Governed) -> RuleChange {
        RuleChange {
            players,
            what,
            when: When::AsUsual,
        }
    }

    static TAUNT: RuleChange = rule(Players::You, Governed::MustBlock(Objects::This));
    static LURE: RuleChange = rule(Players::You, Governed::MustBlock(Objects::AttachedTo));
    static PREY: RuleChange = rule(
        Players::You,
        Governed::MustBeBlocked(Objects::Target(SpecIx::new(0))),
    );
    static CYCLOPS: RuleChange = rule(Players::You, Governed::MustAttack(Objects::This));
    static FLUNKIES: RuleChange = rule(
        Players::You,
        Governed::Alone {
            attack: true,
            block: true,
            creatures: Objects::This,
        },
    );
    static ARBITER: RuleChange = rule(
        Players::EachPlayer,
        Governed::CombatLimit {
            attackers: Some(1),
            blockers: None,
        },
    );
    static PROPAGANDA: RuleChange = rule(Players::You, Governed::AttackCost(ManaCost::generic(2)));
    static PREY_TARGET: [TargetSpec; 1] = [TargetSpec::permanent(Filter::Types(Types::CREATURE))];

    static READ: [Ability; 7] = [
        Ability::rule_change(&TAUNT),
        Ability::rule_change(&LURE),
        Ability::activated(Cost::NONE)
            .targeting(&PREY_TARGET)
            .starting(&PREY),
        Ability::rule_change(&CYCLOPS),
        Ability::rule_change(&FLUNKIES),
        Ability::rule_change(&ARBITER),
        Ability::rule_change(&PROPAGANDA),
    ];

    /// "Enchanted creature attacks each combat if able": no check finds it.
    static ENCHANTED_ATTACKS: RuleChange =
        rule(Players::You, Governed::MustAttack(Objects::AttachedTo));
    /// "Each creature you control can't attack alone": of creatures other than its own.
    static ALL_ALONE: RuleChange = rule(
        Players::You,
        Governed::Alone {
            attack: true,
            block: false,
            creatures: Objects::All {
                zone: beryl_card::Zone::Battlefield,
                filter: Filter::Types(Types::CREATURE),
            },
        },
    );
    /// "No more than one creature can attack you each combat": a limit for one player.
    static YOUR_LIMIT: RuleChange = rule(
        Players::You,
        Governed::CombatLimit {
            attackers: Some(1),
            blockers: None,
        },
    );
    /// "Target creature attacks each combat if able", which an effect would start.
    static TARGET_ATTACKS: RuleChange = rule(
        Players::You,
        Governed::MustAttack(Objects::Target(SpecIx::new(0))),
    );

    static REFUSED: [Ability; 5] = [
        Ability::rule_change(&ENCHANTED_ATTACKS),
        Ability::rule_change(&ALL_ALONE),
        Ability::rule_change(&YOUR_LIMIT),
        // A static ability has no target to be about.
        Ability::rule_change(&PREY),
        Ability::activated(Cost::NONE)
            .targeting(&PREY_TARGET)
            .starting(&TARGET_ATTACKS),
    ];

    /// The table of a game with a creature whose one ability is `ability`.
    fn table_with(ability: &'static Ability) -> Result<GameTable, TableError> {
        let card: &'static CardDef = Box::leak(Box::new(CardDef {
            name: "Fighter",
            shape: beryl_card::definition::Shape::Single,
            faces: Box::leak(Box::new([Face {
                types: Types::CREATURE,
                power: Some(1),
                toughness: Some(1),
                abilities: std::slice::from_ref(ability),
                ..Face::EMPTY
            }])),
            tokens: &[],
        }));
        let cards: &'static CardTable =
            Box::leak(Box::new(CardTable::new(Box::leak(Box::new([card])))));
        GameTable::new(Format::CONSTRUCTED, cards, [&[(1, "Fighter")], &[]])
    }

    #[test]
    fn a_card_with_a_combat_rule_gives_its_game_the_capability() {
        for ability in &READ {
            let t = table_with(ability).unwrap();
            assert!(
                t.can(Capability::CombatRequirements),
                "{:?}",
                ability.rule()
            );
        }
        let plain = table_with(Box::leak(Box::new(Ability::rule_change(
            &crate::rule_changes::fixtures::LIMIT,
        ))))
        .unwrap();
        assert!(!plain.can(Capability::CombatRequirements));
    }

    #[test]
    fn a_combat_rule_in_a_shape_no_check_reads_is_refused() {
        for ability in &REFUSED {
            assert_eq!(
                table_with(ability).err(),
                Some(TableError::Rule {
                    card: "Fighter",
                    unchecked: Unchecked::Rule,
                }),
                "{:?}",
                ability.rule()
            );
        }
    }
}
