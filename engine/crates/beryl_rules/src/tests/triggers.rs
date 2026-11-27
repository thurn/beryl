//! A trigger is an event, a subject and players (`beryl_card::Trigger`): the products no
//! printed card of the pool says, each matched by the code that matches the pool's.

use beryl_card::effect::build::{gain_life, you};

use super::begin;
use super::events::{put, resolve_all};
use crate::bitboard::Bitboard;
use crate::card::{
    Ability, CardDef, Castings, Face, LayerSpec, Modification, Objects, Players, Subject, Subtype,
    Subtypes, Trigger, TriggerEvent, Types,
};
use crate::filter::Filter;
use crate::fixed::Fixed;
use crate::ids::Seat;
use crate::ids::Slot;
use crate::replacement::event::{self, Entry, Event, Reason};
use crate::state::{TurnStep, Zone, ZoneKey};
use crate::zone::Place;
use crate::{CardTable, Decklist, GameTable, Profile, Scratch, State};
use beryl_kernel::format::Format;
use beryl_kernel::two_player::TwoPlayerSeat as _;

/// A permanent with one triggered ability: "[trigger], you gain 1 life."
macro_rules! listener {
    ($name:ident, $text:literal, $trigger:expr) => {
        static $name: CardDef = CardDef {
            name: $text,
            shape: beryl_card::definition::Shape::Single,
            faces: &[Face {
                types: Types::ARTIFACT,
                abilities: &[Ability::triggered(&$trigger).effect(&gain_life(you(), 1))],
                ..Face::EMPTY
            }],
            tokens: &[],
        };
    };
}

static LAND: CardDef = CardDef {
    name: "Test Land",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::LAND,
        ..Face::EMPTY
    }],
    tokens: &[],
};
static BEAR: CardDef = CardDef {
    name: "Test Bear",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(2),
        toughness: Some(2),
        ..Face::EMPTY
    }],
    tokens: &[],
};
static KNIGHT: CardDef = CardDef {
    name: "Test Knight",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::CREATURE,
        subtypes: crate::card::SubtypeWords::new(Subtypes::of(&[Subtype::Knight])),
        power: Some(2),
        toughness: Some(2),
        ..Face::EMPTY
    }],
    tokens: &[],
};
static ARTIFACT: CardDef = CardDef {
    name: "Test Artifact",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ARTIFACT,
        ..Face::EMPTY
    }],
    tokens: &[],
};
// "Whenever an opponent casts a spell".
listener!(
    HECKLER,
    "Test Heckler",
    Trigger::casts(Players::EachOpponent, Filter::Any)
);
// "Whenever you cast a noncreature spell".
listener!(
    PURIST,
    "Test Purist",
    Trigger::casts(Players::You, Filter::Not(&Filter::Types(Types::CREATURE)))
);
// "At the beginning of each of your main phases".
listener!(
    METRONOME,
    "Test Metronome",
    Trigger::main_phase(Players::You)
);
// "Whenever a player draws a card".
listener!(ARCHIVE, "Test Archive", Trigger::draws(Players::EachPlayer));
// "Whenever you attack with one or more Knights".
listener!(
    MARSHAL,
    "Test Marshal",
    Trigger::of(
        TriggerEvent::PlayerAttacks,
        Subject::A(Filter::Subtype(Subtype::Knight)),
    )
    .whose(Players::You)
);

