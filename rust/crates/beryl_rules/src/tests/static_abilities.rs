//! Static abilities written as data no card has yet: each entry is read for what it says,
//! through the path the cards' own instances take, and what the engine has no path for is
//! refused as the card table is built.

use beryl_card::effect::build::{TARGET, YOU, draw, lose_game, modify, sequence, win_game};
use beryl_card::{
    Amount, Comparison, Condition, Duration, Effect, Governed, Objects, Players, RuleChange,
    SpecIx, When,
};
use beryl_kernel::replacement::Unreplaced;
use beryl_kernel::rule_changes::Unchecked;
use beryl_kernel::static_ability::Unapplied;
use beryl_kernel::table::TableError;

use crate::card::{
    Ability, AbilityKind, Behavior, CardDef, Face, LayerSpec, Modification, Replacement, Types,
};
use crate::filter::Filter;
use crate::game::Game;
use crate::ids::{Seat, Slot};
use crate::state::{TurnStep, Zone, ZoneKey};
use crate::{CardTable, GameTable, Outcome, Profile, Scratch, State, drawing, layers, rule_change};

use super::LAND;
use super::events::{find, with_ctx};

/// "The number of creatures you control".
pub(super) static CREATURES: Amount = Amount::Count(&Objects::All {
    zone: Zone::Battlefield,
    filter: Filter::All(&[Filter::Types(Types::CREATURE), Filter::Whose(&Players::You)]),
});

/// "The number of artifacts you control".
static ARTIFACTS: Amount = Amount::Count(&Objects::All {
    zone: Zone::Battlefield,
    filter: Filter::All(&[Filter::Types(Types::ARTIFACT), Filter::Whose(&Players::You)]),
});

/// A 0/3 creature with these abilities.
const fn creature(abilities: &'static [Ability]) -> Face {
    Face {
        types: Types::CREATURE,
        power: Some(0),
        toughness: Some(3),
        abilities,
        ..Face::EMPTY
    }
}

/// A static ability of the permanent itself, with no condition, that sets its power.
const fn power_is(amount: &'static Amount) -> LayerSpec {
    LayerSpec {
        affected: Objects::This,
        condition: None,
        modification: Modification::SetPower(amount),
    }
}

/// "This creature's power is equal to twice the number of artifacts you control."
static SMITH: CardDef = CardDef {
    name: "Test Smith",
    faces: &[creature(&[Ability::static_ability(&power_is(
        &Amount::Times(2, &ARTIFACTS),
    ))])],
    tokens: &[],
};

/// "Enchanted creature's power is equal to the number of artifacts you control": not
/// characteristic-defining, so layer 7b's.
static MANTLE: CardDef = CardDef {
    name: "Test Mantle",
    faces: &[creature(&[Ability::static_ability(&LayerSpec {
        affected: Objects::AttachedTo,
        ..power_is(&ARTIFACTS)
    })])],
    tokens: &[],
};

/// "This creature's power is equal to the number of cards in your hand": nothing the layers
/// have in hand as they apply.
static SCHOLAR: CardDef = CardDef {
    name: "Test Scholar",
    faces: &[creature(&[Ability::static_ability(&power_is(
        &Amount::Count(&Objects::All {
            zone: Zone::Hand,
            filter: Filter::Whose(&Players::You),
        }),
    ))])],
    tokens: &[],
};

