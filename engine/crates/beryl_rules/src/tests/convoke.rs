//! Convoke and improvise (CR 702.51a, CR 702.126a): which creatures and artifacts a spell is
//! offered to tap, what each pays, and the exact payability check against the payment
//! reference, which tries every set of them.

use super::LAND;
use super::cast::{Game, cast};
use crate::Choice;
use crate::card::{
    Ability, CardDef, Colors, Face, Keywords, ManaColor, ManaCost, ManaSet, Objects, Subtype,
    SubtypeWords, Subtypes, Supertypes, Types,
};
use crate::decision::Opt;
use crate::ids::Slot;
use crate::replacement::event::Sink;
use crate::rng::SplitMix64;
use crate::state::{CounterKind, Memo, Zone};
use crate::table::Capability;
use crate::{CardTable, DecisionKind, Decklist, Profile};
use beryl_card::effect::build::{add_counters, draw, you};
use beryl_card::phrases::when_enters;
use beryl_kernel::format::Format;
use beryl_kernel::memos::Link;
use beryl_kernel::store::Duration;
use beryl_kernel::zone::Place;

const fn cost(generic: u8, w: u8, g: u8) -> ManaCost {
    ManaCost::new(generic, [w, 0, 0, 0, g, 0])
}

static FOREST: CardDef = CardDef {
    name: "Test Forest",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        supertypes: Supertypes::BASIC,
        types: Types::LAND,
        subtypes: SubtypeWords::new(Subtypes::of(&[Subtype::Forest])),
        abilities: &[Ability::tap_for_mana(ManaSet::one(ManaColor::Green))],
        ..Face::EMPTY
    }],
};
static WHITE: CardDef = CardDef {
    name: "Test White",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        cost: cost(0, 1, 0),
        colors: Colors::WHITE,
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        ..Face::EMPTY
    }],
};
static GREEN: CardDef = CardDef {
    name: "Test Green",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        cost: cost(0, 0, 1),
        colors: Colors::GREEN,
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        ..Face::EMPTY
    }],
};
static GREEN_WHITE: CardDef = CardDef {
    name: "Test Green White",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        cost: cost(0, 1, 1),
        colors: Colors::GREEN.union(Colors::WHITE),
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        ..Face::EMPTY
    }],
};
/// A colorless artifact creature.
static THOPTER: CardDef = CardDef {
    name: "Test Thopter",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        types: Types::ARTIFACT.union(Types::CREATURE),
        power: Some(0),
        toughness: Some(2),
        ..Face::EMPTY
    }],
};
/// A green mana creature: "{T}: Add {G}."
static ELF: CardDef = CardDef {
    name: "Test Llanowar",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        cost: cost(0, 0, 1),
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        abilities: &[Ability::tap_for_mana(ManaSet::one(ManaColor::Green))],
        ..Face::EMPTY
    }],
};
/// A noncreature artifact with no abilities.
static RELIC: CardDef = CardDef {
    name: "Test Relic",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        cost: cost(1, 0, 0),
        types: Types::ARTIFACT,
        ..Face::EMPTY
    }],
};
/// {1}{W}{G}, convoke: draw a card.
static CHORUS: CardDef = CardDef {
    name: "Test Chorus",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        cost: cost(1, 1, 1),
        types: Types::SORCERY,
        keywords: Keywords::CONVOKE,
        abilities: &[Ability::spell().effect(&draw(you(), 1))],
        ..Face::EMPTY
    }],
};
/// {X}{G}{G}, convoke: draw a card.
static GATHER: CardDef = CardDef {
    name: "Test Gather",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        cost: ManaCost {
            x: 1,
            ..cost(0, 0, 2)
        },
        types: Types::SORCERY,
        keywords: Keywords::CONVOKE,
        abilities: &[Ability::spell().effect(&draw(you(), 1))],
        ..Face::EMPTY
    }],
};
/// {2}{W}, improvise: draw a card.
static IMPROV: CardDef = CardDef {
    name: "Test Improv",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        cost: cost(2, 1, 0),
        types: Types::SORCERY,
        keywords: Keywords::IMPROVISE,
        abilities: &[Ability::spell().effect(&draw(you(), 1))],
        ..Face::EMPTY
    }],
};