// "When this leaves the battlefield".
listener!(
    KEEPSAKE,
    "Test Keepsake",
    Trigger::of(TriggerEvent::Leaves, Subject::This)
);
/// "Whenever a creature dies, you gain 1 life", on a creature, which sees itself die too.
static MOURNER: CardDef = CardDef {
    name: "Test Mourner",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        abilities: &[Ability::triggered(&Trigger::of(
            TriggerEvent::Dies,
            Subject::A(Filter::Types(Types::CREATURE)),
        ))
        .effect(&gain_life(you(), 1))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[
    &LAND, &BEAR, &KNIGHT, &HECKLER, &PURIST, &ARTIFACT, &METRONOME, &ARCHIVE, &MARSHAL, &KEEPSAKE,
    &MOURNER,
]);

const DECK: &Decklist = &[
    (2, "Test Bear"),
    (2, "Test Knight"),
    (1, "Test Heckler"),
    (1, "Test Purist"),
    (1, "Test Artifact"),
    (1, "Test Metronome"),
    (1, "Test Archive"),
    (1, "Test Marshal"),
    (1, "Test Keepsake"),
    (2, "Test Mourner"),
    (28, "Test Land"),
];

/// A game in the starting player's first main phase, each player with a land in hand so
/// that every main phase of theirs is a decision.
fn game() -> (GameTable, State, Seat) {
    let t = GameTable::new(Format::CONSTRUCTED, &CARDS, [DECK, DECK]).unwrap();
    let mut s = State::new(&t, 7, Profile::EXACT).unwrap();
    begin(&t, &mut s);
    for player in Seat::BOTH {
        let land = crate::tests::fixture_slots(s.zone(ZoneKey::Of(player, Zone::Library)))
            .find(|&slot| s.definition_name(&t, slot) == "Test Land")
            .unwrap();
        s.move_object(&mut event::Sink::off(), land, Zone::Hand, Place::Top)
            .unwrap();
    }
    let p = s.turn().starting_player();
    (t, s, p)
}

/// `player` casting `name`, as the event of it.
fn cast(t: &GameTable, s: &State, player: Seat, name: &str) -> Event {
    Event::Cast {
        spell: s.object_ref(super::events::find(s, t, player, name)),
        controller: player,
        targets: Fixed::default(),
        x: 0,
        costs: Castings::default(),
        flashback: false,
        from: Zone::Hand,
    }
}

#[test]
fn a_cast_trigger_is_for_the_players_it_names() {
    let (t, mut s, p) = game();
    let q = p.opponent();
    let mut x = Scratch::new();
    put(&t, &mut s, &mut x, p, "Test Heckler");
    let mine = cast(&t, &s, p, "Test Bear");
    event::settle(&mut s, &t, &mut x.events, mine).unwrap();
    assert!(s.triggers().is_empty(), "not its controller's spell");
    let theirs = cast(&t, &s, q, "Test Bear");
    event::settle(&mut s, &t, &mut x.events, theirs).unwrap();
    assert_eq!(s.triggers().len(), 1);
    assert_eq!(s.triggers()[0].info.player, q, "the player who cast it");
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.player(p).life(), 21);
}

#[test]
fn a_cast_trigger_reads_its_subject_of_the_spell() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    put(&t, &mut s, &mut x, p, "Test Purist");
    let creature = cast(&t, &s, p, "Test Bear");
    event::settle(&mut s, &t, &mut x.events, creature).unwrap();
    assert!(s.triggers().is_empty(), "a creature spell");
    let artifact = cast(&t, &s, p, "Test Artifact");
    event::settle(&mut s, &t, &mut x.events, artifact).unwrap();
    assert_eq!(s.triggers().len(), 1);
    let theirs = cast(&t, &s, p.opponent(), "Test Artifact");
    event::settle(&mut s, &t, &mut x.events, theirs).unwrap();
    assert_eq!(s.triggers().len(), 1, "not an opponent's");
}

/// Both main-phase events match an ordinary trigger for its controller.
#[test]
fn a_main_phase_trigger_matches_its_controllers_main_phase_events() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    put(&t, &mut s, &mut x, p, "Test Metronome");
    for step in [TurnStep::PrecombatMain, TurnStep::PostcombatMain] {
        for active in [p.opponent(), p] {
            let event = Event::StepBegins {
                turn: s.turn().number(),
                step,
                active,
            };
            event::settle(&mut s, &t, &mut x.events, event).unwrap();
            assert_eq!(s.triggers().len(), usize::from(active == p));
            resolve_all(&t, &mut s, &mut x);
        }
    }
    assert_eq!(s.player(p).life(), 22);
}

#[test]
fn a_draw_trigger_fires_once_for_each_card_whoever_draws() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    put(&t, &mut s, &mut x, p, "Test Archive");
    for (player, cards) in [(p, 1), (p.opponent(), 2)] {
        event::settle(&mut s, &t, &mut x.events, Event::Draw { player, cards }).unwrap();
    }
    let drew: Vec<Seat> = s.triggers().iter().map(|tr| tr.info.player).collect();
    assert_eq!(drew, [p, p.opponent(), p.opponent()]);
}