/// "Target creature's power becomes the number of artifacts you control until end of
/// turn": a number locked in as the effect begins.
static REFORGE: CardDef = CardDef {
    name: "Test Reforge",
    faces: &[Face {
        types: Types::INSTANT,
        abilities: &[Ability {
            body: Behavior::Data(&sequence(&[
                draw(YOU, 1),
                modify(
                    TARGET,
                    Modification::SetPower(&ARTIFACTS),
                    Duration::EndOfTurn,
                ),
            ])),
            ..Ability::NONE
        }],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static POWER_CARDS: CardTable =
    CardTable::new(&[&LAND, &DOOM, &SMITH, &MANTLE, &SCHOLAR, &REFORGE]);

/// [CR 613.4a](https://yawgatog.com/resources/magic-rules/#R6134a): "Layer 7a: Effects from
/// characteristic-defining abilities that define power and/or toughness are applied." The
/// amount is whatever the ability says, read each time the layers apply and for the
/// permanent's controller.
#[test]
fn a_defined_power_is_any_amount_the_layers_read_as_they_apply() {
    let deck: &[(u8, &str)] = &[(1, "Test Smith"), (2, "Test Doom"), (7, "Test Land")];
    let t = GameTable::new(&POWER_CARDS, [deck, deck]).unwrap();
    let mut s = State::new(&t, 3, Profile::EXACT).unwrap();
    s.board().begin_at(1, Seat::P0, TurnStep::PrecombatMain);
    let enters = |s: &mut State, player, card| {
        let slot = find(s, &t, player, card);
        s.board().place(slot, Zone::Battlefield).unwrap();
        layers::refresh(s, &t, &mut crate::event::Sink::off()).unwrap();
        slot
    };
    let smith = enters(&mut s, Seat::P0, "Test Smith");
    assert_eq!(s.derived(smith).power, 0);
    // An artifact its controller controls counts twice, and an opponent's not at all.
    enters(&mut s, Seat::P0, "Test Doom");
    assert_eq!(s.derived(smith).power, 2);
    enters(&mut s, Seat::P1, "Test Doom");
    assert_eq!(s.derived(smith).power, 2);
    enters(&mut s, Seat::P0, "Test Doom");
    assert_eq!(s.derived(smith).power, 4);
}

/// The card table refuses a power set to an amount where the engine has no path for it:
/// layer 7b, an amount locked in by a resolving effect, and an amount the layers cannot
/// read.
#[test]
fn a_table_refuses_a_set_power_the_engine_cannot_do() {
    for (card, unapplied) in [
        ("Test Mantle", Unapplied::SetPower),
        ("Test Reforge", Unapplied::SetPower),
        ("Test Scholar", Unapplied::Amount),
    ] {
        let table = GameTable::new(&POWER_CARDS, [&[(1, card)], &[(1, "Test Land")]]);
        assert_eq!(table.err(), Some(TableError::Static { card, unapplied }));
    }
}

/// The face of a permanent with these abilities, and no other characteristic.
const fn face(abilities: &'static [Ability]) -> Face {
    Face {
        types: Types::ARTIFACT,
        abilities,
        ..Face::EMPTY
    }
}

/// "If you would draw a card, you lose the game instead": no condition, and a loss.
static DOOM: CardDef = CardDef {
    name: "Test Doom",
    faces: &[face(&[Ability {
        body: Behavior::Data(&lose_game(YOU)),
        ..Ability::replacing(Replacement::WouldDraw)
    }])],
    tokens: &[],
};

/// "If you would draw a card, each opponent wins the game instead."
static GIFT: CardDef = CardDef {
    name: "Test Gift",
    faces: &[face(&[Ability {
        body: Behavior::Data(&win_game(Players::EachOpponent)),
        ..Ability::replacing(Replacement::WouldDraw)
    }])],
    tokens: &[],
};

/// "If you would draw a card, draw two cards instead": the engine replaces a draw only by
/// ending the game.
static WINDFALL: CardDef = CardDef {
    name: "Test Windfall",
    faces: &[face(&[Ability {
        body: Behavior::Data(&draw(YOU, 2)),
        ..Ability::replacing(Replacement::WouldDraw)
    }])],
    tokens: &[],
};

/// "If you would draw a card, target player wins the game instead": a static ability has
/// no target.
static FAVOR: CardDef = CardDef {
    name: "Test Favor",
    faces: &[face(&[Ability {
        body: Behavior::Data(&Effect::WinGame(Players::Target(SpecIx::new(0)))),
        ..Ability::replacing(Replacement::WouldDraw)
    }])],
    tokens: &[],
};

static DRAW_CARDS: CardTable = CardTable::new(&[&LAND, &DOOM, &GIFT, &WINDFALL, &FAVOR]);

/// A game in the first player's main phase with `card` on the battlefield under their
/// control.
fn with_permanent(card: &'static str) -> (GameTable, State, Slot) {
    with_permanent_of(&DRAW_CARDS, card)
}

/// [`with_permanent`], of the cards in `cards`.
fn with_permanent_of(cards: &'static CardTable, card: &'static str) -> (GameTable, State, Slot) {
    let deck: &[(u8, &str)] = &[(1, card), (9, "Test Land")];
    let t = GameTable::new(cards, [deck, deck]).unwrap();
    let mut s = State::new(&t, 3, Profile::EXACT).unwrap();
    s.board().begin_at(1, Seat::P0, TurnStep::PrecombatMain);
    let permanent = find(&s, &t, Seat::P0, card);
    s.board().place(permanent, Zone::Battlefield).unwrap();
    layers::refresh(&mut s, &t, &mut crate::event::Sink::off()).unwrap();
    (t, s, permanent)
}

fn hand(s: &State, player: Seat) -> u32 {
    s.zone(ZoneKey::Of(player, Zone::Hand)).count()
}

/// [CR 614.11](https://yawgatog.com/resources/magic-rules/#R61411): "Some effects replace
/// card draws." A replacement with no condition replaces a draw from a full library, what
/// it says instead is done whichever it is, and it is its controller's draws it replaces.
#[test]
fn a_draw_is_replaced_by_what_any_replacement_says_instead() {
    for (card, winner) in [("Test Doom", Seat::P1), ("Test Gift", Seat::P1)] {
        let (t, mut s, _) = with_permanent(card);
        let mut x = Scratch::new();
        // The opponent's draw is their own.
        let before = hand(&s, Seat::P1);
        drawing::draw(&mut Game::new(&mut s, &t, &mut x.events), Seat::P1, 1).unwrap();
        assert_eq!((hand(&s, Seat::P1), s.outcome()), (before + 1, None));
        // The controller's is replaced: no card is drawn, and the game ends as it says.
        let before = hand(&s, Seat::P0);
        drawing::draw(&mut Game::new(&mut s, &t, &mut x.events), Seat::P0, 2).unwrap();
        assert_eq!(hand(&s, Seat::P0), before, "{card}");
        assert_eq!(s.outcome(), Some(Outcome::Win(winner)), "{card}");
    }
}

/// The card table refuses a draw replacement the engine has no code to do, so none runs as
/// a draw that silently happens anyway.
#[test]
fn a_table_refuses_a_draw_replacement_the_engine_cannot_do() {
    for (card, unreplaced) in [
        ("Test Windfall", Unreplaced::Instead),
        ("Test Favor", Unreplaced::Players),
    ] {
        let table = GameTable::new(&DRAW_CARDS, [&[(1, card)], &[(1, "Test Land")]]);
        assert_eq!(
            table.err(),
            Some(TableError::Replacement { card, unreplaced })
        );
    }
}

/// An instant, a sorcery and a creature spell, as faces a rule change is asked about.
const fn spell(types: Types) -> Face {
    Face {
        types,
        ..Face::EMPTY
    }
}
static INSTANT: Face = spell(Types::INSTANT);
static SORCERY: Face = spell(Types::SORCERY);
static CREATURE: Face = spell(Types::CREATURE);

/// A static ability that makes `rule`.
const fn makes(rule: &'static RuleChange) -> Ability {
    Ability {
        kind: AbilityKind::Static,
        rule: Some(rule),
        ..Ability::NONE
    }
}

/// "Each player can cast spells only any time they could cast a sorcery."
static CURFEW_RULE: RuleChange = RuleChange {
    players: Players::EachPlayer,
    what: Governed::CastSpells(Filter::Any),
    when: When::OnlyAsSorcery,
};
static CURFEW: CardDef = CardDef {
    name: "Test Curfew",
    faces: &[face(&[makes(&CURFEW_RULE)])],
    tokens: &[],
};

/// "You may cast creature spells as though they had flash."
static AMBUSH_RULE: RuleChange = RuleChange {
    players: Players::You,
    what: Governed::CastSpells(Filter::Types(Types::CREATURE)),
    when: When::AsInstant,
};
static AMBUSH: CardDef = CardDef {
    name: "Test Ambush",
    faces: &[face(&[makes(&AMBUSH_RULE)])],
    tokens: &[],
};

/// "Until end of turn, each opponent can cast instant spells only any time they could cast
/// a sorcery.", which an ability's effect starts.
static HUSH_RULE: RuleChange = RuleChange {
    players: Players::EachOpponent,
    what: Governed::CastSpells(Filter::Types(Types::INSTANT)),
    when: When::OnlyAsSorcery,
};
static HUSH: CardDef = CardDef {
    name: "Test Hush",
    faces: &[face(&[Ability {
        kind: AbilityKind::Activated,
        rule: Some(&HUSH_RULE),
        ..Ability::NONE
    }])],
    tokens: &[],
};

/// "You may activate this permanent's loyalty abilities any time you could cast an
/// instant.", as long as a condition of the ability that says it holds.
static REGENT_RULE: RuleChange = RuleChange {
    players: Players::You,
    what: Governed::ActivateLoyaltyAbilities(Objects::This),
    when: When::AsInstant,
};
/// "As long as you have no cards in hand": read of its controller's own zone.
static REGENT: CardDef = CardDef {
    name: "Test Regent",
    faces: &[face(&[Ability {
        condition: Some(&Condition::Compare(
            Amount::Count(&Objects::All {
                zone: Zone::Hand,
                filter: Filter::Whose(&Players::You),
            }),
            Comparison::Equal,
            Amount::Fixed(0),
        )),
        ..makes(&REGENT_RULE)
    }])],
    tokens: &[],
};
/// "As long as this permanent is tapped": read in a scene.
static SLEEPER_RULE: RuleChange = REGENT_RULE;
static SLEEPER: CardDef = CardDef {
    name: "Test Sleeper",
    faces: &[face(&[Ability {
        condition: Some(&Condition::Is(Objects::This, Filter::Tapped)),
        ..makes(&SLEEPER_RULE)
    }])],
    tokens: &[],
};

static RULE_CARDS: CardTable = CardTable::new(&[&LAND, &CURFEW, &AMBUSH, &HUSH, &REGENT, &SLEEPER]);

/// [CR 613.11](https://yawgatog.com/resources/magic-rules/#R61311): "Some continuous effects
/// affect game rules rather than objects." A static ability changes the rule for the
/// players it names, read for its controller, and for the spells its filter accepts.
#[test]
fn a_static_rule_change_governs_the_players_and_the_spells_it_names() {
    let (t, s, _) = with_permanent_of(&RULE_CARDS, "Test Curfew");
    for player in Seat::BOTH {
        assert!(rule_change::cast_only_as_sorcery(&s, &t, player, &INSTANT));
        assert!(!rule_change::cast_as_instant(&s, &t, player, &SORCERY));
    }
    let (t, s, _) = with_permanent_of(&RULE_CARDS, "Test Ambush");
    assert!(rule_change::cast_as_instant(&s, &t, Seat::P0, &CREATURE));
    assert!(!rule_change::cast_as_instant(&s, &t, Seat::P0, &SORCERY));
    assert!(!rule_change::cast_as_instant(&s, &t, Seat::P1, &CREATURE));
    assert!(!rule_change::cast_only_as_sorcery(
        &s,
        &t,
        Seat::P1,
        &INSTANT
    ));
}

/// A resolving ability starts the rule change it names for the players the rule names,
/// read for its controller as it resolves, for as long as it says (CR 611.2a).
#[test]
fn an_effect_starts_any_rule_change_its_ability_names() {
    let (t, mut s, hush) = with_permanent_of(&RULE_CARDS, "Test Hush");
    let mut x = Scratch::new();
    with_ctx(&t, &mut s, &mut x, hush, |ctx| {
        ctx.change_rule(&HUSH_RULE, crate::state::Duration::EndOfTurn);
    });
    assert!(rule_change::cast_only_as_sorcery(
        &s,
        &t,
        Seat::P1,
        &INSTANT
    ));
    assert!(!rule_change::cast_only_as_sorcery(
        &s,
        &t,
        Seat::P1,
        &CREATURE
    ));
    assert!(!rule_change::cast_only_as_sorcery(
        &s,
        &t,
        Seat::P0,
        &INSTANT
    ));
    s.expire(crate::state::Duration::EndOfTurn);
    layers::refresh(&mut s, &t, &mut x.events).unwrap();
    assert!(!rule_change::cast_only_as_sorcery(
        &s,
        &t,
        Seat::P1,
        &INSTANT
    ));
}

/// A permanent's own rule change applies as long as the condition of the ability that
/// makes it holds, whatever the condition: one read of its controller's zones, and one
/// read in a scene.
#[test]
fn a_rule_change_of_a_permanents_own_applies_as_long_as_its_condition_holds() {
    let (t, mut s, regent) = with_permanent_of(&RULE_CARDS, "Test Regent");
    assert_eq!(
        hand(&s, Seat::P0) == 0,
        rule_change::loyalty_as_instant(&s, &t, regent)
    );
    for card in s.in_candidate_order(s.zone(ZoneKey::Of(Seat::P0, Zone::Hand))) {
        s.board().place(card, Zone::Graveyard).unwrap();
    }
    assert!(rule_change::loyalty_as_instant(&s, &t, regent));

    let (t, mut s, sleeper) = with_permanent_of(&RULE_CARDS, "Test Sleeper");
    assert!(!rule_change::loyalty_as_instant(&s, &t, sleeper));
    s.board().set_tapped(sleeper, true);
    assert!(rule_change::loyalty_as_instant(&s, &t, sleeper));
}

/// "Target player can cast spells only any time they could cast a sorcery": a static
/// ability has no target.
static NAMED_RULE: RuleChange = RuleChange {
    players: Players::Target(SpecIx::new(0)),
    ..CURFEW_RULE
};
/// "Each opponent can cast tapped spells only ...": more than a spell's own
/// characteristics.
static TAPPED_RULE: RuleChange = RuleChange {
    what: Governed::CastSpells(Filter::Tapped),
    ..CURFEW_RULE
};
/// "You may activate loyalty abilities of planeswalkers you control any time you could
/// cast an instant": other permanents' abilities.
static COURT_RULE: RuleChange = RuleChange {
    what: Governed::ActivateLoyaltyAbilities(Objects::All {
        zone: Zone::Battlefield,
        filter: Filter::Types(Types::PLANESWALKER),
    }),
    ..REGENT_RULE
};
/// "You may activate this permanent's loyalty abilities only any time you could cast a
/// sorcery": the timing the rules have already.
static IDLE_RULE: RuleChange = RuleChange {
    when: When::OnlyAsSorcery,
    ..REGENT_RULE
};
static REFUSED_RULES: [(&str, Ability, Unchecked); 6] = [
    ("Test Named", makes(&NAMED_RULE), Unchecked::Players),
    ("Test Tapped", makes(&TAPPED_RULE), Unchecked::Filter),
    ("Test Court", makes(&COURT_RULE), Unchecked::Rule),
    ("Test Idle", makes(&IDLE_RULE), Unchecked::Rule),
    // A rule for players, as long as a condition the layers do not read holds.
    (
        "Test Vigil",
        Ability {
            condition: Some(&Condition::EnteredThisTurn(Objects::This)),
            ..makes(&CURFEW_RULE)
        },
        Unchecked::Condition,
    ),
    // An effect that would start a rule change asked of a permanent.
    (
        "Test Decree",
        Ability {
            kind: AbilityKind::Activated,
            ..makes(&REGENT_RULE)
        },
        Unchecked::Rule,
    ),
];

/// The card table refuses a rule change the engine has no check for, so none is in a game
/// and changes nothing.
#[test]
fn a_table_refuses_a_rule_change_the_engine_cannot_check() {
    for (name, ability, unchecked) in &REFUSED_RULES {
        // A card table is a static, as its cards are: one made for the test and never
        // freed.
        let abilities: &'static [Ability] = std::slice::from_ref(ability);
        let card: &'static CardDef = Box::leak(Box::new(CardDef {
            name,
            faces: Box::leak(Box::new([face(abilities)])),
            tokens: &[],
        }));
        let cards: &'static [&'static CardDef] = Box::leak(Box::new([&LAND, card]));
        let cards: &'static CardTable = Box::leak(Box::new(CardTable::new(cards)));
        let table = GameTable::new(cards, [&[(1, *name)], &[(1, "Test Land")]]);
        let refused = TableError::Rule {
            card: name,
            unchecked: *unchecked,
        };
        assert_eq!(table.err(), Some(refused), "{name}");
    }
}