static CARDS: CardTable = CardTable::new(&[
    &LAND,
    &FOREST,
    &WHITE,
    &GREEN,
    &GREEN_WHITE,
    &THOPTER,
    &ELF,
    &RELIC,
    &CHORUS,
    &GATHER,
    &IMPROV,
]);

const DECK: &Decklist = &[
    (4, "Test Land"),
    (4, "Test Forest"),
    (4, "Test White"),
    (4, "Test Green"),
    (4, "Test Green White"),
    (4, "Test Thopter"),
    (4, "Test Llanowar"),
    (4, "Test Relic"),
    (4, "Test Chorus"),
    (4, "Test Gather"),
    (4, "Test Improv"),
];

fn game() -> Game {
    Game::with(&CARDS, DECK, Profile::EXACT)
}

fn refresh(g: &mut Game) {
    crate::continuous::layers::refresh(&mut g.s, &g.t, &mut Sink::default()).unwrap();
}

/// The options of the open decision, without moving past a silent pass.
fn options(g: &mut Game) -> Vec<Opt> {
    let d = g.current();
    (0..d.len())
        .filter_map(|i| d.candidates.option(Choice(i as u16)))
        .collect()
}

#[test]
fn the_table_takes_every_test_card() {
    assert!(crate::GameTable::new(Format::CONSTRUCTED, &CARDS, [DECK, DECK]).is_ok());
}

/// A creature of two colors, each with a symbol left, is offered once for each, and the one
/// picked pays that color's symbol: white here, so a Forest and a Plains pay {1}{G}.
#[test]
fn a_two_color_creature_is_offered_once_for_each_color() {
    let mut g = game();
    let spell = g.give(g.me, "Test Chorus", Zone::Hand);
    let wolf = g.give(g.me, "Test Green White", Zone::Battlefield);
    g.give(g.me, "Test Forest", Zone::Battlefield);
    g.give(g.me, "Test Land", Zone::Battlefield);
    refresh(&mut g);
    g.pick(cast(spell));
    let offered = options(&mut g);
    let white = Opt::Mana {
        slot: wolf,
        color: ManaColor::White,
    };
    let green = Opt::Mana {
        slot: wolf,
        color: ManaColor::Green,
    };
    assert!(
        offered.contains(&white) && offered.contains(&green),
        "{offered:?}"
    );
    assert!(!offered.contains(&Opt::Object(wolf)), "{offered:?}");
    g.pick(white);
    assert!(g.s.object(wolf).is_tapped());
    assert_eq!(g.s.object(spell).zone(), Zone::Stack);
    assert_eq!(g.current().kind, DecisionKind::Priority);
}

/// Improvise taps artifacts for generic mana alone: a white creature is never offered, and
/// three artifacts with no white mana cannot cast a spell with {W} in its cost.
#[test]
fn improvise_pays_only_generic_mana_and_only_with_artifacts() {
    let mut g = game();
    let spell = g.give(g.me, "Test Improv", Zone::Hand);
    for _ in 0..3 {
        g.give(g.me, "Test Relic", Zone::Battlefield);
    }
    let white = g.give(g.me, "Test White", Zone::Battlefield);
    refresh(&mut g);
    assert!(!g.offers_cast(spell), "artifacts cannot pay {{W}}");
    let plains = g.give(g.me, "Test Land", Zone::Battlefield);
    refresh(&mut g);
    // The relics are interchangeable, so each pick is a single answer, taken as it comes:
    // two of them pay {2}, and the Plains the {W}. The white creature is no artifact.
    g.pick(cast(spell));
    assert_eq!(g.s.object(spell).zone(), Zone::Stack);
    assert!(g.s.object(plains).is_tapped());
    assert!(!g.s.object(white).is_tapped());
    let relics =
        g.s.battlefield()
            .iter()
            .filter(|&&slot| {
                g.s.definition_name(&g.t, slot) == "Test Relic" && g.s.object(slot).is_tapped()
            })
            .count();
    assert_eq!(relics, 2);
}

