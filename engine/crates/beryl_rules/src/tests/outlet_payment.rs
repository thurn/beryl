//! Payments with outlets: mana abilities whose cost sacrifices or taps a permanent their
//! controller chooses. On seeded boards of outlets, artifacts, creatures and lands, whether a
//! cost can be paid agrees with the brute force over every sequence of mana abilities, and every
//! sequence of the options a payment offers ends with the spell cast, under each payment mode.

use super::LAND;
use super::cast::ISLAND;
use crate::bitboard::Bitboard;
use crate::card::{
    Ability, CardDef, Chosen, Cost, CostAction, CostPart, Face, ManaColor, ManaCost, ManaSet,
    Measure, Types,
};
use crate::replacement::event::Sink;
use crate::rng::SplitMix64;
use crate::state::{ManaPool, Zone};
use crate::{CardTable, Choice, DecisionKind, Opt, Payment, Profile, Replay, Scratch, State};
use crate::{GameTable, mana};
use beryl_card::Filter;

/// "Sacrifice an artifact: Add {C}{C}."
static SAC_ARTIFACT: CardDef = CardDef {
    name: "Outlet Ironworks",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[Ability::mana(
            Cost::of_parts(&[CostPart::sacrifice(Filter::Types(Types::ARTIFACT))]),
            ManaSet::of(ManaColor::Colorless, 2),
        )],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "Sacrifice a creature: Add one mana of any color."
static SAC_CREATURE: CardDef = CardDef {
    name: "Outlet Altar",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[Ability::mana(
            Cost::of_parts(&[CostPart::sacrifice(Filter::Types(Types::CREATURE))]),
            ManaSet::any_color(1),
        )],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "{T}, Tap an untapped creature you control: Add one mana of any color."
static TAP_CREATURE: CardDef = CardDef {
    name: "Outlet Drum",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[Ability::mana(
            Cost::of_parts(&[
                CostPart::TAP,
                CostPart::Chosen(
                    CostAction::Tap,
                    Chosen {
                        zone: Zone::Battlefield,
                        filter: Filter::Types(Types::CREATURE),
                        measure: Measure::Count(1),
                        other: false,
                    },
                ),
            ]),
            ManaSet::any_color(1),
        )],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "{T}: Add {C}{C}.", an artifact.
static ROCK: CardDef = CardDef {
    name: "Outlet Rock",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[Ability::tap_for_mana(ManaSet::of(ManaColor::Colorless, 2))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "{T}, Sacrifice this artifact: Add one mana of any color."
static PETAL: CardDef = CardDef {
    name: "Outlet Petal",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[Ability::mana(
            Cost::of_parts(&[CostPart::TAP, CostPart::This(CostAction::Sacrifice)]),
            ManaSet::any_color(1),
        )],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// An artifact with no ability.
static TRINKET: CardDef = CardDef {
    name: "Outlet Trinket",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ARTIFACT,
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// A 0/2 artifact creature.
static THOPTER: CardDef = CardDef {
    name: "Outlet Thopter",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::ARTIFACT.union(Types::CREATURE),
        power: Some(0),
        toughness: Some(2),
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// A creature with "{T}: Add {G}."
static ELF: CardDef = CardDef {
    name: "Outlet Elf",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        abilities: &[Ability::tap_for_mana(ManaSet::one(ManaColor::Green))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// Sorceries of the costs the boards are asked to pay.
static SPELL_3: CardDef = CardDef {
    name: "Outlet Spell 3",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        cost: beryl_card::mana("{3}"),
        types: Types::SORCERY,
        ..Face::EMPTY
    }],
    tokens: &[],
};
static SPELL_5: CardDef = CardDef {
    name: "Outlet Spell 5",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        cost: beryl_card::mana("{5}"),
        types: Types::SORCERY,
        ..Face::EMPTY
    }],
    tokens: &[],
};
static SPELL_2U: CardDef = CardDef {
    name: "Outlet Spell 2U",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        cost: beryl_card::mana("{2}{U}"),
        types: Types::SORCERY,
        ..Face::EMPTY
    }],
    tokens: &[],
};
static SPELL_1GW: CardDef = CardDef {
    name: "Outlet Spell 1GW",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        cost: beryl_card::mana("{1}{G}{W}"),
        types: Types::SORCERY,
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[
    &LAND,
    &ISLAND,
    &SAC_ARTIFACT,
    &SAC_CREATURE,
    &TAP_CREATURE,
    &ROCK,
    &PETAL,
    &TRINKET,
    &THOPTER,
    &ELF,
    &SPELL_3,
    &SPELL_5,
    &SPELL_2U,
    &SPELL_1GW,
]);

const OUTLETS: [&str; 3] = ["Outlet Ironworks", "Outlet Altar", "Outlet Drum"];
const OTHERS: [&str; 7] = [
    "Test Land",
    "Test Island",
    "Outlet Rock",
    "Outlet Petal",
    "Outlet Trinket",
    "Outlet Thopter",
    "Outlet Elf",
];
const SPELLS: [&str; 4] = [
    "Outlet Spell 3",
    "Outlet Spell 5",
    "Outlet Spell 2U",
    "Outlet Spell 1GW",
];

fn table() -> GameTable {
    let deck: &[(u8, &str)] = &[
        (3, "Test Land"),
        (3, "Test Island"),
        (2, "Outlet Ironworks"),
        (2, "Outlet Altar"),
        (2, "Outlet Drum"),
        (3, "Outlet Rock"),
        (3, "Outlet Petal"),
        (3, "Outlet Trinket"),
        (3, "Outlet Thopter"),
        (3, "Outlet Elf"),
        (3, "Outlet Spell 3"),
        (3, "Outlet Spell 5"),
        (3, "Outlet Spell 2U"),
        (3, "Outlet Spell 1GW"),
    ];
    GameTable::new(
        beryl_kernel::format::Format::CONSTRUCTED,
        &CARDS,
        [deck, deck],
    )
    .unwrap()
}

/// Moves a card of `name` from `player`'s library to `zone`, if one is left.
fn place(t: &GameTable, s: &mut State, player: crate::Seat, name: &str, zone: Zone) -> bool {
    let Some(&slot) = (s.library(player).iter()).find(|&&slot| s.definition_name(t, slot) == name)
    else {
        return false;
    };
    s.board().place(slot, zone).unwrap();
    true
}

/// Plays every sequence of answers from the decision now, a spell's cast under way, until the
/// caster has priority again: each ends with the spell cast, gone from its owner's hand (and,
/// with no one to respond, resolved), at most `leaves` of them.
fn every_sequence_casts(
    t: &GameTable,
    s: &State,
    card: crate::ids::Slot,
    ctx: &str,
    leaves: &mut usize,
) {
    let mut s = *s;
    let d = super::decide(t, &mut s);
    if d.kind == DecisionKind::Priority {
        assert_ne!(
            s.object(card).zone(),
            Zone::Hand,
            "the spell is not cast: {ctx}"
        );
        *leaves = leaves.saturating_sub(1);
        return;
    }
    assert!(!d.is_empty(), "a payment question with no answer: {ctx}");
    for i in 0..d.len() {
        if *leaves == 0 {
            return;
        }
        let mut next = s;
        crate::apply(
            &mut next,
            t,
            &mut Scratch::new(),
            &mut Replay,
            Choice(i as u16),
        );
        every_sequence_casts(t, &next, card, ctx, leaves);
    }
}

/// On 200 seeded boards of outlets, artifacts, creatures and lands, `rich::pays` (through
/// `Funds::pays_for` and `mana::payable`) agrees with the brute force over every sequence of
/// mana abilities, a spell is offered exactly when it can be paid, and every sequence of the
/// options its payment offers ends with it cast, under each of the three payment modes.
#[test]
#[allow(clippy::too_many_lines)]
fn outlet_payments_agree_with_the_reference_on_200_seeded_boards() {
    let t = table();
    let mut rng = SplitMix64::new(0x0071_E7ED);
    let mut payable = 0;
    let mut through_outlets = 0;
    for board in 0..200 {
        let mut s = State::new(&t, board + 1, Profile::EXACT).unwrap();
        super::begin(&t, &mut s);
        let p = s.turn().starting_player();
        let mut placed = Vec::new();
        for _ in 0..=rng.below(2) {
            let name = OUTLETS[rng.below(OUTLETS.len() as u64) as usize];
            if place(&t, &mut s, p, name, Zone::Battlefield) {
                placed.push(name);
            }
        }
        for _ in 0..rng.below(6) {
            let name = OTHERS[rng.below(OTHERS.len() as u64) as usize];
            if place(&t, &mut s, p, name, Zone::Battlefield) {
                placed.push(name);
            }
        }
        let spell = SPELLS[rng.below(SPELLS.len() as u64) as usize];
        place(&t, &mut s, p, spell, Zone::Hand);
        let mut pool = ManaPool::default();
        pool[ManaColor::Colorless] = rng.below(2) as u8;
        s.board().set_mana(p, pool);
        crate::continuous::layers::refresh(&mut s, &t, &mut Sink::off()).unwrap();
        let ctx = format!(
            "board {board}: {placed:?}, {spell}, pool {:?}",
            pool.amounts()
        );
        let funds = mana::Funds::of(&s, &t, p).unwrap();
        let none = Bitboard::EMPTY;
        let nothing = mana::Paying::NOTHING;
        for cost in ["{1}", "{3}", "{5}", "{2}{U}", "{1}{G}{W}", "{W}{W}", "{6}"] {
            let cost: ManaCost = beryl_card::mana(cost);
            let expected = crate::reference::reference_outlets(&s, &t, p, cost, nothing)
                .expect("the brute force reads the board");
            assert_eq!(
                funds.pays_for(&s, &t, cost, none, nothing),
                expected,
                "Funds::pays_for disagrees for {cost:?}: {ctx}"
            );
            assert_eq!(
                mana::payable(&s, &t, p, cost, none, nothing).unwrap(),
                expected,
                "mana::payable disagrees for {cost:?}: {ctx}"
            );
            let plain = !mana::plans(&s, &t, &mut Sink::off(), p, cost, none, false, nothing)
                .unwrap()
                .is_empty();
            through_outlets += usize::from(expected && !plain);
        }
        let hand = s.hand_by_definition(p);
        let card = *hand
            .iter()
            .find(|&&slot| s.definition_name(&t, slot) == spell)
            .unwrap();
        let cost = s.card_def(&t, card).unwrap().front().cost;
        let expected = crate::reference::reference_outlets(&s, &t, p, cost, nothing).unwrap();
        for payment in [Payment::KeepMostPayable, Payment::Greedy, Payment::Manual] {
            let mut s = s;
            s.set_profile(Profile {
                payment,
                ..Profile::EXACT
            });
            let d = super::decide(&t, &mut s);
            let offer = (0..d.len()).find(|&i| {
                matches!(d.candidates.option(Choice(i as u16)),
                    Some(Opt::Cast { slot, .. }) if slot == card)
            });
            assert_eq!(
                offer.is_some(),
                expected,
                "the spell is offered as it is payable under {payment:?}: {ctx}"
            );
            let Some(i) = offer else { continue };
            payable += 1;
            crate::apply(
                &mut s,
                &t,
                &mut Scratch::new(),
                &mut Replay,
                Choice(i as u16),
            );
            let mut leaves = 200;
            every_sequence_casts(&t, &s, card, &format!("{payment:?}, {ctx}"), &mut leaves);
        }
    }
    assert!(
        payable > 60,
        "too few boards could cast their spell: {payable}"
    );
    assert!(
        through_outlets > 60,
        "too few costs needed an outlet: {through_outlets}"
    );
}