#[test]
fn an_attack_trigger_counts_the_attackers_its_subject_accepts() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    put(&t, &mut s, &mut x, p, "Test Marshal");
    let bear = put(&t, &mut s, &mut x, p, "Test Bear");
    let knights = [
        put(&t, &mut s, &mut x, p, "Test Knight"),
        put(&t, &mut s, &mut x, p, "Test Knight"),
    ];
    let attack = |attackers: Bitboard<_>| Event::Attack {
        player: p,
        attackers,
    };
    event::settle(&mut s, &t, &mut x.events, attack(Bitboard::single(bear))).unwrap();
    assert!(s.triggers().is_empty(), "no Knight attacks");
    let all: Bitboard<_> = knights.into_iter().chain([bear]).collect();
    event::settle(&mut s, &t, &mut x.events, attack(all)).unwrap();
    assert_eq!(s.triggers().len(), 1, "once, however many");
    assert_eq!(s.triggers()[0].info.amount, 2, "the Knights, not the Bear");
}

/// Moves the permanents in `slots` to `to` together, as one event.
fn move_together(t: &GameTable, s: &mut State, x: &mut Scratch, slots: &[Slot], to: Zone) {
    let batch: Bitboard<Slot> = slots.iter().copied().collect();
    event::move_batch(
        s,
        t,
        &mut x.events,
        batch,
        to,
        Reason::Effect,
        Entry::default(),
    )
    .unwrap();
}

#[test]
fn creatures_that_die_together_each_see_the_others_die() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let mourners = [
        put(&t, &mut s, &mut x, p, "Test Mourner"),
        put(&t, &mut s, &mut x, p, "Test Mourner"),
    ];
    let bear = put(&t, &mut s, &mut x, p, "Test Bear");
    let dying = [mourners[0], mourners[1], bear];
    let refs = dying.map(|slot| s.object_ref(slot));
    move_together(&t, &mut s, &mut x, &dying, Zone::Graveyard);
    assert_eq!(s.triggers().len(), 6, "each Mourner sees all three die");
    for r in refs {
        let row = s.last_known(r).expect("last-known information of each");
        assert!(row.derived.types.contains(Types::CREATURE) && !row.token);
        let seen = s.triggers().iter().filter(|tr| tr.info.object == Some(r));
        assert_eq!(seen.count(), 2, "both Mourners saw it");
    }
    let life = s.player(p).life();
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.player(p).life(), life + 6);
    assert!(
        s.last_known_rows().is_empty(),
        "forgotten once nothing names a row"
    );
}

#[test]
fn a_creature_that_is_exiled_does_not_die() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    let mourner = put(&t, &mut s, &mut x, p, "Test Mourner");
    let bear = put(&t, &mut s, &mut x, p, "Test Bear");
    move_together(&t, &mut s, &mut x, &[bear], Zone::Exile);
    assert!(s.triggers().is_empty() && s.last_known_rows().is_empty());
    move_together(&t, &mut s, &mut x, &[mourner], Zone::Hand);
    assert!(s.triggers().is_empty(), "nor does one returned to a hand");
}

#[test]
fn a_leaves_trigger_fires_for_every_zone() {
    let (t, mut s, p) = game();
    let mut x = Scratch::new();
    for to in [Zone::Exile, Zone::Hand] {
        let keepsake = put(&t, &mut s, &mut x, p, "Test Keepsake");
        let left = s.object_ref(keepsake);
        move_together(&t, &mut s, &mut x, &[keepsake], to);
        assert_eq!(s.triggers().len(), 1, "to {to:?}");
        assert_eq!(s.triggers()[0].info.object, Some(left));
        assert!(
            s.last_known(left)
                .is_some_and(|row| row.derived.types == Types::ARTIFACT)
        );
        resolve_all(&t, &mut s, &mut x);
        assert!(s.last_known_rows().is_empty());
        if to == Zone::Exile {
            // Back to the hand, for the next zone.
            s.move_object(&mut event::Sink::off(), keepsake, Zone::Hand, Place::Top)
                .unwrap();
        }
    }
}