/// A random board of creatures, artifacts, mana creatures and lands, some tapped and some
/// summoning sick, with a convoke spell and an improvise spell in hand.
fn random_board(rng: &mut SplitMix64) -> (Game, Slot, Slot) {
    const PERMANENTS: [&str; 7] = [
        "Test Land",
        "Test Forest",
        "Test White",
        "Test Green",
        "Test Green White",
        "Test Thopter",
        "Test Llanowar",
    ];
    let mut g = game();
    let chorus = g.give(g.me, "Test Chorus", Zone::Hand);
    let improv = g.give(g.me, "Test Improv", Zone::Hand);
    for _ in 0..rng.below(7) {
        let name = PERMANENTS[rng.below(PERMANENTS.len() as u64) as usize];
        if g.s
            .library(g.me)
            .iter()
            .any(|&slot| g.s.definition_name(&g.t, slot) == name)
        {
            let slot = g.give(g.me, name, Zone::Battlefield);
            let mut board = g.s.board();
            if rng.below(5) == 0 {
                board.set_tapped(slot, true);
            }
            if rng.below(4) == 0 {
                board.set_sick(slot, true);
            }
        }
    }
    refresh(&mut g);
    (g, chorus, improv)
}

/// The check the casting steps make agrees with the reference, which tries every set of the
/// objects a spell could tap, on 200 seeded boards and costs.
#[test]
fn the_exact_check_agrees_with_the_reference() {
    let mut rng = SplitMix64::new(29);
    let (mut payable, mut unpayable) = (0, 0);
    for _ in 0..200 {
        let (g, chorus, improv) = random_board(&mut rng);
        let cost = cost(rng.below(4) as u8, rng.below(3) as u8, rng.below(3) as u8);
        for spell in [chorus, improv] {
            let face = crate::card::FaceIx::FRONT;
            let found = crate::casting::pays_with_objects(&g.s, &g.t, g.me, spell, face, cost)
                .unwrap()
                .expect("a spell with convoke or improvise");
            let expected =
                crate::reference::reference_pays_with_objects(&g.s, &g.t, g.me, spell, face, cost)
                    .expect("a board the reference can say");
            assert_eq!(found, expected, "{cost:?} for {spell}");
            if found {
                payable += 1;
            } else {
                unpayable += 1;
            }
        }
    }
    assert!(
        payable > 50 && unpayable > 50,
        "{payable} payable, {unpayable} not"
    );
}

/// A convoke or improvise spell is offered exactly when it can be paid, and every sequence of
/// answers to its payment ends with it cast: no pick leaves a cost that cannot be finished.
#[test]
fn every_sequence_of_offered_picks_ends_with_the_cost_paid() {
    let mut rng = SplitMix64::new(31);
    let mut cast_count = 0;
    for _ in 0..200 {
        let (mut g, chorus, improv) = random_board(&mut rng);
        let gather = g.give(g.me, "Test Gather", Zone::Hand);
        refresh(&mut g);
        for spell in [chorus, improv] {
            let face = crate::card::FaceIx::FRONT;
            let cost = g.s.card_def(&g.t, spell).expect("a card").face(face).cost;
            let payable =
                crate::reference::reference_pays_with_objects(&g.s, &g.t, g.me, spell, face, cost)
                    .expect("a board the reference can say");
            assert_eq!(g.offers_cast(spell), payable, "{spell}");
        }
        let castable: Vec<Slot> = [chorus, improv, gather]
            .into_iter()
            .filter(|&spell| g.offers_cast(spell))
            .collect();
        let Some(&spell) = castable.get(rng.below(castable.len().max(1) as u64) as usize) else {
            continue;
        };
        g.pick(cast(spell));
        for _ in 0..40 {
            let d = g.current();
            if d.kind == DecisionKind::Priority {
                break;
            }
            g.choose(Choice(rng.below(d.len() as u64) as u16));
        }
        assert_eq!(g.s.object(spell).zone(), Zone::Stack);
        assert_eq!(g.current().kind, DecisionKind::Priority);
        cast_count += 1;
    }
    assert!(cast_count > 50, "only {cast_count} cast");
}

static CONVOKE_READER: CardDef = CardDef {
    name: "Test Loxodon",
    shape: beryl_card::definition::Shape::Single,
    tokens: &[],
    faces: &[Face {
        cost: cost(1, 1, 0),
        colors: Colors::WHITE,
        types: Types::CREATURE,
        power: Some(4),
        toughness: Some(4),
        keywords: Keywords::CONVOKE,
        abilities: &[when_enters(&add_counters(
            Objects::Convoked,
            CounterKind::PlusOne,
            1,
        ))],
        ..Face::EMPTY
    }],
};