/// "Creatures have 'Whenever this creature attacks, you gain 1 life.'"
static WARCRY: CardDef = CardDef {
    name: "Test Warcry",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ENCHANTMENT,
        abilities: &[Ability::static_ability(&LayerSpec {
            affected: Objects::All {
                zone: Zone::Battlefield,
                filter: Filter::Types(Types::CREATURE),
            },
            condition: None,
            modifications: &[Modification::grant(0)],
        })],
        grants: &[Ability::triggered(&Trigger::ATTACKS).effect(&gain_life(you(), 1))],
        ..Face::EMPTY
    }],
    tokens: &[],
};
/// "Creatures lose all abilities."
static HUSH: CardDef = CardDef {
    name: "Test Hush",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ENCHANTMENT,
        abilities: &[Ability::static_ability(&LayerSpec {
            affected: Objects::All {
                zone: Zone::Battlefield,
                filter: Filter::Types(Types::CREATURE),
            },
            condition: None,
            modifications: &[Modification::LoseAllAbilities],
        })],
        ..Face::EMPTY
    }],
    tokens: &[],
};
/// "Creatures have 'When this creature dies, you gain 1 life.'"
static WAKE: CardDef = CardDef {
    name: "Test Wake",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        grants: &[Ability::triggered(&Trigger::DIES).effect(&gain_life(you(), 1))],
        ..WARCRY.faces[0]
    }],
    tokens: &[],
};
static GRANTING: CardTable = CardTable::new(&[&LAND, &BEAR, &WARCRY, &HUSH, &WAKE]);
const GRANTING_DECK: &Decklist = &[
    (2, "Test Bear"),
    (2, "Test Warcry"),
    (1, "Test Hush"),
    (20, "Test Land"),
];

/// A game of [`GRANTING_DECK`] at its first turn, and the starting player.
fn granting_game() -> (GameTable, State, Seat) {
    let t = GameTable::new(Format::CONSTRUCTED, &GRANTING, [GRANTING_DECK; 2]).unwrap();
    let mut s = State::new(&t, 7, Profile::EXACT).unwrap();
    begin(&t, &mut s);
    let p = s.turn().starting_player();
    (t, s, p)
}

/// `bear` attacking alone, as `p` declares it, matched against the triggers: how many trigger.
fn attacks(t: &GameTable, s: &mut State, x: &mut Scratch, p: Seat, bear: Slot) -> usize {
    let attack = Event::Attack {
        player: p,
        attackers: Bitboard::single(bear),
    };
    event::settle(s, t, &mut x.events, attack).unwrap();
    s.triggers().len()
}

/// Each row of the table of grants that grants a creature a triggered ability is one instance
/// of it (CR 113.2c), so two Warcries make the creature's attack trigger twice. The creature is
/// each trigger's source and the granted ability resolves from the granting card's grants.
#[test]
fn a_granted_attack_trigger_fires_once_for_each_row() {
    let (t, mut s, p) = granting_game();
    let mut x = Scratch::new();
    let bear = put(&t, &mut s, &mut x, p, "Test Bear");
    put(&t, &mut s, &mut x, p, "Test Warcry");
    put(&t, &mut s, &mut x, p, "Test Warcry");
    assert_eq!(attacks(&t, &mut s, &mut x, p, bear), 2);
    for tr in s.triggers() {
        assert_eq!(tr.of.source.map(|r| r.slot), Some(bear));
        assert_eq!(tr.of.controller.seat(), p);
    }
    let life = s.player(p).life();
    resolve_all(&t, &mut s, &mut x);
    assert_eq!(s.player(p).life(), life + 2);
}

/// An effect that removes all abilities removes a granted trigger whose grant came before it,
/// and not one whose grant came after it (see CR 613.7).
#[test]
fn losing_all_abilities_removes_a_granted_trigger_only_from_before() {
    for (first, second, triggers) in [
        ("Test Warcry", "Test Hush", 0),
        ("Test Hush", "Test Warcry", 1),
    ] {
        let (t, mut s, p) = granting_game();
        let mut x = Scratch::new();
        let bear = put(&t, &mut s, &mut x, p, "Test Bear");
        put(&t, &mut s, &mut x, p, first);
        put(&t, &mut s, &mut x, p, second);
        assert_eq!(
            attacks(&t, &mut s, &mut x, p, bear),
            triggers,
            "{first}, then {second}"
        );
    }
}

/// A granted "dies" trigger would have to be matched against the permanent as it last
/// existed, which the table of grants does not keep, so no table is built with one.
#[test]
fn a_granted_dies_trigger_is_refused() {
    let deck: &Decklist = &[(1, "Test Wake"), (20, "Test Land")];
    let refused = GameTable::new(Format::CONSTRUCTED, &GRANTING, [deck; 2]).err();
    assert_eq!(
        refused,
        Some(beryl_kernel::table::TableError::Granted { card: "Test Wake" })
    );
}