static READER_CARDS: CardTable = CardTable::new(&[&LAND, &WHITE, &CONVOKE_READER]);

const READER_DECK: &Decklist = &[(4, "Test Land"), (4, "Test White"), (4, "Test Loxodon")];

/// A game whose cards never read `Objects::Convoked` has no `Capability::ReadsConvokers` and
/// writes no `Memo::Convoked` when a creature convokes a spell.
#[test]
fn without_reads_convokers_convoke_writes_no_memo() {
    let mut g = game();
    assert!(!g.t.can(Capability::ReadsConvokers));
    let spell = g.give(g.me, "Test Chorus", Zone::Hand);
    let white = g.give(g.me, "Test White", Zone::Battlefield);
    g.give(g.me, "Test Forest", Zone::Battlefield);
    g.give(g.me, "Test Land", Zone::Battlefield);
    refresh(&mut g);
    g.pick(cast(spell));
    assert!(g.s.object(white).is_tapped());
    assert_eq!(g.s.memos(white).iter().count(), 0);
}

/// Convoke memos point at the spell on the stack, re-point at the permanent on resolution,
/// allow the same creature to convoke a second spell after untapping, and are removed when a
/// spell leaves the stack without entering the battlefield or at cleanup.
#[test]
fn convoke_memos_repoint_on_resolution_and_clear_on_cleanup_or_leaving_stack() {
    let mut g = Game::with(&READER_CARDS, READER_DECK, Profile::EXACT);
    assert!(g.t.can(Capability::ReadsConvokers));
    let first = g.give(g.me, "Test Loxodon", Zone::Hand);
    let creature = g.give(g.me, "Test White", Zone::Battlefield);
    let land = g.give(g.me, "Test Land", Zone::Battlefield);
    refresh(&mut g);

    // Cast first spell with creature convoking {W} and land paying {1}.
    g.pick(cast(first));
    assert_eq!(g.s.object(first).zone(), Zone::Stack);
    let first_spell_ref = g.s.object_ref(first);
    assert!(
        g.s.memos(creature)
            .iter()
            .any(|m| m == Memo::Convoked(Link::to(first_spell_ref)))
    );

    // Resolve spell -> enters battlefield, memo re-points at permanent; resolve ETB trigger.
    g.pass_both();
    assert_eq!(g.s.object(first).zone(), Zone::Battlefield);
    let first_perm_ref = g.s.object_ref(first);
    assert!(
        g.s.memos(creature)
            .iter()
            .any(|m| m == Memo::Convoked(Link::to(first_perm_ref)))
    );
    g.pass_both();
    assert_eq!(g.s.object(creature).counters().plus_one, 1);

    // Untap creature and land, convoke a second spell in the same turn: no duplicate panic,
    // both memos coexist, and the creature gets a second +1/+1 counter.
    let second = g.give(g.me, "Test Loxodon", Zone::Hand);
    {
        let mut board = g.s.board();
        board.set_tapped(creature, false);
        board.set_tapped(land, false);
        board.set_tapped(first, true);
    }
    refresh(&mut g);
    g.pick(cast(second));
    g.pass_both();
    g.pass_both();
    assert_eq!(g.s.object(creature).counters().plus_one, 2);
    assert_eq!(g.s.check_invariants(&g.t), Ok(()));

    // Cleanup removes all convoke memos.
    g.s.expire(Duration::EndOfTurn);
    assert_eq!(g.s.memos(creature).iter().count(), 0);

    // A spell leaving the stack without becoming a permanent removes its convoke memos.
    let third = g.give(g.me, "Test Loxodon", Zone::Hand);
    {
        let mut board = g.s.board();
        board.set_tapped(creature, false);
        board.set_tapped(land, false);
        board.set_tapped(second, true);
    }
    refresh(&mut g);
    g.pick(cast(third));
    assert_eq!(g.s.memos(creature).iter().count(), 1);
    g.s.take_top_of_stack();
    g.s.move_object(&mut Sink::off(), third, Zone::Graveyard, Place::Top)
        .unwrap();
    assert_eq!(g.s.memos(creature).iter().count(), 0);
    assert_eq!(g.s.check_invariants(&g.t), Ok(()));
}
